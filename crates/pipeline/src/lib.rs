//! Core orchestration domain for CogWorks.
//!
//! This crate contains every domain concept, newtype identifier, shared primitive
//! type, and cross-cutting error type used throughout the pipeline. Infrastructure
//! crates implement the traits defined here; they never add domain rules.
//!
//! ## Architectural Layer
//!
//! **Business logic + trait definitions.** This crate has no I/O dependencies.
//! It defines *what* is needed; infrastructure crates define *how* to supply it.
//!
//! ## Module Layout
//!
//! | Module | Contents |
//! |--------|----------|
//! | [`identifiers`] | Newtype domain identifiers (`WorkItemId`, `NodeId`, etc.) |
//! | [`types`] | Shared value types (`TokenCount`, `CostBudget`, `Diagnostic`, etc.) |
//! | [`errors`] | Top-level error and retry-policy types |
//! | [`graph`] | Pipeline graph model and runtime state types |
//! | [`github`] | GitHub traits: `EventSource`, `IssueTracker`, `PullRequestManager`, `CodeRepository`, `ProjectBoard` and their data types |
//! | [`templates`] | `TemplateEngine` trait |
//! | [`audit`] | `AuditStore` trait, `AuditEvent` enum, `PipelineSummary` |
//! | [`domain_services`] | `DomainServiceClient`, `InterfaceRegistryLoader`, `ScenarioExecutor`, `TwinProvisioner`, service routing, and their data types |
//! | [`knowledge`] | `SummaryCache`, `PipelineConfigurationLoader`, `ToolProfileStore`, `AdapterSpecLoader` and their data types |
//! | [`tools`] | `LlmProvider`, `StructuredResponse`, `ModelConfig`, `ChatMessage`, `OutputSchema`, `LlmError`, tool discovery, skill validation |
//! | [`security`] | Constitutional layer, injection detection, scope enforcement, tool parameter scope |
//! | [`context`] | Context assembly, context packs, `ClassificationResult`, `ContextPackage` |
//! | [`labels`] | `PipelineLabel` enum, `parse_label`, `generate_label` |
//! | [`execution`] | State machine decisions: `NextAction`, `NodeOutput`, `SubWorkItem`, execution functions |
//! | [`budget`] | Token cost budget enforcement: `acquire_budget`, `BudgetAcquisition`, `CostReport` |
//! | [`classification`] | Safety override and scope threshold: `apply_safety_override`, `check_scope_threshold` |
//! | [`review`] | Review aggregation: `ReviewPass`, `ReviewFinding`, `aggregate_review_results` |
//! | [`interfaces`] | Cross-domain constraint validation: `ConstraintFinding`, `validate_cross_domain_constraints` |
//! | [`json_guard`] | Iterative JSON nesting-depth guard: `exceeds_max_depth`, `MAX_SCHEMA_COMPARISON_DEPTH` |
//! | [`scenarios`] | Scenario satisfaction scoring: `PerScenarioScore`, `ScenarioSatisfactionResult`, `compute_satisfaction` |
//! | [`alignment`] | Alignment verification: `AlignmentFinding`, `AlignmentResult`, `run_deterministic_alignment` |
//! | [`traceability`] | Traceability matrix: `TraceabilityMatrix`, `extract_requirements`, `update_traceability_matrix` |
//! | [`observability`] | Observability hooks: `RootCause`, `record_node_start`, `record_node_complete`, `record_retry`, `record_rework` |
//!
//! ## Specification
//!
//! See [`docs/spec/interfaces/shared-types.md`] for shared types.
//! See [`docs/spec/interfaces/pipeline-graph.md`] for graph model types.
//! See [`docs/spec/interfaces/github-traits.md`] for GitHub trait contracts.
//! See [`docs/spec/interfaces/domain-traits.md`] for domain service, knowledge, and LLM provider contracts.
//! See [`docs/spec/interfaces/security.md`] for security and constitutional layer contracts.
//! See [`docs/spec/interfaces/context.md`] for context assembly and label contracts.
//! See [`docs/spec/interfaces/pipeline-execution.md`] for state machine, budget, review, and cross-domain validation contracts.
//! See [`docs/spec/interfaces/advanced-features.md`] for scenario satisfaction, alignment, traceability, observability, tool discovery, skill validation, and service routing contracts.

pub mod alignment;
pub mod audit;
pub mod budget;
pub mod classification;
pub mod context;
pub mod domain_services;
pub mod errors;
pub mod execution;
pub mod github;
pub mod graph;
pub mod identifiers;
pub mod interfaces;
pub mod json_guard;
pub mod knowledge;
pub mod labels;
pub mod observability;
pub mod review;
pub mod scenarios;
pub mod security;
pub mod templates;
pub mod tools;
pub mod traceability;
pub mod types;

