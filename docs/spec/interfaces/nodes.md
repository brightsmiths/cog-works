# Pipeline Nodes — Interface Specification

**Architectural Layer**: Orchestration (sequences business logic and infrastructure calls; no domain rules)
**Module Paths**: `crates/nodes/src/`
**Specification Version**: 1.0

---

## Overview

The `nodes` crate contains the orchestration layer that drives each pipeline
step. It holds the stage drivers, the LLM gateway used for deterministic helper
calls, coordinator types, and the `PipelineExecutor` step-function loop.

Stage drivers contain **no domain rules** and **no agent personas**. A driver
sequences calls between:

- **Pure business logic** in the `pipeline` crate (classification, alignment,
  traceability, budget enforcement, verdict interpretation, rework policy, tier
  derivation, edge evaluation).
- **Infrastructure traits** also defined in `pipeline` but implemented by
  `github`, `llm`, `harness`, `runner` and `extension-api`.

What a driver does **not** do is run the agent. An agent turn happens in a fresh
sandbox owned by the stage runner service. A driver resolves the profile, builds
the bundle, obtains the mount plan, requests the attempt, observes completion
through GitHub state, and reads the gate verdict. The prompt the agent receives
is assembled inside the sandbox from the harness-resolved profile, not here.

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
| `gateway` | `ConstitutionallyWrappedPrompt`, `LlmGateway`, `assemble_constitutional_prompt` |
| `node` | `NodeInput`, `NodeFailure`, `NodeExecutionResult`; coordinator types; step config types |
| `intake` | The intake step |
| `stages` | One module per stage type, each driving an attempt for that stage type |
| `executor` | `PipelineExecutor`, `StepResult`, `run_step` |
| `templates` | `HandlebarsTemplateEngine`, used for deterministic helper prompts only |

The `stages` modules are named by stage type: `specify`, `contract`, `plan`,
`unit_oracle`, `implement`, `refactor`, `acceptance_oracle`, `architect`,
`review_semantic`, `review_security`. Each is a driver, not an agent: it carries
the sequencing for one stage type and no prompt text.

**No stage-type name is compiled into dispatch.** The set of stage types is data,
resolved from the pinned harness's stage-type registry. A module exists for a
stage type CogWorks knows how to *drive*; whether a given stage type is available
to a run is decided by the harness, not by this list.

---

## Part 1 — LLM Gateway (`gateway.rs`)

The gateway carries **CogWorks' own deterministic helper calls** — alignment
verification, classification, failure interpretation — and nothing else.

Agent turns do not pass through it. An agent turn happens inside a sandbox, where
the resolved profile assembles the prompt and the sandbox boundary preserves the
constitutional rules' position. CogWorks is not the process making that call and
does not see that prompt.

Every helper call CogWorks makes itself **must** go through `LlmGateway::call`.

**Credentials**: none. The gateway targets the deployment's model gateway
endpoint, which holds the provider API keys. Neither this process nor any sandbox
holds a model API key.

**Rate limiting**: not here. The model gateway holds the credentials and sees all
factory traffic, so it is the only component positioned to enforce provider rate
limits. A gateway response indicating throttling surfaces as a retriable
condition.

### ConstitutionallyWrappedPrompt

Opaque type produced only by [`assemble_constitutional_prompt`]. Inner fields
are private; the type cannot be cloned or constructed in any other way.

Enforces **at compile time** that every helper call made in this process passes
through constitutional rule assembly. The token is consumed by
`LlmGateway::call` exactly once.

The scope of this guarantee is worth stating precisely, because it is narrower
than it was when CogWorks made every model call itself. It covers the
deterministic helper calls in this process. It cannot cover an agent turn, since
CogWorks does not assemble that prompt. For agent turns the equivalent assurance
comes from the pinned profile, sandbox isolation, the mount plan the attempt runs
under, and the gate service's independent verdict. See
`docs/adr/ADR-0011-prompt-assets-load-from-the-pinned-harness.md`.

