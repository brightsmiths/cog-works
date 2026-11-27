# Shared Type Registry

This registry catalogs every reusable type, trait, and pattern in the CogWorks
workspace. It is updated incrementally as each PR adds new definitions.

Every reusable type, trait and pattern is registered here. A new type that
duplicates one already listed is a defect, and a new type absent from this
registry is undocumented.

---

## Core Identifiers

All live in `crates/pipeline/src/identifiers.rs` and re-exported from `pipeline`.
Spec: `docs/spec/interfaces/shared-types.md` §Identifiers.

| Type | Wraps | Notes |
|------|-------|-------|
| `WorkItemId` | `u64` | GitHub Issue number (unit of work) |
| `SubWorkItemId` | `u64` | GitHub Issue number (planning sub-task) |
| `MilestoneId` | `u64` | GitHub Milestone number |
| `PullRequestId` | `u64` | GitHub PR number |
| `PipelineRunId` | `Uuid` | Generated per CLI invocation |
| `NodeId` | `String` | Node name within a resolved stage graph |
| `EdgeId` | `String` | Edge name within a resolved stage graph |
| `StageTypeId` | `String` | Stage type a node declares, drawn from the harness registry |
| `ProfileId` | `String` | Agent profile resolved for a stage |
| `HarnessVersion` | `String` | Harness version resolved from `.factory/pin.yaml` |
| `DisciplineId` | `String` | Discipline pack selected for the work |
| `SliceId` | `String` | Slice within an approved `plan.yaml` |
| `IdentityRole` | `String` | Role a stage attempt's commits are pushed under |
| `BundleHash` | `String` | 64-char hex SHA-256 of an assembled context bundle |
| `CheckRunId` | `u64` | GitHub check run carrying a gate verdict (read-only) |
| `BranchName` | `String` | Git branch name |
| `CommitSha` | `String` | 40-char hex git commit SHA |
| `GitObjectSha` | `String` | Git object SHA (blob or tree) as returned by the GitHub Contents API. Not a commit SHA. |
| `RepositoryId` | `String` | `"owner/repo"` format |
| `DomainServiceName` | `String` | Key in CogWorks' own process configuration |
| `ArtifactPath` | `String` | Repo-relative file path |
| `InterfaceId` | `String` | Interface contract ID |
| `ContextPackId` | `String` | Context Pack directory name |

---

## Core Value Types

All live in `crates/pipeline/src/types.rs` and re-exported from `pipeline`.
Spec: `docs/spec/interfaces/shared-types.md` §Value Types.

| Type | Purpose |
|------|---------|
| `TokenCount` | LLM token count (non-negative integer) |
| `TokenCost` | LLM call cost in USD (`f64`) |
| `CostBudget` | Maximum allowed cost cap (`f64`) |
| `AlignmentScore` | Alignment verification score in `[0.0, 1.0]` |
| `DiagnosticSeverity` | `Blocking` / `Warning` / `Informational` |
| `DiagnosticCategory` | Category tag string (open set) |
| `Diagnostic` | Structured finding from domain service / review / alignment |
| `ApiVersion` | Extension API semantic version `{ major, minor }` |
| `Timestamp` | UTC wall-clock timestamp (wraps `chrono::DateTime<Utc>`) |

---

## Core Error Types

All live in `crates/pipeline/src/errors.rs` and re-exported from `pipeline`.
Spec: `docs/spec/interfaces/shared-types.md` §Error Types.

| Type | Purpose |
|------|---------|
| `RetryPolicy` | `Retryable { after }` / `NonRetryable` — cross-cutting retry decision |
| `CogWorksError` | Pipeline-halting conditions (injection, budget, scope, config) |

---

## Domain Types (added in subsequent PRs)

The following domains will add entries here as work proceeds:

### Pipeline Graph (`pipeline/src/graph.rs`)

