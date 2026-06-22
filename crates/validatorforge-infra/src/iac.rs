//! Default Infrastructure-as-Code renderer. Produces Terraform and Ansible
//! snippets from the domain model so an operator can review (or apply) the exact
//! plan a deployment will run. Pure and deterministic — no I/O.

use validatorforge_core::IacRenderer;
use validatorforge_types::{DeploymentKind, OpsActionKind, ValidatorNode};

/// Renders opinionated Terraform/Ansible for a bare-metal validator host.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultIacRenderer;

impl DefaultIacRenderer {
    /// Construct the renderer.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl IacRenderer for DefaultIacRenderer {
    fn render_terraform(&self, node: &ValidatorNode, kind: &DeploymentKind) -> String {
        format!(
            "# Terraform plan for {id} ({cluster}) — {kind}\n\
             resource \"metal_device\" \"{id}\" {{\n  \
               hostname         = \"{host}\"\n  \
               plan             = \"m3.large.x86\"\n  \
               metro            = \"fr\"\n  \
               operating_system = \"ubuntu_22_04\"\n  \
               billing_cycle    = \"hourly\"\n  \
               tags             = [\"solana\", \"{cluster}\", \"{role}\"]\n}}\n",
            id = node.id(),
            cluster = node.cluster(),
            role = node.role(),
            host = node.host(),
            kind = kind.as_str(),
        )
    }

    fn render_ansible(&self, node: &ValidatorNode, action: OpsActionKind) -> String {
        let task = match action {
            OpsActionKind::ApplyInfra => "base bootstrap (users, firewall, fail2ban)",
            OpsActionKind::TuneHost => {
                "performance tuning (sysctl, hugepages, CPU governor=performance, NIC rings)"
            }
            OpsActionKind::StartValidator => "install + start agave-validator systemd unit",
            OpsActionKind::AwaitCatchup => "poll catchup via agave-validator monitor",
            OpsActionKind::Drain => "set identity to junk keypair, wait for restart window",
            OpsActionKind::SwapIdentity => "copy staked identity keypair and hot-swap",
            OpsActionKind::DestroyInfra => "stop services, wipe ledger, deprovision",
        };
        format!(
            "- name: {task}\n  \
             hosts: {host}\n  \
             become: true\n  \
             vars:\n    \
               validator_version: \"{version}\"\n    \
               cluster: \"{cluster}\"\n",
            task = task,
            host = node.host(),
            version = node.version(),
            cluster = node.cluster(),
        )
    }
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

    #[test]
    fn terraform_mentions_host_and_cluster() {
        let tf = DefaultIacRenderer::new().render_terraform(&node(), &DeploymentKind::Provision);
        assert!(tf.contains("val01.internal"));
        assert!(tf.contains("mainnet"));
        assert!(tf.contains("metal_device"));
    }

    #[test]
    fn ansible_varies_by_action() {
        let r = DefaultIacRenderer::new();
        let tune = r.render_ansible(&node(), OpsActionKind::TuneHost);
        assert!(tune.contains("hugepages"));
        let start = r.render_ansible(&node(), OpsActionKind::StartValidator);
        assert!(start.contains("agave-validator"));
    }
}
