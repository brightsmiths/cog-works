//! Observability hooks — structured `tracing` span instrumentation.
//!
//! This module provides thin wrappers around [`tracing`] spans that emit
//! structured event fields aligned with the OpenTelemetry semantic conventions
//! used to produce `CogWorks` pipeline metrics.
//!
//! ## Design
//!
//! `CogWorks` uses no custom metric sink or backend. All metrics flow from
//! `tracing` spans through the `OTel` layer configured at startup in `cli`
//! (`tracing-opentelemetry` + OTLP exporter). Dashboard queries target the
//! `OTel` attribute names emitted here.
//!
//! These functions are the **canonical** way for the `nodes` crate to record
//! pipeline-level events. Direct `tracing` macro usage is permitted for
//! fine-grained node diagnostics, but pipeline metrics (node timing, retry
//! counts, rework counts, token usage) **must** go through these helpers so
//! that `OTel` attribute names remain consistent across all node types.
//!
//! ## Span Ownership
//!
//! All functions accept `&tracing::Span` and record events on that span using
//! `span.record(...)`. The `nodes` crate controls span creation and lifetime;
//! this module only populates fields. Callers are expected to have entered the
//! relevant span before calling.
//!
//! **Fields must be pre-declared at span-creation time.** `tracing::Span::record`
//! only populates fields that were declared when the span was created — for
//! example via `tracing::field::Empty` placeholders in a `tracing::info_span!`
//! call, or the equivalent in an `#[instrument]` attribute. Recording a field
//! name that was **not** declared on the span is a **silent no-op**: no panic,
//! no error, no log line — `tracing` simply drops the value. There is no way
//! to detect this from within this module; `span.record()` returns `()`.
//!
//! The consequence is blunt: if the caller's span does not declare a field
//! this module writes, that attribute will never appear on the exported span,
//! and there is no visible failure anywhere in the pipeline. The `OTel`
//! exporter sends nothing for that attribute, and the dashboard panel built on
//! it will simply show no data — silently, with no error, no warning, and
//! nothing to alert on.
//!
//! Each function below requires the caller's span to have pre-declared the
//! following field names before the function is called:
//!
//! | Function | Required pre-declared fields |
//! |----------|-------------------------------|
//! | [`record_node_start`] | `pipeline.run_id`, `pipeline.node_id`, `pipeline.event` |
//! | [`record_node_complete`] | `pipeline.run_id`, `pipeline.node_id`, `pipeline.event`, `pipeline.token_cost.total_usd` |
//! | [`record_retry`] | `pipeline.run_id`, `pipeline.node_id`, `pipeline.event`, `pipeline.retry.root_cause` |
//! | [`record_rework`] | `pipeline.run_id`, `pipeline.node_id`, `pipeline.event`, `pipeline.rework.misalignment_type` |
//!
//! A `tracing` span's field set is fixed at creation and cannot be extended
//! afterwards. A caller that wants a single span to represent a node's whole
//! lifecycle — start, completion, and any retries/reworks in between — must
//! therefore declare the union of all six possible fields up front:
//!
//! ```rust
//! let span = tracing::info_span!(
//!     "pipeline_node_execution",
//!     pipeline.run_id = tracing::field::Empty,
//!     pipeline.node_id = tracing::field::Empty,
//!     pipeline.event = tracing::field::Empty,
//!     pipeline.token_cost.total_usd = tracing::field::Empty,
//!     pipeline.retry.root_cause = tracing::field::Empty,
//!     pipeline.rework.misalignment_type = tracing::field::Empty,
//! );
//! ```
//!
//! Only declared fields can ever be populated by `record_node_start`,
//! `record_node_complete`, `record_retry`, or `record_rework` when passed this
//! `span` — any of the six names omitted from the `info_span!` call above
//! would be silently dropped by whichever function tries to record it.
//!
//! This module deliberately does **not** provide a helper or macro that
//! declares these fields automatically. Span creation belongs to the caller
//! (see "Span Ownership" above), and this module's scope is limited to
//! populating fields on a span it is handed — it is not the owner of span
//! creation. Whoever wires these hooks into the pipeline executor (#57) is
//! better positioned to decide whether such a helper belongs there.
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/advanced-features.md` §Observability Hooks for
//! the full field catalog and `OTel` attribute mapping.

use serde::{Deserialize, Serialize};

use crate::{
    alignment::MisalignmentType,
    identifiers::{NodeId, PipelineRunId},
    types::TokenCost,
};

// ─── Root cause ───────────────────────────────────────────────────────────────

/// The categorised root cause for a retry or rework cycle.
///
/// Used as a structured field in retry and rework spans so that downstream
/// dashboards can break down retry rates by cause category without requiring
/// free-text parsing.
///
/// See `docs/spec/interfaces/advanced-features.md` §`RootCause`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RootCause {
    /// A build or compilation step produced errors.
    CompilationError,
    /// Automated tests failed for the generated artifacts.
    TestFailure,
    /// A multi-pass review gate found blocking issues.
    ReviewFinding,
    /// Cross-domain interface constraint validation found violations.
    ConstraintViolation,
    /// Alignment verification found blocking misalignments.
    AlignmentFailure,
    /// A node or service call exceeded its configured timeout.
    Timeout,
}

impl std::fmt::Display for RootCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CompilationError => write!(f, "compilation_error"),
            Self::TestFailure => write!(f, "test_failure"),
            Self::ReviewFinding => write!(f, "review_finding"),
            Self::ConstraintViolation => write!(f, "constraint_violation"),
            Self::AlignmentFailure => write!(f, "alignment_failure"),
            Self::Timeout => write!(f, "timeout"),
        }
    }
}

// ─── Observability functions ──────────────────────────────────────────────────

/// Records the start of a pipeline node execution.
///
/// Emits the following structured fields on `span`:
///
/// | Field | Value |
/// |-------|-------|
/// | `pipeline.run_id` | `run_id` as string |
/// | `pipeline.node_id` | `node` as string |
/// | `pipeline.event` | `"node_start"` |
///
/// # Arguments
///
/// * `run_id` — current pipeline run identifier.
/// * `node` — the node being started.
/// * `span` — the `tracing` span to record fields on.
///
/// See `docs/spec/interfaces/advanced-features.md` §`record_node_start`.
pub fn record_node_start(run_id: &PipelineRunId, node: &NodeId, span: &tracing::Span) {
    span.record("pipeline.run_id", run_id.to_string());
    span.record("pipeline.node_id", node.to_string());
    span.record("pipeline.event", "node_start");
}

// ---------------------------------------------------------------------------

/// Records the successful completion of a pipeline node execution.
///
/// Emits the following structured fields on `span`:
///
/// | Field | Value |
/// |-------|-------|
/// | `pipeline.run_id` | `run_id` as string |
/// | `pipeline.node_id` | `node` as string |
/// | `pipeline.event` | `"node_complete"` |
/// | `pipeline.token_cost.total_usd` | total cost in USD |
///
/// Note: [`TokenCost`] is a monetary-only type (USD) with no input/output
/// token-count breakdown, so no `pipeline.token_cost.input` /
/// `pipeline.token_cost.output` fields are emitted. Adding that breakdown is
/// tracked as issue #162. See `docs/spec/interfaces/advanced-features.md`
/// §`record_node_complete`.
///
/// # Arguments
///
/// * `run_id` — current pipeline run identifier.
/// * `node` — the node that completed.
/// * `token_cost` — tokens consumed and estimated cost for this execution.
/// * `span` — the `tracing` span to record fields on.
///
/// See `docs/spec/interfaces/advanced-features.md` §`record_node_complete`.
pub fn record_node_complete(
    run_id: &PipelineRunId,
    node: &NodeId,
    token_cost: &TokenCost,
    span: &tracing::Span,
) {
    span.record("pipeline.run_id", run_id.to_string());
    span.record("pipeline.node_id", node.to_string());
    span.record("pipeline.event", "node_complete");
    span.record("pipeline.token_cost.total_usd", token_cost.as_f64());
}

// ---------------------------------------------------------------------------

/// Records a retry cycle for a pipeline node.
///
/// Emits the following structured fields on `span`:
///
/// | Field | Value |
/// |-------|-------|
/// | `pipeline.run_id` | `run_id` as string |
/// | `pipeline.node_id` | `node` as string |
/// | `pipeline.event` | `"node_retry"` |
/// | `pipeline.retry.root_cause` | string representation of `cause` |
///
/// # Arguments
///
/// * `run_id` — current pipeline run identifier.
/// * `node` — the node being retried.
/// * `cause` — categorised root cause of the retry.
/// * `span` — the `tracing` span to record fields on.
///
/// See `docs/spec/interfaces/advanced-features.md` §`record_retry`.
pub fn record_retry(
    run_id: &PipelineRunId,
    node: &NodeId,
    cause: &RootCause,
    span: &tracing::Span,
) {
    span.record("pipeline.run_id", run_id.to_string());
    span.record("pipeline.node_id", node.to_string());
    span.record("pipeline.event", "node_retry");
    span.record("pipeline.retry.root_cause", cause.to_string());
}

// ---------------------------------------------------------------------------

/// Records a rework cycle for a pipeline node.
///
/// A rework cycle differs from a retry in that the input is modified with
/// findings before the node is re-executed (as opposed to re-executing with
/// the same input). See [`crate::ReworkSemantics`] for the distinction.
///
/// Emits the following structured fields on `span`:
///
/// | Field | Value |
/// |-------|-------|
/// | `pipeline.run_id` | `run_id` as string |
/// | `pipeline.node_id` | `node` as string |
/// | `pipeline.event` | `"node_rework"` |
/// | `pipeline.rework.misalignment_type` | string representation of `misalignment` |
///
/// # Arguments
///
/// * `run_id` — current pipeline run identifier.
/// * `node` — the node being reworked.
/// * `misalignment` — the category of misalignment that triggered rework.
/// * `span` — the `tracing` span to record fields on.
///
/// See `docs/spec/interfaces/advanced-features.md` §`record_rework`.
pub fn record_rework(
    run_id: &PipelineRunId,
    node: &NodeId,
    misalignment: &MisalignmentType,
    span: &tracing::Span,
) {
    span.record("pipeline.run_id", run_id.to_string());
    span.record("pipeline.node_id", node.to_string());
    span.record("pipeline.event", "node_rework");
    span.record(
        "pipeline.rework.misalignment_type",
        misalignment.to_string(),
    );
}

#[cfg(test)]
#[path = "observability_tests.rs"]
mod tests;
