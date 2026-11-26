# Interface Design — Specification Index

This directory contains the canonical interface specifications for CogWorks.
Every source-code stub in the workspace has a corresponding entry here.

These specifications are the contract. Stubs enforce compile-time correctness;
the specifications describe **why** and **what** the types and traits mean.

---

## Workspace Structure

```
Cargo.toml                       (workspace root — 7 members)
crates/
  pipeline/                      # Domain types + business logic + all trait definitions
  nodes/                         # Stage drivers + LLM gateway + PipelineExecutor
  github/                        # GitHub adapter (github-bot-sdk)
  llm/                           # Model gateway client
  extension-api/                 # Domain service client (Extension API protocol)
  listener/                      # Event queue consumer
  cli/                           # Entry point — composition root + observability wiring
docs/spec/interfaces/
  README.md                      — this file
  shared-types.md                — domain identifiers, value types, error types
  pipeline-graph.md              — stage graph model and runtime state
  github-traits.md               — GitHub traits, check-run reader, event source
  domain-traits.md               — domain service traits and routing
  security.md                    — constitutional layer, injection detection, trust zones
  context.md                     — context bundle assembly, pack loading, label parsing
  pipeline-execution.md          — state machine, budget, tier derivation, alignment, traceability
  observability.md               — span and metric emission
  nodes.md                       — LLM gateway + stage drivers + PipelineExecutor
  infrastructure.md              — concrete infrastructure implementation contracts
```

---

## Crate Dependency Graph

```
              ┌──────────────────────────────────────────────────────────────────┐
              │                         pipeline                                 │
              │   domain types · business logic · all trait definitions          │
              └──────────────────┬───────────────────────────────────────────────┘
                                 │ depended on by all crates below
         ┌───────────────────────┼─────────────────────────────────────────────┐
         │                       │                                             │
    ┌────▼───────┐   ┌───────────▼──────────────────────────────────────────┐  │
    │   nodes    │   │           infrastructure crates                      │  │
    │            │   │  github · llm · extension-api · listener             │  │
    └────────────┘   └──────────────────────────────────────────────────────┘  │
         │                                    │                                │
         └───────────────────────┬────────────┘                                │
                                 │                                             │
                            ┌────▼─────────────────────────────────────────────▼┐
                            │                      cli                          │
                            │   composition root · observability · trigger mode │
                            └───────────────────────────────────────────────────┘
```

**Architectural rules:**

- `pipeline` has **no I/O dependencies** (no tokio, reqwest, std::fs, std::process).
- Infrastructure crates implement `pipeline` traits. They do not add domain rules.
- `nodes` orchestrates calls between `pipeline` logic and infrastructure traits.
  It contains no prompt text, no agent personas and no discipline-specific logic:
  those resolve from the pinned harness at run time.
- `cli` is the only crate that imports all others and constructs concrete instances.

---

## Hexagonal Architecture Map

| Component | Layer | Purpose |
|-----------|-------|---------|
| `pipeline` (types, business logic) | Core domain | Domain rules, computations, type definitions |
| `pipeline` (trait definitions) | Port | Abstractions the domain needs from external systems |
| `github`, `llm`, `extension-api`, `listener` | Adapter | Implements domain ports against real APIs |
| `nodes` | Application/orchestration | Sequences port calls for each stage |
| `cli` | Composition root | Wires ports to adapters; configures observability |

---

## Shared Type Registry

See [`docs/spec/shared-registry.md`](../shared-registry.md) for the live catalog
of every reusable type, trait, and pattern with its source location.

---

## Three capabilities deliberately absent

No interface in this directory lets CogWorks execute an agent, decide whether an
artifact passes, or push a commit. Those belong to the stage runner, the gate
service, and the stage runner's identity roles respectively.

If a proposed interface change would add any of them, it is the change that is
wrong, not the boundary. See `docs/spec/architecture.md` §Boundary Rules.