All types re-exported from `pipeline`.
Spec: `docs/spec/interfaces/pipeline-graph.md`.

**Auxiliary scalars**

| Type | Purpose |
|------|---------|
| `Expression` | Newtype — deterministic boolean predicate string |
| `TimeoutSeconds` | Newtype — serialisable timeout (wraps `u64` seconds) |
| `SchemaVersion` | Newtype — serde-enforced version token for `PipelineStateComment`; rejects unknown values at deserialisation |

**Graph structure enums**

| Type | Purpose |
|------|---------|
| `NodeGate` | `AutoProceed` / `HumanGated` |
| `EvaluationMode` | `AllMatching` / `FirstMatching` / `Explicit` |
| `ReworkSemantics` | `Retry` / `Rework` |
| `OverflowBehaviour` | `HaltWithError` / `Escalate` / `TakeEdge(EdgeId)` |
| `EdgeConditionKind` | `Always` / `Expression(Expression)` / `Composite` — no model-evaluated variant |
| `CompositeCondition` | `And` / `Or` / `Not` combinator |

**Graph structure structs**

| Type | Purpose |
|------|---------|
| `NodeDefinition` | Static node declaration (id, type, inputs, outputs, timeout, gate, …) |
| `ReworkEdge` | Back-edge metadata (max traversals ≥ 1, semantics, overflow behaviour) |
| `EdgeDefinition` | Static edge declaration (source, target, condition, rework metadata) |
| `PipelineSettings` | Pipeline-level execution defaults |
| `PipelineGraph` | Validated graph (nodes + edges + eval modes + explicit-edge lists + settings + tool_profiles) |

**Runtime state enums**

| Type | Purpose |
|------|---------|
| `NodeStatus` | `Pending` / `Active` / `Completed` / `Failed` / `HumanGated` |
| `EvaluatorKind` | `Deterministic` / `LlmModel { model_id }` / `Composite` |

**Runtime state structs**

| Type | Purpose |
|------|---------|
| `NodeState` | Per-node mutable state (status, attempts, rework counts, error) |
| `PipelineState` | Full run state (node states, parallel branches, `cost_accumulator: TokenCost`) |
| `EdgeEvaluationRecord` | Audit record for one edge-condition evaluation; `input_snapshot` is `serde_json::Value` |
| `PipelineStateComment` | Self-contained GitHub comment payload; `schema_version: SchemaVersion` enforced at serde |

**Error types**

| Type | Purpose |
|------|---------|
| `CycleError` | Returned by `topological_sort` when forward-edge cycle detected |
| `GraphValidationError` | Single structural violation from `validate_pipeline_graph` |

**Pure functions**

| Function | Signature summary |
|----------|------------------|
| `topological_sort` | `(&[NodeDefinition], &[EdgeDefinition]) → Result<Vec<NodeId>, CycleError>` |
| `evaluate_deterministic_condition` | `(&Expression, &PipelineState) → bool` |
| `validate_pipeline_graph` | `(&PipelineGraph) → Result<(), Vec<GraphValidationError>>` |
| `compute_eligible_nodes` | `(&PipelineState, &PipelineGraph) → Vec<NodeId>` |

### GitHub & Events (`pipeline/src/github.rs`, `pipeline/src/templates.rs`, `pipeline/src/audit.rs`)

All types re-exported from `pipeline`.
Spec: `docs/spec/interfaces/github-traits.md`.

**Event trigger types** (`github.rs`)

| Type | Purpose |
|------|---------|
| `GitHubEvent` | `LabelApplied` / `CommentPosted` / `SubIssueStateChanged` / `PullRequestReviewed` |
| `EventSourceError` | `Timeout` / `ConnectionLost` / `ParseError` / `AuthError` / `QueueError` |
| `WebhookConfig` | Bind address, path prefix, HMAC secret |
| `QueueEventConfig` | Provider config (opaque JSON), queue name, session ordering, retry attempts |

