# Pipeline Execution Core

**Architectural Layer**: Business logic + port layer (pure functions, no I/O)
**Crate**: `pipeline`
**Modules**: `execution.rs`, `budget.rs`, `classification.rs`, `review.rs`, `interfaces.rs`
**Introduced in**: PR 7

---

## Overview

This document specifies the interfaces for the pipeline execution core — the
pure-function modules that together implement the CogWorks state machine:

| Module | Responsibility |
|--------|---------------|
| `execution` | State machine decisions, deterministic edge evaluation, attempt disposition, rework tracking, slice ordering |
| `budget`    | Token cost budget enforcement with explicit parallelism atomicity contract |
| `classification` | Tier derivation from the risk map (raise-only) and scope threshold enforcement |
| `findings`  | Review finding structure; carries findings, decides nothing |
| `interfaces`| Cross-domain interface contract validation |
| `alignment` | Deterministic and semantic alignment of a stage's output against its inputs |
| `traceability` | Requirement coverage across the stages of a run |

Every module here is **pure business logic**: no I/O, no async, fully deterministic
for identical inputs. The `nodes` crate drives these functions within its async
execution loop.

None of these modules computes a gate verdict. Alignment results and constraint
findings bound a stage attempt — CogWorks may fail, retry or rework on them — but
they are never published as a check run and never constitute a pass or fail. That
is the gate service's, from the commit SHA, under its own identity.

---

## Dependencies

```
execution.rs
  ├─ graph::{EdgeConditionKind, EdgeEvaluationRecord, NodeGate, NodeStatus,
  │          PipelineGraph, PipelineState}
  ├─ identifiers::{EdgeId, NodeId, SubWorkItemId}
  ├─ types::{CostBudget, Timestamp, TokenCost}
  └─ errors::RetryPolicy

budget.rs
  ├─ identifiers::{NodeId, SubWorkItemId}
  └─ types::{CostBudget, TokenCost}

classification.rs
  └─ context::ClassificationResult

review.rs
  ├─ execution::EscalationReason
  ├─ identifiers::ArtifactPath
  └─ types::DiagnosticSeverity

interfaces.rs
  ├─ domain_services::{InterfaceDefinition, InterfaceMap}
  ├─ identifiers::{DomainServiceName, InterfaceId}
  └─ types::DiagnosticSeverity
```

No circular dependencies. All modules depend only on types from PRs 1–4.

---

## Module: `execution`

### RDD Responsibilities

**Knows**: Which nodes are eligible to run, the rules for fan-in synchronisation,
how rework counters map to termination conditions, the topological ordering of
sub-work-items.

**Does**: Computes the next actions for the state machine, evaluates edge
conditions (dispatching to graph functions and the LLM gateway), increments rework
counters, and sorts sub-work-items by dependency order.

---

### Type: `NodeStateUpdate`

A single requested change to a node's runtime status, included in `NodeOutput.state_updates`.

| Field | Type | Description |
|-------|------|-------------|
| `node_id` | `NodeId` | The node to update |
| `new_status` | `NodeStatus` | The status to apply |
| `error` | `Option<String>` | Error description when `new_status == Failed` |

---

### Type: `NodeOutput`

The structured result produced by a successfully executed node. Shared across all
node `execute` signatures in the `nodes` crate (PR 9).

| Field | Type | Description |
|-------|------|-------------|
| `artifacts` | `HashMap<String, serde_json::Value>` | Named output slots keyed by slot name |
| `cost_delta` | `TokenCost` | Token cost incurred during this execution step |
| `state_updates` | `Vec<NodeStateUpdate>` | State changes to apply atomically after execution |

**Note**: `artifacts` keys correspond to `NodeDefinition::declared_outputs` slot names.
Values are domain-specific JSON whose schemas are defined per-node in the `nodes` crate.

---

### Type: `GateStatus`

Human-gate review state for a specific node.

```
AwaitingApproval
Approved { approved_by: String }
Rejected  { rejected_by: String, reason: String }
```

