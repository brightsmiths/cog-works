//! Steps 7-8 of `run_step_with_runner`: persist the updated
//! `PipelineStateComment` and build the final `StepResult`.
//!
//! Split out of `executor/mod.rs` to keep that file within the project's
//! file-length standard; see `executor/mod.rs`'s module doc for the full
//! 8-step sequence this module contributes steps 7-8 to.

use std::collections::HashMap;

use pipeline::{
    ConstitutionalRules, ContextPackage, EdgeConditionKind, EdgeId, GateConfig, ModelConfig,
    NaturalLanguageCondition, NodeOutput, NodeState, NodeStatus, OutputSchema, PipelineGraph,
    PipelineState, PipelineStateComment, SchemaVersion, Timestamp, TokenCount,
    WorkItemId, evaluate_edge_condition,
};

use super::{PipelineExecutor, StepContext, sentinel_node_id};

/// Serialises and posts `comment` to the work-item issue. Failures are logged
/// (not propagated): by the time this is called, any node work has already
/// happened, and there is no `StepResult` variant for "succeeded but could
/// not persist" — this is a known limitation, flagged for follow-up.
async fn persist_comment(
    executor: &PipelineExecutor,
    work_item_id: WorkItemId,
    comment: &PipelineStateComment,
) {
    let body = match serde_json::to_string(comment) {
        Ok(b) => b,
        Err(err) => {
            tracing::error!(error = %err, "failed to serialise PipelineStateComment; state not persisted");
            return;
        }
    };
    if let Err(err) = executor.issues.post_comment(work_item_id, &body).await {
        tracing::error!(error = %err, "failed to post PipelineStateComment to issue");
    }
}

/// Builds and persists a `PipelineStateComment` reflecting the current
/// (unchanged) `state`/`gate_config` — used for terminal outcomes (run
/// complete, escalation, halt) where no new node output is being applied.
pub(super) async fn persist_terminal_state(
    ctx: &StepContext<'_>,
    state: &PipelineState,
    gate_config: &GateConfig,
) {
    let comment = PipelineStateComment {
        schema_version: SchemaVersion::current(),
        pipeline_run_id: ctx.run_id,
        work_item_id: ctx.work_item_id,
        state: state.clone(),
        gate_config: gate_config.clone(),
        graph_hash: ctx.graph_hash.clone(),
        written_at: Timestamp::now(),
    };
    persist_comment(ctx.executor, ctx.work_item_id, &comment).await;
}

/// Finds the best-effort "representative" completed node to report when
/// `determine_next_actions` returns an empty vec (run already complete).
///
/// `StepResult::Completed` requires a concrete `{ node_id, output }` pair, but
/// there is no node that "just completed" in this call (see spec-gap note in
/// `executor_tests.rs`). The most-recently-activated `Completed` node is
/// reported with an empty `NodeOutput` as a deliberate, documented
/// best-effort fallback for this genuinely underspecified terminal case.
fn representative_completion(state: &PipelineState) -> (pipeline::NodeId, NodeOutput) {
    let node_id = state
        .node_states
        .iter()
        .filter(|(_, ns)| ns.status == NodeStatus::Completed)
        .max_by_key(|(_, ns)| ns.activated_at)
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| sentinel_node_id("pipeline-complete"));
    (
        node_id,
        NodeOutput {
            artifacts: HashMap::new(),
            cost_delta: pipeline::TokenCost::zero(),
            state_updates: vec![],
        },
    )
}

pub(super) async fn finish_run_complete(
    ctx: &StepContext<'_>,
    state: &PipelineState,
    gate_config: &GateConfig,
) -> super::StepResult {
    persist_terminal_state(ctx, state, gate_config).await;
    let (node_id, output) = representative_completion(state);
    super::StepResult::Completed { node_id, output }
}

// ─── Edge evaluation ──────────────────────────────────────────────────────────

/// System prompt prefix used when resolving an `LlmEvaluated` edge condition
/// to a boolean via the LLM gateway.
const EDGE_CONDITION_SYSTEM_PROMPT: &str = "Evaluate whether the following condition holds, \
     given the preceding node's output. Respond with a JSON object of the \
     exact shape {\"result\": true} or {\"result\": false}.";

fn edge_condition_output_schema() -> OutputSchema {
    OutputSchema::new(serde_json::json!({
        "type": "object",
        "properties": { "result": { "type": "boolean" } },
        "required": ["result"]
    }))
    .unwrap_or_else(|| unreachable!("literal schema is always a valid JSON object"))
}

fn edge_condition_model_config() -> ModelConfig {
    ModelConfig {
        model_id: "edge-condition-evaluator".to_string(),
        context_window_size: 8_000,
        max_output_tokens: 256,
        input_cost_per_token: 0.0,
        output_cost_per_token: 0.0,
    }
}

fn empty_context_package() -> ContextPackage {
    ContextPackage {
        items: vec![],
        total_token_count: TokenCount::new(0),
        truncation_applied: false,
        assembly_errors: vec![],
    }
}

