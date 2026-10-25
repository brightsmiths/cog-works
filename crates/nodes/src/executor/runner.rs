//! The [`NodeRunner`] test seam and its production implementation,
//! [`DefaultNodeRunner`].
//!
//! Split out of `executor/mod.rs` to keep that file within the project's
//! file-length standard.

use std::collections::HashMap;
use std::sync::Arc;

use pipeline::{
    AuditStore, CodeRepository, CostBudget, DomainServiceClient, InterfaceRegistryLoader,
    IssueTracker, NodeId, PipelineGraph, PipelineRunId, PipelineSettings,
    PipelineToolProfileConfig, ProfileName, PullRequestManager, SummaryCache,
};

use crate::gateway::LlmGateway;
use crate::node::{
    BudgetTracker, CliConfig, ConstraintValidator, ContextAssembler, ContextPackLoader,
    NodeExecutionResult, NodeInput, PipelineConfig, WorkingCopyManager,
};

use super::PipelineExecutor;

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
/// functions. Built once per [`run_step`](super::run_step) call.
///
/// Every real node body is still `todo!()` (#58-#62); calling `run_node` will
/// panic for any `node_id`, which is expected and out of scope for this task.
pub(super) struct DefaultNodeRunner {
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
    pub(super) fn new(executor: &PipelineExecutor, config: &CliConfig) -> Self {
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

    /// Builds a [`ContextAssembler`] backed by this runner's summary cache.
    ///
    /// Shared by every node branch that needs pyramid-style context assembly
    /// (`architecture`, `interface_design`, `code_generation`), so the
    /// construction appears once instead of once per branch.
    fn context_assembler(&self) -> ContextAssembler {
        ContextAssembler::new(self.summary_cache.clone())
    }

    /// Builds a [`ConstraintValidator`] backed by this runner's interface
    /// registry.
    ///
    /// Shared by every node branch that needs cross-domain constraint
    /// validation (`architecture`, `review`).
    fn constraint_validator(&self) -> ConstraintValidator {
        ConstraintValidator::new(self.interface_registry.clone())
    }

    async fn run_intake(&self, input: NodeInput) -> NodeExecutionResult {
        crate::intake::IntakeNode::execute(
            input,
            &self.llm_gateway,
            self.issues.as_ref(),
            &self.pipeline_config,
        )
        .await
    }

    async fn run_architecture(&self, input: NodeInput) -> NodeExecutionResult {
        let context = self.context_assembler();
        let constraint_validator = self.constraint_validator();
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

    async fn run_interface_design(&self, input: NodeInput) -> NodeExecutionResult {
        let context = self.context_assembler();
        crate::interface_design::InterfaceDesignNode::execute(
            input,
            &self.llm_gateway,
            &context,
            self.domain_service.as_ref(),
            self.code_repository.as_ref(),
        )
        .await
    }

    async fn run_planning(&self, input: NodeInput) -> NodeExecutionResult {
        crate::planning::PlanningNode::execute(
            input,
            &self.llm_gateway,
            self.issues.as_ref(),
            &self.pipeline_config,
        )
        .await
    }

    async fn run_code_generation(&self, input: NodeInput) -> NodeExecutionResult {
        let context = self.context_assembler();
        let budget_limit = self
            .pipeline_config
            .graph
            .settings
            .default_cost_budget
            .unwrap_or_else(|| {
                CostBudget::new(f64::MAX).unwrap_or_else(|| unreachable!("f64::MAX is valid"))
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

    async fn run_review(&self, input: NodeInput) -> NodeExecutionResult {
        let constraint_validator = self.constraint_validator();
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

    async fn run_integration(&self, input: NodeInput) -> NodeExecutionResult {
        let working_copy =
            WorkingCopyManager::new(self.code_repository.clone(), self.working_branch.clone());
        crate::integration::IntegrationNode::execute(
            input,
            self.pull_requests.as_ref(),
            &working_copy,
        )
        .await
    }

    async fn run_spawning(&self, input: NodeInput) -> NodeExecutionResult {
        crate::spawning::SpawningNode::execute(input, &self.llm_gateway, self.issues.as_ref())
            .await
    }
}

#[async_trait::async_trait]
impl NodeRunner for DefaultNodeRunner {
    async fn run_node(&self, node_id: &NodeId, input: NodeInput) -> NodeExecutionResult {
        match node_id.as_str() {
            "intake" => self.run_intake(input).await,
            "architecture" => self.run_architecture(input).await,
            "interface_design" => self.run_interface_design(input).await,
            "planning" => self.run_planning(input).await,
            "code_generation" => self.run_code_generation(input).await,
            "review" => self.run_review(input).await,
            "integration" => self.run_integration(input).await,
            "spawning" => self.run_spawning(input).await,
            other => panic!(
                "DefaultNodeRunner: unknown node id {other:?} — the pipeline graph referenced a \
                 node name with no corresponding node module"
            ),
        }
    }
}
