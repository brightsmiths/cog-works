# ADR-0014: Upstream Attestations Are Verified, Not Recomputed

**Status:** Accepted
**Date:** 2026-11-27
**Deciders:** Architecture

**Level:** L3

---

## Context

Before CogWorks starts work on an issue, several things have already been decided by services
upstream of it: whether the issue is safe to act on at all, what kind of work it is and how
urgent, and which areas of the system it touches.

CogWorks needs those answers. The question is whether it should trust them, re-derive them, or
refuse to proceed without them.

Re-deriving is tempting because it removes a dependency, and it is wrong for two reasons. It
duplicates logic that belongs to another service, so the two drift and disagree. And it means each
service's safety depends on its own correctness rather than on a chain anyone can check — the
intake screening would then exist in two places, both of which have to be right.

There is also the question of *how* those answers arrive. An upstream service can state its
conclusion in prose a human can read, or in a machine-readable block. Parsing prose to extract a
decision is fragile in a way that fails silently: a wording change becomes a behaviour change, and
untrusted text in the prose becomes an input to a parser.

## Decision

CogWorks verifies the signed conclusions of upstream services and consumes their machine-readable
blocks. It does not re-derive what they decided, and it does not parse prose.

### Decision 1 — Required upstream attestations are verified before acting, and failure is closed

Before acting on a work item, CogWorks verifies every upstream attestation its configured trust
policy requires: the intake safety approval, the triage conclusion, the scoping assessment.

A required attestation that is missing, unsigned, signed by an unexpected identity, or mismatched
against the content it describes is a **halt**. Failing open would make the entire chain
advisory.

### Decision 2 — Machine-readable blocks only; prose is never parsed

Triage conclusions and affected-area assessments are read from the structured blocks the upstream
services publish. Their prose summaries are for humans.

CogWorks never parses prose to extract a decision, an area, a priority or a type.

### Decision 3 — Intake screening is not repeated

Screening an incoming work item for prompt injection belongs to the intake service, which signs
its approval. CogWorks verifies that signature rather than re-running the scan.

One service owns the screening, so there is one place for it to be right and one place to improve
it.

### Decision 4 — Sanitising model-sourced text remains CogWorks' own defence in depth

The above concerns untrusted *external* text. Text that CogWorks itself obtained from a model and
intends to reuse in a later prompt — a finding description fed into a rework attempt, an
interpretation of a failure — is scanned before reuse.

This is not duplicated upstream work. It is model output re-entering a prompt, which no upstream
service ever saw, and it stays here.

## Consequences

### Positive

- Each decision has exactly one owner, so there is no drift between two implementations of the
  same judgement.
- Trust is a chain that can be checked end to end rather than a set of independent assertions.
- Structured blocks mean a wording change upstream cannot change behaviour downstream.
- Not parsing prose removes a parser whose input is untrusted text.

### Negative

- CogWorks cannot start work when a required upstream service is down or has not yet run, and
  fail-closed makes that a visible stop rather than a degraded mode.
- The structured block schemas become a contract between services that has to be versioned, and a
  schema change upstream is a breaking change downstream.
- During bring-up, humans stand in for the upstream services, which means human-produced
  attestations must be verifiable the same way.

## Alternatives Considered

### Re-derive upstream conclusions locally

Rejected: it duplicates another service's logic, the two drift, and safety stops being a checkable
chain.

### Trust upstream conclusions without verifying signatures

Rejected: an unverified claim on an issue is writable by anyone who can comment, which makes the
trust boundary decorative.

### Parse the prose summaries

Rejected: fragile against wording changes, and it makes a parser consume untrusted text to produce
a decision.

### Fail open when an attestation is missing

Rejected: a trust boundary that can be bypassed by omission is not a boundary. Omission is the
easiest condition for an attacker to arrange.

## References

- `docs/spec/requirements.md` — REQ-ATTEST, REQ-CONST
- `docs/spec/responsibilities.md` — Intake Step
- `docs/spec/security.md` — upstream attestation verification
- `docs/spec/edge-cases.md` — missing, unsigned or mismatched upstream attestation
- `brightsmiths/handbook` `docs/architecture/services.md`
