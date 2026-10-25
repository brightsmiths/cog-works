//! Steps 5-6 of `run_step_with_runner`: dispatch the first `NextAction` and
//! execute eligible nodes (serial or parallel).
//!
//! Split out of `executor/mod.rs` to keep that file within the project's
//! file-length standard; see `executor/mod.rs`'s module doc for the full
//! 8-step sequence this module contributes steps 5-6 to.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use pipeline::{
    BudgetAcquisition, GateConfig, GateStatus, MergedGuidance, NextAction, NodeGate, NodeId,
    PipelineGraph, PipelineState, SatisfactionScore, compute_eligible_nodes,
};

use crate::node::{BudgetTracker, NodeExecutionResult, NodeInput};
use pipeline::LoadedContextPacks;

use super::completion::apply_completions_and_persist;
use super::runner::NodeRunner;
use super::{StepContext, StepResult, sentinel_node_id};

/// Finds the first `eligible` node that is `HumanGated` and not yet approved,
/// returning its id and a human-readable reason.
fn find_gated_node(
    eligible: &[NodeId],
    graph: &PipelineGraph,
    gate_config: &GateConfig,
) -> Option<(NodeId, String)> {
    eligible.iter().find_map(|id| {
        let def = graph.nodes.iter().find(|n| &n.id == id)?;
        if def.gate != NodeGate::HumanGated {
            return None;
        }
        match gate_config.gated_nodes.get(id) {
            Some(GateStatus::Approved { .. }) => None,
            _ => Some((
                id.clone(),
                format!("Node '{id}' requires human approval before the pipeline can continue"),
            )),
        }
    })
}

/// Checks whether `id` is a `HumanGated` node awaiting approval. Returns
/// `Some(StepResult::Gated)` if so.
///
/// This is a defensive re-check: `determine_next_actions` already filters
/// gated-but-unapproved nodes out of its execute set, so in practice this
/// should never fire for an `ExecuteNode`/`ExecuteParallel` action. Kept per
/// the documented step-6 contract.
fn check_human_gate(
    id: &NodeId,
    graph: &PipelineGraph,
    gate_config: &GateConfig,
) -> Option<StepResult> {
    find_gated_node(std::slice::from_ref(id), graph, gate_config).map(|(node_id, gate_reason)| {
        StepResult::Gated {
            node_id,
            gate_reason,
        }
    })
}

/// Builds the `StepResult::Gated` outcome for a `NextAction::Wait`.
///
/// `NextAction::Wait` does not itself carry which node is waiting, so the
/// currently eligible nodes are re-derived via `compute_eligible_nodes` to
/// find the (first) human-gated one. If none is found (e.g. the wait is due
/// to another node still `Active`, not a gate), a generic sentinel reason is
/// used.
pub(super) fn build_wait_result(
    state: &PipelineState,
    graph: &PipelineGraph,
    gate_config: &GateConfig,
) -> StepResult {
    let eligible = compute_eligible_nodes(state, graph);
    let (node_id, gate_reason) =
        find_gated_node(&eligible, graph, gate_config).unwrap_or_else(|| {
            (
                sentinel_node_id("pipeline-wait"),
                "Pipeline is waiting on an active node or an external event".to_string(),
            )
        });
    StepResult::Gated {
        node_id,
        gate_reason,
    }
}

/// Checks whether dispatching `ids` would already exceed the applicable cost
/// budget, using the already-accumulated `state.cost_accumulator`.
///
/// Uses [`BudgetTracker`] (seeded fresh per call) to satisfy the documented
/// atomicity contract. This is a pre-flight "already over budget" guard
/// rather than true per-node cost estimation — no cost-estimation API exists
/// at this layer yet (a node's real cost is only known after it completes,
/// via `NodeOutput::cost_delta`). Returns `None` (no check performed) when
/// neither the nodes nor the pipeline declare a cost budget.
fn check_budget(
    state: &PipelineState,
    graph: &PipelineGraph,
    ids: &[&NodeId],
) -> Option<pipeline::PipelineError> {
    let limit = ids
        .iter()
        .find_map(|id| {
            graph
                .nodes
                .iter()
                .find(|n| &n.id == *id)
                .and_then(|n| n.cost_budget)
        })
        .or(graph.settings.default_cost_budget)?;
    let tracker = BudgetTracker::new(limit);
    match tracker.try_acquire(&state.cost_accumulator) {
        BudgetAcquisition::Approved { .. } => None,
        BudgetAcquisition::Denied(_) => Some(pipeline::PipelineError::BudgetExceeded {
            accumulated: state.cost_accumulator,
            limit,
        }),
    }
}

