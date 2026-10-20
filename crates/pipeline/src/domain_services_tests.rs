//! Adversarial test suite for `domain_services.rs` — Domain Service Routing
//! (`select_service_for_artifacts`, `resolve_primary_and_secondary`).
//!
//! ## Phase: RED
//!
//! Both functions under test are `todo!()` stubs. Every test below compiles
//! (GREEN at compile time) but **panics at runtime** until the real
//! implementation lands. This is the expected RED state in TDD.
//!
//! ## Tier map
//!
//! | Tier | Count | Focus |
//! |---|---|---|
//! | 1 — Specification | 20 | Direct behaviour-table mapping (steps 1-6 / 1-3) |
//! | 2 — Adversarial / boundary | 16 | N-1/N/N+1 scoring boundaries, error-payload content, side effects, stub-killers |
//! | 3 — Property-based | 7 | Scoring invariants, order-independence, no-panic, primary/secondary disjointness |
//!
//! ## Interpretation notes (spec gaps resolved per injected Interface Contract)
//!
//! 1. **Scoring semantics (type-centric, not path-centric).** The spec text
//!    ("Score each candidate by the number of `artifact_types` that match the
//!    file extensions of `artifact_paths`") is read literally as: for each
//!    candidate service, `score = |{ t in capabilities.artifact_types : the
//!    extension-part of t (after stripping the leading "domain/") matches,
//!    case-insensitively, the file extension of AT LEAST ONE artifact_path }|`.
//!    This is **type-centric** counting (count matching declared types), not
//!    **path-centric** counting (count matching paths). Under path-centric
//!    counting, passing the same `.rs` file five times would quintuple a
//!    service's score; under type-centric counting it would not, because the
//!    set of distinct extensions present is unchanged. This suite commits to
//!    type-centric counting and pins it down with
//!    `test_select_service_for_artifacts_duplicate_matching_extension_does_not_inflate_score_beyond_distinct_types`
//!    and `test_select_service_for_artifacts_multiple_artifact_paths_scoring_accumulates`
//!    (the latter shows the score increasing specifically because a second,
//!    *distinct* extension is introduced that a broader-coverage service also
//!    declares — not because of sheer path count).
//! 2. **`HandshakePending`-via-filter-first.** Per the injected task note: step 1
//!    (filter to `capabilities: Some`) runs before scoring (step 2), so a
//!    `None`-capabilities registration can never be "the best match" — it is
//!    simply invisible to scoring. The only registration that could ever have
//!    matched is filtered out pre-scoring, so the outcome is
//!    `RoutingError::NoServiceFound`, not `RoutingError::HandshakePending`. See
//!    `test_select_service_for_artifacts_none_capabilities_only_match_returns_no_service_found_not_handshake_pending`.
//!    A consequence: `select_service_for_artifacts` can never produce
//!    `RoutingError::HandshakePending` under this reading, so
//!    `resolve_primary_and_secondary`'s error-propagation tests below only
//!    cover `NoServiceFound` and `Ambiguous` (the two variants that are
//!    actually reachable). Flagging as FYI, not as an open gap (resolved by
//!    the Tech Lead already).
//! 3. **Secondaries matched via `interface_types`, not `artifact_types`.** Per
//!    the injected task note, `docs/spec/interfaces/advanced-features.md`
//!    step 2 for `resolve_primary_and_secondary` literally says
//!    "`artifact_types` intersect with `interface_type_identifiers`", but this
//!    is a documented typo — the in-code rustdoc on the function and on
//!    `ServiceCapabilities::interface_types` both specify `interface_types`.
//!    This suite tests against `interface_types` matching exclusively. See
//!    `test_resolve_primary_and_secondary_secondary_matches_interface_types_intersection`.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use std::collections::BTreeSet;

use proptest::prelude::*;

use super::{
    DomainServiceRegistration, RoutingError, ServiceCapabilities, ServiceRegistry,
    resolve_primary_and_secondary, select_service_for_artifacts,
};
use crate::{ArtifactPath, DomainServiceName};

// ─── Test helpers ────────────────────────────────────────────────────────────

fn ap(s: &str) -> ArtifactPath {
    ArtifactPath::new(s).expect("test artifact path must be valid")
}

fn aps(paths: &[&str]) -> Vec<ArtifactPath> {
    paths.iter().copied().map(ap).collect()
}

fn svc(s: &str) -> DomainServiceName {
    DomainServiceName::new(s).expect("test service name must be non-empty")
}

/// Builds a fully-handshaken registration with the given declared type lists.
fn registration(
    name: &str,
    artifact_types: &[&str],
    interface_types: &[&str],
    supported_methods: &[&str],
) -> DomainServiceRegistration {
    DomainServiceRegistration {
        service_name: svc(name),
        transport_config: serde_json::Value::Null,
        capabilities: Some(ServiceCapabilities {
            domain: svc(name),
            artifact_types: artifact_types.iter().map(ToString::to_string).collect(),
            interface_types: interface_types.iter().map(ToString::to_string).collect(),
            supported_methods: supported_methods.iter().map(ToString::to_string).collect(),
        }),
    }
}

/// Builds a registration whose `service_name` (registry key) and
/// `capabilities.domain` (self-reported identity) are deliberately
/// different — exercising the identity inconsistency described in PR #165
/// review feedback (Bug 1: these two fields are conventionally expected to
/// be equal but are not enforced to be equal by the type system).
fn registration_with_mismatched_identity(
    service_name: &str,
    domain: &str,
    artifact_types: &[&str],
    interface_types: &[&str],
) -> DomainServiceRegistration {
    DomainServiceRegistration {
        service_name: svc(service_name),
        transport_config: serde_json::Value::Null,
        capabilities: Some(ServiceCapabilities {
            domain: svc(domain),
            artifact_types: artifact_types.iter().map(ToString::to_string).collect(),
            interface_types: interface_types.iter().map(ToString::to_string).collect(),
            supported_methods: vec![],
        }),
    }
}

