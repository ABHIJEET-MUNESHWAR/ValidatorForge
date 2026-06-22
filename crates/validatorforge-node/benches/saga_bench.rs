//! Criterion benchmark for the saga planner and the IaC renderer — the two
//! pure, allocation-bound hot paths on the deployment admission path.
//!
//! A flame graph of the hot path can be generated with the bundled `pprof`
//! sampling profiler (no `perf`/root required):
//!
//! ```text
//! cargo bench --bench saga_bench -- --profile-time 10 'plan/provision'
//! # -> target/criterion/saga/plan/provision/profile/flamegraph.svg
//! ```

use chrono::Utc;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use pprof::criterion::{Output, PProfProfiler};

use validatorforge_core::{build_plan, IacRenderer};
use validatorforge_infra::DefaultIacRenderer;
use validatorforge_types::{
    Cluster, ClusterName, DeploymentKind, HostAddr, NodeId, NodeRole, ValidatorNode,
    ValidatorVersion,
};

fn sample_node() -> ValidatorNode {
    ValidatorNode::new(
        NodeId::new("val-01").unwrap(),
        ClusterName::new("eu-fiber").unwrap(),
        Cluster::Testnet,
        HostAddr::new("val01.internal").unwrap(),
        NodeRole::Voting,
        ValidatorVersion::new("2.0.14").unwrap(),
        Utc::now(),
    )
}

fn kinds() -> Vec<(&'static str, DeploymentKind)> {
    vec![
        ("provision", DeploymentKind::Provision),
        (
            "upgrade",
            DeploymentKind::Upgrade {
                target_version: ValidatorVersion::new("2.1.0").unwrap(),
            },
        ),
        (
            "failover",
            DeploymentKind::Failover {
                spare: NodeId::new("spare-01").unwrap(),
            },
        ),
        ("decommission", DeploymentKind::Decommission),
    ]
}

fn bench_plan(c: &mut Criterion) {
    let mut group = c.benchmark_group("plan");
    for (name, kind) in kinds() {
        group.bench_with_input(BenchmarkId::new("build", name), &kind, |b, k| {
            b.iter(|| build_plan(k));
        });
    }
    group.finish();
}

fn bench_render(c: &mut Criterion) {
    let node = sample_node();
    let renderer = DefaultIacRenderer::new();
    let mut group = c.benchmark_group("render");
    for (name, kind) in kinds() {
        group.bench_with_input(BenchmarkId::new("terraform", name), &kind, |b, k| {
            b.iter(|| renderer.render_terraform(&node, k));
        });
        group.bench_with_input(BenchmarkId::new("ansible_plan", name), &kind, |b, k| {
            b.iter(|| {
                for step in build_plan(k) {
                    let _ = renderer.render_ansible(&node, step.action());
                }
            });
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().with_profiler(PProfProfiler::new(1_000, Output::Flamegraph(None)));
    targets = bench_plan, bench_render
}
criterion_main!(benches);
