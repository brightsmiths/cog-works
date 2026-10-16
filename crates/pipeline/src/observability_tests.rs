//! Adversarial test suite for `observability.rs` — `record_node_start`,
//! `record_node_complete`, `record_retry`, `record_rework`.
//!
//! ## Phase: RED
//!
//! All four hook functions under test compile but will **panic** at runtime
//! because they are `todo!()` stubs. This is the expected RED state in TDD.
//! Tests turn GREEN once the implementation lands.
//!
//! `RootCause::fmt` (`observability.rs`) and `MisalignmentType::fmt`
//! (`alignment.rs`) are already implemented (not stubs) — the contract tests
//! for their `Display` output in this file are regression locks on OTel
//! attribute string values, not RED-phase placeholders.
//!
//! ## Test harness
//!
//! None of these four functions return a value; their only observable effect
//! is `span.record(...)` calls on the caller-supplied `tracing::Span`. There
//! is no existing span-field-capture test infrastructure anywhere in this
//! workspace (checked: no `tracing-test`, no custom `Layer`/`Subscriber` for
//! assertions). This file adds a minimal one: [`FieldCaptureLayer`], a
//! `tracing_subscriber::Layer` that records every field set at span creation
//! (`on_new_span`) or via `span.record()` (`on_record`) into a shared map,
//! installed as the thread-local default subscriber for the duration of a
//! closure via [`with_capture`]. This is the standard pattern for
//! field-level span assertions in the `tracing` ecosystem and requires no
//! new runtime dependency — `tracing-subscriber` is already a pinned
//! workspace dependency (used by the `cli` crate); it is added here only as
//! a `pipeline` dev-dependency.
//!
//! **Important:** every span under test must be *created* inside the
//! `with_capture` closure (i.e. while the capturing subscriber is the active
//! thread-local default). A span created before entering the closure is
//! tracked by whatever subscriber was active at creation time, and
//! `span.record()` calls made later under a different subscriber are
//! effectively invisible to the capturing layer.
//!
//! ## Tier map
//!
//! | Tier | Count | Focus |
//! |---|---|---|
//! | 1 — Specification | 15 | Direct field-table mapping, one assertion per function/field pair |
//! | 1 — Contract (Display) | 2 | `RootCause`/`MisalignmentType` OTel string regression lock |
//! | 2 — Adversarial / boundary | 23 | Field isolation, disabled-span panic-freedom, anti-memoisation, N-1/N boundaries, variant tables |
//! | 3 — Property-based | 6 | Field-value/input invariants and panic-freedom across generated inputs |
//!
//! ## Spec gaps found (reported to Tech Lead / Architect)
//!
//! 1. **`pipeline.token_cost.input` / `pipeline.token_cost.output` fields
//!    (pre-existing, accepted gap).** Both the doc comment on
//!    `record_node_complete` in `observability.rs` and
//!    `docs/spec/interfaces/advanced-features.md` §record_node_complete list
//!    these two fields in the emitted-fields table, but [`crate::types::TokenCost`]
//!    exposes only a single total-cost `f64` (`as_f64()`/`Display`) — there is
//!    no token-count breakdown reachable via the `token_cost: &TokenCost`
//!    parameter anywhere in the type's public API. This suite does not assert
//!    on those two fields (no fabricated values), per the task's explicit
//!    "known spec gap" instruction. A future task must either add an
//!    input/output breakdown to `TokenCost` (or a sibling type) or correct the
//!    doc tables to drop those two rows.
//! 2. **`pipeline.token_cost.total_usd` representation: numeric vs. string
//!    (newly identified, resolved by inference for this suite).** The field
//!    tables for `pipeline.run_id`, `pipeline.node_id`,
//!    `pipeline.retry.root_cause`, and `pipeline.rework.misalignment_type` all
//!    explicitly say "... as string" / "string representation of ...". The
//!    `pipeline.token_cost.total_usd` row says only "total cost in USD" — no
//!    "as string" qualifier. Given the OTel semantic-convention style used
//!    throughout this module (costs/counts as numeric attributes, identifiers
//!    as string attributes) and the asymmetry in the doc wording, this suite
//!    treats `pipeline.token_cost.total_usd` as a **numeric `f64` field**
//!    (`span.record("pipeline.token_cost.total_usd", value.as_f64())`), not a
//!    pre-formatted `Display` string. This is reinforced by
//!    `test_record_node_complete_with_high_precision_fractional_cost_not_truncated_to_six_decimals`:
//!    `TokenCost::Display` rounds to 6 decimal places (`"${:.6}"`), so
//!    recording the `Display` string instead of the raw `f64` would silently
//!    destroy precision below 1e-6 USD. If the Coder disagrees with this
//!    reading, flag it back to the architect rather than special-casing the
//!    implementation to dodge this test — it is the behaviourally significant
//!    choice, not an arbitrary assertion.

