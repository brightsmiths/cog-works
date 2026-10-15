# Advanced Pipeline Features — Interface Specification

**Architectural Layer**: Business logic (pure functions, no I/O) + minor domain service routing
**Module Paths**:

- `crates/pipeline/src/scenarios.rs`
- `crates/pipeline/src/alignment.rs`
- `crates/pipeline/src/traceability.rs`
- `crates/pipeline/src/observability.rs`
- `crates/pipeline/src/tools.rs` (additions to existing module)
- `crates/pipeline/src/domain_services.rs` (additions to existing module)
**Specification Version**: 1.0

---

## Overview

This document specifies the advanced pipeline features that sit between the
core execution engine and the node implementations. These
features form the quality, traceability, and observability infrastructure that
the pipeline nodes rely on:

1. **Scenario Satisfaction** — aggregates scenario trajectory results into a
   pass/fail determination with a per-scenario score breakdown.
2. **Alignment Verification** — checks that a node's output addresses the
   inputs it received; both deterministic text-based checks and LLM-semantic
   checks are supported.
3. **Traceability Matrix** — tracks requirement coverage across the four
   pipeline stages (Architecture → Interface → SubWorkItem → Code).
4. **Observability Hooks** — thin wrappers around `tracing` spans that emit
   structured fields aligned with the OpenTelemetry semantic conventions used
   to produce pipeline metrics.
5. **Progressive Tool Discovery** — compact, ranked index of available tools
   and skills for LLM-driven exploration.
6. **Skill Validation** — lifecycle-gated validation of skill invocations
   against their manifests and active tool profiles.
7. **Domain Service Routing** — selects which registered domain service(s)
   should handle a given set of artifacts.

All functions in this document are **pure** (no I/O, no async) except where
explicitly noted. Async behaviour is reserved for domain service interaction
traits defined in `domain_services.rs`.

---

## Dependencies

| This module uses | From |
|-----------------|------|
| `ArtifactPath`, `DomainServiceName`, `SkillName`, `ToolName`, `WorkItemId`, `PipelineRunId`, `NodeId`, `SubWorkItemId` | `crates/pipeline/src/identifiers.rs` |
| `SatisfactionScore`, `AlignmentScore` | `crates/pipeline/src/types.rs` |
| `TrajectoryResult`, `AcceptanceCriteria`, `Scenario` | `crates/pipeline/src/domain_services.rs` |
| `SubWorkItem` | `crates/pipeline/src/execution.rs` |
| `NodeInputs`, `NodeOutput` | `crates/pipeline/src/execution.rs` |
| `ScopeParameters`, `ToolProfile` | `crates/pipeline/src/knowledge.rs` |
| `AlignmentFinding` | `crates/pipeline/src/alignment.rs` |

---

## Part 1 — Scenario Satisfaction (`scenarios.rs`)

### PerScenarioScore

A pass/fail result for a single scenario, including a fractional satisfaction
score calculated from the trajectory results.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `scenario_id` | `String` | Identifier matching [`Scenario::id`] |
| `satisfied_trajectories` | `u32` | Count of trajectories that met all acceptance criteria |
| `total_trajectories` | `u32` | Total number of trajectories executed for this scenario |
| `score` | `SatisfactionScore` | `satisfied_trajectories / total_trajectories`, clamped to `[0.0, 1.0]` |
| `passed` | `bool` | `true` when `score >= threshold` passed to [`compute_satisfaction`] |
| `explicit_failure` | `bool` | `true` if any trajectory in the group has `expected_failure == true` (presence-based — independent of whether the failure was actually observed; see `explicit_failure_violations` for observation-based tracking) |

### ScenarioSatisfactionResult

Aggregated result for all scenarios executed in one simulation pass.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `per_scenario` | `Vec<PerScenarioScore>` | Per-scenario breakdown |
| `overall_score` | `SatisfactionScore` | Unweighted mean of per-scenario scores |
| `passed` | `bool` | `true` when every per-scenario `passed` is `true` (explicit-failure scenarios must also meet their own threshold) **and** `explicit_failure_violations` is empty |
| `explicit_failure_violations` | `Vec<String>` | Scenario IDs of expected-failure scenarios whose failure was *not* observed |

### `fn compute_satisfaction`

```rust
pub fn compute_satisfaction(
    trajectory_results: &[TrajectoryResult],
    threshold: SatisfactionScore,
) -> ScenarioSatisfactionResult
```

**Purpose**: Aggregates raw trajectory results into per-scenario scores and an
overall satisfaction determination.