pub(super) async fn dispatch_action(
    ctx: &StepContext<'_>,
    runner: &dyn NodeRunner,
    state: &mut PipelineState,
    gate_config: GateConfig,
    action: Option<NextAction>,
) -> StepResult {
    match action {
        None => super::completion::finish_run_complete(ctx, state, &gate_config).await,
        Some(NextAction::Wait) => build_wait_result(state, &ctx.graph, &gate_config),
        Some(NextAction::Escalate(reason)) => {
            super::completion::persist_terminal_state(ctx, state, &gate_config).await;
            StepResult::Escalated(reason)
        }
        Some(NextAction::HaltWithError(err)) => {
            super::completion::persist_terminal_state(ctx, state, &gate_config).await;
            StepResult::Halted(err)
        }
        Some(NextAction::ExecuteNode(id)) => {
            execute_single(ctx, runner, state, &gate_config, id).await
        }
        Some(NextAction::ExecuteParallel(ids)) => {
            execute_parallel(ctx, runner, state, &gate_config, ids).await
        }
    }
}

/// Builds an empty [`LoadedContextPacks`] placeholder.
///
/// `run_step_with_runner`'s orchestration does not itself select or merge
/// context packs — that responsibility belongs to the node bodies
/// (`IntakeNode::execute`, etc., still `todo!()` under #58-#62). Until those
/// land, every `NodeInput` carries an empty set.
fn empty_loaded_context_packs() -> LoadedContextPacks {
    LoadedContextPacks {
        matched_packs: vec![],
        merged_guidance: MergedGuidance::default(),
        strictest_threshold: SatisfactionScore::new(0.0)
            .unwrap_or_else(|| unreachable!("0.0 is always a valid SatisfactionScore")),
    }
}

fn build_node_input(state: &PipelineState) -> NodeInput {
    NodeInput {
        artifacts: HashMap::new(),
        pipeline_state: state.clone(),
        loaded_context_packs: empty_loaded_context_packs(),
    }
}

async fn execute_single(
    ctx: &StepContext<'_>,
    runner: &dyn NodeRunner,
    state: &mut PipelineState,
    gate_config: &GateConfig,
    id: NodeId,
) -> StepResult {
    if let Some(gated) = check_human_gate(&id, &ctx.graph, gate_config) {
        return gated;
    }
    if let Some(err) = check_budget(state, &ctx.graph, &[&id]) {
        return StepResult::Halted(err);
    }
    let input = build_node_input(state);
    match runner.run_node(&id, input).await {
        NodeExecutionResult::Success(output) => {
            apply_completions_and_persist(ctx, state, gate_config, vec![(id, output)]).await
        }
        NodeExecutionResult::Failure(failure) => StepResult::Halted(failure.error),
    }
}

async fn execute_parallel(
    ctx: &StepContext<'_>,
    runner: &dyn NodeRunner,
    state: &mut PipelineState,
    gate_config: &GateConfig,
    ids: Vec<NodeId>,
) -> StepResult {
    if let Some((node_id, gate_reason)) = find_gated_node(&ids, &ctx.graph, gate_config) {
        return StepResult::Gated {
            node_id,
            gate_reason,
        };
    }
    let refs: Vec<&NodeId> = ids.iter().collect();
    if let Some(err) = check_budget(state, &ctx.graph, &refs) {
        return StepResult::Halted(err);
    }

    state.active_parallel_branches.push(ids.clone());
    let outcomes = run_batch_concurrently(runner, state, &ids).await;
    state.active_parallel_branches.retain(|b| b != &ids);

    let mut completions = Vec::with_capacity(outcomes.len());
    for (id, result) in outcomes {
        match result {
            NodeExecutionResult::Success(output) => completions.push((id, output)),
            NodeExecutionResult::Failure(failure) => return StepResult::Halted(failure.error),
        }
    }
    apply_completions_and_persist(ctx, state, gate_config, completions).await
}

