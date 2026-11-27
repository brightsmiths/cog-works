# ADR-0010: Context Bundles Are Allowlisted, Hashed and Attested

**Status:** Accepted
**Date:** 2026-11-27
**Deciders:** Architecture

---

## Context

What an agent produces is a function of what it was shown. That makes the set of inputs to an
attempt the most load-bearing thing about it, for three separate reasons.

It is a **security** boundary. Untrusted text — issue bodies, comments, repository files, earlier
model output — reaches a model as content. Anything in the input set can attempt to influence
behaviour, so what is in the set has to be a deliberate decision rather than whatever happened to
be nearby.

It is a **trust-zone** boundary. An implementation stage must not see holdout tests or acceptance
source, or the oracle stops being independent. A barrier of this kind cannot be a list of things
to exclude, because the list is only as good as its author's imagination.

It is a **reproducibility** boundary. A failure that cannot be reproduced cannot be diagnosed, and
an attestation that does not record what the agent saw does not attest to anything useful.

## Decision

Every stage attempt consumes a **context bundle**: an explicitly allowlisted, hashed set of
inputs, recorded in the attestation.

### Decision 1 — Allowlist, not denylist

The bundle is built strictly from the input allowlist in the stage's resolved contract. An input
the allowlist does not name is **absent from the bundle**, not ranked lower and not truncated
away.

This is the central property. A denylist fails open — anything nobody thought to exclude is
included. An allowlist fails closed: a new kind of input is absent until someone admits it.

It is also what makes the trust-zone barrier real. The implement stage's bundle does not contain
holdouts because its allowlist does not name them, not because something remembered to strip
them.

### Decision 2 — The bundle is hashed before the attempt starts

The assembled bundle is hashed and the hash recorded before the attempt is requested. An attempt
whose bundle was not hashed is not requested.

### Decision 3 — The stage contract is never in the bundle it governs

The contract names the allowlist, the writable paths, the limits and the identity role. It is
never placed in the bundle of the stage it governs.

An agent that can read its own constraints can reason about them — about which paths are checked,
which limits apply, and how close it is to an escalation. The constraints are for the machine, not
for the agent.

### Decision 4 — Truncation is deterministic, and never silently drops a mandatory input

Where the allowlisted content exceeds the token budget, truncation is deterministic and
priority-ordered, so the same inputs truncate the same way.

Truncation never removes a mandatory input. If the mandatory inputs alone exceed the budget, that
is an escalation, not a silent truncation — an attempt that silently lost its specification is
worse than an attempt that did not run.

### Decision 5 — The bundle hash is recorded in the attestation

The stage attestation records the bundle hash alongside the resolved profile and harness version.
Together they make the attempt reproducible: the same profile at the same pin over the same bundle.

Context Packs reach the bundle through this mechanism and are covered by its hash.

## Consequences

### Positive

- The input set fails closed, so a new input type cannot leak into a bundle by default.
- The trust-zone barrier is structural rather than a list of exclusions to maintain.
- A failure can be reproduced from its recorded hash, and an attestation records what the agent
  actually saw.
- An agent cannot reason about its own constraints, because it cannot read them.

### Negative

- Every new input a stage needs requires a harness change to its allowlist, which is friction
  during bring-up and the point of the design afterwards.
- A too-narrow allowlist fails in a way that looks like model incapability — the agent is not
  wrong, it was not shown enough. Diagnosing that requires reading the allowlist, not the output.
- Hashing requires a stable canonical form for the bundle; an unstable serialisation would make
  hashes differ for identical content.

## Alternatives Considered

### Priority-ordered context with truncation, and no allowlist

Rank everything available and cut from the bottom. Rejected: it fails open. Whether a holdout
reaches a build stage then depends on the token budget, which is not a security boundary.

### Denylist of things to exclude per stage

Rejected: the barrier is only as complete as the list, and a new input type is included by
default. This is the failure mode the allowlist exists to remove.

### Let the agent retrieve what it needs

Rejected for these stages: what the agent saw would depend on what it chose to fetch, which makes
the hash meaningless and the trust-zone barrier unenforceable.

### Hash after the attempt, from what was actually sent

Rejected: it records what happened rather than committing to it beforehand, so an attempt could
proceed with a bundle nothing had yet constrained.

## References

- `docs/spec/requirements.md` — REQ-BUNDLE, REQ-ORACLE, REQ-CPACK
- `docs/spec/interfaces/context.md` — `ContextBundle`
- `docs/spec/responsibilities.md` — Context Bundle Builder
- `docs/spec/security.md` — context-bundle leakage across trust zones
