//! Adversarial test suite for `scenarios.rs` — `compute_satisfaction`.
//!
//! ## Phase: RED
//!
//! All tests compile but will **panic** at runtime because `compute_satisfaction`
//! is a `todo!()` stub. This is the expected RED state in TDD. Tests turn GREEN
//! once the implementation lands.
//!
//! ## Assertions covered
//!
//! - ASSERT-SCEN-004: satisfaction score computed correctly (satisfied/total).
//! - ASSERT-SCEN-005/006: plain threshold comparison semantics (`score >= threshold`).
//! - ASSERT-SCEN-007: an unobserved explicit failure fails validation regardless
//!   of how high the overall score is.
//!
//! ## Tier map
//!
//! | Tier | Count | Focus |
//! |---|---|---|
//! | 1 — Specification | 13 | Direct assertion mapping, field semantics |
//! | 2 — Adversarial / boundary | 14 | N-1/N/N+1 thresholds, stub-killers, ordering |
//! | 3 — Property-based | 5 | Independent oracle cross-check, invariants |
//!
//! ## Spec gaps / ambiguities found (reported to Tech Lead / Architect)
//!
//! 1. **Weighted vs. unweighted mean.** `docs/spec/interfaces/advanced-features.md`
//!    line 87 (field table) says `overall_score` is a "Weighted mean", but the
//!    Behaviour section (step 4) of the same document, the pre-existing rustdoc
//!    on [`super::ScenarioSatisfactionResult::overall_score`], and the worked
//!    example in both docs are only consistent with an **unweighted** (simple
//!    arithmetic) mean. Per the injected Interface Contract, unweighted mean is
//!    authoritative. See `test_compute_satisfaction_overall_score_is_unweighted_mean_across_scenarios`,
//!    which uses deliberately unbalanced group sizes (1 trajectory vs. 9
//!    trajectories) so that a weighted-mean implementation would produce a
//!    materially different (and wrong, per the contract) result (0.9 vs. the
//!    correct 0.5).
//!
//! 2. **`explicit_failure` field semantics.** The injected Interface Contract's
//!    inline Rust comment reads:
//!    `// true if any trajectory in group has expected_failure==true AND it passed (failure observed)`
//!    — which, read literally, would make `explicit_failure` false whenever the
//!    expected failure was *not* observed. This directly contradicts:
//!    - The already-committed rustdoc on [`super::PerScenarioScore::explicit_failure`]
//!      in `scenarios.rs` ("`true` when this is an explicit-failure scenario (at
//!      least one trajectory in the group had `expected_failure == true`)" — no
//!      "AND it passed" qualifier).
//!    - The injected Interface Contract's own prose "Behavioural contract" bullet 3:
//!      "a group is an 'explicit-failure scenario' if ANY trajectory in it has
//!      `expected_failure == true`. For such groups: `explicit_failure` field ...
//!      is true" (unconditional on observation).
//!    - `docs/spec/interfaces/advanced-features.md` Behaviour step 3, which treats
//!      "identifying explicit-failure scenarios" (presence-based) as a distinct
//!      step from "verifying observation" (which feeds `explicit_failure_violations`,
//!      not the `explicit_failure` field).
//!
//!    This suite follows the majority/authoritative reading (3 of 4 sources): the
//!    `explicit_failure` field is set from *presence* of an `expected_failure ==
//!    true` trajectory in the group, independent of whether it was observed.
//!    Whether the failure was *observed* only affects `explicit_failure_violations`.
//!    See `test_compute_satisfaction_explicit_failure_field_true_even_when_failure_not_observed`,
//!    which is the discriminating test for this ambiguity — it will fail against
//!    an implementation that follows the literal inline-comment reading instead.
//!    **Flagging for architect confirmation.**
//!
//! 3. **`satisfied_trajectories` counting inside explicit-failure groups.** The
//!    already-committed rustdoc on [`super::PerScenarioScore::satisfied_trajectories`]
//!    says, for explicit-failure scenarios, it "counts trajectories where the
//!    expected failure was observed (... `expected_failure == true`)" — which
//!    could be read as excluding any normal (non-`expected_failure`) trajectories
//!    in the same group from the satisfied count. The injected Behavioural
//!    contract, however, is explicit and detailed: "satisfaction of the group
//!    counts trajectories where `passed == true` normally (same score
//!    computation as any other group — **this is unchanged**)". This suite
//!    follows the more detailed/authoritative Behavioural contract: satisfied
//!    count = count of *all* trajectories in the group with `passed == true`,
//!    regardless of `expected_failure`. See
//!    `test_compute_satisfaction_satisfied_count_includes_both_normal_and_observed_expected_failure_passes`.