**Behaviour**:

1. Groups `trajectory_results` by `scenario_id`.
2. For each group: counts satisfied trajectories; divides by total count to
   produce `score`; applies `threshold` to set `passed`.
3. Identifies explicit-failure scenarios (where `TrajectoryResult::expected_failure
   == true`) and verifies that at least one trajectory in the group *did* fail
   as expected. Violations are collected into `explicit_failure_violations`.
4. Computes `overall_score` as the unweighted mean of per-scenario scores.
5. Sets `passed = true` iff every per-scenario `passed` is `true` **and**
   `explicit_failure_violations` is empty.

**Error conditions**: None — always returns a result. Empty input yields
`ScenarioSatisfactionResult { passed: true, overall_score: 1.0, … }` (vacuously
true: no scenarios to fail).

**Example**:

```rust
let results = vec![
    TrajectoryResult { scenario_id: "sc-01".to_string(), passed: true, satisfaction_score: SatisfactionScore::new(1.0).unwrap(), expected_failure: false, diagnostics: Diagnostics::empty() },
    TrajectoryResult { scenario_id: "sc-01".to_string(), passed: false, satisfaction_score: SatisfactionScore::new(0.0).unwrap(), expected_failure: false, diagnostics: Diagnostics::empty() },
    TrajectoryResult { scenario_id: "sc-02".to_string(), passed: true, satisfaction_score: SatisfactionScore::new(1.0).unwrap(), expected_failure: true, diagnostics: Diagnostics::empty() },
];
let r = compute_satisfaction(&results, SatisfactionScore::new(0.5).unwrap());
// sc-01: 1/2 passed → score 0.5 → passed (0.5 >= 0.5)
// sc-02: explicit failure observed → passed
assert!(r.passed);
assert!(r.explicit_failure_violations.is_empty());
```

---

## Part 2 — Alignment Verification (`alignment.rs`)

Alignment verification checks that a node's output properly addresses the
inputs it received. This catches cases where the LLM has drifted from the task
(e.g. implementing the wrong function, producing extraneous files, or missing
a required deliverable).

### AlignmentCheckType

```rust
pub enum AlignmentCheckType {
    Deterministic,
    LlmSemantic,
}
```

- `Deterministic`: Structural checks that can be performed without an LLM
  (e.g. verifying that all declared output slots are populated, that no
  extraneous files outside the approved scope are present).
- `LlmSemantic`: Semantic checks delegated to an LLM (e.g. verifying that the
  implementation matches the specification intent).

### MisalignmentType

```rust
pub enum MisalignmentType {
    Missing,
    Extra,
    Modified,
    Ambiguous,
    ScopeExceeded,
}
```

| Variant | Meaning |
|---------|---------|
| `Missing` | An expected output or requirement was not addressed |
| `Extra` | Output was produced for something not in the input scope |
| `Modified` | An artifact changed in a way that contradicts the input spec |
| `Ambiguous` | The relationship between input and output is unclear |
| `ScopeExceeded` | Output touches artifacts outside the declared scope |

### AlignmentFinding

A single misalignment discovered by one alignment check.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `check_type` | `AlignmentCheckType` | Which kind of check produced this finding |
| `misalignment` | `MisalignmentType` | The category of misalignment |
| `description` | `String` | Human-readable explanation of the finding |
| `blocking` | `bool` | `true` if this finding must halt progression (e.g. `ScopeExceeded`) |

All `ScopeExceeded` findings are always blocking. `Missing` findings are
blocking when the missing item is declared as a required output slot.
`LlmSemantic` check findings are blocking based on LLM judgement encoded in the
response.

### AlignmentResult

The complete result of one alignment check pass.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `score` | `AlignmentScore` | Fraction of items checked that aligned, in `[0.0, 1.0]` |
| `findings` | `Vec<AlignmentFinding>` | All individual misalignments found |
| `traceability_entries` | `Vec<TraceabilityEntry>` | Pairs of (requirement ref, output ref) for matrix update. **Only populated by `LlmSemantic` checks.** Deterministic checks leave this field empty; passing an empty slice to `update_traceability_matrix` records an honest *uncovered* status for that stage rather than fabricating coverage. |
| `aligned` | `bool` | `true` when there are no blocking findings |

### TraceabilityEntry

A single link between a requirement reference and an output artifact or section,
produced by the alignment check for use in the traceability matrix update.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `requirement_ref` | `String` | Identifier of the requirement being addressed |
| `output_ref` | `String` | Identifier of the output that addresses the requirement (file path, section name, etc.) |

