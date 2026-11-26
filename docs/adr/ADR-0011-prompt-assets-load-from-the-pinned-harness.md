# ADR-0011: Prompt Templates, Output Schemas and Constitutional Rules Load From the Pinned Harness

**Status:** Accepted
**Date:** 2026-11-27
**Deciders:** Architecture

---

## Context

Three kinds of configuration shape what a model is asked and what it is allowed to answer: the
prompt templates that frame each stage, the output schemas that constrain the response, and the
constitutional rules that occupy the privileged position in every system prompt and cannot be
overridden.

All three are prompt material. All three are discipline-specific in practice — how you ask for a
Rust implementation is not how you ask for a mechanical design. And all three change on a
different cadence from the orchestrator: a prompt improvement should not require releasing the
component that runs the step function.

They also must not be editable by the agents they govern, or by the repository those agents are
working in. Constitutional rules in particular are the one thing in the system that no content and
no agent may override, which makes their provenance the whole of their value.

## Decision

Prompt templates, output schemas and constitutional rules live in the harness and resolve at the
version the target repository pins. CogWorks keeps the loading, verification and validation
mechanism, and holds none of the content.

### Decision 1 — The harness holds the content; CogWorks holds the mechanism

CogWorks resolves these assets through the harness loader at the pinned version, verifies the
resolution, and halts rather than proceed with assets it cannot verify. It contains no prompt
text, no schema definitions and no rule content of its own.

### Decision 2 — Output schema validation stays in CogWorks, and is not optional

CogWorks validates every stage's output against the output schema the harness declares for that
stage, before the pipeline proceeds. Structured input and output at every boundary is what makes
an LLM a component rather than the machine, so this check is core and remains here.

### Decision 3 — Template rendering survives for deterministic helper calls only

An agent turn happens inside a sandbox. CogWorks does not render that prompt, and does not see it.

Template rendering in CogWorks' own process covers the deterministic helper calls it still makes
itself — alignment verification, classification, failure interpretation. The rendering mechanism
is kept for those, scoped to them, and is not a vestige.

### Decision 4 — Constitutional rules resolve from the pin, and their position is preserved differently in each case

Rules are resolved from the harness, verified against the pin, and loaded unconditionally on every
run before any model call. No configuration option may disable this.

How their privileged position is preserved depends on where the call happens, and this is the
security property that changes shape:

- For a **deterministic helper call** in CogWorks' own process, the privileged position is
  enforced **at the type level**: a model call cannot be constructed without passing through
  constitutional assembly, so the guarantee holds at compile time.
- For an **agent turn**, CogWorks is not the process assembling the prompt, so that compile-time
  guarantee does not extend to it. The resolved profile places the rules, and the sandbox boundary
  preserves the placement.

What replaces the compile-time guarantee for agent turns is the combination of the pinned profile,
sandbox isolation, the mount plan the attempt runs under, and the gate service's independent
verdict. This is written down deliberately rather than left implicit: a compile-time guarantee was
traded for a set of runtime ones, and anyone reasoning about the system should know that.

### Decision 5 — Change control runs through the harness and the pin

Rules change by reviewed pull request in the harness, and a repository adopts the change by moving
its pin. A repository cannot change the rules governing its own runs.

## Consequences

### Positive

- Prompt material is versioned, reviewed and rolled out independently of orchestrator releases.
- A repository cannot edit the rules or schemas that constrain the agents working on it.
- Discipline-specific prompting lives with the discipline pack, keeping CogWorks neutral.
- Output schema validation stays at the boundary where structure is enforced.

### Negative

- A prompt fix requires a harness release and a pin move per repository, which is slower than
  editing a local file. That is the intended trade and it is still a real cost.
- The compile-time constitutional guarantee no longer covers agent turns, which are the majority
  of model calls. The replacement is a set of runtime properties in other services, so the
  guarantee is now distributed rather than local.
- CogWorks cannot run without a reachable harness, including for the deterministic helper calls
  that would otherwise need nothing external.

## Alternatives Considered

### Keep prompt assets in CogWorks' own configuration

Rejected: it makes the orchestrator discipline-specific, couples prompt changes to orchestrator
releases, and puts the rules that govern agents in the repository those agents work on when
CogWorks builds itself.

### Keep prompt assets in each target repository

Rejected: prompt content an agent with write access to that repository could edit, including the
constitutional rules meant to constrain it.

### Render agent prompts in CogWorks and pass the rendered text to the sandbox

This would preserve the compile-time guarantee. Rejected: it would require CogWorks to know each
profile's prompting in detail, re-centralising exactly what the harness exists to hold, and would
make every prompt change an orchestrator change.

### Resolve the harness at a branch rather than a pin

Rejected: rule and prompt changes would take effect under a repository without anyone choosing
them, and two invocations of one run could resolve different content.

## References

- `docs/spec/requirements.md` — REQ-HARNESS, REQ-CONST, REQ-STAGE
- `docs/spec/interfaces/nodes.md` — LLM gateway, constitutional assembly
- `docs/spec/interfaces/security.md` — constitutional rules provenance
- `docs/spec/constraints.md` — Constitutional Layer Constraints
- `docs/spec/security.md` — loss of the compile-time constitutional guarantee for agent turns