#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::float_cmp)]

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use proptest::prelude::*;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use uuid::Uuid;

use super::{RootCause, record_node_complete, record_node_start, record_retry, record_rework};
use crate::alignment::MisalignmentType;
use crate::identifiers::{NodeId, PipelineRunId};
use crate::types::TokenCost;

// ═══════════════════════════════════════════════════════════════════════════
// Test harness: span field capture
// ═══════════════════════════════════════════════════════════════════════════

/// A single captured field value, tagged by the `tracing::field::Visit`
/// method that produced it. Distinguishing `Str` from `F64` (rather than
/// collapsing both to a string) is load-bearing: it is how
/// `test_record_node_complete_sets_total_usd_field_to_token_cost_value_as_numeric`
/// catches an implementation that records a pre-formatted `Display` string
/// instead of the raw numeric value.
#[derive(Debug, Clone, PartialEq)]
enum FieldValue {
    Str(String),
    F64(f64),
    I64(i64),
    U64(u64),
    Bool(bool),
    Debug(String),
}

impl FieldValue {
    fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

type CapturedFields = Arc<Mutex<HashMap<String, FieldValue>>>;

/// Visitor that writes every visited field into the shared map, overwriting
/// any prior value for the same field name — this matches the real semantics
/// of `span.record()`, which replaces the previous value.
struct RecordingVisitor<'a> {
    map: &'a mut HashMap<String, FieldValue>,
}