/// A single boxed, pinned node-execution future, as produced by
/// [`run_batch_concurrently`] and consumed by [`join_all_local`].
type NodeFuture<'a> = Pin<Box<dyn Future<Output = (NodeId, NodeExecutionResult)> + Send + 'a>>;

/// Runs every node in `ids` concurrently via [`runner`](NodeRunner), preserving
/// input order in the returned `Vec`.
async fn run_batch_concurrently(
    runner: &dyn NodeRunner,
    state: &PipelineState,
    ids: &[NodeId],
) -> Vec<(NodeId, NodeExecutionResult)> {
    let futures: Vec<NodeFuture<'_>> = ids
        .iter()
        .map(|id| {
            let id = id.clone();
            let input = build_node_input(state);
            let fut = async move {
                let result = runner.run_node(&id, input).await;
                (id, result)
            };
            Box::pin(fut) as NodeFuture<'_>
        })
        .collect();
    join_all_local(futures).await
}

// ─── Minimal local "join all" combinator ─────────────────────────────────────

/// Polls a dynamic set of futures to completion within the current task,
/// without spawning.
///
/// `tokio::spawn` / `JoinSet::spawn` require `'static` futures, but
/// [`NodeRunner::run_node`] is invoked here through a borrowed `&dyn
/// NodeRunner` whose lifetime is tied to the caller's stack frame — spawning
/// is therefore not an option. This hand-rolled combinator uses the same
/// technique as `futures::future::join_all`: hold all futures pinned locally
/// and round-robin-poll them within a single outer future, so sibling futures
/// genuinely interleave at `.await` points (e.g. `tokio::time::sleep`) without
/// needing `'static` or a second OS thread. This is what makes
/// `test_run_step_with_runner_parallel_branch_executes_concurrently_and_preserves_input_order`
/// observe overlapping wall-clock intervals.
struct JoinAllLocal<'a, T> {
    slots: Vec<Option<Pin<Box<dyn Future<Output = T> + Send + 'a>>>>,
    results: Vec<Option<T>>,
}

// Safe: the only heap data that must stay pinned (the boxed sub-futures) is
// already pinned independently via `Pin<Box<...>>`. Moving the `JoinAllLocal`
// struct itself (e.g. via `Pin::get_mut`) only moves `Vec` headers and `Box`
// pointers, never the pointee — so it is sound to treat this wrapper as
// `Unpin` even though the futures it holds are not.
impl<T> Unpin for JoinAllLocal<'_, T> {}

impl<T> Future for JoinAllLocal<'_, T> {
    type Output = Vec<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut pending = false;
        for (slot, result) in this.slots.iter_mut().zip(this.results.iter_mut()) {
            if result.is_some() {
                continue;
            }
            let Some(fut) = slot else { continue };
            match fut.as_mut().poll(cx) {
                Poll::Ready(value) => {
                    *result = Some(value);
                    *slot = None;
                }
                Poll::Pending => pending = true,
            }
        }
        if pending {
            Poll::Pending
        } else {
            let taken = std::mem::take(&mut this.results);
            Poll::Ready(
                taken
                    .into_iter()
                    .map(|r| r.unwrap_or_else(|| unreachable!("resolved in the loop above")))
                    .collect(),
            )
        }
    }
}

async fn join_all_local<'a, T>(
    futures: Vec<Pin<Box<dyn Future<Output = T> + Send + 'a>>>,
) -> Vec<T> {
    let len = futures.len();
    JoinAllLocal {
        slots: futures.into_iter().map(Some).collect(),
        results: std::iter::repeat_with(|| None).take(len).collect(),
    }
    .await
}
