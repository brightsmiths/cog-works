# System Overview

## What CogWorks Is

CogWorks is the **deterministic foreman**. It is a stateless step function: read GitHub state,
decide the next action, execute one step, write results to GitHub, exit.

It runs a graph of **stages**. For each stage it deterministically resolves an agent profile from
a pinned harness, assembles the exact set of inputs that stage is permitted to see, and asks a
separate service to run the attempt in a fresh sandbox. Whether the resulting work passes is
decided by a separate gate service, whose check runs CogWorks reads and never overrules.

The governing rule, which this whole design exists to serve: **an LLM is a component inside a
deterministic machine, never the machine itself.** Routing, gating, context packaging, retries
and escalation are code.

CogWorks is one service in a larger system. The system-level architecture lives in the
[`brightsmiths/handbook`](https://github.com/brightsmiths/handbook) repository under `docs/`.

## What CogWorks Does Not Do

Three capabilities are deliberately outside this service. They are boundaries, not gaps, and each
is held by a component with no stake in CogWorks' decisions:

| Not this | Whose it is | Why not here |
|----------|-------------|--------------|
| Running an agent | The stage runner service | Isolation needs virtualisation privilege. The component deciding what to run should not hold the privilege to run anything. |
| Deciding pass or fail | The gate service | A verdict computed by the component whose run it concludes is self-certification. |
| Pushing commits | The stage runner, under each stage's identity role | A push credential here would let one flaw write code to every repository. |

CogWorks also holds no model API key, decides no ordering across work items, and never merges a
pull request or cuts a release.

What it does is decide what runs next, decide what each agent may see and touch, and record what
happened.

## Design Philosophy

These principles are load-bearing — they constrain every architectural decision:

1. **Deterministic by default.** Every step that can be performed by conventional tooling MUST
   be. Models are invoked only for reasoning, synthesis, or natural language understanding — and
   never for routing, gating or selecting a profile. This controls cost and makes the system
   debuggable.

2. **Quality through an independent oracle.** The tests that judge an implementation are written
   by a different stage, under a different identity, in a different sandbox, and sealed before
   implementation begins. One implementation with maximum context, iterating on structured
   feedback from gates it cannot influence.

3. **Structured I/O at every boundary.** Every model call has a defined input and output schema,
   declared by the harness. Output is validated before the pipeline proceeds.

4. **Stateless and observable.** All durable state lives in GitHub. No hidden databases. A run's
   position is fully reconstructable by reading GitHub.

5. **Agent-shaping configuration comes from outside the repository being worked on.** Profiles,
   contracts, graphs, prompts, schemas and rules resolve from a pinned harness. A repository
   cannot reconfigure the agents working on it — which matters because CogWorks operates on its
   own repository.

6. **Constitutional boundaries.** External content is data, never instructions. Non-overridable
   rules load before any model call. The boundary between trusted instructions and untrusted
   input is explicit and enforced.

7. **Fail closed, never default.** A missing pin, an unverifiable harness, a malformed risk map,
   an unresolvable profile, a missing stage graph: each halts. Substituting a default would run
   agents under configuration nobody chose, and the failure would be invisible in a passing run.

## Execution Model: CLI-First

CogWorks is a **CLI-first** application. Each invocation is a stateless step function:

```
Read GitHub state → Determine next action → Execute one step → Write results → Exit
```

This design choice has several consequences:

- **Simplicity**: No connection pooling, no graceful shutdown, no in-memory state management
- **Testability**: Each invocation is independent and deterministic given the same GitHub state
- **Crash recovery**: Restart and re-invoke; the system reads GitHub state and picks up where it
  left off
- **Concurrency**: Multiple invocations can run for different work items simultaneously
- **Asynchronous stages**: A stage attempt and its verdict both complete outside this process, so
  an invocation that finds either in flight returns a waiting result rather than blocking

### Trigger Modes

| Mode | Level | Description |
|------|-------|-------------|
| **CLI** | L1 | Direct invocation: `cogworks run --issue <number>` |
| **Queue** | L2 | Normalised events from the factory's event queue invoke the step function |

CogWorks does not receive GitHub webhooks. Webhook ingress, HMAC verification and normalisation
belong to the factory's ingress service, which routes normalised events onto durable queues.
There is one place in the factory where an inbound signature is checked, and it is not here.

A run starts only on the human-only trigger label. An event from an external actor that would
start agent work is ignored.

**An event is a prompt to read GitHub, never a source of truth.** Every decision is made from
GitHub state re-read at the start of the step, so a lost, duplicated or out-of-order event can
only delay or repeat a re-read — it cannot corrupt a run.

### Service-Ready Boundaries

To ensure CLI-first doesn't block future service mode:

- **Component construction is explicit** — dependencies are injected via constructors, not
  resolved from global state
- **No global mutable state** — all state flows through function parameters and return values
- **Resource cleanup is explicit** — cleanup is requested by the caller on every terminal path,
  not by `Drop` side effects that assume a single-invocation lifetime
- **Configuration is loaded once and passed** — not re-read on every operation

## System Context

```
┌──────────────────────────────────────────────────────────────────────────────┐
│  The Harness (pinned, read-only, not writable by CogWorks or any agent)      │
│  stage graphs per (tier, discipline) · agent profiles · stage contracts      │
│  prompt templates · output schemas · Context Packs · constitutional rules    │
└────────────────────────────────┬─────────────────────────────────────────────┘
                                 │ resolved at .factory/pin.yaml
┌──────────────┐          ┌──────▼────────────┐
│   Human      │  creates │   GitHub          │
│   Developer  │────────→ │   Issue           │
│              │  labels  │   (Work Item)     │
└──────────────┘  trigger └────────┬──────────┘
                                   │ CLI / queue event
                                   ▼
                          ┌───────────────────┐
                          │   CogWorks        │
                          │   (Step Function) │
                          │                   │
                          │  Resolves profile │
                          │  Builds bundle    │
                          │  Reads verdicts   │
                          │  Writes state     │
                          └──┬───┬───┬───┬──┬─┘
          ┌──────────────────┘   │   │   │  └──────────────────┐
          ▼                      ▼   │   ▼                     ▼
┌───────────────────┐ ┌───────────────┴──┐ ┌──────────────┐ ┌──────────────────┐
│  Stage Runner     │ │  GitHub API      │ │ Write-Scope  │ │ Domain Services  │
│  one fresh micro- │ │  issues · labels │ │ Service      │ │ (External Procs) │
│  VM per attempt   │ │  comments · PRs  │ │ mount plans  │ │  Extension API   │
│  pushes commits   │ │  check runs (r)  │ │ scope checks │ │  (socket / HTTP) │
└────────┬──────────┘ └──────────────────┘ └──────────────┘ └──────────────────┘
         │ agent turn happens HERE, not in CogWorks
         ▼
┌───────────────────┐        ┌──────────────────────────────────────────────────┐
│  Model Gateway    │        │  Gate Service                                    │
│  holds provider   │        │  recomputes every verdict from a commit SHA in   │
│  API keys         │        │  a clean sandbox, under its own identity;        │
│  enforces limits  │        │  the ONLY source of pass or fail                 │
└───────────────────┘        └────────────────────┬─────────────────────────────┘
                                                  │ publishes check runs
                                                  ▼  (CogWorks reads, never writes)
┌──────────────────────────────────────────────────────────────────────────────┐
│  The target repository supplies exactly two things, both human-owned:        │
│  .factory/pin.yaml  (which harness version governs this repository)          │
│  risk-map.yaml      (path globs → tiers, from which the tier is derived)     │
└──────────────────────────────────────────────────────────────────────────────┘
```

Note what the target repository does **not** supply: no stage graph, no profiles, no prompt
templates, no tool allowlists, no constitutional rules. An agent working in a repository could
edit anything stored there, and the agents CogWorks runs are exactly the ones that configuration
is meant to constrain.

Domain services are external processes reached through the Extension API. CogWorks is
domain-ignorant and contains code for no specific domain.

## Pipeline Flow

The stage graph is resolved from the pinned harness for the run's `(tier, discipline)` pair. The
full stage set and the graphs per tier are described in the handbook under
`docs/architecture/workflow.md`; what follows is the L1 slice.

### The L1 slice

```
┌──────────┐    ┌──────────────┐    ┌────────────┐              ┌──────────┐
│  intake  │───→│ unit-oracle  │───→│  implement │─────────────→│   pull   │
│          │    │              │    │            │              │  request │
│ tier     │    │ locked tests │    │ src/ and   │              │          │
│ scope    │    │ + sealed     │    │ own tests  │              │ opened/  │
│ attests  │    │   manifest   │    │            │              │ updated  │
└──────────┘    └──────┬───────┘    └─────┬──────┘              └──────────┘
                       │                  │
                 oracle identity    builder identity
                 writes tests/      writes src/; locked
                 locked/            tests are READ-ONLY
                       │                  │
                       ▼                  ▼
                 ┌─────────────────────────────────┐
                 │  Gate service verdict on the    │
                 │  attempt's commit SHA           │
                 └──────────┬──────────────────────┘
                            │ fails
                            ▼
                 ┌─────────────────────────────────┐
                 │ rework: structured feedback,    │
                 │ a NEW sandbox, bounded by the   │
                 │ stage contract's limits         │
                 └─────────────────────────────────┘
```

Two stages, two identities, two sandboxes. That separation is the most important property of the
pipeline: the thing being measured cannot shape what it is measured against.

There is no node that runs checks inside CogWorks. All checks are the gate service's, recomputed
from a commit SHA under its own identity.

Each node gate is configurable `auto-proceed` or `human-gated`, and the derived tier can force
human gates regardless of configuration.

### Graph capabilities

A resolved stage graph may use:

- **Parallel fan-out**: multiple nodes executing concurrently when their inputs are all available
- **Fan-in synchronisation**: a node waiting for all upstream parallel nodes to complete
- **Conditional edges**: deterministic expressions over pipeline state and verdicts, or boolean
  composites. **No edge condition is evaluated by a model** — which edge is taken never depends on
  model output, because routing is code
- **Rework loops**: cycles with explicit termination conditions (maximum traversals, cost budget)

Several nodes may share a stage type: a graph with three `implement` nodes, one per slice, has
three node identities and one stage type.

## Data Flow Across Stages

| Stage | Input | Output | Storage | Level |
|-------|-------|--------|---------|-------|
| intake | Work item, risk map, upstream attestations | Derived tier, scope finding, traceability matrix | Issue comment + labels | L1 |
| `specify` | Work item + repo context | Intent and acceptance criteria with REQ-IDs | Specification pull request | L2 |
| `architect` | Approved intent | Decision record, threat-model delta, risk-map patch | Specification pull request | L4 |
| `contract` | Approved specification | Compilable stubs, program design | Specification pull request | L2 |
| `plan` | Approved contracts | `plan.yaml`: slices, sizes, REQ mapping, dependency DAG | Specification PR, then sub-issues in DAG order | L2 |
| `unit-oracle` | Slice + contracts + specification | Locked tests + signed manifest | Branch, pushed under the oracle identity | L1 |
| `implement` | Slice + contracts + locked tests (read-only) | Implementation and its own tests | Branch, pushed under the builder identity | L1 |
| `acceptance-oracle` | Approved specification | Visible and holdout acceptance sets | Acceptance repository | L4 |
| `review-semantic` / `review-security` | The change under review | Structured findings only — no verdict, no file changes | Pull request comments | L4 |
| Pull request | Branch pushed by the runner | Pull request opened or updated | GitHub PR | L1 |

Stage inputs are not merely declared — they are **allowlisted** by the resolved stage contract and
hashed. An input the allowlist does not name is absent from the bundle, which is what makes the
trust-zone barriers structural rather than a list of exclusions to maintain.

## Working Copy Management

CogWorks keeps **no working copy**. It reads what it needs through the GitHub Contents API:
individual files, directory listings, configuration. Those reads are lightweight and
API-based.

Full toolchain operations need real files on disk, and they happen where the files are:

- **Inside a stage attempt's sandbox.** The stage runner creates one fresh sandbox per attempt,
  checks out what the mount plan permits, runs the attempt, and pushes the resulting commits under
  the stage's identity role. The sandbox is torn down afterwards and is never a state store.
- **Inside the gate service's sandbox.** Verdicts are recomputed there from a commit SHA, in a
  clean environment CogWorks does not control.
- **Behind the Domain Service Client.** Domain services manage their own working copies for the
  toolchain operations they own.

Durable outputs — specifications, contracts, plans, implementations and pipeline state — are
persisted through the repository and work-item interfaces. Sandbox-local files are never a source
of durable recovery data.

**Branch scheme**: `cogworks/<work-item-number>/<stage>` for a stage's own branch, and
`cogworks/<work-item-number>/slice-<n>` for a slice. For example `cogworks/42/spec`,
`cogworks/42/contract`, `cogworks/42/slice-3`.

CogWorks does not merge changes, own releases, or publish artifacts. External approvals are
represented through the work-item interface and remain distinct from CogWorks' configurable node
gates and the tier's human gates.