/// Builds a registration whose handshake has not yet completed
/// (`capabilities: None`).
fn pending_registration(name: &str) -> DomainServiceRegistration {
    DomainServiceRegistration {
        service_name: svc(name),
        transport_config: serde_json::Value::Null,
        capabilities: None,
    }
}

fn registry_of(regs: Vec<DomainServiceRegistration>) -> ServiceRegistry {
    ServiceRegistry {
        registrations: regs,
    }
}

/// Asserts that `result` is `Ok` with the given expected service name.
///
/// `RoutingError` intentionally does not derive `PartialEq` (it is
/// production code, not under test-suite control), so `Result<DomainServiceName,
/// RoutingError>` cannot be compared with `assert_eq!` directly against an
/// `Ok(..)` literal. This helper unwraps explicitly instead.
fn assert_selected(result: Result<DomainServiceName, RoutingError>, expected: &str) {
    match result {
        Ok(name) => assert_eq!(name, svc(expected)),
        Err(e) => panic!("expected Ok({expected}), got Err({e:?})"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 1: Specification Tests — select_service_for_artifacts
// ═══════════════════════════════════════════════════════════════════════════

/// Step 2+3: a single service with one declared type matching the single
/// artifact path's extension is the unique highest scorer and is returned.
#[test]
fn test_select_service_for_artifacts_exact_match_returns_service() {
    let registry = registry_of(vec![registration("rust-svc", &["rust/rs"], &[], &[])]);
    let paths = aps(&["src/lib.rs"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "rust-svc");
}

/// Case-insensitive matching: declared type extension is uppercase, actual
/// file extension is lowercase — must still match.
#[test]
fn test_select_service_for_artifacts_case_insensitive_type_uppercase_matches_lowercase_extension() {
    let registry = registry_of(vec![registration("rust-svc", &["rust/RS"], &[], &[])]);
    let paths = aps(&["src/lib.rs"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "rust-svc");
}

/// Case-insensitive matching, the other direction: declared type extension is
/// lowercase, actual file extension is uppercase — must still match.
#[test]
fn test_select_service_for_artifacts_case_insensitive_extension_uppercase_matches_lowercase_type() {
    let registry = registry_of(vec![registration("rust-svc", &["rust/rs"], &[], &[])]);
    let paths = aps(&["src/LIB.RS"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "rust-svc");
}

/// Step 4: two services tie at the (non-zero) highest score → Ambiguous.
#[test]
fn test_select_service_for_artifacts_tied_scores_returns_ambiguous() {
    let registry = registry_of(vec![
        registration("svc-a", &["domain-a/rs"], &[], &[]),
        registration("svc-b", &["domain-b/rs"], &[], &[]),
    ]);
    let paths = aps(&["src/lib.rs"]);

    let result = select_service_for_artifacts(&registry, &paths);

    match result {
        Err(RoutingError::Ambiguous { candidates, .. }) => {
            let set: BTreeSet<_> = candidates.into_iter().collect();
            assert_eq!(
                set,
                BTreeSet::from([svc("svc-a"), svc("svc-b")]),
                "both tied services must be listed as candidates"
            );
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

/// Step 5: no registered service declares a matching type → NoServiceFound.
#[test]
fn test_select_service_for_artifacts_no_match_returns_no_service_found() {
    let registry = registry_of(vec![registration("rust-svc", &["rust/rs"], &[], &[])]);
    let paths = aps(&["src/main.py"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert!(matches!(result, Err(RoutingError::NoServiceFound { .. })));
}

/// Step 2+3: one service declares types matching both extensions present
/// across two artifact paths (score 2); the other declares a type matching
/// only one of them (score 1). The higher, non-tied scorer wins outright.
#[test]
fn test_select_service_for_artifacts_higher_score_wins_over_partial_match() {
    let registry = registry_of(vec![
        registration("partial-svc", &["a/rs"], &[], &[]),
        registration("full-svc", &["b/rs", "b/toml"], &[], &[]),
    ]);
    let paths = aps(&["src/lib.rs", "Cargo.toml"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "full-svc");
}

/// Empty registry: no candidates exist at all → NoServiceFound.
#[test]
fn test_select_service_for_artifacts_empty_registry_returns_no_service_found() {
    let registry = ServiceRegistry::empty();
    let paths = aps(&["src/lib.rs"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert!(matches!(result, Err(RoutingError::NoServiceFound { .. })));
}

/// Empty `artifact_paths`: there is no extension to match against, so every
/// candidate scores zero → NoServiceFound (step 5: "highest score is 0").
#[test]
fn test_select_service_for_artifacts_empty_artifact_paths_returns_no_service_found() {
    let registry = registry_of(vec![registration("rust-svc", &["rust/rs"], &[], &[])]);
    let paths: Vec<ArtifactPath> = vec![];

    let result = select_service_for_artifacts(&registry, &paths);

    assert!(matches!(result, Err(RoutingError::NoServiceFound { .. })));
}

/// Scoring accumulates across multiple distinct extensions introduced by
/// multiple artifact paths: a service covering both extensions outscores one
/// covering only one, and is selected unambiguously.
#[test]
fn test_select_service_for_artifacts_multiple_artifact_paths_scoring_accumulates() {
    let registry = registry_of(vec![
        registration("rs-only", &["x/rs"], &[], &[]),
        registration("rs-and-toml", &["x/rs", "x/toml"], &[], &[]),
    ]);
    let paths = aps(&["src/lib.rs", "Cargo.toml"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "rs-and-toml");
}

/// A `capabilities: None` registration must be excluded from scoring entirely
/// — even when another, fully-handshaken candidate exists and wins, the
/// pending registration's absence from the Ambiguous/score computation is
/// implicit confirmation it was filtered.
#[test]
fn test_select_service_for_artifacts_none_capabilities_registration_excluded_from_scoring() {
    let registry = registry_of(vec![
        pending_registration("pending-svc"),
        registration("rust-svc", &["rust/rs"], &[], &[]),
    ]);
    let paths = aps(&["src/lib.rs"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "rust-svc");
}

/// Pinning interpretation note 2: the ONLY registration in the registry is
/// `capabilities: None`. Even though (hypothetically) it might have matched
/// once handshaken, it is filtered out before scoring and therefore the
/// outcome must be `NoServiceFound`, not `HandshakePending`.
#[test]
fn test_select_service_for_artifacts_none_capabilities_only_match_returns_no_service_found_not_handshake_pending()
 {
    let registry = registry_of(vec![pending_registration("pending-svc")]);
    let paths = aps(&["src/lib.rs"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert!(
        matches!(result, Err(RoutingError::NoServiceFound { .. })),
        "expected NoServiceFound (filter-first), got {result:?}"
    );
}

/// Multi-dot filenames: the matched extension is the final dot-delimited
/// component (`"gz"` for `archive.tar.gz`), consistent with both a manual
/// last-dot split and `std::path::Path::extension()`.
#[test]
fn test_select_service_for_artifacts_multi_dot_filename_uses_final_extension() {
    let registry = registry_of(vec![registration("gzip-svc", &["archive/gz"], &[], &[])]);
    let paths = aps(&["dist/archive.tar.gz"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "gzip-svc");
}

/// A path with no extension at all cannot match any declared artifact type.
#[test]
fn test_select_service_for_artifacts_path_without_extension_does_not_match() {
    let registry = registry_of(vec![registration(
        "makefile-svc",
        &["build/makefile"],
        &[],
        &[],
    )]);
    let paths = aps(&["Makefile"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert!(matches!(result, Err(RoutingError::NoServiceFound { .. })));
}

/// `resolve_primary_and_secondary` step 1: the primary equals whatever
/// `select_service_for_artifacts` would independently compute for the same
/// `artifact_paths`.
#[test]
fn test_resolve_primary_and_secondary_primary_matches_select_service_for_artifacts() {
    let registry = registry_of(vec![registration("rust-svc", &["rust/rs"], &[], &[])]);
    let paths = aps(&["src/lib.rs"]);

    let direct = select_service_for_artifacts(&registry, &paths);
    let combined = resolve_primary_and_secondary(&registry, &paths, &[]);

    assert_selected(direct, "rust-svc");
    match combined {
        Ok((primary, _)) => assert_eq!(primary, svc("rust-svc")),
        Err(e) => panic!("expected Ok, got Err({e:?})"),
    }
}

/// Step 2+3: a non-primary service whose `interface_types` intersects
/// `interface_type_identifiers` is returned as a secondary.
#[test]
fn test_resolve_primary_and_secondary_secondary_matches_interface_types_intersection() {
    let registry = registry_of(vec![
        registration("rust-svc", &["rust/rs"], &[], &[]),
        registration("openapi-svc", &[], &["openapi/path"], &[]),
    ]);
    let paths = aps(&["src/lib.rs"]);
    let interface_ids = vec!["openapi/path".to_string()];

    let result = resolve_primary_and_secondary(&registry, &paths, &interface_ids);

    let (primary, secondaries) = result.expect("expected Ok result");
    assert_eq!(primary, svc("rust-svc"));
    assert_eq!(secondaries, vec![svc("openapi-svc")]);
}

/// Step 3: secondaries exclude the primary even when the primary's own
/// `interface_types` also intersects `interface_type_identifiers`.
#[test]
fn test_resolve_primary_and_secondary_secondary_excludes_primary_even_if_primary_interface_types_intersect()
 {
    let registry = registry_of(vec![registration(
        "rust-svc",
        &["rust/rs"],
        &["rust/trait"],
        &[],
    )]);
    let paths = aps(&["src/lib.rs"]);
    let interface_ids = vec!["rust/trait".to_string()];

    let result = resolve_primary_and_secondary(&registry, &paths, &interface_ids);

    let (primary, secondaries) = result.expect("expected Ok result");
    assert_eq!(primary, svc("rust-svc"));
    assert!(
        secondaries.is_empty(),
        "primary must never appear in its own secondaries list, even on self-intersection"
    );
}

/// Step 3: no other service's `interface_types` intersects
/// `interface_type_identifiers` → secondaries is empty.
#[test]
fn test_resolve_primary_and_secondary_no_intersection_returns_empty_secondaries() {
    let registry = registry_of(vec![
        registration("rust-svc", &["rust/rs"], &[], &[]),
        registration("kicad-svc", &[], &["kicad/schematic"], &[]),
    ]);
    let paths = aps(&["src/lib.rs"]);
    let interface_ids = vec!["openapi/path".to_string()];

    let result = resolve_primary_and_secondary(&registry, &paths, &interface_ids);

    let (_, secondaries) = result.expect("expected Ok result");
    assert!(secondaries.is_empty());
}

/// Multiple distinct secondaries are all returned when more than one other
/// service's `interface_types` intersects.
#[test]
fn test_resolve_primary_and_secondary_multiple_secondaries_all_included() {
    let registry = registry_of(vec![
        registration("rust-svc", &["rust/rs"], &[], &[]),
        registration("openapi-svc", &[], &["openapi/path"], &[]),
        registration("grpc-svc", &[], &["grpc/service"], &[]),
    ]);
    let paths = aps(&["src/lib.rs"]);
    let interface_ids = vec!["openapi/path".to_string(), "grpc/service".to_string()];

    let result = resolve_primary_and_secondary(&registry, &paths, &interface_ids);

    let (_, secondaries) = result.expect("expected Ok result");
    let set: BTreeSet<_> = secondaries.into_iter().collect();
    assert_eq!(set, BTreeSet::from([svc("openapi-svc"), svc("grpc-svc")]));
}

/// `resolve_primary_and_secondary` propagates `NoServiceFound` unchanged when
/// no primary match exists.
#[test]
fn test_resolve_primary_and_secondary_propagates_no_service_found_error() {
    let registry = registry_of(vec![registration("rust-svc", &["rust/rs"], &[], &[])]);
    let paths = aps(&["src/main.py"]);

    let result = resolve_primary_and_secondary(&registry, &paths, &[]);

    assert!(matches!(result, Err(RoutingError::NoServiceFound { .. })));
}

/// `resolve_primary_and_secondary` propagates `Ambiguous` unchanged when the
/// primary selection is itself ambiguous.
#[test]
fn test_resolve_primary_and_secondary_propagates_ambiguous_error() {
    let registry = registry_of(vec![
        registration("svc-a", &["domain-a/rs"], &[], &[]),
        registration("svc-b", &["domain-b/rs"], &[], &[]),
    ]);
    let paths = aps(&["src/lib.rs"]);

    let result = resolve_primary_and_secondary(&registry, &paths, &[]);

    assert!(matches!(result, Err(RoutingError::Ambiguous { .. })));
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 2: Adversarial / Boundary Tests
// ═══════════════════════════════════════════════════════════════════════════

/// `NoServiceFound`'s payload must carry the exact input `artifact_paths`
/// verbatim (not an empty vec, not a subset).
#[test]
fn test_select_service_for_artifacts_no_service_found_error_includes_input_artifact_paths() {
    let registry = ServiceRegistry::empty();
    let paths = aps(&["src/main.py", "src/other.py"]);

    let result = select_service_for_artifacts(&registry, &paths);

    match result {
        Err(RoutingError::NoServiceFound { artifact_paths }) => {
            assert_eq!(artifact_paths, paths);
        }
        other => panic!("expected NoServiceFound, got {other:?}"),
    }
}

/// `Ambiguous`'s payload must carry the exact input `artifact_paths` verbatim.
#[test]
fn test_select_service_for_artifacts_ambiguous_error_includes_input_artifact_paths() {
    let registry = registry_of(vec![
        registration("svc-a", &["domain-a/rs"], &[], &[]),
        registration("svc-b", &["domain-b/rs"], &[], &[]),
    ]);
    let paths = aps(&["src/lib.rs"]);

    let result = select_service_for_artifacts(&registry, &paths);

    match result {
        Err(RoutingError::Ambiguous { artifact_paths, .. }) => {
            assert_eq!(artifact_paths, paths);
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

/// Three services with strictly increasing scores (1, 2, 3): only the unique
/// top scorer (3) is returned — the middle and bottom scorers must not leak
/// into an Ambiguous result or be selected instead.
#[test]
fn test_select_service_for_artifacts_three_way_strict_ranking_only_top_scorer_wins() {
    let registry = registry_of(vec![
        registration("score-1", &["a/rs"], &[], &[]),
        registration("score-2", &["b/rs", "b/toml"], &[], &[]),
        registration("score-3", &["c/rs", "c/toml", "c/json"], &[], &[]),
    ]);
    let paths = aps(&["lib.rs", "Cargo.toml", "data.json"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "score-3");
}

/// Boundary: a one-point score difference (2 vs 1) must NOT be treated as a
/// tie — only an EXACT score equality triggers Ambiguous.
#[test]
fn test_select_service_for_artifacts_near_tie_one_point_difference_is_not_ambiguous() {
    let registry = registry_of(vec![
        registration("score-1", &["a/rs"], &[], &[]),
        registration("score-2", &["b/rs", "b/toml"], &[], &[]),
    ]);
    let paths = aps(&["lib.rs", "Cargo.toml"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "score-2");
}

/// A declared artifact type that matches none of the given extensions must
/// not contribute to the score at all — a service with one matching and one
/// non-matching declared type scores the same as a service with only the
/// matching type declared (stub-killer: rules out "score = len(artifact_types)").
#[test]
fn test_select_service_for_artifacts_unrelated_artifact_type_does_not_inflate_score() {
    let registry = registry_of(vec![
        registration("lean-svc", &["a/rs"], &[], &[]),
        registration(
            "padded-svc",
            &["b/rs", "b/completely-unrelated-ext"],
            &[],
            &[],
        ),
    ]);
    let paths = aps(&["lib.rs"]);

    let result = select_service_for_artifacts(&registry, &paths);

    // Both score 1 (only the "rs"-matching entry counts in each) → tie.
    assert!(
        matches!(result, Err(RoutingError::Ambiguous { .. })),
        "an unrelated declared type must not inflate padded-svc's score above lean-svc's; \
         expected a tie (Ambiguous), got {result:?}"
    );
}

/// Duplicate extensions across many artifact paths must not inflate the
/// score beyond the number of *distinct* matching declared types (type-centric
/// scoring, see module doc interpretation note 1).
#[test]
fn test_select_service_for_artifacts_duplicate_matching_extension_does_not_inflate_score_beyond_distinct_types()
 {
    let registry = registry_of(vec![
        registration("rs-only", &["a/rs"], &[], &[]),
        registration("rs-and-toml", &["b/rs", "b/toml"], &[], &[]),
    ]);
    // Five distinct .rs files, but only ONE distinct extension ("rs") is
    // present overall — "rs-and-toml" must still win via its second declared
    // type ("toml") only once a .toml path is introduced, which it is not
    // here. If scoring were path-centric, "rs-only" could rack up a score of
    // 5 from five .rs files and incorrectly tie or beat "rs-and-toml"'s
    // type-centric score of 1.
    let paths = aps(&["a.rs", "b.rs", "c.rs", "d.rs", "e.rs"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert!(
        matches!(result, Err(RoutingError::Ambiguous { .. })),
        "both services declare exactly one type matching the sole distinct \
         extension 'rs' present; expected a tie regardless of how many .rs \
         paths were passed, got {result:?}"
    );
}

/// Stub-killer: the matching service is NOT first in `registrations` — rules
/// out an implementation that always returns `registrations[0]`.
#[test]
fn test_select_service_for_artifacts_stub_killer_matching_service_not_first_in_registry() {
    let registry = registry_of(vec![
        registration("decoy-svc", &["decoy/toml"], &[], &[]),
        registration("rust-svc", &["rust/rs"], &[], &[]),
    ]);
    let paths = aps(&["src/lib.rs"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "rust-svc");
}

/// Stub-killer: the matching service is NOT last in `registrations` — rules
/// out an implementation that always returns `registrations.last()`.
#[test]
fn test_select_service_for_artifacts_stub_killer_matching_service_not_last_in_registry() {
    let registry = registry_of(vec![
        registration("rust-svc", &["rust/rs"], &[], &[]),
        registration("decoy-svc", &["decoy/toml"], &[], &[]),
    ]);
    let paths = aps(&["src/lib.rs"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "rust-svc");
}

/// Stub-killer: two independent calls with two different, non-overlapping
/// registries/paths must return two different service names — rules out a
/// hardcoded constant `DomainServiceName` return value.
#[test]
fn test_select_service_for_artifacts_stub_killer_distinct_inputs_produce_distinct_outputs() {
    let registry_a = registry_of(vec![registration("alpha-svc", &["a/rs"], &[], &[])]);
    let registry_b = registry_of(vec![registration("beta-svc", &["b/py"], &[], &[])]);

    let result_a = select_service_for_artifacts(&registry_a, &aps(&["x.rs"]));
    let result_b = select_service_for_artifacts(&registry_b, &aps(&["x.py"]));

    assert_selected(result_a, "alpha-svc");
    assert_selected(result_b, "beta-svc");
}

/// Side-effect verification: `select_service_for_artifacts` takes the
/// registry by immutable reference and must not alter it (e.g. no sorting,
/// draining, or mutation of `registrations`).
#[test]
fn test_select_service_for_artifacts_does_not_mutate_input_registry() {
    let registry = registry_of(vec![
        registration("svc-b", &["b/toml"], &[], &[]),
        registration("svc-a", &["a/rs"], &[], &[]),
    ]);
    let before = registry.clone();
    let paths = aps(&["lib.rs"]);

    let _ = select_service_for_artifacts(&registry, &paths);

    assert_eq!(
        registry.registrations.len(),
        before.registrations.len(),
        "registration count must be unchanged"
    );
    for (actual, expected) in registry
        .registrations
        .iter()
        .zip(before.registrations.iter())
    {
        assert_eq!(actual.service_name, expected.service_name);
    }
}

/// Side-effect verification: `resolve_primary_and_secondary` must not mutate
/// the registry it is given.
#[test]
fn test_resolve_primary_and_secondary_does_not_mutate_input_registry() {
    let registry = registry_of(vec![
        registration("rust-svc", &["rust/rs"], &[], &[]),
        registration("openapi-svc", &[], &["openapi/path"], &[]),
    ]);
    let before = registry.clone();
    let paths = aps(&["lib.rs"]);
    let interface_ids = vec!["openapi/path".to_string()];

    let _ = resolve_primary_and_secondary(&registry, &paths, &interface_ids);

    assert_eq!(registry.registrations.len(), before.registrations.len());
    for (actual, expected) in registry
        .registrations
        .iter()
        .zip(before.registrations.iter())
    {
        assert_eq!(actual.service_name, expected.service_name);
    }
}

/// Empty `interface_type_identifiers` slice: no secondary can possibly
/// intersect an empty set, so secondaries must be empty even when other
/// registered services declare non-empty `interface_types`.
#[test]
fn test_resolve_primary_and_secondary_empty_interface_type_identifiers_returns_empty_secondaries() {
    let registry = registry_of(vec![
        registration("rust-svc", &["rust/rs"], &[], &[]),
        registration("openapi-svc", &[], &["openapi/path"], &[]),
    ]);
    let paths = aps(&["lib.rs"]);

    let result = resolve_primary_and_secondary(&registry, &paths, &[]);

    let (_, secondaries) = result.expect("expected Ok result");
    assert!(secondaries.is_empty());
}

/// Stub-killer: secondaries must reflect a REAL intersection check, not a
/// hardcoded empty vec — this is the positive control for the previous test.
#[test]
fn test_resolve_primary_and_secondary_stub_killer_nonempty_intersection_produces_nonempty_secondaries()
 {
    let registry = registry_of(vec![
        registration("rust-svc", &["rust/rs"], &[], &[]),
        registration("openapi-svc", &[], &["openapi/path"], &[]),
    ]);
    let paths = aps(&["lib.rs"]);
    let interface_ids = vec!["openapi/path".to_string()];

    let result = resolve_primary_and_secondary(&registry, &paths, &interface_ids);

    let (_, secondaries) = result.expect("expected Ok result");
    assert!(
        !secondaries.is_empty(),
        "a real intersection exists and must be reported"
    );
}

/// A `capabilities: None` registration can never contribute a secondary
/// (it has no `interface_types` to check) — even if its `service_name`
/// coincides with an identifier string, that must not matter since matching
/// is on `interface_types`, which does not exist for a pending registration.
#[test]
fn test_resolve_primary_and_secondary_none_capabilities_registration_never_becomes_secondary() {
    let registry = registry_of(vec![
        registration("rust-svc", &["rust/rs"], &[], &[]),
        pending_registration("pending-svc"),
    ]);
    let paths = aps(&["lib.rs"]);
    let interface_ids = vec!["openapi/path".to_string(), "pending-svc".to_string()];

    let result = resolve_primary_and_secondary(&registry, &paths, &interface_ids);

    let (_, secondaries) = result.expect("expected Ok result");
    assert!(
        !secondaries.contains(&svc("pending-svc")),
        "a pending (None-capabilities) registration must never appear as a secondary"
    );
}

/// Partial string overlap must not be treated as intersection: `"rust/trait"`
/// and `"rust/traits"` are DISTINCT identifiers — exact string equality is
/// required, not substring/prefix matching.
#[test]
fn test_resolve_primary_and_secondary_partial_string_overlap_is_not_intersection() {
    let registry = registry_of(vec![
        registration("rust-svc", &["rust/rs"], &[], &[]),
        registration("other-svc", &[], &["rust/traits"], &[]),
    ]);
    let paths = aps(&["lib.rs"]);
    let interface_ids = vec!["rust/trait".to_string()];

    let result = resolve_primary_and_secondary(&registry, &paths, &interface_ids);

    let (_, secondaries) = result.expect("expected Ok result");
    assert!(
        secondaries.is_empty(),
        "'rust/trait' must not fuzzily match 'rust/traits'"
    );
}

/// The `domain/extension` stripping must only remove the FIRST path segment:
/// an artifact type like `"a/b/rs"` strips only the leading `"a/"`, leaving
/// `"b/rs"`, which must NOT match a plain `"rs"` extension (exact-remainder
/// comparison, not "ends with").
#[test]
fn test_select_service_for_artifacts_multi_segment_artifact_type_only_strips_first_segment() {
    let registry = registry_of(vec![registration("weird-svc", &["a/b/rs"], &[], &[])]);
    let paths = aps(&["lib.rs"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert!(
        matches!(result, Err(RoutingError::NoServiceFound { .. })),
        "'a/b/rs' strips to 'b/rs', which must not match extension 'rs' via suffix matching; \
         got {result:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Regression Tests — PR #165 code review findings
// ═══════════════════════════════════════════════════════════════════════════

/// Regression (PR #165 review, Bug 1): `service_name` (registry key) and
/// `capabilities.domain` are set to different `DomainServiceName` values on
/// the sole registration in the registry. Since there is only one service,
/// it MUST be its own (and only) primary, and therefore can never validly
/// appear in its own secondaries list — regardless of which field is used
/// as "the" identity. The current implementation selects `domain` as the
/// primary but excludes secondaries by comparing against `service_name`, so
/// the single registration fails to be excluded from its own secondaries
/// list.
#[test]
fn test_resolve_primary_and_secondary_primary_excluded_even_when_domain_and_service_name_differ() {
    let registry = registry_of(vec![registration_with_mismatched_identity(
        "registry-key",
        "different-name",
        &["rust/rs"],
        &["rust/trait"],
    )]);
    let paths = aps(&["src/lib.rs"]);
    let interface_ids = vec!["rust/trait".to_string()];

    let result = resolve_primary_and_secondary(&registry, &paths, &interface_ids);

    let (_, secondaries) = result.expect("expected Ok result");
    assert!(
        secondaries.is_empty(),
        "the registry's only service can never validly be its own secondary, \
         regardless of whether `service_name` or `capabilities.domain` is used \
         as the exclusion identity; got secondaries = {secondaries:?}"
    );
}

/// Regression (PR #165 review, Bug 1): when a registration's `service_name`
/// (the registry key) differs from its `capabilities.domain`, the registry
/// key is the correct identity to return — `service_name` is what callers
/// use to look the service up again (e.g. for secondary exclusion, logging,
/// transport dispatch), whereas `domain` is merely self-reported capability
/// metadata. The current implementation returns `capabilities.domain`
/// instead.
#[test]
fn test_select_service_for_artifacts_returns_service_name_not_domain_when_they_differ() {
    let registry = registry_of(vec![registration_with_mismatched_identity(
        "registry-key",
        "different-name",
        &["rust/rs"],
        &[],
    )]);
    let paths = aps(&["src/lib.rs"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "registry-key");
}

/// Regression (PR #165 review, Bug 2): a service declaring the SAME
/// `artifact_types` entry twice (`["rust/rs", "rust/rs"]`) must not score
/// higher than its distinct-type count (1), consistent with type-centric
/// scoring established elsewhere in this suite (see
/// `test_select_service_for_artifacts_duplicate_matching_extension_does_not_inflate_score_beyond_distinct_types`).
/// A service declaring two genuinely distinct matching types must win
/// outright over a service whose duplicate inflates its raw entry count to
/// the same number.
#[test]
fn test_select_service_for_artifacts_duplicate_artifact_type_does_not_tie_against_distinct_types() {
    let registry = registry_of(vec![
        registration("duplicate-svc", &["rust/rs", "rust/rs"], &[], &[]),
        registration("distinct-svc", &["rust/rs", "rust/toml"], &[], &[]),
    ]);
    let paths = aps(&["src/lib.rs", "Cargo.toml"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "distinct-svc");
}

/// Regression (PR #165 review, Bug 3): a dotfile whose name is ENTIRELY a
/// single leading-dot segment (e.g. `.gitignore`) has NO extension by
/// convention — the leading dot marks it as hidden/config, not an extension
/// separator (consistent with `std::path::Path::extension()`, which returns
/// `None` for such names). `file_extension` must therefore return `None`
/// for `.gitignore`, so it cannot match any declared `artifact_types`.
#[test]
fn test_select_service_for_artifacts_leading_dot_only_filename_has_no_extension() {
    let registry = registry_of(vec![registration(
        "dotfile-svc",
        &["dotfile/gitignore"],
        &[],
        &[],
    )]);
    let paths = aps(&[".gitignore"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert!(
        matches!(result, Err(RoutingError::NoServiceFound { .. })),
        "'.gitignore' has no extension by convention and must not match \
         'dotfile/gitignore'; got {result:?}"
    );
}

/// Characterization (PR #165 review, Bug 3 companion): a leading dot
/// followed by ADDITIONAL dots elsewhere in the filename (e.g.
/// `.config.toml`) is NOT the single-dot-is-the-whole-name special case —
/// it still correctly extracts the final component (`"toml"`) as its
/// extension. This is already-correct behaviour, included to pin it down
/// as a non-regression check alongside the Bug 3 fix.
#[test]
fn test_select_service_for_artifacts_leading_dot_with_additional_dot_still_extracts_extension() {
    let registry = registry_of(vec![registration("toml-svc", &["config/toml"], &[], &[])]);
    let paths = aps(&[".config.toml"]);

    let result = select_service_for_artifacts(&registry, &paths);

    assert_selected(result, "toml-svc");
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 3: Property-Based Tests
// ═══════════════════════════════════════════════════════════════════════════

/// Generates a short lowercase-ASCII "extension-like" token.
fn ext_strategy() -> impl Strategy<Value = String> {
    "[a-z]{1,6}"
}

/// Generates a small pool of distinct extension tokens to combine into
/// artifact_types / artifact_paths without the shrinker producing degenerate
/// all-identical cases every time.
fn distinct_ext_pair() -> impl Strategy<Value = (String, String)> {
    (ext_strategy(), ext_strategy()).prop_filter("must be distinct", |(a, b)| a != b)
}

proptest! {
    /// No-panic invariant: for ANY registry of fully-handshaken services with
    /// single-type capabilities and any non-empty single-extension artifact
    /// path, `select_service_for_artifacts` must return either `Ok` or one of
    /// the three defined `RoutingError` variants — it must never panic.
    #[test]
    fn prop_select_service_for_artifacts_never_panics(
        names in proptest::collection::vec("[a-z]{3,8}", 0..6),
        type_exts in proptest::collection::vec(ext_strategy(), 0..6),
        path_ext in ext_strategy(),
    ) {
        let n = names.len().min(type_exts.len());
        let regs: Vec<DomainServiceRegistration> = (0..n)
            .map(|i| registration(&names[i], &[&format!("domain/{}", type_exts[i])], &[], &[]))
            .collect();
        let registry = registry_of(regs);
        let paths = vec![ap(&format!("file.{path_ext}"))];

        // Must not panic; any Result variant is acceptable here.
        let _ = select_service_for_artifacts(&registry, &paths);
    }

    /// Uniqueness invariant: if exactly one service in the registry declares
    /// a type matching the (sole) extension present, and all other services'
    /// declared extensions are guaranteed distinct from it, that service is
    /// always selected — for arbitrary extension tokens.
    #[test]
    fn prop_select_service_for_artifacts_sole_matching_service_always_wins(
        (matching_ext, decoy_ext) in distinct_ext_pair(),
    ) {
        let registry = registry_of(vec![
            registration("decoy-svc", &[&format!("decoy/{decoy_ext}")], &[], &[]),
            registration("match-svc", &[&format!("real/{matching_ext}")], &[], &[]),
        ]);
        let paths = vec![ap(&format!("file.{matching_ext}"))];

        let result = select_service_for_artifacts(&registry, &paths);

        match result {
            Ok(name) => prop_assert_eq!(name, svc("match-svc")),
            Err(e) => prop_assert!(false, "expected Ok(match-svc), got Err({:?})", e),
        }
    }

    /// Case-insensitivity invariant: scrambling the case of the artifact
    /// type's extension-part and/or the path's extension never changes
    /// whether they match, for arbitrary alphabetic extension tokens.
    #[test]
    fn prop_select_service_for_artifacts_case_insensitive_for_arbitrary_extensions(
        ext in ext_strategy(),
        type_upper in any::<bool>(),
        path_upper in any::<bool>(),
    ) {
        let type_ext = if type_upper { ext.to_uppercase() } else { ext.clone() };
        let path_ext = if path_upper { ext.to_uppercase() } else { ext.clone() };

        let registry = registry_of(vec![registration(
            "svc",
            &[&format!("domain/{type_ext}")],
            &[],
            &[],
        )]);
        let paths = vec![ap(&format!("file.{path_ext}"))];

        let result = select_service_for_artifacts(&registry, &paths);

        match result {
            Ok(name) => prop_assert_eq!(name, svc("svc")),
            Err(e) => prop_assert!(false, "expected Ok(svc), got Err({:?})", e),
        }
    }

    /// Symmetry invariant: two services with IDENTICAL declared artifact
    /// types always tie — never arbitrarily resolved to one or the other —
    /// for arbitrary extension tokens and distinct service names.
    #[test]
    fn prop_select_service_for_artifacts_identical_capabilities_always_ambiguous(
        ext in ext_strategy(),
        name_a in "[a-z]{3,8}",
        name_b in "[a-z]{3,8}",
    ) {
        prop_assume!(name_a != name_b);
        let registry = registry_of(vec![
            registration(&name_a, &[&format!("domain/{ext}")], &[], &[]),
            registration(&name_b, &[&format!("domain/{ext}")], &[], &[]),
        ]);
        let paths = vec![ap(&format!("file.{ext}"))];

        let result = select_service_for_artifacts(&registry, &paths);

        let is_ambiguous = matches!(result, Err(RoutingError::Ambiguous { .. }));
        prop_assert!(is_ambiguous);
    }

    /// Order-independence invariant: reversing the order of `registrations`
    /// must not change the routing outcome (grouping/scoring must not depend
    /// on registry position).
    #[test]
    fn prop_select_service_for_artifacts_result_independent_of_registration_order(
        exts in proptest::collection::vec(ext_strategy(), 1..5),
        path_ext in ext_strategy(),
    ) {
        let regs: Vec<DomainServiceRegistration> = exts
            .iter()
            .enumerate()
            .map(|(i, e)| registration(&format!("svc{i}"), &[&format!("domain/{e}")], &[], &[]))
            .collect();
        let registry_forward = registry_of(regs.clone());
        let mut reversed = regs;
        reversed.reverse();
        let registry_backward = registry_of(reversed);
        let paths = vec![ap(&format!("file.{path_ext}"))];

        let forward = select_service_for_artifacts(&registry_forward, &paths);
        let backward = select_service_for_artifacts(&registry_backward, &paths);

        // Compare by discriminant/content rather than requiring literal Ok
        // equality, since Ambiguous candidate *order* may legitimately
        // differ; but the SET of outcomes must agree.
        match (forward, backward) {
            (Ok(f), Ok(b)) => prop_assert_eq!(f, b),
            (Err(RoutingError::NoServiceFound { .. }), Err(RoutingError::NoServiceFound { .. })) => {}
            (Err(RoutingError::Ambiguous { candidates: fc, .. }), Err(RoutingError::Ambiguous { candidates: bc, .. })) => {
                let fs: BTreeSet<_> = fc.into_iter().collect();
                let bs: BTreeSet<_> = bc.into_iter().collect();
                prop_assert_eq!(fs, bs);
            }
            (f, b) => prop_assert!(false, "forward/backward outcomes diverged: {f:?} vs {b:?}"),
        }
    }

    /// Disjointness invariant: whenever `resolve_primary_and_secondary`
    /// succeeds, the primary never appears in the secondaries list, for
    /// arbitrary registries and interface-type-identifier sets.
    #[test]
    fn prop_resolve_primary_and_secondary_primary_never_in_secondaries(
        matching_ext in ext_strategy(),
        other_names in proptest::collection::vec("[a-z]{3,8}", 0..5),
        shared_iface in "[a-z]{3,8}",
    ) {
        let mut regs = vec![registration(
            "primary-svc",
            &[&format!("domain/{matching_ext}")],
            &[&shared_iface],
            &[],
        )];
        for (i, n) in other_names.iter().enumerate() {
            prop_assume!(n != "primary-svc");
            regs.push(registration(&format!("{n}{i}"), &[], &[&shared_iface], &[]));
        }
        let registry = registry_of(regs);
        let paths = vec![ap(&format!("file.{matching_ext}"))];
        let interface_ids = vec![shared_iface.clone()];

        let result = resolve_primary_and_secondary(&registry, &paths, &interface_ids);

        if let Ok((primary, secondaries)) = result {
            prop_assert!(!secondaries.contains(&primary));
        }
    }

    /// Error-propagation invariant: whenever no artifact-type match exists
    /// at all, `resolve_primary_and_secondary` must fail with the SAME error
    /// `select_service_for_artifacts` would independently produce, for
    /// arbitrary non-matching extension tokens.
    #[test]
    fn prop_resolve_primary_and_secondary_propagates_same_error_as_select_service(
        declared_ext in ext_strategy(),
        unrelated_ext in ext_strategy(),
    ) {
        prop_assume!(declared_ext != unrelated_ext);
        let registry = registry_of(vec![registration(
            "svc",
            &[&format!("domain/{declared_ext}")],
            &[],
            &[],
        )]);
        let paths = vec![ap(&format!("file.{unrelated_ext}"))];

        let direct = select_service_for_artifacts(&registry, &paths);
        let combined = resolve_primary_and_secondary(&registry, &paths, &[]);

        let direct_is_no_service_found = matches!(direct, Err(RoutingError::NoServiceFound { .. }));
        let combined_is_no_service_found =
            matches!(combined, Err(RoutingError::NoServiceFound { .. }));
        prop_assert!(direct_is_no_service_found);
        prop_assert!(combined_is_no_service_found);
    }
}