### AlignmentConfig

Configuration for one alignment check pass.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `threshold` | `AlignmentScore` | Minimum required score; below this the check fails |
| `enabled_checks` | `Vec<AlignmentCheckType>` | Which check types to run |
| `rework_budget` | `u32` | Maximum number of rework cycles before escalation |
| `use_different_model` | `bool` | If `true`, the `nodes` crate uses a separate (critic) model for `LlmSemantic` checks |

### `fn run_deterministic_alignment`

```rust
pub fn run_deterministic_alignment(
    inputs: &DeclaredNodeInputs,
    node_output: &NodeOutput,
) -> Vec<AlignmentFinding>
```

**Purpose**: Performs structural alignment checks that do not require an LLM.

**Note**: The parameter type is `DeclaredNodeInputs` (defined in `alignment.rs`)
rather than `NodeInputs` (defined in the future `nodes` crate). This subset type
contains exactly the fields that deterministic checks require, avoiding a
cross-crate dependency on PR 9.

**Behaviour**:

1. Verifies that every output slot declared in `inputs.required_output_slots`
   is present as a key in `node_output.artifacts`.
2. Checks that no artifact key in `node_output.artifacts` falls outside the
   paths declared in `inputs.approved_scope`.
3. Verifies no artifacts from `inputs.protected_paths` were modified
   (compares presence against `node_output.artifacts` keys).

**Returns**: Zero or more `AlignmentFinding` values. An empty vec means the
structural alignment checks all passed.

---

## Part 3 — Traceability Matrix (`traceability.rs`)

The traceability matrix tracks which requirements from the original work item
have been addressed at each pipeline stage, providing an audit trail and a
human-readable summary that can be committed to GitHub.

### Requirement

A single requirement extracted from the original work item.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `String` | Short identifier (e.g. `"REQ-001"`) |
| `description` | `String` | Full text of the requirement |
| `source_work_item` | `WorkItemId` | The GitHub work item from which this requirement was extracted |

### TraceabilityStage

```rust
pub enum TraceabilityStage {
    Architecture,
    Interface,
    SubWorkItem,
    Code,
}
```

The four stages of artifact production in the pipeline, in progression order.

### RequirementRow

One row of the traceability matrix — tracks one requirement's coverage across
all four stages.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `requirement` | `Requirement` | The requirement being tracked |
| `architecture_covered` | `bool` | `true` after the Architecture node addresses this requirement |
| `interface_covered` | `bool` | `true` after the Interface Design node addresses this requirement |
| `sub_work_item_covered` | `bool` | `true` after a Planning sub-work-item addresses this requirement |
| `code_covered` | `bool` | `true` after Code Generation addresses this requirement |
| `status` | `TraceabilityStatus` | `Complete`, `Partial`, or `Missing` |

### TraceabilityStatus

```rust
pub enum TraceabilityStatus {
    Complete,
    Partial,
    Missing,
}
```

- `Complete`: All four stages have coverage.
- `Partial`: At least one but not all stages have coverage.
- `Missing`: No stage has coverage (newly extracted requirement).

### TraceabilityMatrix

The full matrix for one pipeline run.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `rows` | `Vec<RequirementRow>` | One row per requirement |
| `pipeline_run_id` | `PipelineRunId` | Identifies which run produced this matrix |
| `work_item_id` | `WorkItemId` | The work item being implemented |
| `complete` | `bool` | `true` when all rows have `TraceabilityStatus::Complete` |

### `fn extract_requirements`

```rust
pub fn extract_requirements(work_item_body: &str) -> Vec<Requirement>
```

**Purpose**: Parses requirement declarations from a GitHub issue body.

**Behaviour**:

- Scans `work_item_body` for lines matching the pattern `REQ-NNN: <description>`
  (case-insensitive tag, colon separator, arbitrary description text).
- Returns one `Requirement` per matching line, with `id` set to the tag and
  `description` set to the remainder of the line.
- Lines that do not match the pattern are silently ignored.
- `source_work_item` is not populated by this function (caller must set it).

**Returns**: Empty vec if no requirements are found; never returns an error.

**Note**: The `source_work_item` field of each returned `Requirement` is
initialised to `WorkItemId::new(0)` (sentinel zero value). Callers must set it
to the relevant `WorkItemId` after calling this function.

### `fn update_traceability_matrix`

```rust
pub fn update_traceability_matrix(
    matrix: TraceabilityMatrix,
    stage: TraceabilityStage,
    entries: &[TraceabilityEntry],
) -> TraceabilityMatrix
```

