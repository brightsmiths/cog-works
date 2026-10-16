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
/// `pipeline.token_cost.output` fields are emitted. See
/// `docs/spec/interfaces/advanced-features.md` §`record_node_complete`.
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
