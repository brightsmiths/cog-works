# Pipeline Nodes — Interface Specification

**Architectural Layer**: Orchestration (sequences business logic and infrastructure calls; no domain rules)
**Module Paths**: `crates/nodes/src/`
**Specification Version**: 1.0

---

## Overview

The `nodes` crate contains the orchestration layer that drives each pipeline
step. It holds all node implementations (Intake through Integration plus the
Spawning node), the LLM gateway, coordinator types, and the `PipelineExecutor`
step-function loop.

Nodes contain **no domain rules**. Their job is to sequence calls between:

- **Pure business logic** in the `pipeline` crate (classification, alignment,
  traceability, budget enforcement, review aggregation, edge evaluation).
- **Infrastructure traits** also defined in `pipeline` but implemented by
  `github`, `llm`, and `extension-api`.

---

## Dependencies

| This crate uses | From |
|-----------------|------|
| All domain types, traits, and pure functions | `pipeline` |
| Infrastructure implementations | Injected via `Arc<dyn Trait>` — never imported directly |

---

## Module Layout

| Module | Contents |
|--------|----------|
| `gateway` | `ConstitutionallyWrappedPrompt`, `RateLimitState`, `LlmGateway`, `assemble_constitutional_prompt` |
| `node` | `NodeInput`, `NodeFailure`, `NodeExecutionResult`; coordinator types; step config types |
| `intake` | `IntakeNode` |
| `architecture` | `ArchitectureNode` |
| `interface_design` | `InterfaceDesignNode` |
| `planning` | `PlanningNode` |
| `code_generation` | `CodeGenerationNode` |
| `review` | `ReviewNode` |
| `integration` | `IntegrationNode` |
| `spawning` | `SpawningNode` |
| `executor` | `PipelineExecutor`, `StepResult`, `run_step` |
| `templates` | `HandlebarsTemplateEngine` (stub added in PR 10) |

---

## Part 1 — LLM Gateway (`gateway.rs`)

All LLM calls in nodes **must** go through `LlmGateway::call`. The gateway
enforces constitutional prompt position and manages shared rate-limit state.

### ConstitutionallyWrappedPrompt

Opaque type produced only by [`assemble_constitutional_prompt`]. Inner fields
are private; the type cannot be cloned or constructed in any other way.

Enforces at compile time that every LLM call passes through constitutional
rule assembly. The token is consumed by `LlmGateway::call` exactly once.

### RateLimitState

Runtime tracking of LLM provider rate limits. Shared across all concurrent
nodes via `Arc<Mutex<RateLimitState>>`.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `remaining_requests` | `u32` | Requests remaining before the rate-limit window resets |
| `window_reset` | `Option<std::time::Instant>` | When the window resets; `None` until a rate-limit response is received |
| `backoff_active` | `bool` | `true` when exponential backoff is active (429 received) |

### LlmGateway

Holds the LLM provider implementation and a shared `Arc<Mutex<RateLimitState>>`
so parallel nodes share the rate-limit window.

**Fields** (private):

| Field | Type |
|-------|------|
| `provider` | `Arc<dyn LlmProvider>` |
| `rate_limit` | `Arc<Mutex<RateLimitState>>` |

#### `new`

```rust
pub fn new(provider: Arc<dyn LlmProvider>) -> Self
```

Initialises the gateway with `RateLimitState::default()`.

#### `rate_limit_handle`

```rust
pub fn rate_limit_handle(&self) -> Arc<Mutex<RateLimitState>>
```

Returns a clone of the shared rate-limit state handle so `PipelineExecutor`
can share the same handle across all nodes.

### `fn assemble_constitutional_prompt`

```rust
pub fn assemble_constitutional_prompt(
    rules: &ConstitutionalRules,
    system_prompt: &str,
) -> Result<ConstitutionallyWrappedPrompt, ConstitutionalError>
```

The return type is `Result`, not a bare `ConstitutionallyWrappedPrompt`: the
function must propagate `ConstitutionalError` from `validate_constitutional_prompt`
without panicking (no `unwrap()`/`expect()` in production code), so a fallible
internal call requires a fallible return type. `ApprovedBranches::default()` is
used internally; there is no caller-supplied `ApprovedBranches` parameter.