---

### Type: `GateConfig`

All gate states for an active run, keyed by `NodeId`.

| Field | Type | Description |
|-------|------|-------------|
| `gated_nodes` | `HashMap<NodeId, GateStatus>` | Per-node gate status |

**Location note**: `GateConfig` and `GateStatus` are defined in
`pipeline/src/graph.rs` (alongside `PipelineState`) and re-exported from
`pipeline/src/execution.rs`. This placement avoids a circular module
dependency: `execution.rs` imports from `graph.rs`, so placing gate types in
`graph.rs` allows `PipelineStateComment` to embed `GateConfig` without a
circular import. See `pipeline-graph.md §GateConfig`.

---

### Type: `PipelineError`

Execution-engine error enum for unrecoverable failures.

| Variant | When produced |
|---------|--------------|
| `NodeFailed { node_id, error, retry_policy }` | Node exhausted all retry budget |
| `BudgetExceeded { accumulated, limit }` | `acquire_budget` denied; no headroom |
| `GraphInvalid { message }` | `validate_pipeline_graph` failed at load time |
| `ConstitutionalRulesLoadFailed { message }` | Rules file missing or invalid at step start |

---

### Type: `EscalationReason`

Structured human-readable escalation context attached to `NextAction::Escalate`.

| Field | Type | Description |
|-------|------|-------------|
| `description` | `String` | What went wrong |
| `node_id` | `NodeId` | Node that triggered escalation |
| `attempt_count` | `u32` | Total execution attempts for the node |
| `rework_count` | `u32` | Total rework iterations for the node |
| `cost_spent` | `TokenCost` | Cumulative cost across all attempts |

---

### Type: `NextAction`

The set of possible actions the execution engine should take next.

```
ExecuteNode(NodeId)
ExecuteParallel(Vec<NodeId>)
Wait
Escalate(EscalationReason)
HaltWithError(PipelineError)
```

`determine_next_actions` returns a `Vec<NextAction>`. An empty vec signals that the
pipeline run is complete (all non-gated nodes have status `Completed`).

**Vec contents contract**:

| Scenario | Vec contents |
|----------|--------------|
| One or more auto-proceed eligible nodes (no fan-in blocking) | `[ExecuteNode(id)]` or `[ExecuteParallel(ids)]` |
| Mix of auto-proceed eligible and gated-waiting nodes | Only execute actions for the eligible nodes; `Wait` is **not** co-returned |
| All eligible nodes are awaiting gate approval | `[Wait]` |
| A gated node was rejected | `[Escalate(reason)]` |
| A node timeout was exceeded | `[HaltWithError(error)]` |
| No eligible nodes and no active nodes | `[]` (run complete) |

Rationale for the "mix" row: the orchestrator starts available work while waiting for gate
decisions. Mixing `Wait` with execute actions would force the orchestrator to split action
types itself. Instead the unblocked nodes are returned immediately; the gate-waiting nodes
are discovered on the next call after the current nodes complete.

---

### Type: `TerminationConditionReached`

Error from `increment_rework_counter` when the traversal limit is exceeded.

| Field | Type | Description |
|-------|------|-------------|
| `edge_id` | `EdgeId` | The rework edge that hit its limit |
| `current_traversals` | `u32` | Traversal count at the point of overflow |
| `max_traversals` | `u32` | Configured maximum |

---

### Type: `SubWorkItem`

A single planned implementation sub-task with dependency declarations.

| Field | Type | Description |
|-------|------|-------------|
| `id` | `SubWorkItemId` | GitHub sub-issue number |
| `description` | `String` | Human-readable task description |
| `depends_on` | `Vec<SubWorkItemId>` | IDs that must complete before this item starts |

---

### Type: `DependencyError`

Error from `topological_sort_sub_work_items`.

```
CyclicDependency  { cycle: Vec<SubWorkItemId> }
UnknownDependency { item_id: SubWorkItemId, unknown_dep: SubWorkItemId }
```

