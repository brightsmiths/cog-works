//! Scenario satisfaction scoring.
//!
//! This module aggregates raw scenario trajectory results (produced by the
//! [`crate::ScenarioExecutor`] trait) into per-scenario scores and an overall
//! pass/fail determination.
//!
//! ## Explicit-Failure Scenarios
//!
//! Some scenarios are designed to verify that the system *fails safely* in a
//! given situation (e.g. a sensor disconnection causes a graceful stop rather
//! than undefined behaviour). These are marked with
//! `TrajectoryResult::expected_failure == true`. For these scenarios,
//! satisfaction means the expected failure was observed; if the failure was
//! *not* observed, the scenario is considered violated and reported in
//! `ScenarioSatisfactionResult::explicit_failure_violations`.
//!
//! ## Pure Business Logic
//!
//! No I/O. [`compute_satisfaction`] is deterministic for identical inputs.
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/advanced-features.md` §Scenario Satisfaction for
//! the full contract and examples.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{domain_services::TrajectoryResult, types::SatisfactionScore};

// ─── Per-scenario score ───────────────────────────────────────────────────────

/// Pass/fail result for a single scenario, including a fractional satisfaction
/// score and explicit-failure detection.
///
/// One `PerScenarioScore` is produced for each distinct `scenario_id` found
/// across the trajectory results passed to [`compute_satisfaction`].
///
/// See `docs/spec/interfaces/advanced-features.md` §`PerScenarioScore`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerScenarioScore {
    /// Identifier matching [`crate::Scenario::id`].
    pub scenario_id: String,

    /// Count of trajectories that met all acceptance criteria.
    ///
    /// This counts every trajectory in the group with `TrajectoryResult::passed
    /// == true`, whether it is a normal-scenario pass or an observed
    /// expected-failure pass (`TrajectoryResult::expected_failure == true`
    /// combined with `passed == true`).
    pub satisfied_trajectories: u32,

    /// Total number of trajectories executed for this scenario.
    pub total_trajectories: u32,

    /// Fraction of trajectories that were satisfied, in `[0.0, 1.0]`.
    ///
    /// Computed as `satisfied_trajectories / total_trajectories`. When
    /// `total_trajectories == 0` this is `1.0` (vacuously satisfied).
    pub score: SatisfactionScore,

    /// `true` when `score >= threshold` (where threshold was passed to
    /// [`compute_satisfaction`]).
    pub passed: bool,

    /// `true` when this is an explicit-failure scenario (at least one
    /// trajectory in the group had `expected_failure == true`).
    pub explicit_failure: bool,
}

// ─── Aggregate result ─────────────────────────────────────────────────────────

/// Aggregated result for all scenarios executed in one simulation pass.
///
/// Produced by [`compute_satisfaction`] from a flat slice of
/// [`TrajectoryResult`] values.
///
/// See `docs/spec/interfaces/advanced-features.md` §`ScenarioSatisfactionResult`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioSatisfactionResult {
    /// Per-scenario score breakdown; one entry per distinct `scenario_id`.
    pub per_scenario: Vec<PerScenarioScore>,

    /// Unweighted mean of all per-scenario scores, in `[0.0, 1.0]`.
    ///
    /// When `per_scenario` is empty this is `1.0` (vacuously satisfied).
    pub overall_score: SatisfactionScore,

    /// `true` when:
    /// - Every per-scenario `passed == true`, **and**
    /// - `explicit_failure_violations` is empty.
    pub passed: bool,

    /// Scenario IDs of expected-failure scenarios whose failure was *not*
    /// observed.
    ///
    /// An entry here means a scenario designed to verify graceful failure
    /// behaviour succeeded when it should have failed — a potential safety
    /// concern for safety-critical systems.
    pub explicit_failure_violations: Vec<String>,
}

// ─── Core function ────────────────────────────────────────────────────────────

