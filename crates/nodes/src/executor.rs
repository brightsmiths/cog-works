//! Pipeline executor — step-function loop and infrastructure dependency holder.
//!
//! [`PipelineExecutor`] holds every infrastructure dependency as an `Arc<dyn Trait>`
//! and is constructed once at startup in `cli/src/main.rs`. All pipeline runs
//! in service mode share the same executor instance.
//!
//! [`run_step`] drives one step of the pipeline state machine:
//!
//! 1. Load and validate constitutional rules.
//! 2. Load and validate the pipeline graph.
//! 3. Reconstruct pipeline state from GitHub.
//! 4. Determine next actions.
//! 5. Dispatch — route action variants to the appropriate outcome path.
//! 6. Execute eligible nodes (with budget serialisation for parallel batches).
//! 7. Persist updated `PipelineStateComment` to GitHub.
//! 8. Return a `StepResult`; callers must loop to process subsequent steps.
//!
//! See `docs/spec/interfaces/nodes.md` §Pipeline Executor for the full contract.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use pipeline::{
    ApprovedBranches, AuditStore, BudgetAcquisition, CodeRepository, ConfigError,
    ConstitutionalRules, ContextPackage, CostBudget, DomainServiceClient, EdgeConditionKind,
    EdgeId, EscalationReason, GateConfig, GateStatus, InterfaceRegistryLoader, IssueTracker,
    LlmProvider, MergedGuidance, ModelConfig, NaturalLanguageCondition, NextAction, NodeGate,
    NodeId, NodeOutput, NodeState, NodeStatus, OutputSchema, PipelineConfiguration,
    PipelineConfigurationLoader, PipelineError, PipelineGraph, PipelineRunId, PipelineSettings,
    PipelineState, PipelineStateComment, PipelineToolProfileConfig, ProfileName, PromptAssembly,
    PullRequestManager, RetryPolicy, SatisfactionScore, SchemaVersion, SummaryCache, Timestamp,
    TokenCost, TokenCount, ToolProfileStore, WorkItemId, compute_eligible_nodes,
    determine_next_actions, evaluate_edge_condition, validate_constitutional_prompt,
};
use tracing::instrument;

use crate::gateway::LlmGateway;
use crate::node::{
    BudgetTracker, CliConfig, ConstraintValidator, ContextAssembler, ContextPackLoader,
    NodeExecutionResult, NodeInput, PipelineConfig, WorkingCopyManager,
};
use pipeline::LoadedContextPacks;

// ─── StepResult ──────────────────────────────────────────────────────────────

/// The outcome of one invocation of [`run_step`].
///
/// The `cli` crate inspects `StepResult` to decide whether to loop (service
/// mode) or exit (single-shot mode).
///
/// See `docs/spec/interfaces/nodes.md` §StepResult.
#[derive(Debug)]
pub enum StepResult {
    /// A single node completed successfully in this step.
    Completed {
        /// The node that completed.
        node_id: NodeId,
        /// The node's output, ready to be applied to the pipeline state.
        output: NodeOutput,
    },
    /// Multiple nodes completed concurrently in this step (parallel branch).
    ///
    /// Used when an `ExecuteParallel` action fired multiple nodes. The caller
    /// must apply **all** results to the pipeline state before evaluating
    /// outgoing edges. Results are in input order (the order nodes were given
    /// to `ExecuteParallel`); `tokio::join_all` preserves submission order.
    CompletedParallel {
        /// Per-node results for every node in the parallel branch.
        results: Vec<(NodeId, NodeOutput)>,
    },
    /// A [`pipeline::NodeGate::HumanGated`] node is awaiting reviewer approval.
    Gated {
        /// The node awaiting approval.
        node_id: NodeId,
        /// Human-readable explanation of what the reviewer must decide.
        gate_reason: String,
    },
    /// The pipeline cannot continue without human intervention.
    ///
    /// An escalation comment has been posted on the work-item issue.
    Escalated(EscalationReason),
    /// The pipeline encountered an unrecoverable error.
    ///
    /// A failure comment has been posted on the work-item issue.
    Halted(PipelineError),
}

// ─── PipelineExecutor ────────────────────────────────────────────────────────

