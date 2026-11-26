# ADR-0009: Assurance Review Produces Findings, Not Verdicts

**Status:** Accepted
**Date:** 2026-11-27
**Deciders:** Architecture

---

## Context

Some questions about a change cannot be answered by running it. Whether an abstraction is in the
right place, whether an error path is handled in a way the system can actually recover from,
whether a change widens an attack surface in a way no test would notice — these need judgement
over the diff rather than execution of it.

Model judgement is useful here and unreliable as an authority. It is inconsistent between runs,
sensitive to how the question is asked, and prone to confident findings that do not survive
scrutiny. A model that blocks a change on its own judgement blocks correct changes at a rate
nobody has measured, and the only available response is to re-run it until it agrees.

There is also a structural problem with letting review decide. A review stage is an agent stage,
run by CogWorks, under a profile CogWorks resolved. If its output were a verdict, CogWorks would
be deciding pass and fail through an agent it selected — exactly the self-certification that
having a separate gate service exists to prevent.

## Decision

Review is an **assurance** activity. A review stage produces structured findings and nothing else.

### Decision 1 — Review stages are read-only and emit findings

`review-semantic` and `review-security` run in a read-only trust zone. They write no files. Their
only output is a structured set of findings, each carrying a category, a severity, a location, and
the reasoning behind it.

A review stage has no writable paths in its stage contract, so it cannot modify the change it is
reviewing.

### Decision 2 — Whether a finding blocks is decided by the gate service

Findings are an input to a gate, not a decision. The gate service applies the tier's thresholds to
the findings and decides whether they block.

Severity on a finding is a claim about seriousness, not an instruction. A finding marked blocking
by a review stage does not block anything by itself.

### Decision 3 — Findings are attached where humans will read them

CogWorks posts findings as non-blocking review comments on the pull request, linked to the
locations they concern. This is additive: it informs the human reviewer rather than gating the
change.

### Decision 4 — No aggregation into a single decision inside CogWorks

CogWorks does not combine review results into a proceed, remediate or escalate decision. It
collects findings, attaches them, and reads the gate's verdict like any other verdict.

Where the gate fails a change on findings, the rework feedback carries the findings. Model-sourced
finding text is scanned before being reused in a prompt, since it is model output re-entering a
prompt and is therefore an injection surface.

## Consequences

### Positive

- A judgement that is inconsistent between runs cannot block a correct change on its own.
- Review scales to as many perspectives as are useful, because adding one adds findings rather
  than another thing that can veto.
- CogWorks cannot decide pass or fail through an agent it selected.
- Human reviewers get the findings as context, which is where model judgement is most valuable.

### Negative

- A genuine serious finding does not stop a change unless the gate's thresholds catch it. The
  thresholds therefore carry real weight and need to be set deliberately per tier.
- Findings that nobody acts on accumulate as noise on pull requests, and noisy review is ignored
  review.
- Splitting "produce the finding" from "decide whether it blocks" means two components must agree
  on the finding schema for the severity thresholds to mean anything.

## Alternatives Considered

### Review passes aggregate to a blocking decision inside CogWorks

Rejected: it is self-certification through an agent CogWorks selected, and it makes a model's
inconsistent judgement authoritative.

### Review findings block directly by severity

Rejected: severity is assigned by the same model that produced the finding, so "blocking" becomes
a label the reviewer can apply to its own output without any threshold having been agreed.

### Review stages fix what they find

Rejected: a reviewer that edits the change is no longer reviewing it, and the review loses the
independence that made it worth running.

### No automated review at all

Rejected: the findings are genuinely useful to human reviewers, and the cost of producing them is
low once they cannot block.

## References

- `docs/spec/requirements.md` — REQ-ASSURE, REQ-VERDICT, REQ-INT
- `docs/spec/vocabulary.md` — Review Result, Trust Zone
- `docs/spec/responsibilities.md` — Pull Request Manager
- `brightsmiths/handbook` `docs/architecture/workflow.md` — assurance gates
