# ADR-0004: Provider-Neutral Stage Attempt Runtime

**Status:** Accepted
**Date:** 2026-11-27
**Deciders:** Architecture

---

## Context

Every agent turn runs in isolation: a fresh microVM with a read-only root filesystem, no
secrets, a non-root user, and deny-by-default egress reaching only the model gateway and a
curated dependency proxy. That isolation is what makes running an agent unattended acceptable.

Providing it requires privilege. Creating a microVM needs virtualisation capability on a host;
pushing the agent's commits needs a credential that can write to the target repository. Both are
exactly the privileges a component that also decides what runs next should not hold: a flaw in
the decision-making would otherwise reach the host and the repository.

The deployment environment is also not knowable in advance. A deployment may run on-premises or
in any cloud, with microsandbox as the reference provider and Firecracker or Kata as
alternatives. Nothing CogWorks decides should depend on which.

## Decision

A stage attempt runs in a sandbox created for it by a separate **stage runner** service, reached
through a provider-neutral interface.

CogWorks decides *that* an attempt should happen and *what it may see and touch*. It requests one
attempt, passing the image, the context bundle, the mount plan obtained from the mount-plan
provider, the network profile, the resource limits, and the identity role — every one of them
resolved from the stage contract. The runner creates the sandbox, runs the attempt, and pushes
the resulting commits under that identity role.

**One fresh sandbox per attempt.** A retry is a new attempt and therefore a new sandbox. No
sandbox is reused across attempts, and no state carries from one attempt to the next except
through GitHub. Reuse would let a failed attempt's residue — a partial edit, a cached artifact, a
stale lock — influence the next, which makes a failure unreproducible and an attestation
untrue.

**CogWorks holds no virtualisation privilege, no host privilege and no push credential.** It is
an API client of the stage runner and nothing more.

Completion is observed through GitHub state: the expected branch carries the attempt's commits
and its output record is present. A queue event may prompt an earlier read, but it is a hint and
never the source of truth, because a signal that exists only in a queue cannot be reconstructed
after a restart.

Sandbox-local files are transient. Durable state and deliverables are persisted through the
work-item and repository interfaces before teardown. If persistence fails, teardown still
proceeds and recovery uses the last successfully persisted state; sandbox-local state is never a
source of durable recovery data.

## Consequences

### Positive

- The privileges needed to isolate an agent sit in a service whose only job is to provide that
  isolation, not in the component that decides what to run.
- Attempts are genuinely independent, so a failure is reproducible from its recorded bundle hash.
- Nothing CogWorks decides depends on a container runtime, orchestrator or cloud API.
- An attempt's outcome survives a restart of CogWorks, because it is observable in GitHub.

### Negative

- A fresh sandbox per attempt costs start-up latency and dependency fetching on every retry.
  Warm pools and a dependency proxy mitigate it; the isolation guarantee is worth the cost.
- CogWorks cannot run a stage at all without the stage runner, so the runner is on the critical
  path for every run.
- Observing completion through GitHub state is slower than being told directly, and requires the
  output record's shape to be specified precisely enough to detect a partial write.

## Alternatives Considered

### CogWorks runs the sandbox itself

Rejected: it would require CogWorks to hold virtualisation privilege and a push credential,
putting the host and the repository within reach of a flaw in the orchestration logic.

### Reuse one sandbox across the attempts of a stage

Rejected: residue from a failed attempt influences the next, which makes failures unreproducible
and defeats the purpose of recording a bundle hash per attempt.

### A persistent shared working directory

Rejected: it couples attempts to host lifecycle and retains state across work that should be
isolated.

### CogWorks selects a concrete runtime

Rejected: it binds a deliberately deployment-neutral component to infrastructure that differs
between deployments.

### Queue events as the completion signal

Rejected as the authority: a run must be able to reconstruct its position from GitHub alone, and
a queue event cannot be re-read after a restart. Retained as an optional accelerator.

## References

- `docs/spec/requirements.md` — REQ-SANDBOX, REQ-PIPE, REQ-EXEC
- `docs/spec/interfaces/infrastructure.md` — Stage-Runner Client
- `docs/spec/architecture.md` — Stage Runner abstraction, Boundary Rules
- `docs/spec/edge-cases.md` — attempt never reports completion
- `brightsmiths/handbook` `docs/architecture/isolation-and-security.md`