**Issue types** (`github.rs`)

| Type | Purpose |
|------|---------|
| `IssueState` | `Open` / `Closed` |
| `Label` | Name + optional CSS hex colour |
| `Milestone` | Numeric ID, title, optional due date |
| `TypedLinkKind` | `Blocks` / `IsBlockedBy` |
| `TypedLink` | Source ID, target ID, kind |
| `Issue` | Full issue view (ID, repo, title, body, state, labels, milestone, timestamps) |
| `SubIssue` | Sub-task view (ID, parent ID, title, state, created_at) |

**Pull request types** (`github.rs`)

| Type | Purpose |
|------|---------|
| `ReviewDecision` | `Approved` / `ChangesRequested` / `Commented` / `Dismissed` |
| `ReviewStatus` | Approval count, `changes_requested` flag, `approved` flag |
| `PullRequest` | Full PR view (ID, repo, title, body, branches, SHA, open/merged, review status, created_at) |
| `PullRequestFilter` | Optional base/head branch and open-only filter |

**Repository types** (`github.rs`)

| Type | Purpose |
|------|---------|
| `FileContent` | Path, raw bytes, SHA, content type; `as_text() -> Option<&str>` |
| `DirectoryEntryKind` | `File` / `Directory` / `Symlink` / `Submodule` |
| `DirectoryEntry` | Name, path, kind, SHA |

**Error type** (`github.rs`)

| Type | Purpose |
|------|---------|
| `GitHubOperationError` | `NotFound` / `PermissionDenied` / `RateLimitExhausted` / `Transient` / `ParseFailure` / `SdkCapabilityMissing` |

**Port traits** (`github.rs`)

| Trait | Implemented by | Purpose |
|-------|---------------|---------|
| `EventSource` | `GitHubWebhookEventSource`, `QueueEventSource`, CLI one-shot | Trigger source abstraction |
| `IssueTracker` | `GithubClient` | Issue / sub-issue / label / comment / milestone operations |
| `PullRequestManager` | `GithubClient` | PR lifecycle and review operations |
| `CodeRepository` | `GithubClient` | Read-only file and tree access |

**Template types** (`templates.rs`)

| Type | Purpose |
|------|---------|
| `TemplateError` | `NotFound` / `MissingVariables` / `SyntaxError` / `ConstraintViolation` |
| `TemplateEngine` *(trait)* | `render(name, context) -> String`, `list_required_variables(name) -> Vec<String>` |

**Audit types** (`audit.rs`)

| Type | Purpose |
|------|---------|
| `LlmCallRecord` | Model ID, token counts, cost, latency, schema_validated, timestamp |
| `ValidationRecord` | Node ID, kind, passed, diagnostics, timestamp |
| `StateTransitionRecord` | Node ID, from/to status, reason, timestamp |
| `CostSnapshot` | Node ID, accumulated, budget, budget_exceeded, timestamp |
| `InjectionDetectionRecord` | Node ID, source label, offending text, pattern name, timestamp |
| `ScopeViolationRecord` | Node ID, artifact path, description, violation kind, timestamp |
| `AuditEvent` | Union of all above + `EdgeEvaluation(EdgeEvaluationRecord)` |
| `PipelineOutcome` | `Completed` / `Failed` / `HumanGated` / `Escalated` |
| `PipelineSummary` | Run ID, work item, outcome, cost, duration, node counts, rework count, terminal message |
| `AuditStoreError` | `Unavailable` / `SerialisationError` — non-fatal |
| `AuditStore` *(trait)* | `record_event(...)`, `write_summary(...)` |

### Domain Services (`pipeline/src/domain_services.rs`)

All types re-exported from `pipeline`.
Spec: `docs/spec/interfaces/domain-traits.md` §Domain Service Traits.

**Data types**