impl Visit for RecordingVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.map
            .insert(field.name().to_string(), FieldValue::Str(value.to_string()));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.map
            .insert(field.name().to_string(), FieldValue::F64(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.map
            .insert(field.name().to_string(), FieldValue::I64(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.map
            .insert(field.name().to_string(), FieldValue::U64(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.map
            .insert(field.name().to_string(), FieldValue::Bool(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.map.insert(
            field.name().to_string(),
            FieldValue::Debug(format!("{value:?}")),
        );
    }
}

/// Minimal test-only `tracing_subscriber::Layer` that captures every field
/// recorded on any span — both at span-creation time (`on_new_span`) and via
/// subsequent `span.record()` calls (`on_record`) — into a shared map.
///
/// `tracing::field::Empty` placeholders deliberately do **not** invoke the
/// visitor (this is `tracing`'s own documented behaviour), so declaring a
/// field as `Empty` and never recording it leaves it permanently absent from
/// the captured map — which is exactly the property the field-isolation
/// tests below rely on.
#[derive(Clone)]
struct FieldCaptureLayer {
    captured: CapturedFields,
}

impl<S> Layer<S> for FieldCaptureLayer
where
    S: tracing::Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        let mut guard = self.captured.lock().expect("capture lock poisoned");
        let mut visitor = RecordingVisitor { map: &mut guard };
        attrs.record(&mut visitor);
    }

    fn on_record(&self, _id: &Id, values: &Record<'_>, _ctx: Context<'_, S>) {
        let mut guard = self.captured.lock().expect("capture lock poisoned");
        let mut visitor = RecordingVisitor { map: &mut guard };
        values.record(&mut visitor);
    }
}

/// Installs a fresh [`FieldCaptureLayer`] as the thread-local default
/// subscriber for the duration of `f`, then returns `f`'s result together
/// with every field captured during its execution.
///
/// Any span created (and any `span.record()` call made) *inside* `f` is
/// visible to the returned map. Spans created outside `f` are not tracked.
fn with_capture<R>(f: impl FnOnce() -> R) -> (R, HashMap<String, FieldValue>) {
    let captured: CapturedFields = Arc::new(Mutex::new(HashMap::new()));
    let layer = FieldCaptureLayer {
        captured: captured.clone(),
    };
    let subscriber = tracing_subscriber::registry().with(layer);
    let result = tracing::subscriber::with_default(subscriber, f);
    let map = captured.lock().expect("capture lock poisoned").clone();
    (result, map)
}

/// Superset span: declares every field any of the four hooks might record,
/// as `Empty`. Used as the default fixture so positive tests exercise a
/// realistic single-span-per-node-execution shape, and so field-isolation
/// tests can assert that a hook leaves *other* hooks' fields untouched.
fn full_span() -> tracing::Span {
    tracing::info_span!(
        "pipeline_node_execution",
        pipeline.run_id = tracing::field::Empty,
        pipeline.node_id = tracing::field::Empty,
        pipeline.event = tracing::field::Empty,
        pipeline.token_cost.total_usd = tracing::field::Empty,
        pipeline.retry.root_cause = tracing::field::Empty,
        pipeline.rework.misalignment_type = tracing::field::Empty,
    )
}

/// Minimal span declaring only the three fields `record_node_start` is
/// documented to write. Proves the hook does not implicitly depend on other
/// fields being declared on the span.
fn start_scoped_span() -> tracing::Span {
    tracing::info_span!(
        "pipeline_node_execution",
        pipeline.run_id = tracing::field::Empty,
        pipeline.node_id = tracing::field::Empty,
        pipeline.event = tracing::field::Empty,
    )
}

/// Minimal span declaring only the four fields `record_node_complete` is
/// documented to write (excluding the known-gap input/output fields).
fn complete_scoped_span() -> tracing::Span {
    tracing::info_span!(
        "pipeline_node_execution",
        pipeline.run_id = tracing::field::Empty,
        pipeline.node_id = tracing::field::Empty,
        pipeline.event = tracing::field::Empty,
        pipeline.token_cost.total_usd = tracing::field::Empty,
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// Fixtures
// ═══════════════════════════════════════════════════════════════════════════

fn run_id_seed(seed: u128) -> PipelineRunId {
    PipelineRunId::from_uuid(Uuid::from_u128(seed))
}

fn node_id(name: &str) -> NodeId {
    NodeId::new(name).expect("test node id literal must be non-empty")
}

fn cost(v: f64) -> TokenCost {
    TokenCost::new(v).expect("test token cost must be finite and non-negative")
}

const ALL_ROOT_CAUSES: [(RootCause, &str); 6] = [
    (RootCause::CompilationError, "compilation_error"),
    (RootCause::TestFailure, "test_failure"),
    (RootCause::ReviewFinding, "review_finding"),
    (RootCause::ConstraintViolation, "constraint_violation"),
    (RootCause::AlignmentFailure, "alignment_failure"),
    (RootCause::Timeout, "timeout"),
];

const ALL_MISALIGNMENT_TYPES: [(MisalignmentType, &str); 5] = [
    (MisalignmentType::Missing, "missing"),
    (MisalignmentType::Extra, "extra"),
    (MisalignmentType::Modified, "modified"),
    (MisalignmentType::Ambiguous, "ambiguous"),
    (MisalignmentType::ScopeExceeded, "scope_exceeded"),
];

// ═══════════════════════════════════════════════════════════════════════════
// Tier 1: Specification Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_record_node_start_sets_event_field_to_node_start() {
    let run_id = run_id_seed(1);
    let node = node_id("plan");

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_start(&run_id, &node, &span);
    });

    assert_eq!(
        fields.get("pipeline.event"),
        Some(&FieldValue::Str("node_start".to_string())),
        "pipeline.event must be exactly \"node_start\""
    );
}

#[test]
fn test_record_node_start_sets_run_id_field_to_run_id_string() {
    let run_id = run_id_seed(42);
    let node = node_id("plan");

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_start(&run_id, &node, &span);
    });

    let expected = run_id.to_string();
    assert_eq!(
        fields.get("pipeline.run_id"),
        Some(&FieldValue::Str(expected))
    );
}

#[test]
fn test_record_node_start_sets_node_id_field_to_node_id_string() {
    let run_id = run_id_seed(43);
    let node = node_id("code_generation");

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_start(&run_id, &node, &span);
    });

    assert_eq!(
        fields.get("pipeline.node_id"),
        Some(&FieldValue::Str("code_generation".to_string()))
    );
}

#[test]
fn test_record_node_complete_sets_event_field_to_node_complete() {
    let run_id = run_id_seed(2);
    let node = node_id("code_generation");
    let token_cost = cost(1.234_567);

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_complete(&run_id, &node, &token_cost, &span);
    });

    assert_eq!(
        fields.get("pipeline.event"),
        Some(&FieldValue::Str("node_complete".to_string()))
    );
}

#[test]
fn test_record_node_complete_sets_run_id_field_to_run_id_string() {
    let run_id = run_id_seed(44);
    let node = node_id("code_generation");
    let token_cost = cost(1.0);

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_complete(&run_id, &node, &token_cost, &span);
    });

    let expected = run_id.to_string();
    assert_eq!(
        fields.get("pipeline.run_id"),
        Some(&FieldValue::Str(expected))
    );
}

#[test]
fn test_record_node_complete_sets_node_id_field_to_node_id_string() {
    let run_id = run_id_seed(45);
    let node = node_id("integration");
    let token_cost = cost(1.0);

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_complete(&run_id, &node, &token_cost, &span);
    });

    assert_eq!(
        fields.get("pipeline.node_id"),
        Some(&FieldValue::Str("integration".to_string()))
    );
}