/// Holds all infrastructure dependencies for one pipeline service instance.
///
/// Constructed once at startup in `cli/src/main.rs` and shared across all
/// `run_step` calls. All fields are `Arc`-wrapped so they can be cloned across
/// concurrent tasks when processing parallel node batches.
///
/// ## Construction
///
/// Use [`PipelineExecutor::new`] to assemble the executor from pre-built
/// infrastructure instances. The `cli` crate wires up the concrete types.
///
/// See `docs/spec/interfaces/nodes.md` §PipelineExecutor.
pub struct PipelineExecutor {
    /// Issue tracker for fetching work items and posting state comments.
    issues: Arc<dyn IssueTracker>,
    /// Pull request manager for creating and reviewing PRs.
    pull_requests: Arc<dyn PullRequestManager>,
    /// Code repository for reading files and constitutional rules.
    code_repository: Arc<dyn CodeRepository>,
    /// Domain service client for validation and interface extraction.
    domain_service: Arc<dyn DomainServiceClient>,
    /// LLM gateway shared across all nodes in each step.
    llm_gateway: Arc<LlmGateway>,
    /// Audit store for recording every LLM call, state transition, and finding.
    audit: Arc<dyn AuditStore>,
    /// Summary cache for pyramid-style context assembly.
    summary_cache: Arc<dyn SummaryCache>,
    /// Interface registry loader for cross-domain constraint validation.
    interface_registry: Arc<dyn InterfaceRegistryLoader>,
    /// Pipeline configuration loader for resolving the active pipeline graph.
    config_loader: Arc<dyn PipelineConfigurationLoader>,
    /// Tool profile store for resolving node tool availability.
    tool_profile_store: Arc<dyn ToolProfileStore>,
}

impl PipelineExecutor {
    /// Constructs a `PipelineExecutor` from the given infrastructure instances.
    ///
    /// This is the only constructor. It is called once from `cli/src/main.rs`
    /// after all infrastructure crates have been initialised.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        issues: Arc<dyn IssueTracker>,
        pull_requests: Arc<dyn PullRequestManager>,
        code_repository: Arc<dyn CodeRepository>,
        domain_service: Arc<dyn DomainServiceClient>,
        llm_provider: Arc<dyn LlmProvider>,
        audit: Arc<dyn AuditStore>,
        summary_cache: Arc<dyn SummaryCache>,
        interface_registry: Arc<dyn InterfaceRegistryLoader>,
        config_loader: Arc<dyn PipelineConfigurationLoader>,
        tool_profile_store: Arc<dyn ToolProfileStore>,
    ) -> Self {
        Self {
            issues,
            pull_requests,
            code_repository,
            domain_service,
            llm_gateway: Arc::new(LlmGateway::new(llm_provider)),
            audit,
            summary_cache,
            interface_registry,
            config_loader,
            tool_profile_store,
        }
    }
}

// ─── Local hashing helper ────────────────────────────────────────────────────

/// Path (relative to the repository root) from which constitutional rules are
/// loaded at the start of every pipeline step.
const CONSTITUTIONAL_RULES_PATH: &str = ".cogworks/constitutional-rules.md";

/// Computes a SHA-256 hex digest.
///
/// Mirrors `pipeline::security`'s private hashing helper (same crate, same
/// algorithm) so `ConstitutionalRules::source_hash` and
/// `PipelineStateComment::graph_hash` are computed consistently even though
/// `pipeline::security`'s own helper is not exported.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    Sha256::digest(data)
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Canonicalises a [`PipelineGraph`] to the JSON text whose hash becomes
/// `PipelineStateComment::graph_hash`.
///
/// All fixture graphs used by tests have empty `HashMap` fields, so a direct
/// `serde_json::to_string` is stable regardless of `HashMap` iteration order.
/// Falls back to an empty string (logged) if serialisation ever fails, which
/// never happens for a well-formed `PipelineGraph` in practice.
fn canonical_graph_json(graph: &PipelineGraph) -> String {
    serde_json::to_string(graph).unwrap_or_else(|err| {
        tracing::error!(error = %err, "failed to serialise PipelineGraph for hashing");
        String::new()
    })
}

/// Builds a [`NodeId`] from a non-empty string literal used internally as a
/// placeholder identifier (e.g. for infrastructure failures that are not
/// attributable to any specific graph node).
fn sentinel_node_id(label: &str) -> NodeId {
    NodeId::new(label)
        .unwrap_or_else(|| unreachable!("sentinel_node_id: label {label:?} must be non-empty"))
}