| Type | Purpose |
|------|---------|
| `Diagnostics` | Collection of `Diagnostic` items; has `has_blocking() -> bool` helper |
| `NormaliseResult` | Files modified + diagnostics from a normalisation pass |
| `SimulationResults` | Executed and passed counts + diagnostics + opaque detail payload |
| `DependencyResult` | Dependency satisfaction flag + missing dep list + diagnostics |
| `InterfaceMap` | List of `InterfaceDefinition` entries extracted from artifacts |
| `DependencyGraph` | Directed graph: `nodes: Vec<String>`, `edges: Vec<(String, String)>` |
| `HealthStatus` | `Healthy` / `Degraded { message }` / `Unhealthy { message }` |
| `InterfaceDefinition` | Interface contract: id, domain, JSON schema, artifact types, version |
| `ValidationResult` | Schema validation outcome: `valid: bool` + `diagnostics: Diagnostics` |
| `AcceptanceCriteria` | Min score threshold + required/prohibited behaviour lists |
| `HandshakeResult` | Extension API handshake response: domain, api_version, capabilities, artifact/interface types |

**Error types**

| Type | Variants |
|------|---------|
| `DomainServiceError` | `ConnectionFailed` / `RequestFailed` / `ProtocolError` / `HandshakeFailed` / `ServiceUnavailable` |
| `RegistryError` | `LoadFailed` / `SchemaInvalid` / `NotFound` |

**Port traits**

| Trait | Implemented by | Purpose |
|-------|---------------|---------|
| `DomainServiceClient` | `ExtensionApiClient` | All domain service operations via Extension API |
| `InterfaceRegistryLoader` | Config adapter (wired in `cli`) | Human-authored interface registry |

### Knowledge & Configuration (`pipeline/src/knowledge.rs`)

All types re-exported from `pipeline`.
Spec: `docs/spec/interfaces/domain-traits.md` §Knowledge & Configuration Traits.

**Data types**

| Type | Purpose |
|------|---------|
| `ScopeParameters` | Artifact scope constraints: max file/new-file counts, allowed/prohibited glob patterns |

**Error types**

| Type | Variants |
|------|---------|
| `CacheError` | `Unavailable` / `SerialisationError` |
| `ConfigError` | `NotFound` / `ParseError` / `InvalidConfiguration` |
| `ProfileError` | `LoadFailed` / `NotFound` |
| `SpecError` | `LoadFailed` / `NotFound` |

**Port traits**

| Trait | Implemented by | Purpose |
|-------|---------------|---------|

### LLM Provider (`pipeline/src/tools.rs`)

All types re-exported from `pipeline`.
Spec: `docs/spec/interfaces/domain-traits.md` §LLM Provider Trait.

**Data types**

| Type | Purpose |
|------|---------|
| `ChatRole` | `System` / `User` / `Assistant` |
| `ChatMessage` | Chat turn: `role: ChatRole`, `content: String`; constructor helpers `system()`, `user()`, `assistant()` |
| `OutputSchema` | JSON Schema wrapper (newtype over `serde_json::Value`); validated at construction |
| `ModelConfig` | LLM model selection: model ID, context window, max tokens, cost per input/output token |
| `StructuredResponse` | Validated completion: `content`, `input_tokens`, `output_tokens`, `latency_ms`, `schema_validated` |

**Error type**

| Type | Variants |
|------|---------|
| `LlmError` | `RateLimited` / `ApiError` / `SchemaValidationFailed` / `NetworkError` / `ContextWindowExceeded` |

**Port trait**

| Trait | Implemented by | Purpose |
|-------|---------------|---------|
| `LlmProvider` | `ModelGatewayProvider` (llm crate) | Model gateway endpoint; holds no credential |

### Security (`pipeline/src/security.rs`)

All types re-exported from `pipeline`.
Spec: `docs/spec/interfaces/security.md`.

**Constitutional layer**