#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::cast_precision_loss)]

use std::collections::BTreeMap;

use proptest::prelude::*;

use super::{PerScenarioScore, ScenarioSatisfactionResult, compute_satisfaction};
use crate::{
    domain_services::{Diagnostics, TrajectoryResult},
    types::SatisfactionScore,
};

// ─── Test helpers ────────────────────────────────────────────────────────────

const EPSILON: f64 = 1e-9;

/// Builds a [`TrajectoryResult`] with the given fields; `diagnostics` is always
/// empty (irrelevant to `compute_satisfaction`'s contract).
fn traj(
    scenario_id: &str,
    passed: bool,
    satisfaction_score: f64,
    expected_failure: bool,
) -> TrajectoryResult {
    TrajectoryResult {
        scenario_id: scenario_id.to_string(),
        passed,
        satisfaction_score: SatisfactionScore::new(satisfaction_score)
            .expect("test satisfaction_score must be within [0.0, 1.0]"),
        expected_failure,
        diagnostics: Diagnostics::empty(),
    }
}

/// Builds `total` trajectories for `scenario_id`, the first `passed_count` of
/// which have `passed == true`.
fn repeat_trajectories(
    scenario_id: &str,
    passed_count: u32,
    total: u32,
    expected_failure: bool,
) -> Vec<TrajectoryResult> {
    (0..total)
        .map(|i| traj(scenario_id, i < passed_count, 0.5, expected_failure))
        .collect()
}

fn score(v: f64) -> SatisfactionScore {
    SatisfactionScore::new(v).expect("test score must be within [0.0, 1.0]")
}

fn find<'a>(result: &'a ScenarioSatisfactionResult, id: &str) -> &'a PerScenarioScore {
    result
        .per_scenario
        .iter()
        .find(|p| p.scenario_id == id)
        .unwrap_or_else(|| panic!("expected scenario_id '{id}' in per_scenario, found none"))
}

