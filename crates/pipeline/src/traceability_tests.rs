//! Adversarial test suite for `traceability.rs` — requirement extraction and
//! traceability matrix updates.
//!
//! Functions under test:
//! - [`extract_requirements`] — scans a GitHub issue body for `REQ-NNN:` tags.
//! - [`update_traceability_matrix`] — applies stage coverage flags and
//!   recomputes row/matrix status.
//!
//! Also includes contract/regression tests for the already-implemented
//! [`RequirementRow::compute_status`] and the [`TraceabilityStage`] `Display`
//! impl, since `update_traceability_matrix` depends on both and mutations to
//! either would otherwise go undetected by this suite.
//!
//! ## Tier map
//!
//! | Target | Tier 1 (spec) | Tier 2 (adversarial) | Tier 3 (proptest) |
//! |---|---|---|---|
//! | Contract: `compute_status` / `Display` | 9 | - | - |
//! | `extract_requirements` | 12 | 16 | 3 |
//! | `update_traceability_matrix` | 13 | 10 | 4 |
//!
//! Both `extract_requirements` and `update_traceability_matrix` are `todo!()`
//! stubs as of this writing. Every test below compiles (GREEN) but panics at
//! runtime (RED) until the implementation is written.
//!
//! ## Spec gaps noted (see task report for the Tester's full write-up)
//!
//! - The contract says `id` captures "exactly as it appears in the text", so
//!   this suite treats the `id` field as verbatim-preserving case (e.g.
//!   `"req-002"` stays `"req-002"`, not normalised to `"REQ-002"`). If the
//!   intended behaviour is case-normalisation, the tests asserting verbatim
//!   preservation (`test_extract_requirements_lowercase_prefix_is_recognized_case_insensitively`,
//!   `test_extract_requirements_mixed_case_prefix_preserved_verbatim`) will need
//!   architect clarification.
//! - Whether characters between the digits and the colon (e.g. `"REQ-001a:"`)
//!   invalidate the match is unspecified. This suite does not test that case
//!   to avoid locking in an unconfirmed assumption.
//! - Whether Unicode (non-ASCII) digit characters count as `\d` for the digit
//!   run is unspecified; only covered indirectly by the no-panic property test.

#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use proptest::prelude::*;

use super::{
    Requirement, RequirementRow, TraceabilityMatrix, TraceabilityStage, TraceabilityStatus,
    extract_requirements, update_traceability_matrix,
};
use crate::alignment::TraceabilityEntry;
use crate::identifiers::{PipelineRunId, WorkItemId};

// ─── Test helpers ────────────────────────────────────────────────────────────

fn make_requirement(id: &str, description: &str) -> Requirement {
    Requirement {
        id: id.to_string(),
        description: description.to_string(),
        source_work_item: WorkItemId::new(0),
    }
}

#[allow(clippy::fn_params_excessive_bools)]
fn make_row(id: &str, arch: bool, iface: bool, swi: bool, code: bool) -> RequirementRow {
    RequirementRow {
        requirement: make_requirement(id, "placeholder description"),
        architecture_covered: arch,
        interface_covered: iface,
        sub_work_item_covered: swi,
        code_covered: code,
        status: TraceabilityStatus::Missing,
    }
}

fn make_matrix(rows: Vec<RequirementRow>) -> TraceabilityMatrix {
    TraceabilityMatrix {
        rows,
        pipeline_run_id: PipelineRunId::new_random(),
        work_item_id: WorkItemId::new(42),
        complete: false,
    }
}