| Type | Purpose |
|------|------|
| `RequiredRule` | Enum of 5 required behavioural guardrails that must appear in every constitutional document |
| `ConstitutionalRules` | Loaded rules doc: `content`, `source_hash` (SHA-256 hex), `source_branch` |
| `ConstitutionalValidationResult` | Two-bool intermediate record: `all_required_rules_present`, `privileged_position_confirmed` |
| `PromptAssembly` | Raw materials before constitutional wrapping: `system_prompt`, `user_content` |
| `ValidatedPrompt` | Opaque wrapper; only constructor is `validate_constitutional_prompt` |
| `ConstitutionalError` | `MissingRules { missing }` / `InvalidSourceBranch { branch }` / `HashMismatch { expected, computed }` |

**Injection detection**

| Type | Purpose |
|------|------|
| `InjectionPattern` | `InstructionInjection` / `PersonaOverride` / `BehavioralModification` / `SystemPromptExtractionAttempt` |
| `InjectionDetectionResult` | `Clean` / `InjectionDetected { source, offending_text, pattern }` |

**Scope enforcement**

| Type | Purpose |
|------|------|
| `ScopeViolationKind` | `ScopeUnderspecified` / `ScopeAmbiguous` / `ProtectedPathViolation` / `UnauthorizedCapability` |
| `ScopeViolation` | Single scope violation: `kind`, `artifact_path: Option<ArtifactPath>`, `description` |
| `ApprovedScope` | Approved artifact patterns + max file/new-file counts for one operation; `from_scope_parameters()` ctor |
| `ProtectedPath` | Glob pattern + reason; matched by `is_protected` |

**Tool parameter scope**

| Type | Purpose |
|------|------|
| `ToolParams` | `HashMap<String, serde_json::Value>` parameter map; `empty()` ctor |
| `ToolScopeViolation` | Tool name + parameter name + violated constraint description |

**Pure functions**

| Function | Signature summary |
|----------|---------|
| `validate_constitutional_prompt` | `(&ConstitutionalRules, PromptAssembly) → Result<ValidatedPrompt, ConstitutionalError>` |
| `detect_injection` | `(&str, &str) → InjectionDetectionResult` — infallible |
| `validate_scope` | `(&[ArtifactPath], &ApprovedScope, &[ProtectedPath]) → Result<(), Vec<ScopeViolation>>` |
| `is_protected` | `(&ArtifactPath, &[ProtectedPath]) → bool` — infallible |
| `validate_tool_scope` | `(&ToolName, &ToolParams, &ScopeParameters) → Result<(), ToolScopeViolation>` |

### Context Assembly & Labels (`pipeline/src/context.rs`, `pipeline/src/labels.rs`)

All types re-exported from `pipeline`.
Spec: `docs/spec/interfaces/context.md`.

**Classification result** (`context.rs`)

| Type | Purpose |
|------|---------|
| `TaskType` | `Feature` / `BugFix` / `Documentation` / `Refactoring` / `Configuration` / `Testing` / `Security` / `Unknown` |
| `ClassificationResult` | Intake node output: `task_type`, `safety_affecting: bool`, `estimated_scope: u32`, `affected_modules` |

**Context item and package** (`context.rs`)

| Type | Purpose |
|------|---------|
| `ContextPriority` | `CurrentInterfaceDefinition(0)` → `TransitiveDependency(5)`; `Ord` by declaration order |
| `ContextItem` | One knowledge unit: content, summary_level, priority, token_count, source_path |

**Context pack types** (`context.rs`)

| Type | Purpose |
|------|---------|
| `ContextPackTrigger` | Label patterns + component tag patterns + `requires_safety_critical` flag |
| `ContextPack` | Domain knowledge bundle: id, trigger, knowledge text, safe/anti patterns, required artifacts, threshold override |
| `MergedGuidance` | Union-merged safe patterns, anti-patterns, required artifacts from all matched packs |
| `LoadedContextPacks` | Matched packs + `merged_guidance` + `strictest_threshold` |

