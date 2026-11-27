# ADR-0003: Pipeline State Trust and Stage Dispatch

**Status:** Accepted
**Date:** 2026-11-27
**Deciders:** Architecture

---

## Context

CogWorks is a step function. Each invocation reads GitHub, decides the next action, executes one
step, writes the result back, and exits. It holds nothing between invocations, so every run
rebuilds its view of where a work item has got to from GitHub alone.

That design puts weight on two things. First, the state CogWorks reads back must be state
CogWorks wrote — a run's position is reconstructed from issue comments, and an issue comment is
something anyone can post. Second, dispatching a stage must not require CogWorks to know the set
of stages in advance, because the stage graph and the stage types in it are data drawn from the
pinned harness, not a list compiled into the binary.

A third question falls out of resuming: the graph a run started under may not be the graph that
resolves on resume, if the repository's pin moved in between.

## Decision

### Decision 1 — Pipeline state is authenticated before it is trusted

State is transported as a structured hidden comment on the work item. A candidate comment is
trusted as pipeline state only if both hold:

- its author identity matches the identity CogWorks itself authenticates as, and
- the work item it names is the work item being processed.

A comment that merely parses as valid state JSON is not state. Without the author check, anyone
able to comment on an issue could set a run's position, its rework counters and its recorded
gate approvals.

This makes the configured GitHub identity load-bearing. CogWorks MUST run under a dedicated
application or bot identity. A credential shared with a person's interactive GitHub use would
make every comment that person writes a candidate for trusted pipeline state.

### Decision 2 — Stage dispatch goes through a seam, and stage types are validated at load

Dispatch is expressed as a trait that maps a node to its stage driver. Production code resolves
the node's declared stage type; tests substitute a double, so the orchestration logic — budget
checks, parallel dispatch, edge evaluation, state persistence — is testable without depending on
any particular stage driver existing.

Stage types are **validated against the pinned harness's stage-type registry when the graph is
loaded**, before any dispatch is attempted. A node declaring a stage type absent from the
registry fails graph validation.

Validating at load does not hardcode the orchestration layer against a fixed list, because the
registry is itself data, resolved from the harness. The check is "is this stage type one this
harness version defines", not "is this stage type one of the names we compiled in". The
orchestration layer still knows no stage type by name.

Dispatch remains defensive as a second line: a node that reaches dispatch with an unresolvable
stage type returns a non-retryable failure rather than panicking. A panic in a process that may
be handling several work items would take down unrelated runs.

### Decision 3 — Graph drift on resume escalates

Pipeline state records the hash of the resolved stage graph. On resume the hash is recomputed and
compared. A mismatch means the graph changed between the invocation that wrote the state and the
one resuming it — the repository's pin moved, or the harness content at that pin did.

A mismatch escalates. It does not halt, and it does not resume against the new graph.

Silently resuming would act on per-stage state — rework counters, gate approvals, attempt
history — that no longer corresponds to the graph's structure. That is a correctness problem, not
an inconvenience. But it is also not an unrecoverable fault: a human needs to decide whether to
restart the run or move the pin back. No stage executes when drift is detected.

### Decision 4 — Configuration carries the repository, and nothing that shapes an agent

CogWorks' own launch configuration carries the target repository and, optionally, a pin override
for operating against a harness version other than the one the repository pins.

It carries no stage graph, no stage contract, no profile, no prompt template, no output schema
and no constitutional rules. All of those resolve from the harness. A repository cannot
reconfigure the agents that work on it, and an operator cannot do so by editing a local file.

## Consequences

### Positive

- A run's position cannot be forged by anyone who can comment on an issue.
- Orchestration is fully testable without any stage driver being implemented.
- An unknown stage type is caught at load, with a clear error, rather than part-way through a run.
- Graph drift during a run is detected rather than silently acted upon.
- There is exactly one source of agent-shaping configuration, so there is nothing to reconcile.

### Negative

- CogWorks requires a dedicated bot identity. A deployment that shares a human credential is
  insecure in a way that is not locally obvious, so the requirement has to be documented where
  operators will read it.
- Load-time validation means a harness version that drops a stage type breaks every graph still
  referencing it, at load, for every repository pinned to it. That is the intended behaviour, but
  it makes harness compatibility a real constraint.
- Escalating on drift costs a human intervention in a case that is often benign.

## Alternatives Considered

### Trust any comment that parses as state

Rejected: it makes a run's position writable by anyone who can comment, including rework counters
and recorded approvals.

### Message authentication on the state comment

Sign the state comment and verify the signature, rather than checking authorship. Stronger, and
compatible with this decision, but not sufficient on its own — a signature proves integrity, not
that this state belongs to this work item. The author and work-item checks are the floor;
signing is a hardening step layered on top.

### Dispatch validation only, with no load-time check

Let an unknown stage type fail at dispatch. Rejected: the failure then arrives after the run has
already begun executing a graph that was never valid, and the error points at a stage rather than
at the graph.

### Resume against the newly resolved graph

Rejected: per-stage state from the old graph has no defined meaning against a new one.

### Halt on graph drift

Rejected: drift is recoverable by a human decision, which is what escalation is for.

## References

- `docs/spec/interfaces/nodes.md` — `run_step` sequence, `CliConfig`, dispatch seam
- `docs/spec/interfaces/pipeline-graph.md` — graph resolution and validation
- `docs/spec/security.md` — state reconstruction authenticity
- `docs/spec/requirements.md` — REQ-EXEC, REQ-STAGE, REQ-HARNESS
