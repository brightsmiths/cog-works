//! Adversarial test suite for `run_deterministic_alignment` in `alignment.rs`.
//!
//! Tests are derived from `docs/spec/interfaces/advanced-features.md`
//! §run_deterministic_alignment, the behavioral assertion ASSERT-ALIGN-001,
//! and the cross-referenced precedence assertion ASSERT-SEC-005 (from
//! `security.rs`'s `validate_scope`, used here only as design guidance for
//! how the scope check and the protected-path check should interact when
//! both conditions hold for the *same* artifact).
//!
//! `run_deterministic_alignment` is currently `todo!()`; every test below is
//! expected to **compile** but **fail at runtime** (RED) until the
//! implementation is written.
//!
//! ## Spec gaps surfaced by this suite (see test-plan report for detail)
//!
//! 1. **Unparseable artifact key vs. the protected-path check.** An output
//!    artifact key is a plain `String` (slot name), not necessarily a valid
//!    [`ArtifactPath`] (e.g. it could contain a `..` component or be empty).
//!    `is_protected` requires `&ArtifactPath`. This suite assumes (and tests)
//!    that a key which fails `ArtifactPath::new` is **skipped** by the
//!    protected-path check (check 3) for that key — it produces no finding
//!    from check 3, though it may still be flagged by the scope check
//!    (check 2), which operates on the raw string. See
//!    `test_unparseable_artifact_key_is_skipped_by_protected_path_check`.
//! 2. **Same-artifact double-violation precedence.** When a single artifact
//!    is simultaneously out of approved scope AND matches a protected path,
//!    the contract does not explicitly say whether this yields one finding
//!    or two. This suite assumes single-finding precedence (protected-path
//!    wins, mirroring `validate_scope`'s documented precedence rule,
//!    ASSERT-SEC-005) — see
//!    `test_artifact_both_out_of_scope_and_protected_produces_single_finding`.
//!    This is the least certain assumption in the suite; flagged for
//!    architect confirmation.

#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;

use super::*;
use crate::{ArtifactPath, execution::NodeOutput, security::ApprovedScope, types::TokenCost};

// ─── Test helpers ────────────────────────────────────────────────────────────

/// Builds a valid `ArtifactPath`, panicking (test-only) if the literal is bad.
fn art(s: &str) -> ArtifactPath {
    ArtifactPath::new(s).expect("test artifact path must be valid")
}

/// Builds an `ApprovedScope` from a list of glob patterns with unrestricted
/// file-count limits (so `max_files`/`max_new_files` never contribute noise
/// to a test that is only exercising the three `run_deterministic_alignment`
/// checks).
fn scope(patterns: &[&str]) -> ApprovedScope {
    ApprovedScope {
        artifact_patterns: patterns.iter().map(ToString::to_string).collect(),
        max_files: None,
        max_new_files: u32::MAX,
    }
}

/// Builds the `protected_paths: Vec<ArtifactPath>` field from plain strings.
fn protected(paths: &[&str]) -> Vec<ArtifactPath> {
    paths.iter().map(|p| art(p)).collect()
}

/// Builds a complete `DeclaredNodeInputs` fixture.
fn declared_inputs(
    required_output_slots: &[&str],
    scope_patterns: &[&str],
    protected_paths: &[&str],
) -> DeclaredNodeInputs {
    DeclaredNodeInputs {
        required_output_slots: required_output_slots
            .iter()
            .map(ToString::to_string)
            .collect(),
        approved_scope: scope(scope_patterns),
        protected_paths: protected(protected_paths),
    }
}

/// Builds an artifacts map where every key maps to an arbitrary JSON value.
fn artifacts_map(keys: &[&str]) -> HashMap<String, serde_json::Value> {
    keys.iter()
        .map(|k| ((*k).to_string(), serde_json::json!("content")))
        .collect()
}

/// Builds a `NodeOutput` fixture with the given artifact keys.
fn output(keys: &[&str]) -> NodeOutput {
    NodeOutput {
        artifacts: artifacts_map(keys),
        cost_delta: TokenCost::zero(),
        state_updates: vec![],
    }
}

fn count_of(findings: &[AlignmentFinding], kind: MisalignmentType) -> usize {
    findings.iter().filter(|f| f.misalignment == kind).count()
}

// ─── Group A: all-pass / vacuous paths ───────────────────────────────────────

