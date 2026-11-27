# Domain Vocabulary

This document defines the precise meaning of every domain concept in CogWorks. These definitions establish the language that all subsequent design, implementation, and documentation must use consistently.

---

## Factory Concepts

CogWorks is one service in a larger system. These are the concepts that cross its boundary. The
system-level definitions live in the [`brightsmiths/handbook`](https://github.com/brightsmiths/handbook)
repository under `docs/`; what follows is how each appears to CogWorks.

### Stage Type

The kind of work a graph node does, named rather than described: `specify`, `architect`,
`contract`, `plan`, `unit-oracle`, `implement`, `refactor`, `acceptance-oracle`,
`review-semantic`, `review-security`, `drift-scan`, `debt-fix`.

- Source: the stage-type registry in the resolved harness. No stage-type name is compiled into
  CogWorks
- Validated: every node's stage type is checked against the registry when the graph is loaded,
  before any dispatch
- Distinct from a node: a node is an instance with a position in the graph; a stage type is the
  kind of work that selects its profile. Several nodes may share one stage type
- Not a persona: a stage type names work, not a character. The persona is in the profile

### Stage Attempt

One execution of one stage, in one sandbox, producing one outcome.

- Granularity: a retry is a **new attempt**, with a new sandbox and a new bundle hash
- Bounded by: the attempt and rework limits its stage contract declares
- Observed: through GitHub state — the expected branch carries its commits and its output record
  is present
- Never reused: no sandbox, no working copy and no cached artifact carries from one attempt to the
  next except through GitHub

### Agent Profile

Everything that determines what an agent is for one stage: persona, skills, tool allowlist,
identity role, and the stage contract.

- Source: the harness, at the version the target repository pins
- Resolved by: `(stage type, discipline, language or toolchain, tier)`, deterministically
- Recorded: in the stage attestation, so the choice is evidence rather than an implementation
  detail
- Not selected by a model: selection is a routing decision, and routing is code
- No fallback: no matching profile halts the run. An approximately-right profile means an agent
  operating under a contract nobody chose for it

### Stage Contract

The machine-readable terms a stage attempt runs under.

- Contains: input allowlist, output schema, writable paths, network profile, token and cost
  budget, attempt and rework limits, identity role
- Part of: the agent profile
- **Never visible to the agent it governs.** An agent that can read its own allowlist, limits and
  checked paths can reason about them; the constraints are for the machine

### Harness

The repository holding everything that shapes an agent: stage graphs per `(tier, discipline)`,
agent profiles, stage contracts, prompt templates, output schemas, Context Packs and
constitutional rules.

- Read-only to CogWorks. There is no write path, and a harness source configured with write
  credentials is a misconfiguration
- Not writable by any agent or any repository CogWorks operates on — which is the point: the
  agents it configures are exactly the ones it must constrain
- Versioned, reviewed and canary-rolled, and treated at product tier

### Harness Pin

The harness version a target repository is governed by, recorded in `.factory/pin.yaml`.

- Human-owned, and a protected path: no mount plan makes it writable
- The unit of adoption: a harness change cannot alter a repository's behaviour until someone moves
  its pin
- Verified: resolved content is checked against what the pin names, and a mismatch halts
- Stable within a run: the resolved version is fixed for the whole run, so content cannot change
  underneath it
- Absent, malformed or naming a withdrawn version: a halt, never a default

### Discipline and Discipline Pack

A **discipline** is a field of engineering work — software, firmware, electrical. A **discipline
pack** is the part of the harness carrying that discipline's profiles, graphs, Context Packs and
checks.

CogWorks is discipline-neutral: it contains code for no specific discipline, and adding one adds a
pack to the harness rather than changing this service.

### Trust Zone

Which class of work a stage does, and therefore what it may see and where it may write.

| Zone | Stage types | Writes | Must not see |
|------|-------------|--------|--------------|
| Design | `specify`, `architect`, `contract`, `plan` | specification paths only | implementation source |
| Oracle | `unit-oracle`, `acceptance-oracle` | locked test paths | implementation source |
| Build | `implement`, `refactor`, `debt-fix` | `src/` and its own tests | holdout content, acceptance source |
| Review | `review-semantic`, `review-security` | nothing | — |

The oracle barrier matters as much as the build barrier and is easier to forget: an oracle that
can see the implementation writes tests describing what was built rather than what was asked for.

### Identity Role

The identity a stage attempt's commits are pushed under.

- Held by: the stage runner, never by CogWorks
- Declared by: the stage contract
- Distinct per zone: the oracle and build zones MUST run under different roles, which is what
  makes the oracle unforgeable by its author

### Context Bundle and Bundle Hash

The **context bundle** is the allowlisted, hashed set of inputs one stage attempt may see. The
**bundle hash** is a SHA-256 over its canonical serialisation.

- Allowlist, not denylist: an input the allowlist does not name is **absent**, not ranked lower
- Hashed before use: an attempt whose bundle was not hashed is never requested
- Recorded: the hash goes into the attestation, which is what makes an attempt reproducible

### Oracle

A stage whose output judges another stage's work: `unit-oracle` and `acceptance-oracle`.

Its value rests on one property: it was not written, chosen or adjusted by whatever produced the
code it measures.

### Locked Test and Oracle Manifest

A **locked test** is a test produced by an oracle stage and sealed before implementation begins.
The **oracle manifest** is the signed record that seals a slice's locked tests.

- Written by: the oracle identity
- Read-only to: the build identity, enforced by the mount plan and the commit-range scope check
- Sealed before implementation: no `implement` attempt is requested until a valid manifest exists
- A protected path: no mount plan makes a locked test writable by a build stage

### Dispute

A build stage's recorded assertion that a locked test is wrong, raised for a human.

- The only path by which a locked test changes in response to a build stage
- Never resolved by editing: an escape hatch permitting a justified edit is indistinguishable from
  one permitting the deletion of an inconvenient test, because the justification comes from the
  party that benefits

### Holdout

An acceptance test the implementation was produced without knowledge of.

- Location: outside the implementation repository at the product tier, so the barrier rests on
  repository access rather than on allowlist correctness
- Executed by: the gate service, which can reach the acceptance repository
- Feedback: **failing requirement identities only** — never test names, assertions or diffs.
  Feedback rich enough to fix a holdout failure directly is feedback rich enough to overfit to it

### Visible Set

The acceptance tests an implementation stage may see, as distinct from the holdout set. Both are
produced by `acceptance-oracle`; only the visible set reaches a build-zone bundle.

### Seam

An interface boundary a refactor must preserve. A `refactor` stage that changes a seam, or changes
a locked test, fails: refactoring is behaviour-preserving by definition, and a changed seam means
the behaviour other code depends on has changed.

### Tier

How much it matters if this work is wrong: T0 prototype, T1 internal, T2 product, T3
safety/security-critical.

- Derived from: path globs in a human-owned `risk-map.yaml` over the paths the work affects —
  the **maximum** over those paths
- **Raise-only**: a newly derived tier may exceed a recorded one but never lower it
- **Never read from a label.** A label edit must not be able to reduce the rigour applied to a
  piece of work
- Selects: the stage graph, the profiles, the thresholds and the human gates
- Missing or malformed risk map: a halt, never a default to the lowest tier

### Risk Map

The human-owned `risk-map.yaml` in a target repository, mapping path globs to tiers.

CogWorks reads it and never writes it. The `architect` stage may propose a patch, which a human
reviews and merges like any other change.

### Gate

An independently recomputed pass-or-fail check, owned by the gate service.

- Recomputed from a commit SHA, in a clean sandbox, under the gate service's own identity
- Reads its configuration from the default branch, never from the branch under judgement
- The only permitted source of required checks
- Distinct from a **node gate**, which decides whether the graph advances and is not a verdict

### Check-Run Verdict

The pass-or-fail state CogWorks derives from the gate service's check runs on a commit SHA.

- States: passing, failing, **pending**, **absent**
- Trust anchor: only check runs authored by the gate service identity are considered. A check run
  from any other author is ignored entirely, not weighted
- Pending or absent means **wait**, never assume
- Bound to a SHA: a verdict on any other SHA is treated as absent, so an old pass cannot be
  replayed against new work

### Mount Plan

The set of paths a stage attempt may write, obtained from the write-scope service for the stage's
identity role.

CogWorks passes it into the attempt request and is **never the only enforcement point** for a
write boundary.

### Stage Attestation

The signed record of one stage attempt: stage type, resolved profile, harness version, bundle
hash, identity role, mount plan, the verdicts read and their authoring identity, and the outcome.

- **Evidence, never workflow state.** A run's position is always reconstructable from GitHub
  without reading one, which keeps the evidence store off the critical path

### Escalation Dossier

The issue CogWorks creates when a stage exhausts its budget or hits a repeating failure class,
carrying the attempt history, the failure classes seen and the bundle hashes.

A human inherits the context rather than reconstructing it.

### Model Gateway

The service holding model provider credentials, through which every model call in the factory
passes.

- Holds: the provider API keys. CogWorks holds none, and neither does any sandbox
- Enforces: provider rate limits, because it is the only component that sees all factory traffic
- Resolves: a logical model name to a concrete provider model, so no provider name appears in
  CogWorks

### Specification Pull Request

The pull request a design-zone stage produces, which a human approves by review.

- Produced by: `specify`, `architect`, `contract` and `plan`
- Gated by: the gate service, running the specification gates on that pull request
- The human approval is a real gate: a plan's sub-issues are not created until the plan is
  approved

### Trigger Label

The label whose application starts a run.

- **Human-only.** CogWorks ignores it when applied by any other actor
- Named in configuration, never in this specification: the factory's label namespaces are
  harmonised across services in the handbook, and hardcoding a name here would mean changing it
  twice

### Actor Trust

The field on a normalised event recording who caused it, established by the factory's ingress
service.

CogWorks uses it to ignore events from external actors that would otherwise start agent work.

---

## Pipeline Concepts

### Pipeline

A directed graph of nodes that takes a work item from intake to pull request.

- Identified by: The parent work item's GitHub Issue number
- Contains: A directed graph of nodes connected by edges, resolved from the pinned harness for the work item's tier and discipline
- State: Represented by GitHub labels on the parent work item and a structured state comment (JSON) updated at each node boundary
- Lifespan: From the EventSource/work-item adapter reporting readiness to terminal node completion or pipeline failure
- No default: There is no fallback graph. If no graph exists for the resolved tier and discipline, the run halts (REQ-GRAPH-005)
- See also: Stage Graph, Harness, Tier

### Pipeline Graph

The directed graph structure defining a pipeline's execution flow. Used interchangeably with **Stage Graph**.

- Contains: Nodes (each declaring one stage type) and edges (transitions with optional deterministic conditions)
- Supports: Sequential execution, parallel fan-out, fan-in synchronisation, conditional routing, and controlled cycles (rework loops)
- Cycles: Permitted but every cycle MUST have an explicit termination condition (max traversals, cost budget, or deterministic exit condition)
- Ordering: Execution order computed from edge definitions via topological sorting; unconnected nodes may execute concurrently
- Source: Resolved from the harness at the pinned version, keyed by tier and discipline. Not supplied, extended or overridden by the target repository

### Node

A position in the stage graph. Every node declares exactly one stage type, which determines what it does.

- Identified by: `NodeId` — unique within the resolved graph
- Stage type: Exactly one `StageTypeId` drawn from the harness's stage-type registry
- Inputs: Named artifacts or state that must exist before the node can execute
- Outputs: Named artifacts or state that the node produces for downstream nodes
- Validation: Output checked against the output schema its resolved profile declares
- Timeout: Maximum wall-clock time; exceeded → node failure
- Budget and attempts: Taken from the stage contract its resolved profile declares
- Gate: `auto-proceed` or `human-gated`, declared by the resolved graph for the work item's tier

A node is an *instance*; a stage type is a *kind*. Several nodes in one graph may share a stage type — for example one `implement` node per slice.

### Edge

A transition between two nodes in the pipeline graph.

- Contains: Source node, target node, optional condition
- Unconditional edges: Always taken (condition omitted)
- Conditional edges: Taken only when the condition evaluates to true
- Evaluation modes (per source node): `all-matching` (fan-out to all true edges), `first-matching` (exclusive routing), or `explicit` (node output includes a `next_edges` field — a list of edge IDs — that the orchestrator uses to determine exactly which edges to activate; the node's output schema must declare this field)

### Edge Condition

A rule that determines whether an edge is taken after its source node completes.

- **Deterministic condition**: Expression evaluated against pipeline state using a simple expression language (e.g., `gate.g6.conclusion == "success"`, `implement.attempt_count < 3`)
- **Composite condition**: Boolean combination (AND, OR, NOT) of deterministic conditions

Routing is always deterministic. No edge condition is evaluated by an LLM, and no model output selects which edge is taken: an LLM produces artifacts, not control flow.

### Rework Edge

An edge that creates a cycle (loop) in the pipeline graph for retry or rework scenarios.

- MUST specify: Maximum traversal count, which node outputs to preserve vs. discard on re-entry, retry vs. rework semantics
- Retry: Same input, try again (e.g., LLM schema validation failure)
- Rework: Modified input, different approach (e.g., review findings fed back to code generation)
- Overflow behaviour: When max traversals reached — halt with error, escalate to human, or take a configured overflow edge

### Node Gate

A configurable decision point after a node completes where the pipeline may pause for human approval.

- Configuration: `auto-proceed` (continue immediately) or `human-gated` (wait for explicit approval)
- Scope: Declared per node by the resolved stage graph
- Tier: Which stages carry a human gate is a property of the work item's tier, and a tier's human gates cannot be weakened by any other configuration source
- Represented by: an awaiting-review label while waiting
- Boundary: A node gate decides whether the *graph advances*. It is not a verdict. Whether an artifact passes is computed by the gate service and read from its check runs (Check-Run Verdict).

### Step Function

A single CLI invocation that reads GitHub state, executes one graph traversal pass, and writes results back.

- Stateless: No carried state between invocations; pipeline state reconstructed from GitHub
- Idempotent: Re-invoking for the same state produces the same result (or detects prior completion)
- Scope: One invocation may execute one or more nodes (e.g., a node and its unconditional successors, or parallel fan-out nodes)

---

## Work Item Concepts

### Work Item

A GitHub Issue that represents a unit of work to be implemented by CogWorks.

- Identified by: GitHub Issue number
- Contains: Title, description, optional structured fields (affected components, priority, type)
- Trigger: Pipeline starts when the EventSource/work-item adapter reports that the work item meets its configured readiness condition; no particular label, project status, or webhook mapping is prescribed
- State labels: the configured run-state labels (active stage, run lock, awaiting review); see Label Categories
- Milestone: If the work item has a GitHub Milestone set, it is preserved and inherited by sub-work-items (see Milestone)

### Sub-Work-Item

A GitHub Issue created by CogWorks from one slice of an approved plan, representing one implementation task within a larger work item.

- Identified by: GitHub Issue number
- Created from: One slice of an approved `plan.yaml`
- Created as: A native GitHub sub-issue of the parent work item (using the sub-issue API)
- Contains: Title, description, size estimate, file list, contract references, requirement-ID mapping, dependency references
- Dependencies: Expressed via native GitHub typed issue links (`blocks` / `is blocked by`), not via labels
- Milestone: Inherits the parent work item's GitHub Milestone, if one is set
- Constraint: Maximum configurable count per work item (default: 10), and each slice within its declared size budget
- Constraint: Must represent logical changes, not individual files
- Ordering: Topologically sorted by declared dependencies (read from GitHub typed issue links)
- See also: Slice

### Slice

One vertical unit of implementable work in an approved plan — the unit a single `implement` stage attempt addresses.

- Identified by: `SliceId` within the plan
- Declares: Size estimate, the requirement IDs it covers, the contracts it implements, and its dependencies on other slices
- Becomes: One sub-work-item issue, and one branch `cogworks/<issue>/slice-<n>`
- A slice is the unit of the oracle barrier: its oracle is locked before its implementation begins

### Classification

The result of analysing a work item to determine its type, scope, and proposed tier.

- Contains: Task type (enum), affected modules (list), estimated scope, proposed tier, rationale
- Task types: new feature, bug fix, refactor, configuration change, documentation, dependency update
- Determines: The proposed tier, which in turn selects the stage graph, the profiles and the human gates

### Scope Estimate

A rough measure of work item size used to decide whether human confirmation is needed.

- Contains: Estimated file count, estimated interface count, complexity rating
- Threshold: Configurable; exceeding triggers escalation

---

## Artifact Concepts

### Specification Document

The design artifacts produced by the zone A stages — `specify`, `architect` and `contract` — describing what will be built and why.

- Contains: Confirmed intent, acceptance criteria with requirement IDs, ADRs, non-functional requirements, threat-model delta, program design
- Delivered via: Specification pull request (referencing the work item)
- Approved by: A GitHub review from an authorised human, followed by a merge
- Lives in: The repository's specification paths at the lower tiers; a separate specification repository at product tier and above

### Contract

The program design produced by the `contract` stage: types, signatures, error model, call-tree and file-tree diff, plus compilable stubs.

- Language: The target language of the discipline pack, not pseudocode
- Delivered via: Specification pull request (referencing the work item and the preceding specification pull requests)
- Validated: Stubs must compile and satisfy the declared interface conventions

### Plan

The `plan.yaml` produced by the `plan` stage: the vertical slices of the work and the dependency graph over them.

- Contains: One slice entry per unit of implementable work, with size estimate, requirement-ID mapping, contract references and dependencies
- Validated: Topological sort must succeed (no cycles), every requirement ID covered, every slice within its size budget
- Becomes: One sub-work-item issue per slice, created in dependency order once the plan is approved

### Implementation Output

The source changes produced by an `implement` or `refactor` stage attempt for a single slice.

- Contains: Source files (new or modified) within the stage's declared writable paths, plus the stage's own tests
- Delivered via: A branch pushed by the stage runner, and a pull request CogWorks opens
- Judged by: The gate service, from the commit SHA. The stage's own claim that it succeeded carries no weight

### Review Result

The structured findings produced by an assurance review stage.

- Contains: Per-finding file and line references, severity, and explanation
- Severity levels: Blocking, warning, informational
- Not a verdict: A review stage reports findings and writes nothing. Whether a finding blocks is decided by the gate service from the findings and the tier's thresholds

---

## Context and LLM Concepts

### Context Bundle

The assembled, hashed set of inputs provided to a single stage attempt. The unit of what an agent is allowed to see.

- Assembled from: Exactly the inputs the stage's profile allowlists — nothing more
- Exclusion is absolute: An input not on the allowlist is absent, not merely deprioritised, truncated or summarised
- Hashed: Every bundle carries a `BundleHash` covering all of its content, recorded in the stage attestation and referenced from the resulting pull request
- Constraint: Must fit within the target model's context window
- Truncation: Deterministic priority-based strategy when content exceeds the window; truncation may never admit an excluded input nor drop a required one
- Trust zone: Assembly enforces the information barrier the profile's zone requires — an oracle stage receives no implementation source; an implementing stage receives no holdouts
- Untrusted content: Issue bodies, comments, repository files and earlier model output enter the bundle as data to be analysed, never as instructions

### Context Priority Order

The deterministic ranking used to select content when a bundle exceeds the model's window.

1. The current slice's contract definition (highest priority — never truncated)
2. Directly depended-upon slice outputs
3. Architectural constraints
4. Context Pack domain knowledge (from loaded packs)
5. Coding standards
6. Remaining content by import-graph proximity (lowest priority)

### Prompt Template

A versioned template with variable placeholders defining the instructions given to an agent for a stage type.

- Lives in: The harness, as part of an agent profile
- Resolved: At the harness version pinned by the target repository
- Constraint: Never hardcoded in CogWorks, and never supplied by the target repository
- Contract: Each template declares its required variables and its output schema

### Output Schema

A JSON Schema definition specifying the structure of a stage's output.

- Lives in: The harness, declared by the stage contract
- Purpose: Enables deterministic validation of stage output before the pipeline proceeds
- Enforcement: Invalid output fails the attempt within its attempt budget. Schema validity is not a verdict — a schema-valid output still has to pass the gate

---

## Infrastructure Concepts

### Domain Service

An external process providing domain-specific tooling capabilities. Domain services run as separate binaries from CogWorks and communicate through the Extension API.

- Capabilities: Validation, normalisation, rule review, simulation/testing, dependency validation, interface extraction, dependency graph computation
- Outputs: Always structured data in a common diagnostic format (pass/fail, artifact, location, severity, message)
- Execution: Separate process communicating over Unix domain socket (default) or HTTP/gRPC
- CogWorks is domain-ignorant: it does not interpret results beyond the structured output format
- Initial implementation: Rust domain service (software/firmware domain), shipped as a separate binary alongside CogWorks
- Extensibility: Any team can build a domain service (KiCad, FreeCAD, etc.) by implementing the Extension API

### Working Copy

A checkout of the target repository used for toolchain operations.

- Owned by: The stage runner, inside a stage attempt sandbox. CogWorks does not hold a working copy
- CogWorks reads: Individual files, directory listings and trees through the GitHub Contents and Trees APIs — never through a local clone
- Branch convention: `cogworks/<issue>/<stage>` for specification artifacts, `cogworks/<issue>/slice-<n>` for implementation work
- Lifecycle: Created and destroyed with the sandbox it lives in

### Escalation

Transfer of control from CogWorks to a human reviewer when the system cannot resolve an issue within its budget.

- Triggers: Retry budget exceeded, cost budget exceeded, scope threshold exceeded, unresolvable review findings
- Mechanism: Issue comment with structured failure report + `cogworks:node:failed` label
- Contains: All attempts, all failures, accumulated context at point of failure

### Cost Budget

A configurable limit on total LLM tokens consumed per pipeline run.

- Scope: Per-pipeline (across all nodes and sub-work-items)
- Tracking: Accumulated in-memory during processing, written to GitHub as audit artifact on completion
- Enforcement: Pipeline halts immediately when budget exceeded
- Reporting: Per-node and per-sub-work-item breakdown
- Parallel execution: Shared across concurrent nodes; budget checks are atomic

### Stage Attempt Sandbox

One fresh, short-lived sandbox obtained through the stage-runner interface for a single stage attempt.

- One per attempt: Never reused across attempts. Every retry, rework cycle or resumed attempt gets a new sandbox, so nothing from a previous attempt survives into the next
- Ownership: CogWorks requests the attempt and requests cleanup on success, failure or cancellation; the runner performs the isolation-specific operations
- Provider neutrality: The isolation technology is an infrastructure choice behind the interface, not a CogWorks contract choice
- Privilege: CogWorks holds no virtualisation privilege, no host access, and no push token. Commits are pushed by the runner under the stage's identity role
- Contents: A read-only root filesystem, non-root execution, no secrets, and egress restricted to the stage contract's network profile
- Durability: Sandbox-local state is temporary; pipeline state and deliverables remain recoverable through the repository and work-item interfaces

### Stage Attempt

One execution of one stage, in one sandbox, under one identity role.

- Inputs: Its context bundle and its mount plan
- Output: Artifacts within its declared writable paths, plus a structured output record validated against its output schema
- Bounded: By the budget and maximum attempt count its stage contract declares
- Completion: Observed through GitHub state — the expected branch reference carrying its commits, and its output record
- Attested: Each attempt produces a signed stage attestation recording what was selected, what was seen, and what was produced

### Fan-Out

A pattern where a completed node activates multiple downstream nodes for concurrent execution.

- Occurs when: Multiple edges leave a source node and all their conditions evaluate to true (using `all-matching` evaluation mode)
- Concurrent execution: Fan-out nodes execute as concurrent async tasks within the orchestrator process
- Constraints: Shared cost budget, configurable max concurrent LLM calls (default: 3)

### Fan-In

A synchronisation point where a downstream node waits for multiple upstream nodes to complete.

- Occurs when: A node declares inputs from multiple upstream nodes
- Behaviour: The downstream node stays in `pending` state until ALL input nodes have completed
- Partial failure: If one upstream node fails, the fan-in node cannot proceed. Depending on configuration, siblings may be aborted or allowed to complete.

### Pipeline State

The structured representation of a pipeline run's progress, maintained by the orchestrator.

- Contains: Active nodes (currently executing), completed nodes with outputs, pending nodes (inputs not yet available, waiting for upstream to finish), blocked nodes (an upstream dependency failed — cannot proceed without human intervention or rerouting), failed nodes with error info, per-cycle traversal counts, cumulative cost
- Node state distinctions: `pending` waits for upstream to complete normally; `blocked` means upstream has failed and this node's inputs will never arrive — requires escalation rather than waiting
- Format: JSON document, written to a GitHub comment on the parent work item at each node boundary
- Purpose: Human visibility and crash recovery
- Recovery: On re-invocation, the orchestrator reads this state and resumes from where it left off

### Audit Trail

A complete record of every decision, LLM call, validation result, and state transition in a pipeline run.

- Contents: LLM calls (model, input hash, output, tokens, latency), validation results, state transitions, total cost
- Storage: GitHub issue comments or linked artifacts
- Purpose: ISO 9001 traceability, systematic improvement, debugging

### Observability Field

A stable structured field recorded on a tracing span by CogWorks' observability hooks.

- Current fields include pipeline run ID, node ID, event kind, total USD cost at node completion, retry root cause, and rework misalignment type
- Export: The CLI routes tracing through JSON console output and the OpenTelemetry OTLP exporter
- Derived metrics: External observability systems may aggregate fields actually exported; CogWorks does not define a metric-sink abstraction or promise unavailable measurements

### Improvement Backlog

A set of GitHub Issues tagged `process:improvement` tracking systematic improvements to CogWorks' configuration, prompts, scenarios, and processes.

- Each issue captures: triggering metric, root cause diagnosis, proposed change, expected impact, verification plan
- After implementation: actual impact measured over verification period and recorded
- Purpose: ISO 9001 evidence of continuous improvement; searchable history of system optimization

### Review Cadence

The structured schedule at which CogWorks performance is reviewed by humans.

- Weekly (30 min): Operational review — failed runs, gate overrides, stuck issues
- Monthly (60 min): Quality review — post-merge defects, codebase health, safety escapes, gate calibration
- Quarterly (2 hrs): Strategic review — 3-month trends, cost analysis, shift work boundary adjustment, risk register update
- Purpose: Ensure CogWorks output is measured and issues drive action, not just accumulate

---

## GitHub State Concepts

### Processing Lock

A lightweight concurrency control using the `cogworks:processing` label.

- Applied: Before a CLI invocation starts processing a work item
- Checked: If already present, the invocation backs off (another instance is working on it)
- Removed: After the invocation completes its action
- Stale-lock override: If the label was applied more than a configurable duration ago (default: 60 minutes) and no active pipeline run is detectable, a new invocation may remove the stale label and proceed
- Limitation: Race condition window between check and set; acceptable for expected concurrency levels

### Branch Convention

The naming pattern for git branches created by CogWorks.

- Pattern: `cogworks/<work-item-number>/<slug>`
- Slugs: `spec` (architecture), `interfaces` (interface design), `swi-<n>` (sub-work-item implementation)

---

## Cross-Domain Concepts

### Interface Registry

A version-controlled repository of cross-domain interface definitions, held in the target repository's specification paths.

- Contains: Structured definitions of interfaces that span two or more domains
- Format: TOML files conforming to a published JSON Schema
- Authorship: MUST be authored and maintained by humans; CogWorks MUST NOT create or modify definitions autonomously
- Validation: Registry is validated deterministically on every pipeline run, before any pipeline node executes
- Purpose: Provide a single source of truth for inter-domain contracts

### Interface Definition

A structured specification of a cross-domain interface contract.

- Identified by: Unique human-readable ID (e.g., `SWD-IF-CAN-01`)
- Contains: Interface type, participating domains, contract parameters, ownership declarations, version number
- Interface types: `bus_protocol`, `power_rail`, `mechanical_mounting`, `thermal_interface`, `connector`, `signal`, etc.
- Versioned: Version incremented on contract change; domain services declare compatible versions

### Interface Contract

The set of constraints that all participating domains must respect for a given cross-domain interface.

- Expressed as: Named parameters with values and tolerances
- Parameter types: Numeric with tolerance, numeric exact, enumerated, boolean, reference, structured (nested)
- Ownership: Each parameter has an owning domain (defines the constraint) and complying domains (must respect it)
- Enforcement: Validated deterministically by the constraint validator; no LLM involved

### Contract Parameter

A single constraint within an interface contract.

- Numeric with tolerance: Value with min/max bounds (e.g., `voltage: { nominal: 24.0, min: 21.6, max: 26.4, unit: "V" }`)
- Numeric exact: Value without tolerance (e.g., `baud_rate: { value: 500000, unit: "bps" }`)
- Enumerated: Value from a defined set (e.g., `connector_type: { value: "JST-PH-4", allowed: ["JST-PH-4", "JST-PH-6"] }`)
- Boolean: A flag (e.g., `termination_required: true`)
- Reference: Pointer to a detailed specification document
- Structured: Nested object for complex contracts (e.g., CAN message definitions)
- Computed constraints: Derived values (e.g., total bus load) are validated by deterministic checks in the constraint validator, not expressed as formulas in the registry

---

## Extension API Concepts

### Extension API

The protocol by which external domain services register with and are invoked by CogWorks.

- Transport: Unix domain sockets (default, for co-located services) or HTTP/gRPC (for remote services)
- Message format: JSON conforming to published JSON Schemas
- Versioned: CogWorks and domain services declare compatible API versions; incompatible versions rejected during handshake
- Schemas: Published in CogWorks repository (`schemas/extension-api/`)

### Domain Service Registration

The configuration by which CogWorks knows how to reach a domain service.

- Declared in: CogWorks' own process configuration, which describes the host it runs on rather than the repository it is working on
- Contains: Service name, transport type, and connection endpoint (socket path or URL)
- Does NOT contain: capabilities, artifact types, interface types, or domain — these are discovered dynamically via the handshake
- Multiple services: Multiple domain services may be registered simultaneously
- Selection: CogWorks routes operations to the appropriate service based on artifact types and domains discovered during handshake

### Service Capability

A method that a domain service implements from the Extension API.

- Methods: `validate`, `normalise`, `review_rules`, `simulate`, `validate_deps`, `extract_interfaces`, `dependency_graph`
- Optional: Not all domain services need all methods (e.g., a service may support `validate` and `extract_interfaces` but not `normalise`)
- Discovery: CogWorks queries capabilities during health check handshake

### Domain Service Health Check (Handshake)

A combined availability check and capability discovery mechanism. The terms "health check" and "handshake" refer to the same operation.

- Protocol: JSON request-response (e.g., `POST /api/v1/handshake` for HTTP transport)
- Returns: Service name, service version, API version, domain, supported capabilities, supported artifact types, supported interface types, and service status
- Timing: Checked before invoking any domain service method at the start of a pipeline run
- Caching: Consumers cache handshake results and re-query periodically or on error
- Failure handling: Primary domain service unavailable → pipeline halts; secondary domain service unavailable → pipeline continues with warning that cross-domain validation was skipped

### Long-Running Operation Handling

The mechanism for handling domain service operations that may take extended time (e.g., simulation, FEA).

- v1 baseline: Synchronous request-response with configurable per-method timeouts (default: 10 minutes for `simulate`, 5 minutes for other methods)
- Future: Progress polling via operation IDs or streaming may be added in a future API version
- Design constraint: The protocol must not preclude adding asynchronous patterns later
- Timeout behavior: Operations exceeding the timeout are treated as failures and reported as structured diagnostics

### Request Envelope

The standardised wrapper around every Extension API request.

- Contains: `request_id` (UUID, for tracing), `api_version`, `method`, `caller` context (system, node, work item IDs), `repository` context (path, ref), `params` (method-specific), and optional `interface_contracts` (relevant cross-domain contracts from the registry)
- The `caller` context allows domain services to include traceability information in their responses without needing to understand the caller's pipeline
- The `repository.path` field is semantically overloaded: for co-located services (Unix socket), it is a local filesystem path to the repository root; for remote services (HTTP), it is a clone URL. Domain service authors must handle both string formats — check for a URL scheme (`http://`, `https://`) to distinguish them
- The `interface_contracts` field is populated by CogWorks when the invoked method is one that performs cross-domain constraint validation (`validate`, `review_rules`, `extract_interfaces`). It contains the subset of the interface registry contracts relevant to the artifacts being processed. Domain services return `constraint_results` in the response when this field is present

### Response Envelope

The standardised wrapper around every Extension API response.

- Contains: `request_id` (echoed), `status` (`success` / `failure` / `error`), `result` (method-specific), `diagnostics` (array of structured findings), optional `constraint_results` (when `interface_contracts` were provided), and `metadata` (timing, tool versions, counts)
- `success`: All checks passed — the diagnostics array MAY contain `warning` or `informational` severity findings (e.g. style notes, performance suggestions) that do not constitute failure. A service MUST return `success` when no `blocking` diagnostics were found, even if informational diagnostics are present
- `failure`: Checks ran and found blocking issues — the diagnostics array contains at least one `blocking`-severity finding
- `error`: Service-level failure — checks could not run (includes a structured error with code and recoverability)

### Diagnostic Category

A standardised classification for domain service diagnostic findings, enabling consumers to process diagnostics generically regardless of the originating domain.

- Standard categories: `syntax_error`, `type_error`, `constraint_violation`, `interface_mismatch`, `dependency_error`, `style_violation`, `safety_concern`, `performance_concern`, `test_failure`, `completeness`
- Domain services map their tool-specific findings to these categories
- Domain services may use additional domain-specific categories
- Consumers must handle unknown categories gracefully (treated as informational)

### Extension API Error Code

A standardised code for service-level errors (when the domain service cannot process a request at all).

- Standard codes: `tool_not_found` (non-retryable), `tool_failed` (potentially retryable), `invalid_request` (non-retryable), `unsupported_method` (non-retryable), `api_version_mismatch` (non-retryable), `timeout` (potentially retryable), `artifact_not_found` (non-retryable), `internal_error` (potentially retryable)
- Each code has a defined recoverability that consumers use to decide retry strategy
- Distinct from diagnostic findings: error codes mean the operation could not complete; diagnostics mean the operation completed but found issues

---

## Knowledge and Safety Concepts

### Context Pack

A structured set of domain knowledge, safe patterns and anti-patterns for a specific technical domain, supplied by the harness as part of a discipline pack.

- Located in: The harness, under the discipline pack, alongside the profile skills it supports
- Contains: Trigger definition, domain knowledge document, safe patterns document, anti-patterns document (with explanations of why each pattern is unsafe)
- Loading: Deterministic, driven by the work item's tier, discipline, stage type and detected language or toolchain — never by model inference
- Timing: Loaded during context bundle assembly for a stage, before the attempt starts
- Not repository-supplied: A pack is prompt material. In-repository packs would be prompt content an agent working in that repository could edit, so packs live only in the harness
- Multiple packs: A single stage attempt may load multiple packs simultaneously
- Conflict resolution: Where packs contain contradictory guidance, the more restrictive rule applies
- Versioned: Pinned with the rest of the harness, so a pack change cannot alter a repository's behaviour until its pin moves
- Hashed: Pack content is covered by the context bundle hash

### Constitutional Rules

A set of non-overridable behavioural constraints occupying a privileged position in the system prompt of every agent turn.

- Located in: The harness, resolved at the version pinned by the target repository
- Loading: Unconditional on every pipeline run — NOT a configurable gate
- Position: Privileged and non-overridable. For agent turns inside a sandbox, the profile places the rules and the sandbox boundary preserves that position; for deterministic helper calls in CogWorks' own process, the privileged position is enforced at the type level so no call can reach a model without passing through constitutional assembly
- Separation: No content in the context bundle (issue bodies, specifications, external documents, earlier model output) may modify, append to, or override them
- Change control: Changes are reviewed and merged in the harness, then adopted by a repository moving its pin
- Verification: CogWorks verifies the loaded rules against the pin before use, and halts rather than proceed with rules it cannot verify
- Format: Plain language that a non-specialist reviewer can evaluate

### Prompt Injection

The presence of text in untrusted content structured to influence model behaviour as if it were an instruction.

- Examples: Persona override instructions, behavioural modification directives, instruction overrides embedded in code comments
- Primary defence: Untrusted content enters a context bundle as data, never as instructions, and event or issue fields are never interpolated into a prompt
- Intake screening: Screening an incoming work item is the upstream intake service's responsibility; CogWorks verifies its signed approval rather than repeating the scan
- Defence in depth: CogWorks scans model-sourced text for injection patterns before reusing it in a later prompt — for example a finding description fed into a rework attempt
- Consequence: The work item enters a hold state that only a human can clear

### Scope Violation

A stage attempt producing changes that implement capabilities, touch files, or introduce dependencies not present in the approved specification and contract documents.

- Types: Unauthorized network calls, file system access, IPC mechanisms, external process invocations, hardware access not in the contract document
- Detection: The mount plan constrains what the attempt can write at all, and the commit-range scope check rejects commits reaching outside the attempt's writable paths. CogWorks is never the only enforcement point
- Consequence: The attempt fails, and the pipeline halts with a `SCOPE_UNDERSPECIFIED` or `SCOPE_AMBIGUOUS` event

### Authorised File Set

The set of source files a specific work item is permitted to create or modify, derived from the contract document and the specification.

- Source: The `contract` stage's output together with the specification document
- Expressed as: The writable paths of the stage contract, realised as a mount plan from the mount-plan provider
- Enforcement: The mount plan bounds the attempt, and the commit-range scope check verifies the commits it produced
- Violation: Writing outside this set is a scope violation

### Hold State

A work item state in which it is suspended from all automated processing pending human action.

- Entry: An unverifiable upstream attestation, a scope or injection event raised during a run, or an escalation CogWorks cannot resolve within the stage contract's budget
- Behavior: Work item is NOT automatically requeued or retried
- Exit: A human must review the recorded cause and either clear the hold with a recorded justification or close the work item as contaminated
- Label: The configured hold label, distinct from the configured node-failure label

### Alignment Verification

The process of verifying that a pipeline node's output matches the intent of its upstream input — semantic correctness, distinct from structural correctness (schema validation) and technical correctness (domain service validation).

- Purpose: Catches cases where the output is valid and correct code but doesn't address what was requested
- Examples: Architecture spec solves a different problem than the work item described; interfaces add methods not in the spec; code implements a different algorithm than specified
- Timing: Runs as part of step 4 of the node execution lifecycle, after schema and domain service validation
- Two check types: Deterministic (structural comparison — fast, cheap, no LLM bias) and LLM (semantic comparison — adversarial prompt, ideally different model)
- Output: Alignment result with score, findings, and traceability matrix entries
- Failure triggers: Rework (not retry) with specific misalignment findings in context
- Not a verdict: An alignment result bounds a stage attempt. CogWorks may fail, retry or rework the attempt on it, but MUST NOT publish it as a check run or treat it as a gate verdict. An attempt that passes every alignment check still has to pass the gate service

### Alignment Finding

A structured report of a specific misalignment between a node's output and its upstream input.

- Contains: type, severity, description, input reference, output reference, suggestion
- Finding types:
  - `missing`: Something required by the input is absent from the output
  - `extra`: Something present in the output was not requested by the input
  - `modified`: Something in the output contradicts or changes what the input specified
  - `ambiguous`: The input is unclear and the output made an assumption that should be confirmed
  - `scope_exceeded`: The output addresses concerns beyond the scope of the work item
- Severity levels: `blocking` (fails alignment check, rework required), `warning` (passes, but included in review context), `informational` (logged in audit trail)
- Each finding references both the input and the output, enabling targeted remediation

### Alignment Score

A numerical measure (0.0–1.0) of how well a node's output matches the intent of its upstream input.

- Produced by: The LLM alignment check as part of its assessment
- Threshold: Declared by the resolved profile's stage contract, floored by the work item's tier (REQ-ALIGN-041)
- Pass criteria: Score ≥ threshold AND zero blocking findings
- Tracked in: Audit trail and performance metrics (per stage)

### Rework

Re-execution of a node with modified context because the previous output was technically valid but semantically misaligned with its input.

- Distinguished from: Retry (re-execution because the output was technically invalid — didn't compile, didn't parse)
- Context: Rework context includes the specific misalignment findings from the alignment check, giving the LLM targeted feedback on what to fix
- Budget: Separate from the retry budget; declared by the stage contract (default: 3 cycles)
- Counter: Tracked independently from retry counter in pipeline state
- Audit: Each rework cycle is recorded with its alignment findings and the resulting output

### Traceability Matrix

A structured artifact mapping each requirement from the work item through the pipeline stages to the final deliverable.

- Built: Incrementally as the pipeline progresses — each alignment check adds its columns
- Columns: Requirement → Architecture → Interface → Sub-Work-Item → Code → Status
- Status values: ✅ (satisfied), ⚠️ N/A (not applicable at this stage, with reason), ❌ (not addressed)
- Published: Posted as a comment on the work item issue at pipeline completion
- ISO 9001: Serves as evidence that requirements flow through to implementation
- Tier: At the highest tier, the matrix requires human sign-off (REQ-ALIGN-041)

### Pipeline Events (Safety)

Structured events emitted by the constitutional layer and scope enforcer when behavioral boundaries are violated.

- **INJECTION_DETECTED**: External content contains text structured as a directive to CogWorks. Includes: pipeline run ID, work item ID, source document, offending text. Triggers pipeline halt and hold state.
- **SCOPE_UNDERSPECIFIED**: Fulfilling the work item would require capabilities not in the approved specification. Includes: missing capability description, relevant spec section. Triggers generation halt.
- **SCOPE_AMBIGUOUS**: Specification is ambiguous for a safety-affecting work item. Includes: ambiguous section, conflicting interpretations. Triggers generation halt and human clarification request.
- **PROTECTED_PATH_VIOLATION**: A stage attempt's changes match protected path patterns. Triggers pre-PR validation failure.

### Extraction Completeness

A domain service's assessment of whether it could confidently extract all relevant values when computing actual values for cross-domain constraint validation.

- Reported as: `status: "incomplete"` in the `extract_interfaces` response
- Consequence: Constraint validation produces a warning requiring human review, not a silent pass
- Related risk: CW-R07 (Cross-domain constraint false negative)

### Protected Path

A file path pattern identifying files that CogWorks must never create or modify through the normal pipeline.

- Examples: The pin file, the risk map, locked tests and their manifest, gate configuration, and the Extension API schemas
- Enforcement: The mount plan does not make a protected path writable, and pre-PR validation checks the attempt's changes against the protected path patterns
- Change control: Protected paths require a human-approved pull request via CODEOWNERS or equivalent

---

## Label Categories

CogWorks uses GitHub labels for two distinct purposes. Labels are split into categories to support mixed human and agent development workflows.

Label *names* are configuration, not vocabulary. The factory's label namespaces are harmonised
across services in the [`brightsmiths/handbook`](https://github.com/brightsmiths/handbook)
repository under `docs/architecture/state-model.md`, and CogWorks reads its label names from
its own configuration (REQ-LABEL-001). This document names the *roles* a label plays, never a
literal label string.

### Run-State Labels

Labels that represent CogWorks' own run state. These are written exclusively by CogWorks and
should not be modified by hand except during troubleshooting.

- Run lock — indicates an active run holds this work item
- Active stage — tracks the stage or stages currently executing
- Stage failure — indicates a stage has failed
- Slice status — tracks the processing status of a slice sub-issue

### Workflow-Semantic Labels

Labels that represent workflow state meaningful to both humans and agents.

- Trigger — starts a run. Human-applied only: CogWorks MUST ignore it when applied by any other actor (REQ-EXEC-004)
- Restart — requests a full run restart
- Awaiting review — a gate is waiting for human approval
- Hold — the work item is suspended pending human review
- Cancellation — signals that the run should stop
- Slice — identifies a slice sub-issue

A work item's tier is **not** carried by a label that CogWorks trusts. Tier is derived from
`risk-map.yaml` on every run (REQ-TIER-001), so that a label edit cannot lower the stringency
applied to a piece of work.

---

## Milestone

A GitHub Milestone associated with a work item, representing a development phase, release target, or software version.

- Optional: Work items are not required to have a milestone
- Inherited: When CogWorks creates sub-work-items, each sub-work-item inherits the parent work item's milestone (if set)
- Not modified: CogWorks MUST NOT create, modify, or delete milestones. Milestone management is a human responsibility
- Not an ordering input: CogWorks does not choose which work item to take next, so a milestone never affects its ordering (REQ-BOUND-008)
- Visibility: Milestones provide natural grouping for human oversight, showing which work items belong to which release or development phase

---

## Semantic Stalling

A condition where repeated attempts produce novel but equally incorrect output, consuming the attempt budget without making progress.

- Detection: CogWorks classifies attempt failures (e.g., `type_error`, `constraint_violation`) across consecutive attempts for a slice. If the same failure class recurs for a configurable number of consecutive attempts (default: 3), CogWorks flags semantic stalling
- Distinguished from: Flaky checks (identical code, different results — EDGE-015) and normal retries (different failure classes on each attempt, suggesting progress)
- Response: Early escalation with a structured dossier identifying the recurring failure class and every attempt, rather than consuming the remaining attempt budget
- Rationale: An agent that produces different code but hits the same class of failure is unlikely to resolve it with more attempts. A repeating failure class usually means the specification or the oracle is wrong, not the implementation — so escalating early is the correct response, not a last resort

