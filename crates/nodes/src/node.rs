//! Common node types, coordinator types, and step configuration.
//!
//! ## Node I/O types
//!
//! Every node `execute` function takes a [`NodeInput`] and returns a
//! [`NodeExecutionResult`]. [`NodeOutput`] (from `pipeline::execution`) is the
//! success payload; [`NodeFailure`] wraps the error path.
//!
//! ## Coordinator types
//!
//! [`ContextAssembler`], [`ConstraintValidator`], [`ContextPackLoader`],
//! [`BudgetTracker`], and [`WorkingCopyManager`] combine infrastructure traits
//! into focused helpers. They contain no domain logic — just wiring.
//!
//! ## Configuration types
//!
//! [`PipelineConfig`] carries the resolved graph and run identifier.
//! [`CliConfig`] carries the CLI-level settings (working directory, pipeline
//! name, approved branch for constitutional rules).
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/nodes.md` §Common Node Types,
//! §Coordinator Types, and §Step Configuration Types.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use pipeline::{
    BranchName, BudgetAcquisition, CodeRepository, CostBudget, InterfaceRegistryLoader,
    LoadedContextPacks, NodeOutput, PipelineError, PipelineGraph, PipelineName, PipelineRunId,
    PipelineState, RetryPolicy, SummaryCache, TokenCost,
};

// ─── Node I/O types ───────────────────────────────────────────────────────────

/// The inputs delivered to a node's `execute` function.
///
/// Assembled by the [`PipelineExecutor`](crate::executor::PipelineExecutor)
/// from the current pipeline state and the matched context packs before
/// dispatching to any node.
///
/// See `docs/spec/interfaces/nodes.md` §NodeInput.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInput {
    /// Named input artifact slots from the pipeline graph's `declared_inputs`.
    ///
    /// Keys are slot names; values are domain-specific JSON payloads whose
    /// schemas are defined per-node.
    pub artifacts: HashMap<String, Value>,

    /// Snapshot of the pipeline state at the start of this node's execution.
    pub pipeline_state: PipelineState,

    /// Context packs matched and merged for this node invocation.
    pub loaded_context_packs: LoadedContextPacks,
}

// ---------------------------------------------------------------------------

/// A node execution that did not complete successfully.
///
/// See `docs/spec/interfaces/nodes.md` §NodeFailure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeFailure {
    /// The error that caused the node to fail.
    pub error: PipelineError,
    /// Whether the failure can be retried and, if so, after what delay.
    pub retry_policy: RetryPolicy,
}

// ---------------------------------------------------------------------------

/// The result of one node `execute` invocation.
///
/// On `Success`, the [`NodeOutput`] is applied to the pipeline state by the
/// executor. On `Failure`, the executor consults the [`NodeFailure::retry_policy`]
/// to decide whether to retry, rework, escalate, or halt.
///
/// See `docs/spec/interfaces/nodes.md` §NodeExecutionResult.
#[derive(Debug)]
pub enum NodeExecutionResult {
    /// The node completed successfully; the output is ready to be applied.
    Success(NodeOutput),
    /// The node failed; inspect [`NodeFailure::retry_policy`] to decide next steps.
    Failure(NodeFailure),
}

// ─── Step configuration types ────────────────────────────────────────────────

/// Resolved per-run pipeline configuration.
///
/// Obtained from [`pipeline::PipelineConfigurationLoader`] at the start of
/// `run_step`. Passed to nodes that need to know the graph structure or the
/// current run identifier.
///
/// See `docs/spec/interfaces/nodes.md` §PipelineConfig.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// The pipeline graph for this run.
    pub graph: PipelineGraph,
    /// The run identifier for the current step.
    pub run_id: PipelineRunId,
}

// ---------------------------------------------------------------------------

/// CLI-level launch configuration.
///
/// Set once at startup and held unchanged for the lifetime of a service-mode
/// run. Passed to `run_step` on every invocation.
///
/// See `docs/spec/interfaces/nodes.md` §CliConfig.
#[derive(Debug, Clone)]
pub struct CliConfig {
    /// Working directory for this pipeline run.
    pub working_dir: PathBuf,
    /// Named pipeline to use; the default pipeline is used when `None`.
    pub pipeline_name: Option<PipelineName>,
    /// The repository branch approved for loading constitutional rules.
    ///
    /// Only the default branch (or an explicitly approved list) is accepted.
    /// Feature branches and forks are rejected.
    pub approved_branch: BranchName,
}

// ─── Coordinator types ────────────────────────────────────────────────────────

/// Assembles [`pipeline::ContextPackage`] values for LLM node invocations.
///
/// Delegates to [`pipeline::SummaryCache`] and the pure
/// [`pipeline::assemble_context`] function. Nodes call this instead of
/// constructing context packages directly.
///
/// See `docs/spec/interfaces/nodes.md` §ContextAssembler.
pub struct ContextAssembler {
    summary_cache: Arc<dyn SummaryCache>,
}

impl ContextAssembler {
    /// Creates a new `ContextAssembler` backed by `summary_cache`.
    pub fn new(summary_cache: Arc<dyn SummaryCache>) -> Self {
        Self { summary_cache }
    }

    /// Returns a reference to the underlying summary cache.
    pub(crate) fn summary_cache(&self) -> &dyn SummaryCache {
        self.summary_cache.as_ref()
    }
}

// ---------------------------------------------------------------------------

/// Validates cross-domain interface constraints.
///
/// Loads interface definitions via [`pipeline::InterfaceRegistryLoader`] and
/// delegates to [`pipeline::validate_cross_domain_constraints`].
///
/// See `docs/spec/interfaces/nodes.md` §ConstraintValidator.
pub struct ConstraintValidator {
    registry_loader: Arc<dyn InterfaceRegistryLoader>,
}

impl ConstraintValidator {
    /// Creates a new `ConstraintValidator` backed by `registry_loader`.
    pub fn new(registry_loader: Arc<dyn InterfaceRegistryLoader>) -> Self {
        Self { registry_loader }
    }

    /// Returns a reference to the underlying registry loader.
    pub(crate) fn registry_loader(&self) -> &dyn InterfaceRegistryLoader {
        self.registry_loader.as_ref()
    }
}

// ---------------------------------------------------------------------------

/// Reads context pack TOML files from the repository.
///
/// Delegates to [`pipeline::CodeRepository`] for file access.
///
/// See `docs/spec/interfaces/nodes.md` §ContextPackLoader.
pub struct ContextPackLoader {
    repository: Arc<dyn CodeRepository>,
}

impl ContextPackLoader {
    /// Creates a new `ContextPackLoader` backed by `repository`.
    pub fn new(repository: Arc<dyn CodeRepository>) -> Self {
        Self { repository }
    }

    /// Returns a reference to the underlying code repository.
    pub(crate) fn repository(&self) -> &dyn CodeRepository {
        self.repository.as_ref()
    }
}

// ---------------------------------------------------------------------------

/// Tracks token cost budget with serialised check semantics for parallel safety.
///
/// All parallel nodes in a batch must call [`try_acquire`](Self::try_acquire)
/// while the same internal mutex is serialising those calls. `BudgetTracker`
/// is `Clone`; all clones share the same accumulator via `Arc`.
///
/// ## Atomicity Contract
///
/// `try_acquire` holds the mutex for the full `acquire_budget` call **and**
/// the accumulator update before releasing. This satisfies the atomicity
/// requirement in `docs/spec/interfaces/pipeline-execution.md §Budget
/// Enforcement`.
///
/// See `docs/spec/interfaces/nodes.md` §BudgetTracker.
#[derive(Clone)]
pub struct BudgetTracker {
    accumulated: Arc<Mutex<TokenCost>>,
    limit: CostBudget,
}

