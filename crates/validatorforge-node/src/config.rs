//! Command-line interface and argument types for the ValidatorForge node.

use clap::{Args, Parser, Subcommand};

/// Solana validator-fleet operations control plane.
#[derive(Debug, Parser)]
#[command(name = "validatorforge-node", version, about, long_about = None)]
pub struct Cli {
    /// Emit logs as JSON instead of human-readable text.
    #[arg(
        long,
        global = true,
        env = "VALIDATORFORGE_LOG_JSON",
        default_value_t = false
    )]
    pub log_json: bool,

    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level node commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Serve the GraphQL operations API over HTTP (queries, mutations, live subscriptions).
    Serve(ServeArgs),
    /// Render the Terraform + Ansible plan for a deployment kind and print it (no I/O).
    Plan(PlanArgs),
}

/// Arguments for the `serve` command.
#[derive(Debug, Clone, Args)]
pub struct ServeArgs {
    /// Address to bind.
    #[arg(long, env = "VALIDATORFORGE_HOST", default_value = "127.0.0.1")]
    pub host: String,

    /// Port to bind.
    #[arg(long, env = "VALIDATORFORGE_PORT", default_value_t = 8080)]
    pub port: u16,

    /// Broadcast channel capacity for the live `opsEvents` subscription.
    #[arg(long, env = "VALIDATORFORGE_EVENT_CAPACITY", default_value_t = 1024)]
    pub event_capacity: usize,
}

impl Default for ServeArgs {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            event_capacity: 1024,
        }
    }
}

/// The deployment kind to render a plan for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PlanKind {
    /// Stand up a brand-new validator host.
    Provision,
    /// Zero-downtime version upgrade.
    Upgrade,
    /// Hot-spare failover.
    Failover,
    /// Tear a node down.
    Decommission,
}

/// Arguments for the `plan` command.
#[derive(Debug, Clone, Args)]
pub struct PlanArgs {
    /// The deployment kind to render.
    #[arg(long, value_enum, default_value_t = PlanKind::Provision)]
    pub kind: PlanKind,

    /// Node id to render the plan for.
    #[arg(long, default_value = "val-01")]
    pub id: String,

    /// Logical cluster name.
    #[arg(long, default_value = "eu-fiber")]
    pub cluster_name: String,

    /// Host address for the node.
    #[arg(long, default_value = "val01.internal")]
    pub host: String,

    /// Validator version (current, or the upgrade target).
    #[arg(long, default_value = "2.0.14")]
    pub version: String,

    /// Hot-spare host id (only used by `failover`).
    #[arg(long, default_value = "spare-01")]
    pub spare: String,
}

impl Default for PlanArgs {
    fn default() -> Self {
        Self {
            kind: PlanKind::Provision,
            id: "val-01".to_string(),
            cluster_name: "eu-fiber".to_string(),
            host: "val01.internal".to_string(),
            version: "2.0.14".to_string(),
            spare: "spare-01".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn defaults_are_populated() {
        let serve = ServeArgs::default();
        assert_eq!(serve.port, 8080);
        assert_eq!(serve.event_capacity, 1024);
        let plan = PlanArgs::default();
        assert_eq!(plan.kind, PlanKind::Provision);
        assert_eq!(plan.id, "val-01");
    }

    #[test]
    fn parses_serve_with_flags() {
        let cli = Cli::try_parse_from(["validatorforge-node", "serve", "--port", "9090"]).unwrap();
        match cli.command {
            Command::Serve(a) => assert_eq!(a.port, 9090),
            _ => panic!("expected serve"),
        }
    }

    #[test]
    fn parses_plan_with_flags() {
        let cli = Cli::try_parse_from([
            "validatorforge-node",
            "plan",
            "--kind",
            "upgrade",
            "--version",
            "2.1.0",
        ])
        .unwrap();
        match cli.command {
            Command::Plan(a) => {
                assert_eq!(a.kind, PlanKind::Upgrade);
                assert_eq!(a.version, "2.1.0");
            }
            _ => panic!("expected plan"),
        }
    }
}
