# ADR-0002: Context Packs Feed the Context Bundle

**Status:** Accepted
**Date:** 2026-02-24
**Deciders:** Architecture

---

## Context

An agent working in an unfamiliar domain needs more than the work item and the code it touches.
It needs the domain's conventions, the patterns that are safe in that domain, and the patterns
that look reasonable but are not. Without that knowledge an agent produces output that compiles
and passes shallow checks while violating a convention every practitioner in the domain knows.

This knowledge is stable, reusable across work items, and specific to a domain rather than to a
repository. It is also prompt material: whatever it says reaches a model as trusted,
configuration-level content.

That last property decides where it may live. Content that shapes an agent's behaviour must not
be editable by an agent, or by the repository an agent is working in. A knowledge document stored
in a target repository is prompt content that an agent with write access to that repository could
rewrite — and the agent working there is exactly the one the document is meant to constrain.

## Decision

Domain knowledge is packaged as **Context Packs**, which live in the harness as part of a
discipline pack and reach a stage attempt only through its context bundle.

A pack carries a trigger definition, a domain knowledge document, a safe-patterns document, and
an anti-patterns document that explains why each pattern is unsafe.

Pack selection is deterministic. Triggers key on the work item's tier, discipline, stage type and
detected language or toolchain. No model participates in selecting a pack, and no model output is
an input to the selection.

A pack reaches an agent only as part of a context bundle, and therefore only if the stage
contract's input allowlist admits it. Its content is covered by the bundle hash, so what the
agent was told is recorded alongside what it produced.

Packs are versioned with the rest of the harness. A repository adopts a pack change by moving its
pin, so a pack edit cannot alter a repository's behaviour until someone chooses it.

Where two loaded packs give contradictory guidance, the more restrictive rule applies. A pack may
tighten guidance for its domain; it may never relax a global threshold, and it may never override
the constitutional rules.

## Consequences

### Positive

- Domain knowledge is written once and reused across every repository in a discipline.
- Pack content cannot be edited by an agent working in a target repository, so the content that
  constrains an agent is outside that agent's reach.
- Deterministic selection means the same work resolves to the same packs, which is what makes a
  run reproducible and an attestation meaningful.
- Pin-gated adoption means a pack change rolls out deliberately rather than silently.

### Negative

- A repository with a genuinely local convention cannot express it as a pack. It belongs in the
  repository's own specification paths, or in the discipline pack if it generalises.
- Pack authorship concentrates in the harness, which needs its own review discipline to avoid
  becoming a bottleneck.
- Multiple loaded packs consume context budget that the truncation rules then have to arbitrate.

## Alternatives Considered

### Packs stored in the target repository

Each repository carries its own packs under a well-known path. Rejected: the content is prompt
material, so a repository-local pack is prompt content an agent working in that repository could
edit, and the anti-faking controls require that agent-shaping content not be agent-writable.

### Knowledge retrieved by the agent on demand

Give the agent a search tool over a knowledge corpus and let it decide what to read. Rejected for
the stages this governs: what the agent saw would then depend on what it chose to look for, which
makes a bundle hash meaningless and a failure impossible to reproduce. Retrieval remains
appropriate for discovery work, not for the knowledge that constrains an attempt.

### Knowledge embedded in prompt templates

Fold domain guidance into each stage's prompt template. Rejected because guidance then multiplies
across every template that needs it, and the same convention drifts between stages.

## References

- `docs/spec/requirements.md` — REQ-CPACK, REQ-BUNDLE
- `docs/spec/interfaces/context.md` — Context bundle assembly
- `docs/spec/vocabulary.md` — Context Pack, Context Bundle, Harness
- `brightsmiths/handbook` `docs/architecture/agent-profiles.md` — profile skills and packs