impl BudgetTracker {
    /// Creates a new `BudgetTracker` with zero accumulated cost and `limit`.
    pub fn new(limit: CostBudget) -> Self {
        Self {
            accumulated: Arc::new(Mutex::new(TokenCost::zero())),
            limit,
        }
    }

    /// Checks whether `estimated` fits within the remaining budget.
    ///
    /// Holds the mutex for the full check and — if approved — the accumulator
    /// update before releasing, satisfying the atomicity contract in
    /// `docs/spec/interfaces/pipeline-execution.md §Budget Enforcement`.
    ///
    /// See `docs/spec/interfaces/nodes.md` §BudgetTracker.
    pub fn try_acquire(&self, estimated: &TokenCost) -> BudgetAcquisition {
        let mut accumulated = crate::lock_or_recover(&self.accumulated);

        let decision = pipeline::acquire_budget(&accumulated, estimated, &self.limit, || {
            pipeline::CostReport {
                per_node: vec![],
                per_sub_work_item: vec![],
                total: *accumulated,
                budget_limit: self.limit,
            }
        });

        if let BudgetAcquisition::Approved { .. } = decision {
            *accumulated += *estimated;
        }

        decision
    }
}

// ---------------------------------------------------------------------------

/// Manages file reads and branch operations on the repository working copy.
///
/// Delegates to [`pipeline::CodeRepository`] for all I/O.
///
/// See `docs/spec/interfaces/nodes.md` §WorkingCopyManager.
pub struct WorkingCopyManager {
    repository: Arc<dyn CodeRepository>,
    working_branch: BranchName,
}

impl WorkingCopyManager {
    /// Creates a new `WorkingCopyManager` for `working_branch`.
    pub fn new(repository: Arc<dyn CodeRepository>, working_branch: BranchName) -> Self {
        Self {
            repository,
            working_branch,
        }
    }

    /// Returns the working branch.
    pub fn working_branch(&self) -> &BranchName {
        &self.working_branch
    }

    /// Returns a reference to the underlying code repository.
    pub(crate) fn repository(&self) -> &dyn CodeRepository {
        self.repository.as_ref()
    }
}

#[cfg(test)]
#[path = "node_tests.rs"]
mod tests;