fn assert_close(actual: f64, expected: f64, context: &str) {
    assert!(
        (actual - expected).abs() < EPSILON,
        "{context}: expected {expected:.9}, got {actual:.9}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 1: Specification Tests
// ═══════════════════════════════════════════════════════════════════════════

/// ASSERT-SCEN-004: 10 trajectories, 9 satisfy criteria, 1 does not → score = 0.9.
#[test]
fn test_compute_satisfaction_ten_trajectories_nine_satisfied_score_is_zero_point_nine() {
    let trajectories = repeat_trajectories("sc-01", 9, 10, false);
    let threshold = score(0.5);

    let result = compute_satisfaction(&trajectories, threshold);

    let entry = find(&result, "sc-01");
    assert_eq!(entry.satisfied_trajectories, 9);
    assert_eq!(entry.total_trajectories, 10);
    assert_close(entry.score.as_f64(), 0.9, "ASSERT-SCEN-004 score");
}

/// ASSERT-SCEN-005/006: `score == threshold` must pass (`>=`, not `>`).
#[test]
fn test_compute_satisfaction_score_equal_to_threshold_passes() {
    // 2 trajectories, 1 passed → score = 0.5. Threshold = 0.5 exactly.
    let trajectories = repeat_trajectories("sc-01", 1, 2, false);
    let threshold = score(0.5);

    let result = compute_satisfaction(&trajectories, threshold);

    assert!(
        find(&result, "sc-01").passed,
        "score == threshold must pass"
    );
}

/// ASSERT-SCEN-005/006: `score < threshold` must fail.
#[test]
fn test_compute_satisfaction_score_below_threshold_fails() {
    // 2 trajectories, 1 passed → score = 0.5. Threshold = 0.500001 (just above).
    let trajectories = repeat_trajectories("sc-01", 1, 2, false);
    let threshold = score(0.500_001);

    let result = compute_satisfaction(&trajectories, threshold);

    assert!(
        !find(&result, "sc-01").passed,
        "score strictly below threshold must fail"
    );
}

/// ASSERT-SCEN-007 (core assertion): overall score 0.98 (well above any
/// reasonable threshold) but one trajectory's expected failure was NOT
/// observed → overall validation fails, regardless of the high score.
#[test]
fn test_compute_satisfaction_high_score_but_unobserved_explicit_failure_fails_validation() {
    // A single scenario group of 50 trajectories: 49 normal passes, plus 1
    // explicit-failure trajectory whose failure was NOT observed (passed=false).
    // satisfied = 49 (only passed==true trajectories count), total = 50 →
    // score = 49/50 = 0.98.
    let mut trajectories = repeat_trajectories("sc-01", 49, 49, false);
    trajectories.push(traj("sc-01", false, 0.0, true)); // expected failure NOT observed
    let threshold = score(0.5);

    let result = compute_satisfaction(&trajectories, threshold);

    assert_close(
        result.overall_score.as_f64(),
        0.98,
        "ASSERT-SCEN-007 overall_score",
    );
    assert!(
        !result.passed,
        "ASSERT-SCEN-007 VIOLATION: overall passed must be false despite score 0.98 \
         when an expected failure was not observed"
    );
    assert_eq!(
        result.explicit_failure_violations,
        vec!["sc-01".to_string()],
        "unobserved explicit failure must be recorded in explicit_failure_violations"
    );
}

/// Explicit-failure semantics, positive path: failure IS observed → no violation.
#[test]
fn test_compute_satisfaction_explicit_failure_observed_no_violation() {
    let trajectories = vec![traj("sc-02", true, 1.0, true)];
    let threshold = score(0.5);

    let result = compute_satisfaction(&trajectories, threshold);

    assert!(
        result.explicit_failure_violations.is_empty(),
        "observed explicit failure must not produce a violation"
    );
    assert!(result.passed, "observed explicit failure scenario passes");
}

/// Explicit-failure semantics, negative path: failure NOT observed → violation recorded.
#[test]
fn test_compute_satisfaction_explicit_failure_not_observed_recorded_as_violation() {
    let trajectories = vec![traj("sc-03", false, 0.0, true)];
    let threshold = score(0.5);

    let result = compute_satisfaction(&trajectories, threshold);

    assert_eq!(
        result.explicit_failure_violations,
        vec!["sc-03".to_string()]
    );
}

/// Empty input → vacuously-satisfied result.
#[test]
fn test_compute_satisfaction_empty_trajectories_returns_vacuous_pass() {
    let result = compute_satisfaction(&[], score(0.9));

    assert!(result.per_scenario.is_empty());
    assert_close(
        result.overall_score.as_f64(),
        1.0,
        "empty input overall_score",
    );
    assert!(result.passed);
    assert!(result.explicit_failure_violations.is_empty());
}

/// Grouping: distinct `scenario_id`s produce distinct `per_scenario` entries.
#[test]
fn test_compute_satisfaction_groups_multiple_scenario_ids_into_separate_entries() {
    let mut trajectories = repeat_trajectories("sc-a", 2, 2, false);
    trajectories.extend(repeat_trajectories("sc-b", 1, 3, false));
    trajectories.extend(repeat_trajectories("sc-c", 4, 4, false));
    let threshold = score(0.5);

    let result = compute_satisfaction(&trajectories, threshold);

    assert_eq!(result.per_scenario.len(), 3);
    assert_eq!(find(&result, "sc-a").total_trajectories, 2);
    assert_eq!(find(&result, "sc-b").total_trajectories, 3);
    assert_eq!(find(&result, "sc-c").total_trajectories, 4);
}

/// All scenarios pass → overall `passed == true`.
#[test]
fn test_compute_satisfaction_all_scenarios_pass_overall_passed_true() {
    let mut trajectories = repeat_trajectories("sc-a", 5, 5, false);
    trajectories.extend(repeat_trajectories("sc-b", 4, 4, false));
    let threshold = score(0.9);

    let result = compute_satisfaction(&trajectories, threshold);

    assert!(result.passed);
    assert!(result.explicit_failure_violations.is_empty());
}

/// One scenario group fails → overall `passed == false`, even though other
/// scenarios pass.
#[test]
fn test_compute_satisfaction_one_scenario_fails_overall_passed_false() {
    let mut trajectories = repeat_trajectories("sc-a", 5, 5, false); // score 1.0
    trajectories.extend(repeat_trajectories("sc-b", 1, 5, false)); // score 0.2
    let threshold = score(0.5);

    let result = compute_satisfaction(&trajectories, threshold);

    assert!(find(&result, "sc-a").passed);
    assert!(!find(&result, "sc-b").passed);
    assert!(
        !result.passed,
        "one failing scenario must make overall passed false"
    );
}

/// `overall_score` is the UNWEIGHTED mean of per-scenario scores, not weighted
/// by trajectory count. See module doc "Spec gaps" item 1.
#[test]
fn test_compute_satisfaction_overall_score_is_unweighted_mean_across_scenarios() {
    // sc-a: 1 trajectory, 0 passed → score 0.0.
    // sc-b: 9 trajectories, all 9 passed → score 1.0.
    // Unweighted mean: (0.0 + 1.0) / 2 = 0.5.
    // Weighted mean (by trajectory count) would be: (0*1 + 1*9) / 10 = 0.9.
    let mut trajectories = repeat_trajectories("sc-a", 0, 1, false);
    trajectories.extend(repeat_trajectories("sc-b", 9, 9, false));
    let threshold = score(0.1);

    let result = compute_satisfaction(&trajectories, threshold);

    assert_close(
        result.overall_score.as_f64(),
        0.5,
        "overall_score must be the unweighted mean (0.5), not the trajectory-weighted mean (0.9)",
    );
}

/// `explicit_failure` field is set from PRESENCE of an `expected_failure ==
/// true` trajectory, independent of whether it was observed. See module doc
/// "Spec gaps" item 2 — this is the discriminating test for that ambiguity.
#[test]
fn test_compute_satisfaction_explicit_failure_field_true_even_when_failure_not_observed() {
    let trajectories = vec![traj("sc-04", false, 0.0, true)]; // NOT observed
    let threshold = score(0.5);

    let result = compute_satisfaction(&trajectories, threshold);

    let entry = find(&result, "sc-04");
    assert!(
        entry.explicit_failure,
        "explicit_failure field must be true whenever the group contains an \
         expected_failure==true trajectory, regardless of whether it was observed"
    );
}

/// `explicit_failure` is false for a group with no `expected_failure == true` trajectories.
#[test]
fn test_compute_satisfaction_explicit_failure_field_false_for_normal_scenario() {
    let trajectories = repeat_trajectories("sc-05", 3, 3, false);
    let threshold = score(0.5);

    let result = compute_satisfaction(&trajectories, threshold);

    assert!(!find(&result, "sc-05").explicit_failure);
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 2: Adversarial / Boundary Tests
// ═══════════════════════════════════════════════════════════════════════════

/// N+1 boundary: score strictly above threshold passes.
#[test]
fn test_compute_satisfaction_score_just_above_threshold_passes() {
    // 10 trajectories, 7 passed → score 0.7. Threshold 0.6.
    let trajectories = repeat_trajectories("sc-01", 7, 10, false);
    let threshold = score(0.6);

    let result = compute_satisfaction(&trajectories, threshold);

    assert!(find(&result, "sc-01").passed);
}

/// N-1 boundary: score strictly below threshold fails (paired with the N and
/// N+1 cases above/below for a full 3-point boundary sweep).
#[test]
fn test_compute_satisfaction_score_just_below_threshold_fails_at_boundary() {
    // 10 trajectories, 5 passed → score 0.5. Threshold 0.6.
    let trajectories = repeat_trajectories("sc-01", 5, 10, false);
    let threshold = score(0.6);

    let result = compute_satisfaction(&trajectories, threshold);

    assert!(!find(&result, "sc-01").passed);
}

/// Threshold 0.0 (minimum boundary): any score, including 0.0, passes.
#[test]
fn test_compute_satisfaction_zero_threshold_always_passes() {
    let trajectories = repeat_trajectories("sc-01", 0, 5, false); // score 0.0
    let threshold = score(0.0);

    let result = compute_satisfaction(&trajectories, threshold);

    assert!(
        find(&result, "sc-01").passed,
        "score 0.0 >= threshold 0.0 must pass"
    );
}

/// Threshold 1.0 (maximum boundary): only a perfect score passes.
#[test]
fn test_compute_satisfaction_threshold_one_requires_perfect_score() {
    let trajectories = repeat_trajectories("sc-01", 4, 5, false); // score 0.8
    let threshold = score(1.0);

    let result = compute_satisfaction(&trajectories, threshold);

    assert!(
        !find(&result, "sc-01").passed,
        "score 0.8 < threshold 1.0 must fail"
    );
}

/// Threshold 1.0 with a perfect score passes.
#[test]
fn test_compute_satisfaction_threshold_one_with_perfect_score_passes() {
    let trajectories = repeat_trajectories("sc-01", 5, 5, false); // score 1.0
    let threshold = score(1.0);

    let result = compute_satisfaction(&trajectories, threshold);

    assert!(find(&result, "sc-01").passed);
}

/// Multiple explicit-failure scenarios: only the ones whose failure was NOT
/// observed appear in `explicit_failure_violations`; observed ones are absent.
#[test]
fn test_compute_satisfaction_multiple_explicit_failure_scenarios_only_violated_ones_listed() {
    let mut trajectories = vec![traj("sc-observed", true, 1.0, true)]; // observed, no violation
    trajectories.push(traj("sc-violated-1", false, 0.0, true)); // not observed
    trajectories.push(traj("sc-violated-2", false, 0.0, true)); // not observed
    let threshold = score(0.1);

    let result = compute_satisfaction(&trajectories, threshold);

    let mut violations = result.explicit_failure_violations.clone();
    violations.sort();
    assert_eq!(
        violations,
        vec!["sc-violated-1".to_string(), "sc-violated-2".to_string()]
    );
    assert!(!violations.contains(&"sc-observed".to_string()));
}

/// A scenario group mixing normal and expected-failure trajectories: the
/// satisfied count includes BOTH normal passes and the observed expected
/// failure (per the authoritative Behavioural contract — see module doc
/// "Spec gaps" item 3).
#[test]
fn test_compute_satisfaction_satisfied_count_includes_both_normal_and_observed_expected_failure_passes()
 {
    let trajectories = vec![
        traj("sc-mix", true, 1.0, false),  // normal pass
        traj("sc-mix", true, 1.0, true),   // expected failure observed
        traj("sc-mix", false, 0.0, false), // normal fail
    ];
    let threshold = score(0.1);

    let result = compute_satisfaction(&trajectories, threshold);

    let entry = find(&result, "sc-mix");
    assert_eq!(
        entry.satisfied_trajectories, 2,
        "satisfied count must include both the normal pass and the observed \
         expected-failure trajectory (2 of 3 have passed==true)"
    );
    assert_eq!(entry.total_trajectories, 3);
    assert_close(entry.score.as_f64(), 2.0 / 3.0, "mixed-group score");
    assert!(entry.explicit_failure);
    assert!(
        result.explicit_failure_violations.is_empty(),
        "the failure WAS observed by one trajectory in the group; no violation"
    );
}

/// A group with multiple `expected_failure == true` trajectories: as long as
/// AT LEAST ONE was observed, there is no violation.
#[test]
fn test_compute_satisfaction_multiple_expected_failure_trajectories_at_least_one_observed_no_violation()
 {
    let trajectories = vec![
        traj("sc-multi", false, 0.0, true), // not observed
        traj("sc-multi", true, 1.0, true),  // observed
        traj("sc-multi", true, 1.0, false), // normal pass
    ];
    let threshold = score(0.1);

    let result = compute_satisfaction(&trajectories, threshold);

    assert!(
        result.explicit_failure_violations.is_empty(),
        "at least one observed expected failure in the group must prevent a violation"
    );
}

/// Large trajectory counts exercise `u32` counting arithmetic beyond trivial
/// small numbers.
#[test]
fn test_compute_satisfaction_large_trajectory_count_u32_counts_correct() {
    let trajectories = repeat_trajectories("sc-big", 733, 1000, false);
    let threshold = score(0.5);

    let result = compute_satisfaction(&trajectories, threshold);

    let entry = find(&result, "sc-big");
    assert_eq!(entry.satisfied_trajectories, 733);
    assert_eq!(entry.total_trajectories, 1000);
    assert_close(entry.score.as_f64(), 0.733, "large-count score");
}

/// Reversing the input order must not change the result (grouping is
/// order-independent; a HashMap-backed implementation guarantees this
/// naturally, but a buggy positional implementation might not).
#[test]
fn test_compute_satisfaction_ordering_independence_reversed_input_same_result() {
    let mut trajectories = repeat_trajectories("sc-a", 3, 5, false);
    trajectories.extend(repeat_trajectories("sc-b", 2, 2, false));
    trajectories.push(traj("sc-c", false, 0.0, true)); // unobserved explicit failure
    let threshold = score(0.5);

    let forward = compute_satisfaction(&trajectories, threshold);
    let mut reversed_input = trajectories;
    reversed_input.reverse();
    let backward = compute_satisfaction(&reversed_input, threshold);

    assert_close(
        forward.overall_score.as_f64(),
        backward.overall_score.as_f64(),
        "overall_score must be order-independent",
    );
    assert_eq!(forward.passed, backward.passed);

    let mut forward_violations = forward.explicit_failure_violations.clone();
    let mut backward_violations = backward.explicit_failure_violations.clone();
    forward_violations.sort();
    backward_violations.sort();
    assert_eq!(forward_violations, backward_violations);

    for id in ["sc-a", "sc-b", "sc-c"] {
        let f = find(&forward, id);
        let b = find(&backward, id);
        assert_eq!(
            f.satisfied_trajectories, b.satisfied_trajectories,
            "id={id}"
        );
        assert_eq!(f.total_trajectories, b.total_trajectories, "id={id}");
        assert_eq!(f.passed, b.passed, "id={id}");
        assert_eq!(f.explicit_failure, b.explicit_failure, "id={id}");
    }
}

/// Stub-killer: the implementation must use `TrajectoryResult::passed`
/// (the boolean) to count satisfied trajectories, NOT the raw
/// `satisfaction_score` field. A trajectory with a high raw score but
/// `passed == false` must NOT count as satisfied, and vice versa.
#[test]
fn test_compute_satisfaction_uses_passed_field_not_raw_satisfaction_score_for_counting() {
    let trajectories = vec![
        traj("sc-01", false, 0.99, false), // high raw score, but did NOT pass
        traj("sc-01", true, 0.01, false),  // low raw score, but DID pass
    ];
    let threshold = score(0.1);

    let result = compute_satisfaction(&trajectories, threshold);

    let entry = find(&result, "sc-01");
    assert_eq!(
        entry.satisfied_trajectories, 1,
        "must count based on `passed`, not `satisfaction_score`"
    );
    assert_close(entry.score.as_f64(), 0.5, "score must be 1/2 = 0.5");
}

/// Stub-killer: `scenario_id` must be preserved verbatim from input, not
/// replaced with a constant or empty string.
#[test]
fn test_compute_satisfaction_scenario_id_preserved_verbatim_in_output() {
    let trajectories = vec![traj("unusual-scenario-id-42", true, 1.0, false)];
    let threshold = score(0.5);

    let result = compute_satisfaction(&trajectories, threshold);

    assert_eq!(result.per_scenario.len(), 1);
    assert_eq!(result.per_scenario[0].scenario_id, "unusual-scenario-id-42");
}

/// Stub-killer: two scenarios with different pass ratios must produce
/// distinctly different `score` values (rules out a hardcoded constant score).
#[test]
fn test_compute_satisfaction_two_scenarios_produce_distinct_scores_not_hardcoded() {
    let mut trajectories = repeat_trajectories("sc-low", 1, 10, false); // 0.1
    trajectories.extend(repeat_trajectories("sc-high", 9, 10, false)); // 0.9
    let threshold = score(0.05);

    let result = compute_satisfaction(&trajectories, threshold);

    let low = find(&result, "sc-low").score.as_f64();
    let high = find(&result, "sc-high").score.as_f64();
    assert_close(low, 0.1, "sc-low score");
    assert_close(high, 0.9, "sc-high score");
    assert!(
        (high - low).abs() > 0.5,
        "scores must differ meaningfully between scenarios with different pass ratios"
    );
}

/// Stub-killer: `passed` must be false when the score is 0.0 for any
/// non-trivial threshold (rules out a hardcoded `passed: true` stub).
#[test]
fn test_compute_satisfaction_result_passed_false_when_all_scores_zero() {
    let trajectories = repeat_trajectories("sc-01", 0, 5, false);
    let threshold = score(0.01);

    let result = compute_satisfaction(&trajectories, threshold);

    assert!(!find(&result, "sc-01").passed);
    assert!(!result.passed);
}

/// Single-trajectory scenario: score must be exactly 0.0 or 1.0 depending on
/// the sole trajectory's `passed` flag (boundary total_trajectories == 1).
#[test]
fn test_compute_satisfaction_single_trajectory_scenario_score_matches_passed_flag() {
    let passing = compute_satisfaction(&[traj("sc-p", true, 1.0, false)], score(0.5));
    let failing = compute_satisfaction(&[traj("sc-f", false, 0.0, false)], score(0.5));

    assert_close(
        find(&passing, "sc-p").score.as_f64(),
        1.0,
        "single passing trajectory",
    );
    assert_close(
        find(&failing, "sc-f").score.as_f64(),
        0.0,
        "single failing trajectory",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 3: Property-Based Tests
// ═══════════════════════════════════════════════════════════════════════════

/// Generates a scenario_id from a small fixed pool so that proptest inputs
/// exercise realistic grouping (repeated ids) rather than always-unique ids.
fn scenario_id_strategy() -> impl Strategy<Value = String> {
    (0usize..4).prop_map(|i| format!("s{i}"))
}

/// Generates a single `TrajectoryResult` with a fixed valid `satisfaction_score`
/// (0.5) — the raw score value must not influence `compute_satisfaction`'s
/// output per the contract, so it is held constant here and varied separately
/// in `prop_compute_satisfaction_ignores_raw_satisfaction_score_field`.
fn trajectory_strategy() -> impl Strategy<Value = TrajectoryResult> {
    (scenario_id_strategy(), any::<bool>(), any::<bool>())
        .prop_map(|(id, passed, expected_failure)| traj(&id, passed, 0.5, expected_failure))
}

/// Independently recomputes, per scenario_id, `(satisfied, total,
/// has_expected_failure, failure_observed)` — a from-scratch oracle that does
/// not share code with any candidate implementation.
fn oracle_stats(trajectories: &[TrajectoryResult]) -> BTreeMap<String, (u32, u32, bool, bool)> {
    let mut map: BTreeMap<String, (u32, u32, bool, bool)> = BTreeMap::new();
    for t in trajectories {
        let entry = map
            .entry(t.scenario_id.clone())
            .or_insert((0, 0, false, false));
        entry.1 += 1;
        if t.passed {
            entry.0 += 1;
        }
        if t.expected_failure {
            entry.2 = true;
            if t.passed {
                entry.3 = true;
            }
        }
    }
    map
}

proptest! {
    /// Comprehensive invariant: `compute_satisfaction`'s output must match an
    /// independently-computed oracle across all fields — grouping, counts,
    /// per-scenario score/passed, explicit_failure, overall_score, overall
    /// passed, and explicit_failure_violations — for arbitrary generated
    /// trajectory sets and thresholds.
    #[test]
    fn prop_compute_satisfaction_matches_independent_oracle(
        trajectories in proptest::collection::vec(trajectory_strategy(), 0..30),
        threshold_f64 in 0.0f64..=1.0,
    ) {
        let threshold = score(threshold_f64);
        let result = compute_satisfaction(&trajectories, threshold);
        let oracle = oracle_stats(&trajectories);

        prop_assert_eq!(result.per_scenario.len(), oracle.len());

        let mut expected_overall_sum = 0.0f64;
        let mut expected_violations: Vec<String> = Vec::new();
        let mut expected_all_passed = true;

        for (id, (satisfied, total, has_ef, observed)) in &oracle {
            let entry = result
                .per_scenario
                .iter()
                .find(|p| &p.scenario_id == id);
            prop_assert!(entry.is_some(), "missing scenario '{}' in result", id);
            let entry = entry.unwrap();

            prop_assert_eq!(entry.satisfied_trajectories, *satisfied);
            prop_assert_eq!(entry.total_trajectories, *total);
            prop_assert_eq!(
                entry.explicit_failure, *has_ef,
                "explicit_failure must equal presence of an expected_failure==true trajectory"
            );

            let expected_score = if *total == 0 {
                1.0
            } else {
                f64::from(*satisfied) / f64::from(*total)
            };
            prop_assert!((entry.score.as_f64() - expected_score).abs() < EPSILON);

            let expected_passed = entry.score.as_f64() >= threshold.as_f64();
            prop_assert_eq!(entry.passed, expected_passed);
            if !expected_passed {
                expected_all_passed = false;
            }

            expected_overall_sum += expected_score;

            if *has_ef && !*observed {
                expected_violations.push(id.clone());
            }
        }

        let expected_overall = if oracle.is_empty() {
            1.0
        } else {
            expected_overall_sum / oracle.len() as f64
        };
        prop_assert!((result.overall_score.as_f64() - expected_overall).abs() < EPSILON);

        let mut actual_violations = result.explicit_failure_violations.clone();
        actual_violations.sort();
        expected_violations.sort();
        prop_assert_eq!(actual_violations, expected_violations.clone());

        let expected_passed = expected_all_passed && expected_violations.is_empty();
        prop_assert_eq!(result.passed, expected_passed);
    }

    /// Reordering the input (rotate + reverse, seeded) must not change the
    /// result: same per-scenario stats (compared as a sorted set), same
    /// overall_score, same passed, same violations (as a sorted set).
    #[test]
    fn prop_compute_satisfaction_result_independent_of_input_order(
        trajectories in proptest::collection::vec(trajectory_strategy(), 1..20),
        threshold_f64 in 0.0f64..=1.0,
        rotate_seed in any::<u32>(),
    ) {
        let threshold = score(threshold_f64);
        let result_a = compute_satisfaction(&trajectories, threshold);

        let mut shuffled = trajectories.clone();
        let len = shuffled.len();
        let rotate_by = (rotate_seed as usize) % len;
        shuffled.rotate_left(rotate_by);
        shuffled.reverse();
        let result_b = compute_satisfaction(&shuffled, threshold);

        let mut a_sorted: Vec<(String, u32, u32, bool, bool)> = result_a
            .per_scenario
            .iter()
            .map(|p| {
                (
                    p.scenario_id.clone(),
                    p.satisfied_trajectories,
                    p.total_trajectories,
                    p.passed,
                    p.explicit_failure,
                )
            })
            .collect();
        let mut b_sorted: Vec<(String, u32, u32, bool, bool)> = result_b
            .per_scenario
            .iter()
            .map(|p| {
                (
                    p.scenario_id.clone(),
                    p.satisfied_trajectories,
                    p.total_trajectories,
                    p.passed,
                    p.explicit_failure,
                )
            })
            .collect();
        a_sorted.sort();
        b_sorted.sort();

        prop_assert_eq!(a_sorted, b_sorted);
        prop_assert!(
            (result_a.overall_score.as_f64() - result_b.overall_score.as_f64()).abs() < EPSILON
        );
        prop_assert_eq!(result_a.passed, result_b.passed);

        let mut va = result_a.explicit_failure_violations.clone();
        let mut vb = result_b.explicit_failure_violations.clone();
        va.sort();
        vb.sort();
        prop_assert_eq!(va, vb);
    }

    /// The raw `satisfaction_score` field on `TrajectoryResult` must NOT
    /// influence `compute_satisfaction`'s output — only `passed`,
    /// `expected_failure`, and `scenario_id` matter. Two otherwise-identical
    /// trajectories differing only in `satisfaction_score` must produce
    /// identical `PerScenarioScore` results.
    #[test]
    fn prop_compute_satisfaction_ignores_raw_satisfaction_score_field(
        scenario_idx in 0usize..4,
        passed in any::<bool>(),
        expected_failure in any::<bool>(),
        score_a in 0.0f64..=1.0,
        score_b in 0.0f64..=1.0,
        threshold_f64 in 0.0f64..=1.0,
    ) {
        let id = format!("s{scenario_idx}");
        let t_a = traj(&id, passed, score_a, expected_failure);
        let t_b = traj(&id, passed, score_b, expected_failure);
        let threshold = score(threshold_f64);

        let result_a = compute_satisfaction(std::slice::from_ref(&t_a), threshold);
        let result_b = compute_satisfaction(std::slice::from_ref(&t_b), threshold);

        let entry_a = &result_a.per_scenario[0];
        let entry_b = &result_b.per_scenario[0];

        prop_assert_eq!(entry_a.satisfied_trajectories, entry_b.satisfied_trajectories);
        prop_assert_eq!(entry_a.total_trajectories, entry_b.total_trajectories);
        prop_assert!((entry_a.score.as_f64() - entry_b.score.as_f64()).abs() < EPSILON);
        prop_assert_eq!(entry_a.passed, entry_b.passed);
        prop_assert_eq!(entry_a.explicit_failure, entry_b.explicit_failure);
    }

    /// `overall_score` must always land in the valid `[0.0, 1.0]` range for
    /// any generated input (the `SatisfactionScore` type itself enforces this
    /// at construction, but a mutant producing an unclamped raw f64 written
    /// via an internal bypass would violate the invariant at the boundary).
    #[test]
    fn prop_compute_satisfaction_overall_score_always_within_unit_interval(
        trajectories in proptest::collection::vec(trajectory_strategy(), 0..25),
        threshold_f64 in 0.0f64..=1.0,
    ) {
        let threshold = score(threshold_f64);
        let result = compute_satisfaction(&trajectories, threshold);

        prop_assert!(result.overall_score.as_f64() >= 0.0);
        prop_assert!(result.overall_score.as_f64() <= 1.0);
        for entry in &result.per_scenario {
            prop_assert!(entry.score.as_f64() >= 0.0);
            prop_assert!(entry.score.as_f64() <= 1.0);
        }
    }

    /// Invariant: `result.passed == true` implies every per-scenario `passed`
    /// is true AND `explicit_failure_violations` is empty (the AND-definition
    /// from the contract, checked in the "implies" direction across random
    /// inputs — the converse is checked by
    /// `prop_compute_satisfaction_matches_independent_oracle`).
    #[test]
    fn prop_compute_satisfaction_passed_implies_no_violations_and_all_scenarios_passed(
        trajectories in proptest::collection::vec(trajectory_strategy(), 0..25),
        threshold_f64 in 0.0f64..=1.0,
    ) {
        let threshold = score(threshold_f64);
        let result = compute_satisfaction(&trajectories, threshold);

        if result.passed {
            prop_assert!(
                result.per_scenario.iter().all(|p| p.passed),
                "overall passed=true but some per_scenario.passed=false"
            );
            prop_assert!(
                result.explicit_failure_violations.is_empty(),
                "overall passed=true but explicit_failure_violations is non-empty"
            );
        }
    }
}
