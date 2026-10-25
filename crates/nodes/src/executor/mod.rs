//! Pipeline executor — step-function loop and infrastructure dependency holder.
//!
//! [`PipelineExecutor`] holds every infrastructure dependency as an `Arc<dyn Trait>`
//! and is constructed once at startup in `cli/src/main.rs`. All pipeline runs
//! in service mode share the same executor instance.
//!
//! [`run_step`] drives one step of the pipeline state machine:
//!
//! 1. Load and validate constitutional rules. (`steps` submodule)
//! 2. Load and validate the pipeline graph. (`steps` submodule)
//! 3. Reconstruct pipeline state from GitHub. (`steps` submodule)
//! 4. Detect graph-hash drift on resume. (`steps` submodule)
//! 5. Determine next actions and dispatch — route action variants to the
//!    appropriate outcome path. (`dispatch` submodule)
//! 6. Execute eligible nodes (with budget serialisation for parallel
//!    batches). (`dispatch` submodule)
//! 7. Persist updated `PipelineStateComment` to GitHub. (`completion` submodule)
//! 8. Return a `StepResult`; callers must loop to process subsequent steps.
//!    (`completion` submodule)
//!
//! Node dispatch itself (the `NodeRunner` test seam and its production
//! implementation, `DefaultNodeRunner`) lives in the `runner` submodule.
//!
//! This module (`mod.rs`) holds only the items shared across all of the
//! above: `StepResult`, `PipelineExecutor`, the SHA-256 hashing helpers, the
//! `StepContext` bundle threaded through steps 4-8, and the top-level
//! `run_step_with_runner` / `run_step` entry points. Each step group was
//! split into its own file to keep every file within the project's
//! 500-line-per-file standard; see `docs/catalog/nodes.md` for the full
//! abstraction index.
//!
//! See `docs/spec/interfaces/nodes.md` §Pipeline Executor for the full contract.

use std::sync::Arc;

use pipeline::{
    AuditStore, CodeRepository, ConstitutionalRules, DomainServiceClient, EscalationReason,
    InterfaceRegistryLoader, IssueTracker, LlmProvider, NodeId, NodeOutput,
    PipelineConfigurationLoader, PipelineError, PipelineGraph, PipelineRunId, PullRequestManager,
    RetryPolicy, SummaryCache, ToolProfileStore, WorkItemId, determine_next_actions,
};
use tracing::instrument;

use crate::gateway::LlmGateway;
use crate::node::CliConfig;

mod completion;
mod dispatch;
mod runner;
mod steps;

pub(crate) use runner::NodeRunner;

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

// ─── Shared hashing helpers ───────────────────────────────────────────────────
//
// Used by both `steps` (constitutional-rules content hash, step 1) and this
// module's `run_step_with_runner` (pipeline-graph hash, step 4).

/// Computes a SHA-256 hex digest.
///
/// Mirrors `pipeline::security`'s private hashing helper (same crate, same
/// algorithm) so `ConstitutionalRules::source_hash` and
/// `PipelineStateComment::graph_hash` are computed consistently even though
/// `pipeline::security`'s own helper is not exported. This duplication is
/// tracked as a deferred cross-crate finding — see
/// `.llm/findings/pipeline-executor-57.md`.
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
    let rules = match steps::load_constitutional_rules(executor, config).await {
        Ok(rules) => rules,
        Err(halted) => return halted,
    };

    let graph = match steps::resolve_pipeline_graph(executor, config).await {
        Ok(graph) => graph,
        Err(halted) => return halted,
    };

    let (mut state, gate_config, run_id, prior_comment) =
        match steps::reconstruct_state(executor, work_item_id).await {
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
        return steps::finalize_graph_hash_drift(&ctx, comment).await;
    }

    let now = pipeline::Timestamp::now();
    let action = determine_next_actions(&state, &ctx.graph, &gate_config, now)
        .into_iter()
        .next();

    dispatch::dispatch_action(&ctx, runner, &mut state, gate_config, action).await
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
    let runner = runner::DefaultNodeRunner::new(executor, config);
    run_step_with_runner(executor, work_item_id, config, &runner).await
}

#[cfg(test)]
#[path = "../executor_tests.rs"]
mod tests;