**Purpose**: Applies structured traceability entries from one pipeline stage to
advance the coverage flags in the traceability matrix.

**Behaviour**:

1. For each `TraceabilityEntry` in `entries`, match `requirement_ref` against
   the `id` field of each row in `matrix.rows`.
2. When a match is found, set the corresponding stage coverage flag
   (`architecture_covered`, `interface_covered`, `sub_work_item_covered`, or
   `code_covered`) to `true`.
3. Recompute each row's `status` field (Complete / Partial / Missing).
4. Recompute the matrix-level `complete` flag.

**Source of entries**: `entries` must be `AlignmentResult::traceability_entries`
from a completed `LlmSemantic` alignment check. That field is only populated by
the LLM-semantic check path — deterministic checks do not produce traceability
entries. If only deterministic checks were run for a stage, the caller passes an
empty slice; stage coverage flags remain unchanged, recording an honest
*uncovered* status rather than fabricating coverage.

**Returns**: The updated matrix (value, not in-place mutation).

---

## Part 4 — Observability Hooks (`observability.rs`)

These are thin wrappers around `tracing` spans. They emit structured fields
aligned with the OpenTelemetry semantic conventions used to produce pipeline
metrics. No custom metric sink or backend exists in CogWorks — the OTel layer
in `cli` collects and exports all spans.

The functions in this module are the canonical way for the `nodes` crate to
record pipeline-level events. Direct use of `tracing` macros in `nodes` is
permitted for fine-grained diagnostics, but pipeline metrics (node timing, retry
counts, rework counts, token usage) **must** go through these helpers so that
the OTel attribute names remain consistent across all node types.

### RootCause

The categorised root cause for a retry or rework cycle.

```rust
pub enum RootCause {
    CompilationError,
    TestFailure,
    ReviewFinding,
    ConstraintViolation,
    AlignmentFailure,
    Timeout,
}
```

Used as a structured field in retry and rework spans so that downstream
dashboards can break down retry rates by cause category.

### Observability Functions

All functions take borrowed references and emit events on the **current**
`tracing` span (i.e. they call `tracing::Span::current()` internally; callers
are expected to have entered the relevant span before calling).

#### `fn record_node_start`

```rust
pub fn record_node_start(run_id: &PipelineRunId, node: &NodeId, span: &tracing::Span)
```

Emits structured fields:

- `pipeline.run_id` = `run_id` string
- `pipeline.node_id` = `node` string
- `pipeline.event` = `"node_start"`

#### `fn record_node_complete`

```rust
pub fn record_node_complete(
    run_id: &PipelineRunId,
    node: &NodeId,
    token_cost: &TokenCost,
    span: &tracing::Span,
)
```

Emits structured fields:

- `pipeline.run_id`, `pipeline.node_id`, `pipeline.event` = `"node_complete"`
- `pipeline.token_cost.input` = input token count
- `pipeline.token_cost.output` = output token count
- `pipeline.token_cost.total_usd` = total cost in USD

#### `fn record_retry`

```rust
pub fn record_retry(
    run_id: &PipelineRunId,
    node: &NodeId,
    cause: &RootCause,
    span: &tracing::Span,
)
```

Emits structured fields:

- `pipeline.run_id`, `pipeline.node_id`, `pipeline.event` = `"node_retry"`
- `pipeline.retry.root_cause` = string representation of `cause`

#### `fn record_rework`

```rust
pub fn record_rework(
    run_id: &PipelineRunId,
    node: &NodeId,
    misalignment: &MisalignmentType,
    span: &tracing::Span,
)
```

Emits structured fields:

- `pipeline.run_id`, `pipeline.node_id`, `pipeline.event` = `"node_rework"`
- `pipeline.rework.misalignment_type` = string representation of `misalignment`

---

## Part 5 — Progressive Tool Discovery (`tools.rs` additions)

These types and functions are added to the existing `crates/pipeline/src/tools.rs`
module alongside the `LlmProvider` trait and related types.

### ToolIndexEntry

A single entry in the compact tool index, used for progressive tool discovery
by LLM nodes.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `tool_name` | `ToolName` | The tool's canonical name |
| `description` | `String` | One-line human-readable description |
| `is_skill` | `bool` | `true` if this is a composite skill rather than a raw tool call |

### CompactToolIndex

An ordered list of tool index entries, with skills ranked above raw tools to
encourage LLM agents to prefer composable, validated workflows.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `entries` | `Vec<ToolIndexEntry>` | Skills first, then raw tools; within each group, alphabetical |