**Purpose**: Embeds constitutional rules at the privileged leading position of
the system prompt and combines them with the node-specific system prompt text.

**Composition with `validate_constitutional_prompt`**:

`assemble_constitutional_prompt` calls `validate_constitutional_prompt` internally
as its first step. The call chain is:

```text
assemble_constitutional_prompt(rules, system_prompt)
  └─ validate_constitutional_prompt(rules, PromptAssembly { system_prompt, user_content: "" })
       ├─ Verifies source_branch is on approved list
       ├─ Verifies SHA-256 hash of rules.content matches source_hash
       ├─ Verifies every RequiredRule signature is present in rules.content
       └─ Returns ValidatedPrompt (or Err(ConstitutionalError))
  └─ Wraps ValidatedPrompt into ConstitutionallyWrappedPrompt
```

`ConstitutionallyWrappedPrompt` is therefore a strict superset of `ValidatedPrompt`:
it carries all the same invariants (rules present, branch valid, hash verified) plus
the `nodes`-layer invariant that the assembled prompt is ready for LLM dispatch.

Callers in the `nodes` crate never call `validate_constitutional_prompt` directly;
they call `assemble_constitutional_prompt` which performs the validation internally.
Code in the `pipeline` crate may call `validate_constitutional_prompt` directly for
unit-testable validation without going through the full assembly.

`run_step` step 1 does not call `assemble_constitutional_prompt` directly.  It calls
`validate_constitutional_prompt` to verify the rules file is intact — the full
assembly happens inside each node's `execute` function when it calls `LlmGateway`.

**Behaviour**:

1. Calls `validate_constitutional_prompt(rules, PromptAssembly { system_prompt, user_content: "" }, &ApprovedBranches::default())`.
   If this returns `Err(ConstitutionalError)`, propagates the error immediately as
   `Err(ConstitutionalError)`. Mapping to `PipelineError::ConstitutionalRulesLoadFailed`
   (if desired) is the caller's responsibility at the `run_step`/node-execute layer —
   `assemble_constitutional_prompt` itself returns the raw `ConstitutionalError`.
2. On success, constructs a `ConstitutionallyWrappedPrompt` (private fields) directly
   from `rules.content` and `system_prompt` — the validation outcome is checked but
   the already-joined `ValidatedPrompt::assembled_system_prompt()` is not reused,
   since `ConstitutionallyWrappedPrompt` keeps the two parts as separate fields.
3. Returns `Ok(ConstitutionallyWrappedPrompt)`.

**Contract**: `run_step` calls this only after Step 1 (constitutional rules
load and validation) has succeeded. No constitutional rule text ever appears
in user-role messages.

### `LlmGateway::call`

```rust
pub async fn call(
    &self,
    prompt: ConstitutionallyWrappedPrompt,
    context: &ContextPackage,
    schema: &OutputSchema,
    model: &ModelConfig,
) -> Result<StructuredResponse, LlmError>
```

**Purpose**: Submits the constitutional prompt and assembled context to the
LLM provider and returns a validated structured completion.

**Behaviour**:

1. Acquires the rate-limit lock. If `backoff_active` and now < `window_reset`,
   returns `Err(LlmError::RateLimited { retry_after })`.
2. Releases the lock.
3. Decomposes `prompt` into a system message (constitutional content +
   node system prompt).
4. Converts `ContextPackage::items` into a user-role message sequence.
5. Delegates to `self.provider.complete(system, messages, schema, model)`.
6. On success: acquires the lock again; updates `remaining_requests` and
   `window_reset` from response metadata in `StructuredResponse`; releases.
7. On 429: acquires lock; sets `backoff_active = true`; updates `window_reset`.
8. Returns the result.

**Thread safety**: The rate-limit lock is held only during the check and
update phases, never during the HTTP call itself.

---

## Part 2 — Common Node Types (`node.rs`)

### NodeInput

The inputs delivered to every node's `execute` function.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `artifacts` | `HashMap<String, serde_json::Value>` | Named input artifact slots from the graph's `declared_inputs` |
| `pipeline_state` | `PipelineState` | Snapshot of the pipeline state at the start of this execution |
| `loaded_context_packs` | `LoadedContextPacks` | Context packs matched and merged for this node invocation |

