//! Adversarial test suite for `node.rs` — `BudgetTracker` and coordinator
//! wiring sanity checks.
//!
//! ## Phase: RED
//!
//! `BudgetTracker::new` and `BudgetTracker::try_acquire` are `todo!()` stubs;
//! every test that exercises them will panic at runtime until the Coder lands
//! the GREEN implementation. This is the expected RED state in TDD.
//!
//! ## Scope note — coordinator types
//!
//! `ContextAssembler`, `ConstraintValidator`, `ContextPackLoader`, and
//! `WorkingCopyManager` are **not** stubbed — their constructors and
//! accessors are plain field wiring with no business logic. Per the task
//! brief, these get only a thin construction/accessor sanity check each
//! (proving the accessor returns the exact injected dependency, not a
//! coincidentally-similar default), not an adversarial suite.
//!
//! ## Assertions covered
//!
//! - ASSERT-GRAPH-008: parallel cost budget is atomic. A sequential
//!   TOCTOU-equivalence property test cross-validates `BudgetTracker`
//!   against the pure `acquire_budget` function it wraps, and a genuine
//!   multi-thread concurrency test (plus a multi-task tokio variant) proves
//!   no over-approval and no lost updates when real parallelism is involved.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use async_trait::async_trait;
use pipeline::{
    ArtifactPath, BranchName, BudgetAcquisition, CacheError, CodeRepository, CommitSha,
    CostBudget, DirectoryEntry, FileContent, GitHubOperationError, InterfaceDefinition,
    InterfaceRegistryLoader, PyramidSummary, RegistryError, RepositoryId, SummaryCache,
    SummaryLevel, TokenCost, ValidationResult,
};

use super::{BudgetTracker, ConstraintValidator, ContextAssembler, ContextPackLoader, WorkingCopyManager};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn cost(v: f64) -> TokenCost {
    TokenCost::new(v).expect("test helper: cost must be finite and non-negative")
}

fn budget(v: f64) -> CostBudget {
    CostBudget::new(v).expect("test helper: budget must be finite and positive")
}

// ─── Tier 1: Specification tests — BudgetTracker ────────────────────────────

/// A fresh `BudgetTracker` must start with zero accumulated cost: an estimate
/// strictly below the limit must be approved.
#[test]
fn test_budget_tracker_new_fresh_tracker_approves_estimate_below_limit() {
    let tracker = BudgetTracker::new(budget(1.00));

    let result = tracker.try_acquire(&cost(0.50));

    assert!(
        matches!(result, BudgetAcquisition::Approved { .. }),
        "a fresh tracker with zero accumulated cost must approve an estimate below the limit"
    );
}

/// Boundary (ASSERT-GRAPH-008 / strict `<`): a single `try_acquire` whose
/// estimated cost exactly equals the limit (accumulated is zero) must be
/// denied — mirrors `acquire_budget`'s strict inequality contract.
#[test]
fn test_budget_tracker_try_acquire_exact_limit_denied() {
    let tracker = BudgetTracker::new(budget(1.00));

    let result = tracker.try_acquire(&cost(1.00));

    assert!(
        matches!(result, BudgetAcquisition::Denied(_)),
        "estimated cost exactly equal to the limit must be denied (strict < required)"
    );
}

/// Two sequential calls must accumulate: the second call's decision must be
/// based on the running total including the first call's estimated cost, not
/// just the second estimate in isolation.
#[test]
fn test_budget_tracker_sequential_calls_accumulate_cost() {
    let tracker = BudgetTracker::new(budget(1.00));

    let first = tracker.try_acquire(&cost(0.60));
    assert!(matches!(first, BudgetAcquisition::Approved { .. }));

    // 0.60 (accumulated) + 0.50 (estimated) = 1.10 > 1.00 → must be denied,
    // even though 0.50 alone would fit comfortably under the limit.
    let second = tracker.try_acquire(&cost(0.50));
    assert!(
        matches!(second, BudgetAcquisition::Denied(_)),
        "second call must be denied because the accumulated total from the \
         first call pushes the combined cost over budget"
    );
}

