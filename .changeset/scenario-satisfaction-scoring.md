---
"pipeline": minor
---

Implement `compute_satisfaction` in `crates/pipeline/src/scenarios.rs`, completing the scenario satisfaction scoring subsystem.

`compute_satisfaction` groups a flat slice of `TrajectoryResult` values by `scenario_id`, computes a `PerScenarioScore` for each scenario (satisfied/total trajectory counts, a `[0.0, 1.0]` score, and a `passed` flag driven by the caller-supplied `threshold`), and aggregates these into a `ScenarioSatisfactionResult` with an unweighted mean `overall_score` and a top-level `passed` flag.

Explicit-failure scenarios (`TrajectoryResult::expected_failure == true`) are handled with two distinct signals:

- `PerScenarioScore::explicit_failure` is presence-based: `true` whenever the scenario's trajectory group contains at least one `expected_failure == true` trajectory, regardless of whether that failure was actually observed.
- `ScenarioSatisfactionResult::explicit_failure_violations` is observation-based: it lists the IDs of expected-failure scenarios where the expected failure did *not* occur in any trajectory — a potential safety concern, since these scenarios are designed to verify graceful failure behaviour. Any such violation forces the overall `passed` flag to `false`, even if every per-scenario `passed` flag is otherwise `true`.

Empty input is treated as vacuously satisfied (`overall_score: 1.0`, `passed: true`).

No public API shapes changed — `compute_satisfaction`, `PerScenarioScore`, and `ScenarioSatisfactionResult` were already fully specified; this change fills in the previously-`todo!()` implementation body.
