# ADR-0006: A Node Is a Stage Type; Profiles and Stage Graphs Come From the Pinned Harness

**Status:** Accepted
**Date:** 2026-11-27
**Deciders:** Architecture

---

## Context

CogWorks runs a graph of nodes, and each node's work is done by an agent. Something has to decide
which agent — which persona, which skills, which tool allowlist, which output schema, which
writable paths, which budget and which identity.

Two properties constrain how that decision can be made.

The first is that an LLM is a component inside a deterministic machine, never the machine itself.
Routing, gating, context packaging, retries and escalation are code. Choosing which agent runs is
a routing decision, so it cannot be made by a model.

The second is that the thing that constrains an agent must not be editable by that agent, or by
the repository it is working in. An agent profile names the tools the agent may call and the paths
it may write; a stage graph names the stages a tier requires and the gates between them. If
either lived in the repository being worked on, an agent with write access there could widen its
own permissions or route around a stage its tier requires — and the first repositories CogWorks
operates on are control-plane repositories mapped to a product tier, including CogWorks' own.

CogWorks also has to stay discipline-neutral. It knows nothing about Rust, or firmware, or
mechanical design. Anything discipline-specific is therefore not CogWorks' to hold.

## Decision

### Decision 1 — A node declares a stage type; CogWorks contains no agent personas

A graph node declares a **stage type** — `specify`, `contract`, `plan`, `unit-oracle`,
`implement`, `refactor`, `acceptance-oracle`, `architect`, `review-semantic`, `review-security`,
`drift-scan`, `debt-fix`. The stage type names the kind of work, not who does it.

CogWorks contains no agent personas, no role prompts and no discipline-specific logic. The set of
stage types is data, resolved from the harness's **stage-type registry**, and no stage-type name
is compiled into CogWorks.

`NodeId` and stage type stay separate concepts. A node is an instance with a position in a graph;
a stage type is the kind of work that selects its profile. Several nodes in one graph may share a
stage type — a graph with three `implement` nodes, one per slice, has three node identities and
one stage type.

### Decision 2 — Profiles resolve deterministically from the pinned harness

For each stage, CogWorks resolves an **agent profile** from the harness, keyed by
`(stage type, discipline, language or toolchain, tier)`.

Resolution is deterministic: identical inputs at a given pin resolve to an identical profile. No
model participates, and no model output is an input. The resolved profile and the harness version
are recorded in the stage attestation, so the choice is evidence rather than an implementation
detail.

A profile carries its persona, skills, tool allowlist and **stage contract** — the input
allowlist, output schema, writable paths, network profile, budget, attempt and rework limits, and
identity role.

No matching profile is a **halt**, never a fallback to a default. Running a stage under an
approximately-right profile means an agent operating under a contract nobody chose for it.

### Decision 3 — Stage graphs come from the harness too, and there is no repository-local graph file

The stage graph governing a work item is resolved from the harness for its `(tier, discipline)`
pair. There is no repository-local pipeline configuration file, and no mechanism by which a target
repository supplies or overrides a graph.

The harness is versioned, pinned, reviewed and canary-rolled. A file in the repository being
worked on is none of those things, and is writable by the agents working there.

### Decision 4 — The pin is the unit of adoption

A target repository names a harness version in `.factory/pin.yaml`. Everything above resolves at
that version, and the resolved version is stable for the whole run.

A harness change therefore cannot alter a repository's behaviour until someone moves its pin. An
absent, malformed or withdrawn pin is a halt.

## Consequences

### Positive

- Which agent runs a stage is a deterministic function of recorded inputs, so a run is
  reproducible and an attestation is meaningful.
- The configuration that constrains an agent is outside the reach of that agent and of the
  repository it works in.
- CogWorks stays discipline-neutral: adding a discipline adds a pack to the harness and changes
  nothing here.
- Profiles and graphs roll out deliberately, per repository, by moving a pin.
- A graph cannot route around a stage its tier requires, because the tier selects the graph.

### Negative

- CogWorks cannot run at all without a reachable harness and a resolvable pin. The harness becomes
  a hard dependency on the critical path of every run.
- A repository with a genuine local need — an extra check, a different budget — cannot express it
  by editing a local file. It has to be expressed in the harness, for a tier or discipline, where
  it applies to everyone sharing that key.
- Harness and CogWorks versions can skew: a harness version may define a stage type a given
  CogWorks build cannot drive, or omit one a pinned graph still names.
- Halting rather than defaulting makes a missing profile a hard stop, which is the intended
  behaviour but is less forgiving during early bring-up.

## Alternatives Considered

### Roles built into the product

Compile the agent roles in, as named modules with fixed prompts. Rejected: it makes CogWorks
discipline-specific, puts prompt material in the same repository as the orchestration logic, and
means a prompt change requires releasing the orchestrator.

### Profiles in the target repository

Each repository carries its own profiles and graph. Rejected: an agent with write access to that
repository could widen its own tool allowlist or writable paths, and a repository could route
around a stage its tier requires.

### A model selects the profile

Let a model read the work item and choose the agent best suited to it. Rejected: it is a routing
decision, and routing is code. It would also make the selection unreproducible and the attestation
unverifiable.

### Repository-local overrides, constrained so they cannot weaken a graph

Allow a local file to add stages or tighten budgets but never to remove a required stage or change
a gate. Rejected: it reintroduces a second source of truth for graph structure and a constraint
system to police it, for a case the harness already handles per tier and discipline.

### Floating harness reference instead of a pin

Resolve the harness at a branch so improvements arrive immediately. Rejected: behaviour would
change under a repository without anyone choosing it, and a run could resolve two different
harness versions across two invocations.

## References

- `docs/spec/requirements.md` — REQ-STAGE, REQ-PROFSEL, REQ-HARNESS, REQ-GRAPH
- `docs/spec/architecture.md` — Stage-Type Registry, Profile Selection, Stage-Graph Resolution
- `docs/spec/interfaces/pipeline-graph.md` — graph resolution and registry validation
- `docs/spec/vocabulary.md` — Stage Type, Agent Profile, Harness, Harness Pin, Stage Contract
- `brightsmiths/handbook` `docs/architecture/agent-profiles.md`