/// Pins down the type of `pipeline.token_cost.total_usd` as a numeric `f64`
/// field (see spec-gap note #2 in the module doc comment), not a
/// pre-formatted `Display` string.
#[test]
fn test_record_node_complete_sets_total_usd_field_to_token_cost_value_as_numeric() {
    let run_id = run_id_seed(3);
    let node = node_id("code_generation");
    let token_cost = cost(2.5);

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_complete(&run_id, &node, &token_cost, &span);
    });

    match fields.get("pipeline.token_cost.total_usd") {
        Some(FieldValue::F64(v)) => assert_eq!(*v, token_cost.as_f64()),
        other => panic!(
            "expected pipeline.token_cost.total_usd to be recorded as a numeric f64 field, got {other:?}"
        ),
    }
}

#[test]
fn test_record_retry_sets_event_field_to_node_retry() {
    let run_id = run_id_seed(4);
    let node = node_id("implementation");
    let cause = RootCause::CompilationError;

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_retry(&run_id, &node, &cause, &span);
    });

    assert_eq!(
        fields.get("pipeline.event"),
        Some(&FieldValue::Str("node_retry".to_string()))
    );
}

#[test]
fn test_record_retry_sets_run_id_field_to_run_id_string() {
    let run_id = run_id_seed(46);
    let node = node_id("implementation");
    let cause = RootCause::TestFailure;

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_retry(&run_id, &node, &cause, &span);
    });

    let expected = run_id.to_string();
    assert_eq!(
        fields.get("pipeline.run_id"),
        Some(&FieldValue::Str(expected))
    );
}

#[test]
fn test_record_retry_sets_node_id_field_to_node_id_string() {
    let run_id = run_id_seed(47);
    let node = node_id("implementation");
    let cause = RootCause::Timeout;

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_retry(&run_id, &node, &cause, &span);
    });

    assert_eq!(
        fields.get("pipeline.node_id"),
        Some(&FieldValue::Str("implementation".to_string()))
    );
}

#[test]
fn test_record_retry_sets_root_cause_field_to_display_string() {
    let run_id = run_id_seed(5);
    let node = node_id("implementation");
    let cause = RootCause::ReviewFinding;

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_retry(&run_id, &node, &cause, &span);
    });

    assert_eq!(
        fields.get("pipeline.retry.root_cause"),
        Some(&FieldValue::Str(cause.to_string()))
    );
}

#[test]
fn test_record_rework_sets_event_field_to_node_rework() {
    let run_id = run_id_seed(6);
    let node = node_id("integration");
    let misalignment = MisalignmentType::Missing;

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_rework(&run_id, &node, &misalignment, &span);
    });

    assert_eq!(
        fields.get("pipeline.event"),
        Some(&FieldValue::Str("node_rework".to_string()))
    );
}

#[test]
fn test_record_rework_sets_run_id_field_to_run_id_string() {
    let run_id = run_id_seed(48);
    let node = node_id("integration");
    let misalignment = MisalignmentType::Extra;

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_rework(&run_id, &node, &misalignment, &span);
    });

    let expected = run_id.to_string();
    assert_eq!(
        fields.get("pipeline.run_id"),
        Some(&FieldValue::Str(expected))
    );
}

#[test]
fn test_record_rework_sets_node_id_field_to_node_id_string() {
    let run_id = run_id_seed(49);
    let node = node_id("integration");
    let misalignment = MisalignmentType::Modified;

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_rework(&run_id, &node, &misalignment, &span);
    });

    assert_eq!(
        fields.get("pipeline.node_id"),
        Some(&FieldValue::Str("integration".to_string()))
    );
}

