# ADR-0009: Provider-Neutral Implementation Job Runtime

**Status:** Accepted
**Date:** 2026-10-28
**Deciders:** CogWorks maintainers

---

## Context

CogWorks runs generic engineering implementation jobs across software and other
domains. An implementation job needs an isolated workspace to reduce context
and constrain side effects, but the eventual deployment environment is not
known. It may use a local container runtime, Kubernetes, or a cloud provider.
Pipeline state and deliverables must remain durable outside that execution
environment.

## Decision

The CogWorks orchestrator owns the implementation-job lifecycle. For each job,
it requests exactly one ephemeral container through a provider-neutral runtime
interface and requests cleanup on success, failure, or cancellation. The
provider owns runtime-specific container operations; it does not decide when
CogWorks jobs begin or what pipeline state is durable.

Retries within the same active implementation job reuse its container. A new
or resumed implementation job requests a new container. Before cleanup, the
orchestrator attempts to persist the latest recoverable state and deliverables
through the work-item and repository interfaces. If persistence fails, cleanup
is still requested; recovery uses only the last successfully persisted state.
Container-local state is never a source of durable recovery data.

## Alternatives Considered

- **Persistent shared working directory:** Rejected because it couples jobs to
  host lifecycle and retains state across work that should be isolated.
- **CogWorks chooses a concrete runtime:** Rejected because it binds the generic
  orchestrator to infrastructure that may differ between deployments.
- **Provider owns orchestration and cleanup policy:** Rejected because it
  couples CogWorks job semantics to runtime-specific behavior and weakens the
  orchestrator's ability to guarantee cleanup on every terminal path.

## Consequences

### Positive

- Jobs remain isolated and reduce context carried between implementation jobs.
- Runtime providers can vary without changing pipeline business logic.
- Durable state remains reconstructable after container loss.

### Negative

- The runtime-provider contract and orchestration recovery behavior must be
  implemented and tested before production container execution is available.
- Persistence failure can lose uncommitted job-local outputs; the system favors
  cleanup and recovery from the last durable checkpoint over retaining a
  container indefinitely.

## References

- `docs/spec/requirements.md` — REQ-PIPE-012, REQ-EXEC-008, REQ-EXEC-009
- `docs/spec/assertions.md` — ASSERT-JOB-001, ASSERT-JOB-003
- `docs/spec/edge-cases.md` — EDGE-061, EDGE-062
- `docs/spec/interfaces/infrastructure.md` — Implementation Job Runtime Provider