### NodeFailure

A node execution that did not complete successfully.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `error` | `PipelineError` | The error that caused the node to fail |
| `retry_policy` | `RetryPolicy` | Whether the failure is retryable and after what delay |

### NodeExecutionResult

```rust
pub enum NodeExecutionResult {
    Success(NodeOutput),
    Failure(NodeFailure),
}
```

---

## Part 3 — Step Configuration Types (`node.rs`)

### PipelineConfig

Resolved per-run configuration obtained from `PipelineConfigurationLoader`
before any node executes.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `graph` | `PipelineGraph` | The pipeline graph for this run |
| `run_id` | `PipelineRunId` | The run identifier for the current step |

### CliConfig

CLI-level launch configuration. Set once at startup and held unchanged for the
lifetime of a service-mode run.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `working_dir` | `PathBuf` | Working directory for this pipeline run |
| `pipeline_name` | `Option<PipelineName>` | Named pipeline to use; default pipeline if `None` |
| `approved_branch` | `BranchName` | The branch approved for loading constitutional rules |
| `repository` | `RepositoryId` | The repository this pipeline run operates against; passed to `CodeRepository::read_file` for the constitutional-rules load in `run_step` step 1 |

---

## Part 4 — Coordinator Types (`node.rs`)

Coordinator types combine multiple infrastructure traits into focused helpers
that individual node `execute` functions accept as parameters. They have no
domain logic of their own.

### ContextAssembler

Provides assembled `ContextPackage` values for LLM node invocations. Delegates
to `SummaryCache` and the pure `assemble_context` function.

**Fields** (private):

| Field | Type |
|-------|------|
| `summary_cache` | `Arc<dyn SummaryCache>` |

**Constructor**:

```rust
pub fn new(summary_cache: Arc<dyn SummaryCache>) -> Self
```

### ConstraintValidator

Loads interface definitions and validates cross-domain constraints via
`validate_cross_domain_constraints`.

**Fields** (private):

| Field | Type |
|-------|------|
| `registry_loader` | `Arc<dyn InterfaceRegistryLoader>` |

**Constructor**:

```rust
pub fn new(registry_loader: Arc<dyn InterfaceRegistryLoader>) -> Self
```

### ContextPackLoader

Reads context pack TOML files from the repository via `CodeRepository`.

**Fields** (private):

| Field | Type |
|-------|------|
| `repository` | `Arc<dyn CodeRepository>` |

**Constructor**:

```rust
pub fn new(repository: Arc<dyn CodeRepository>) -> Self
```

### BudgetTracker

Wraps the shared cost accumulator with serial budget-check semantics for
parallel-node safety.

**Fields** (private):

| Field | Type |
|-------|------|
| `accumulated` | `Arc<Mutex<TokenCost>>` — running total of tokens spent so far |
| `limit` | `CostBudget` — maximum spend allowed for the pipeline run |

**Constructor**:

```rust
pub fn new(limit: CostBudget) -> Self
```

**Method**:

```rust
pub fn try_acquire(&self, estimated: &TokenCost) -> BudgetAcquisition
```

Locks `accumulated`, calls `acquire_budget`, and — if approved — updates
`accumulated` before releasing the lock. This satisfies the atomicity contract
in `docs/spec/interfaces/pipeline-execution.md §Budget Enforcement`.

**Derives**: `Clone` (via `Arc` clone; all clones share the same accumulator).

### WorkingCopyManager

Manages file reads and branch operations on the repository working copy.

**Fields** (private):

| Field | Type |
|-------|------|
| `repository` | `Arc<dyn CodeRepository>` |
| `working_branch` | `BranchName` |

**Constructor**:

```rust
pub fn new(repository: Arc<dyn CodeRepository>, working_branch: BranchName) -> Self
```

---

## Part 5 — Node Execute Signatures

All `execute` methods are `async`. All return `NodeExecutionResult`. The input
`NodeInput` is accepted by value (moved); all other parameters are borrowed.

### IntakeNode (`intake.rs`)

