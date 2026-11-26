# ADR-0008: The Gate Service Is the Sole Source of Pass or Fail

**Status:** Accepted
**Date:** 2026-11-27
**Deciders:** Architecture

---

## Context

Something must decide whether the work a stage produced is acceptable. That decision is the point
at which everything else in the system either means something or does not.

The component that orchestrates agents is the wrong component to make it. It has an interest in
the answer: it is measured by whether runs complete, it holds the attempt budget, and it is the
thing that would otherwise have to escalate to a human. A verdict computed by the component whose
run it concludes is self-certification, and self-certification is indistinguishable from no
certification.

The decision also has to be reproducible by someone who does not trust the run that produced it.
That means recomputing it from a commit SHA, in a clean environment, under an identity the run
could not influence — not reading a claim the run left behind.

## Decision

Pass and fail are computed by a separate **gate service**, which recomputes every verdict from a
commit SHA in a clean sandbox under its own identity, reading gate configuration from the default
branch rather than from the branch under judgement. It is the only permitted source of required
checks.

### Decision 1 — CogWorks computes no verdict and authors no check run

CogWorks holds no credential capable of creating or updating a check run. It never computes a
pass or fail of its own, and never satisfies a required check.

### Decision 2 — Verdicts are read from check runs on the attempt's SHA

CogWorks reads the check runs on the SHA its attempt produced and derives a verdict state:
passing, failing, pending or absent.

Only check runs **authored by the gate service's identity** are considered. A check run from any
other author is ignored entirely — not weighted, not treated as advisory. This filter is applied
at the infrastructure boundary, so business logic cannot act on a check run from an unexpected
author even by mistake.

A verdict bound to a SHA other than the attempt's is treated as absent. A pending or absent
verdict means **wait**, never assume.

### Decision 3 — The verdict is the rework loop's condition

Whether a stage advances, reworks, or escalates is decided by the gate verdict, not by the
agent's report of its own success and not by CogWorks' own checks.

### Decision 4 — CogWorks' own checks bound an attempt but are never a verdict

CogWorks runs deterministic pre- and post-checks around a stage attempt: schema validation,
alignment verification, protected-path checks, cross-domain constraint validation. These are
useful and they are kept.

The line is **publication and authority, not computation**. CogWorks may fail, retry or rework an
*attempt* on the result of its own check. It may not publish that result as a check run, may not
set or satisfy a required check, and may not treat it as a gate verdict. An attempt that passes
every CogWorks post-check still has to pass the gate service.

### Decision 5 — Holdout feedback is bounded

Where a verdict comes from a holdout run, the feedback CogWorks may read and pass on is limited to
the failing requirement identities. Holdout content never reaches a build stage, because feedback
rich enough to fix a holdout failure directly is feedback rich enough to overfit to it.

## Consequences

### Positive

- A green run is independently reproducible from a SHA by someone who trusts nothing about the
  run that produced it.
- The component with an interest in the answer cannot produce it.
- Accepting only one identity's check runs gives CogWorks exactly one trust anchor for pass and
  fail, which is a small and auditable surface.
- Waiting on a pending verdict rather than assuming one removes a whole class of race condition.

### Negative

- The gate service is on the critical path for every stage. Its outage stops all progress, and
  CogWorks' correct behaviour in that case is to wait, which looks like a hang.
- Verdicts arrive asynchronously, so the step function must handle pending states and resume
  cleanly, rather than reading a result inline.
- That single trust anchor is also a single point of forgery: a check run that appears to come
  from the gate identity would be believed. Author verification must be exact.
- Holdout feedback is deliberately coarse, so holdout failures are harder to fix and consume more
  attempts.

## Alternatives Considered

### CogWorks computes the verdict from checks it runs

Rejected: self-certification by the component whose run it concludes. It also puts toolchain
execution back inside CogWorks, which breaks discipline-neutrality.

### The agent reports whether it succeeded

Rejected: the claim is produced by the thing being judged, and nothing recomputes it.

### CogWorks publishes its own check runs alongside the gate's

Rejected: two authorities for the same question. In practice the required check would end up being
whichever is more convenient, and the distinction between "bounds an attempt" and "is a verdict"
would be lost.

### Gate configuration read from the branch under judgement

Rejected: a branch could then weaken the gates applied to itself. Configuration comes from the
default branch.

## References

- `docs/spec/requirements.md` — REQ-VERDICT, REQ-REWORK, REQ-BOUND
- `docs/spec/architecture.md` — Verdict Interpretation, Check-Run Reader, Boundary Rules
- `docs/spec/interfaces/github-traits.md` — `CheckRunReader`
- `docs/spec/security.md` — forged or spoofed gate check run
- `brightsmiths/handbook` `docs/architecture/workflow.md` — gates