**Context assembly request** (`context.rs`)

| Type | Purpose |
|------|---------|
| `ContextAssemblyRequest` | Assembly inputs: `stage_type`, `stage_contract`, `slice_description`, `affected_modules`, `zone_barrier_paths` |

**Context assembly pure/async functions** (`context.rs`)

| Function | Signature summary |
|----------|---------|
| `select_context_packs` | `(&ClassificationResult, &[ContextPack]) → Vec<ContextPackId>` — infallible |
| `merge_pack_guidance` | `(&[ContextPack]) → MergedGuidance` — infallible |
| `assemble_bundle` | `async (&ContextAssemblyRequest, &LoadedContextPacks, &[InterfaceDefinition], TokenCount) → ContextBundle` |
| `apply_priority_truncation` | `(Vec<ContextItem>, TokenCount) → ContextPackage` — infallible |
| `enforce_trust_zone` | `(Vec<ContextItem>, TrustZone) → ZoneFilteredItems` — infallible; defence in depth behind the allowlist |

**Pipeline labels** (`labels.rs`)

| Type | Purpose |
|------|---------|
| `PipelineLabel` | 12-variant enum covering trigger, status, gate, lock, security, and safety labels |

| Function | Signature summary |
|----------|---------|
| `parse_label` | `(&str) → Option<PipelineLabel>` — returns `None` for non-CogWorks labels |
| `generate_label` | `(&PipelineLabel) → String` — round-trip guaranteed with `parse_label` |

### Harness and Factory Boundaries (`pipeline/src/harness.rs`)

Spec: `docs/spec/interfaces/domain-traits.md`, `docs/spec/interfaces/github-traits.md`.

| Type | Kind | Notes |
|------|------|-------|
| `HarnessPin` | struct | The pin read from `.factory/pin.yaml` |
| `ResolvedHarness` | struct | A verified harness at one specific version; stable for a whole run |
| `HarnessPinProvenance` | struct | Asserts resolved content matches what the pin names |
| `StageTypeRegistry` | struct | Stage types a harness version defines; the validation source at graph load |
| `StageContract` | struct | Input allowlist, output schema, writable paths, network profile, budget, attempt and rework limits, identity role |
| `ProfileRef` | struct | The resolved profile and the harness version it came from |
| `Tier` | enum | `T0`–`T3`, ordered so a maximum is well defined and raise-only is enforceable |
| `RiskMap` | struct | Path globs to tiers, read from the target repository, human-owned |
| `TrustZone` | enum | Design, Oracle, Build, Review — determines bundle barriers and identity role |
| `ContextBundle` | struct | Allowlisted, hashed inputs for one stage attempt |
| `ZoneFilteredItems` | struct | Items retained and removed by the zone barrier, with the barrier that removed each |
| `MountPlan` | struct | Writable paths for one attempt, obtained from the write-scope service |
| `AttemptRequest` | struct | Image, bundle, mount plan, network profile, limits, identity role, branch |
| `AttemptHandle` | struct | Handle to an in-flight attempt; completion is observed in GitHub, not here |
| `CheckRun` | struct | A check run with its authoring App identity, conclusion and structured output |
| `CheckConclusion` | enum | `conclusion: None` means in progress, which is distinct from any conclusion |
| `GateOutput` | struct | Machine-readable gate detail; for a holdout, only failing requirement IDs |
| `VerdictState` | enum | Passing, failing, pending, absent |
| `FailureClass` | enum | Classified attempt failure, compared across attempts for early escalation |
| `AttemptDisposition` | enum | Retry, Rework, Escalate, Halt — the bounded-loop decision |
| `ReworkFeedback` | struct | Structured guidance for the next attempt; never test source |
| `StageAttestation` | struct | Stage type, profile, harness version, bundle hash, identity role, verdicts read |
| `SignedComment` | struct | An attestation payload with its detached signature, transported as a hidden comment |
| `HarnessLoader` | trait | Resolves the harness and reads everything agent-shaping from it. Read-only |
| `StageRunner` | trait | Runs one attempt in a fresh sandbox. The only path by which execution leaves this process |
| `CheckRunReader` | trait | Reads gate verdicts. The only path by which a verdict enters this process. Read-only |
| `MountPlanProvider` | trait | Obtains the mount plan bounding an attempt's writes |
| `RiskMapLoader` | trait | Reads `risk-map.yaml`. Read-only |

