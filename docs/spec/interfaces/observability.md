# Observability — Interface Specification

**Architectural Layer**: Business logic (pure functions, no I/O)
**Module Paths**: `crates/pipeline/src/observability.rs`
**Specification Version**: 1.0

---

## Overview

Thin wrappers around `tracing` spans that emit structured fields aligned with the
OpenTelemetry semantic conventions used to produce pipeline metrics.

CogWorks emits telemetry through the OpenTelemetry SDK exporter. There is no
custom metric sink and no pluggable metrics backend: a deployment configures an
OTel collector endpoint and receives spans and metrics in a standard format.

Telemetry is **evidence, never workflow state**. A run's position is always
reconstructable from GitHub without reading a span. Losing telemetry loses
observability, not the ability to continue.

Fields that carry a resolved profile, a harness version or a context bundle hash
make an attempt traceable from a span back to exactly what ran. Fields that would
carry credential material, bundle *content*, or holdout content are never emitted.

---

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

> **Field pre-declaration is required.** `tracing::Span::record` only
> populates fields that were declared when the span was created (e.g. via
> `tracing::field::Empty` in a `tracing::info_span!`/`#[instrument]` call);
> recording an undeclared field name is a silent no-op with no panic, no
> error, and no visible failure — the attribute simply never reaches the
> `OTel` exporter and dashboards show nothing for it. See
> `crates/pipeline/src/observability.rs`'s module doc comment (`## Span
> Ownership`) for the full per-function field-declaration list and a runnable
> example of declaring all required fields up front.

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
- `pipeline.token_cost.total_usd` = total cost in USD

> **Note**: Earlier revisions of this spec also listed `pipeline.token_cost.input`
> and `pipeline.token_cost.output` fields. `TokenCost` (`crates/pipeline/src/types.rs`)
> is a monetary-only type (USD, `f64`) with no input/output token-count
> breakdown, so the implementation does not emit these two fields. Adding a
> token-count breakdown would require widening `TokenCost` (or introducing a
> separate type) and is tracked as issue #162, not part of this change.
>
> **Open design question for #57 (pipeline executor)**: `record_node_start`,
> `record_node_complete`, `record_retry`, and `record_rework` all write
> `pipeline.event` on whatever span they are given. If a caller reuses a
> single span across a node's whole lifecycle, each later call overwrites the
> earlier `pipeline.event` value (e.g. `record_node_start`'s `"node_start"` is
> overwritten by `record_node_complete`'s `"node_complete"` on the same
> span) — so per-event counts (how many retries, how many reworks) cannot be
> derived from that span's final attributes alone. Whoever implements #57
> will need to decide whether a single lifecycle span is sufficient (and
> counts are derived from separate `tracing` events instead) or whether retries
> and reworks should each get their own span.

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

---

### Observability Span Ownership

The functions in `observability.rs` take `&tracing::Span` but emit events on
the **passed-in span** using `span.record(...)`. This design lets the `nodes`
crate control span creation and lifetime while ensuring consistent field names
in all events.