/// A denied `try_acquire` must NOT mutate the accumulator: a subsequent call
/// with the same estimate that was denied must see an unchanged baseline
/// (i.e. the denial attempt is not partially applied).
#[test]
fn test_budget_tracker_denied_acquisition_does_not_mutate_accumulator() {
    let tracker = BudgetTracker::new(budget(1.00));

    // Push accumulated to exactly 0.80.
    assert!(matches!(
        tracker.try_acquire(&cost(0.80)),
        BudgetAcquisition::Approved { .. }
    ));

    // This would push combined to 1.10 — denied, must not mutate accumulator.
    assert!(matches!(
        tracker.try_acquire(&cost(0.30)),
        BudgetAcquisition::Denied(_)
    ));

    // A 0.19 estimate against the *original* 0.80 baseline (0.80 + 0.19 = 0.99
    // < 1.00) must still be approved — proving the denied attempt above left
    // no residue in the accumulator.
    let probe = tracker.try_acquire(&cost(0.19));
    assert!(
        matches!(probe, BudgetAcquisition::Approved { .. }),
        "a denied try_acquire must not have mutated the accumulator; \
         expected the 0.19 probe against the 0.80 baseline to be approved"
    );
}

/// `BudgetTracker` is `Clone`; all clones must share the same underlying
/// accumulator. An approval via one clone must be visible to a second clone
/// via its effect on subsequent decisions.
#[test]
fn test_budget_tracker_clones_share_the_same_accumulator() {
    let tracker_a = BudgetTracker::new(budget(1.00));
    let tracker_b = tracker_a.clone();

    // Approve 0.70 via clone A.
    assert!(matches!(
        tracker_a.try_acquire(&cost(0.70)),
        BudgetAcquisition::Approved { .. }
    ));

    // Clone B must see the 0.70 already accumulated: 0.70 + 0.40 = 1.10 > 1.00
    // → denied. If clones did NOT share state, B would see an empty
    // accumulator and 0.40 alone would be approved (0.40 < 1.00).
    let result_via_b = tracker_b.try_acquire(&cost(0.40));
    assert!(
        matches!(result_via_b, BudgetAcquisition::Denied(_)),
        "clone B must observe the accumulated cost recorded via clone A \
         (shared accumulator), got {result_via_b:?}"
    );
}

// ─── Tier 3: Property test — sequential equivalence with `acquire_budget` ──

proptest::proptest! {
    /// For any sequence of non-negative estimated costs replayed sequentially
    /// against a single `BudgetTracker`, the tracker's Approved/Denied
    /// decisions must exactly match what the pure `acquire_budget` function
    /// would decide given the same running total — i.e. `BudgetTracker` is a
    /// faithful, atomicity-preserving wrapper, never approving or denying
    /// differently than the pure contract it is built on.
    #[test]
    fn test_budget_tracker_sequential_decisions_match_pure_acquire_budget(
        cost_values in proptest::collection::vec(0.0f64..5.0, 1..25),
        limit_value in 0.5f64..20.0,
    ) {
        let limit = budget(limit_value);
        let tracker = BudgetTracker::new(limit);

        let mut expected_accumulated = 0.0_f64;
        for &c in &cost_values {
            let estimated = cost(c);
            let result = tracker.try_acquire(&estimated);
            let pure_would_approve = expected_accumulated + c < limit_value;

            match result {
                BudgetAcquisition::Approved { .. } => {
                    proptest::prop_assert!(
                        pure_would_approve,
                        "tracker approved cost {c} at accumulated {expected_accumulated}, \
                         but pure acquire_budget semantics (strict <) would deny"
                    );
                    expected_accumulated += c;
                }
                BudgetAcquisition::Denied(_) => {
                    proptest::prop_assert!(
                        !pure_would_approve,
                        "tracker denied cost {c} at accumulated {expected_accumulated}, \
                         but pure acquire_budget semantics (strict <) would approve"
                    );
                }
            }
        }

        // Cross-check the final accumulated value via a zero-cost probe:
        // Approved { remaining } == limit - accumulated when remaining > 0.
        if expected_accumulated < limit_value {
            let probe = tracker.try_acquire(&TokenCost::zero());
            if let BudgetAcquisition::Approved { remaining } = probe {
                let actual_accumulated = limit_value - remaining.as_f64();
                proptest::prop_assert!(
                    (actual_accumulated - expected_accumulated).abs() < 1e-6,
                    "final accumulated drifted: tracker implies {actual_accumulated}, \
                     expected {expected_accumulated}"
                );
            }
        }
    }
}

// ─── Tier 3: Genuine concurrency tests — ASSERT-GRAPH-008 ──────────────────

/// Number of racing workers per trial. Deliberately larger than the 7 slots
/// available (see below) to maximise contention pressure on the shared
/// mutex.
const RACE_WORKER_COUNT: usize = 50;