`CyclicDependency.cycle` contains only strict cycle members (nodes that are part of
at least one directed cycle). Nodes that are merely downstream of a cycle are not
included.

---

### Function: `determine_next_actions`

```rust
pub fn determine_next_actions(
    state: &PipelineState,
    graph: &PipelineGraph,
    gate_config: &GateConfig,
    now: Timestamp,
) -> Vec<NextAction>
```

**Purpose**: Central state machine dispatch. Determines what the execution engine
should do next for an active pipeline run.

**Parameters**:
- `state` — Current runtime state of the pipeline run.
- `graph` — Validated pipeline graph (nodes, edges, settings).
- `gate_config` — Current human-gate approval/rejection status for gated nodes.
- `now` — Current wall-clock time, passed in so the function remains pure and testable.

**Algorithm**:

1. Check all `Active` nodes for elapsed timeouts → `HaltWithError(NodeFailed)` if any timed out.
   Timeout = node-level `timeout` if set, otherwise `graph.settings.default_timeout`.
   Detection rule: `now - activated_at > timeout`. If both are `None` → no timeout check.
2. Call `compute_eligible_nodes(state, graph)` to find nodes ready to start.
3. For each eligible node, check its gate configuration:
   - `HumanGated`: consult `gate_config.gated_nodes` for this node.
     Not present → mark as waiting. `Approved` → include in execute set. `Rejected` → `Escalate`.
   - `AutoProceed`: include in execute set directly.
4. Fan-in nodes: include only if `check_fan_in_ready(node, state, graph)` is true.
5. If execute set is non-empty: multiple → `ExecuteParallel`; single → `ExecuteNode`.
   (`Wait` is **not** co-returned when there are also nodes to execute.)
6. If execute set is empty and all eligible were waiting on gates: `[Wait]`.
7. No eligible and no active → return `[]` (run complete).
   Implicit: no eligible but active nodes exist → return `[Wait]`.

**Return**: See Vec contents contract table in the `NextAction` type section above.

---

### Function: `evaluate_edge_condition`

```rust
pub fn evaluate_edge_condition(
    condition: &EdgeCondition,
    state: &PipelineState,
    verdicts: &VerdictView,
) -> bool
```

Evaluates one edge condition deterministically over pipeline state and the gate
verdicts read for the relevant commits.

**No condition is evaluated by a model.** Routing, gating, context packaging,
retries and escalation are code, so which edge is taken never depends on model
output. There is no `LlmEvaluated` variant and no results map parameter: a model
cannot participate in routing even by configuration.

**Fail closed.** A condition that cannot be evaluated — an unresolvable state
reference, a verdict that is absent when the condition requires one — returns
`false`. An edge is taken only when its condition is affirmatively satisfied. A
condition that failed to evaluate must never open a path through the graph.

**Composites** (`All`, `Any`, `Not`) evaluate their children with the same rules
and the same fail-closed behaviour.

### Function: `check_fan_in_ready`

```rust
pub fn check_fan_in_ready(
    node: &NodeId,
    state: &PipelineState,
    graph: &PipelineGraph,
) -> bool
```

**Purpose**: Returns `true` if all direct predecessor nodes of `node` (via
non-rework forward edges) have `NodeStatus::Completed`.

Used by `determine_next_actions` to prevent fan-in nodes from starting before
all parallel branches are done.

---

### Function: `classify_attempt_outcome`

```rust
pub fn classify_attempt_outcome(
    failure: &NodeFailure,
    verdict: &VerdictState,
    attempts: u32,
    reworks: u32,
    contract: &StageContract,
    recent_classes: &[FailureClass],
) -> AttemptDisposition
```

Decides what happens after a failed stage attempt. This is the rule that bounds
how long a stage may keep trying, stated here rather than left to each driver.

```rust
pub enum AttemptDisposition {
    /// Re-run the attempt with the same context.
    Retry,
    /// Re-run the attempt with structured feedback added.
    Rework { feedback: ReworkFeedback },
    /// Hand the work to a human with a dossier.
    Escalate(EscalationReason),
    /// Stop; the condition is not recoverable by trying again.
    Halt(PipelineError),
}
```

