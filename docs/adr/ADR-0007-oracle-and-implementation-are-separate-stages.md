# ADR-0007: Oracle and Implementation Are Separate Stages, Identities and Sandboxes

**Status:** Accepted
**Date:** 2026-11-27
**Deciders:** Architecture

---

## Context

The tests that decide whether an implementation is correct are the only independent check on that
implementation. Their value rests entirely on one property: the tests were not written, chosen or
adjusted by whatever produced the code.

An agent asked to write tests and then satisfy them has every incentive and every opportunity to
converge them. It can weaken an assertion it finds inconvenient, delete a case it cannot pass,
narrow a test to the behaviour it happened to implement, or write tests shaped around the
implementation it has already decided on. None of this requires bad faith — it is the path of
least resistance for a system optimising to make checks pass. The result looks like a green run
and carries no evidence at all.

Giving one agent both jobs in one workspace makes that outcome the default. The oracle must be
unforgeable by its author, which means the author must not be able to touch it.

## Decision

Writing the tests and writing the implementation are **two stage types, under two identities, in
two sandboxes**, with the oracle sealed before implementation begins.

### Decision 1 — Two stages, in order, with a lock between them

`unit-oracle` runs first. It produces the locked tests for a slice and a **signed manifest** that
seals them. `implement` runs afterwards, against tests that already exist and are already sealed.

An `implement` attempt is not requested until the locked tests and their sealed manifest exist for
that slice. The lock is what makes the oracle an oracle: without it, "written first" degrades to
"written first and then revised".

### Decision 2 — Two identities, and the builder cannot write the oracle's paths

The stages run under different identity roles. The oracle identity writes the locked-test paths.
The builder identity writes `src/` and its own tests, and the locked-test paths are **read-only**
to it.

This is enforced outside CogWorks, by the mount plan the attempt runs under and the commit-range
scope check applied to what it produced. CogWorks is never the only thing standing between the
builder and the tests that judge it.

### Decision 3 — Two sandboxes, and the oracle never sees the implementation

Each stage runs in its own fresh sandbox, with its own context bundle built from its own profile's
input allowlist.

The barrier runs in both directions. The builder's bundle carries the locked tests as read-only
input, and carries no holdout content and no acceptance source. The oracle's bundle carries the
slice, the contracts and the specification — never implementation source, so the tests describe
what was asked for rather than what was built.

### Decision 4 — Disputes are raised, never resolved by editing

Where the implement stage judges a locked test to be wrong, the outcome is a **dispute** recorded
for a human. It is never resolved by editing the test.

Some locked tests will genuinely be wrong. An escape hatch that lets the builder fix them is
indistinguishable from an escape hatch that lets the builder delete them, so the resolution path
runs through a human rather than through the agent that is blocked.

### Decision 5 — Feedback carries results, not test source

Feedback into a rework attempt carries the gate's structured output: failing test identities and
their messages. It does not carry test source. For a holdout, it carries no more than the failing
requirement identities.

Handing back the source of a failing test invites an implementation shaped to that test rather
than to the requirement, which is the same failure the separation exists to prevent.

## Consequences

### Positive

- A passing run is evidence, because the thing that passed could not have shaped what it was
  measured against.
- The guarantee rests on identity and mount boundaries enforced by other services, not on an
  agent's good behaviour or a prompt instruction.
- Genuinely wrong tests still get fixed, through a path that leaves a record.
- The oracle describes the requirement rather than the implementation, because it never sees the
  implementation.

### Negative

- Two stages, two sandboxes and two profiles per slice, where one would have done: more latency,
  more cost, more moving parts.
- A wrong locked test blocks progress until a human resolves the dispute, and in the worst case
  that is a slow stop on work that is otherwise ready.
- Rework feedback is deliberately less informative than the full failure output, which can make
  some failures take more attempts to fix.
- The separation only holds if the mount plan and scope check are correct. It moves the critical
  guarantee into services outside this repository.

## Alternatives Considered

### One agent writes tests then implementation in one sandbox

Rejected: the author can revise its own oracle, so a green run carries no evidence. This is the
single change this decision exists to make.

### One agent, two phases, with the tests committed between them

Rejected: committing is not sealing. Without a lock enforced by identity and mount boundaries,
the second phase can still modify what the first produced.

### Same identity, with CogWorks rejecting commits that touch locked tests

Rejected: it makes CogWorks the only enforcement point for the system's most important boundary.
A flaw in CogWorks would then silently void the guarantee everywhere.

### Let the builder edit a locked test when it can justify the change

Rejected: a justification is produced by the same agent that benefits from the edit, and
"justified" and "inconvenient" are not separable from inside the loop.

## References

- `docs/spec/requirements.md` — REQ-ORACLE, REQ-CODE, REQ-SANDBOX, REQ-BUNDLE
- `docs/spec/responsibilities.md` — Oracle Stage Driver, Implement Stage Driver
- `docs/spec/constraints.md` — Stage Attempt Constraints
- `docs/spec/vocabulary.md` — Oracle, Locked Test, Oracle Manifest, Dispute, Trust Zone
- `brightsmiths/handbook` `docs/architecture/oracles.md`
