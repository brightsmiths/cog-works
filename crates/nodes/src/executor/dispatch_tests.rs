//! Direct unit tests for `dispatch.rs`'s private helpers that are not
//! reachable through the public `run_step_with_runner` orchestration path.
//!
//! ## Module placement rationale
//!
//! `executor_tests.rs` (included as a child module of `executor::mod`) only
//! exercises `dispatch.rs` indirectly, via `run_step_with_runner` ->
//! `determine_next_actions`. `determine_next_actions`/`classify_eligible_nodes`
//! (in the `pipeline` crate) never includes a `HumanGated`-and-unapproved node
//! in an `ExecuteNode`/`ExecuteParallel` action — so `check_human_gate`'s
//! defensive re-check inside `execute_single`/`execute_parallel` can never be
//! reached from that path (see its doc comment: "this should never fire ...
//! Kept per the documented step-6 contract"). The only way to exercise this
//! defensive branch directly is a unit test that is itself a descendant of
//! `dispatch` (so it can see the private `check_human_gate` function),
//! matching the existing `#[path = "..."] mod tests;` convention used by
//! `gateway.rs` and `architecture.rs`.
//!
//! Found by the #59 cycle-2 mutation audit: `cargo mutants` reported
//! `replace check_human_gate -> Option<StepResult> with None` as MISSED —
//! no test anywhere observed `check_human_gate`'s `Some` branch.

use std::collections::HashMap;

use pipeline::{
    GateConfig, GateStatus, NodeDefinition, NodeGate, NodeId, NodeType, PipelineGraph,
    PipelineSettings, PipelineToolProfileConfig, ProfileName, ValidationKind,
};

use super::{StepResult, check_human_gate};

fn node_id(id: &str) -> NodeId {
    NodeId::new(id).expect("non-empty node id")
}

fn node_def(id: &str, gate: NodeGate) -> NodeDefinition {
    NodeDefinition {
        id: node_id(id),
        node_type: NodeType::Deterministic,
        declared_inputs: vec![],
        declared_outputs: vec![],
        timeout: None,
        cost_budget: None,
        gate,
        validation_kind: ValidationKind::None,
        abort_siblings_on_failure: false,
    }
}

fn graph_with(nodes: Vec<NodeDefinition>) -> PipelineGraph {
    PipelineGraph {
        nodes,
        edges: vec![],
        evaluation_modes: HashMap::new(),
        explicit_edge_lists: HashMap::new(),
        settings: PipelineSettings {
            default_timeout: None,
            default_cost_budget: None,
            max_node_retries: 0,
        },
        tool_profiles: PipelineToolProfileConfig {
            default_profile: ProfileName::new("default").expect("non-empty profile name"),
            node_overrides: HashMap::new(),
        },
    }
}

/// A `HumanGated` node with no recorded gate decision must produce
/// `Some(StepResult::Gated { .. })` — the defensive re-check's reason for
/// existing at all. Kills the `None`-replacement mutant on `check_human_gate`.
#[test]
fn test_check_human_gate_awaiting_approval_returns_gated() {
    let id = node_id("review");
    let graph = graph_with(vec![node_def("review", NodeGate::HumanGated)]);
    let gate_config = GateConfig {
        gated_nodes: HashMap::new(),
    };

    let result = check_human_gate(&id, &graph, &gate_config);

    match result {
        Some(StepResult::Gated {
            node_id: returned_id,
            gate_reason,
        }) => {
            assert_eq!(returned_id, id, "returned node id must be the checked node");
            assert!(
                gate_reason.contains("review"),
                "gate reason must reference the waiting node, got {gate_reason:?}"
            );
        }
        other => panic!("expected Some(StepResult::Gated {{ .. }}), got {other:?}"),
    }
}

/// A `HumanGated` node that has already been approved must return `None` —
/// proving the function distinguishes approval state, not merely "is this
/// node gated at all".
#[test]
fn test_check_human_gate_approved_returns_none() {
    let id = node_id("review");
    let graph = graph_with(vec![node_def("review", NodeGate::HumanGated)]);
    let mut gated_nodes = HashMap::new();
    gated_nodes.insert(
        id.clone(),
        GateStatus::Approved {
            approved_by: "reviewer".to_string(),
        },
    );
    let gate_config = GateConfig { gated_nodes };

    let result = check_human_gate(&id, &graph, &gate_config);

    assert!(
        result.is_none(),
        "an approved HumanGated node must not be reported as gated, got {result:?}"
    );
}

/// An `AutoProceed` node must never be reported as gated regardless of the
/// `GateConfig` contents.
#[test]
fn test_check_human_gate_auto_proceed_node_returns_none() {
    let id = node_id("build");
    let graph = graph_with(vec![node_def("build", NodeGate::AutoProceed)]);
    let gate_config = GateConfig {
        gated_nodes: HashMap::new(),
    };

    let result = check_human_gate(&id, &graph, &gate_config);

    assert!(
        result.is_none(),
        "an AutoProceed node must never be reported as gated, got {result:?}"
    );
}