fn make_entry(requirement_ref: &str, output_ref: &str) -> TraceabilityEntry {
    TraceabilityEntry {
        requirement_ref: requirement_ref.to_string(),
        output_ref: output_ref.to_string(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Contract tests: RequirementRow::compute_status / TraceabilityStage Display
// (already implemented — locked in here because `update_traceability_matrix`
// depends on both and mutations to either must be caught by this suite)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_compute_status_all_flags_false_is_missing() {
    let row = make_row("REQ-001", false, false, false, false);
    assert_eq!(row.compute_status(), TraceabilityStatus::Missing);
}

#[test]
fn test_compute_status_one_flag_true_is_partial() {
    let row = make_row("REQ-001", true, false, false, false);
    assert_eq!(row.compute_status(), TraceabilityStatus::Partial);
}

#[test]
fn test_compute_status_two_flags_true_is_partial() {
    let row = make_row("REQ-001", true, true, false, false);
    assert_eq!(row.compute_status(), TraceabilityStatus::Partial);
}

#[test]
fn test_compute_status_three_flags_true_is_partial() {
    let row = make_row("REQ-001", true, true, true, false);
    assert_eq!(row.compute_status(), TraceabilityStatus::Partial);
}

#[test]
fn test_compute_status_all_flags_true_is_complete() {
    let row = make_row("REQ-001", true, true, true, true);
    assert_eq!(row.compute_status(), TraceabilityStatus::Complete);
}

#[test]
fn test_traceability_stage_display_architecture() {
    assert_eq!(TraceabilityStage::Architecture.to_string(), "architecture");
}

#[test]
fn test_traceability_stage_display_interface() {
    assert_eq!(TraceabilityStage::Interface.to_string(), "interface");
}

#[test]
fn test_traceability_stage_display_sub_work_item() {
    assert_eq!(TraceabilityStage::SubWorkItem.to_string(), "sub_work_item");
}

#[test]
fn test_traceability_stage_display_code() {
    assert_eq!(TraceabilityStage::Code.to_string(), "code");
}

// ═══════════════════════════════════════════════════════════════════════════
// extract_requirements
// ═══════════════════════════════════════════════════════════════════════════

// ─── Tier 1: Specification Tests ──────────────────────────────────────────────

#[test]
fn test_extract_requirements_single_match_extracts_id_and_description() {
    let body = "REQ-001: System must validate inputs";
    let reqs = extract_requirements(body);
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].id, "REQ-001");
    assert_eq!(reqs[0].description, "System must validate inputs");
}

#[test]
fn test_extract_requirements_multiple_matches_returns_all_in_order() {
    let body = "## Requirements\nREQ-001: System must validate inputs\nREQ-002: Errors must be logged";
    let reqs = extract_requirements(body);
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].id, "REQ-001");
    assert_eq!(reqs[1].id, "REQ-002");
    assert_eq!(reqs[1].description, "Errors must be logged");
}

#[test]
fn test_extract_requirements_no_matches_returns_empty_vec() {
    let body = "This issue has no requirement tags at all.";
    assert!(extract_requirements(body).is_empty());
}

#[test]
fn test_extract_requirements_empty_string_returns_empty_vec() {
    assert!(extract_requirements("").is_empty());
}

#[test]
fn test_extract_requirements_sets_sentinel_work_item_id() {
    let reqs = extract_requirements("REQ-001: text");
    assert_eq!(reqs[0].source_work_item, WorkItemId::new(0));
}

#[test]
fn test_extract_requirements_trims_surrounding_whitespace_in_description() {
    let reqs = extract_requirements("REQ-001:    padded description   ");
    assert_eq!(reqs[0].description, "padded description");
}

#[test]
fn test_extract_requirements_lowercase_prefix_is_recognized_case_insensitively() {
    let reqs = extract_requirements("req-002: lowercase tag body");
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].id, "req-002");
}

#[test]
fn test_extract_requirements_mixed_case_prefix_preserved_verbatim() {
    let reqs = extract_requirements("Req-003: Mixed case tag");
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].id, "Req-003");
}

#[test]
fn test_extract_requirements_description_may_contain_additional_colon() {
    let reqs = extract_requirements("REQ-001: System must: validate and log inputs");
    assert_eq!(reqs[0].description, "System must: validate and log inputs");
}

#[test]
fn test_extract_requirements_empty_description_after_colon() {
    let reqs = extract_requirements("REQ-001:");
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].description, "");
}

#[test]
fn test_extract_requirements_tag_not_anchored_to_line_start_still_matches() {
    let reqs = extract_requirements("- REQ-001: bullet list item requirement");
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].id, "REQ-001");
}

#[test]
fn test_extract_requirements_tag_embedded_mid_sentence_still_matches() {
    let reqs = extract_requirements("See REQ-003: the described requirement for context");
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].id, "REQ-003");
}

// ─── Tier 2: Boundary & Adversarial Tests ─────────────────────────────────────

#[test]
fn test_extract_requirements_malformed_no_digits_ignored() {
    let reqs = extract_requirements("REQ-: missing digits entirely");
    assert!(reqs.is_empty());
}

#[test]
fn test_extract_requirements_malformed_no_hyphen_ignored() {
    let reqs = extract_requirements("REQ001: missing hyphen separator");
    assert!(reqs.is_empty());
}

#[test]
fn test_extract_requirements_malformed_no_colon_ignored() {
    let reqs = extract_requirements("REQ-001 missing colon separator");
    assert!(reqs.is_empty());
}

