# CogWorks Architecture Specification

This folder contains the living architecture specification for CogWorks — a deterministic orchestration system for software and other engineering work, coordinated through a structured test-first pipeline.

## How to Read This Spec

Start with **overview.md** for the big picture, then **vocabulary.md** to understand the domain language. From there, the documents can be read in any order depending on what you need.

| Document | Purpose |
| ---------- | --------- |
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

## Source Requirements

Functional requirements are defined in [requirements.md](requirements.md), which catalogues ~160 requirements across 23 categories (PIPE, GRAPH, NODE, EDGE, EXEC, CLASS, ARCH, IFACE, PLAN, CODE, SCEN, REVIEW, INT, AUDIT, BOUND, DTU, EXT, XDOM, XVAL, CPACK, CONST, CTX, ALIGN) plus 6 tool-related categories (TOOL, PROFILE, ENFORCE, SKILL, DISC, ADAPT). Each `REQ-*` identifier in `assertions.md` traces to a corresponding entry in `requirements.md`.

## Key Capabilities

CogWorks provides advanced validation, context management, and extensibility capabilities:

1. **Scenario Validation** — Probabilistic behavior testing using scenarios held out from code generation context. Scenarios define desired behaviors as trajectories with satisfaction scores (default 0.95 threshold). This validates that code exhibits expected behaviors without overfitting to test cases.

2. **Digital Twin Universe** — High-fidelity behavioral clones of external dependencies, built using the same CogWorks pipeline. Twins enable testing against realistic external system behaviors without network calls or rate limits.

3. **Pyramid Summaries** — Multi-level context representation (L1: one-line, L2: paragraph, L3: full interface, L4: source) that enables efficient LLM context assembly. Dependencies further from the work scope get higher-level summaries; direct dependencies get full source.

4. **Domain Generalisation** — CogWorks is domain-agnostic across software and other engineering. It owns the generic test-first, implementation, validation, and iteration flow; domain-specific tests, toolchains, and validation remain behind the Extension API. The Rust domain service ships as a reference implementation.

5. **Cross-Domain Interface Registry** — A version-controlled, human-authored repository of interface contracts that span domains (CAN bus, power rails, mounting points, etc.). Enables deterministic validation that changes in one domain respect constraints from others.

6. **Extension API** — A protocol for external domain services to register with and be invoked by CogWorks. Supports Unix domain sockets (default) and HTTP/gRPC. Capabilities are discovered dynamically via handshake. Standardised diagnostic categories and error codes enable consumers to process results generically across domains. Any team can build a domain service without modifying CogWorks.

7. **Capability Profiles** — Machine-readable definitions of what a domain service for a specific engineering domain must provide (required methods, artifact types, interface types). Profiles are a development-time tool for domain service authors and a conformance-testing artifact — CogWorks does not enforce profiles at runtime.

8. **Context Pack System** — Structured, version-controlled domain knowledge packs loaded deterministically before generation begins. Packs contain domain knowledge, safe patterns, anti-patterns, and required artefact declarations. Selection is driven by work item classification labels and component tags, not by LLM inference. Multiple packs may load simultaneously; conflicting guidance resolves to the most restrictive rule.

9. **Constitutional Security Layer** — Non-overridable behavioral rules loaded as a privileged system prompt component on every pipeline run, before context assembly and before any LLM call. Prevents prompt injection, scope violations, and unauthorized capability generation. Injection detection halts the pipeline immediately; work items enter a hold state requiring human review.

10. **Tracing-Based Observability** — CogWorks records structured pipeline fields through `tracing`. The CLI configures JSON console logging and an OpenTelemetry OTLP exporter. External systems derive metrics and dashboards from available exported fields; CogWorks has no custom metric-sink abstraction or metrics storage.

11. **Tool Access Control** — Each pipeline node receives a scoped tool profile controlling which tools (filesystem, git, domain service, shell, network) it can access and with what constraints. Defence-in-depth enforcement operates at three independent layers: orchestrator filtering (unscoped tools never appear in LLM calls), tool-level scope validation (each tool validates parameters against constraints), and OS-level sandboxing for write-capable nodes. Only Code Generation has write access by default.

12. **Skill Crystallisation** — Proven tool invocation patterns are extracted from audit trails and crystallised as parameterised, deterministic scripts that replay without LLM involvement. Skills are human-reviewed, scope-enforced, and lifecycle-managed (Proposed → Reviewed → Active → Deprecated → Retired).

13. **Progressive Discovery** — When a node's tool count exceeds a threshold, the LLM receives a compact tool index with meta-tools for on-demand schema expansion, reducing context token consumption while maintaining full tool access.

14. **Adapter Generation** — CogWorks tool definitions are auto-generated from external API specifications (OpenAPI, EAB schemas) via a CLI command. Generated adapters participate in standard tool profile scoping and are version-controlled as derived artifacts with CI drift detection.

## Key Architectural Decisions

1. **CLI-first execution model** — each invocation is a stateless step function; service modes are additive wrappers
2. **GitHub as sole durable state** — no local database; pipeline state reconstructed from labels, issues, PRs
3. **Deterministic-first** — LLMs invoked only where reasoning/synthesis is genuinely required
4. **Result-based error handling** — business errors are values; exceptions only for unrecoverable infrastructure failures
5. **Domain-ignorant orchestrator** — CogWorks contains no domain-specific code; all domain operations delegated to external domain services via Extension API
6. **Domain services as external processes** — domain services run as separate binaries communicating over Unix sockets (default) or HTTP/gRPC; no built-in privileged path
7. **Clean architecture** — business logic depends only on abstractions; infrastructure implementations are swappable
8. **Cross-domain interface registry** — human-authored, version-controlled interface contracts enable deterministic cross-domain constraint validation
9. **Context Packs for domain knowledge** — structured, versioned domain knowledge loaded deterministically at Architecture node based on classification
10. **Constitutional security layer** — non-overridable rules loaded before any LLM call; injection detection halts pipeline; scope enforcement prevents unauthorized capabilities
11. **Tool profiles with defence in depth** — per-node tool scoping with three independent enforcement layers; generated adapters preferred over MCP servers (see ADR-0005)