**The rule**, evaluated in order:

1. **Halt** when the condition cannot be improved by another attempt: an
   unverifiable pin, an unresolvable harness or profile, a malformed risk map, a
   missing stage graph, an invalid graph. Retrying these burns budget on a
   configuration problem.
2. **Escalate** when `recent_classes` shows the same `FailureClass` for the
   contract's stall threshold (default 3) of consecutive attempts. Novel but
   equally incorrect output consumes the budget without making progress, so this
   fires *before* the budget is exhausted.
3. **Escalate** when `attempts >= contract.max_attempts` or
   `reworks >= contract.max_reworks`. Exhausting a budget is never a silent pass.
4. **Retry** when the output was *technically invalid* — it did not compile, did
   not parse, or failed output-schema validation. The context was adequate; the
   attempt was not. Same context, new sandbox.
5. **Rework** when the output was *technically valid* but failed a gate verdict or
   an alignment check. More trying will not help; different information might. The
   feedback carries the gate's structured output.

Retry and rework counters are **independent**, each with its own budget from the
stage contract. Exhausting one does not affect the other, and both appear
separately in the audit trail.

**Feedback bounds.** `ReworkFeedback` carries failing test identities and their
messages, never test source. For a holdout verdict it carries no more than the
failing requirement identities. Feedback rich enough to fix a holdout failure
directly is feedback rich enough to overfit to it.

**Every disposition is recorded** with the failure class that produced it, so the
sequence of attempts on a stage is reconstructable from the audit trail.

### Function: `increment_rework_counter`

```rust
pub fn increment_rework_counter(
    edge: &EdgeId,
    state: &mut PipelineState,
    graph: &PipelineGraph,
) -> Result<u32, TerminationConditionReached>
```

**Purpose**: Increments `NodeState::rework_edge_traversals[edge]` and returns
the new count, or `Err(TerminationConditionReached)` if the count would exceed
`ReworkEdge::max_traversals`.

**Caller responsibility**: Check `ReworkEdge::overflow_behaviour` on `Err` to
decide whether to `HaltWithError`, `Escalate`, or `TakeEdge`.

---

### Function: `topological_sort_sub_work_items`

```rust
pub fn topological_sort_sub_work_items(
    items: &[SubWorkItem],
) -> Result<Vec<SubWorkItemId>, DependencyError>
```

**Purpose**: Returns a topological ordering of sub-work-items (sources first).

**Validation**:

- Unknown `depends_on` references → `DependencyError::UnknownDependency`.
- Cycles → `DependencyError::CyclicDependency`.

---

## Module: `budget`

### RDD Responsibilities

**Knows**: The budget limit, accumulated cost, and estimated next-node cost.

**Does**: Decides whether to approve or deny a budget acquisition request.

---

### Type: `CostReport`

Full cost breakdown attached to `BudgetAcquisition::Denied`.

| Field | Type | Description |
|-------|------|-------------|
| `per_node` | `Vec<NodeCostEntry>` | Per-node total cost, descending by cost |
| `per_sub_work_item` | `Vec<SubWorkItemCostEntry>` | Per-sub-work-item total cost |
| `total` | `TokenCost` | Total accumulated cost at failure |
| `budget_limit` | `CostBudget` | The exceeded limit |

---

### Type: `BudgetAcquisition`

```
Approved { remaining: CostBudget }
Denied(CostReport)
```

---

### Function: `acquire_budget`

```rust
pub fn acquire_budget(
    accumulated: &TokenCost,
    estimated: &TokenCost,
    limit: &CostBudget,
    report: impl FnOnce() -> CostReport,
) -> BudgetAcquisition
```

**Purpose**: Returns `Approved` if `accumulated + estimated < limit`, else `Denied`.

**Strict inequality**: The check uses `<` (not `<=`). A node whose estimated cost exactly
equals the remaining headroom is denied. This preserves a minimum headroom guard against
f64 accumulation error (see below).

