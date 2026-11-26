# ADR-0005: Credential and Attestation Provider Boundaries

**Status:** Accepted
**Date:** 2026-11-27
**Deciders:** Architecture

---

## Context

CogWorks talks to GitHub, to a model gateway, to the stage runner and to upstream factory
services. It also has to prove what it did: the factory's chain of custody requires each service
to sign what it saw and produced, and the next service to verify before acting.

Credentials and private keys are the most dangerous values in the process. A key that reaches a
prompt, an audit record, a log line or a generated artifact has leaked. Business logic that
handles authentication flows or key lifecycle both couples the pipeline to infrastructure and
widens the surface on which that leak can happen.

What CogWorks needs is also narrower than it first appears. It writes labels, comments, sub-issues
and pull requests, so it needs a GitHub credential. It does not push commits — the stage runner
does, under the stage's identity role — and it does not call a model provider directly, because
the model gateway holds those keys. It authors no check run.

## Decision

### Decision 1 — Credentials resolve through a provider, and the scope is narrow

Credential acquisition and key lifecycle sit behind a credential-provider interface, backed by
the deployment's secret store. Infrastructure clients consume resolved credentials through that
boundary. The provider's authentication flow and key lifecycle are its own concern, abstracted
over any Vault or KMS so the deployment stays cloud-neutral.

Raw key material never appears in a context bundle, a prompt, an audit record, a log, or a
generated artifact, and is never returned to business logic.

The GitHub credential is scoped to what CogWorks actually does: issues read and write, pull
requests read and write, labels read and write, **contents read only**, **checks read only**.

Three credentials CogWorks must not hold, in any deployment:

- a credential that can **push commits** to a target repository,
- a **model provider API key**,
- a credential that can **create or update a check run**.

Each is held by the component that owns the corresponding responsibility: the stage runner's
identity roles, the model gateway, and the gate service.

### Decision 2 — Attestation signing and verification sit behind their own provider

Verification and signing sit behind an attestation-provider interface, distinct from the
credential provider.

Before acting, CogWorks verifies every upstream attestation the configured trust policy requires,
and **fails closed** if required content is missing, invalid or mismatched. It signs its own
stage attestations through the provider. Signing uses the credential provider's capability;
private key material is never returned to business logic.

Signed attestations are transported as hidden comments through the work-item interface, which
does not interpret their cryptographic content.

Attestations are **evidence, not workflow state**. A run's position is always reconstructable
from GitHub without reading one. This keeps the attestation store off the critical path: losing
it loses evidence, not the ability to continue.

## Consequences

### Positive

- The secret store and the signing implementation can change without touching pipeline logic.
- The blast radius of a compromise of this process excludes pushing code, calling a model
  provider directly, and authoring a verdict.
- Required upstream attestations are verified before CogWorks acts, so a missing signature stops
  work rather than being discovered later.
- Treating attestations as evidence keeps workflow recovery independent of the evidence store.

### Negative

- Fail-closed verification means an outage or misconfiguration of an upstream signer stops runs.
  That is the correct trade for a trust boundary, but it needs a clear operator diagnostic.
- Several interfaces must exist and be tested before attestation enforcement can be switched on.
- Trust policy, payload canonicalisation, identity binding and key rotation remain to be settled
  in the concrete interface contract.

## Alternatives Considered

### Direct secret store and cryptographic calls from pipeline logic

Rejected: it makes domain logic depend on infrastructure APIs and widens the surface on which key
material can leak into a prompt or an audit record.

### Pass private keys to CogWorks and sign inline

Rejected: key material would be reachable from general application code, and the failure mode is
silent inclusion in a prompt, log or artifact.

### Treat upstream signed comments as ordinary work-item data

Rejected: content whose whole purpose is to establish trust must be verified before it is acted
on, not parsed as data.

### Fail open when an upstream attestation is missing

Rejected: it makes the trust boundary advisory, which is the same as not having one.

## References

- `docs/spec/requirements.md` — REQ-BOUND, REQ-ATTEST
- `docs/spec/architecture.md` — Credential Provider, Attestation Provider, Boundary Rules
- `docs/spec/interfaces/infrastructure.md` — Credential Provider, Attestation Provider
- `docs/spec/interfaces/github-traits.md` — signed attestation comment transport
- `docs/spec/constraints.md` — Security