Three of these traits have **no write counterpart, deliberately**: `HarnessLoader`,
`CheckRunReader` and `RiskMapLoader`. Adding a write method to any of them would breach a
boundary rule in `docs/spec/architecture.md`.

### Execution Core (`pipeline/src/execution.rs`, `budget.rs`, `classification.rs`, `review.rs`, `interfaces.rs`)

All types re-exported from `pipeline`.
Spec: `docs/spec/interfaces/pipeline-execution.md`.

**Execution — node output and state machine** (`execution.rs`)

| Type | Purpose |
|------|---------|
| `NodeStateUpdate` | Requested runtime status change for a node: `node_id`, `new_status`, `error` |
| `NodeOutput` | Node execution result: `artifacts`, `cost_delta`, `state_updates` |
| `GateStatus` | `AwaitingApproval` / `Approved { approved_by }` / `Rejected { rejected_by, reason }` |
| `GateConfig` | All gate states for an active run: `gated_nodes: HashMap<NodeId, GateStatus>` |
| `PipelineError` | `NodeFailed` / `BudgetExceeded` / `GraphInvalid` / `ConstitutionalRulesLoadFailed` |
| `EscalationReason` | Human-readable escalation context: node, attempts, rework count, cost, description |
| `NextAction` | `ExecuteNode` / `ExecuteParallel` / `Wait` / `Escalate` / `HaltWithError` |
| `TerminationConditionReached` | Rework traversal limit exceeded: `edge_id`, `current_traversals`, `max_traversals` |
| `SubWorkItem` | Planning sub-task: `id: SubWorkItemId`, `description`, `depends_on: Vec<SubWorkItemId>` |
| `DependencyError` | `CyclicDependency { cycle }` / `UnknownDependency { item_id, unknown_dep }` |

**Execution — pure functions** (`execution.rs`)

| Function | Signature summary |
|----------|---------|
| `determine_next_actions` | `(&PipelineState, &PipelineGraph, &GateConfig) → Vec<NextAction>` |
| `evaluate_edge_condition` | `(&EdgeId, &EdgeConditionKind, &PipelineState, &NodeOutput, Timestamp) → (bool, EdgeEvaluationRecord)` |
| `check_fan_in_ready` | `(&NodeId, &PipelineState, &PipelineGraph) → bool` |
| `increment_rework_counter` | `(&EdgeId, &mut PipelineState, &PipelineGraph) → Result<u32, TerminationConditionReached>` |
| `topological_sort_sub_work_items` | `(&[SubWorkItem]) → Result<Vec<SubWorkItemId>, DependencyError>` |

**Budget enforcement** (`budget.rs`)

| Type | Purpose |
|------|---------|
| `NodeCostEntry` | Per-node cost breakdown entry: `node_id`, `total_cost` |
| `SubWorkItemCostEntry` | Per-sub-work-item cost entry: `sub_work_item_id`, `total_cost` |
| `CostReport` | Full cost breakdown: `per_node`, `per_sub_work_item`, `total`, `budget_limit` |
| `BudgetAcquisition` | `Approved { remaining: CostBudget }` / `Denied(CostReport)` |

| Function | Signature summary |
|----------|---------|
| `acquire_budget` | `(&TokenCost, &TokenCost, &CostBudget, impl FnOnce() -> CostReport) → BudgetAcquisition` — ⚠️ caller must hold mutex for parallel nodes; report closure only called on `Denied` |

