# ADR-0001: Dynamic Capability Discovery via Handshake

**Status:** Accepted
**Date:** 2026-02-24
**Deciders:** Architecture

---

## Context

CogWorks routes domain operations — normalise, validate, review rules, simulate, dependency
graph, interface extraction — to external domain services over the Extension API. To route an
operation it must know which service handles which domain, artifact type and interface type.

That knowledge can come from two places: a static declaration an operator maintains, or the
service itself. A static declaration can drift from what the service actually implements, and
nothing detects the drift: a service that gains a capability is silently not used for it, and a
service rolled back to a version that lost one is still sent requests it cannot serve. Both
failures surface as confusing errors at the point of use rather than at startup.

Every conformant domain service already answers a handshake that reports its `name`,
`service_version`, `api_version`, `domain`, `capabilities`, `artifact_types` and
`interface_types`. The information is therefore available from the authoritative source.

## Decision

CogWorks discovers domain service capabilities from the handshake, at startup and on reconnect.

Its own process configuration carries only what is needed to open a connection:

```toml
[[services]]
name = "firmware-service"
transport = "unix"
endpoint = "/var/run/cogworks/firmware.sock"
```

Capabilities, artifact types, interface types and domain identity are read from the handshake
response and never stored in configuration. The result is cached in memory for the duration of a
run and re-queried on connection error.

A service whose `api_version` falls outside CogWorks' supported range is rejected at startup and
reported as unavailable. Remaining stages proceed without it, and the operator is notified
through structured log output.

## Consequences

### Positive

- A service's declared capabilities cannot drift from what it implements, because there is no
  separate declaration to drift.
- Adding or removing a capability requires no configuration change and no redeployment of
  CogWorks.
- Version incompatibility is detected once, at startup, instead of at each point of use.
- Connection configuration is small enough to review at a glance.

### Negative

- A run cannot start routing to a service that is unreachable at startup, even for capabilities
  it would have declared statically.
- The handshake adds a round trip per service per run, and an in-memory cache that must be
  invalidated correctly on reconnect.

## Alternatives Considered

### Static capability declaration in configuration

An operator declares each service's domain and capabilities alongside its endpoint. Rejected
because the declaration is a second source of truth for something the service already knows, and
the two drift silently in both directions.

### Declaration with startup verification

Declare capabilities statically, then verify them against a handshake and fail on mismatch.
Rejected as the worst of both: it keeps the duplicate declaration and its maintenance burden,
and adds a failure mode where a correct service is rejected for a stale config entry.

## References

- `docs/spec/interfaces/domain-traits.md` — Domain Service Client and the handshake
- `docs/spec/responsibilities.md` — Domain Service Registry
- `docs/spec/constraints.md` — Extension API