/// Builds a [`StepResult::Halted`] for an infrastructure failure that is not
/// attributable to any specific graph node (e.g. a GitHub API failure while
/// reconstructing pipeline state).
///
/// `PipelineError`'s variants are a closed, load-time/runtime "combined
/// lifecycle" enum (see its doc comment) with no general-purpose
/// infrastructure-failure variant. `NodeFailed` with a sentinel node id is
/// the closest fit; this is a deliberate simplification documented here for
/// future reviewers.
fn infra_halt(sentinel_label: &str, message: String) -> StepResult {
    StepResult::Halted(PipelineError::NodeFailed {
        node_id: sentinel_node_id(sentinel_label),
        error: message,
        retry_policy: RetryPolicy::NonRetryable,
    })
}

// ─── Step 1: constitutional rules ────────────────────────────────────────────

/// Builds a [`StepResult::Halted`] with [`PipelineError::ConstitutionalRulesLoadFailed`].
fn constitutional_halt(message: String) -> StepResult {
    StepResult::Halted(PipelineError::ConstitutionalRulesLoadFailed { message })
}

/// Loads and validates the constitutional rules document (step 1).
///
/// Reads `.cogworks/constitutional-rules.md` from `config.repository` at
/// `config.approved_branch`, computes its content hash, and validates it via
/// [`validate_constitutional_prompt`]. Any failure halts immediately, before
/// any other infrastructure call is made.
async fn load_constitutional_rules(
    executor: &PipelineExecutor,
    config: &CliConfig,
) -> Result<ConstitutionalRules, StepResult> {
    let file = executor
        .code_repository
        .read_file(
            &config.repository,
            CONSTITUTIONAL_RULES_PATH,
            config.approved_branch.as_str(),
        )
        .await
        .map_err(|err| {
            constitutional_halt(format!("failed to read constitutional rules: {err}"))
        })?;

    let content = String::from_utf8(file.content).map_err(|err| {
        constitutional_halt(format!(
            "constitutional rules file is not valid UTF-8: {err}"
        ))
    })?;

    let rules = ConstitutionalRules {
        source_hash: sha256_hex(content.as_bytes()),
        content,
        source_branch: config.approved_branch.clone(),
    };

    let approved = ApprovedBranches::new(vec![config.approved_branch.clone()]);
    let prompt = PromptAssembly {
        system_prompt: String::new(),
        user_content: String::new(),
    };
    validate_constitutional_prompt(&rules, prompt, &approved)
        .map_err(|err| constitutional_halt(err.to_string()))?;

    Ok(rules)
}

// ─── Step 2: pipeline configuration ──────────────────────────────────────────

/// Resolves the active [`PipelineGraph`] for this run (step 2).
///
/// `ConfigError::NotFound` falls back to the built-in default pipeline (per
/// that variant's documented contract); any other `ConfigError` halts with
/// [`PipelineError::GraphInvalid`], since those indicate a genuinely broken
/// configuration file rather than a simple absence.
async fn resolve_pipeline_graph(
    executor: &PipelineExecutor,
    config: &CliConfig,
) -> Result<PipelineGraph, StepResult> {
    let configuration = match executor
        .config_loader
        .load_pipeline_config(&config.working_dir)
        .await
    {
        Ok(cfg) => cfg,
        Err(ConfigError::NotFound { .. }) => PipelineConfiguration {
            pipelines: HashMap::new(),
        },
        Err(err) => {
            return Err(StepResult::Halted(PipelineError::GraphInvalid {
                message: err.to_string(),
            }));
        }
    };

    let graph = match &config.pipeline_name {
        Some(name) => executor
            .config_loader
            .get_named_pipeline(&configuration, name)
            .cloned()
            .unwrap_or_else(|| executor.config_loader.get_default_pipeline(&configuration)),
        None => executor.config_loader.get_default_pipeline(&configuration),
    };
    Ok(graph)
}

// ─── Step 3: reconstruct state ───────────────────────────────────────────────