```rust
pub async fn execute(
    input: NodeInput,
    gateway: &LlmGateway,
    issues: &dyn IssueTracker,
    config: &PipelineConfig,
) -> NodeExecutionResult
```

**Responsibilities**:

1. Fetch issue details via `issues.get_issue(work_item_id)`.
2. Detect injection in the issue body via `detect_injection`.
3. Classify the work item via `LlmGateway::call` to produce `ClassificationResult`.
4. Apply safety override via `apply_safety_override`.
5. Check scope threshold via `check_scope_threshold`.
6. Extract requirements via `extract_requirements` and initialise `TraceabilityMatrix`.
7. Write `ClassificationResult` and empty `TraceabilityMatrix` to output artifacts.

### ArchitectureNode (`architecture.rs`)

```rust
pub async fn execute(
    input: NodeInput,
    gateway: &LlmGateway,
    context: &ContextAssembler,
    constraint_validator: &ConstraintValidator,
    pack_loader: &ContextPackLoader,
    github: &dyn CodeRepository,
) -> NodeExecutionResult
```

**Responsibilities**:

1. Load context packs for the `ClassificationResult` from input.
2. Validate cross-domain constraints via `constraint_validator`.
3. Assemble context via `context`.
4. Call LLM to produce the architecture document.
5. Run deterministic alignment (`run_deterministic_alignment`).
6. If `LlmSemantic` checks are enabled: run LLM semantic alignment;
   update traceability matrix via `update_traceability_matrix` with
   `AlignmentResult::traceability_entries`.
7. Write architecture document to output artifacts.

### InterfaceDesignNode (`interface_design.rs`)

```rust
pub async fn execute(
    input: NodeInput,
    gateway: &LlmGateway,
    context: &ContextAssembler,
    domain_svc: &dyn DomainServiceClient,
    github: &dyn CodeRepository,
) -> NodeExecutionResult
```

**Responsibilities**:

1. Assemble context including current interface definitions.
2. Call LLM to generate interface stubs.
3. Validate stubs via `domain_svc.validate()`.
4. Run alignment; update traceability (LLM-semantic path only).
5. Write interface stub artifacts to output.

### PlanningNode (`planning.rs`)

```rust
pub async fn execute(
    input: NodeInput,
    gateway: &LlmGateway,
    issues: &dyn IssueTracker,
    config: &PipelineConfig,
) -> NodeExecutionResult
```

**Responsibilities**:

1. Decompose the architecture document into sub-work-items via LLM call.
2. Create GitHub sub-issues via `issues.create_sub_issue`.
3. Topological sort sub-work-items via `topological_sort_sub_work_items`.
4. Write ordered `Vec<SubWorkItem>` to output artifacts.

### CodeGenerationNode (`code_generation.rs`)

```rust
pub async fn execute(
    input: NodeInput,
    gateway: &LlmGateway,
    context: &ContextAssembler,
    domain_svc: &dyn DomainServiceClient,
    budget: &BudgetTracker,
) -> NodeExecutionResult
```

**Responsibilities**:

1. Enforce scenario holdout before context assembly.
2. For each generation step: budget-check via `budget.try_acquire()` before LLM call.
3. Call LLM to generate code for the current sub-work-item.
4. Validate via `domain_svc.validate()`.
5. Write generated artifacts to output.

### ReviewNode (`review.rs`)

```rust
pub async fn execute(
    input: NodeInput,
    gateway: &LlmGateway,
    constraint_validator: &ConstraintValidator,
    packs: &LoadedContextPacks,
    audit: &dyn AuditStore,
) -> NodeExecutionResult
```

**Responsibilities**:

1. Run Quality, Architecture, and Security review passes (each as a separate
   `LlmGateway::call` with a review-specific schema).
2. Aggregate via `aggregate_review_results`.
3. Record findings via `audit.record_event(AuditEvent::ValidationResult { ... })`.
4. Write `AggregateReviewDecision` to output artifacts.

### IntegrationNode (`integration.rs`)

```rust
pub async fn execute(
    input: NodeInput,
    pr_manager: &dyn PullRequestManager,
    working_copy: &WorkingCopyManager,
) -> NodeExecutionResult
```