#[test]
fn test_extract_requirements_malformed_non_digit_suffix_ignored() {
    let reqs = extract_requirements("REQ-ABC: letters instead of digits");
    assert!(reqs.is_empty());
}

#[test]
fn test_extract_requirements_single_char_r_line_does_not_panic() {
    // Mutation-kill: `match_tag_at`'s prefix-length bounds check
    // (`start + PREFIX_LEN > bytes.len()`) must reject `start == 0` on a line
    // shorter than 4 bytes *before* indexing past the end of the slice. A
    // 1-byte line consisting of a single `R` (matching the first prefix byte,
    // case-insensitively) would panic on out-of-bounds access if that bounds
    // check were broken (e.g. `start * PREFIX_LEN` instead of `start +
    // PREFIX_LEN`, which is always `0` at `start == 0` and so never trips).
    let reqs = extract_requirements("R");
    assert!(reqs.is_empty());
}

#[test]
fn test_extract_requirements_two_and_three_char_lines_starting_with_r_do_not_panic() {
    // Companion boundary cases to the 1-byte case above: lines of length 2
    // and 3 starting with the prefix's first byte must also be safely
    // rejected without panicking, for every too-short length.
    assert!(extract_requirements("RE").is_empty());
    assert!(extract_requirements("REQ").is_empty());
}

#[test]
fn test_extract_requirements_single_digit_id_is_minimum_valid_boundary() {
    let reqs = extract_requirements("REQ-1: minimal digit count");
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].id, "REQ-1");
}

#[test]
fn test_extract_requirements_many_digit_id_preserves_full_number() {
    let reqs = extract_requirements("REQ-123456: large numeric id");
    assert_eq!(reqs[0].id, "REQ-123456");
}

#[test]
fn test_extract_requirements_leading_zeros_preserved_verbatim() {
    let reqs = extract_requirements("REQ-007: leading zeros preserved");
    assert_eq!(reqs[0].id, "REQ-007");
}

#[test]
fn test_extract_requirements_duplicate_ids_produce_separate_rows() {
    let body = "REQ-001: first occurrence\nREQ-001: second occurrence";
    let reqs = extract_requirements(body);
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].description, "first occurrence");
    assert_eq!(reqs[1].description, "second occurrence");
}

#[test]
fn test_extract_requirements_description_does_not_span_multiple_lines() {
    let body = "REQ-001: first line of description\nthis continuation line is not part of the description";
    let reqs = extract_requirements(body);
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].description, "first line of description");
}

#[test]
fn test_extract_requirements_blank_lines_and_headers_interspersed() {
    let body = "# Title\n\nREQ-001: first\n\n## Section\nREQ-002: second\n";
    let reqs = extract_requirements(body);
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].id, "REQ-001");
    assert_eq!(reqs[1].id, "REQ-002");
}

#[test]
fn test_extract_requirements_crlf_line_endings_do_not_corrupt_description() {
    let body = "REQ-001: windows line ending\r\nREQ-002: next requirement";
    let reqs = extract_requirements(body);
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].description, "windows line ending");
    assert_eq!(reqs[1].description, "next requirement");
}

#[test]
fn test_extract_requirements_unicode_description_preserved() {
    let body = "REQ-001: supports multiple languages: \u{652f}\u{6301}\u{591a}\u{8bed}\u{8a00} and emoji \u{1f680}";
    let reqs = extract_requirements(body);
    assert_eq!(reqs.len(), 1);
    assert_eq!(
        reqs[0].description,
        "supports multiple languages: \u{652f}\u{6301}\u{591a}\u{8bed}\u{8a00} and emoji \u{1f680}"
    );
}

#[test]
fn test_extract_requirements_very_long_adversarial_input_does_not_hang() {
    let pathological = "REQ-".repeat(50_000);
    let reqs = extract_requirements(&pathological);
    assert!(reqs.is_empty());
}

#[test]
fn test_extract_requirements_unmatched_req_fragments_ignored_without_panic() {
    let body = "REQ-REQ-REQ-: : : REQ-REQ nonsense REQ-001";
    let reqs = extract_requirements(body);
    // "REQ-001" has no trailing colon on this line, so it must not match either.
    assert!(reqs.is_empty());
}

#[test]
fn test_extract_requirements_distinct_counts_for_distinct_inputs_not_hardcoded() {
    let five = extract_requirements("REQ-001: a\nREQ-002: b\nREQ-003: c\nREQ-004: d\nREQ-005: e");
    let two = extract_requirements("REQ-010: x\nREQ-020: y");
    assert_eq!(five.len(), 5);
    assert_eq!(two.len(), 2);
    assert_ne!(five.len(), two.len());
}

