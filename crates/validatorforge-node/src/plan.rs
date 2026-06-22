//! One-shot `plan` command: render the Terraform + Ansible a deployment would
//! run, without touching any infrastructure. Pure, deterministic, reviewable.

use anyhow::{Context, Result};

use validatorforge_core::{build_plan, IacRenderer};
use validatorforge_infra::DefaultIacRenderer;
use validatorforge_types::{
    Cluster, ClusterName, DeploymentKind, HostAddr, NodeId, NodeRole, ValidatorNode,
    ValidatorVersion,
};

use crate::config::{PlanArgs, PlanKind};

/// Render and print the plan for the requested deployment kind.
///
/// # Errors
/// Fails if any of the supplied identifiers violate their domain invariants.
pub async fn run_plan(args: PlanArgs) -> Result<()> {
    let node = ValidatorNode::new(
        NodeId::new(args.id).context("invalid node id")?,
        ClusterName::new(args.cluster_name).context("invalid cluster name")?,
        Cluster::Testnet,
        HostAddr::new(args.host).context("invalid host address")?,
        NodeRole::Voting,
        ValidatorVersion::new(args.version.clone()).context("invalid version")?,
        chrono::Utc::now(),
    );

    let kind = match args.kind {
        PlanKind::Provision => DeploymentKind::Provision,
        PlanKind::Upgrade => DeploymentKind::Upgrade {
            target_version: ValidatorVersion::new(args.version)
                .context("invalid target version")?,
        },
        PlanKind::Failover => DeploymentKind::Failover {
            spare: NodeId::new(args.spare).context("invalid spare id")?,
        },
        PlanKind::Decommission => DeploymentKind::Decommission,
    };

    let renderer = DefaultIacRenderer::new();

    println!("# === Terraform ===");
    println!("{}", renderer.render_terraform(&node, &kind));

    println!("# === Ansible (saga steps) ===");
    for (i, step) in build_plan(&kind).iter().enumerate() {
        let action = step.action();
        println!("# step {} — {}", i + 1, action);
        println!("{}", renderer.render_ansible(&node, action));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn renders_provision_plan() {
        let args = PlanArgs {
            kind: PlanKind::Provision,
            ..PlanArgs::default()
        };
        run_plan(args).await.unwrap();
    }

    #[tokio::test]
    async fn renders_each_kind() {
        for kind in [
            PlanKind::Provision,
            PlanKind::Upgrade,
            PlanKind::Failover,
            PlanKind::Decommission,
        ] {
            let args = PlanArgs {
                kind,
                ..PlanArgs::default()
            };
            run_plan(args).await.unwrap();
        }
    }

    #[tokio::test]
    async fn rejects_invalid_id() {
        let args = PlanArgs {
            id: String::new(),
            ..PlanArgs::default()
        };
        assert!(run_plan(args).await.is_err());
    }
}
