# ADR-0010: Vault-Backed Credential and Attestation Providers

**Status:** Accepted
**Date:** 2026-10-28
**Deciders:** CogWorks maintainers

---

## Context

CogWorks integrates with GitHub, LLM providers, container infrastructure, and
upstream factory services. These integrations require credentials, while the
factory also requires signed attestations that preserve chain of custody.
Embedding Vault access, authentication flows, key lifecycle, or cryptographic
details in pipeline business logic would couple the generic pipeline to
infrastructure and increase the risk of exposing secrets to the LLM.

## Decision

Credential acquisition and key lifecycle are handled by a credential provider
backed by the selected Vault library. CogWorks infrastructure clients consume
resolved credentials through this boundary. Raw key material is not part of
LLM context, prompts, audit records, logs, or generated artifacts. Vault
authentication and key lifecycle are implementation concerns of the provider.

Attestation verification and signing are handled by a separate
attestation-provider interface. Before processing, CogWorks verifies any
upstream attestation required by the configured trust policy and fails closed
if required content is missing, invalid, or mismatched. CogWorks signs its own
action attestations through that provider. Signing capability uses the
credential provider; private key material is never returned to pipeline
business logic. Signed attestations are transported as hidden HTML comments
through the work-item interface, which does not interpret their cryptographic
contents.

This decision does not select a Vault protocol, signature algorithm, signed
payload format, identity policy, or attestation-chain representation. Those
details belong in the concrete interface contract before implementation.

## Alternatives Considered

- **Direct Vault and cryptographic calls from pipeline logic:** Rejected because
  it makes domain logic depend on infrastructure APIs and expands the secret
  handling surface.
- **Pass private keys to CogWorks and sign inline:** Rejected because key
  material would be accessible to broader application code and risks accidental
  inclusion in prompts, logs, or audit data.
- **Treat signed comments as ordinary work-item data:** Rejected because
  downstream processing must verify integrity and origin before trusting an
  upstream attestation.

## Consequences

### Positive

- Vault implementation and cryptographic provider can evolve independently of
  pipeline business logic.
- Required upstream attestations are verified before processing, and CogWorks
  actions can extend the chain of custody.
- Key material remains outside model-visible and durable audit content.

### Negative

- Provider interfaces and their failure behavior must be implemented and
  tested before attestation enforcement can be enabled.
- Trust policy, payload canonicalization, identity binding, and key rotation
  semantics require concrete interface decisions before production use.

## References

- `docs/spec/requirements.md` — REQ-BOUND-005, REQ-BOUND-006
- `docs/spec/assertions.md` — ASSERT-SEC-006, ASSERT-SEC-007
- `docs/spec/architecture.md` — Credential Provider, Attestation Provider
- `docs/spec/interfaces/infrastructure.md` — Credential Provider, Attestation Provider
- `docs/spec/interfaces/github-traits.md` — signed attestation comment transport