/// Mirrors the module's own doctest: all declared slots present, no scope or
/// protected-path violations → empty findings vec.
#[test]
fn test_all_required_slots_present_and_in_scope_returns_no_findings() {
    let inputs = declared_inputs(&["spec_doc"], &["**"], &[]);
    let node_output = output(&["spec_doc"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert!(
        findings.is_empty(),
        "all declared slots populated and in scope must yield no findings, got {findings:?}"
    );
}

/// Empty required slots and empty artifacts is a vacuously-aligned call.
#[test]
fn test_empty_required_slots_and_empty_artifacts_returns_no_findings() {
    let inputs = declared_inputs(&[], &["**"], &[]);
    let node_output = output(&[]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert!(
        findings.is_empty(),
        "no required slots and no artifacts must yield no findings"
    );
}

/// No required slots declared, but the node still produced in-scope
/// artifacts → nothing to check against, no findings.
#[test]
fn test_empty_required_slots_with_in_scope_artifacts_returns_no_findings() {
    let inputs = declared_inputs(&[], &["**"], &[]);
    let node_output = output(&["notes.md", "summary.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert!(
        findings.is_empty(),
        "undeclared-but-in-scope artifacts must not be flagged when no slots are required"
    );
}

/// An artifact key that is not one of the required slots, but is within the
/// approved scope, is not itself a violation — `run_deterministic_alignment`
/// only flags missing *required* slots and out-of-scope/protected keys. It
/// does not flag "extra" artifacts that are in scope.
#[test]
fn test_extra_in_scope_artifact_beyond_required_slots_produces_no_finding() {
    let inputs = declared_inputs(&["spec_doc"], &["**"], &[]);
    let node_output = output(&["spec_doc", "scratch_notes.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert!(
        findings.is_empty(),
        "an extra in-scope artifact beyond the required slots must not be flagged"
    );
}

// ─── Group B: check 1 — missing required output slots ───────────────────────

/// ASSERT-ALIGN-001: architecture requires three components (A, B, C); the
/// node only produced A and B → exactly one `Missing` finding referencing C.
#[test]
fn test_single_missing_required_slot_produces_missing_finding() {
    let inputs = declared_inputs(&["component_a", "component_b", "component_c"], &["**"], &[]);
    let node_output = output(&["component_a", "component_b"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::Missing),
        1,
        "exactly one Missing finding expected for the one absent slot, got {findings:?}"
    );
}

/// The Missing finding's `check_type` must be `Deterministic` (no LLM
/// involved in this structural check).
#[test]
fn test_missing_finding_has_deterministic_check_type() {
    let inputs = declared_inputs(&["component_c"], &["**"], &[]);
    let node_output = output(&[]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    let missing = findings
        .iter()
        .find(|f| f.misalignment == MisalignmentType::Missing)
        .expect("expected a Missing finding");
    assert_eq!(missing.check_type, AlignmentCheckType::Deterministic);
}

/// A missing required output slot is always a blocking finding.
#[test]
fn test_missing_finding_is_blocking() {
    let inputs = declared_inputs(&["component_c"], &["**"], &[]);
    let node_output = output(&[]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    let missing = findings
        .iter()
        .find(|f| f.misalignment == MisalignmentType::Missing)
        .expect("expected a Missing finding");
    assert!(missing.blocking, "a missing required slot must be blocking");
}

/// The Missing finding's description must reference the specific slot name
/// that was absent, so a human (or downstream tooling) can identify which
/// declared output was not produced.
#[test]
fn test_missing_finding_description_references_missing_slot_name() {
    let inputs = declared_inputs(&["component_a", "component_b", "component_c"], &["**"], &[]);
    let node_output = output(&["component_a", "component_b"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    let missing = findings
        .iter()
        .find(|f| f.misalignment == MisalignmentType::Missing)
        .expect("expected a Missing finding");
    assert!(
        missing.description.contains("component_c"),
        "description must reference the missing slot name 'component_c', got: {}",
        missing.description
    );
}

/// Multiple missing slots produce one `Missing` finding each, not a single
/// aggregated finding.
#[test]
fn test_multiple_missing_slots_produce_one_finding_each() {
    let inputs = declared_inputs(
        &["component_a", "component_b", "component_c", "component_d"],
        &["**"],
        &[],
    );
    let node_output = output(&["component_a"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::Missing),
        3,
        "three absent slots (b, c, d) must produce three Missing findings, got {findings:?}"
    );
}

/// Presence is a key-existence check: a required slot populated with JSON
/// `null` still counts as present (kills a stub that mistakes "falsy value"
/// for "absent").
#[test]
fn test_required_slot_present_with_null_json_value_is_not_missing() {
    let inputs = declared_inputs(&["spec_doc"], &["**"], &[]);
    let mut artifacts = HashMap::new();
    artifacts.insert("spec_doc".to_string(), serde_json::Value::Null);
    let node_output = NodeOutput {
        artifacts,
        cost_delta: TokenCost::zero(),
        state_updates: vec![],
    };

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::Missing),
        0,
        "a slot key present with a null value must not be treated as missing"
    );
}

/// All required slots absent (empty artifacts map) → one Missing finding per
/// declared slot.
#[test]
fn test_all_required_slots_missing_when_artifacts_map_is_empty() {
    let inputs = declared_inputs(&["component_a", "component_b"], &["**"], &[]);
    let node_output = output(&[]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::Missing),
        2,
        "both required slots absent must produce two Missing findings, got {findings:?}"
    );
}

// ─── Group C: check 2 — scope containment ────────────────────────────────────

/// An artifact key that does not match any approved-scope pattern produces a
/// `ScopeExceeded` finding.
#[test]
fn test_artifact_outside_approved_scope_produces_scope_exceeded_finding() {
    let inputs = declared_inputs(&[], &["src/**"], &[]);
    let node_output = output(&["docs/readme.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        1,
        "an out-of-scope artifact must produce one ScopeExceeded finding, got {findings:?}"
    );
}

/// All `ScopeExceeded` findings are unconditionally blocking.
#[test]
fn test_scope_exceeded_finding_is_blocking() {
    let inputs = declared_inputs(&[], &["src/**"], &[]);
    let node_output = output(&["docs/readme.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    let scope_finding = findings
        .iter()
        .find(|f| f.misalignment == MisalignmentType::ScopeExceeded)
        .expect("expected a ScopeExceeded finding");
    assert!(
        scope_finding.blocking,
        "ScopeExceeded findings must always be blocking"
    );
}

/// The scope check is deterministic/structural — `check_type` must be
/// `Deterministic`.
#[test]
fn test_scope_exceeded_finding_check_type_is_deterministic() {
    let inputs = declared_inputs(&[], &["src/**"], &[]);
    let node_output = output(&["docs/readme.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    let scope_finding = findings
        .iter()
        .find(|f| f.misalignment == MisalignmentType::ScopeExceeded)
        .expect("expected a ScopeExceeded finding");
    assert_eq!(scope_finding.check_type, AlignmentCheckType::Deterministic);
}

/// Multiple out-of-scope artifacts each produce their own finding.
#[test]
fn test_multiple_artifacts_outside_scope_produce_one_finding_each() {
    let inputs = declared_inputs(&[], &["src/**"], &[]);
    let node_output = output(&["docs/readme.md", "notes.txt", "src/main.rs"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        2,
        "two out-of-scope artifacts (docs/readme.md, notes.txt) expected, got {findings:?}"
    );
}

/// `"**"` matches everything — no scope violation regardless of the key.
#[test]
fn test_double_star_pattern_matches_everything_no_scope_violation() {
    let inputs = declared_inputs(&[], &["**"], &[]);
    let node_output = output(&["anything/at/all.rs"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(count_of(&findings, MisalignmentType::ScopeExceeded), 0);
}

/// `"src/**"` matches nested paths under `src/`.
#[test]
fn test_prefix_pattern_matches_nested_subpath_no_violation() {
    let inputs = declared_inputs(&[], &["src/**"], &[]);
    let node_output = output(&["src/nested/deep/foo.rs"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        0,
        "src/** must match a deeply-nested path under src/"
    );
}

/// `"src/**"` must not match a sibling tree outside `src/`.
#[test]
fn test_prefix_pattern_rejects_path_outside_prefix() {
    let inputs = declared_inputs(&[], &["src/**"], &[]);
    let node_output = output(&["docs/readme.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        1,
        "src/** must not match docs/readme.md"
    );
}

/// A pattern with no `/` matches at any depth (gitignore-style semantics,
/// matching `is_protected`'s documented behaviour).
#[test]
fn test_pattern_without_slash_matches_at_any_depth() {
    let inputs = declared_inputs(&[], &["config.json"], &[]);
    let node_output = output(&["deeply/nested/config.json"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        0,
        "pattern 'config.json' (no slash) must match at any depth"
    );
}

/// An empty `approved_scope.artifact_patterns` list means nothing matches —
/// every produced artifact is flagged `ScopeExceeded`. (There is no
/// `ScopeUnderspecified` variant in `MisalignmentType`; the empty-pattern
/// case degrades to "everything is out of scope".)
#[test]
fn test_empty_scope_patterns_flags_every_artifact_as_scope_exceeded() {
    let inputs = declared_inputs(&[], &[], &[]);
    let node_output = output(&["anything.rs"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        1,
        "with no approved patterns at all, every artifact must be flagged ScopeExceeded"
    );
}

// ─── Group D: check 3 — protected paths ──────────────────────────────────────

/// An artifact key matching a protected path produces a `ScopeExceeded`
/// finding (protected-path violations are reported under the same
/// `MisalignmentType::ScopeExceeded` variant per the enum's doc comments).
#[test]
fn test_protected_path_artifact_produces_scope_exceeded_finding() {
    let inputs = declared_inputs(&[], &["**"], &["SECURITY.md"]);
    let node_output = output(&["SECURITY.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        1,
        "a protected-path artifact must produce one ScopeExceeded finding, got {findings:?}"
    );
}

/// Protected-path findings are unconditionally blocking.
#[test]
fn test_protected_path_finding_is_blocking() {
    let inputs = declared_inputs(&[], &["**"], &["SECURITY.md"]);
    let node_output = output(&["SECURITY.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    let finding = findings
        .iter()
        .find(|f| f.misalignment == MisalignmentType::ScopeExceeded)
        .expect("expected a ScopeExceeded finding");
    assert!(finding.blocking, "protected-path findings must be blocking");
}

/// Protected-path findings are `Deterministic` (reuse of `is_protected`, no
/// LLM involved).
#[test]
fn test_protected_path_finding_check_type_is_deterministic() {
    let inputs = declared_inputs(&[], &["**"], &["SECURITY.md"]);
    let node_output = output(&["SECURITY.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    let finding = findings
        .iter()
        .find(|f| f.misalignment == MisalignmentType::ScopeExceeded)
        .expect("expected a ScopeExceeded finding");
    assert_eq!(finding.check_type, AlignmentCheckType::Deterministic);
}

/// Multiple protected artifacts each produce their own finding; a
/// non-protected artifact is not flagged.
#[test]
fn test_multiple_protected_artifacts_each_produce_a_finding() {
    let inputs = declared_inputs(&[], &["**"], &["SECURITY.md", "LICENSE"]);
    let node_output = output(&["SECURITY.md", "LICENSE", "src/main.rs"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        2,
        "two protected artifacts (SECURITY.md, LICENSE) expected, src/main.rs untouched, got {findings:?}"
    );
}

/// A protected-path entry that contains a `/` is used as an exact-match
/// pattern (per `is_protected`'s `normalize_glob_pattern` semantics) — it
/// must not match a deeper nested occurrence of the same relative tail.
#[test]
fn test_protected_path_with_slash_requires_exact_match_not_nested() {
    let inputs = declared_inputs(&[], &["**"], &["docs/rules.md"]);
    let node_output = output(&["sub/docs/rules.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        0,
        "protected pattern 'docs/rules.md' must not match 'sub/docs/rules.md' (exact match only)"
    );
}

/// A protected-path entry with no `/` matches the same filename at any
/// depth, mirroring `is_protected`'s documented any-depth semantics.
#[test]
fn test_protected_path_without_slash_matches_at_any_depth() {
    let inputs = declared_inputs(&[], &["**"], &["SECURITY.md"]);
    let node_output = output(&["nested/sub/SECURITY.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        1,
        "protected pattern 'SECURITY.md' (no slash) must match at any depth"
    );
}

/// An empty `protected_paths` list never flags any artifact.
#[test]
fn test_empty_protected_paths_never_flags_any_artifact() {
    let inputs = declared_inputs(&[], &["**"], &[]);
    let node_output = output(&["SECURITY.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert!(
        findings.is_empty(),
        "an empty protected_paths list must never produce a finding"
    );
}

// ─── Group E: check interplay / combinations ─────────────────────────────────

/// Contract point 5: the three checks are not short-circuited. A single call
/// can produce a `Missing` finding (for an absent required slot) AND a
/// `ScopeExceeded` finding (for a different, out-of-scope artifact)
/// simultaneously.
#[test]
fn test_missing_and_scope_exceeded_findings_both_produced_for_different_artifacts() {
    let inputs = declared_inputs(&["summary"], &["summary"], &[]);
    let node_output = output(&["other_file.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::Missing),
        1,
        "{findings:?}"
    );
    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        1,
        "{findings:?}"
    );
}

/// All three checks can fire within a single call, each triggered by a
/// distinct, non-overlapping artifact: `summary` is missing, `other.md` is
/// out of scope, and `SECRET.md` is in scope but protected.
#[test]
fn test_all_three_checks_fire_simultaneously_on_distinct_artifacts() {
    let inputs = declared_inputs(&["summary"], &["summary", "SECRET.md"], &["SECRET.md"]);
    let node_output = output(&["other.md", "SECRET.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::Missing),
        1,
        "missing 'summary' expected, got {findings:?}"
    );
    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        2,
        "'other.md' (out of scope) and 'SECRET.md' (protected) expected, got {findings:?}"
    );
}

/// Design decision (ASSERT-SEC-005-informed, flagged as a spec gap): when a
/// single artifact is simultaneously out of approved scope AND matches a
/// protected path, exactly one `ScopeExceeded` finding is produced for that
/// artifact — not two. This mirrors `validate_scope`'s documented precedence
/// rule (protected-path violations take precedence over, and suppress, a
/// redundant unauthorized-scope violation for the same artifact).
#[test]
fn test_artifact_both_out_of_scope_and_protected_produces_single_finding() {
    let inputs = declared_inputs(&[], &["src/**"], &["SECURITY.md"]);
    let node_output = output(&["SECURITY.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        1,
        "an artifact that is both out of scope and protected must yield exactly one \
         ScopeExceeded finding (protected-path precedence), got {findings:?}"
    );
}

/// Security remediation (fail-closed, path-traversal bypass): an artifact
/// key that cannot be parsed as an `ArtifactPath` (contains a `..`
/// traversal component) must never be silently waved through. Previously
/// such a key was skipped by the protected-path check (check 3, since
/// `is_protected` requires a parsed `ArtifactPath`) *and* matched as a raw,
/// unresolved string by the scope check (check 2), so a permissive pattern
/// like `"**"` produced zero findings for a key that, once path-resolved,
/// could target a protected file. The corrected behaviour is fail-closed:
/// an unparseable key always produces exactly one blocking
/// `ScopeExceeded` finding, regardless of how permissive the approved
/// scope is.
#[test]
fn test_unparseable_artifact_key_produces_blocking_finding_fail_closed() {
    let inputs = declared_inputs(&[], &["**"], &["b"]);
    let node_output = output(&["a/../b"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        findings.len(),
        1,
        "an unparseable artifact key must produce exactly one finding, got {findings:?}"
    );
    assert_eq!(
        findings[0].misalignment,
        MisalignmentType::ScopeExceeded,
        "the fail-closed finding for an unparseable key must be ScopeExceeded, got {findings:?}"
    );
    assert!(
        findings[0].blocking,
        "the fail-closed finding for an unparseable key must be blocking, got {findings:?}"
    );
}

/// Robustness companion to the above: the unparseable-key case must not
/// panic, regardless of how the checks are implemented internally. Note
/// that `required_output_slots` entries are plain `String`s (no
/// `ArtifactPath` parsing applies to them), so an unparseable-looking slot
/// name there is unremarkable; only the artifact *keys* in
/// `node_output.artifacts` are exercised against `ArtifactPath::new`.
/// Strengthened per the fail-closed remediation: both artifact keys here
/// (`"a/../b"` containing a `..` component, and `""` the empty string) fail
/// `ArtifactPath::new`, so the call must not panic AND must yield a
/// non-empty findings vec (the fail-closed behaviour is observable, not
/// just silently absorbed).
#[test]
fn test_unparseable_artifact_key_does_not_panic() {
    let inputs = declared_inputs(&["a/../b"], &["**"], &["b"]);
    let node_output = output(&["a/../b", ""]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert!(
        !findings.is_empty(),
        "two unparseable artifact keys must still produce observable fail-closed findings, \
         got {findings:?}"
    );
}

/// Security remediation: the original bypass report used a narrow approved
/// scope (`"allowed/**"`), not `"**"`, where the unparseable key's *raw*
/// string form happens to coincidentally match a scope pattern that looks
/// plausible (`"a/**"`) even though the key contains a `..` segment that
/// would resolve outside of it. The fail-closed check must fire regardless
/// — an unparseable key can never be trusted as a scope-approved member,
/// no matter what the raw string coincidentally lines up with.
#[test]
fn test_unparseable_artifact_key_flagged_even_when_raw_string_coincidentally_matches_narrow_scope()
{
    let inputs = declared_inputs(&[], &["a/**"], &["b"]);
    let node_output = output(&["a/../b"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        findings.len(),
        1,
        "an unparseable key must be flagged even though its raw string coincidentally \
         matches the approved scope pattern 'a/**', got {findings:?}"
    );
    assert_eq!(findings[0].misalignment, MisalignmentType::ScopeExceeded);
    assert!(findings[0].blocking);
}

/// Companion to the above: when the unparseable key's raw string form does
/// *not* match the approved scope either, the fail-closed check must still
/// produce exactly **one** finding for that key — not two (one from the
/// fail-closed rule and a redundant one from the raw-string scope
/// fallback). This proves the fail-closed rule short-circuits the scope
/// check for unparseable keys rather than running both.
#[test]
fn test_unparseable_artifact_key_produces_exactly_one_finding_when_raw_string_also_out_of_scope() {
    let inputs = declared_inputs(&[], &["allowed/**"], &["b"]);
    let node_output = output(&["a/../b"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        findings.len(),
        1,
        "an unparseable key whose raw string also fails the scope pattern must still \
         produce exactly one finding (no double-counting), got {findings:?}"
    );
    assert_eq!(findings[0].misalignment, MisalignmentType::ScopeExceeded);
    assert!(findings[0].blocking);
}

/// Property: regardless of what the approved-scope patterns are — maximally
/// permissive, narrowly-but-coincidentally matching, or entirely
/// non-matching — an unparseable artifact key always produces exactly one
/// blocking `ScopeExceeded` finding, because it can never be a trusted
/// member of an approved-scope decision.
#[test]
fn prop_unparseable_artifact_key_always_produces_exactly_one_scope_exceeded_finding() {
    proptest::proptest!(|(scope_patterns in proptest::collection::vec(
        proptest::sample::select(vec![
            "**".to_string(),
            "a/**".to_string(),
            "allowed/**".to_string(),
            "b".to_string(),
            "".to_string(),
        ]),
        0..4usize,
    ))| {
        let inputs = declared_inputs(&[], &scope_patterns.iter().map(String::as_str).collect::<Vec<_>>(), &[]);
        let node_output = output(&["a/../b"]);

        let findings = run_deterministic_alignment(&inputs, &node_output);

        assert_eq!(
            findings.len(),
            1,
            "an unparseable key must always produce exactly one finding regardless of \
             approved_scope patterns {scope_patterns:?}, got {findings:?}"
        );
        assert_eq!(findings[0].misalignment, MisalignmentType::ScopeExceeded);
        assert!(findings[0].blocking);
    });
}

// ─── Group F: determinism / stub-killing ─────────────────────────────────────

/// Pure function: calling twice with identical inputs must yield the same
/// number of findings (kills non-deterministic or stateful stubs).
#[test]
fn test_run_deterministic_alignment_is_pure_same_inputs_same_finding_count() {
    let inputs = declared_inputs(&["summary"], &["summary"], &[]);
    let node_output = output(&["other.md"]);

    let first = run_deterministic_alignment(&inputs, &node_output);
    let second = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        first.len(),
        second.len(),
        "repeated calls with identical inputs must produce the same number of findings"
    );
}

/// Duplicate entries in `required_output_slots` are iterated as declared —
/// each absent occurrence produces its own `Missing` finding. (Documented
/// assumption: `required_output_slots` is treated as a list of declarations,
/// not de-duplicated into a set, since nothing in the contract calls for
/// deduplication.)
#[test]
fn test_duplicate_required_slot_names_each_produce_a_missing_finding() {
    let inputs = declared_inputs(&["summary", "summary"], &["**"], &[]);
    let node_output = output(&[]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::Missing),
        2,
        "two declared (duplicate) occurrences of 'summary' absent must produce two Missing \
         findings, got {findings:?}"
    );
}

/// A hardcoded "always one finding" stub would wrongly flag a fully-aligned
/// call; a hardcoded "always empty" stub would fail every Missing/ScopeExceeded
/// test above. This test specifically targets a stub that always returns
/// exactly the number of artifacts as findings (an easy coincidental-pass
/// shape for a lazy implementation).
#[test]
fn test_finding_count_is_not_simply_artifact_count() {
    let inputs = declared_inputs(&[], &["**"], &[]);
    let node_output = output(&["a.rs", "b.rs", "c.rs"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        findings.len(),
        0,
        "three in-scope, non-required artifacts must yield zero findings, not one per artifact"
    );
}

// ─── Group G: property-based tests ───────────────────────────────────────────

/// Property: for any generated set of slot names that are all present as
/// artifact keys, no `Missing` finding is ever produced — regardless of how
/// many slots or what their names are.
#[test]
fn prop_all_declared_slots_present_yields_no_missing_finding() {
    proptest::proptest!(|(slot_names in proptest::collection::vec("[a-z]{1,10}", 1..6usize))| {
        let mut artifacts: HashMap<String, serde_json::Value> = HashMap::new();
        for s in &slot_names {
            artifacts.insert(s.clone(), serde_json::json!("v"));
        }
        let inputs = DeclaredNodeInputs {
            required_output_slots: slot_names,
            approved_scope: ApprovedScope {
                artifact_patterns: vec!["**".to_string()],
                max_files: None,
                max_new_files: u32::MAX,
            },
            protected_paths: vec![],
        };
        let node_output = NodeOutput {
            artifacts,
            cost_delta: TokenCost::zero(),
            state_updates: vec![],
        };

        let findings = run_deterministic_alignment(&inputs, &node_output);
        let missing_count = findings
            .iter()
            .filter(|f| f.misalignment == MisalignmentType::Missing)
            .count();
        assert_eq!(
            missing_count, 0,
            "every declared slot is present as an artifact key; no Missing finding expected"
        );
    });
}

/// Property: the number of `Missing` findings equals exactly the number of
/// required slots that are absent from the artifacts map — never more, never
/// fewer, regardless of how many slots are declared or present.
#[test]
fn prop_missing_finding_count_equals_absent_slot_count() {
    proptest::proptest!(|(
        present_slots in proptest::collection::vec("[a-z]{1,8}", 0..4usize),
        absent_slots in proptest::collection::vec("[A-Z]{1,8}", 0..4usize),
    )| {
        let mut artifacts: HashMap<String, serde_json::Value> = HashMap::new();
        for s in &present_slots {
            artifacts.insert(s.clone(), serde_json::json!("v"));
        }
        let mut required: Vec<String> = present_slots.clone();
        required.extend(absent_slots.iter().cloned());

        let inputs = DeclaredNodeInputs {
            required_output_slots: required,
            approved_scope: ApprovedScope {
                artifact_patterns: vec!["**".to_string()],
                max_files: None,
                max_new_files: u32::MAX,
            },
            protected_paths: vec![],
        };
        let node_output = NodeOutput {
            artifacts,
            cost_delta: TokenCost::zero(),
            state_updates: vec![],
        };

        let findings = run_deterministic_alignment(&inputs, &node_output);
        let missing_count = findings
            .iter()
            .filter(|f| f.misalignment == MisalignmentType::Missing)
            .count();
        // present_slots and absent_slots use disjoint character classes
        // (lowercase vs. uppercase), so they cannot collide as HashMap keys.
        assert_eq!(missing_count, absent_slots.len(), "{findings:?}");
    });
}

/// Fuzz: `run_deterministic_alignment` must never panic on arbitrary slot
/// names, artifact keys (including keys that fail `ArtifactPath::new`, e.g.
/// containing `..`), scope patterns, and protected-path entries.
#[test]
fn prop_run_deterministic_alignment_never_panics() {
    proptest::proptest!(|(
        required in proptest::collection::vec(".*", 0..5usize),
        artifact_keys in proptest::collection::vec(".*", 0..8usize),
        scope_patterns in proptest::collection::vec("[a-z*./_]{0,20}", 0..5usize),
        protected_candidates in proptest::collection::vec("[a-z*./_]{0,20}", 0..5usize),
    )| {
        let mut artifacts: HashMap<String, serde_json::Value> = HashMap::new();
        for k in &artifact_keys {
            artifacts.insert(k.clone(), serde_json::json!("v"));
        }
        let protected_paths: Vec<ArtifactPath> = protected_candidates
            .iter()
            .filter_map(|p| ArtifactPath::new(p.clone()))
            .collect();
        let inputs = DeclaredNodeInputs {
            required_output_slots: required,
            approved_scope: ApprovedScope {
                artifact_patterns: scope_patterns,
                max_files: None,
                max_new_files: u32::MAX,
            },
            protected_paths,
        };
        let node_output = NodeOutput {
            artifacts,
            cost_delta: TokenCost::zero(),
            state_updates: vec![],
        };

        let _ = run_deterministic_alignment(&inputs, &node_output);
    });
}

// ─── Group H: PR #170 review remediation regressions ─────────────────────────
//
// These tests target three review findings on `run_deterministic_alignment`
// (see PR #170 review comments on `alignment.rs`):
//
// 1. `check_scope_and_protected_paths` builds each `ProtectedPath.pattern`
//    from `ArtifactPath::as_str()` unescaped, so a *literal* protected path
//    containing glob metacharacters (`[`, `]`, `{`, `}`, `*`, `?`) does not
//    protect itself, because those characters are reinterpreted as glob
//    syntax rather than matched literally. The fix escapes glob
//    metacharacters (e.g. via `globset::escape`) when adapting
//    `ArtifactPath` → `ProtectedPath`.
// 3. `classify_artifact_key` checks protected paths against the *parsed,
//    normalized* `path`, but checks scope containment against the *raw* `key`
//    string. Since `ArtifactPath::new` strips a leading `./`, a key like
//    `"./src/main.rs"` normalizes to `"src/main.rs"` but the scope check
//    still matches the un-normalized raw string, producing a spurious
//    `ScopeExceeded` finding for an in-scope artifact. The fix matches
//    `path.as_str()` instead of `key`.
// 4. `deterministic_finding`'s callers interpolate untrusted,
//    LLM-influenced strings (artifact keys, slot names) directly into
//    `AlignmentFinding.description` with no control-character handling and
//    no length bound. Since findings feed into rework-loop prompts sent back
//    to an LLM, a crafted key with embedded control characters or an
//    extreme length could inject into, or blow up, that downstream prompt.
// The fixtures below are deliberately chosen so that each test FAILS against
// the current (unfixed) implementation, proving each defect is real rather
// than already covered by the existing suite above.

/// Item 1: a protected path containing `[`/`]` glob metacharacters must
/// protect the literal file of the same name. Pre-fix, the unescaped
/// pattern `"src/[id].rs"` is compiled as a glob where `[id]` is a
/// single-character class (matching one of `i` or `d`), so it does not match
/// its own four-character literal span `[id]` and the artifact slips through
/// unprotected.
#[test]
fn test_protected_literal_path_with_bracket_metacharacters_protects_itself() {
    let inputs = declared_inputs(&[], &["**"], &["src/[id].rs"]);
    let node_output = output(&["src/[id].rs"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        1,
        "a declared protected path containing glob metacharacters ('[', ']') must protect \
         the literal artifact of the same name once the pattern is escaped, got {findings:?}"
    );
}

/// Item 1, second metacharacter class: a protected path containing `{`/`}`
/// must likewise protect the literal file of the same name. Pre-fix, the
/// unescaped pattern `"a{b}.md"` is compiled as a glob alternation group
/// (`{b}` matching the single literal `b`), so the pattern only matches
/// `"ab.md"`, never the literal eight-character string `"a{b}.md"` itself.
#[test]
fn test_protected_literal_path_with_brace_metacharacters_protects_itself() {
    let inputs = declared_inputs(&[], &["**"], &["a{b}.md"]);
    let node_output = output(&["a{b}.md"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert_eq!(
        count_of(&findings, MisalignmentType::ScopeExceeded),
        1,
        "a declared protected path containing glob metacharacters ('{{', '}}') must protect \
         the literal artifact of the same name once the pattern is escaped, got {findings:?}"
    );
}

/// Item 1 companion: escaping the protected pattern must not leave it
/// over-broad. Pre-fix, the unescaped pattern `"src/[id].rs"` coincidentally
/// matches the unrelated key `"src/i.rs"` (via the `[id]` character class
/// matching a lone `i`), wrongly protecting a file that was never declared.
/// Once the pattern is escaped to match only the literal `"src/[id].rs"`,
/// this unrelated key must not be flagged.
#[test]
fn test_protected_literal_path_escaping_does_not_match_unrelated_coincidental_key() {
    let inputs = declared_inputs(&[], &["**"], &["src/[id].rs"]);
    let node_output = output(&["src/i.rs"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert!(
        findings.is_empty(),
        "escaping the protected pattern 'src/[id].rs' must not leave it matching the \
         unrelated key 'src/i.rs', which only matched under the old, unescaped \
         character-class interpretation; got {findings:?}"
    );
}

/// Item 3: the scope-containment check must operate on the same normalized
/// path as the protected-path check. `ArtifactPath::new` strips a leading
/// `./`, so the artifact key `"./src/main.rs"` normalizes to `"src/main.rs"`,
/// which exactly matches the deliberately non-wildcard approved pattern
/// `"src/main.rs"`. Pre-fix, the scope check matches the *raw* key
/// `"./src/main.rs"` against that exact-match pattern, which never matches
/// (the leading `./` makes the raw string differ from the pattern), so a
/// fully in-scope artifact is wrongly flagged `ScopeExceeded`.
#[test]
fn test_scope_check_uses_normalized_path_not_raw_key_with_leading_dot_slash() {
    let inputs = declared_inputs(&[], &["src/main.rs"], &[]);
    let node_output = output(&["./src/main.rs"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    assert!(
        findings.is_empty(),
        "artifact key './src/main.rs' normalizes (via ArtifactPath::new, which strips a \
         leading './') to 'src/main.rs', which exactly matches the approved pattern \
         'src/main.rs'; the scope check must match on the normalized path, not the raw \
         key, got {findings:?}"
    );
}

/// Item 4: a `ScopeExceeded` finding's description must never carry a raw
/// control character from an untrusted (LLM-generated) artifact key.
/// Findings feed into rework-loop prompts sent back to an LLM, so an
/// embedded newline in the key must not survive verbatim into the
/// description text.
#[test]
fn test_scope_exceeded_description_does_not_contain_raw_newline_from_artifact_key() {
    let inputs = declared_inputs(&[], &["allowed/**"], &[]);
    let node_output = output(&["evil\nkey"]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    let finding = findings
        .iter()
        .find(|f| f.misalignment == MisalignmentType::ScopeExceeded)
        .expect("expected a ScopeExceeded finding for the out-of-scope key");
    assert!(
        !finding.description.contains('\n'),
        "finding description must not contain a raw newline byte copied verbatim from an \
         untrusted artifact key, got: {:?}",
        finding.description
    );
}

/// Item 4 companion: a `ScopeExceeded` finding's description must be bounded
/// in length even when the untrusted artifact key is extremely long (here,
/// 500 characters). The 400-character threshold leaves ample headroom for
/// the fixed prose around the key while still proving truncation actually
/// happened rather than the raw key appearing verbatim.
#[test]
fn test_scope_exceeded_description_is_bounded_for_extremely_long_artifact_key() {
    let long_key = "a".repeat(500);
    let inputs = declared_inputs(&[], &["allowed/**"], &[]);
    let node_output = output(&[long_key.as_str()]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    let finding = findings
        .iter()
        .find(|f| f.misalignment == MisalignmentType::ScopeExceeded)
        .expect("expected a ScopeExceeded finding for the out-of-scope key");
    assert!(
        finding.description.len() < 400,
        "finding description must truncate an overlong (500-char) artifact key rather than \
         interpolating it verbatim, got description of length {}: {:?}",
        finding.description.len(),
        finding.description
    );
}

/// Item 4, `check_required_slots` side: a `Missing` finding's description
/// must likewise never carry a raw control character from an untrusted
/// required-slot name (slot names, like artifact keys, are declared data
/// that can originate from upstream LLM-influenced configuration).
#[test]
fn test_missing_finding_description_does_not_contain_raw_newline_from_slot_name() {
    let inputs = declared_inputs(&["evil\nslot"], &["**"], &[]);
    let node_output = output(&[]);

    let findings = run_deterministic_alignment(&inputs, &node_output);

    let finding = findings
        .iter()
        .find(|f| f.misalignment == MisalignmentType::Missing)
        .expect("expected a Missing finding for the absent slot");
    assert!(
        !finding.description.contains('\n'),
        "finding description must not contain a raw newline byte copied verbatim from an \
         untrusted required-slot name, got: {:?}",
        finding.description
    );
}