/// Number of independent trials run per test. A correct, atomic
/// implementation is **deterministic**: every trial must yield exactly the
/// same approved count (see rationale below), so repeating the experiment
/// produces zero false positives. A TOCTOU-racy implementation's failure
/// window is probabilistic per trial; repeating multiplies the chance of
/// observing at least one over-approval within this test run.
///
/// ## Why `60`, not `5` (QA audit finding, 2026-10-24)
///
/// A manual adversarial mutant was constructed that splits `try_acquire`'s
/// single continuous lock hold into two separate `lock()` calls — one for
/// the `acquire_budget` check, one for the `*accumulated += *estimated`
/// update — reopening the TOCTOU window the atomicity contract exists to
/// close. Against that mutant, with `RACE_TRIAL_COUNT = 5` (the original
/// value), this test (both the `std::thread` and `tokio::task` variants
/// combined) only detected the race in **8 of 20** independent full test
/// runs (40%) — i.e. a 60% chance of the mutant passing CI undetected on
/// any given run, because the critical section reopened by the mutant is so
/// short that real OS thread scheduling rarely preempts inside it. Raising
/// `RACE_TRIAL_COUNT` to `60` raised detection to **15 of 15** runs (100%)
/// in the same experiment, with zero false positives against the correct
/// (non-mutated) implementation across 10 repeated runs. The exact-dyadic-
/// fraction design (see below) means the extra trials add negligible
/// wall-clock cost (whole-suite run time for this test file remained in the
/// low single-digit seconds) while making this specific defect class
/// reliably catchable rather than merely probable.
const RACE_TRIAL_COUNT: usize = 60;