### `fn build_compact_index`

```rust
pub fn build_compact_index(
    tool_list: &[ToolName],
    profiles: &[ToolProfile],
) -> CompactToolIndex
```

**Purpose**: Constructs a compact tool index from a list of available tool names
and their profile metadata.

**Behaviour**:

1. For each `ToolName` in `tool_list`, looks up whether it is a skill in
   `profiles` (a tool is a skill if any `ToolProfile::skills` list contains it).
2. Constructs a `ToolIndexEntry` for each tool, setting `is_skill` accordingly.
3. Returns entries with skills sorted before raw tools; within each group,
   sorted alphabetically by `tool_name`.

**Note**: `description` for each entry is currently populated as an empty
string. Profile information in `ToolProfile` does not yet carry per-tool
descriptions; this is expected to be enriched when the `nodes` crate assembles
the index for LLM consumption.

### `fn search_tools`

```rust
pub fn search_tools(index: &CompactToolIndex, query: &str) -> Vec<ToolIndexEntry>
```

**Purpose**: Returns all entries in the index whose name or description contains
`query` as a case-insensitive substring.

**Returns**: Matching entries in the same order they appear in `index.entries`
(skills before raw tools, alphabetical within each group).

---

## Part 6 — Skill Validation (`tools.rs` additions)

### SkillLifecycle

The lifecycle state of a skill, governing whether it may be invoked.

```rust
pub enum SkillLifecycle {
    Proposed,
    Reviewed,
    Active,
    Deprecated { alternative: SkillName },
    Retired,
}
```

| Variant | Invocable? | Note |
|---------|-----------|------|
| `Proposed` | No | Under review, not yet approved for use |
| `Reviewed` | No | Review complete but not yet activated |
| `Active` | Yes | Fully approved |
| `Deprecated` | No | Use `alternative` instead |
| `Retired` | No | Permanently removed |

### SkillManifest

The full specification of a skill, loaded from the skill registry.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `name` | `SkillName` | Canonical skill name |
| `lifecycle` | `SkillLifecycle` | Current lifecycle state |
| `parameter_schema` | `OutputSchema` | JSON Schema for this skill's parameters |
| `tool_call_sequence` | `Vec<ToolName>` | Ordered list of raw tool calls the skill expands to |
| `scope_constraints` | `ScopeParameters` | Scope restrictions applied when this skill is invoked |

### SkillInvocation

A request to invoke a skill, before validation.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `skill_name` | `SkillName` | The skill to invoke |
| `parameters` | `serde_json::Value` | Caller-supplied parameter values |

### ValidatedSkillInvocation

An opaque wrapper produced only when skill validation passes. The `nodes` crate
cannot construct this directly — it can only be obtained by calling
[`validate_skill_invocation`].

This type enforces the invariant: a skill can only be executed if it has been
validated against its manifest and the active profile.

### SkillError

```rust
pub enum SkillError {
    LifecycleInactive { skill_name: SkillName, lifecycle_state: String },
    SchemaValidationFailed { description: String },
    ProfileProhibited { skill_name: SkillName },
    UnknownSkill { skill_name: SkillName },
}
```

| Variant | Meaning |
|---------|---------|
| `LifecycleInactive` | The skill's lifecycle is not `Active` |
| `SchemaValidationFailed` | Parameters do not conform to the skill's `parameter_schema` |
| `ProfileProhibited` | The active `ToolProfile` does not permit this skill |
| `UnknownSkill` | No manifest exists for the requested skill name |

### `fn validate_skill_invocation`

```rust
pub fn validate_skill_invocation(
    invocation: &SkillInvocation,
    manifest: &SkillManifest,
    profile: &ToolProfile,
) -> Result<ValidatedSkillInvocation, SkillError>
```

**Purpose**: Validates that a skill invocation is permitted by its lifecycle,
its parameter schema, and the active tool profile.

**Behaviour** (in order):

1. Returns `Err(SkillError::LifecycleInactive)` if `manifest.lifecycle` is not
   `Active`.
2. Returns `Err(SkillError::ProfileProhibited)` if `manifest.name` is not in
   `profile.allowed_skills`.
3. Validates `invocation.parameters` against `manifest.parameter_schema` using
   the same JSON Schema validation logic as [`OutputSchema`]. Returns
   `Err(SkillError::SchemaValidationFailed)` on mismatch.
4. Returns `Ok(ValidatedSkillInvocation { .. })` with the validated data.

---