**Floating-point note**: `TokenCost` and `CostBudget` wrap `f64`. Repeated addition of
small costs accumulates rounding error. Callers should pad budget limits by a small epsilon
if sub-cent precision is required. The strict `<` check provides a minimum guard.

**Lazy report**: `report` is a `FnOnce() -> CostReport` closure; it is only called if the
check is `Denied`. This avoids two `Vec` allocations on the hot-path (the vast majority of
checks are `Approved`).

### ⚠️ Atomicity Contract

This function is **pure and not thread-safe**. When parallel nodes execute:

1. Hold a `Mutex<TokenCost>` for the entire call.
2. Update the accumulator immediately after `Approved`, **while still holding the lock**.
3. Release only after the accumulator is updated.

Releasing before updating allows two nodes each estimating 40 % of the budget to both
be approved even when their combined cost is 80 % (which may push total over 100 %).

The `PipelineExecutor` in the `nodes` crate owns `Arc<Mutex<TokenCost>>` for this purpose.

---

## Module: `classification`

### RDD Responsibilities

**Knows**: the risk map's path-glob-to-tier mapping, the scope threshold, and the
tier previously recorded for a work item.

**Does**: derives a work item's tier from the paths it affects, and rejects a work
item whose estimated scope exceeds the threshold.

**Level**: tier derivation is L2. The scope threshold applies from L1.

### Type: `RiskMap`

Path globs mapped to tiers, read from `risk-map.yaml` in the target repository.
Human-owned: CogWorks reads it and never writes it.

```rust
pub struct RiskMap {
    /// Path glob to tier, in declaration order. Later entries do not override
    /// earlier ones — every match contributes, and the maximum wins.
    pub entries: Vec<(String, Tier)>,
}
```

**Glob matching is case-sensitive**, consistent with protected-path matching. On a
case-insensitive filesystem a glob that differs only in case does not match, so
deployments either enumerate case variants or run on a case-sensitive filesystem.

### Type: `Tier`

```rust
pub enum Tier { T0, T1, T2, T3 }
```

Ordered, so a maximum over a set of paths is well defined and two tiers can be
compared to enforce raise-only semantics.

### Type: `EscalationResult`



| Field | Type | Description |
|-------|------|-------------|
| `estimated_scope` | `u32` | Work item scope estimate (1–10) |
| `threshold` | `u32` | Maximum allowed scope |

Provides `description()` → human-readable explanation for the escalation comment.

---

### Function: `derive_tier`

```rust
pub fn derive_tier(
    risk_map: &RiskMap,
    affected_paths: &[ArtifactPath],
    recorded: Option<Tier>,
) -> Tier
```

Returns the maximum tier over every `risk_map` entry whose glob matches any path
in `affected_paths`, then raises it to `recorded` if `recorded` is higher.

**Raise-only.** The returned tier is never lower than `recorded`. Scope grows
during a run — a slice turns out to touch a path nobody anticipated — and allowing
the tier to fall would let work already assessed as critical be re-assessed
downward by a later, narrower derivation. The ratchet turns one way.

**No path matches**: returns `recorded` if present. Where neither a match nor a
recorded tier exists, the caller halts rather than defaulting. Defaulting to `T0`
would apply the least rigour to work nobody has assessed, which is the worst
available outcome.

**No model input.** Derivation is a pure function of the map and the path set. A
tier is never read from a label, so a label edit cannot weaken the stringency
applied to a piece of work.

### Function: `check_scope_threshold`



```rust
pub fn check_scope_threshold(
    result: ClassificationResult,
    threshold: u32,
) -> Result<ClassificationResult, EscalationResult>
```

Returns `Ok(result)` when `result.estimated_scope <= threshold`.
Returns `Err(EscalationResult { estimated_scope, threshold })` otherwise.

---

## Module: `findings`

### RDD Responsibilities

**Knows**: the structure of a review finding — its category, severity, location
and reasoning.