/// Resolves one `LlmEvaluated` edge condition to a boolean via the LLM
/// gateway. Any failure (prompt assembly or the gateway call itself) is
/// treated conservatively as `false` and logged; this orchestration-level
/// call must never halt the pipeline on its own.
async fn resolve_llm_edge(
    executor: &PipelineExecutor,
    rules: &ConstitutionalRules,
    condition: &NaturalLanguageCondition,
) -> bool {
    let system_prompt = format!(
        "{EDGE_CONDITION_SYSTEM_PROMPT}\n\nCondition: {}",
        condition.as_str()
    );
    let prompt = match crate::gateway::assemble_constitutional_prompt(rules, &system_prompt) {
        Ok(p) => p,
        Err(err) => {
            tracing::warn!(error = %err, "failed to assemble prompt for LlmEvaluated edge; defaulting to false");
            return false;
        }
    };
    let context = empty_context_package();
    match executor
        .llm_gateway
        .call(
            prompt,
            &context,
            &edge_condition_output_schema(),
            &edge_condition_model_config(),
        )
        .await
    {
        Ok(response) => response
            .content()
            .get("result")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        Err(err) => {
            tracing::warn!(error = %err, "LlmGateway call failed resolving LlmEvaluated edge; defaulting to false");
            false
        }
    }
}

/// Resolves every outgoing `LlmEvaluated` edge of any node in `completed_ids`
/// to a boolean, collecting the results into the map consumed by
/// [`evaluate_edge_condition`].
async fn resolve_llm_evaluated_edges(
    ctx: &StepContext<'_>,
    completed_ids: &[pipeline::NodeId],
) -> HashMap<EdgeId, bool> {
    let mut results = HashMap::new();
    for edge in &ctx.graph.edges {
        if !completed_ids.contains(&edge.source) {
            continue;
        }
        if let EdgeConditionKind::LlmEvaluated(condition) = &edge.condition {
            let outcome = resolve_llm_edge(ctx.executor, &ctx.rules, condition).await;
            results.insert(edge.id.clone(), outcome);
        }
    }
    results
}

/// Evaluates every outgoing edge of the just-completed node(s), for
/// observability. The boolean result is logged (not currently wired into the
/// audit trail — see the deferred-issue note in the Coder's report; follow-up
/// is tracked separately).
fn log_edge_evaluations(
    graph: &PipelineGraph,
    state: &PipelineState,
    completions: &[(pipeline::NodeId, NodeOutput)],
    completed_ids: &[pipeline::NodeId],
    llm_results: &HashMap<EdgeId, bool>,
    now: Timestamp,
) {
    for edge in &graph.edges {
        if !completed_ids.contains(&edge.source) {
            continue;
        }
        let Some((_, output)) = completions.iter().find(|(id, _)| id == &edge.source) else {
            continue;
        };
        let (result, _records) =
            evaluate_edge_condition(&edge.id, &edge.condition, state, output, llm_results, now);
        tracing::debug!(edge_id = %edge.id, result, "edge condition evaluated");
    }
}

/// Builds a fresh [`NodeState`] with default (never-started) values.
fn fresh_node_state() -> NodeState {
    NodeState {
        status: NodeStatus::Pending,
        attempt_count: 0,
        rework_count: 0,
        current_error: None,
        rework_edge_traversals: HashMap::new(),
        activated_at: None,
    }
}

/// Applies one node's [`NodeOutput`] to `state`: accumulates cost, marks the
/// node `Completed`, and applies any `state_updates` (used by the Spawning
/// node to activate child nodes).
fn apply_node_output(
    state: &mut PipelineState,
    id: &pipeline::NodeId,
    output: &NodeOutput,
    now: Timestamp,
) {
    state.cost_accumulator += output.cost_delta;

    let entry = state
        .node_states
        .entry(id.clone())
        .or_insert_with(fresh_node_state);
    entry.status = NodeStatus::Completed;
    entry.attempt_count += 1;
    entry.current_error = None;
    entry.activated_at = Some(now);

    for update in &output.state_updates {
        let target = state
            .node_states
            .entry(update.node_id.clone())
            .or_insert_with(fresh_node_state);
        target.status = update.new_status;
        target.current_error = update.error.clone();
    }
}

fn build_completion_result(
    mut completions: Vec<(pipeline::NodeId, NodeOutput)>,
) -> super::StepResult {
    if completions.len() == 1 {
        let (node_id, output) = completions.remove(0);
        super::StepResult::Completed { node_id, output }
    } else {
        super::StepResult::CompletedParallel {
            results: completions,
        }
    }
}

/// Applies a batch of node completions to `state` (step 7), resolves and
/// evaluates outgoing edges, persists the updated `PipelineStateComment`, and
/// builds the final `StepResult` (step 8).
pub(super) async fn apply_completions_and_persist(
    ctx: &StepContext<'_>,
    state: &mut PipelineState,
    gate_config: &GateConfig,
    completions: Vec<(pipeline::NodeId, NodeOutput)>,
) -> super::StepResult {
    let now = Timestamp::now();
    let completed_ids: Vec<pipeline::NodeId> =
        completions.iter().map(|(id, _)| id.clone()).collect();

    for (id, output) in &completions {
        apply_node_output(state, id, output, now);
    }

    let llm_results = resolve_llm_evaluated_edges(ctx, &completed_ids).await;
    log_edge_evaluations(
        &ctx.graph,
        state,
        &completions,
        &completed_ids,
        &llm_results,
        now,
    );

    persist_terminal_state(ctx, state, gate_config).await;

    build_completion_result(completions)
}