/// Reconstructs [`PipelineState`] and [`GateConfig`] from the latest
/// `PipelineStateComment` on the work-item issue (step 3).
///
/// Scans comments **newest first** for the first one whose body parses as a
/// [`PipelineStateComment`]; non-JSON comments (human discussion) are skipped
/// without error. No match means a fresh run: a new [`PipelineState`] and the
/// default [`GateConfig`] are returned, along with a freshly generated
/// [`PipelineRunId`].
async fn reconstruct_state(
    executor: &PipelineExecutor,
    work_item_id: WorkItemId,
) -> Result<
    (
        PipelineState,
        GateConfig,
        PipelineRunId,
        Option<PipelineStateComment>,
    ),
    StepResult,
> {
    let comments = executor
        .issues
        .list_comments(work_item_id)
        .await
        .map_err(|err| {
            infra_halt(
                "pipeline-state-reconstruction",
                format!("failed to list issue comments: {err}"),
            )
        })?;

    let prior_comment = comments
        .iter()
        .rev()
        .find_map(|c| serde_json::from_str::<PipelineStateComment>(&c.body).ok());

    let (state, gate_config, run_id) = match &prior_comment {
        Some(comment) => (
            comment.state.clone(),
            comment.gate_config.clone(),
            comment.pipeline_run_id,
        ),
        None => {
            let run_id = PipelineRunId::new_random();
            (
                PipelineState {
                    run_id,
                    node_states: HashMap::new(),
                    active_parallel_branches: vec![],
                    cost_accumulator: TokenCost::zero(),
                },
                GateConfig::default(),
                run_id,
            )
        }
    };

    Ok((state, gate_config, run_id, prior_comment))
}

// ─── Step context ────────────────────────────────────────────────────────────

/// Immutable context threaded through steps 4-8, bundled to keep per-function
/// argument counts manageable.
struct StepContext<'a> {
    executor: &'a PipelineExecutor,
    work_item_id: WorkItemId,
    rules: ConstitutionalRules,
    graph: PipelineGraph,
    run_id: PipelineRunId,
    graph_hash: String,
}

// ─── Step 4: graph-hash drift ────────────────────────────────────────────────

/// Builds the [`EscalationReason`] for a detected `graph_hash` mismatch on
/// resume.
///
/// The mismatch is not attributable to any single node (it reflects a drift
/// between the persisted state and the freshly loaded graph as a whole), so
/// the node with the highest `attempt_count` is reported as representative;
/// absent any node states, a sentinel node id is used.
fn graph_hash_drift_escalation(
    comment: &PipelineStateComment,
    computed_hash: &str,
) -> EscalationReason {
    let (node_id, attempt_count, rework_count) = comment
        .state
        .node_states
        .iter()
        .max_by_key(|(_, ns)| ns.attempt_count)
        .map(|(id, ns)| (id.clone(), ns.attempt_count, ns.rework_count))
        .unwrap_or_else(|| (sentinel_node_id("pipeline-graph"), 0, 0));
    EscalationReason {
        description: format!(
            "Pipeline graph hash drift detected on resume: persisted state was \
             computed against a different graph (persisted {}, current {computed_hash})",
            comment.graph_hash
        ),
        node_id,
        attempt_count,
        rework_count,
        cost_spent: comment.state.cost_accumulator,
    }
}

/// Handles a detected `graph_hash` mismatch: escalates without executing any
/// node (Architectural Decision #4).
async fn finalize_graph_hash_drift(
    ctx: &StepContext<'_>,
    comment: &PipelineStateComment,
) -> StepResult {
    let reason = graph_hash_drift_escalation(comment, &ctx.graph_hash);
    persist_terminal_state(ctx, &comment.state, &comment.gate_config).await;
    StepResult::Escalated(reason)
}

// ─── Persistence helpers ─────────────────────────────────────────────────────

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
async fn persist_terminal_state(
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
fn representative_completion(state: &PipelineState) -> (NodeId, NodeOutput) {
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
            cost_delta: TokenCost::zero(),
            state_updates: vec![],
        },
    )
}

async fn finish_run_complete(
    ctx: &StepContext<'_>,
    state: &PipelineState,
    gate_config: &GateConfig,
) -> StepResult {
    persist_terminal_state(ctx, state, gate_config).await;
    let (node_id, output) = representative_completion(state);
    StepResult::Completed { node_id, output }
}

// ─── Step 5: dispatch ────────────────────────────────────────────────────────

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
fn build_wait_result(
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
) -> Option<PipelineError> {
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
        BudgetAcquisition::Denied(_) => Some(PipelineError::BudgetExceeded {
            accumulated: state.cost_accumulator,
            limit,
        }),
    }
}

