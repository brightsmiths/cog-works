# ADR-0012: Tier Derivation From the Risk Map, Raise-Only

**Status:** Accepted
**Date:** 2026-11-27
**Deciders:** Architecture

**Level:** L2

---

## Context

How much rigour a piece of work gets — which stages run, which thresholds apply, whether a human
must sign off — depends on how much it matters if it is wrong. A documentation fix and a change to
a credential boundary should not be treated alike.

So something has to establish criticality, and it has to be something neither an agent nor a model
can talk its way around. A classification produced by a model is exactly the wrong mechanism: the
component asking for a lower tier benefits from getting one, and the assessment is unverifiable.

A label is not much better. Labels are editable by anyone with write access to the repository, so
a tier carried by a label is a tier an agent with label-write permission can lower.

What is stable about criticality is that it follows the code. A path either is part of a security
boundary or it is not, and that fact changes rarely and deliberately.

## Context of use

Tier selects the stage graph, the profiles resolved for each stage, the alignment thresholds, the
rework budgets and the human gates. Everything downstream depends on getting it right.

## Decision

Tier is derived deterministically from a human-owned `risk-map.yaml` in the target repository,
mapping path globs to tiers T0–T3.

### Decision 1 — Derivation is a pure function of paths

The tier of a work item is the **maximum** over the tiers of all paths it affects. A change
touching one T2 path is a T2 change, whatever else it touches.

No model participates. No label is consulted. The derivation is reproducible from the risk map and
the affected path set alone.

### Decision 2 — Derivation is raise-only

A newly derived tier may exceed a previously recorded one. It may never lower it.

Scope grows during a run — a slice turns out to touch a path nobody anticipated. Allowing the tier
to fall would mean work that has already been assessed as critical could be re-assessed downward
by a later, narrower derivation. The ratchet only turns one way.

### Decision 3 — The risk map is human-owned

CogWorks reads the risk map and never writes it. The `architect` stage may propose a patch to it
as part of its output, which a human reviews and merges like any other change.

### Decision 4 — A missing or malformed risk map halts

An absent or unparseable risk map is a halt, not a default to the lowest tier. Defaulting would
silently apply the least rigour to work nobody has assessed, which is the worst available
outcome.

### Decision 5 — Tier is never read from a label

No label is trusted as a source of tier, at any point. A label may *report* a derived tier for
human benefit, but it is never an input.

## Consequences

### Positive

- Criticality cannot be lowered by a model, an agent, or a label edit.
- The assessment is reviewable: a human reads one YAML file to see what the repository treats as
  critical.
- Derivation is reproducible, so a tier recorded in an attestation can be rechecked.
- Raise-only means growing scope mid-run increases rigour rather than quietly reducing it.

### Negative

- Every repository CogWorks operates on needs a risk map before it can run at all, which is
  onboarding friction.
- Path globs are a coarse proxy for criticality. A genuinely trivial change to a T2 path gets the
  full T2 treatment, and there is deliberately no override.
- Raise-only means an over-broad initial derivation cannot be corrected downward within a run; it
  needs a new run.
- A risk map that is wrong in the lenient direction is a silent problem, since nothing detects
  that a path *should* have been rated higher.

## Alternatives Considered

### Model classification of safety impact

Rejected: unverifiable, inconsistent, and produced by a component that benefits from a lower
answer.

### A registry of safety-critical modules

A list of module names treated as critical. Rejected in favour of path globs, which match how
code is actually organised, compose with directory structure, and can express "everything under
this subtree".

### Tier carried by a label

Rejected: editable by anyone with write access, including the agents whose rigour it governs.

### Allow a tier to be lowered with justification

Rejected: the justification is produced by the party that benefits from it, and there is no
mechanism to evaluate it that is cheaper than just doing the work at the higher tier.

### Default to the lowest tier when no risk map exists

Rejected: it applies the least rigour exactly where the most uncertainty is.

## References

- `docs/spec/requirements.md` — REQ-TIER, REQ-CLASS
- `docs/spec/architecture.md` — Tier Derivation, Risk-Map Loader
- `docs/spec/responsibilities.md` — Tier Deriver
- `docs/spec/vocabulary.md` — Tier, Risk Map
- `brightsmiths/handbook` `docs/architecture/workflow.md` — tiers
