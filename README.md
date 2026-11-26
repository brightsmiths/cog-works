# cog_works

A deterministic foreman for software delivery. CogWorks is a step function over GitHub state: it
reads where a work item has got to, decides the next action, executes one step, writes the result
back, and exits.

It runs a graph of stages. For each stage it deterministically selects an agent profile from a
pinned harness, builds the exact set of inputs that stage is allowed to see, and asks a separate
service to run the attempt in a fresh sandbox. Whether the work passes is decided by a separate
gate service, which CogWorks reads and never overrules.

The governing rule: an LLM is a component inside a deterministic machine, never the machine
itself. Routing, gating, context packaging, retries and escalation are code.

## What CogWorks does not do

These are boundaries, not gaps:

- **It does not run an agent.** Every agent turn happens in a fresh microVM owned by a separate
  stage runner service. CogWorks holds no virtualisation or host privilege.
- **It does not decide whether work passes.** Verdicts are computed by a separate gate service
  from a commit SHA, in a clean sandbox, under its own identity. CogWorks reads its check runs
  and authors none.
- **It does not push commits.** Commits produced by a stage attempt are pushed by the stage
  runner under that stage's identity role. CogWorks opens and updates pull requests; it does not
  push and does not merge.
- **It holds no push token and no model API key.** Model credentials live in a model gateway.
- **It contains no agent personas and no discipline-specific logic.** Those live in the harness
  and its discipline packs.
- **It does not decide what to work on next.** Ordering work across issues belongs to a separate
  service.

## Status

Under active development, pre-release. The pipeline executor exists; the CLI composition root,
the GitHub and model-gateway adapters, and the stage drivers are in progress. There is no
end-user command to run yet.

CogWorks depends on three services that do not yet exist: the stage runner, the gate service and
the write-scope service. Until they do, humans stand in for them, producing the same observable
GitHub state a real service would.

## Documentation

The architecture specification lives in [`docs/spec/`](docs/spec/) — start with
[`overview.md`](docs/spec/overview.md), then [`vocabulary.md`](docs/spec/vocabulary.md).
Architecture decisions are in [`docs/adr/`](docs/adr/).

CogWorks is one service in a larger system. The system-level architecture — how the services fit
together, the agent profiles, the gates, the trust zones and the roadmap — lives in the
[`brightsmiths/handbook`](https://github.com/brightsmiths/handbook) repository under `docs/`.
