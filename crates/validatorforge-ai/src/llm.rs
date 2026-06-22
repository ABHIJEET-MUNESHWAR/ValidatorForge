//! Optional LLM-backed advisor (feature `llm`).
//!
//! Wraps an OpenAI-compatible chat-completions endpoint to add natural-language
//! reasoning on top of the deterministic heuristic. Every call is guarded by a
//! per-attempt timeout and bounded retries ([`validatorforge_resilience`]); on
//! **any** failure — empty key, transport error, timeout, or malformed response
//! — it degrades gracefully to the [`HeuristicAdvisor`]. The network is never on
//! the critical path for a recommendation.

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use validatorforge_core::OpsAdvisor;
use validatorforge_resilience::{with_timeout, RetryPolicy};
use validatorforge_types::ValidatorNode;

use crate::advisor::HeuristicAdvisor;

const SYSTEM_PROMPT: &str = "You are a Solana validator SRE. Given a node's state \
and an operator's context note, recommend the single safest next operational \
action and briefly justify it. Be concise.";

/// Connection + behaviour settings for [`LlmAdvisor`].
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// Chat-completions endpoint URL.
    pub endpoint: String,
    /// Model identifier.
    pub model: String,
    /// Bearer API key. Empty disables the network path (always falls back).
    pub api_key: String,
    /// Per-attempt deadline.
    pub timeout: Duration,
    /// Total attempts (including the first).
    pub max_retries: u32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://api.openai.com/v1/chat/completions".to_string(),
            model: "gpt-4o-mini".to_string(),
            api_key: String::new(),
            timeout: Duration::from_secs(4),
            max_retries: 2,
        }
    }
}

#[derive(Debug)]
enum LlmError {
    Disabled,
    Timeout,
    Provider(String),
}

impl LlmError {
    const fn is_retryable(&self) -> bool {
        matches!(self, LlmError::Timeout | LlmError::Provider(_))
    }
}

/// LLM advisor that falls back to the heuristic on any failure.
#[derive(Debug, Clone)]
pub struct LlmAdvisor {
    http: reqwest::Client,
    config: LlmConfig,
    fallback: HeuristicAdvisor,
}

impl LlmAdvisor {
    /// Build the advisor and its HTTP client.
    ///
    /// # Errors
    /// Returns an error string if the HTTP client cannot be constructed.
    pub fn new(config: LlmConfig) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            http,
            config,
            fallback: HeuristicAdvisor::new(),
        })
    }

    async fn call_model(&self, node: &ValidatorNode, context: &str) -> Result<String, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::Disabled);
        }
        let hint = self.fallback.recommend(node, context);
        let user = format!(
            "Node {id} (role={role}, state={state}, version={version}). Context: \"{context}\". \
             A deterministic heuristic suggests: {action} ({urgency:?}) — {rationale}. \
             Confirm or refine.",
            id = node.id(),
            role = node.role(),
            state = node.state(),
            version = node.version(),
            context = context,
            action = hint.action.as_str(),
            urgency = hint.urgency,
            rationale = hint.rationale,
        );

        let policy = RetryPolicy::new(
            self.config.max_retries.max(1),
            Duration::from_millis(100),
            Duration::from_secs(2),
        );
        policy
            .retry(
                || async {
                    match with_timeout(self.config.timeout, self.request_once(&user)).await {
                        Ok(inner) => inner,
                        Err(_elapsed) => Err(LlmError::Timeout),
                    }
                },
                LlmError::is_retryable,
            )
            .await
    }

    async fn request_once(&self, user: &str) -> Result<String, LlmError> {
        let req = ChatRequest {
            model: self.config.model.clone(),
            temperature: 0.2,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: SYSTEM_PROMPT.to_string(),
                },
                ChatMessage {
                    role: "user",
                    content: user.to_string(),
                },
            ],
        };
        let resp = self
            .http
            .post(&self.config.endpoint)
            .bearer_auth(&self.config.api_key)
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::Provider(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(LlmError::Provider(format!("status {}", resp.status())));
        }
        let body: ChatResponse = resp
            .json()
            .await
            .map_err(|e| LlmError::Provider(e.to_string()))?;
        body.choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| LlmError::Provider("empty completion".into()))
    }
}

#[async_trait]
impl OpsAdvisor for LlmAdvisor {
    async fn advise(&self, node: &ValidatorNode, context: &str) -> String {
        match self.call_model(node, context).await {
            Ok(text) => text,
            Err(err) => {
                let reason = match &err {
                    LlmError::Disabled => "disabled (no api key)".to_string(),
                    LlmError::Timeout => "timed out".to_string(),
                    LlmError::Provider(msg) => format!("provider error: {msg}"),
                };
                tracing::debug!(reason, "llm advisor falling back to heuristic");
                self.fallback.advise(node, context).await
            }
        }
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    temperature: f32,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use validatorforge_types::{
        Cluster, ClusterName, HostAddr, NodeId, NodeRole, ValidatorVersion,
    };

    fn node() -> ValidatorNode {
        ValidatorNode::new(
            NodeId::new("eu-val-01").unwrap(),
            ClusterName::new("eu-fiber").unwrap(),
            Cluster::Mainnet,
            HostAddr::new("val01.internal").unwrap(),
            NodeRole::Voting,
            ValidatorVersion::new("2.0.14").unwrap(),
            Utc::now(),
        )
    }

    #[tokio::test]
    async fn empty_key_falls_back_to_heuristic() {
        let advisor = LlmAdvisor::new(LlmConfig::default()).unwrap();
        // No API key configured -> deterministic heuristic JSON.
        let out = advisor.advise(&node(), "all good").await;
        assert!(out.contains("\"action\""));
    }
}