## Part 7 — Domain Service Routing (`domain_services.rs` additions)

These types and functions are added to `crates/pipeline/src/domain_services.rs`.

### ServiceCapabilities

The cached capabilities of a registered domain service, populated after a
successful handshake.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `domain` | `DomainServiceName` | The service's own name |
| `artifact_types` | `Vec<String>` | Artifact type identifiers the service can process |
| `interface_types` | `Vec<String>` | Interface type identifiers the service can extract or validate |
| `supported_methods` | `Vec<String>` | Capability identifiers (e.g. `"validate"`, `"simulate"`) |

Derived from the [`HandshakeResult`] at startup. Cached in
`DomainServiceRegistration` to avoid repeated handshakes.

### DomainServiceRegistration

A single registered domain service entry.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `service_name` | `DomainServiceName` | Unique registration key |
| `transport_config` | `serde_json::Value` | Opaque transport configuration (socket path or HTTP URL); parsed by `extension-api` |
| `capabilities` | `Option<ServiceCapabilities>` | `None` before handshake, `Some` after |

### ServiceRegistry

The complete set of registered domain services for this pipeline run.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `registrations` | `Vec<DomainServiceRegistration>` | All registered services |

### RoutingError

Errors returned by routing functions.

```rust
pub enum RoutingError {
    NoServiceFound { artifact_paths: Vec<ArtifactPath> },
    Ambiguous { artifact_paths: Vec<ArtifactPath>, candidates: Vec<DomainServiceName> },
    HandshakePending { service: DomainServiceName },
}
```

| Variant | Meaning |
|---------|---------|
| `NoServiceFound` | No registered service can handle the given artifact paths |
| `Ambiguous` | Multiple services claim to handle the same artifact paths with equal specificity |
| `HandshakePending` | The best-matching service has not yet completed its handshake |

### `fn select_service_for_artifacts`

```rust
pub fn select_service_for_artifacts(
    registry: &ServiceRegistry,
    artifact_paths: &[ArtifactPath],
) -> Result<DomainServiceName, RoutingError>
```

**Purpose**: Selects the single domain service that should process a given set
of artifact paths.

**Behaviour**:

1. Filters to services whose `capabilities` are `Some` (handshake complete).
2. For each candidate, scores it by the number of `artifact_types` that match
   the file extensions of `artifact_paths`.
3. If exactly one service has the highest score: returns its name.
4. If multiple services tie at the highest score: returns
   `Err(RoutingError::Ambiguous)`.
5. If no service matches: returns `Err(RoutingError::NoServiceFound)`.
6. If the best-matching service has `capabilities == None`:
   returns `Err(RoutingError::HandshakePending)`.

### `fn resolve_primary_and_secondary`

```rust
pub fn resolve_primary_and_secondary(
    registry: &ServiceRegistry,
    artifact_paths: &[ArtifactPath],
    interface_type_identifiers: &[String],
) -> Result<(DomainServiceName, Vec<DomainServiceName>), RoutingError>
```

**Purpose**: Resolves the primary domain service (for validation and normalisation)
and any secondary services (for interface extraction and dependency checks) for
a sub-work-item's artifact and interface set.

**Behaviour**:

1. Calls [`select_service_for_artifacts`] with `artifact_paths` to find the primary service.
2. Finds all other services whose `artifact_types` intersect with
   `interface_type_identifiers` (for cross-domain validation).
3. Returns `(primary, secondaries)` where `secondaries` excludes the primary.

---

## Implementation Notes

### JSON Schema Validation

`OutputSchema::new` accepts any JSON object. Schema validation in
`validate_skill_invocation` uses a compatible JSON Schema validator; the
`nodes` crate is responsible for selecting and wiring one up.

### Requirement ID Format

The `extract_requirements` function recognises tags of the form `REQ-` followed
by one or more digits. Non-standard tags are ignored. Projects may pre-populate
the work item body with requirement tags before the pipeline runs.

### Observability Span Ownership

The functions in `observability.rs` take `&tracing::Span` but emit events on
the **passed-in span** using `span.record(...)`. This design lets the `nodes`
crate control span creation and lifetime while ensuring consistent field names
in all events.

### Artifact Type Matching

Service routing matches `artifact_paths` against `artifact_types` by comparing
the file extension of each path against each artifact type identifier. The
convention is `domain/extension` (e.g. `"rust/source"` matches `.rs` files,
`"kicad/schematic"` matches `.kicad_sch` files). This convention is owned by
the domain service; CogWorks treats type identifiers as opaque strings for
matching.