#[test]
fn test_record_rework_sets_misalignment_type_field_to_display_string() {
    let run_id = run_id_seed(7);
    let node = node_id("integration");
    let misalignment = MisalignmentType::ScopeExceeded;

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_rework(&run_id, &node, &misalignment, &span);
    });

    assert_eq!(
        fields.get("pipeline.rework.misalignment_type"),
        Some(&FieldValue::Str(misalignment.to_string()))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 1: Contract Tests — Display implementations consumed by this module
// ═══════════════════════════════════════════════════════════════════════════

/// Regression lock on `RootCause`'s OTel attribute string values. These
/// strings flow directly into `pipeline.retry.root_cause` and are consumed by
/// dashboards; any accidental rename (e.g. via a refactor) must fail here
/// before it reaches `record_retry`'s own tests.
#[test]
fn test_root_cause_display_matches_otel_snake_case_names_for_all_variants() {
    for (variant, expected) in ALL_ROOT_CAUSES {
        assert_eq!(
            variant.to_string(),
            expected,
            "RootCause::{variant:?} Display mismatch"
        );
    }
}

/// Regression lock on `MisalignmentType`'s OTel attribute string values,
/// consumed by `pipeline.rework.misalignment_type`.
#[test]
fn test_misalignment_type_display_matches_otel_snake_case_names_for_all_variants() {
    for (variant, expected) in ALL_MISALIGNMENT_TYPES {
        assert_eq!(
            variant.to_string(),
            expected,
            "MisalignmentType::{variant:?} Display mismatch"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 2: Adversarial / Boundary Tests
// ═══════════════════════════════════════════════════════════════════════════

// --- Field isolation: each hook must only ever write its own fields -------

#[test]
fn test_record_node_start_does_not_record_fields_owned_by_other_hooks() {
    let run_id = run_id_seed(50);
    let node = node_id("plan");

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_start(&run_id, &node, &span);
    });

    assert!(!fields.contains_key("pipeline.token_cost.total_usd"));
    assert!(!fields.contains_key("pipeline.retry.root_cause"));
    assert!(!fields.contains_key("pipeline.rework.misalignment_type"));
}

#[test]
fn test_record_node_complete_does_not_record_fields_owned_by_other_hooks() {
    let run_id = run_id_seed(51);
    let node = node_id("code_generation");
    let token_cost = cost(0.5);

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_complete(&run_id, &node, &token_cost, &span);
    });

    assert!(!fields.contains_key("pipeline.retry.root_cause"));
    assert!(!fields.contains_key("pipeline.rework.misalignment_type"));
}

#[test]
fn test_record_retry_does_not_record_fields_owned_by_other_hooks() {
    let run_id = run_id_seed(52);
    let node = node_id("implementation");
    let cause = RootCause::ConstraintViolation;

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_retry(&run_id, &node, &cause, &span);
    });

    assert!(!fields.contains_key("pipeline.token_cost.total_usd"));
    assert!(!fields.contains_key("pipeline.rework.misalignment_type"));
}

#[test]
fn test_record_rework_does_not_record_fields_owned_by_other_hooks() {
    let run_id = run_id_seed(53);
    let node = node_id("integration");
    let misalignment = MisalignmentType::Ambiguous;

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_rework(&run_id, &node, &misalignment, &span);
    });

    assert!(!fields.contains_key("pipeline.token_cost.total_usd"));
    assert!(!fields.contains_key("pipeline.retry.root_cause"));
}

// --- Distinct event values: catches copy-paste / swapped-literal bugs ----

#[test]
fn test_all_four_hook_functions_emit_pairwise_distinct_event_values() {
    let run_id = run_id_seed(54);
    let node = node_id("plan");
    let token_cost = cost(0.0);
    let cause = RootCause::Timeout;
    let misalignment = MisalignmentType::Missing;

    let ((), start_fields) = with_capture(|| {
        let span = full_span();
        record_node_start(&run_id, &node, &span);
    });
    let ((), complete_fields) = with_capture(|| {
        let span = full_span();
        record_node_complete(&run_id, &node, &token_cost, &span);
    });
    let ((), retry_fields) = with_capture(|| {
        let span = full_span();
        record_retry(&run_id, &node, &cause, &span);
    });
    let ((), rework_fields) = with_capture(|| {
        let span = full_span();
        record_rework(&run_id, &node, &misalignment, &span);
    });

    let events = [
        start_fields
            .get("pipeline.event")
            .and_then(FieldValue::as_str)
            .unwrap_or_default(),
        complete_fields
            .get("pipeline.event")
            .and_then(FieldValue::as_str)
            .unwrap_or_default(),
        retry_fields
            .get("pipeline.event")
            .and_then(FieldValue::as_str)
            .unwrap_or_default(),
        rework_fields
            .get("pipeline.event")
            .and_then(FieldValue::as_str)
            .unwrap_or_default(),
    ];
    let mut unique: Vec<&str> = events.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        4,
        "expected 4 pairwise-distinct event values across the four hooks, got {events:?}"
    );
}

// --- Disabled span: must never panic (explicit acceptance criterion) ------

#[test]
fn test_record_node_start_does_not_panic_with_disabled_span() {
    let run_id = run_id_seed(60);
    let node = node_id("plan");
    let span = tracing::Span::none();

    record_node_start(&run_id, &node, &span);
}

#[test]
fn test_record_node_complete_does_not_panic_with_disabled_span() {
    let run_id = run_id_seed(61);
    let node = node_id("code_generation");
    let token_cost = cost(1.0);
    let span = tracing::Span::none();

    record_node_complete(&run_id, &node, &token_cost, &span);
}

