//! # validatorforge-node
//!
//! Composition root for ValidatorForge. Wires the simulated infra adapters into
//! the ops engine, exposes the resilient GraphQL API (queries, mutations, and a
//! live `opsEvents` subscription) over axum with a Prometheus `/metrics`
//! endpoint, and provides a CLI with two subcommands: `serve` (run the API) and
//! `plan` (render the Terraform/Ansible for a deployment kind).

#![forbid(unsafe_code)]

pub mod config;
pub mod plan;
pub mod startup;
pub mod telemetry;

pub use config::{Cli, Command};

/// Dispatch a parsed CLI invocation.
///
/// # Errors
/// Propagates failures from the chosen subcommand.
pub async fn run(cli: Cli) -> anyhow::Result<()> {
    telemetry::init_tracing(cli.log_json);
    match cli.command {
        Command::Serve(args) => startup::run_server(args).await,
        Command::Plan(args) => plan::run_plan(args).await,
    }
}