// Re-export everything at the crate root for ergonomic usage by downstream crates.
pub use audit::{
    AuditEvent, AuditStore, AuditStoreError, CostSnapshot, InjectionDetectionRecord, LlmCallRecord,
    PipelineOutcome, PipelineSummary, ScopeViolationRecord, StateTransitionRecord,
    ValidationRecord,
};
pub use errors::{CogWorksError, RetryPolicy};
pub use github::{
    CodeRepository, DirectoryEntry, DirectoryEntryKind, EventSource, EventSourceError, FileContent,
    GitHubEvent, GitHubOperationError, Issue, IssueComment, IssueState, IssueTracker, Label,
    Milestone, ProjectBoard, PullRequest, PullRequestFilter, PullRequestManager, QueueEventConfig,
    ReviewDecision, ReviewStatus, SubIssue, TypedLink, TypedLinkKind, WebhookConfig,
};
pub use graph::{
    CompositeCondition, CycleError, EdgeConditionKind, EdgeDefinition, EdgeEvaluationRecord,
    EvaluationMode, EvaluatorKind, Expression, GateConfig, GateStatus, GraphValidationError,
    NaturalLanguageCondition, NodeDefinition, NodeGate, NodeState, NodeStatus, NodeType,
    OverflowBehaviour, PipelineConfiguration, PipelineGraph, PipelineSettings, PipelineState,
    PipelineStateComment, PipelineToolProfileConfig, ReworkEdge, ReworkSemantics, SchemaVersion,
    TimeoutSeconds, ValidationKind, compute_eligible_nodes, evaluate_deterministic_condition,
    topological_sort, validate_pipeline_graph,
};
pub use identifiers::{
    ArtifactPath, BranchName, CommitSha, ContextPackId, DomainServiceName, EdgeId, GitObjectSha,
    InterfaceId, MilestoneId, NodeId, PipelineName, PipelineRunId, ProfileName, PullRequestId,
    RepositoryId, SkillName, SubWorkItemId, ToolName, WorkItemId,
};
pub use templates::{TemplateEngine, TemplateError};
pub use types::{
    AlignmentScore, ApiVersion, CostBudget, Diagnostic, DiagnosticCategory, DiagnosticSeverity,
    SatisfactionScore, Timestamp, TokenCost, TokenCount,
};

// Re-exports from domain_services
pub use domain_services::{
    AcceptanceCriteria, DependencyGraph, DependencyResult, Diagnostics, DomainServiceClient,
    DomainServiceError, FailureInjection, FailureProfile, HandshakeResult, HealthStatus,
    InterfaceDefinition, InterfaceMap, InterfaceRegistryLoader, NormaliseResult, RegistryError,
    SatisfactionDetermination, Scenario, ScenarioError, ScenarioExecutor, SimulationResults,
    TrajectoryResult, TwinError, TwinHandle, TwinProvisioner, TwinSpec, ValidationResult,
};

// Re-exports from knowledge
pub use knowledge::{
    AdapterSpecLoader, ApiSpec, CacheError, ConfigError, OperationSpec,
    PipelineConfigurationLoader, ProfileError, PyramidSummary, ScopeParameters, SpecError,
    SpecInfo, SummaryCache, SummaryLevel, ToolOverrides, ToolProfile, ToolProfileStore,
};

// Re-exports from tools
pub use tools::{
    ChatMessage, ChatRole, LlmError, LlmProvider, ModelConfig, OutputSchema, StructuredResponse,
};

// Re-exports from security
pub use security::{
    ApprovedBranches, ApprovedScope, ConstitutionalError, ConstitutionalRules,
    InjectionDetectionResult, InjectionPattern, InvalidGlobPattern, PromptAssembly, ProtectedPath,
    RequiredRule, ScopeViolation, ScopeViolationKind, ToolParams, ToolScopeViolation,
    ValidatedPrompt, detect_injection, is_protected, validate_constitutional_prompt,
    validate_protected_paths, validate_scope, validate_tool_scope,
};

// Re-exports from context
pub use context::{
    ClassificationResult, ContextAssemblyRequest, ContextItem, ContextPack, ContextPackTrigger,
    ContextPackage, ContextPriority, HoldoutFilteredItems, LoadedContextPacks, MergedGuidance,
    TaskType, apply_priority_truncation, assemble_context, enforce_scenario_holdout,
    estimate_token_count, merge_pack_guidance, select_context_packs,
};

// Re-exports from labels
pub use labels::{PipelineLabel, generate_label, parse_label};

// Re-exports from execution
pub use execution::{
    DependencyError, EscalationReason, NextAction, NodeOutput, NodeStateUpdate, PipelineError,
    SubWorkItem, TerminationConditionReached, check_fan_in_ready, determine_next_actions,
    evaluate_edge_condition, increment_rework_counter, topological_sort_sub_work_items,
};

// Re-exports from budget
pub use budget::{
    BudgetAcquisition, CostReport, NodeCostEntry, SubWorkItemCostEntry, acquire_budget,
};

// Re-exports from classification
pub use classification::{
    EscalationResult, SafetyCriticalRegistry, apply_safety_override, check_scope_threshold,
};

// Re-exports from review
pub use review::{
    AggregateReviewDecision, ReviewFinding, ReviewPass, ReviewResult, aggregate_review_results,
};

// Re-exports from interfaces
pub use interfaces::{ConstraintFinding, validate_cross_domain_constraints};

// Re-exports from scenarios
pub use scenarios::{PerScenarioScore, ScenarioSatisfactionResult, compute_satisfaction};

// Re-exports from alignment
pub use alignment::{
    AlignmentCheckType, AlignmentConfig, AlignmentFinding, AlignmentResult, DeclaredNodeInputs,
    MisalignmentType, TraceabilityEntry, run_deterministic_alignment,
};

// Re-exports from traceability
pub use traceability::{
    Requirement, RequirementRow, TraceabilityMatrix, TraceabilityStage, TraceabilityStatus,
    extract_requirements, update_traceability_matrix,
};

// Re-exports from observability
pub use observability::{
    RootCause, record_node_complete, record_node_start, record_retry, record_rework,
};

// Re-exports from tools (additions)
pub use tools::{
    CompactToolIndex, SkillError, SkillInvocation, SkillLifecycle, SkillManifest, ToolIndexEntry,
    ValidatedSkillInvocation, build_compact_index, search_tools, validate_skill_invocation,
};

// Re-exports from domain_services (additions)
pub use domain_services::{
    DomainServiceRegistration, RoutingError, ServiceCapabilities, ServiceRegistry,
    resolve_primary_and_secondary, select_service_for_artifacts,
};
