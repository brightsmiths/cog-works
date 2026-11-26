# Pipeline Execution Core

**Architectural Layer**: Business logic + port layer (pure functions, no I/O)
**Crate**: `pipeline`
**Modules**: `execution.rs`, `budget.rs`, `classification.rs`, `review.rs`, `interfaces.rs`
**Introduced in**: PR 7

---

## Overview

This document specifies the interfaces for the pipeline execution core — the five
pure-function modules that together implement the CogWorks state machine:

| Module | Responsibility |
|--------|---------------|
| `execution` | State machine decisions, edge evaluation, rework tracking, sub-work-item ordering |
| `budget`    | Token cost budget enforcement with explicit parallelism atomicity contract |
| `classification` | Safety-critical module detection and scope threshold enforcement |
| `review`    | Review-pass aggregation into a binary `Proceed / Remediate / Escalate` decision |
| `interfaces`| Cross-domain interface contract validation |

All five modules are **pure business logic**: no I/O, no async, fully deterministic
for identical inputs. The `nodes` crate drives these functions within its async
execution loop.

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
    edge_id: &EdgeId,
    cond: &EdgeConditionKind,
    state: &PipelineState,
    output: &NodeOutput,
    llm_evaluated_results: &HashMap<EdgeId, bool>,
    evaluated_at: Timestamp,
) -> (bool, Vec<EdgeEvaluationRecord>)
```

**Purpose**: Evaluates a single edge condition and produces audit records.
Pure synchronous function — no I/O.

**Returns**: A `(bool, Vec<EdgeEvaluationRecord>)` tuple. The `Vec` contains one record for leaf
conditions (`Deterministic`, `LlmEvaluated`) and a root record plus inner records for `Composite`
conditions. The `Vec` is never empty — every call produces at least one record.

**Parameters**:

- `edge_id` — Required to populate `EdgeEvaluationRecord::edge_id`.
- `llm_evaluated_results` — Pre-resolved LLM evaluation outcomes, keyed by
  `EdgeId`. The `nodes` crate populates this before calling this function (see
  **Calling convention** below).
- `evaluated_at` — Passed explicitly so the function remains pure.

**Dispatch**:

- `Deterministic(expr)` → delegates to `evaluate_deterministic_condition(expr, state)`.
- `LlmEvaluated(_)` → looks up `llm_evaluated_results[edge_id]`. If the key
  is absent (should not occur in correct usage), treats as `false` (conservative
  fallback). The caller is responsible for ensuring the map is fully populated.
- `Composite(_)` → recursively evaluates inner conditions.

**Calling convention** (`nodes` crate, PR 9):

```text
1. After a node completes, collect all outgoing edges with LlmEvaluated conditions.
2. For each such edge, call LlmGateway::call asynchronously with the NLC and
   serialised node output; collect the bool result.
3. Build llm_evaluated_results: HashMap<EdgeId, bool> from these results.
4. For every outgoing edge (all condition kinds), call evaluate_edge_condition
   with the same llm_evaluated_results map (empty if no LlmEvaluated edges).