**Classification** (`classification.rs`)

| Type | Purpose |
|------|---------|
| `EscalationResult` | `estimated_scope: u32`, `threshold: u32`; `description() → String` |

| Function | Signature summary |
|----------|---------|
| `derive_tier` | `(&RiskMap, &[ArtifactPath], Option<Tier>) → Tier` — maximum over paths, raise-only |
| `check_scope_threshold` | `(ClassificationResult, u32) → Result<ClassificationResult, EscalationResult>` |

**Review aggregation** (`review.rs`)

| Type | Purpose |
|------|---------|
| `ReviewFinding` | Single finding: `pass`, `severity: DiagnosticSeverity`, `description`, `location: Option<ArtifactPath>` |
| `ReviewResult` | Pass result: `pass`, `findings`; helpers `has_blocking()`, `blocking_findings()` |

| Function | Signature summary |
|----------|---------|
| `aggregate_review_results` | `(ReviewResult, ReviewResult, ReviewResult, u32, u32) → AggregateReviewDecision` |

**Cross-domain constraint validation** (`interfaces.rs`)

| Type | Purpose |
|------|---------|
| `ConstraintFinding` | Interface mismatch: `interface_id`, `parameter_name`, `expected_value`, `actual_value`, `owning_domain`, `violating_domain`, `severity` |

| Function | Signature summary |
|----------|---------|
| `validate_cross_domain_constraints` | `(&[InterfaceDefinition], &InterfaceMap) → Vec<ConstraintFinding>` |

### Nodes (`nodes/src/`)

| Type | Purpose |
|------|---------|
| `PipelineConfig` | Resolved per-run config passed to stage drivers: `harness_version`, `graph`, `graph_hash`, `stage_registry`, `run_id`, `tier`, `discipline`, `risk_map`, `scope_threshold`, `constitutional_rules` |
| `IntakeNode` | First pipeline node: fetch issue → injection scan (title + body) → LLM classify → safety override → scope check → extract requirements → initial `TraceabilityMatrix` |
| *(to be added)* | `NodeInput`, `NodeOutput`, `LlmGateway`, `PipelineExecutor`, `StepResult`, etc. |

---

## Infrastructure Types

Stubs introduced in PRs 3–4; full method bodies added in PR 10.

| Crate | Type | Implements | Added in |
|-------|------|-----------|----------|
| `listener` | `GitHubWebhookEventSource` | `EventSource` | PR 3 |
| `listener` | `QueueEventSource` | `EventSource` | PR 3 |
| `extension-api` | `ExtensionApiClient` | `DomainServiceClient` | PR 4 |
| `extension-api` | `ServiceTransportConfig` | — | PR 4 |
| `extension-api` | `TransportKind` | — | PR 4 |
| `llm` | `ModelGatewayProvider` | `LlmProvider` | — |
| `llm` | `ModelGatewayConfig` | — | — |

---

## Patterns

### Error Handling

All domain operations return `Result<T, E>`.
Infrastructure errors implement a `retry_policy(&self) -> RetryPolicy` method.
`CogWorksError` variants are all `NonRetryable`.

### Validation at Boundaries

Newtype constructors validate invariants (non-empty strings, non-negative costs,
bounded scores). **Never bypass constructors** by accessing inner fields directly.

### Observability

All public operations in `pipeline` that may emit structured events use
`tracing::instrument` or explicit `tracing::Span::enter()` calls.
Field names follow OpenTelemetry semantic conventions where applicable.

### Async

All infrastructure trait methods are `async`. Business logic functions in
`pipeline` are synchronous (pure functions on data).

### Serialisation

All types that appear in `PipelineStateComment` (written to GitHub) derive
`Serialize` and `Deserialize`. The format is JSON (via `serde_json`). The set
of serialisable types grows with each PR; this registry notes which types are
serialisable.