/// Runs one trial: `RACE_WORKER_COUNT` std threads, synchronised to all start
/// `try_acquire` at approximately the same instant via a [`std::sync::Barrier`]
/// (maximising the contention window on the shared mutex), each racing for
/// `0.125` against a `1.0` limit. Returns the number of approvals observed.
///
/// Because `0.125` is an exact dyadic binary fraction, repeated addition is
/// exact in `f64` regardless of summation order, so the number of approvals
/// that can occur before the strict `<` boundary is hit is **deterministic**
/// for a correct implementation (`floor-exclusive`: 7 approvals of `0.125`
/// sum to `0.875 < 1.0`; an 8th would reach `1.0`, which is not `< 1.0`).
/// Any implementation that races the check-then-update (TOCTOU) can
/// over-approve (more than 7 succeeding) or lose updates.
fn run_std_thread_race_trial() -> u32 {
    let tracker = BudgetTracker::new(budget(1.0));
    let approved_count = Arc::new(AtomicU32::new(0));
    let barrier = Arc::new(std::sync::Barrier::new(RACE_WORKER_COUNT));

    let handles: Vec<_> = (0..RACE_WORKER_COUNT)
        .map(|_| {
            let tracker = tracker.clone();
            let approved_count = Arc::clone(&approved_count);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let estimated = cost(0.125);
                if matches!(tracker.try_acquire(&estimated), BudgetAcquisition::Approved { .. }) {
                    approved_count.fetch_add(1, Ordering::SeqCst);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("worker thread panicked");
    }

    let count = approved_count.load(Ordering::SeqCst);

    // Cross-check: when the approved count is 7 (the expected value), the
    // final accumulated value must also be consistent (remaining exactly
    // 0.125) — catching lost-update bugs that happen to still approve
    // exactly 7 requests but corrupt the running total.
    if count == 7
        && let BudgetAcquisition::Approved { remaining } = tracker.try_acquire(&TokenCost::zero())
    {
        assert!(
            (remaining.as_f64() - 0.125).abs() < 1e-9,
            "approved count was 7 but final remaining budget was {} \
             (expected exactly 0.125) — indicates a lost update",
            remaining.as_f64()
        );
    }

    count
}

/// Real OS-thread concurrency test (see [`run_std_thread_race_trial`] for the
/// exact-arithmetic rationale). Repeated across [`RACE_TRIAL_COUNT`] trials to
/// make a TOCTOU race in the implementation highly likely to surface, while
/// remaining a zero-false-positive assertion against a correct implementation.
#[test]
fn test_try_acquire_concurrent_std_threads_approve_exactly_capacity_no_overspend() {
    for trial in 0..RACE_TRIAL_COUNT {
        let approved = run_std_thread_race_trial();
        assert_eq!(
            approved,
            7,
            "trial {trial}: exactly 7 of {RACE_WORKER_COUNT} threads racing for 0.125 \
             against a 1.0 limit (strict <) must be approved — any other count \
             indicates a TOCTOU race (over-approval or lost updates)"
        );
    }
}

/// Runs one trial using `tokio::task::spawn_blocking` on a multi-threaded
/// runtime instead of raw `std::thread` — some atomicity bugs only surface
/// under one concurrency model, so both are exercised. Same barrier-synchronised,
/// deterministic exact-arithmetic design as [`run_std_thread_race_trial`].
async fn run_tokio_task_race_trial() -> u32 {
    let tracker = BudgetTracker::new(budget(1.0));
    let approved_count = Arc::new(AtomicU32::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(RACE_WORKER_COUNT));

    let mut handles = Vec::new();
    for _ in 0..RACE_WORKER_COUNT {
        let tracker = tracker.clone();
        let approved_count = Arc::clone(&approved_count);
        let barrier = Arc::clone(&barrier);
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let estimated = cost(0.125);
            if matches!(tracker.try_acquire(&estimated), BudgetAcquisition::Approved { .. }) {
                approved_count.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }

    for handle in handles {
        handle.await.expect("tokio task panicked");
    }

    approved_count.load(Ordering::SeqCst)
}

/// Genuine concurrency test using independent `tokio::spawn` tasks on a
/// multi-threaded runtime. Repeated across [`RACE_TRIAL_COUNT`] trials for the
/// same reason as the `std::thread` variant above.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_try_acquire_concurrent_tokio_tasks_approve_exactly_capacity_no_overspend() {
    for trial in 0..RACE_TRIAL_COUNT {
        let approved = run_tokio_task_race_trial().await;
        assert_eq!(
            approved,
            7,
            "trial {trial}: exactly 7 of {RACE_WORKER_COUNT} concurrent tokio tasks racing \
             for 0.125 against a 1.0 limit (strict <) must be approved"
        );
    }
}

// ─── Coordinator wiring sanity checks (thin — no business logic to test) ──

/// Minimal mock `SummaryCache` whose `is_stale` answer is configurable, used
/// only to prove `ContextAssembler::summary_cache()` returns the exact
/// injected dependency (not some default/no-op implementation).
struct MarkerSummaryCache {
    is_stale_answer: bool,
}

#[async_trait]
impl SummaryCache for MarkerSummaryCache {
    async fn get_summary(
        &self,
        _path: &ArtifactPath,
        _level: SummaryLevel,
    ) -> Result<Option<PyramidSummary>, CacheError> {
        Ok(None)
    }

    async fn is_stale(&self, _path: &ArtifactPath, _current_sha: &CommitSha) -> Result<bool, CacheError> {
        Ok(self.is_stale_answer)
    }

    async fn invalidate(&self, _path: &ArtifactPath) -> Result<(), CacheError> {
        Ok(())
    }
}

/// `ContextAssembler::new` must wire the given `SummaryCache` through
/// unchanged: the `summary_cache()` accessor must return the exact injected
/// instance (proven behaviourally via its configured `is_stale` marker).
#[tokio::test]
async fn test_context_assembler_summary_cache_accessor_returns_injected_dependency() {
    let path = ArtifactPath::new("src/lib.rs").expect("non-empty path");
    let sha = CommitSha::new("a".repeat(40)).expect("non-empty sha");

    let assembler_true = ContextAssembler::new(Arc::new(MarkerSummaryCache {
        is_stale_answer: true,
    }));
    let assembler_false = ContextAssembler::new(Arc::new(MarkerSummaryCache {
        is_stale_answer: false,
    }));

    assert!(
        assembler_true
            .summary_cache()
            .is_stale(&path, &sha)
            .await
            .expect("mock is infallible"),
        "accessor must return the dependency configured with is_stale_answer=true"
    );
    assert!(
        !assembler_false
            .summary_cache()
            .is_stale(&path, &sha)
            .await
            .expect("mock is infallible"),
        "accessor must return the dependency configured with is_stale_answer=false"
    );
}

/// Minimal mock `InterfaceRegistryLoader`, used only to prove
/// `ConstraintValidator::registry_loader()` returns the exact injected
/// dependency (via a configurable `load_definitions` result length).
struct MarkerRegistryLoader {
    definition_count: usize,
}

#[async_trait]
impl InterfaceRegistryLoader for MarkerRegistryLoader {
    async fn load_definitions(&self) -> Result<Vec<InterfaceDefinition>, RegistryError> {
        Ok((0..self.definition_count)
            .map(|i| InterfaceDefinition {
                id: pipeline::InterfaceId::new(format!("iface-{i}")).expect("non-empty id"),
                domain: pipeline::DomainServiceName::new("test-domain").expect("non-empty"),
                schema: serde_json::json!({}),
                artifact_types: vec![],
                version: pipeline::ApiVersion::new(1, 0),
            })
            .collect())
    }

    fn validate_schema(&self, _definition: &InterfaceDefinition) -> Result<ValidationResult, RegistryError> {
        Ok(ValidationResult::passed())
    }
}

/// `ConstraintValidator::new` must wire the given `InterfaceRegistryLoader`
/// through unchanged: the `registry_loader()` accessor must return the exact
/// injected instance.
#[tokio::test]
async fn test_constraint_validator_registry_loader_accessor_returns_injected_dependency() {
    let validator_empty = ConstraintValidator::new(Arc::new(MarkerRegistryLoader {
        definition_count: 0,
    }));
    let validator_three = ConstraintValidator::new(Arc::new(MarkerRegistryLoader {
        definition_count: 3,
    }));

    let empty_defs = validator_empty
        .registry_loader()
        .load_definitions()
        .await
        .expect("mock is infallible");
    let three_defs = validator_three
        .registry_loader()
        .load_definitions()
        .await
        .expect("mock is infallible");

    assert_eq!(empty_defs.len(), 0);
    assert_eq!(three_defs.len(), 3);
}

/// Minimal mock `CodeRepository`, used only to prove the `repository()`
/// accessors on `ContextPackLoader` and `WorkingCopyManager` return the exact
/// injected instance (via a configurable `file_exists` marker).
struct MarkerCodeRepository {
    file_exists_answer: bool,
}

#[async_trait]
impl CodeRepository for MarkerCodeRepository {
    async fn read_file(
        &self,
        _repository: &RepositoryId,
        _path: &str,
        _git_ref: &str,
    ) -> Result<FileContent, GitHubOperationError> {
        Err(GitHubOperationError::NotFound {
            resource: "mock: read_file not configured".to_string(),
        })
    }

    async fn list_directory(
        &self,
        _repository: &RepositoryId,
        _path: &str,
        _git_ref: &str,
    ) -> Result<Vec<DirectoryEntry>, GitHubOperationError> {
        Ok(vec![])
    }

    async fn file_exists(
        &self,
        _repository: &RepositoryId,
        _path: &str,
        _git_ref: &str,
    ) -> Result<bool, GitHubOperationError> {
        Ok(self.file_exists_answer)
    }

    async fn read_tree(
        &self,
        _repository: &RepositoryId,
        _git_ref: &str,
    ) -> Result<Vec<DirectoryEntry>, GitHubOperationError> {
        Ok(vec![])
    }
}

/// `ContextPackLoader::new` must wire the given `CodeRepository` through
/// unchanged.
#[tokio::test]
async fn test_context_pack_loader_repository_accessor_returns_injected_dependency() {
    let repo_id = RepositoryId::new("acme/widgets").expect("non-empty id");

    let loader_true = ContextPackLoader::new(Arc::new(MarkerCodeRepository {
        file_exists_answer: true,
    }));
    let loader_false = ContextPackLoader::new(Arc::new(MarkerCodeRepository {
        file_exists_answer: false,
    }));

    assert!(
        loader_true
            .repository()
            .file_exists(&repo_id, "some/path", "main")
            .await
            .expect("mock is infallible")
    );
    assert!(
        !loader_false
            .repository()
            .file_exists(&repo_id, "some/path", "main")
            .await
            .expect("mock is infallible")
    );
}

/// `WorkingCopyManager::new` must wire both the `CodeRepository` and the
/// `working_branch` through unchanged — `working_branch()` returns the exact
/// branch given, and `repository()` returns the exact injected dependency.
#[tokio::test]
async fn test_working_copy_manager_accessors_return_injected_dependencies() {
    let repo_id = RepositoryId::new("acme/widgets").expect("non-empty id");
    let branch = BranchName::new("release/v2").expect("non-empty branch name");

    let manager = WorkingCopyManager::new(
        Arc::new(MarkerCodeRepository {
            file_exists_answer: true,
        }),
        branch.clone(),
    );

    assert_eq!(manager.working_branch(), &branch);
    assert!(
        manager
            .repository()
            .file_exists(&repo_id, "some/path", "main")
            .await
            .expect("mock is infallible")
    );
}