**Responsibilities**:

1. Create or update pull request via `pr_manager.create_pull_request`.
2. Post review findings as PR review comments.
3. Apply safety-gating labels if `ClassificationResult::safety_affecting`.
4. Write `PullRequestId` to output artifacts.

### SpawningNode (`spawning.rs`)

```rust
pub async fn execute(
    input: NodeInput,
    gateway: &LlmGateway,
    issues: &dyn IssueTracker,
) -> NodeExecutionResult
```

**Responsibilities**:

1. Call LLM to identify sub-tasks to spawn as separate work items.
2. Create GitHub sub-issues via `issues.create_sub_issue`.
3. Emit `NodeStateUpdate` entries in `NodeOutput::state_updates` marking
   newly spawned child nodes as `Active`.
4. Write spawned `Vec<WorkItemId>` to output artifacts.

---

## Part 6 — Pipeline Executor (`executor.rs`)

### StepResult

The outcome of one call to `run_step`.

```rust
pub enum StepResult {
    /// A single node completed successfully in this step.
    Completed { node_id: NodeId, output: NodeOutput },
    /// Multiple nodes completed concurrently (parallel branch).
    ///
    /// Emitted when an `ExecuteParallel` action fires multiple nodes.
    /// The caller must apply **all** results to the pipeline state before
    /// evaluating outgoing edges. `results` is in input order (the order
    /// nodes were submitted to `ExecuteParallel`).
    CompletedParallel { results: Vec<(NodeId, NodeOutput)> },
    /// A `HumanGated` node is awaiting approval.
    Gated { node_id: NodeId, gate_reason: String },
    /// The pipeline cannot continue without human intervention.
    Escalated(EscalationReason),
    /// The pipeline encountered an unrecoverable error.
    Halted(PipelineError),
}
```

### PipelineExecutor

Holds all infrastructure dependencies injected at startup. Constructed once in
`cli/src/main.rs` and passed to every `run_step` call.

**Fields** (private):

| Field | Type |
|-------|------|
| `issues` | `Arc<dyn IssueTracker>` |
| `pull_requests` | `Arc<dyn PullRequestManager>` |
| `code_repository` | `Arc<dyn CodeRepository>` |
| `domain_service` | `Arc<dyn DomainServiceClient>` |
| `llm_gateway` | `Arc<LlmGateway>` |
| `audit` | `Arc<dyn AuditStore>` |
| `summary_cache` | `Arc<dyn SummaryCache>` |
| `interface_registry` | `Arc<dyn InterfaceRegistryLoader>` |
| `config_loader` | `Arc<dyn PipelineConfigurationLoader>` |
| `tool_profile_store` | `Arc<dyn ToolProfileStore>` |

### `fn run_step`

```rust
pub async fn run_step(
    executor: &PipelineExecutor,
    work_item_id: WorkItemId,
    config: &CliConfig,
) -> StepResult
```

**Step sequence**:

1. **Load constitutional rules** — read from `config.approved_branch` via
   `executor.code_repository`. Hash verification or branch check failure →
   `StepResult::Halted(PipelineError::ConstitutionalRulesLoadFailed { ... })`.
   This is an unconditional halt; no other step executes without valid rules.

2. **Load pipeline config** — call
   `executor.config_loader.get_named_pipeline(name)` or `get_default_pipeline()`.
   Validate the graph; validation failure → `StepResult::Halted(PipelineError::GraphInvalid { ... })`.

3. **Reconstruct pipeline state** — call `executor.issues.list_comments(work_item_id)`
   and scan the returned list (newest first) for a comment that is authentic and
   current:
   - **Author check** (THREAT-022): `comment.author_id` must equal
     `executor.issues.authenticated_bot_user_id()` — the comment's numeric GitHub
     user ID, not the spoofable `author` login string. A comment from any other
     account is skipped, regardless of content.
   - **Parse check**: the body must be valid JSON that deserialises as a
     `PipelineStateComment` (the `schema_version` field must be present).
   - **Self-consistency check** (THREAT-022, defense in depth): the deserialised
     `PipelineStateComment.work_item_id` must equal the `work_item_id` being
     processed.

   The first comment (scanning newest first) that passes all three checks is the
   latest state snapshot; deserialise both `state` and `gate_config` from it. If no
   comment passes all three checks, initialise a fresh `PipelineState` and an empty
   `GateConfig` (new run). See `docs/spec/security.md` §THREAT-022 for the full
   threat model.