```

This preserves the purity of `evaluate_edge_condition` (no async, no I/O) while
satisfying the `pipeline` crate's no-I/O constraint
(`docs/spec/constraints.md §Module Boundaries`).

The `input_snapshot` field of the returned `EdgeEvaluationRecord` is
`serde_json::to_value(state)` (serialised state at evaluation time).

---

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

**Knows**: The safety-critical registry patterns, the scope threshold.

**Does**: Applies safety overrides to a classification result, enforces scope thresholds.

---

### Type: `SafetyCriticalRegistry`

| Field | Type | Description |
|-------|------|-------------|
| `patterns` | `Vec<String>` | Glob patterns for safety-critical module paths |

Pattern syntax matches `.gitignore` conventions (`*`, `**`, `?`).

---

### Type: `EscalationResult`

| Field | Type | Description |
|-------|------|-------------|
| `estimated_scope` | `u32` | Work item scope estimate (1–10) |
| `threshold` | `u32` | Maximum allowed scope |

Provides `description()` → human-readable explanation for the escalation comment.

---

### Function: `apply_safety_override`

```rust
pub fn apply_safety_override(
    result: ClassificationResult,
    registry: &SafetyCriticalRegistry,
) -> ClassificationResult
```

Tests each `ArtifactPath` in `result.affected_modules` against `registry.patterns`.
If any path matches, sets `result.safety_affecting = true`. Override is one-way
(`false → true` only; never sets to `false`).

---

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

## Module: `review`

### RDD Responsibilities

**Knows**: The three review passes, the remediation budget, and the blocking-finding
rule.

**Does**: Aggregates findings from all three passes into a single gate decision.

---

### Type: `ReviewPass`

```
Quality       — correctness, tests, docs, style, performance
Architecture  — ADR conformance, clean architecture, RDD, interface contract compliance
Security      — OWASP Top-10, secrets, scope, injection, protected paths
```

---

### Type: `ReviewFinding`

| Field | Type | Description |
|-------|------|-------------|
| `pass` | `ReviewPass` | Which pass produced this finding |
| `severity` | `DiagnosticSeverity` | `Blocking`, `Warning`, or `Informational` |
| `description` | `String` | Human-readable finding and remediation guidance |
| `location` | `Option<ArtifactPath>` | Artifact path; `None` for non-file-specific findings |

---

### Type: `ReviewResult`

| Field | Type | Description |
|-------|------|-------------|
| `pass` | `ReviewPass` | Which pass produced these findings |
| `findings` | `Vec<ReviewFinding>` | All findings; blocking findings ordered first |

Helper methods: `has_blocking() -> bool`, `blocking_findings() -> impl Iterator`.

---

### Type: `AggregateReviewDecision`

```
Proceed
Remediate(Vec<ReviewFinding>)   — blocking findings from all passes
Escalate(EscalationReason)      — rework budget exhausted
```

---

### Function: `aggregate_review_results`

```rust
pub fn aggregate_review_results(
    quality: ReviewResult,
    architecture: ReviewResult,
    security: ReviewResult,
    remediation_count: u32,
    limit: u32,
) -> AggregateReviewDecision
```

**Decision table**:

| Any blocking findings? | `remediation_count >= limit`? | Decision |
|------------------------|-------------------------------|----------|
| No | Any | `Proceed` |
| Yes | No | `Remediate(blocking_findings)` |
| Yes | Yes | `Escalate(reason)` |

The `EscalationReason` in `Escalate` has `description` listing all blocking findings
(pass, severity, and text), `rework_count` set to `remediation_count`, and
`attempt_count`/`cost_spent` defaulted to zero (`aggregate_review_results` has no input
for either).

`node_id` is **not** sourced from the Code Generation node. `aggregate_review_results`
has no `NodeId` parameter and constructs the complete `EscalationReason` itself, so it
sets `node_id` to a fixed placeholder, `"review-gate"`.

---

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

### `evaluate_edge_condition` and LLM-evaluated edges

The `LlmEvaluated` variant requires an LLM call, which is async I/O. The `nodes`
crate (PR 9) provides the `LlmGateway` and wires the LLM call. The stub in
`execution.rs` defines the complete signature; the `nodes` crate calls this
indirectly by handling `LlmEvaluated` edges at the orchestration layer.

### `increment_rework_counter` and the graph parameter

The `PipelineGraph` parameter is required to look up `ReworkEdge::max_traversals`
for the given edge ID. Without the graph, the function cannot determine the limit.

### `CostReport.per_node` ordering

The per-node breakdown is ordered descending by cost to make escalation reports
immediately useful to human reviewers (highest spenders appear first in the GitHub
comment).

### `aggregate_review_results` finding order

`Remediate` carries only `Blocking` findings — `Warning` and `Informational` findings
are filtered out and never appear in it (see the Decision Algorithm above). The
retained `Blocking` findings are ordered: Quality findings first, then Architecture,
then Security, with no reordering within a pass (source order is preserved) and no
deduplication. The Code Generation node uses this ordering to structure its rework
prompt.