#[test]
fn test_record_retry_does_not_panic_with_disabled_span() {
    let run_id = run_id_seed(62);
    let node = node_id("implementation");
    let cause = RootCause::AlignmentFailure;
    let span = tracing::Span::none();

    record_retry(&run_id, &node, &cause, &span);
}

#[test]
fn test_record_rework_does_not_panic_with_disabled_span() {
    let run_id = run_id_seed(63);
    let node = node_id("integration");
    let misalignment = MisalignmentType::Extra;
    let span = tracing::Span::none();

    record_rework(&run_id, &node, &misalignment, &span);
}

// --- Anti-memoisation: a second call on the same span must overwrite ------

#[test]
fn test_record_node_start_field_values_reflect_latest_call_not_cached_from_first_call() {
    let run_id_a = run_id_seed(70);
    let node_a = node_id("plan");
    let run_id_b = run_id_seed(71);
    let node_b = node_id("integration");

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_start(&run_id_a, &node_a, &span);
        record_node_start(&run_id_b, &node_b, &span);
    });

    assert_eq!(
        fields.get("pipeline.run_id"),
        Some(&FieldValue::Str(run_id_b.to_string()))
    );
    assert_eq!(
        fields.get("pipeline.node_id"),
        Some(&FieldValue::Str(node_b.to_string()))
    );
}

#[test]
fn test_record_node_complete_total_usd_field_reflects_latest_call_not_cached_from_first_call() {
    let run_id = run_id_seed(72);
    let node = node_id("code_generation");
    let token_cost_a = cost(1.0);
    let token_cost_b = cost(9.75);

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_complete(&run_id, &node, &token_cost_a, &span);
        record_node_complete(&run_id, &node, &token_cost_b, &span);
    });

    match fields.get("pipeline.token_cost.total_usd") {
        Some(FieldValue::F64(v)) => assert_eq!(*v, token_cost_b.as_f64()),
        other => panic!("expected numeric field reflecting second call, got {other:?}"),
    }
}

#[test]
fn test_record_retry_root_cause_field_reflects_latest_call_not_cached_from_first_call() {
    let run_id = run_id_seed(73);
    let node = node_id("implementation");
    let cause_a = RootCause::CompilationError;
    let cause_b = RootCause::Timeout;

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_retry(&run_id, &node, &cause_a, &span);
        record_retry(&run_id, &node, &cause_b, &span);
    });

    assert_eq!(
        fields.get("pipeline.retry.root_cause"),
        Some(&FieldValue::Str(cause_b.to_string()))
    );
}

#[test]
fn test_record_rework_misalignment_field_reflects_latest_call_not_cached_from_first_call() {
    let run_id = run_id_seed(74);
    let node = node_id("integration");
    let misalignment_a = MisalignmentType::Missing;
    let misalignment_b = MisalignmentType::ScopeExceeded;

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_rework(&run_id, &node, &misalignment_a, &span);
        record_rework(&run_id, &node, &misalignment_b, &span);
    });

    assert_eq!(
        fields.get("pipeline.rework.misalignment_type"),
        Some(&FieldValue::Str(misalignment_b.to_string()))
    );
}

// --- Boundary values -------------------------------------------------------

#[test]
fn test_record_node_start_with_minimal_single_character_node_id_records_value_verbatim() {
    let run_id = run_id_seed(80);
    let node = node_id("x");

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_start(&run_id, &node, &span);
    });

    assert_eq!(
        fields.get("pipeline.node_id"),
        Some(&FieldValue::Str("x".to_string()))
    );
}

#[test]
fn test_record_node_start_with_nil_uuid_run_id_records_value_verbatim() {
    let run_id = PipelineRunId::from_uuid(Uuid::nil());
    let node = node_id("plan");

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_start(&run_id, &node, &span);
    });

    assert_eq!(
        fields.get("pipeline.run_id"),
        Some(&FieldValue::Str(
            "00000000-0000-0000-0000-000000000000".to_string()
        ))
    );
}

#[test]
fn test_record_node_start_node_id_with_special_characters_preserved_verbatim() {
    let run_id = run_id_seed(81);
    let node = node_id("sub/task.v2-final_build");

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_start(&run_id, &node, &span);
    });

    assert_eq!(
        fields.get("pipeline.node_id"),
        Some(&FieldValue::Str("sub/task.v2-final_build".to_string()))
    );
}

#[test]
fn test_record_node_complete_with_zero_token_cost_records_zero() {
    let run_id = run_id_seed(82);
    let node = node_id("plan");
    let token_cost = TokenCost::zero();

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_complete(&run_id, &node, &token_cost, &span);
    });

    match fields.get("pipeline.token_cost.total_usd") {
        Some(FieldValue::F64(v)) => assert_eq!(*v, 0.0),
        other => panic!("expected numeric 0.0, got {other:?}"),
    }
}