4. **Determine next actions** — call
   `determine_next_actions(&state, &graph, &gate_config)`.

5. **Dispatch actions**:
   - Empty vec → run is complete; write final state comment → return
     `StepResult::Completed`.
   - `NextAction::Wait` → return `StepResult::Gated`.
   - `NextAction::Escalate(reason)` → write comment to issue →
     `StepResult::Escalated(reason)`.
   - `NextAction::HaltWithError(err)` → write comment → `StepResult::Halted(err)`.
   - `NextAction::ExecuteNode(id)` / `ExecuteParallel(ids)` → proceed to step 6.

6. **Execute nodes** — for each eligible node:
   - Check gate; `HumanGated` node pending approval → return `StepResult::Gated`.
   - Budget-check via `BudgetTracker::try_acquire` (mutex held across all parallel
     checks per the atomicity contract in `docs/spec/interfaces/pipeline-execution.md`).
   - Dispatch to the appropriate node `execute` function.
   - On `NodeExecutionResult::Failure` → apply retry/rework policy; on
     `NonRetryable` escalate or halt as appropriate.
   - After a node completes successfully: for every outgoing edge with an
     `LlmEvaluated` condition, call `LlmGateway::call` (async) to resolve the
     natural-language condition to a `bool`. Collect results into a
     `HashMap<EdgeId, bool>`. Pass this map as `llm_evaluated_results` to
     `evaluate_edge_condition` for each outgoing edge. See
     `docs/spec/interfaces/pipeline-execution.md §evaluate_edge_condition`.

7. **Persist state** — write updated `PipelineStateComment` to the issue via
   `executor.issues.post_comment`.

8. Return `StepResult::Completed { node_id, output }` for a single completed node,
   or `StepResult::CompletedParallel { results }` when an `ExecuteParallel` action
   fired multiple nodes. For `CompletedParallel`, `results` contains one entry per
   node in the parallel branch in input order (the order they were submitted to
   `ExecuteParallel`).

---

## Traceability Update Rule (from Pre-Implementation Decision)

`update_traceability_matrix` takes `&[TraceabilityEntry]` from
`AlignmentResult::traceability_entries`. This field is **only populated by the
`LlmSemantic` alignment check path**.

Node code must follow this pattern:

```rust
// Always: feed findings to review/escalation
let result = run_deterministic_alignment(&inputs, &output);

// Only if LlmSemantic was enabled and ran:
if llm_semantic_ran {
    let llm_result = /* LLM semantic alignment result */;
    matrix = update_traceability_matrix(matrix, stage, &llm_result.traceability_entries);
}
```

If only deterministic checks ran, pass an empty slice (or do not call
`update_traceability_matrix`). The stage coverage flags remain unchanged,
recording an honest *uncovered* status rather than fabricating coverage.

See `docs/spec/interfaces/advanced-features.md` §update_traceability_matrix for
the full contract.

---

## Implementation Constraints

- Constitutional rules are validated before **any** node runs in a step; no node
  function is called if Step 1 fails.
- Budget checks for parallel nodes must be serialised via `BudgetTracker`'s
  internal mutex — never call `acquire_budget` from two threads concurrently
  without coordination.
- Scenario files must never appear in a code-generation context (enforced by
  `enforce_scenario_holdout` in `CodeGenerationNode::execute`).
- All `todo!()` bodies in this PR's stubs reference this document.

---

## Reviewer Checklist

- Constitutional rules load before any other action in `run_step`.
- All node signatures correctly reference their required traits.
- `BudgetTracker::try_acquire` holds the mutex for the full acquire + update cycle.
- `update_traceability_matrix` is never called with `&[AlignmentFinding]` — only
  with `&[TraceabilityEntry]`.
- `ConstitutionallyWrappedPrompt` fields are private (cannot be forged).
- No domain logic in any node stub — all bodies are `todo!()`.
