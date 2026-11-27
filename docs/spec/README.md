# CogWorks Architecture Specification

This folder contains the living architecture specification for CogWorks — a deterministic
foreman for software and other engineering work, running a graph of agent stages over GitHub
state.

CogWorks is one service in a larger system. The system-level architecture lives in the
[`brightsmiths/handbook`](https://github.com/brightsmiths/handbook) repository under `docs/`. This
specification describes what CogWorks itself does and the boundaries it holds; it does not restate
the system design.

## How to Read This Spec

Start with **overview.md** for the big picture, then **vocabulary.md** for the domain language.
From there the documents can be read in any order.

| Document | Purpose |
|----------|---------|
| [overview.md](overview.md) | System context, scope, design philosophy, high-level data flow |
| [vocabulary.md](vocabulary.md) | Domain concepts with precise definitions and constraints |
| [requirements.md](requirements.md) | Functional requirements (REQ-* IDs) — what the system must do |
| [responsibilities.md](responsibilities.md) | CRC-style responsibility breakdown for each component |
| [architecture.md](architecture.md) | Clean architecture boundaries — business logic, abstractions, infrastructure |
| [assertions.md](assertions.md) | Behavioral assertions (Given/When/Then) for key system behaviors |
| [constraints.md](constraints.md) | Type system, module boundary, error handling, testing, and performance rules |
| [tradeoffs.md](tradeoffs.md) | Design alternatives evaluated with pros/cons |
| [testing.md](testing.md) | Strategy for testing CogWorks itself |
| [security.md](security.md) | Threat model and mitigations |
| [edge-cases.md](edge-cases.md) | Non-standard flows and failure modes |
| [operations.md](operations.md) | Deployment model, monitoring, cost alerting, runbook |
| [risk-register.md](risk-register.md) | Risk catalog with mitigations and residual risk assessment |
| [shared-registry.md](shared-registry.md) | Catalog of reusable types, traits and patterns |
| [interfaces/](interfaces/) | Concrete interface specifications per module |

## Increment Levels

Requirements and decisions carry a level marker giving the increment at which they come into
force. Levels correspond to the increments of CogWorks' role in the wider factory, described in
the handbook under `docs/roadmap.md`.

| Level | Scope |
| ----- | ----- |
| **L1** | First dark slice: the `unit-oracle` and `implement` stage types on one internal-tier repository, driven from the CLI |
| **L2** | Event-driven runs; the `specify`, `contract` and `plan` stage types; sub-issue creation; tier derivation; signed attestations |
| **L3** | Consumption of automated intake: upstream attestations verified and their machine-readable blocks consumed |
| **L4** | Product-tier support: `architect`, `acceptance-oracle`, `refactor` and the assurance stages; holdouts; the repository split |

A requirement at a later level is specified now and implemented when that level is built, so the
later increments are not lost while the earlier ones are built.

## Source Requirements

Functional requirements are defined in [requirements.md](requirements.md). Each `REQ-*` identifier
referenced anywhere in this specification traces to an entry there. A retired requirement ID is
never reused: old IDs appear in commit messages, test names and issue history, and reassigning one
would silently change what that history refers to.

## Key Capabilities

1. **Stage Types and Agent Profiles** — A graph node declares a stage type. For each stage,
   CogWorks deterministically resolves an agent profile from the pinned harness, keyed by
   `(stage type, discipline, language or toolchain, tier)`. No model participates in the
   selection, and the choice is recorded in the stage attestation.

2. **Harness Pinning** — Stage graphs, stage contracts, agent profiles, prompt templates, output
   schemas, Context Packs and constitutional rules all resolve from the harness version a target
   repository pins. A repository cannot supply the configuration that governs the agents working
   on it. A missing, malformed or unverifiable pin halts the run rather than defaulting.

3. **Context Bundles** — Each stage attempt consumes an explicitly allowlisted, hashed set of
   inputs. An input the allowlist does not name is absent, not merely ranked lower. The bundle
   hash is recorded before the attempt starts, which makes an attempt reproducible and an
   attestation meaningful.

4. **Oracle Separation** — The tests that judge an implementation are written by a different
   stage, under a different identity, in a different sandbox, and sealed by a signed manifest
   before implementation begins. The implementation identity cannot write the locked-test paths,
   enforced outside CogWorks.

5. **Verdicts from the Gate Service** — CogWorks never computes a pass or fail and never authors a
   check run. It reads check runs on an attempt's commit SHA, considering only those authored by
   the gate service identity. A pending or absent verdict means wait, never assume.

6. **Delegated Execution** — Stage attempts run in fresh sandboxes owned by a separate stage
   runner service, one per attempt. CogWorks holds no virtualisation privilege, no host privilege
   and no push credential.

7. **Tier Derivation** — A work item's tier is derived from path globs in a human-owned
   `risk-map.yaml`, raise-only. A tier is never read from a label, so a label edit cannot reduce
   the rigour applied to a piece of work.

8. **Bounded Rework and Escalation** — Retry, rework, escalate and halt are distinguished by an
   explicit rule, with limits from the stage contract. A repeating failure class escalates early
   rather than consuming the remaining budget, and escalation produces a dossier so a human
   inherits the context.

9. **Stage Attestations** — Each attempt records what ran under which profile, at which harness
   version, over which bundle, and which verdicts were read. Attestations are evidence, never
   workflow state: a run's position is always reconstructable from GitHub without reading one.

10. **Constitutional Security Layer** — Non-overridable rules resolve from the pinned harness and
    load unconditionally before any model call. For deterministic helper calls made in CogWorks'
    own process, their privileged position is enforced at the type level.

11. **Context Packs** — Structured domain knowledge from the harness discipline pack, selected
    deterministically by tier, discipline, stage type and toolchain, and reaching an agent only
    through its context bundle.

12. **Extension API** — A protocol for external domain services to register with and be invoked
    by CogWorks over Unix sockets (default) or HTTP. Capabilities are discovered dynamically via
    handshake, so a declaration cannot drift from what a service implements.

13. **Cross-Domain Interface Registry** — Human-authored, version-controlled interface contracts
    spanning domains, enabling deterministic constraint validation across them.

14. **Tracing-Based Observability** — Structured fields emitted through the OpenTelemetry SDK
    exporter. No custom metric sink and no pluggable metrics backend.

## Key Architectural Decisions

1. **A node is a stage type** — CogWorks contains no agent personas, role prompts or
   discipline-specific logic; profiles and stage graphs resolve from the pinned harness
2. **Oracle and implementation are separate stages, identities and sandboxes** — the tests that
   judge an implementation cannot be shaped by whatever produced it
3. **The gate service is the sole source of pass or fail** — no self-certification; CogWorks
   authors no check run and computes no verdict
4. **Assurance review produces findings, not verdicts** — a model's inconsistent judgement cannot
   block a correct change on its own
5. **Allowlisted, hashed context bundles** — the input set fails closed, and what an agent saw is
   recorded
6. **CLI-first execution model** — each invocation is a stateless step function; service modes are
   additive wrappers
7. **GitHub as sole durable state** — no local database; a run's position is reconstructed from
   issues, labels, comments, pull requests and check runs
8. **Deterministic-first** — models are invoked only where reasoning or synthesis is genuinely
   required; routing and gating never are
9. **Provider-neutral stage runtime** — one fresh sandbox per attempt through a provider-neutral
   interface, with no cloud-specific dependency
10. **Clean architecture** — business logic depends only on abstractions; infrastructure
    implementations are swappable
11. **Domain-ignorant orchestrator** — all domain operations delegate to external domain services
    through the Extension API, with no built-in privileged path
12. **Result-based error handling** — business errors are values; exceptions only for
    unrecoverable infrastructure failures

See [`docs/adr/`](../adr/) for the decision records, each carrying its context, consequences and
the alternatives rejected.

## A note on two standards documents

[`docs/standards/rust-embedded-safety.md`](../standards/rust-embedded-safety.md) and
[`docs/standards/iso9001-traceability.md`](../standards/iso9001-traceability.md) are
discipline-specific guidance living in a deliberately discipline-neutral service. They belong in
the harness's software discipline pack and move there once that repository exists. They remain
here in the interim rather than being lost.

[`docs/standards/code.md`](../standards/code.md) and
[`docs/standards/testing.md`](../standards/testing.md) describe how CogWorks itself is built and
stay.