/// `TokenCost::Display` rounds to 6 decimal places (`"${:.6}"`). If the
/// implementation recorded the formatted `Display` string instead of the raw
/// `f64`, this 10-decimal-place value would be silently rounded away (and
/// `$`-prefixed), and this test would fail.
#[test]
fn test_record_node_complete_with_high_precision_fractional_cost_not_truncated_to_six_decimals() {
    let run_id = run_id_seed(83);
    let node = node_id("plan");
    let token_cost = cost(0.000_000_123_4);

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_complete(&run_id, &node, &token_cost, &span);
    });

    match fields.get("pipeline.token_cost.total_usd") {
        Some(FieldValue::F64(v)) => assert_eq!(*v, 0.000_000_123_4),
        other => panic!("expected high-precision numeric value, got {other:?}"),
    }
}

#[test]
fn test_record_node_complete_with_large_token_cost_preserves_value() {
    let run_id = run_id_seed(84);
    let node = node_id("plan");
    let token_cost = cost(123_456.789_012);

    let ((), fields) = with_capture(|| {
        let span = full_span();
        record_node_complete(&run_id, &node, &token_cost, &span);
    });

    match fields.get("pipeline.token_cost.total_usd") {
        Some(FieldValue::F64(v)) => assert_eq!(*v, 123_456.789_012),
        other => panic!("expected large numeric value, got {other:?}"),
    }
}

// --- Variant tables: exercise the actual function call, not just Display --

#[test]
fn test_record_retry_maps_each_root_cause_variant_to_correct_field_value() {
    for (cause, expected) in ALL_ROOT_CAUSES {
        let run_id = run_id_seed(90);
        let node = node_id("implementation");

        let ((), fields) = with_capture(|| {
            let span = full_span();
            record_retry(&run_id, &node, &cause, &span);
        });

        assert_eq!(
            fields.get("pipeline.retry.root_cause"),
            Some(&FieldValue::Str(expected.to_string())),
            "RootCause::{cause:?} should record field value {expected:?}"
        );
    }
}

#[test]
fn test_record_rework_maps_each_misalignment_type_variant_to_correct_field_value() {
    for (misalignment, expected) in ALL_MISALIGNMENT_TYPES {
        let run_id = run_id_seed(91);
        let node = node_id("integration");

        let ((), fields) = with_capture(|| {
            let span = full_span();
            record_rework(&run_id, &node, &misalignment, &span);
        });

        assert_eq!(
            fields.get("pipeline.rework.misalignment_type"),
            Some(&FieldValue::Str(expected.to_string())),
            "MisalignmentType::{misalignment:?} should record field value {expected:?}"
        );
    }
}

// --- Minimal-span robustness: hook must not depend on unrelated fields ----

#[test]
fn test_record_node_start_works_with_minimally_scoped_span_declaring_only_its_own_fields() {
    let run_id = run_id_seed(100);
    let node = node_id("plan");

    let ((), fields) = with_capture(|| {
        let span = start_scoped_span();
        record_node_start(&run_id, &node, &span);
    });

    assert_eq!(
        fields.get("pipeline.event"),
        Some(&FieldValue::Str("node_start".to_string()))
    );
}

