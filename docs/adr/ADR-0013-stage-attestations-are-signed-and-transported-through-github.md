# ADR-0013: Stage Attestations Are Signed and Transported Through GitHub

**Status:** Accepted
**Date:** 2026-11-27
**Deciders:** Architecture

**Level:** L2

---

## Context

The factory's guarantees are a chain. The oracle was sealed before implementation started; the
agent that ran was the one the harness selected; the inputs it saw were the ones its allowlist
admitted; the verdict came from the gate service. Each link is only worth something if the next
service can check it rather than assume it.

Checking requires evidence that cannot be fabricated by the party it describes. A log line is not
evidence — it is a claim by the component whose behaviour is in question.

There is a second requirement that pulls in the opposite direction. GitHub is the only system of
record, and any service must be able to restart and rebuild its view from GitHub alone. If
attestations were workflow state, an evidence store outage would stop work, and the evidence store
would be on the critical path for every run.

## Decision

Each stage attempt produces a signed **stage attestation**, transported as a hidden comment on the
work item, carrying what was chosen and what was seen.

### Decision 1 — What an attestation records

The stage type and node, the resolved agent profile and the harness version it came from, the
context bundle hashes, the identity role the attempt ran under, the mount plan it was given, the
verdicts read and the identity that authored them, and the attempt's outcome.

Together these make the attempt reproducible and the selection auditable: the same profile at the
same pin over the same bundle.

### Decision 2 — Signing goes through the shared library, not through local cryptography

Attestations are signed through the factory's shared signing and attestation library, so every
service produces the same shape and verifies the same way. CogWorks does not implement signing,
and private key material never reaches business logic.

Ed25519 today, moving to in-toto/DSSE. The payload format and algorithm are the library's concern,
not CogWorks'.

### Decision 3 — Transport is a hidden comment on the work item

Attestations travel as structured hidden comments through the work-item interface, which does not
interpret their cryptographic content. This keeps GitHub the single system of record and makes an
attestation visible to any service that can read the issue.

### Decision 4 — Attestations are evidence, never workflow state

A run's position is always reconstructable from GitHub without reading a single attestation. The
attestation store is an evidence store and telemetry sink, not a source of truth for what to do
next.

Losing it loses the ability to audit, not the ability to continue. This is what keeps it off the
critical path.

### Decision 5 — Verification is the consumer's job, and it fails closed

CogWorks verifies the upstream attestations its trust policy requires before acting, and fails
closed when a required one is missing, unsigned or mismatched. It does not verify its own.

## Consequences

### Positive

- Each stage's inputs and choices are provable after the fact by a party that trusts nothing about
  the run.
- A shared library means a uniform attestation shape across services, and one place to move from
  Ed25519 to DSSE.
- Keeping attestations out of workflow state means an evidence store outage degrades auditability,
  not throughput.
- Transporting through GitHub means no additional system has to be reachable to read the evidence.

### Negative

- Hidden comments accumulate on long-running work items, and a busy issue becomes slow to read
  through the API.
- A comment is only as durable as the issue. A deleted issue takes its attestations with it, so
  the evidence store must also receive them.
- Fail-closed verification of upstream attestations means an upstream signer's outage stops work.
- Attestation volume grows with attempts, and a heavily reworked slice produces a lot of evidence
  for one outcome.

## Alternatives Considered

### Unsigned structured records

Rejected: a record the producing component could have written freely is a claim, not evidence, and
the next service would be assuming rather than checking.

### Attestations as workflow state

Store run position in the attestation chain. Rejected: it puts the evidence store on the critical
path and breaks the rule that any service can rebuild its view from GitHub alone.

### CogWorks implements its own signing

Rejected: per-service cryptography means per-service bugs, divergent payload formats, and a
migration to DSSE that has to happen independently everywhere.

### A dedicated attestation API instead of GitHub comments

Rejected as the primary transport: it adds a system that must be reachable for evidence to be
readable, and GitHub is already the system of record. The evidence store receives a copy.

## References

- `docs/spec/requirements.md` — REQ-ATTEST, REQ-AUDIT, REQ-BOUND
- `docs/spec/responsibilities.md` — Stage Attestation Writer
- `docs/spec/interfaces/github-traits.md` — signed attestation comment transport
- `brightsmiths/handbook` `docs/architecture/state-model.md`