**Does**: carries findings from an assurance stage to the pull request and, where
a gate fails on them, into rework feedback.

**Does not**: combine findings into a pass, fail, proceed, remediate or escalate
decision. Whether a finding blocks is decided by the gate service from the
findings and the tier's thresholds. There is deliberately no aggregation function
here — see `docs/adr/ADR-0009-assurance-review-produces-findings-not-verdicts.md`.

**Level**: L4.

### Type: `FindingCategory`

```rust
pub enum FindingCategory { Semantic, Security }
```

Which assurance stage produced the finding.

### Type: `ReviewFinding`

```rust
pub struct ReviewFinding {
    pub category: FindingCategory,
    pub severity: FindingSeverity,
    pub location: Option<(ArtifactPath, u32)>,
    pub summary: String,
    pub reasoning: String,
}
```

`severity` is a **claim about seriousness, not an instruction**. A finding marked
`Blocking` by a review stage does not block anything by itself.

`summary` and `reasoning` are model output. Both are scanned via
`detect_injection` before being reused in any later prompt.

### Type: `FindingSeverity`

```rust
pub enum FindingSeverity { Blocking, Warning, Informational }
```

## Module: `interfaces`

### RDD Responsibilities

**Knows**: The registry contracts and extracted interfaces.

**Does**: Finds mismatches between declared and actual interface definitions.

---

### Type: `ConstraintFinding`

| Field | Type | Description |
|-------|------|-------------|
| `interface_id` | `InterfaceId` | The violated interface |
| `parameter_name` | `String` | Field or parameter with the mismatch |
| `expected_value` | `String` | Value in the authoritative registry |
| `actual_value` | `String` | Value extracted from artifacts |
| `owning_domain` | `DomainServiceName` | Domain that authored the contract |
| `violating_domain` | `DomainServiceName` | Domain whose extraction shows the mismatch |
| `severity` | `DiagnosticSeverity` | `Blocking` = structural incompatibility |

---

### Function: `validate_cross_domain_constraints`

```rust
pub fn validate_cross_domain_constraints(
    contracts: &[InterfaceDefinition],
    extracted: &InterfaceMap,
) -> Vec<ConstraintFinding>
```

**Algorithm**:

1. For each `contracts[i]`:
   a. Find matching entry in `extracted.entries` by `InterfaceId`.
   b. If absent → emit `Blocking` finding (`actual_value = "<not present>"`).
   c. If found → compare schemas field-by-field; each mismatch is one finding.
2. Extra definitions in `extracted` with no registry entry → **not reported** (new
   interfaces are permitted).

Returns an empty `Vec` on full conformance; any `Blocking` finding must block the
review gate.

---

## Error Values by Module

| Error type | Module | Produced by |
|-----------|--------|------------|
| `PipelineError` | `execution` | `determine_next_actions` (in `HaltWithError`) |

`PipelineError` combines load-time failures (`GraphInvalid`, `ConstitutionalRulesLoadFailed`)
with runtime failures (`NodeFailed`, `BudgetExceeded`) so that the `run_step` entry point
in `PipelineExecutor` uses a single error channel for the full step lifecycle. Load-time
variants cannot occur after the pre-flight phase, but having one error type simplifies
the entry-point signature and caller error handling.
| `TerminationConditionReached` | `execution` | `increment_rework_counter` |
| `DependencyError` | `execution` | `topological_sort_sub_work_items` |
| `EscalationResult` | `classification` | `check_scope_threshold` |

---

## Implementation Notes

### `increment_rework_counter` and the graph parameter

The `PipelineGraph` parameter is required to look up `ReworkEdge::max_traversals`
for the given edge ID. Without the graph, the function cannot determine the limit.

### `CostReport.per_node` ordering

The per-node breakdown is ordered descending by cost to make escalation reports
immediately useful to human reviewers (highest spenders appear first in the GitHub
comment).

### Review finding order

Findings reach a pull request and any rework feedback in a stable order: semantic
findings first, then security, preserving source order within each category, with
no deduplication.

