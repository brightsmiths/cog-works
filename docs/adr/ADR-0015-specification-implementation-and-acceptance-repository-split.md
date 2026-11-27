# ADR-0015: Specification, Implementation and Acceptance Repository Split, and Holdouts

**Status:** Accepted
**Date:** 2026-11-27
**Deciders:** Architecture

**Level:** L4

---

## Context

At the lower tiers, keeping the specification, the implementation and the acceptance tests in one
repository is convenient and harmless. At the product tier it stops being harmless, for a reason
that follows directly from how the oracle separation works.

The mount plan bounds what an attempt can write within a repository. It is a strong boundary, but
everything it protects is still *present*, and an attempt that can read a path can be influenced
by it. Acceptance criteria that an implementation must satisfy, sitting in the same tree as the
implementation, are readable by the agent writing that implementation — and an implementation
shaped around the acceptance criteria it can read is overfitted to them.

**Holdout** acceptance tests make this concrete. Their value is that the implementation was
produced without knowledge of them. That property cannot be provided by read permissions inside
one repository, because the content is still there to be found, and a single mistake in a mount
plan or an allowlist exposes it permanently.

Specifications have a related but different problem: they are written by one trust zone and read
by others, and at the product tier the human approval of a specification is a gate in its own
right.

## Decision

At the product tier and above, a product occupies three repositories: specification,
implementation, and acceptance. Holdouts live outside the implementation repository.

### Decision 1 — Three repositories, by trust zone

The **specification** repository holds intent, contracts, plans and architectural decisions. The
design zone writes it; every other zone reads it.

The **implementation** repository holds source and the implementation's own tests. The build zone
writes it.

The **acceptance** repository holds acceptance criteria and holdout tests. The oracle zone writes
it, and the build zone never reads it.

The split follows the trust zones, so the boundary is enforced by repository access rather than by
path permissions within one tree.

### Decision 2 — Holdouts are never present in the implementation repository

Holdout content does not exist in the repository the implementation agent works in. It is not
present and write-protected; it is absent.

A boundary that depends on content being present but unreadable fails to a single mount-plan or
allowlist error. A boundary that depends on content being in a repository the attempt has no
credential for does not.

### Decision 3 — Holdout runs happen in the gate service, and feedback is bounded

Holdout tests are executed by the gate service, which can reach the acceptance repository.
CogWorks reads the resulting check run like any other verdict.

Feedback from a holdout failure carries **no more than the failing requirement identities**. Not
the test names, not the assertions, not the diff. Feedback rich enough to fix a holdout failure
directly is feedback rich enough to overfit to it, which would reintroduce the problem the holdout
exists to detect.

### Decision 4 — Lower tiers stay in one repository

At T0 and T1 a single repository with path-based separation is sufficient and the operational cost
of three repositories is not justified. Tier selects the layout, and the layout is not a per-repository
choice.

### Decision 5 — Cross-repository state stays in the work item

A run spanning three repositories still keeps its state on the work item. There is one run, one
state comment, and one place to reconstruct position from.

## Consequences

### Positive

- The holdout guarantee rests on repository access rather than on the correctness of a mount plan.
- A mistake in an allowlist cannot expose holdout content that is not in the tree.
- Specification approval becomes a reviewable gate on its own repository.
- The trust zones map onto an enforcement mechanism that is simple to audit: who can write where.

### Negative

- Three repositories per product is substantially more operational overhead — three sets of
  permissions, branch protections and pins to keep consistent.
- A change spanning specification, implementation and acceptance becomes three pull requests that
  must be coordinated, and partial merges are a new failure mode.
- Cross-repository addressing has to be threaded through bundle assembly, branch naming and pull
  request linking.
- Coarse holdout feedback makes holdout failures genuinely hard to diagnose, and some will need
  human investigation.

## Alternatives Considered

### One repository with path-based holdout protection

Rejected at the product tier: the content is present, so the guarantee depends on every mount plan
and every allowlist being correct forever. One error exposes it permanently.

### One repository, with holdouts added only at gate time

Inject holdouts into a clean checkout during the gate run. Workable, and close to what the gate
service does — but the acceptance tests still have to live somewhere, and somewhere the build zone
cannot read is a separate repository.

### Two repositories: specification and implementation, with holdouts in the specification repository

Rejected: the build zone must read specifications, so holdouts in that repository are readable by
the build zone.

### Three repositories at every tier

Rejected: at T0 and T1 the overhead buys a guarantee the risk does not require.

### Richer holdout feedback

Rejected: it defeats the purpose. A holdout the implementation can iterate against is a visible
test with extra steps.

## References

- `docs/spec/requirements.md` — REQ-SPLIT, REQ-ORACLE, REQ-VERDICT
- `docs/spec/overview.md` — product-tier repository layout
- `docs/spec/security.md` — holdout leakage through rework feedback
- `docs/spec/vocabulary.md` — Holdout, Trust Zone
- `brightsmiths/handbook` `docs/architecture/oracles.md`