// ─── Tier 3: Property-Based Tests ─────────────────────────────────────────────

proptest! {
    /// Fuzz: `extract_requirements` must never panic on arbitrary multiline
    /// input, including pathological-looking text (long runs of `REQ-`,
    /// unmatched fragments, embedded control characters).
    #[test]
    fn test_extract_requirements_never_panics_on_arbitrary_multiline_input(
        body in "(?s).{0,500}",
    ) {
        let _ = extract_requirements(&body);
    }

    /// `source_work_item` must always be the sentinel `WorkItemId::new(0)`,
    /// regardless of input shape — callers rely on this to detect "not yet
    /// assigned" before patching in the real work item id.
    #[test]
    fn test_extract_requirements_source_work_item_is_always_sentinel_zero(
        body in "(?s).{0,500}",
    ) {
        let reqs = extract_requirements(&body);
        for r in &reqs {
            prop_assert_eq!(r.source_work_item, WorkItemId::new(0));
        }
    }

    /// Round-trip: for text composed entirely of well-formed `REQ-<digits>: <desc>`
    /// lines, every line must produce exactly one `Requirement` with the
    /// matching `id` and `description`, in the same order. This kills stubs
    /// that return a fixed-size or empty `Vec` regardless of input.
    #[test]
    fn test_extract_requirements_round_trip_on_generated_well_formed_lines(
        digit_groups in proptest::collection::vec("[1-9][0-9]{0,4}", 1..6usize),
        descriptions in proptest::collection::vec("[a-zA-Z0-9]+( [a-zA-Z0-9]+){0,4}", 1..6usize),
    ) {
        let n = digit_groups.len().min(descriptions.len());
        let digit_groups = &digit_groups[..n];
        let descriptions = &descriptions[..n];
        let body: String = digit_groups
            .iter()
            .zip(descriptions.iter())
            .map(|(d, desc)| format!("REQ-{d}: {desc}"))
            .collect::<Vec<_>>()
            .join("\n");

        let reqs = extract_requirements(&body);

        prop_assert_eq!(reqs.len(), n);
        for (i, r) in reqs.iter().enumerate() {
            prop_assert_eq!(&r.id, &format!("REQ-{}", digit_groups[i]));
            prop_assert_eq!(&r.description, &descriptions[i]);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// update_traceability_matrix
// ═══════════════════════════════════════════════════════════════════════════

// ─── Tier 1: Specification Tests ──────────────────────────────────────────────

#[test]
fn test_update_traceability_matrix_architecture_entry_sets_architecture_flag() {
    let rows = vec![make_row("REQ-001", false, false, false, false)];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-001", "docs/architecture.md#foo")];
    let updated = update_traceability_matrix(m, TraceabilityStage::Architecture, &entries);
    assert!(updated.rows[0].architecture_covered);
    assert!(!updated.rows[0].interface_covered);
    assert!(!updated.rows[0].sub_work_item_covered);
    assert!(!updated.rows[0].code_covered);
}

#[test]
fn test_update_traceability_matrix_interface_entry_sets_interface_flag() {
    let rows = vec![make_row("REQ-001", false, false, false, false)];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-001", "docs/interfaces/foo.md")];
    let updated = update_traceability_matrix(m, TraceabilityStage::Interface, &entries);
    assert!(updated.rows[0].interface_covered);
    assert!(!updated.rows[0].architecture_covered);
}

#[test]
fn test_update_traceability_matrix_sub_work_item_entry_sets_sub_work_item_flag() {
    let rows = vec![make_row("REQ-001", false, false, false, false)];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-001", "sub-work-item-12")];
    let updated = update_traceability_matrix(m, TraceabilityStage::SubWorkItem, &entries);
    assert!(updated.rows[0].sub_work_item_covered);
    assert!(!updated.rows[0].code_covered);
}

#[test]
fn test_update_traceability_matrix_code_entry_sets_code_flag() {
    let rows = vec![make_row("REQ-001", false, false, false, false)];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-001", "src/lib.rs")];
    let updated = update_traceability_matrix(m, TraceabilityStage::Code, &entries);
    assert!(updated.rows[0].code_covered);
    assert!(!updated.rows[0].sub_work_item_covered);
}

#[test]
fn test_update_traceability_matrix_unmatched_requirement_ref_is_ignored_and_creates_no_row() {
    let rows = vec![make_row("REQ-001", false, false, false, false)];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-999", "irrelevant")];
    let updated = update_traceability_matrix(m, TraceabilityStage::Architecture, &entries);
    assert_eq!(updated.rows.len(), 1);
    assert!(!updated.rows[0].architecture_covered);
}

#[test]
fn test_update_traceability_matrix_row_status_transitions_missing_to_partial_after_first_stage() {
    let rows = vec![make_row("REQ-001", false, false, false, false)];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-001", "arch.md")];
    let updated = update_traceability_matrix(m, TraceabilityStage::Architecture, &entries);
    assert_eq!(updated.rows[0].status, TraceabilityStatus::Partial);
}

#[test]
fn test_update_traceability_matrix_row_status_transitions_partial_to_complete_after_last_stage() {
    let rows = vec![make_row("REQ-001", true, true, true, false)];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-001", "code.rs")];
    let updated = update_traceability_matrix(m, TraceabilityStage::Code, &entries);
    assert_eq!(updated.rows[0].status, TraceabilityStatus::Complete);
    assert!(updated.complete);
}

#[test]
fn test_update_traceability_matrix_full_four_stage_progression_reaches_complete() {
    let rows = vec![make_row("REQ-001", false, false, false, false)];
    let mut m = make_matrix(rows);

    m = update_traceability_matrix(
        m,
        TraceabilityStage::Architecture,
        &[make_entry("REQ-001", "a")],
    );
    assert_eq!(m.rows[0].status, TraceabilityStatus::Partial);
    assert!(!m.complete);

    m = update_traceability_matrix(m, TraceabilityStage::Interface, &[make_entry("REQ-001", "b")]);
    assert_eq!(m.rows[0].status, TraceabilityStatus::Partial);

    m = update_traceability_matrix(
        m,
        TraceabilityStage::SubWorkItem,
        &[make_entry("REQ-001", "c")],
    );
    assert_eq!(m.rows[0].status, TraceabilityStatus::Partial);

    m = update_traceability_matrix(m, TraceabilityStage::Code, &[make_entry("REQ-001", "d")]);
    assert_eq!(m.rows[0].status, TraceabilityStatus::Complete);
    assert!(m.complete);
}

#[test]
fn test_update_traceability_matrix_complete_true_when_every_row_complete() {
    let rows = vec![
        make_row("REQ-001", true, true, true, true),
        make_row("REQ-002", true, true, true, true),
    ];
    let m = make_matrix(rows);
    let updated = update_traceability_matrix(m, TraceabilityStage::Code, &[]);
    assert!(updated.complete);
}

#[test]
fn test_update_traceability_matrix_complete_false_when_any_row_incomplete() {
    let rows = vec![
        make_row("REQ-001", true, true, true, true),
        make_row("REQ-002", true, true, true, false),
    ];
    let m = make_matrix(rows);
    let updated = update_traceability_matrix(m, TraceabilityStage::Code, &[]);
    assert!(!updated.complete);
}

#[test]
fn test_update_traceability_matrix_empty_rows_complete_is_vacuously_true() {
    // Locked-in contract: `.all()` over an empty iterator is `true`, so a
    // matrix with zero rows is (vacuously) complete. This must not be "fixed"
    // to `false` by a future implementation.
    let m = make_matrix(vec![]);
    let updated = update_traceability_matrix(m, TraceabilityStage::Architecture, &[]);
    assert!(updated.rows.is_empty());
    assert!(updated.complete);
}

#[test]
fn test_update_traceability_matrix_empty_entries_is_noop_for_coverage_flags_but_recomputes_status()
 {
    let rows = vec![make_row("REQ-001", true, false, false, false)];
    let m = make_matrix(rows);
    let updated = update_traceability_matrix(m, TraceabilityStage::Architecture, &[]);
    assert!(updated.rows[0].architecture_covered);
    assert!(!updated.rows[0].interface_covered);
    assert_eq!(updated.rows[0].status, TraceabilityStatus::Partial);
}

#[test]
fn test_update_traceability_matrix_preserves_pipeline_run_id_and_work_item_id() {
    let run_id = PipelineRunId::new_random();
    let work_id = WorkItemId::new(77);
    let m = TraceabilityMatrix {
        rows: vec![make_row("REQ-001", false, false, false, false)],
        pipeline_run_id: run_id,
        work_item_id: work_id,
        complete: false,
    };
    let updated = update_traceability_matrix(
        m,
        TraceabilityStage::Architecture,
        &[make_entry("REQ-001", "x")],
    );
    assert_eq!(updated.pipeline_run_id, run_id);
    assert_eq!(updated.work_item_id, work_id);
}

#[test]
fn test_update_traceability_matrix_entry_with_empty_requirement_ref_is_ignored() {
    let rows = vec![make_row("REQ-001", false, false, false, false)];
    let m = make_matrix(rows);
    let entries = vec![make_entry("", "x")];
    let updated = update_traceability_matrix(m, TraceabilityStage::Architecture, &entries);
    assert!(!updated.rows[0].architecture_covered);
    assert_eq!(updated.rows.len(), 1);
}

// ─── Tier 2: Boundary & Adversarial Tests ─────────────────────────────────────

#[test]
fn test_update_traceability_matrix_multiple_entries_update_different_rows_independently() {
    let rows = vec![
        make_row("REQ-001", false, false, false, false),
        make_row("REQ-002", false, false, false, false),
    ];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-001", "a"), make_entry("REQ-002", "b")];
    let updated = update_traceability_matrix(m, TraceabilityStage::Interface, &entries);
    assert!(updated.rows[0].interface_covered);
    assert!(updated.rows[1].interface_covered);
}

#[test]
fn test_update_traceability_matrix_entry_only_affects_targeted_row_not_others() {
    let rows = vec![
        make_row("REQ-001", false, false, false, false),
        make_row("REQ-002", false, false, false, false),
    ];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-001", "a")];
    let updated = update_traceability_matrix(m, TraceabilityStage::Interface, &entries);
    assert!(updated.rows[0].interface_covered);
    assert!(!updated.rows[1].interface_covered);
    assert_eq!(updated.rows[1].status, TraceabilityStatus::Missing);
}

#[test]
fn test_update_traceability_matrix_duplicate_entries_for_same_requirement_do_not_duplicate_rows() {
    let rows = vec![make_row("REQ-001", false, false, false, false)];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-001", "a"), make_entry("REQ-001", "b")];
    let updated = update_traceability_matrix(m, TraceabilityStage::Architecture, &entries);
    assert!(updated.rows[0].architecture_covered);
    assert_eq!(updated.rows.len(), 1);
}

#[test]
fn test_update_traceability_matrix_entry_matches_multiple_rows_sharing_same_id_sets_both() {
    let rows = vec![
        make_row("REQ-001", false, false, false, false),
        make_row("REQ-001", false, false, false, false),
    ];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-001", "x")];
    let updated = update_traceability_matrix(m, TraceabilityStage::Code, &entries);
    assert!(updated.rows[0].code_covered);
    assert!(updated.rows[1].code_covered);
}

#[test]
fn test_update_traceability_matrix_stage_update_never_resets_other_already_true_flags() {
    let rows = vec![make_row("REQ-001", false, true, true, true)];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-001", "arch.md")];
    let updated = update_traceability_matrix(m, TraceabilityStage::Architecture, &entries);
    assert!(updated.rows[0].architecture_covered);
    assert!(updated.rows[0].interface_covered);
    assert!(updated.rows[0].sub_work_item_covered);
    assert!(updated.rows[0].code_covered);
    assert_eq!(updated.rows[0].status, TraceabilityStatus::Complete);
}

#[test]
fn test_update_traceability_matrix_entry_for_one_stage_does_not_set_other_stage_flags() {
    let rows = vec![make_row("REQ-001", false, false, false, false)];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-001", "x")];
    let updated = update_traceability_matrix(m, TraceabilityStage::SubWorkItem, &entries);
    assert!(!updated.rows[0].architecture_covered);
    assert!(!updated.rows[0].interface_covered);
    assert!(updated.rows[0].sub_work_item_covered);
    assert!(!updated.rows[0].code_covered);
}

#[test]
fn test_update_traceability_matrix_idempotent_reapplying_same_entries_yields_same_result() {
    let rows = vec![make_row("REQ-001", false, false, false, false)];
    let m = make_matrix(rows);
    let entries = vec![make_entry("REQ-001", "x")];
    let once = update_traceability_matrix(m, TraceabilityStage::Architecture, &entries);
    let twice = update_traceability_matrix(once.clone(), TraceabilityStage::Architecture, &entries);
    assert_eq!(
        once.rows[0].architecture_covered,
        twice.rows[0].architecture_covered
    );
    assert_eq!(once.rows[0].status, twice.rows[0].status);
    assert_eq!(once.complete, twice.complete);
}

#[test]
fn test_update_traceability_matrix_empty_rows_with_nonempty_entries_still_returns_empty_rows() {
    let m = make_matrix(vec![]);
    let entries = vec![make_entry("REQ-001", "x")];
    let updated = update_traceability_matrix(m, TraceabilityStage::Architecture, &entries);
    assert!(updated.rows.is_empty());
    assert!(updated.complete);
}

#[test]
fn test_update_traceability_matrix_does_not_hardcode_complete_true_for_mixed_rows() {
    let rows = vec![
        make_row("REQ-001", false, false, false, false),
        make_row("REQ-002", true, true, true, true),
    ];
    let m = make_matrix(rows);
    let updated = update_traceability_matrix(m, TraceabilityStage::Architecture, &[]);
    assert!(!updated.complete);
    assert_eq!(updated.rows[0].status, TraceabilityStatus::Missing);
    assert_eq!(updated.rows[1].status, TraceabilityStatus::Complete);
}

#[test]
fn test_update_traceability_matrix_does_not_hardcode_complete_false_for_all_complete_rows() {
    let rows = vec![make_row("REQ-001", true, true, true, true)];
    let m = make_matrix(rows);
    let updated = update_traceability_matrix(m, TraceabilityStage::Code, &[]);
    assert!(updated.complete);
}

#[test]
fn test_update_traceability_matrix_returns_value_independent_of_consumed_input() {
    // Value semantics: the function takes `matrix` by value, so there is no
    // aliasing hazard to test directly, but the returned matrix must reflect
    // the row count of the input it was built from, not some fixed value.
    let rows_a = vec![make_row("REQ-001", false, false, false, false)];
    let rows_b = vec![
        make_row("REQ-001", false, false, false, false),
        make_row("REQ-002", false, false, false, false),
        make_row("REQ-003", false, false, false, false),
    ];
    let updated_a = update_traceability_matrix(make_matrix(rows_a), TraceabilityStage::Code, &[]);
    let updated_b = update_traceability_matrix(make_matrix(rows_b), TraceabilityStage::Code, &[]);
    assert_eq!(updated_a.rows.len(), 1);
    assert_eq!(updated_b.rows.len(), 3);
}

// ─── Tier 3: Property-Based Tests ─────────────────────────────────────────────

fn stage_from_index(idx: u8) -> TraceabilityStage {
    match idx {
        0 => TraceabilityStage::Architecture,
        1 => TraceabilityStage::Interface,
        2 => TraceabilityStage::SubWorkItem,
        _ => TraceabilityStage::Code,
    }
}

proptest! {
    /// Fuzz: `update_traceability_matrix` must never panic for any
    /// combination of rows, stage, and entries.
    #[test]
    fn test_update_traceability_matrix_never_panics_on_arbitrary_input(
        row_specs in proptest::collection::vec(
            ("REQ-[0-9]{1,4}", any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>()),
            0..6usize,
        ),
        entry_specs in proptest::collection::vec(("REQ-[0-9]{1,4}", ".{0,20}"), 0..6usize),
        stage_idx in 0u8..4u8,
    ) {
        let rows: Vec<RequirementRow> = row_specs
            .into_iter()
            .map(|(id, a, i, s, c)| make_row(&id, a, i, s, c))
            .collect();
        let entries: Vec<TraceabilityEntry> = entry_specs
            .into_iter()
            .map(|(req_ref, out_ref)| make_entry(&req_ref, &out_ref))
            .collect();
        let stage = stage_from_index(stage_idx);
        let m = make_matrix(rows);
        let _ = update_traceability_matrix(m, stage, &entries);
    }

    /// Invariant: `matrix.complete` must exactly equal "every row's status is
    /// `Complete`" after every update, for arbitrary rows/entries/stage.
    #[test]
    fn test_update_traceability_matrix_complete_flag_matches_all_rows_complete_invariant(
        row_specs in proptest::collection::vec(
            ("REQ-[0-9]{1,4}", any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>()),
            0..6usize,
        ),
        entry_specs in proptest::collection::vec(("REQ-[0-9]{1,4}", ".{0,20}"), 0..6usize),
        stage_idx in 0u8..4u8,
    ) {
        let rows: Vec<RequirementRow> = row_specs
            .into_iter()
            .map(|(id, a, i, s, c)| make_row(&id, a, i, s, c))
            .collect();
        let entries: Vec<TraceabilityEntry> = entry_specs
            .into_iter()
            .map(|(req_ref, out_ref)| make_entry(&req_ref, &out_ref))
            .collect();
        let stage = stage_from_index(stage_idx);
        let m = make_matrix(rows);
        let updated = update_traceability_matrix(m, stage, &entries);

        let expected_complete = updated
            .rows
            .iter()
            .all(|r| r.status == TraceabilityStatus::Complete);
        prop_assert_eq!(updated.complete, expected_complete);
    }

    /// Idempotence: applying the same `(stage, entries)` twice in a row must
    /// yield the same coverage flags, statuses, and `complete` flag as
    /// applying it once.
    #[test]
    fn test_update_traceability_matrix_idempotent_across_arbitrary_inputs(
        row_specs in proptest::collection::vec(
            ("REQ-[0-9]{1,4}", any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>()),
            0..6usize,
        ),
        entry_specs in proptest::collection::vec(("REQ-[0-9]{1,4}", ".{0,20}"), 0..6usize),
        stage_idx in 0u8..4u8,
    ) {
        let rows: Vec<RequirementRow> = row_specs
            .into_iter()
            .map(|(id, a, i, s, c)| make_row(&id, a, i, s, c))
            .collect();
        let entries: Vec<TraceabilityEntry> = entry_specs
            .into_iter()
            .map(|(req_ref, out_ref)| make_entry(&req_ref, &out_ref))
            .collect();
        let stage = stage_from_index(stage_idx);
        let m = make_matrix(rows);
        let once = update_traceability_matrix(m, stage, &entries);
        let twice = update_traceability_matrix(once.clone(), stage, &entries);

        prop_assert_eq!(once.rows.len(), twice.rows.len());
        for (r1, r2) in once.rows.iter().zip(twice.rows.iter()) {
            prop_assert_eq!(r1.architecture_covered, r2.architecture_covered);
            prop_assert_eq!(r1.interface_covered, r2.interface_covered);
            prop_assert_eq!(r1.sub_work_item_covered, r2.sub_work_item_covered);
            prop_assert_eq!(r1.code_covered, r2.code_covered);
            prop_assert_eq!(r1.status, r2.status);
        }
        prop_assert_eq!(once.complete, twice.complete);
    }

    /// Stage isolation invariant: updating for one stage must never flip any
    /// *other* stage's coverage flag, and the targeted stage's flag is
    /// monotonic (once `true`, it can never become `false`).
    #[test]
    fn test_update_traceability_matrix_only_targeted_stage_flag_can_change(
        row_specs in proptest::collection::vec(
            ("REQ-[0-9]{1,4}", any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>()),
            1..6usize,
        ),
        entry_specs in proptest::collection::vec(("REQ-[0-9]{1,4}", ".{0,20}"), 0..6usize),
        stage_idx in 0u8..4u8,
    ) {
        let rows: Vec<RequirementRow> = row_specs
            .into_iter()
            .map(|(id, a, i, s, c)| make_row(&id, a, i, s, c))
            .collect();
        let entries: Vec<TraceabilityEntry> = entry_specs
            .into_iter()
            .map(|(req_ref, out_ref)| make_entry(&req_ref, &out_ref))
            .collect();
        let stage = stage_from_index(stage_idx);
        let original_flags: Vec<(bool, bool, bool, bool)> = rows
            .iter()
            .map(|r| {
                (
                    r.architecture_covered,
                    r.interface_covered,
                    r.sub_work_item_covered,
                    r.code_covered,
                )
            })
            .collect();
        let m = make_matrix(rows);
        let updated = update_traceability_matrix(m, stage, &entries);

        for (row, (oa, oi, os, oc)) in updated.rows.iter().zip(original_flags.iter()) {
            match stage {
                TraceabilityStage::Architecture => {
                    prop_assert_eq!(row.interface_covered, *oi);
                    prop_assert_eq!(row.sub_work_item_covered, *os);
                    prop_assert_eq!(row.code_covered, *oc);
                    prop_assert!(row.architecture_covered || !*oa);
                }
                TraceabilityStage::Interface => {
                    prop_assert_eq!(row.architecture_covered, *oa);
                    prop_assert_eq!(row.sub_work_item_covered, *os);
                    prop_assert_eq!(row.code_covered, *oc);
                    prop_assert!(row.interface_covered || !*oi);
                }
                TraceabilityStage::SubWorkItem => {
                    prop_assert_eq!(row.architecture_covered, *oa);
                    prop_assert_eq!(row.interface_covered, *oi);
                    prop_assert_eq!(row.code_covered, *oc);
                    prop_assert!(row.sub_work_item_covered || !*os);
                }
                TraceabilityStage::Code => {
                    prop_assert_eq!(row.architecture_covered, *oa);
                    prop_assert_eq!(row.interface_covered, *oi);
                    prop_assert_eq!(row.sub_work_item_covered, *os);
                    prop_assert!(row.code_covered || !*oc);
                }
            }
        }
    }
}