The order is stable so that two runs over the same change produce comparable
output. It carries no decision weight — ordering is presentation, and whether any
finding blocks is decided by the gate service.

---

## Module: `alignment`

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

**Edge-case behaviour** (resolved during implementation; recorded here so the
spec matches the shipped behaviour):

- **Unparseable artifact keys fail closed.** An artifact key in
  `node_output.artifacts` that cannot be parsed as a valid `ArtifactPath`
  (e.g. it contains a `..` traversal component, or is empty) is **never**
  matched against the raw-string approved-scope pattern. It immediately
  produces its own blocking `ScopeExceeded` finding instead. This is
  deliberate and externally visible: an unresolved string cannot be trusted
  to reflect the artifact's true, path-resolved location, so falling back to
  a raw-string scope match would let a crafted key (e.g.
  `"src/../.factory/pin.yaml"`) defeat both the scope check
  and the protected-path check simultaneously.
- **`protected_paths` adaptation for matching.** `DeclaredNodeInputs.protected_paths`
  holds concrete, already-resolved `ArtifactPath` values (not glob patterns).
  To reuse `security::is_protected`'s matching engine, each entry's string
  value is glob-escaped (so metacharacters such as `[`, `]`, `{`, `}`, `*`,
  `?` are treated literally) before being passed in as a `ProtectedPath`
  pattern. A path containing no `/` still matches at any tree depth (see
  `DeclaredNodeInputs::protected_paths` rustdoc for the full rationale) —
  this over-blocks rather than under-blocks, so it does not weaken the
  guarantee.
- **Protected-path precedence.** When a single artifact is both outside the
  approved scope *and* matches a protected path, exactly one finding is
  produced (the protected-path reason wins) — not two. This mirrors the
  precedence rule `security::validate_scope` already applies between
  `ProtectedPathViolation` and `UnauthorizedCapability` (see `ASSERT-SEC-005`).
- **No deduplication of required-slot names.** If `required_output_slots`
  contains the same slot name more than once and it is absent from
  `node_output.artifacts`, each duplicate occurrence produces its own
  `Missing` finding (callers are expected not to declare duplicates; the
  function does not defensively dedupe).

---

---

## Module: `traceability`

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

---

## Implementation Notes (relocated)

### JSON Schema Validation

Stage output is validated against the `OutputSchema` the harness declares for that
stage type, before the pipeline proceeds. Structured output at every boundary is
what makes a model a component rather than the machine, so this validation is core
and is not optional.

`OutputSchema::new` accepts any JSON object. Validation is performed by a private,
hand-rolled JSON Schema **subset** matcher implemented entirely within the
`pipeline` crate (no external `jsonschema`/`schemars`-family dependency, per the
project's minimal-dependency policy). It supports only the `type`
(`string`/`number`/`integer`/`boolean`/`object`/`array`/`null`), `required`, and
`properties` keywords (the last recursively); any other keyword present on a
schema object is ignored.

The matcher is **fail-closed** rather than strictly spec-compliant in three cases:
a schema of JSON `false` rejects every instance; an unrecognised `type` string
rejects rather than matching anything; and a non-empty `required`/`properties` on
an instance that is not a JSON object rejects, even without an explicit
`"type": "object"` keyword. The literal JSON Schema specification treats
`required`/`properties` as vacuously satisfied for non-object instances; this
implementation deliberately deviates, because the validation sits on a security
boundary and silently accepting a malformed instance is the worse failure.

A harness-declared schema relying on unsupported keywords (`enum`, `pattern`,
`additionalProperties`, `items`, `minimum`/`maximum`) for a security-relevant
constraint will **not** have that constraint enforced here. A schema that needs
one of those is a reason to extend the matcher, not a reason to assume the
constraint holds.

### Requirement ID Format

The `extract_requirements` function recognises tags of the form `REQ-` followed
by one or more digits. Non-standard tags are ignored. Projects may pre-populate
the work item body with requirement tags before the pipeline runs.