### LlmGateway

Holds the model-gateway-backed provider implementation.

**Fields** (private):

| Field | Type |
|-------|------|
| `provider` | `Arc<dyn LlmProvider>` |

#### `new`

```rust
pub fn new(provider: Arc<dyn LlmProvider>) -> Self
```

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
3. Scans every `ContextPackage::items[i].content` via `pipeline::detect_injection`
   (label: the item's `source_path` if present, else `"context item"`). On the
   first `InjectionDetectionResult::InjectionDetected`, returns
   `Err(LlmError::InjectionDetected { source, offending_text, pattern })`
   immediately — the provider is never called (Security Reviewer finding H2,
   task #59 follow-up).
4. Decomposes `prompt` into a system message (constitutional content +
   node system prompt).
5. Converts `ContextPackage::items` into a user-role message sequence.
6. Delegates to `self.provider.complete(system, messages, schema, model)`.
7. On success: acquires the lock again; updates `remaining_requests` and
   `window_reset` from response metadata in `StructuredResponse`; releases.
8. On 429: acquires lock; sets `backoff_active = true`; updates `window_reset`.
9. Returns the result.

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
| `stage_type` | `StageTypeId` | The stage type this node declares, validated against the harness registry at graph load |
| `profile` | `ProfileRef` | The agent profile resolved for this stage, recorded in the attestation |
| `stage_contract` | `StageContract` | The resolved contract: input allowlist, output schema, writable paths, network profile, budget, attempt and rework limits, identity role |
| `context_bundle` | `ContextBundle` | The allowlisted, hashed inputs this attempt may see |
| `loaded_context_packs` | `LoadedContextPacks` | Harness-supplied packs matched for this stage, already folded into the bundle and covered by its hash |

`stage_contract` is the driver's own input. It is **never** placed in
`context_bundle`, so it never reaches the agent the stage runs. An agent that can
read its own allowlist, limits and checked paths can reason about them; the
constraints are for the machine.

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

Resolved per-run configuration, obtained before any stage executes. Everything
that shapes an agent in it comes from the harness at the pinned version.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `harness_version` | `HarnessVersion` | The harness version resolved from the target repository's pin. Stable for the whole run, so harness movement cannot change behaviour mid-run |
| `graph` | `PipelineGraph` | The stage graph resolved from the harness for this run's `(tier, discipline)` pair |
| `graph_hash` | `String` | Hash of the resolved graph, recorded in state so drift is detected on resume |
| `stage_registry` | `StageTypeRegistry` | The stage types this harness version defines; every graph node's stage type is validated against it at load |
| `run_id` | `PipelineRunId` | The run identifier for the current step |
| `tier` | `Tier` | The tier derived from `risk-map.yaml`, raise-only against any previously recorded tier |
| `discipline` | `DisciplineId` | The discipline pack selected for this work |
| `risk_map` | `RiskMap` | Path globs to tiers, read from the target repository and owned by humans |
| `scope_threshold` | `u32` | Maximum `estimated_scope` (1-10) a single work item may have before `check_scope_threshold` rejects it for splitting |
| `constitutional_rules` | `ConstitutionalRules` | Rules resolved from the pinned harness and verified against the pin, wrapped around every helper call via `assemble_constitutional_prompt` |

There is no field for a repository-supplied graph, named pipeline, prompt
template set, output schema set or tool profile. None of those exist as
repository-local configuration: they resolve from the harness.

### CliConfig

CLI-level launch configuration. Set once at startup and held unchanged for the
lifetime of a service-mode run.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `repository` | `RepositoryId` | The repository this run operates against |
| `pin_override` | `Option<HarnessVersion>` | Operate against a harness version other than the one the repository pins. For controlled rollout and diagnosis; the resolved version is still recorded in every attestation |

This struct is deliberately small. A working directory is not here because stage
attempts run in sandboxes CogWorks does not own. A pipeline name is not here
because graphs resolve per tier and discipline from the harness. An approved
branch for constitutional rules is not here because rules resolve from the pin,
not from a branch of the repository being worked on — a repository cannot supply
the rules that govern its own runs.

---

## Part 4 — Coordinator Types (`node.rs`)

Coordinator types combine multiple infrastructure traits into focused helpers
that stage drivers accept as parameters. They have no domain logic of their own.

### ContextBundleBuilder

Builds the `ContextBundle` a stage attempt is permitted to see, from the input
allowlist in its resolved stage contract. Delegates to the pure
`assemble_bundle` function.

**Fields** (private):

| Field | Type |
|-------|------|
| `harness` | `Arc<dyn HarnessLoader>` |
| `code_repository` | `Arc<dyn CodeRepository>` |
| `domain_service` | `Arc<dyn DomainServiceClient>` |
| `interface_registry` | `Arc<dyn InterfaceRegistryLoader>` |

**Constructor**:

```rust
pub fn new(
    harness: Arc<dyn HarnessLoader>,
    code_repository: Arc<dyn CodeRepository>,
    domain_service: Arc<dyn DomainServiceClient>,
    interface_registry: Arc<dyn InterfaceRegistryLoader>,
) -> Self
```

**Contract**: an input the allowlist does not name is **absent** from the
resulting bundle — not ranked lower, not truncated away. The builder hashes the
assembled bundle and records the hash before the attempt is requested. It never
places the stage's own contract in the bundle. Where the allowlist's mandatory
inputs alone exceed the token budget, the builder reports a structured error
rather than truncating one away.

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

Its results bound a stage attempt. They are never published as a check run and
never constitute a gate verdict.

### ContextPackLoader

Reads Context Packs from the pinned harness via `HarnessLoader`, matching their
triggers against the stage's tier, discipline, stage type and detected toolchain.

**Fields** (private):

| Field | Type |
|-------|------|
| `harness` | `Arc<dyn HarnessLoader>` |

**Constructor**:

```rust
pub fn new(harness: Arc<dyn HarnessLoader>) -> Self
```

Packs are read from the harness, never from the target repository. A pack is
prompt material, and a repository-local pack would be prompt content an agent
working in that repository could edit.

Trigger matching is deterministic and takes no model input.

### BudgetTracker

Wraps the shared cost accumulator with serial budget-check semantics for
parallel-stage safety.

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

Budget tracking stays with CogWorks. The model gateway cannot enforce a per-run
budget, because it does not know which run a call belongs to.

---

## Part 5 — Stage Driver Signatures

All `execute` methods are `async`. All return `NodeExecutionResult`. The input
`NodeInput` is accepted by value (moved); all other parameters are borrowed.

### The common shape

Every agent-backed stage driver has the same shape, because every stage does the
same five things with different data:

```rust
pub async fn execute(
    input: NodeInput,
    runner: &dyn StageRunner,
    mounts: &dyn MountPlanProvider,
    verdicts: &dyn CheckRunReader,
    issues: &dyn IssueTracker,
    config: &PipelineConfig,
) -> NodeExecutionResult
```

**Responsibilities**, in order, for every agent-backed stage:

1. Obtain a mount plan from `mounts.mount_plan(...)` for the identity role and
   writable paths in `input.stage_contract`. A plan that cannot be produced, or
   that would be narrower than the contract requires, fails the stage.
2. Request one stage attempt via `runner.start_attempt(...)`, carrying the image,
   `input.context_bundle`, the mount plan, the network profile, the resource
   limits and the identity role. **One fresh sandbox per attempt**; a retry is a
   new attempt and therefore a new request.
3. Observe completion through GitHub state: the expected branch carries the
   attempt's commits and its output record is present. A queue event may prompt an
   earlier read but is never the authority.
4. Read the verdict via `verdicts.check_runs_for(repository, sha)`. Check runs not
   authored by the gate service identity are discarded at the adapter boundary. A
   pending or absent verdict returns a waiting result — never an assumed pass.
5. Validate the attempt's output against the output schema the harness declares
   for this stage, then run the stage's deterministic pre/post checks. These bound
   the attempt. They are **never** published as a check run and never constitute a
   gate verdict.

What no driver does: assemble the agent's prompt, call a model on the agent's
behalf, push a commit, or author a check run.

Per-stage drivers differ only in which artifacts they read from
`input.artifacts`, which they write to the output, and which deterministic checks
apply. They hold no prompt text and no persona.

### Intake step (`intake.rs`)

The intake step is deterministic and runs no agent, so it does not take a stage
runner.

```rust
pub async fn execute(
    input: NodeInput,
    gateway: &LlmGateway,
    issues: &dyn IssueTracker,
    risk_map: &dyn RiskMapLoader,
    attestations: &dyn AttestationProvider,
    config: &PipelineConfig,
) -> NodeExecutionResult
```

**Responsibilities**:

1. Fetch issue details via `issues.get_issue(work_item_id)`. `work_item_id` is
   read from `input.artifacts["work_item_id"]` (a JSON integer); missing or
   malformed values fail the step rather than defaulting silently.
2. Verify every upstream attestation the configured trust policy requires, via
   `attestations`. A required attestation that is missing, unsigned, signed by an
   unexpected identity, or mismatched against the content it describes **fails
   closed**.
3. Read the upstream machine-readable triage and affected-areas blocks. Prose is
   never parsed to extract a decision, an area, a priority or a type.
4. Derive the tier from `risk_map` over the affected paths: the maximum over those
   paths, raise-only against any previously recorded tier. An absent or malformed
   risk map halts rather than defaulting to the lowest tier.
5. Check the scope threshold via `check_scope_threshold`; exceeding it escalates
   rather than proceeding.
6. Extract requirements via `extract_requirements` and initialise
   `TraceabilityMatrix`.
7. Write the derived tier, the verified attestation record and the
   `TraceabilityMatrix` (one row per extracted requirement, each
   `TraceabilityStatus::Missing`) to output artifacts.

**Injection screening is not here.** Screening an incoming work item belongs to
the upstream intake service, which signs its approval; step 2 verifies that
signature rather than repeating the scan. Sanitising *model-sourced* text before
reusing it in a later prompt remains CogWorks' own concern and happens where that
reuse occurs, not here.

### Oracle stages (`unit_oracle.rs`, `acceptance_oracle.rs`)

Follow the common shape, under the oracle identity role, writing the locked-test
paths and producing the signed manifest that seals them.

Additional contract:

- The bundle carries the slice, the contracts and the specification. It carries
  **no implementation source**, so the tests describe what was asked for rather
  than what was built.
- The stage is not complete until the manifest is present and sealed. No
  `implement` attempt may be requested for the slice before it is.
- `acceptance_oracle` additionally produces the holdout set, which at the product
  tier lands in the acceptance repository rather than the implementation
  repository.

### Build stages (`implement.rs`, `refactor.rs`)

Follow the common shape, under the builder identity role, writing `src/` and the
stage's own tests.

Additional contract:

- The attempt may not start until the locked tests and sealed manifest exist for
  the slice.
- The mount plan makes the locked-test paths **read-only**. CogWorks is not the
  only enforcement point for this.
- The bundle carries the locked tests as read-only input and carries **no holdout
  content and no acceptance source**.
- Where the stage judges a locked test to be wrong, the driver records a
  **dispute** for a human. A locked test is never edited by the stage it
  constrains.
- Rework feedback carries the gate's structured output — failing test identities
  and messages — never test source. For a holdout, no more than the failing
  requirement identities.
- `refactor` additionally fails the attempt if the locked tests changed at all.

### Design stages (`specify.rs`, `contract.rs`, `plan.rs`, `architect.rs`)

Follow the common shape, under the design identity role, writing **specification
paths only**.

Additional contract:

- Each produces a specification pull request that a human approves by review. The
  gate service runs the specification gates on that pull request.
- `plan` produces `plan.yaml` carrying slices, sizes, requirement mapping and a
  dependency DAG. Sub-issues are created from an **approved** plan, in DAG order,
  deterministically — the order comes from the DAG, not from a model.
- `architect` produces the decision record, the threat-model delta and a proposed
  risk-map patch. The patch is a proposal a human merges; the driver never writes
  the risk map.
- Deterministic cross-domain constraint validation runs as a post-check on
  `architect` and `contract` output. It bounds the attempt and is not a verdict.

### Assurance stages (`review_semantic.rs`, `review_security.rs`)

Follow the common shape, with **no writable paths at all**.

Additional contract:

- The stage contract grants no writable path, so the stage cannot modify the
  change it is reviewing.
- The only output is a structured set of findings, each carrying a category, a
  severity, a location and its reasoning.
- Severity is a claim about seriousness, not an instruction. Whether a finding
  blocks is decided by the gate service from the findings and the tier's
  thresholds.
- The driver does **not** aggregate findings into a proceed, remediate or
  escalate decision.
- Finding text is model output. It is scanned via `detect_injection` before being
  reused in any later prompt.

### Pull request handling (`integration.rs`)

```rust
pub async fn execute(
    input: NodeInput,
    pull_requests: &dyn PullRequestManager,
    issues: &dyn IssueTracker,
    config: &PipelineConfig,
) -> NodeExecutionResult
```

Opens or updates the pull request for the branch the stage runner pushed, posts
findings as non-blocking review comments, and links the pull request to its slice
and parent work item.

It does not push commits, author a check run, approve, or merge.

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
    /// A stage attempt is in flight, or its verdict has not yet been published.
    ///
    /// Returned rather than assuming an outcome. The next invocation re-reads
    /// GitHub state and the gate service's check runs.
    Waiting { node_id: NodeId, awaiting: WaitReason },
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
| `interface_registry` | `Arc<dyn InterfaceRegistryLoader>` |
| `harness` | `Arc<dyn HarnessLoader>` |
| `stage_runner` | `Arc<dyn StageRunner>` |
| `check_runs` | `Arc<dyn CheckRunReader>` |
| `mount_plans` | `Arc<dyn MountPlanProvider>` |
| `risk_map` | `Arc<dyn RiskMapLoader>` |
| `attestations` | `Arc<dyn AttestationProvider>` |

`config_loader`, `tool_profile_store` and `summary_cache` are absent. The first
has no subject — there is no repository-local pipeline configuration to load, and
graph resolution happens through `harness`. The second has no subject either: the
agent does not run in this process, so CogWorks cannot filter its tool list; the
allowlist is in the profile and the boundary is the sandbox. The third cached
multi-level module summaries, which the allowlist-driven bundle replaces.

`stage_runner` is the only field through which execution leaves this process, and
`check_runs` is the only field through which a verdict enters it.

### `fn run_step`

```rust
pub async fn run_step(
    executor: &PipelineExecutor,
    work_item_id: WorkItemId,
    config: &CliConfig,
) -> StepResult
```

**Step sequence**:

1. **Read the pin and resolve the harness** — read `.factory/pin.yaml` from
   `config.repository` via `executor.harness`, then resolve the harness at that
   version (or at `config.pin_override` when set) and verify the resolution. An
   absent, malformed or withdrawn pin, an unreachable harness, or a resolution
   that fails verification →
   `StepResult::Halted(PipelineError::HarnessUnresolvable { .. })`. No default
   and no fallback: everything that shapes an agent comes from here.

2. **Load constitutional rules** — resolve from the harness at the pinned version
   and verify against the pin. Failure →
   `StepResult::Halted(PipelineError::ConstitutionalRulesLoadFailed { .. })`.
   This is an unconditional halt; no further step executes without valid rules.

3. **Derive the tier** — read `risk-map.yaml` via `executor.risk_map` and compute
   the maximum tier over the affected paths, raise-only against any previously
   recorded tier. An absent or malformed risk map →
   `StepResult::Halted(PipelineError::RiskMapInvalid { .. })`, never a default to
   the lowest tier. The tier is never read from a label.

4. **Resolve and validate the stage graph** — obtain the graph for
   `(tier, discipline)` from the harness. Validate every node's stage type against
   the harness's stage-type registry, then validate the graph's structure. Either
   failure → `StepResult::Halted(PipelineError::GraphInvalid { .. })`. Record the
   graph hash.

5. **Reconstruct pipeline state** — call `executor.issues.list_comments(work_item_id)`
   and scan the returned list (newest first) for a comment that is authentic and
   current:
   - **Author check**: `comment.author_id` must equal
     `executor.issues.authenticated_bot_user_id()` — the comment's numeric GitHub
     user ID, not the spoofable `author` login string. A comment from any other
     account is skipped, regardless of content.
   - **Parse check**: the body must be valid JSON that deserialises as a
     `PipelineStateComment` (the `schema_version` field must be present).
   - **Self-consistency check**: the deserialised
     `PipelineStateComment.work_item_id` must equal the `work_item_id` being
     processed.

   The first comment (scanning newest first) that passes all three checks is the
   latest state snapshot; deserialise both `state` and `gate_config` from it. If no
   comment passes all three checks, initialise a fresh `PipelineState` and an empty
   `GateConfig` (new run). See `docs/spec/security.md` §THREAT-022.

6. **Check graph drift** — compare the recorded `graph_hash` against the hash from
   step 4. A mismatch means the pin moved, or the harness content at that pin did,
   between the invocation that wrote the state and this one →
   `StepResult::Escalated(EscalationReason::GraphDrift { .. })`. No stage executes.
   This is not a halt: a human decides whether to restart the run or move the pin
   back.

7. **Determine next actions** — call
   `determine_next_actions(&state, &graph, &gate_config)`.

8. **Dispatch actions**:
   - Empty vec → run is complete; write final state comment → return
     `StepResult::Completed`.
   - `NextAction::Wait` → return `StepResult::Gated`.
   - `NextAction::AwaitVerdict(id)` → read check runs for the recorded SHA via
     `executor.check_runs`; a pending or absent verdict returns
     `StepResult::Waiting`.
   - `NextAction::Escalate(reason)` → write comment to issue →
     `StepResult::Escalated(reason)`.
   - `NextAction::HaltWithError(err)` → write comment → `StepResult::Halted(err)`.
   - `NextAction::ExecuteNode(id)` / `ExecuteParallel(ids)` → proceed to step 9.

9. **Resolve the profile for each eligible stage** — resolve
   `(stage type, discipline, toolchain, tier)` to a profile and its stage contract
   via `executor.harness`. Resolution is deterministic and takes no model input. No
   matching profile → `StepResult::Halted(PipelineError::ProfileUnresolvable { .. })`,
   never a fallback to a default profile.

10. **Build and hash the context bundle** — assemble strictly from the stage
    contract's input allowlist. An input the allowlist does not name is absent.
    Enforce the trust-zone barrier for the stage's zone. Hash the bundle and record
    the hash. The stage contract is never placed in the bundle it governs. If the
    allowlist's mandatory inputs alone exceed the token budget →
    `StepResult::Escalated`, never a silent truncation.

11. **Obtain the mount plan** — request a plan from `executor.mount_plans` for the
    stage contract's identity role and writable paths.

12. **Execute stages** — for each eligible stage:
    - Check gate; `HumanGated` stage pending approval → return `StepResult::Gated`.
    - Budget-check via `BudgetTracker::try_acquire` (mutex held across all parallel
      checks per the atomicity contract in
      `docs/spec/interfaces/pipeline-execution.md`).
    - Request **one fresh sandbox** for this attempt via `executor.stage_runner`,
      carrying the image, bundle, mount plan, network profile, limits and identity
      role. No sandbox is reused across attempts.
    - Observe completion through GitHub state: the expected branch carries the
      attempt's commits and its output record is present.
    - Read the verdict for the resulting SHA via `executor.check_runs`. Only check
      runs authored by the gate service identity are considered. Pending or absent
      → `StepResult::Waiting`.
    - Validate the output against the harness-declared output schema, then run the
      stage's deterministic pre/post checks. These bound the attempt; they are
      never published as a check run.
    - On failure, apply the rework and escalation policy: a technically invalid
      output retries with the same context; a valid output that fails a gate or an
      alignment check reworks with the gate's structured feedback; a repeating
      failure class escalates early; reaching the contract's attempt or rework
      limit escalates with a dossier. See
      `docs/spec/interfaces/pipeline-execution.md`.
    - After a stage completes successfully, evaluate each outgoing edge's condition
      deterministically over pipeline state and verdicts. No edge condition is
      evaluated by a model: routing is code, so which edge is taken never depends
      on model output.

13. **Write the stage attestation** — assemble the attestation (stage type,
    resolved profile, harness version, bundle hash, identity role, mount plan,
    verdicts read and their authoring identity, attempt outcome), sign it via
    `executor.attestations`, and transport it as a hidden comment. Attestations are
    evidence, not state: a failure to write one is recorded but does not change the
    run's position.

14. **Persist state** — write the updated `PipelineStateComment` to the issue via
    `executor.issues.post_comment`, including the resolved `graph_hash`.

15. Return `StepResult::Completed { node_id, output }` for a single completed
    stage, or `StepResult::CompletedParallel { results }` when an `ExecuteParallel`
    action fired multiple stages. For `CompletedParallel`, `results` contains one
    entry per stage in the parallel branch in input order.

**Ordering note.** Steps 1 to 4 resolve everything that governs the run before any
state is read, so a run that cannot establish its own rules, tier or graph never
reaches a dispatch decision. Steps 9 to 11 resolve everything that governs a
single attempt before that attempt is requested, so an attempt is never started
with an unresolved profile, an unhashed bundle, or no mount plan.

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

See `docs/spec/interfaces/pipeline-execution.md` §update_traceability_matrix for
the full contract.

---

## Implementation Constraints

- The pin resolves and constitutional rules are validated before **any** stage
  runs in a step. No stage driver is called if steps 1 or 2 fail.
- One fresh sandbox per stage attempt. A retry is a new attempt and therefore a
  new request to the stage runner; no sandbox is reused.
- An attempt is never requested without a hashed context bundle and a mount plan.
- A stage contract is never placed in the context bundle of the stage it governs.
- Budget checks for parallel stages must be serialised via `BudgetTracker`'s
  internal mutex — never call `acquire_budget` from two threads concurrently
  without coordination.
- No business logic in this crate computes a verdict, authors a check run, pushes
  a commit, or holds a credential capable of doing either.
- Only check runs authored by the gate service identity are considered. The filter
  is applied at the adapter boundary, so business logic cannot act on a check run
  from an unexpected author even by mistake.
- No edge condition is evaluated by a model.
- No prompt text, persona or discipline-specific logic appears in this crate.

---

## Reviewer Checklist

- The pin resolves and constitutional rules load before any other action in
  `run_step`.
- A missing pin, unresolvable harness, malformed risk map, invalid graph or
  unresolvable profile halts — none of them falls back to a default.
- Graph drift on resume escalates; it neither halts nor resumes against the new
  graph.
- Every stage attempt carries a mount plan and a hashed bundle.
- `NodeInput.stage_contract` is never reachable from `NodeInput.context_bundle`.
- A pending or absent verdict yields `StepResult::Waiting`, never an assumed pass.
- `BudgetTracker::try_acquire` holds the mutex for the full acquire + update cycle.
- `update_traceability_matrix` is never called with `&[AlignmentFinding]` — only
  with `&[TraceabilityEntry]`.
- `ConstitutionallyWrappedPrompt` fields are private (cannot be forged).
- No stage driver contains prompt text, a persona, or domain rules.