/// Aggregates raw trajectory results into per-scenario scores and an overall
/// satisfaction determination.
///
/// # Arguments
///
/// * `trajectory_results` — flat slice of per-trajectory outcomes returned by
///   [`crate::ScenarioExecutor::execute_trajectory`].
/// * `threshold` — minimum per-scenario score required to consider a scenario
///   passed.
///
/// # Returns
///
/// A [`ScenarioSatisfactionResult`] with:
/// - One [`PerScenarioScore`] per distinct `scenario_id`.
/// - `overall_score` as the unweighted mean of per-scenario scores.
/// - `passed` iff all scenarios pass **and** no explicit-failure violations
///   are found.
///
/// # Empty Input
///
/// When `trajectory_results` is empty returns a result with `passed: true`
/// and `overall_score: 1.0` (vacuously satisfied — no scenarios to fail).
///
/// # Examples
///
/// ```no_run
/// use pipeline::scenarios::compute_satisfaction;
/// use pipeline::domain_services::TrajectoryResult;
/// use pipeline::types::SatisfactionScore;
///
/// let results = vec![
///     TrajectoryResult {
///         scenario_id: "sc-01".to_string(),
///         passed: true,
///         satisfaction_score: SatisfactionScore::new(1.0).unwrap(),
///         expected_failure: false,
///         diagnostics: pipeline::domain_services::Diagnostics::empty(),
///     },
///     TrajectoryResult {
///         scenario_id: "sc-01".to_string(),
///         passed: false,
///         satisfaction_score: SatisfactionScore::new(0.0).unwrap(),
///         expected_failure: false,
///         diagnostics: pipeline::domain_services::Diagnostics::empty(),
///     },
/// ];
/// let threshold = SatisfactionScore::new(0.5).unwrap();
/// let result = compute_satisfaction(&results, threshold);
/// // sc-01: 1/2 passed → overall score 0.5 → passed (0.5 >= 0.5)
/// assert!(result.passed);
/// ```
///
/// See `docs/spec/interfaces/advanced-features.md` §`compute_satisfaction`.
#[must_use]
pub fn compute_satisfaction(
    trajectory_results: &[TrajectoryResult],
    threshold: SatisfactionScore,
) -> ScenarioSatisfactionResult {
    let groups = build_scenario_groups(trajectory_results);

    let mut per_scenario = Vec::with_capacity(groups.len());
    let mut explicit_failure_violations = Vec::new();

    for (scenario_id, group) in &groups {
        let score = score_from_fraction(group.satisfied, group.total);
        let passed = score >= threshold;

        if group.has_explicit_failure && !group.explicit_failure_observed {
            explicit_failure_violations.push(scenario_id.clone());
        }

        per_scenario.push(PerScenarioScore {
            scenario_id: scenario_id.clone(),
            satisfied_trajectories: group.satisfied,
            total_trajectories: group.total,
            score,
            passed,
            explicit_failure: group.has_explicit_failure,
        });
    }

    let scores: Vec<SatisfactionScore> = per_scenario.iter().map(|entry| entry.score).collect();
    let overall_score = mean_score(&scores);
    let passed =
        per_scenario.iter().all(|entry| entry.passed) && explicit_failure_violations.is_empty();

    ScenarioSatisfactionResult {
        per_scenario,
        overall_score,
        passed,
        explicit_failure_violations,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Per-scenario accumulator used while grouping trajectory results by
/// `scenario_id`.
#[derive(Debug, Default)]
struct ScenarioGroup {
    /// Count of trajectories in the group with `passed == true`.
    satisfied: u32,
    /// Total count of trajectories in the group.
    total: u32,
    /// `true` when at least one trajectory in the group has
    /// `expected_failure == true` (presence-based, independent of whether the
    /// failure was observed).
    has_explicit_failure: bool,
    /// `true` when at least one `expected_failure == true` trajectory in the
    /// group also has `passed == true` (the expected failure was observed).
    explicit_failure_observed: bool,
}

/// Groups trajectory results by `scenario_id`, accumulating satisfied/total
/// counts and explicit-failure presence/observation flags for each group.
fn build_scenario_groups(results: &[TrajectoryResult]) -> HashMap<String, ScenarioGroup> {
    let mut groups: HashMap<String, ScenarioGroup> = HashMap::new();

    for result in results {
        let group = groups.entry(result.scenario_id.clone()).or_default();
        group.total += 1;
        if result.passed {
            group.satisfied += 1;
        }
        if result.expected_failure {
            group.has_explicit_failure = true;
            if result.passed {
                group.explicit_failure_observed = true;
            }
        }
    }

    groups
}

/// Constructs a [`SatisfactionScore`] from an already-validated `[0.0, 1.0]`
/// value, panicking with a contextual message if the invariant was violated.
///
/// Shared by [`score_from_fraction`] and [`mean_score`], both of which compute
/// a value that is mathematically guaranteed to fall within `[0.0, 1.0]` given
/// their own invariants, but need a descriptive panic message (rather than a
/// bare `unwrap`) if that guarantee is ever broken by a future change.
///
/// # Panics
///
/// Never panics in practice: callers only pass values already proven to lie
/// within `[0.0, 1.0]`. The `unreachable!` below only guards against that
/// invariant being violated in the future; `context` is evaluated lazily so
/// the (rarely needed) message formatting has no cost on the success path.
fn score_or_panic(value: f64, context: impl FnOnce() -> String) -> SatisfactionScore {
    SatisfactionScore::new(value)
        .unwrap_or_else(|| unreachable!("{value} out of [0.0, 1.0]: {}", context()))
}

/// Computes `numerator / denominator` as a [`SatisfactionScore`], treating a
/// zero denominator as vacuously satisfied (`1.0`).
///
/// # Panics
///
/// Never panics in practice: `numerator` is always `<= denominator` by
/// construction in this module, so the resulting ratio is always within
/// `[0.0, 1.0]`. See [`score_or_panic`] for the invariant guard.
fn score_from_fraction(numerator: u32, denominator: u32) -> SatisfactionScore {
    let ratio = if denominator == 0 {
        1.0
    } else {
        f64::from(numerator) / f64::from(denominator)
    };

    score_or_panic(ratio, || {
        format!("numerator={numerator}, denominator={denominator}")
    })
}

/// Computes the unweighted arithmetic mean of `scores`, treating an empty
/// slice as vacuously satisfied (`1.0`).
///
/// # Panics
///
/// Never panics in practice: every [`SatisfactionScore`] is already within
/// `[0.0, 1.0]`, so their arithmetic mean is too. See [`score_or_panic`] for
/// the invariant guard.
#[allow(clippy::cast_precision_loss)] // score count is one-per-scenario, far below f64's exact-integer limit
fn mean_score(scores: &[SatisfactionScore]) -> SatisfactionScore {
    if scores.is_empty() {
        return score_from_fraction(1, 1);
    }

    let sum: f64 = scores.iter().map(|entry| entry.as_f64()).sum();
    let mean = sum / scores.len() as f64;

    score_or_panic(mean, || format!("{} scores", scores.len()))
}

#[cfg(test)]
#[path = "scenarios_tests.rs"]
mod tests;