async fn dispatch_action(
    ctx: &StepContext<'_>,
    runner: &dyn NodeRunner,
    state: &mut PipelineState,
    gate_config: GateConfig,
    action: Option<NextAction>,
) -> StepResult {
    match action {
        None => finish_run_complete(ctx, state, &gate_config).await,
        Some(NextAction::Wait) => build_wait_result(state, &ctx.graph, &gate_config),
        Some(NextAction::Escalate(reason)) => {
            persist_terminal_state(ctx, state, &gate_config).await;
            StepResult::Escalated(reason)
        }
        Some(NextAction::HaltWithError(err)) => {
            persist_terminal_state(ctx, state, &gate_config).await;
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

// ─── Step 6: execute nodes ────────────────────────────────────────────────────

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

// ─── Step 7: persist completions; Step 8: build StepResult ──────────────────

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
    completed_ids: &[NodeId],
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
    completions: &[(NodeId, NodeOutput)],
    completed_ids: &[NodeId],
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
fn apply_node_output(state: &mut PipelineState, id: &NodeId, output: &NodeOutput, now: Timestamp) {
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

fn build_completion_result(mut completions: Vec<(NodeId, NodeOutput)>) -> StepResult {
    if completions.len() == 1 {
        let (node_id, output) = completions.remove(0);
        StepResult::Completed { node_id, output }
    } else {
        StepResult::CompletedParallel {
            results: completions,
        }
    }
}

/// Applies a batch of node completions to `state` (step 7), resolves and
/// evaluates outgoing edges, persists the updated `PipelineStateComment`, and
/// builds the final `StepResult` (step 8).
async fn apply_completions_and_persist(
    ctx: &StepContext<'_>,
    state: &mut PipelineState,
    gate_config: &GateConfig,
    completions: Vec<(NodeId, NodeOutput)>,
) -> StepResult {
    let now = Timestamp::now();
    let completed_ids: Vec<NodeId> = completions.iter().map(|(id, _)| id.clone()).collect();

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

// ─── run_step_with_runner ─────────────────────────────────────────────────────

/// Drives one step of the pipeline state machine for `work_item_id`, routing
/// node dispatch (step 6) through the injected `runner` rather than calling
/// the real node `execute` functions directly.
///
/// This holds the complete 8-step sequence documented on [`run_step`] above.
/// The public `run_step` function is a thin wrapper around this function.
///
/// See `docs/spec/interfaces/nodes.md` §run_step for the detailed decision
/// table this function must implement.
pub(crate) async fn run_step_with_runner(
    executor: &PipelineExecutor,
    work_item_id: WorkItemId,
    config: &CliConfig,
    runner: &dyn NodeRunner,
) -> StepResult {
    let rules = match load_constitutional_rules(executor, config).await {
        Ok(rules) => rules,
        Err(halted) => return halted,
    };

    let graph = match resolve_pipeline_graph(executor, config).await {
        Ok(graph) => graph,
        Err(halted) => return halted,
    };

    let (mut state, gate_config, run_id, prior_comment) =
        match reconstruct_state(executor, work_item_id).await {
            Ok(v) => v,
            Err(halted) => return halted,
        };

    let graph_hash = sha256_hex(canonical_graph_json(&graph).as_bytes());
    let ctx = StepContext {
        executor,
        work_item_id,
        rules,
        graph,
        run_id,
        graph_hash,
    };

    if let Some(comment) = &prior_comment
        && comment.graph_hash != ctx.graph_hash
    {
        return finalize_graph_hash_drift(&ctx, comment).await;
    }

    let now = Timestamp::now();
    let action = determine_next_actions(&state, &ctx.graph, &gate_config, now)
        .into_iter()
        .next();

    dispatch_action(&ctx, runner, &mut state, gate_config, action).await
}

// ─── NodeRunner seam ──────────────────────────────────────────────────────────

/// Dispatches a single node's `execute` function given its [`NodeId`] and
/// [`NodeInput`].
///
/// Exists as a test seam: `run_step_with_runner`'s orchestration logic (state
/// reconstruction, budget checks, edge evaluation, state persistence) is
/// exercised against a `MockNodeRunner` without touching the real node bodies
/// (`IntakeNode::execute`, etc.), which remain `todo!()` under #58-#62.
/// `async_trait` is required here (not just in `[dev-dependencies]`) because
/// this trait is used from production code via `&dyn NodeRunner`, which
/// requires object safety that a native `async fn` in a trait does not
/// provide on its own.
#[async_trait::async_trait]
pub(crate) trait NodeRunner: Send + Sync {
    /// Executes the node identified by `node_id` with the given `input`.
    async fn run_node(&self, node_id: &NodeId, input: NodeInput) -> NodeExecutionResult;
}

// ─── DefaultNodeRunner ────────────────────────────────────────────────────────

/// Builds a placeholder [`PipelineGraph`] for [`DefaultNodeRunner`]'s
/// `PipelineConfig`.
///
/// The real node bodies (`IntakeNode::execute`, etc.) are still `todo!()`
/// (#58-#62) and panic before ever reading their `PipelineConfig` argument
/// meaningfully, so this placeholder is never actually consulted. Once those
/// node bodies land, `DefaultNodeRunner` will need the real resolved graph
/// threaded through — tracked as a follow-up, since `NodeRunner::run_node`'s
/// fixed signature does not carry it today.
fn placeholder_graph() -> PipelineGraph {
    PipelineGraph {
        nodes: vec![],
        edges: vec![],
        evaluation_modes: HashMap::new(),
        explicit_edge_lists: HashMap::new(),
        settings: PipelineSettings {
            default_timeout: None,
            default_cost_budget: None,
            max_node_retries: 0,
        },
        tool_profiles: PipelineToolProfileConfig {
            default_profile: ProfileName::new("default")
                .unwrap_or_else(|| unreachable!("non-empty literal")),
            node_overrides: HashMap::new(),
        },
    }
}

/// Default [`NodeRunner`] that dispatches to the real per-node `execute`
/// functions. Built once per [`run_step`] call.
///
/// Every real node body is still `todo!()` (#58-#62); calling `run_node` will
/// panic for any `node_id`, which is expected and out of scope for this task.
struct DefaultNodeRunner {
    issues: Arc<dyn IssueTracker>,
    pull_requests: Arc<dyn PullRequestManager>,
    code_repository: Arc<dyn CodeRepository>,
    domain_service: Arc<dyn DomainServiceClient>,
    llm_gateway: Arc<LlmGateway>,
    audit: Arc<dyn AuditStore>,
    summary_cache: Arc<dyn SummaryCache>,
    interface_registry: Arc<dyn InterfaceRegistryLoader>,
    pipeline_config: PipelineConfig,
    working_branch: pipeline::BranchName,
}

impl DefaultNodeRunner {
    fn new(executor: &PipelineExecutor, config: &CliConfig) -> Self {
        Self {
            issues: executor.issues.clone(),
            pull_requests: executor.pull_requests.clone(),
            code_repository: executor.code_repository.clone(),
            domain_service: executor.domain_service.clone(),
            llm_gateway: executor.llm_gateway.clone(),
            audit: executor.audit.clone(),
            summary_cache: executor.summary_cache.clone(),
            interface_registry: executor.interface_registry.clone(),
            pipeline_config: PipelineConfig {
                graph: placeholder_graph(),
                run_id: PipelineRunId::new_random(),
            },
            working_branch: config.approved_branch.clone(),
        }
    }
}

#[async_trait::async_trait]
impl NodeRunner for DefaultNodeRunner {
    async fn run_node(&self, node_id: &NodeId, input: NodeInput) -> NodeExecutionResult {
        match node_id.as_str() {
            "intake" => {
                crate::intake::IntakeNode::execute(
                    input,
                    &self.llm_gateway,
                    self.issues.as_ref(),
                    &self.pipeline_config,
                )
                .await
            }
            "architecture" => {
                let context = ContextAssembler::new(self.summary_cache.clone());
                let constraint_validator =
                    ConstraintValidator::new(self.interface_registry.clone());
                let pack_loader = ContextPackLoader::new(self.code_repository.clone());
                crate::architecture::ArchitectureNode::execute(
                    input,
                    &self.llm_gateway,
                    &context,
                    &constraint_validator,
                    &pack_loader,
                    self.code_repository.as_ref(),
                )
                .await
            }
            "interface_design" => {
                let context = ContextAssembler::new(self.summary_cache.clone());
                crate::interface_design::InterfaceDesignNode::execute(
                    input,
                    &self.llm_gateway,
                    &context,
                    self.domain_service.as_ref(),
                    self.code_repository.as_ref(),
                )
                .await
            }
            "planning" => {
                crate::planning::PlanningNode::execute(
                    input,
                    &self.llm_gateway,
                    self.issues.as_ref(),
                    &self.pipeline_config,
                )
                .await
            }
            "code_generation" => {
                let context = ContextAssembler::new(self.summary_cache.clone());
                let budget_limit = self
                    .pipeline_config
                    .graph
                    .settings
                    .default_cost_budget
                    .unwrap_or_else(|| {
                        CostBudget::new(f64::MAX)
                            .unwrap_or_else(|| unreachable!("f64::MAX is valid"))
                    });
                let budget = BudgetTracker::new(budget_limit);
                crate::code_generation::CodeGenerationNode::execute(
                    input,
                    &self.llm_gateway,
                    &context,
                    self.domain_service.as_ref(),
                    &budget,
                )
                .await
            }
            "review" => {
                let constraint_validator =
                    ConstraintValidator::new(self.interface_registry.clone());
                let packs = input.loaded_context_packs.clone();
                crate::review::ReviewNode::execute(
                    input,
                    &self.llm_gateway,
                    &constraint_validator,
                    &packs,
                    self.audit.as_ref(),
                )
                .await
            }
            "integration" => {
                let working_copy = WorkingCopyManager::new(
                    self.code_repository.clone(),
                    self.working_branch.clone(),
                );
                crate::integration::IntegrationNode::execute(
                    input,
                    self.pull_requests.as_ref(),
                    &working_copy,
                )
                .await
            }
            "spawning" => {
                crate::spawning::SpawningNode::execute(
                    input,
                    &self.llm_gateway,
                    self.issues.as_ref(),
                )
                .await
            }
            other => panic!(
                "DefaultNodeRunner: unknown node id {other:?} — the pipeline graph referenced a \
                 node name with no corresponding node module"
            ),
        }
    }
}

// ─── run_step ────────────────────────────────────────────────────────────────

/// Drives one step of the pipeline state machine for `work_item_id`.
///
/// ## Step Sequence
///
/// 1. **Load constitutional rules** — read from `config.approved_branch`.
///    Failure halts immediately (`StepResult::Halted`).
/// 2. **Load pipeline config** — resolve via `config.pipeline_name`.
///    Graph validation failure halts immediately.
/// 3. **Reconstruct state** — fetch the latest `PipelineStateComment` from GitHub.
/// 4. **Determine next actions** — call `determine_next_actions`.
/// 5. **Dispatch** — inspect actions:
///    - Empty vec → run complete; write final state comment; return `StepResult::Completed`.
///    - `Wait` → return `StepResult::Gated`.
///    - `Escalate` / `HaltWithError` → write comment; return `StepResult::Escalated` /
///      `StepResult::Halted`.
///    - `ExecuteNode` / `ExecuteParallel` → proceed to step 6.
/// 6. **Execute nodes** — for eligible nodes; serialise budget checks across
///    any parallel batch via `BudgetTracker`. For `ExecuteParallel`, all listed
///    nodes run concurrently within this single call. On any `NonRetryable`
///    failure, apply the retry/rework policy; escalate or halt as appropriate.
/// 7. **Persist state** — write updated `PipelineStateComment` to the issue
///    via `executor.issues.post_comment`. State includes contributions from all
///    nodes in the batch.
/// 8. **Return** — `StepResult::Completed { node_id, output }` for the node
///    that completed. For a parallel batch, one representative node is returned;
///    the caller **must loop** — calling `run_step` again will pick up the next
///    set of eligible nodes from the persisted state.
///
/// See `docs/spec/interfaces/nodes.md` §run_step for the detailed decision table.
#[instrument(skip_all, fields(work_item_id = %work_item_id))]
pub async fn run_step(
    executor: &PipelineExecutor,
    work_item_id: WorkItemId,
    config: &CliConfig,
) -> StepResult {
    let runner = DefaultNodeRunner::new(executor, config);
    run_step_with_runner(executor, work_item_id, config, &runner).await
}

#[cfg(test)]
#[path = "executor_tests.rs"]
mod tests;