#[test]
fn test_record_node_complete_works_with_minimally_scoped_span_declaring_only_its_own_fields() {
    let run_id = run_id_seed(101);
    let node = node_id("plan");
    let token_cost = cost(1.0);

    let ((), fields) = with_capture(|| {
        let span = complete_scoped_span();
        record_node_complete(&run_id, &node, &token_cost, &span);
    });

    assert_eq!(
        fields.get("pipeline.event"),
        Some(&FieldValue::Str("node_complete".to_string()))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 3: Property-Based Tests
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    /// No valid `(run_id, node)` combination may cause `record_node_start` to
    /// panic, even against a disabled span.
    #[test]
    fn prop_record_node_start_never_panics_for_arbitrary_inputs(
        seed in any::<u128>(),
        node_name in "[a-zA-Z0-9_./-]{1,40}",
    ) {
        let run_id = PipelineRunId::from_uuid(Uuid::from_u128(seed));
        let node = NodeId::new(node_name).expect("non-empty by regex construction");
        let span = tracing::Span::none();

        record_node_start(&run_id, &node, &span);
    }

    /// For any generated `(run_id, node)`, `record_node_start` must set
    /// `pipeline.run_id` / `pipeline.node_id` to the exact `Display` strings
    /// of the inputs, and `pipeline.event` to the literal `"node_start"`.
    #[test]
    fn prop_record_node_start_fields_always_equal_input_display_strings(
        seed in any::<u128>(),
        node_name in "[a-zA-Z0-9_./-]{1,40}",
    ) {
        let run_id = PipelineRunId::from_uuid(Uuid::from_u128(seed));
        let node = NodeId::new(node_name).expect("non-empty by regex construction");

        let ((), fields) = with_capture(|| {
            let span = full_span();
            record_node_start(&run_id, &node, &span);
        });

        prop_assert_eq!(fields.get("pipeline.run_id"), Some(&FieldValue::Str(run_id.to_string())));
        prop_assert_eq!(fields.get("pipeline.node_id"), Some(&FieldValue::Str(node.to_string())));
        prop_assert_eq!(fields.get("pipeline.event"), Some(&FieldValue::Str("node_start".to_string())));
    }

    /// For any finite non-negative `TokenCost`, `pipeline.token_cost.total_usd`
    /// must be recorded as a numeric field whose value exactly equals
    /// `token_cost.as_f64()` — a pure passthrough with no rounding or
    /// re-formatting.
    #[test]
    fn prop_record_node_complete_total_usd_field_always_equals_input_token_cost(
        value in 0.0f64..1_000_000_000.0,
    ) {
        let run_id = PipelineRunId::new_random();
        let node = NodeId::new("code_generation").expect("non-empty literal");
        let token_cost = TokenCost::new(value)
            .expect("finite and non-negative by range construction");

        let ((), fields) = with_capture(|| {
            let span = full_span();
            record_node_complete(&run_id, &node, &token_cost, &span);
        });

        let total_usd = match fields.get("pipeline.token_cost.total_usd") {
            Some(FieldValue::F64(v)) => *v,
            other => panic!("expected numeric pipeline.token_cost.total_usd field, got {other:?}"),
        };
        prop_assert_eq!(total_usd, value);
    }

    /// For every `RootCause` variant, `pipeline.retry.root_cause` must equal
    /// that variant's `Display` string.
    #[test]
    fn prop_record_retry_root_cause_field_always_equals_display_of_generated_variant(
        variant_idx in 0usize..6,
    ) {
        let (cause, _) = ALL_ROOT_CAUSES[variant_idx];
        let run_id = PipelineRunId::new_random();
        let node = NodeId::new("implementation").expect("non-empty literal");

        let ((), fields) = with_capture(|| {
            let span = full_span();
            record_retry(&run_id, &node, &cause, &span);
        });

        prop_assert_eq!(
            fields.get("pipeline.retry.root_cause"),
            Some(&FieldValue::Str(cause.to_string()))
        );
    }

    /// For every `MisalignmentType` variant, `pipeline.rework.misalignment_type`
    /// must equal that variant's `Display` string.
    #[test]
    fn prop_record_rework_misalignment_field_always_equals_display_of_generated_variant(
        variant_idx in 0usize..5,
    ) {
        let (misalignment, _) = ALL_MISALIGNMENT_TYPES[variant_idx];
        let run_id = PipelineRunId::new_random();
        let node = NodeId::new("integration").expect("non-empty literal");

        let ((), fields) = with_capture(|| {
            let span = full_span();
            record_rework(&run_id, &node, &misalignment, &span);
        });

        prop_assert_eq!(
            fields.get("pipeline.rework.misalignment_type"),
            Some(&FieldValue::Str(misalignment.to_string()))
        );
    }

    /// Combined panic-freedom property: calling all four hooks back-to-back
    /// against a disabled span, for arbitrary generated inputs spanning the
    /// full valid domain of each parameter type, must never panic.
    #[test]
    fn prop_all_four_hooks_never_panic_for_arbitrary_valid_inputs(
        seed in any::<u128>(),
        node_name in "[a-zA-Z0-9_./-]{1,40}",
        cost_value in 0.0f64..1_000_000_000.0,
        cause_idx in 0usize..6,
        misalignment_idx in 0usize..5,
    ) {
        let run_id = PipelineRunId::from_uuid(Uuid::from_u128(seed));
        let node = NodeId::new(node_name).expect("non-empty by regex construction");
        let token_cost = TokenCost::new(cost_value)
            .expect("finite and non-negative by range construction");
        let (cause, _) = ALL_ROOT_CAUSES[cause_idx];
        let (misalignment, _) = ALL_MISALIGNMENT_TYPES[misalignment_idx];

        let span = tracing::Span::none();
        record_node_start(&run_id, &node, &span);
        record_node_complete(&run_id, &node, &token_cost, &span);
        record_retry(&run_id, &node, &cause, &span);
        record_rework(&run_id, &node, &misalignment, &span);
    }
}
