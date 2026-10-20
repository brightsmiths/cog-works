---
"pipeline": minor
---

Implement `select_service_for_artifacts` and `resolve_primary_and_secondary` in `crates/pipeline/src/domain_services.rs`, completing the artifact-to-domain-service routing subsystem.

`select_service_for_artifacts` filters the registry to services with a completed handshake (`capabilities: Some`), scores each by how many of its declared `artifact_types` (convention `"domain/extension"`, e.g. `"rust/source"`) match — case-insensitively — the file extensions found in the supplied `artifact_paths`, and returns the unique highest-scoring service. A tie at the highest score yields `RoutingError::Ambiguous` with the tied candidates; no service matching any extension yields `RoutingError::NoServiceFound`.

`resolve_primary_and_secondary` delegates to `select_service_for_artifacts` to determine the primary service, then collects all other registered, handshake-complete services whose `interface_types` intersect the caller-supplied `interface_type_identifiers` as secondaries — always excluding the primary, even when the primary's own `interface_types` also intersects.

No public API shapes changed — `select_service_for_artifacts`, `resolve_primary_and_secondary`, `ServiceCapabilities`, `DomainServiceRegistration`, `ServiceRegistry`, and `RoutingError` were already fully specified; this change fills in the previously-`todo!()` implementation bodies.
