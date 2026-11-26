# Functional Requirements

This document defines the functional requirements for CogWorks. Every requirement has a unique ID that is referenced in `assertions.md` to establish traceability between requirements and testable behavioral assertions.

Requirements are organized by functional area. Each requirement states what the system **MUST** or **MUST NOT** do. Implementation constraints (how it must be done) are in `constraints.md`.

## Increment levels

Each category carries a **Level** marker giving the increment at which its requirements come into force. Levels correspond to the increments of CogWorks' role in the wider factory, described in the [`brightsmiths/handbook`](https://github.com/brightsmiths/handbook) repository under `docs/roadmap.md`.

| Level | Scope |
| ----- | ----- |
| **L1** | First dark slice: the `unit-oracle` and `implement` stage types on a single internal-tier repository, driven from the CLI, with profile selection and stage graphs resolved from the pinned harness and verdicts read from gate check runs. |
| **L2** | Event-driven runs, the `specify`, `contract` and `plan` stage types producing specification pull requests, sub-issue creation, tier derivation and signed stage attestations. |
| **L3** | Consumption of automated intake: upstream attestations verified and their machine-readable blocks consumed. |
| **L4** | Product-tier support: the `architect`, `acceptance-oracle`, `refactor`, `review-semantic` and `review-security` stage types, holdouts, and the specification / implementation / acceptance repository split. |

A requirement at a later level is specified now and implemented when that level is built. Requirements with no level marker are foundational and apply from L1.

## Retired requirement IDs

Some requirement IDs appear in this document only as retirement notes. A retired ID **MUST NOT** be reused for a new requirement: old IDs appear in commit messages, test names and issue history, and reassigning one would silently change what that history refers to. New requirements always take the next unused number in their category.

---

## REQ-PIPE: Pipeline Execution

### REQ-PIPE-001: Trigger on work-item readiness

CogWorks MUST initiate the pipeline when its EventSource reports that a work item is ready for processing and the work item has no current active-node state. The readiness signal and its mapping from provider events or work-item state MUST be handled behind the GitHub/EventSource interface. The current event contract uses `LabelApplied`; an adapter may map configured label, project, webhook, or other readiness signals to that event. This specification does not select a particular mapping.

### REQ-PIPE-002: Node label tracking

CogWorks MUST apply `cogworks:node:<name>` labels to track the currently active node(s) of each work item. Multiple node labels may be present simultaneously when nodes execute in parallel.

### REQ-PIPE-003: Node advancement

CogWorks MUST evaluate outgoing edge conditions when a node completes and activate all downstream nodes whose edge conditions are satisfied and whose other input requirements are met.

### REQ-PIPE-004: Idempotent operations

All pipeline operations MUST be idempotent. Re-invoking the step function for the same GitHub state MUST produce the same outcome or detect prior completion and take no action.

### REQ-PIPE-005: Audit comments

CogWorks MUST post a structured status comment on the work item issue when entering each node, when a node completes successfully, and when a failure occurs.

### REQ-PIPE-006: Node gate enforcement

CogWorks MUST respect the node gate configuration (`auto-proceed` or `human-gated`) declared by the resolved stage graph. The work item's tier (REQ-TIER-001) determines which stages carry a human gate; a tier's human gates MUST NOT be weakened by any other configuration source.

These are CogWorks pipeline-advancement gates: they decide whether the stage graph advances, never whether an artifact passes. Gate verdicts are a separate concern, computed elsewhere and read through check runs (REQ-VERDICT-001). Human approvals recorded as pull request reviews are conveyed through the work-item interface and do not change the configured CogWorks gates.

**Exception**: Constitutional rules loading (REQ-CONST-001) is unconditional and not subject to gate configuration. It runs on every pipeline invocation regardless of gate settings.

### REQ-PIPE-007: Processing lock

CogWorks MUST apply a `cogworks:processing` label before processing a work item and MUST back off (exit without taking action) if that label is already present when a new invocation begins.

### REQ-PIPE-008: Duplicate pipeline prevention

The orchestrator MUST prevent duplicate pipeline runs for the same work item. If a pipeline is already running for an issue, a new trigger MUST either be rejected (with a comment explaining the conflict) or queue for execution after the current run completes (configurable).

### REQ-PIPE-009: Pipeline resumption

Re-triggering a failed pipeline MUST support resuming from the failed node (using the persisted pipeline state from REQ-EXEC-002) rather than restarting from the beginning. A full restart MUST be supported only on explicit request through the configured work-item interface. `cogworks:restart` and `/cogworks restart` are default adapter mappings, not fixed business-logic signals.

### REQ-PIPE-012: Stage attempt workspace

For each stage attempt, CogWorks MUST request one fresh sandbox through the stage-runner interface (REQ-SANDBOX-001). CogWorks MUST request cleanup on success, failure, or cancellation. The sandbox provider and isolation technology are infrastructure choices behind that interface. Durable pipeline state and deliverables MUST remain recoverable from the work-item system and repository; sandbox-local state is an execution aid, not a durability mechanism.

_Note: Requirement IDs REQ-PIPE-010 and REQ-PIPE-011 are retired and MUST NOT be reused._

---

## REQ-GRAPH: Pipeline Graph Structure

### REQ-GRAPH-001: Directed graph with controlled cycles

The pipeline MUST be a directed graph where nodes represent processing steps and edges represent transitions. The graph MUST support sequential execution, parallel fan-out, fan-in synchronisation, and conditional edges. The graph MAY contain cycles (for retry and rework loops). Every cycle MUST have an explicit termination condition that guarantees eventual exit. The orchestrator MUST enforce termination conditions and halt the pipeline with a clear error if a cycle would exceed its limit.

### REQ-GRAPH-002: Node identity and ordering

Each node in the graph MUST have a unique name within the resolved stage graph, and MUST declare exactly one stage type (REQ-STAGE-001). The orchestrator MUST compute the execution order from the edge definitions using topological sorting. For nodes that are not connected by any path, the orchestrator MUST support concurrent execution (see REQ-EXEC-006).

### REQ-GRAPH-005: Stage graph source

The stage graph MUST be resolved from the harness at the version pinned by the target repository (REQ-HARNESS-001), selected by the work item's tier and discipline. The target repository MUST NOT be able to define, extend, override or replace the stage graph. There MUST be no fallback graph: if no graph exists for the resolved `(tier, discipline)` pair, CogWorks MUST halt the run rather than substitute a default.

_Note: Requirement IDs REQ-GRAPH-003 and REQ-GRAPH-004 are retired and MUST NOT be reused._

---

## REQ-NODE: Node Types

### REQ-NODE-001: Node interface

Every node MUST implement a common interface: name (unique within the graph), stage type (REQ-STAGE-001), inputs (required artifacts or state), outputs (produced artifacts or state), validation (how success is determined), timeout (maximum wall-clock time), and cost budget. The orchestrator MUST verify all declared inputs are available before starting a node.

A node's behaviour is determined entirely by its stage type and the agent profile resolved for it (REQ-PROFSEL-001). CogWorks MUST NOT distinguish node kinds by any built-in taxonomy of its own.

_Note: Requirement IDs REQ-NODE-002, REQ-NODE-003 and REQ-NODE-004 are retired and MUST NOT be reused._

---

## REQ-STAGE: Stage Types

### REQ-STAGE-001: Stage type registry

CogWorks MUST resolve the set of valid stage types from a registry supplied by the harness at the pinned version. A stage type names a kind of work — for example `specify`, `architect`, `contract`, `plan`, `unit-oracle`, `acceptance-oracle`, `implement`, `refactor`, `review-semantic`, `review-security`, `drift-scan` or `debt-fix`. Each node in the resolved stage graph MUST declare exactly one stage type drawn from that registry.

### REQ-STAGE-002: Registry validation at graph load

When loading a stage graph, CogWorks MUST validate every node's declared stage type against the registry **before** dispatching any node. A stage type absent from the registry MUST halt the run with a graph-invalid error. Dispatch-time rejection of an unknown stage type MUST also be implemented, as a second line of defence, and MUST fail the node rather than terminate the process.

### REQ-STAGE-003: No built-in stage types

CogWorks MUST NOT contain a built-in list of stage types, agent personas, role prompts, or any discipline-specific logic. A stage type is data supplied by the harness. Adding a stage type MUST NOT require a change to CogWorks.

---

## REQ-PROFSEL: Agent Profile Selection

_Note: the retired `REQ-PROFILE-*` IDs described per-node tool profiles and MUST NOT be reused. This category is distinct._

### REQ-PROFSEL-001: Deterministic profile resolution

For each stage, CogWorks MUST resolve exactly one agent profile from the pinned harness, keyed by `(stage type, discipline, language or toolchain, tier)`. Resolution MUST be deterministic: the same key against the same harness version MUST always resolve to the same profile at the same version. No LLM may participate in, influence, or override profile resolution.

### REQ-PROFSEL-002: Stage contract loading

Resolving a profile MUST also load its stage contract, which declares: the input allowlist, the output schema, the writable paths, the network profile, the budget, the maximum attempt count, and the identity role. CogWorks MUST take these values from the contract and MUST NOT substitute defaults of its own for any value the contract declares.

### REQ-PROFSEL-003: Selection recorded in the attestation

The resolved stage type, profile identifier, profile version, harness version, discipline, language or toolchain, and tier MUST be recorded in the stage attestation (REQ-ATTEST-003), so that the selection can be reproduced and audited after the fact.

### REQ-PROFSEL-004: The agent never sees its own contract

The stage contract MUST NOT be placed in the context bundle or otherwise made visible to the agent it governs. The contract is enforced by CogWorks, the mount-plan provider and the gate service; an agent that could read its own constraints could reason about evading them.

### REQ-PROFSEL-005: Unresolvable profile halts

If no profile resolves for a stage's key, CogWorks MUST halt the run with a diagnostic naming the unresolved key. CogWorks MUST NOT fall back to another profile, to a default profile, or to an unprofiled agent turn.

---

## REQ-HARNESS: Harness Loading and Pinning

### REQ-HARNESS-001: The harness is the source of agent configuration

CogWorks MUST load the stage-type registry, stage graphs, agent profiles, stage contracts, prompt templates, output schemas, Context Packs and constitutional rules from the harness, at the version pinned by the target repository in `.factory/pin.yaml`.

### REQ-HARNESS-002: Pin resolution and verification

CogWorks MUST read the pin from the target repository and MUST verify that the harness content it loaded matches that pin before using any of it. A missing, malformed, unresolvable or withdrawn pin MUST halt the run. CogWorks MUST NOT substitute a default version, the latest version, or a cached version from a previous run.

### REQ-HARNESS-003: The harness is read-only

CogWorks MUST treat the harness as read-only. CogWorks MUST NOT write to the harness, and MUST NOT grant any stage write access to it (REQ-BOUND-010).

### REQ-HARNESS-004: No repository-supplied agent configuration

The target repository MUST NOT be able to supply, extend or override any of the material listed in REQ-HARNESS-001. The repository contributes exactly two inputs to a run: `risk-map.yaml` and `.factory/pin.yaml`, both human-owned.

### REQ-HARNESS-005: Harness content drift during a run

CogWorks MUST record the resolved harness version and the hash of the resolved stage graph in the pipeline state. On resuming a run, a mismatch between the recorded values and the freshly resolved values MUST escalate for human decision rather than halting outright or silently continuing against the new content.

---

## REQ-BUNDLE: Context Bundles

### REQ-BUNDLE-001: Allowlisted inputs only

The context bundle for a stage attempt MUST be assembled strictly from the inputs its profile's allowlist declares. An input not on the allowlist MUST be absent from the bundle — not merely deprioritised, truncated or summarised. CogWorks MUST NOT add an input to a bundle because it appears useful.

### REQ-BUNDLE-002: Bundle hashing

Every context bundle MUST be hashed, and the hash MUST be recorded in the stage attestation and referenced from the resulting pull request (REQ-INT-002). The hash MUST cover every input in the bundle, including Context Pack content and constitutional rules.

### REQ-BUNDLE-003: Trust-zone barrier

Bundle assembly MUST enforce the information barrier its profile's trust zone requires. In particular: a stage that writes an oracle MUST NOT receive implementation source, and a stage that writes an implementation MUST NOT receive holdout content or, at product tier and above, acceptance source. Enforcement MUST be structural — a barrier that depends on the agent choosing not to look is not a barrier.

### REQ-BUNDLE-004: Untrusted content is data

Content originating outside the harness — issue bodies, comments, repository files, model output from an earlier stage — MUST be placed in the bundle as data to be analysed, never as instructions. Event or issue fields MUST NOT be interpolated into a prompt.

---

## REQ-SANDBOX: Stage Attempt Execution

### REQ-SANDBOX-001: Stage attempts execute in a sandbox

Every agent stage attempt MUST execute in a fresh sandbox obtained through the stage-runner interface. CogWorks MUST pass the image, the context bundle, the mount plan, the network profile, the resource limits and the identity role, and MUST NOT execute the agent itself (REQ-BOUND-001).

### REQ-SANDBOX-002: Identity role and push

Each stage attempt MUST run under the identity role its stage contract declares. Commits produced by the attempt MUST be pushed by the stage runner under that identity, never by CogWorks (REQ-BOUND-007).

### REQ-SANDBOX-003: Mount plan

The writable paths a stage attempt is permitted MUST be expressed as a mount plan obtained from the mount-plan provider for the stage's identity role, and passed to the stage runner. CogWorks MUST NOT be the only enforcement point for write boundaries.

### REQ-SANDBOX-004: Completion observed through GitHub state

A stage attempt's completion MUST be determined from GitHub state: the expected branch reference carrying the attempt's commits, and the attempt's output record. A queue or callback event MAY prompt CogWorks to re-read that state sooner, but MUST NOT be the authority for completion — a run must be reconstructable from GitHub alone after a restart.

### REQ-SANDBOX-005: Network profile

A stage attempt's egress MUST be restricted to the network profile its stage contract declares. No stage attempt may be granted general network access, and no secret may be placed inside a sandbox.

---

## REQ-VERDICT: Gate Verdicts

### REQ-VERDICT-001: Verdicts are read, not computed

CogWorks MUST obtain every pass/fail verdict by reading the gate service's check runs for the relevant commit SHA, together with their structured output.

### REQ-VERDICT-002: CogWorks never authors a verdict

CogWorks MUST NOT compute a pass/fail verdict, MUST NOT author or publish a check run, and MUST NOT configure which checks are required (REQ-BOUND-008). CogWorks' own stage pre/post checks bound an attempt — they may cause a retry, a rework cycle or an escalation — but they are never a verdict and are never published as a check run. An attempt that passes every CogWorks check still has to pass the gate.

### REQ-VERDICT-003: Single trust anchor

CogWorks MUST accept a check run as a verdict only when it was authored by the gate service's own identity. A check run authored by any other identity MUST be ignored regardless of its name, conclusion or content. This is CogWorks' only trust anchor for pass and fail, so it MUST NOT be configurable per repository.

### REQ-VERDICT-004: Coarse feedback on hidden checks

Where a verdict derives from checks hidden from the implementing stage, the feedback CogWorks passes back MUST be limited to the affected requirement identifiers. CogWorks MUST NOT relay hidden check names, assertions, source or output.

### REQ-VERDICT-005: Pending and stale verdicts

A pending verdict MUST cause CogWorks to wait, not to advance. A verdict bound to a SHA other than the one under consideration MUST be treated as absent. A missing verdict MUST NOT be interpreted as a pass.

---

## REQ-ORACLE: Oracle Separation

### REQ-ORACLE-001: Separate stages, identities and sandboxes

For a given slice, the oracle and the implementation MUST be produced by different stage types, running under different identity roles, in different sandboxes. CogWorks MUST NOT run a single stage that produces both.

### REQ-ORACLE-002: Authorship barrier

The stage that writes the oracle MUST NOT receive the implementation source, and the stage that writes the implementation MUST NOT be able to modify the oracle (REQ-BUNDLE-003).

### REQ-ORACLE-003: Locked before implementation

The oracle for a slice MUST be hashed into a manifest, and that manifest MUST be signed by the oracle identity, before the implementing stage for that slice starts. CogWorks MUST verify that a signed manifest exists and covers the slice before dispatching the implementing stage.

### REQ-ORACLE-004: Locked paths are read-only to the implementer

Locked oracle paths — including test sources, fixtures, snapshots and test-affecting configuration — MUST be mounted read-only for the implementing stage, and MUST be outside its declared writable paths.

### REQ-ORACLE-005: Feedback never carries oracle source

Structured feedback returned to an implementing stage MUST describe what failed, why it matters and what is permitted. It MUST NOT include oracle source, assertions or expected values.

### REQ-ORACLE-006: Disputes rather than edits

An agent that believes an oracle is wrong MUST have a route to raise a dispute, and MUST NOT be able to resolve the disagreement by editing the oracle. CogWorks MUST route a dispute to the oracle stage or to a human, and MUST record its resolution.

---

## REQ-REWORK: Rework and Escalation

### REQ-REWORK-001: Bounded rework

Rework MUST be bounded by the maximum attempt count the stage contract declares. Each rework cycle MUST pass the structured findings from the failed attempt into the next attempt's context bundle, subject to REQ-ORACLE-005 and REQ-VERDICT-004.

### REQ-REWORK-002: Early escalation on a repeated failure class

CogWorks MUST classify attempt failures and MUST escalate early when the same failure class recurs, rather than consuming the remaining attempt budget. A repeating failure class usually indicates that the specification or the oracle is wrong, not the implementation.

### REQ-REWORK-003: Escalation produces a dossier for a human

Escalation MUST produce an escalation issue acting as an evidence dossier for a human: the failure history, the findings from each attempt, any disputes raised with the agent's argument, the resolved profile and harness version, and the context bundle hashes. Escalation MUST NOT start another agent loop.

---

## REQ-TIER: Criticality Tiers

**Level:** L2

### REQ-TIER-001: Tier derived from the risk map

The tier of a piece of work MUST be derived deterministically from the target repository's `risk-map.yaml`, which maps path globs to tiers and is human-owned. The tier of the work is the highest tier among the paths it touches.

### REQ-TIER-002: Raise only

CogWorks MAY raise a derived tier — for example from the affected areas an upstream scoping service reports — and MUST NOT lower one. Lowering a tier MUST require a human.

### REQ-TIER-003: Human confirmation above internal tier

For work at product tier or above, the derived tier MUST be confirmed by a human before the run proceeds past the design stages.

### REQ-TIER-004: Missing or malformed risk map

A missing, malformed or unparseable `risk-map.yaml` MUST halt the run. CogWorks MUST NOT assume the lowest tier, or any tier, in its absence.

---

## REQ-SPEC: Specification Stages

**Level:** L2

### REQ-SPEC-001: Specify stage output

The `specify` stage MUST produce: the confirmed intent, acceptance criteria expressed in a structured form with unique requirement identifiers, the open questions it could not resolve, and a proposed tier with reasoning.

### REQ-SPEC-002: Design stage output is a specification pull request

Each design stage — `specify`, `architect`, `contract` and `plan` — MUST deliver its output as a specification pull request rather than by writing to a branch directly.

### REQ-SPEC-003: Human approval is a pull request review

Approval of a specification pull request MUST be a GitHub review by an authorised human followed by a merge. CogWorks MUST NOT approve, merge or self-certify a specification pull request, and MUST NOT treat its own stage checks as approval.

### REQ-SPEC-004: Requirement identifiers are the traceability spine

Requirement identifiers established by the `specify` stage MUST be carried through every downstream stage: mapped to slices by `plan`, traced by the oracle stages, and used as the unit of coarse feedback (REQ-VERDICT-004).

---

## REQ-ATTEST: Attestations

### REQ-ATTEST-001: Upstream attestations are verified, not recomputed

**Level:** L3

Before acting on a work item, CogWorks MUST verify the signed attestations of the upstream intake, triage and scoping services that the configured trust policy requires, per REQ-BOUND-006. CogWorks MUST NOT repeat the checks those services performed.

### REQ-ATTEST-002: Machine-readable blocks only

**Level:** L3

Where CogWorks consumes an upstream service's output, it MUST read the machine-readable block that service publishes. CogWorks MUST NOT parse prose to recover structured meaning.

### REQ-ATTEST-003: Stage attestations are signed

**Level:** L2

CogWorks MUST produce a signed attestation for every stage attempt, recording what it selected, what the stage saw, and what the stage produced. Attestations MUST be transported as hidden signed comments through the work-item interface.

### REQ-ATTEST-004: Attestation content

**Level:** L2

A stage attestation MUST record at least: the work item and slice, the stage type, the resolved profile and its version, the harness version, the discipline, the language or toolchain, the tier, the context bundle hash, the identity role, the commit SHA produced, and the verdict subsequently read for that SHA.

---

## REQ-ASSURE: Assurance Review Stages

**Level:** L4

### REQ-ASSURE-001: Reviews produce findings, not verdicts

The `review-semantic` and `review-security` stage types MUST produce structured findings. They MUST NOT produce a pass/fail result, and their output MUST NOT be treated as a verdict (REQ-VERDICT-002).

### REQ-ASSURE-002: Reviewers write nothing

An assurance review stage MUST have no writable paths. Its sole output is its structured findings.

### REQ-ASSURE-003: Reviewer visibility

An assurance review stage MUST receive the diff, the specification and the accumulated evidence. It MUST NOT receive the implementing agent's transcript or reasoning, the pull request narrative, holdout content, or secrets.

### REQ-ASSURE-004: Blocking is decided by the gate

Whether a finding blocks is decided by the gate service from the findings and the tier's thresholds. CogWorks MUST surface findings and MUST NOT decide which of them block.

---

## REQ-SPLIT: Repository Split and Holdouts

**Level:** L4

### REQ-SPLIT-001: Repository layout per tier

CogWorks MUST support the repository layout the work item's tier requires: a single repository with protected path zones at the lower tiers, and separate specification, implementation and acceptance repositories at product tier and above.

### REQ-SPLIT-002: Holdouts live outside the implementation repository

Holdout content MUST reside outside the implementation repository, in a private location per deployment. CogWorks MUST NOT place holdout content in a context bundle, a pull request, an issue comment, or an attestation.

### REQ-SPLIT-003: Holdout runs are the gate's

Holdout checks MUST be injected and run by the gate service at gate time, not by CogWorks. CogWorks reads the resulting verdict and relays only the affected requirement identifiers (REQ-VERDICT-004).

---

## REQ-EDGE: Edge Conditions

### REQ-EDGE-001: Edge condition types

The orchestrator MUST support two types of edge conditions: deterministic conditions (evaluated by the orchestrator against pipeline state via a simple expression language) and composite conditions (boolean combinations AND/OR/NOT of deterministic conditions).

Routing MUST be deterministic. No edge condition may be evaluated by an LLM, and no LLM output may select which edge is taken. An LLM produces artifacts; it does not decide control flow.

### REQ-EDGE-002: Edge priority and mutual exclusion

When multiple edges leave the same source node, the orchestrator MUST evaluate them in declared order. The configuration MUST support: `all-matching` (all true edges taken — fan-out), `first-matching` (only first true edge taken — exclusive routing), and `explicit` (the pipeline configuration declares exactly which edges fire for nodes using this mode; the explicit edge list is fixed at configuration load time and stored in `PipelineGraph::explicit_edge_lists`). The evaluation mode MUST be declared per source node.

### REQ-EDGE-003: Rework edges

Edges that create cycles MUST specify a maximum traversal count, which node outputs to preserve vs. discard on re-entry, and whether to increment the retry or rework counter. The orchestrator MUST track traversal counts per cycle and enforce the maximum. When the maximum is reached, the pipeline MUST either halt with an error, escalate to a human, or take a configured overflow edge.

---

## REQ-EXEC: Pipeline Execution

### REQ-EXEC-001: Ephemeral stage attempt workspace

Each stage attempt MUST execute in one fresh sandbox requested through the stage-runner interface. The orchestrator MUST arrange cleanup when the attempt succeeds, fails, or is cancelled. The provider supplies the isolation; CogWorks does not prescribe a specific sandbox platform. Pipeline state MUST be reconstructable from durable work-item and repository records, so loss of a sandbox MUST NOT require restarting the pipeline from scratch.

### REQ-EXEC-002: Pipeline state machine

The orchestrator MUST maintain a state machine for each pipeline run tracking: current active nodes, completed nodes with outputs, pending nodes (inputs not yet available), blocked nodes (upstream dependency failed — inputs will never arrive), failed nodes with error info, per-cycle traversal counts, and cumulative cost. The state machine MUST be representable as a JSON document and MUST be written to a GitHub comment on the parent work item at each node boundary.

### REQ-EXEC-003: Node execution lifecycle

Each node execution MUST follow this lifecycle: (1) precondition check — verify all declared inputs are available, (2) resolve — select the agent profile and load its stage contract (REQ-PROFSEL-001), (3) announce — update pipeline state comment, (4) execute — run the stage attempt in its sandbox, (5) validate — check output against the output schema the harness declares, plus any configured stage pre/post checks, (6) record — write outputs and audit trail, (7) announce — update GitHub state, (8) evaluate edges — evaluate outgoing edge conditions.

Stage pre/post checks bound an attempt; they never constitute a gate verdict and are never published as a check run (REQ-VERDICT-002). Where the resolved graph declares a gate on the node, the node's outcome additionally depends on the gate verdict read per REQ-VERDICT-001.

If execution fails and retries are available, the node re-enters execute with error information in context. If a post-check or gate verdict fails and rework cycles are available, the node re-enters execute with the specific findings in context (REQ-REWORK-001). If retries and rework cycles are both exhausted, the node enters `failed` state.

### REQ-EXEC-004: Pipeline triggering

The pipeline MUST be triggerable through the EventSource abstraction, including manual CLI invocation. Provider-specific readiness events and their mapping to a work item MUST remain behind the GitHub/EventSource interface; this specification does not mandate a particular label, project status, or webhook payload. The orchestrator MUST prevent duplicate pipeline runs for the same work item.

### REQ-EXEC-005: Pipeline cancellation

A running pipeline MUST be cancellable through the EventSource/work-item interface. The provider-specific cancellation signal is not prescribed here. On cancellation: active stage attempts are terminated, current state is persisted through the work-item interface, a summary is posted, and every active sandbox is cleaned up.

### REQ-EXEC-006: Parallel node execution

When the graph has multiple nodes whose inputs are all available simultaneously, the orchestrator MUST support executing them concurrently as async tasks within the orchestrator process. Parallel execution MUST respect the pipeline's total cost budget (shared), the maximum concurrent LLM calls limit (configurable, default: 3), report progress for each node independently, and handle partial failure (other nodes continue unless a failed node is marked `abort_siblings_on_failure: true`). Fan-in occurs when a downstream node declares inputs from multiple upstream nodes; it stays pending until all inputs complete.

### REQ-EXEC-007: Sub-work-item execution within graph

Sub-work-items produced by the `plan` stage MUST be processed as a sub-graph within the pipeline, following the slice sub-graph the resolved stage graph declares. Sub-work-items MUST be processed in topological dependency order. Sub-work-items with no mutual dependency path MAY execute concurrently when the resolved graph allows parallel fan-out.

### REQ-EXEC-008: Stage attempt sandbox lifecycle

For each stage attempt, the orchestrator MUST request exactly one fresh sandbox through the stage-runner interface and MUST request cleanup after success, failure, or cancellation. **A sandbox MUST NOT be reused across attempts.** Every retry, rework cycle or resumed attempt gets a new sandbox, so that no state, artifact or credential from a previous attempt can survive into the next one. The provider interface MUST isolate CogWorks' orchestration contract from the selected isolation technology or hosting provider. These requirements do not prescribe provider-specific APIs or sandbox configuration.

### REQ-EXEC-009: Persist before sandbox cleanup

Before requesting cleanup of a sandbox on a terminal path, the orchestrator MUST attempt to persist the latest recoverable pipeline state and deliverables through the work-item and repository interfaces. If persistence fails, the orchestrator MUST still request sandbox cleanup, record the persistence failure through available operational logging, and resume only from the last successfully persisted state. Sandbox-local state MUST NOT be treated as durable recovery data.

---

## REQ-CLASS: Task Classification

### REQ-CLASS-001: Classification schema

The intake step MUST produce structured output conforming to the classification schema: task type (enum), affected modules (list), estimated scope, proposed tier, and rationale.

### REQ-CLASS-003: Scope threshold

When the estimated scope exceeds the configured threshold, the intake step MUST produce an escalation result and MUST NOT proceed to subsequent nodes.

_Note: Requirement ID REQ-CLASS-002 is retired and MUST NOT be reused. Tier derivation replaces the safety-critical module registry; see REQ-TIER-001._

---

## REQ-ARCH: Architect Stage

**Level:** L4

### REQ-ARCH-001: Architectural context

The context bundle for the `architect` stage MUST include the relevant ADRs, coding standards and architectural constraints its profile's input allowlist declares (REQ-BUNDLE-001).

### REQ-ARCH-002: Architect stage output

The `architect` stage MUST produce: one or more ADRs, the non-functional requirements for the work, a threat-model delta, and a patch to the repository's `risk-map.yaml` where the work changes the tier of any path.

### REQ-ARCH-003: Specification delivered as PR

The `architect` stage's output MUST be delivered as a specification pull request targeting the repository that owns the specification paths for the work item's tier (REQ-SPLIT-001).

### REQ-ARCH-004: Specification gate

The pipeline MUST NOT advance past the `architect` node until the specification pull request has the approvals its tier requires and the corresponding gate verdict is green (REQ-VERDICT-001).

### REQ-ARCH-005: Architect stage pre/post checks

The `architect` stage MUST run deterministic pre/post checks verifying that all referenced modules exist in the repository (or are explicitly marked new) and that proposed dependency changes do not violate the project's declared architectural constraints. These checks bound the attempt and MUST NOT be published as a check run (REQ-VERDICT-002).

---

## REQ-IFACE: Contract Stage

**Level:** L2

### REQ-IFACE-001: Context from approved specification

The context bundle for the `contract` stage MUST be built from the approved specification, the existing interface conventions in the repository, and any cross-domain interface registry entries relevant to the affected modules, as declared by its profile's input allowlist.

### REQ-IFACE-002: Structured contract output

The `contract` stage MUST produce a program design — types, signatures, error model, call-tree and file-tree diff — together with compilable stubs in the target discipline's artifact format. Partial or placeholder stubs MUST be rejected against the stage's output schema.

### REQ-IFACE-003: Contract stage pre/post checks

Generated stubs MUST be checked for syntactic validity and interface-convention conformance before the stage's output is accepted. Failures MUST be fed back as structured findings within the stage's rework budget (REQ-REWORK-001). Whether the contract passes is decided by its gate verdict, not by these checks (REQ-VERDICT-002).

### REQ-IFACE-004: Retry budget

Each `contract` stage attempt MUST be counted against the retry budget its stage contract declares (REQ-PROFSEL-002). Exceeding the retry budget MUST trigger escalation with a summary of all attempts and their failure findings.

### REQ-IFACE-005: Delivered as a Pull Request

The `contract` stage's output MUST be delivered as a specification pull request. The pull request MUST reference the parent work item and the preceding specification pull requests.

### REQ-IFACE-006: Contract gate

The pipeline MUST NOT advance to the `plan` node until the contract pull request has the approvals its tier requires and the corresponding gate verdict is green.

### REQ-IFACE-007: Cross-domain registry consistency

Where the affected modules declare cross-domain interfaces in the repository's specification paths, the `contract` stage MUST verify that the generated definitions are consistent with the declared registry contracts. Inconsistencies MUST be reported as structured findings before the pull request is opened.

---

## REQ-PLAN: Plan Stage

**Level:** L2

### REQ-PLAN-001: Plan schema

The `plan` stage MUST produce a `plan.yaml` describing vertical slices, each with a title, description, size estimate, file list, contract references, requirement-ID mapping and dependency references, together with the dependency graph over those slices.

### REQ-PLAN-002: Slice minimum

A plan MUST contain at least one slice. A plan with zero slices is invalid and MUST be rejected against the stage's output schema.

### REQ-PLAN-003: Dependency graph validation

CogWorks MUST compute a topological ordering of the plan's slices. Circular dependencies MUST be detected and the specific cycle returned as a structured finding within the stage's rework budget.

### REQ-PLAN-004: Granularity limit

CogWorks MUST enforce a configurable maximum number of slices per work item (default: 10), and MUST enforce the per-slice size budget the plan declares. Exceeding either limit MUST trigger escalation.

### REQ-PLAN-005: Sub-issue creation

Each slice of an **approved** plan MUST be created as a native GitHub sub-issue of the parent work item, in dependency order, with the required labels applied. Dependencies between sub-issues MUST be expressed via native GitHub typed issue links (`blocks` / `is blocked by`). Sub-issues MUST NOT be created from an unapproved plan.

### REQ-PLAN-006: Requirement coverage

CogWorks MUST verify that every requirement ID established by the `specify` stage is mapped to at least one slice, and that every contract produced by the `contract` stage is covered by at least one slice.

### REQ-PLAN-007: Milestone inheritance

When the parent work item has a GitHub Milestone set, all sub-issues created from the plan MUST inherit the same milestone.

---

## REQ-CODE: Implement Stage

### REQ-CODE-001: Dependency-ordered processing with prior context

Slices MUST be processed in topological dependency order. Each slice's context bundle MUST include the implementation outputs of the prior completed slices it depends on, where its profile's input allowlist declares them. Slices with no mutual dependency path MAY execute concurrently when the resolved graph allows parallel fan-out; each concurrent slice receives the outputs of its own dependency chain.

### REQ-CODE-002: Structured feedback loop

CogWorks MUST feed structured findings back to the agent on failure. Findings MUST describe what failed, why it matters and what is permitted; they MUST NOT include oracle or test source (REQ-ORACLE-005). Feedback on hidden checks MUST be limited to the affected requirement IDs (REQ-VERDICT-004).

### REQ-CODE-003: Retry budget

Each slice MUST have a maximum attempt count taken from the stage contract its resolved profile declares (REQ-PROFSEL-002). Exceeding it MUST trigger escalation with a summary of all attempts and their failure reasons.

### REQ-CODE-004: Cost budget

The pipeline MUST track accumulated LLM token cost and halt when the pipeline budget is exceeded. The halt MUST include a per-node, per-sub-work-item cost report.

### REQ-CODE-005: Context truncation

Context bundle assembly MUST apply deterministic priority-based truncation when the assembled bundle would exceed the model's context window. The current slice's contract definition MUST never be removed regardless of truncation pressure. Truncation MUST NOT be used to admit an input the profile's allowlist excludes, nor to drop an input the allowlist marks as required.

### REQ-CODE-006: Semantic stalling detection

CogWorks MUST track failure classes across consecutive attempts for each slice. If the same failure class recurs for a configurable number of consecutive attempts (default: 3), CogWorks MUST escalate with a structured stalling report rather than consuming the remaining attempt budget. A repeating failure class usually means the specification or the oracle is wrong rather than the implementation, so escalating early is correct. The stalling report MUST include the recurring failure class and a summary of all attempts. This is distinct from flaky test detection (EDGE-015), which covers identical code producing different results.

### REQ-CODE-007: Oracle precedes implementation

The oracle for a slice MUST exist, be locked and be signed before the `implement` stage for that slice starts (REQ-ORACLE-003). The oracle and the implementation MUST be produced by separate stages, under separate identities, in separate sandboxes (REQ-ORACLE-001). CogWorks MUST NOT run a stage that produces both the oracle and the implementation for the same slice.

The `implement` stage then produces the change, and iterates on structured gate feedback within its attempt and rework budgets. This contract applies to software and to other engineering disciplines; what constitutes an oracle is supplied by the discipline pack.

---

## REQ-INT: Integration (PR Creation)

### REQ-INT-001: Non-blocking findings as inline comments

Non-blocking findings MUST be posted as inline review comments on the pull request at the relevant file and line number.

### REQ-INT-002: PR traceability references

Every slice pull request opened by CogWorks MUST include references to: the slice sub-issue, the parent work item, the specification pull requests, the resolved harness version, and the context bundle hash for each stage attempt that contributed to it.

### REQ-INT-003: CogWorks opens, the runner pushes

CogWorks MUST open and update pull requests, and MUST NOT push commits. Commits are pushed into the branch by the stage runner under the stage's identity role (REQ-SANDBOX-002). CogWorks MUST NOT hold a credential capable of pushing to the target repository (REQ-BOUND-007).

---

## REQ-AUDIT: Audit Trail

### REQ-AUDIT-001: LLM call recording

Every LLM call MUST be recorded in the audit trail with: model name, input token count, output token count, latency, node, and sub-work-item identifier (if applicable).

### REQ-AUDIT-002: State transition recording

Every pipeline state transition (node entry, node completion, gate evaluation) MUST be recorded in the audit trail.

### REQ-AUDIT-003: Failure reporting

When a node fails, CogWorks MUST post a structured failure report as a GitHub issue comment and apply the `cogworks:node:failed` label.

### REQ-AUDIT-004: Tracing observability fields

CogWorks MUST record pipeline lifecycle, retry, rework, and cost fields through its structured tracing instrumentation. The CLI MUST route tracing output to structured console logging and MAY export spans through the configured OpenTelemetry exporter. This requirement does not mandate a custom metric-sink abstraction or guarantee derived metrics that are not represented by recorded tracing fields.

### REQ-AUDIT-005: Tracing field completeness

Tracing fields MUST use stable names for the pipeline run identifier, node identifier, event kind, and any event-specific values defined by the observability interface. External systems MAY derive and aggregate metrics from available spans and events. CogWorks MUST NOT claim that dimensions or measurements absent from those records are emitted.

### REQ-AUDIT-006: Tracing-based observability

CogWorks MUST use its existing tracing instrumentation and OpenTelemetry exporter path for external observability. Backend selection and aggregation are handled by the configured OpenTelemetry collector/exporter and external monitoring systems; CogWorks MUST NOT introduce a separate metric-sink interface or implement metrics storage, aggregation, or dashboarding.

---

## REQ-BOUND: System Boundaries

### REQ-BOUND-001: No code or agent execution within CogWorks

CogWorks MUST NOT execute generated code or LLM output within its own process. CogWorks MUST NOT run an agent in its own process or container: every agent turn executes inside a sandbox obtained through the stage-runner interface. CogWorks MUST NOT hold virtualisation privileges or host access.

### REQ-BOUND-002: No PR merging

CogWorks MUST NOT merge, approve, close, or request changes on any Pull Request. CogWorks does not own releases or artifact publication. PR lifecycle decisions and any external factory/GateKeeper approvals belong outside CogWorks and are passed through the work-item interface.

### REQ-BOUND-003: No metrics storage

CogWorks MUST NOT implement its own metrics database, time-series store, or dashboarding. Metrics storage, aggregation, trend analysis, and dashboarding are delegated to external purpose-built tools.

### REQ-BOUND-004: External status and approval pass-through

CogWorks MUST consume readiness, implementation-stage, and external approval status through the work-item interface. The mapping of provider-specific states (including a possible ready-for-implementation stage) MUST remain behind that interface and MUST NOT be fixed by this specification. CogWorks MUST report pipeline outcomes through the same interface without performing external approval, merge, release, or artifact-publication actions.

### REQ-BOUND-005: Credential provider boundary

Authentication and key lifecycle MUST be handled behind the credential-provider interface. CogWorks MUST rely on that provider to resolve credentials for infrastructure clients and MUST NOT prescribe the secret store's authentication or rotation details. Key material MUST NOT be included in context bundles or prompts, audit records, generated artifacts, or logs.

### REQ-BOUND-006: Attestation verification and signing

Before acting on a work item, CogWorks MUST verify any upstream attestation required by the configured trust policy through an attestation-provider interface. Missing, invalid, or content-mismatched required attestations MUST fail closed: CogWorks MUST NOT process the work item and MUST record a non-secret failure outcome. CogWorks MUST sign its own action attestations through the attestation-provider boundary and publish them as comments through the work-item interface. Cryptographic implementation and key access are delegated to the provider; the specific signature algorithm and provider protocol are not prescribed here. The attestation provider uses the credential boundary for signing operations.

### REQ-BOUND-007: No push tokens, no model API keys

CogWorks MUST NOT hold a credential capable of pushing commits to a target repository, and MUST NOT hold a model provider API key. Commits are pushed by the stage runner under the stage's identity role. Model credentials are held by the model gateway; CogWorks and every sandbox reach models only through that gateway endpoint.

### REQ-BOUND-008: No verdict authorship

CogWorks MUST NOT compute a pass/fail verdict, MUST NOT author or publish a check run, and MUST NOT configure which checks are required. Verdicts are produced by the gate service and read per REQ-VERDICT-001.

### REQ-BOUND-009: No cross-issue ordering

CogWorks MUST NOT decide the priority or relative ordering of work items. It takes the next work item it is given and runs it. Concurrency limits on how many work items CogWorks processes at once are its own configuration and are not an ordering decision.

### REQ-BOUND-010: No agent-writable policy

CogWorks MUST NOT write to the harness, to the repository's `risk-map.yaml`, or to `.factory/`. Where a stage's output proposes a change to any of these (for example the `architect` stage's risk-map patch), that change MUST be delivered as a pull request for human review, never applied directly.

---

## REQ-LABEL: Label Configuration

### REQ-LABEL-001: Configurable workflow labels

Workflow-semantic labels and signals (trigger, awaiting-review, hold, cancel, slice) MUST have configurable mappings in CogWorks' own process configuration. Business logic MUST NOT depend on a particular label string or project status. A work item's tier MUST NOT be taken from a label (REQ-TIER-001).

### REQ-LABEL-002: Pipeline-internal labels are not configurable

Pipeline-internal labels (`cogworks:processing`, `cogworks:node:<name>`, `cogworks:node:failed`, `cogworks:status:<status>`) MUST always use the `cogworks:` prefix and MUST NOT be configurable. Restart signals are workflow-semantic adapter mappings, not pipeline-internal labels.

### REQ-LABEL-003: Label configuration validation

Label configuration MUST be validated at load time. If two workflow labels are configured to the same string, the system MUST produce a clear error and halt. Additionally, if any workflow label is configured to the same string as a pipeline-internal label (e.g., `cogworks:processing`, `cogworks:node:*`), the system MUST produce a clear error and halt — the same GitHub label string carrying two distinct semantic meanings causes undefined pipeline behavior.

---

## REQ-EXT: Extension API (Domain Services)

### REQ-EXT-001: Health check before invocation

CogWorks MUST perform a health check on a domain service before each pipeline run that requires it. Unhealthy services MUST be detected before any domain service method is called.

### REQ-EXT-002: Unsupported method handling

A domain service method invocation that the service does not support MUST return a clear error identifying the unsupported method. This MUST be treated as a non-retryable error.

### REQ-EXT-003: Response schema validation

All domain service responses MUST be validated against the Extension API JSON Schema before being used. Responses that do not conform MUST be rejected.

### REQ-EXT-004: Operation timeouts

Every Extension API method call MUST have a configurable timeout. Operations exceeding the timeout MUST be terminated and reported as failures (default: 10 minutes for `simulate`, 5 minutes for all other methods).

### REQ-EXT-005: Transport abstraction

The domain service communication layer MUST be transport-agnostic. Unix domain sockets are the default transport; HTTP/gRPC MUST be supported as an alternative.

### REQ-EXT-006: Long-running operation handling

The Extension API baseline is synchronous request-response with configurable timeouts. The protocol design MUST NOT preclude adding progress polling (via operation IDs) or streaming transport in a future API version. When progress polling is added, domain services that support it MUST declare the capability in their handshake response.

### REQ-EXT-007: Service availability policies

Primary domain service unavailability MUST halt the pipeline with a clear diagnostic. Secondary domain service unavailability (services that would only participate in cross-domain validation) MUST produce a warning and allow the pipeline to continue with that validation skipped.

### REQ-EXT-008: Structured diagnostics

All domain service diagnostic output MUST be structured data: artifact identifier, location (domain-specific JSON object), severity (`blocking`/`warning`/`informational`), category (from the standardised diagnostic category set), and message. CogWorks MUST NOT parse free-form text from domain services.

### REQ-EXT-009: API version compatibility

Domain services declaring an Extension API version incompatible with CogWorks MUST be rejected during health check. The rejection MUST identify the version mismatch clearly.

---

## REQ-XDOM: Cross-Domain Interface Registry

**Level:** L4

### REQ-XDOM-001: Human-authored only

Cross-domain interface definitions live in the repository's specification paths and MUST be authored and maintained by humans. CogWorks MUST NOT create or modify interface definitions autonomously. CogWorks MAY suggest additions as recommendations for human review.

### REQ-XDOM-002: Schema conformance

All interface definition files MUST conform to the published interface definition JSON Schema. Non-conformant files MUST be rejected with a clear error identifying the specific violation.

### REQ-XDOM-003: Computed constraints

Derived constraints (such as total bus load computed from message parameters) MUST be evaluated deterministically by the constraint validator. Computed constraint formulas MUST NOT be stored in the registry.

### REQ-XDOM-004: Version mismatch detection

The Interface Registry Manager MUST detect and report mismatches between the interface version declared in the registry and the interface version a domain service declares compatibility with.

### REQ-XDOM-005: Pre-pipeline validation

The interface registry MUST be validated on every pipeline run, before any node executes. Registry validation failures MUST prevent any pipeline node from running.

---

## REQ-XVAL: Cross-Domain Constraint Validation

**Level:** L4

### REQ-XVAL-001: Deterministic and token-free

Cross-domain constraint validation MUST be deterministic and MUST NOT consume LLM tokens. It runs as a stage pre/post check and therefore bounds an attempt rather than producing a verdict (REQ-VERDICT-002).

### REQ-XVAL-002: Interface extraction

The constraint validator MUST use the domain service's `extract_interfaces` method to obtain actual interface values from generated artifacts before comparing against registry contracts.

### REQ-XVAL-003: Severity levels

Hard constraint violations (values outside declared min/max bounds) MUST produce blocking findings. Nominal deviations (within bounds but outside the declared nominal range) MUST produce warnings.

### REQ-XVAL-004: Single-service operation

Cross-domain constraint validation MUST be able to validate a single domain's artifacts against registry contracts without requiring other participating domains' services to be running.

### REQ-XVAL-005: Architect-stage validation

Cross-domain constraint validation MUST also run as a pre/post check on the `architect` stage, to catch violations before implementation begins (REQ-ARCH-005).

---

## REQ-CPACK: Context Pack System

### REQ-CPACK-001: Pack trigger mechanism

Context Pack loading MUST be driven deterministically by the work item's tier, discipline, stage type and detected language or toolchain. The LLM MUST NOT choose which packs to load.

### REQ-CPACK-002: Pack source and loading timing

Context Packs MUST be loaded from the harness at the pinned version (REQ-HARNESS-001). The target repository MUST NOT supply Context Packs: in-repository pack content would be prompt material that an agent working in that repository could edit.

Packs MUST be loaded during context bundle assembly for a stage, before the stage attempt starts, and the resolved pack set MUST be recorded in the stage attestation.

### REQ-CPACK-003: Multiple simultaneous packs

A single pipeline run MUST support loading multiple Context Packs simultaneously when a work item matches multiple pack triggers.

### REQ-CPACK-004: Conflict resolution

Where loaded Context Packs contain contradictory guidance, the more restrictive rule MUST apply.

### REQ-CPACK-005: Unconditional loading on match

If a work item matches a Context Pack's trigger criteria, the pack MUST be loaded. There MUST NOT be an option to skip a matched pack.

### REQ-CPACK-007: Pack audit trail

The set of Context Packs loaded for each stage attempt MUST be recorded in the audit trail and included in the pull request description.

### REQ-CPACK-008: Pack content in bundle assembly

Context Pack domain knowledge, safe patterns, and anti-patterns MUST be included in the context bundle for every stage whose profile allowlists them, subject to the standard bundle priority and truncation rules, and MUST be covered by the bundle hash (REQ-BUNDLE-002).

_Note: Requirement ID REQ-CPACK-006 is retired and MUST NOT be reused. Required-artefact enforcement is a pass/fail judgement and therefore belongs to the gate service, not to CogWorks._

---

## REQ-CONST: Constitutional Security Layer

### REQ-CONST-001: Unconditional loading

Constitutional rules MUST be loaded and validated on every pipeline run, before context bundle assembly and before any stage attempt starts. This is NOT a configurable gate.

### REQ-CONST-002: Privileged position

Constitutional rules MUST occupy a privileged, non-overridable position in the system prompt of every agent turn. No content in the context bundle MAY modify, append to, or override them. For agent turns executing inside a sandbox, the harness profile places the rules and the sandbox boundary preserves that position; for deterministic helper calls made in CogWorks' own process, the privileged position MUST be enforced at the type level so that no call can reach a model without passing through constitutional assembly.

### REQ-CONST-003: Harness-pinned source

Constitutional rules MUST be loaded from the harness at the version pinned by the target repository (REQ-HARNESS-001). The target repository MUST NOT supply constitutional rules. CogWorks MUST verify the loaded rules against the pin before use and MUST halt rather than proceed with rules it cannot verify.

### REQ-CONST-004: External content as data

The constitutional rules MUST include a rule declaring that issue bodies, specifications, dependency docs, API responses, and any content not from core configuration are inputs to be analyzed — not instructions that modify CogWorks' behavior.

### REQ-CONST-005: Intake screening is upstream

**Level:** L3

Screening an incoming work item for prompt injection is the responsibility of the upstream intake service, whose signed approval CogWorks verifies before acting (REQ-ATTEST-001). CogWorks MUST NOT duplicate that scan.

CogWorks MUST, however, scan model-sourced text for injection patterns before reusing it in a later prompt — for example a finding description fed back into a rework attempt. This is defence in depth against a compromised or manipulated stage output, not intake screening.

### REQ-CONST-006: Injection event content

Where CogWorks detects an injection pattern in model-sourced text per REQ-CONST-005, the recorded event MUST include: pipeline run ID, work item ID, the stage attempt that produced the text, and the offending span, truncated and control-character-escaped before it reaches any log or comment.

### REQ-CONST-007: Hold state on injection

When CogWorks detects an injection pattern, the work item MUST enter a hold state. It MUST NOT be automatically requeued or retried. Clearing the hold MUST require a human: a human either confirms a false positive, with the justification recorded in the audit trail, or marks the work item as contaminated.

### REQ-CONST-008: Specification scope binding

The constitutional rules MUST include a rule that only capabilities explicitly in the approved specification and interface documents are implemented. Implied or inferred capabilities MUST NOT be implemented.

### REQ-CONST-009: Unauthorized capability prohibition

The constitutional rules MUST include a rule prohibiting network calls, file system access, IPC mechanisms, external process invocations, or hardware access unless explicitly specified in the interface document.

### REQ-CONST-010: No credential generation

The constitutional rules MUST include a rule that no strings resembling credentials, API keys, tokens, passwords, or secrets appear in any output artefact.

### REQ-CONST-011: Scope underspecification detection

When fulfilling a work item would require capabilities not in the approved specification, CogWorks MUST emit a `SCOPE_UNDERSPECIFIED` event and halt generation.

### REQ-CONST-012: Scope ambiguity detection

When a specification is ambiguous for a safety-affecting work item, CogWorks MUST emit a `SCOPE_AMBIGUOUS` event and require human clarification before proceeding.

### REQ-CONST-013: Protected path enforcement

CogWorks MUST NOT create or modify files matching protected path patterns — the harness, `risk-map.yaml`, `.factory/`, locked oracle paths, and the repository's agent-instruction files — through the normal pipeline (REQ-BOUND-010). Where a stage's writable paths are declared by its stage contract, CogWorks MUST pass those paths to the mount-plan provider rather than enforcing them itself; the authoritative enforcement is the sandbox mount plan and the commit-range scope check.

---

## REQ-ALIGN: Alignment Verification

### REQ-ALIGN-001: Alignment verification step

Every LLM node in the pipeline MUST have an alignment verification step that runs as part of validation (REQ-EXEC-003, step 4) — after schema validation and domain service validation, but before the output is accepted. The alignment verification step compares the node's output against its upstream inputs and reports any misalignment. Alignment verification checks that the output addresses the intent of the input — not just structural or syntactic correctness. The alignment checker MUST be a different LLM call from the generating node (different prompt, and SHOULD use a different model — see REQ-ALIGN-004).

### REQ-ALIGN-002: Alignment finding structure

Each alignment check MUST produce a structured result containing: an `aligned` boolean, a numerical `score` (0.0–1.0), the configured `threshold`, and a list of `findings`. Each finding MUST include: a `type` (one of `missing`, `extra`, `modified`, `ambiguous`, `scope_exceeded`), a `severity` (`blocking`, `warning`, `informational`), a human-readable `description`, an `input_reference` (source and location in the upstream input), an `output_reference` (source and location in the node output), and an optional `suggestion` for remediation.

Finding type definitions:

- `missing`: Something required by the input is absent from the output
- `extra`: Something present in the output was not requested by the input
- `modified`: Something in the output contradicts or changes what the input specified
- `ambiguous`: The input is unclear and the output made an assumption that should be confirmed
- `scope_exceeded`: The output addresses concerns beyond the scope of the work item

### REQ-ALIGN-003: Alignment score

Each alignment check MUST produce a numerical alignment score between 0.0 and 1.0. The score is produced by the LLM alignment check as part of its assessment. The alignment check passes if the score meets or exceeds a configurable threshold (default: 0.90) AND there are no `blocking` findings.

### REQ-ALIGN-004: Alignment check types

Alignment checks MUST use an appropriate mix of two categories:

**Deterministic alignment checks** compare structural properties of the output against the input. These are fast, cheap, and not subject to LLM bias. Examples: function name matching between interfaces and generated code, value preservation across stages, coverage checking (every interface covered by a sub-work-item), file-to-module mapping.

**LLM alignment checks** use an LLM call to assess semantic alignment that cannot be reduced to structural comparison. The LLM alignment prompt MUST be focused and adversarial: "Here is what was requested. Here is what was produced. Identify every way the output fails to match the request." LLM alignment checks SHOULD use a different model from the generating node to reduce correlated bias (see CW-R02).

### REQ-ALIGN-005: Retry vs. rework distinction

The pipeline MUST distinguish between retries and rework:

- **Retry**: The output is technically invalid (doesn't compile, doesn't parse, schema validation failure). Same input, try again. Counted against the node's retry budget.
- **Rework**: The output is technically valid but doesn't match the intent of the input (alignment verification failure). The node is re-entered with the specific misalignment findings as additional context. Counted against the node's rework budget (separate from retry budget).

Retries and rework MUST have separate counters, separate budgets, and separate context strategies. The distinction MUST be maintained in the pipeline state and audit trail.

### REQ-ALIGN-006: Rework budget exhaustion

When a node's rework budget is exhausted (all configured rework cycles have been attempted and alignment still fails), the pipeline MUST:

1. Record the final alignment findings in the audit trail, tagged as rework-exhaustion
2. Post a detailed alignment failure comment on the work item issue, including: the node name, the number of rework cycles attempted, and the alignment findings from the final attempt
3. Mark the node as failed with a structured error that distinguishes rework exhaustion from retry exhaustion
4. Halt the pipeline — the pipeline MUST NOT silently pass or skip the node after rework exhaustion

Rework exhaustion is a distinct failure mode from retry exhaustion. The structured error MUST include the failure category (`rework_exhausted` vs `retry_exhausted`), enabling downstream tooling and human reviewers to distinguish "couldn't produce valid output" from "couldn't produce aligned output".

_Note: Requirement IDs 007–009 are reserved for future per-stage rework specialization requirements._

### REQ-ALIGN-010: Work item to architecture alignment

The alignment check between the work item and the architecture specification MUST verify:

**Deterministic checks:**

- Specific values mentioned in the work item (numbers with units) appear in the specification
- Components or modules named in the work item are addressed in the specification
- Explicit exclusions in the work item ("do not change X", "out of scope: Y") are respected — the specification does not modify excluded items

**LLM alignment check** — a single, focused LLM call that assesses:

- Problem-solution alignment: Does the spec solve the problem described in the work item, or a related but different problem?
- Completeness: Are there aspects of the work item that the spec does not address at all?
- Scope creep: Does the spec introduce capabilities, components, or complexity not justified by the work item?
- Assumption surfacing: Has the spec made assumptions about the work item's intent that should be confirmed?
- Difficulty avoidance: Has the spec simplified or omitted the hardest parts of the requirement?

### REQ-ALIGN-011: Architecture to interface alignment

The alignment check between the architecture specification and the interface definitions MUST verify:

**Deterministic checks:**

- Data types referenced in the spec are defined in the interfaces
- Cross-domain interfaces identified in the spec are declared in the interface definitions

**LLM alignment check** — assesses:

- Interaction completeness: Do the interfaces cover all interactions described in the architecture spec?
- Abstraction level: Are the interfaces at the right level of abstraction?
- Missing error paths: Does the architecture spec describe failure modes that the interfaces lack error types for?

### REQ-ALIGN-012: Spec and interfaces to plan alignment

The alignment check between the architecture spec, interfaces, and the sub-work-item plan MUST verify:

**Deterministic checks:**

- Every module in the architecture spec is covered by at least one sub-work-item
- Every interface is implemented by at least one sub-work-item (extends existing REQ-PLAN-006)
- Sub-work-item dependencies are consistent with the architecture (if the architecture says "module A depends on module B", the sub-work-item implementing A depends on the sub-work-item implementing B)
- No sub-work-item addresses components outside the architecture spec scope

**LLM alignment check** (optional, default: off — planning alignment is mostly structural) — assesses:

- Decomposition appropriateness: Are the sub-work-items decomposed along the boundaries the architecture spec defines?
- Ordering rationale: Does the dependency order make sense given the architecture's module interactions?

### REQ-ALIGN-013: Plan and interfaces to code alignment

The alignment check between the sub-work-item description, interface definitions, and generated code MUST verify:

**Deterministic checks:**

- Every function/method declared in the relevant interfaces exists in the generated code
- Function signatures match interface declarations (parameter types, return types, error types)
- Public API surface matches the interface — no undeclared public items (using domain service's `extract_interfaces` method)
- Specific constants and values from the spec appear in the code
- Test cases exist for each acceptance criterion in the sub-work-item description

**LLM alignment check** — assesses:

- Algorithm alignment: Does the code implement the approach described in the spec, not a superficially similar alternative?
- Behavioural alignment: Will the code behave as the spec describes under the specified conditions?
- Constraint adherence: Does the code respect constraints mentioned in the spec (memory limits, timing requirements, safety invariants)?
- Scope discipline: Does the code implement only what the sub-work-item specifies, without adding unrequested features?
- Error handling alignment: Does the code handle error conditions described in the spec and interfaces using the declared error types?

### REQ-ALIGN-015: End-to-end alignment check

After all sub-work-items are complete and before the pipeline reports success, a final end-to-end alignment check MUST verify that the aggregate output addresses the original work item. This check runs as a pipeline-level validation step after the terminal node (Integration) completes — it is not a separate node in the pipeline graph but a pipeline executor responsibility triggered on successful pipeline completion. This check is necessary because small acceptable deviations at each stage can compound into significant drift from the original intent.

**Deterministic checks:**

- Every acceptance criterion from the work item has a traceability chain: it appears in the architecture spec → maps to an interface → maps to a sub-work-item → the sub-work-item's code was merged
- All sub-work-items completed successfully
- No scope creep across the pipeline: files modified across all sub-work-item PRs are within the affected modules declared in the architecture spec

**LLM alignment check** — assesses:

- Requirement satisfaction: Would the delivered changes, taken together, satisfy the person who wrote the work item?
- Missing pieces: Is there anything the work item asked for that was not delivered and was not explicitly deferred?
- Accumulated drift: Have the small decisions made at each stage moved the final output away from the original intent?

### REQ-ALIGN-020: Alignment check configuration

Each LLM node MUST support an alignment configuration block specifying: which upstream inputs to check against, whether deterministic checks are enabled, whether the LLM alignment check is enabled, the alignment score threshold, and the maximum rework cycles. The alignment configuration is part of the node definition in the pipeline configuration.

### REQ-ALIGN-021: Alignment failure handling

When an alignment check fails (score below threshold or any blocking finding), the orchestrator MUST:

1. Record the alignment findings in the audit trail
2. Check the rework cycle count against the configured maximum
3. If rework cycles remain: loop back to the generating node with the specific misalignment findings appended to context; increment the rework counter (separate from the retry counter)
4. If rework cycles are exhausted: post a detailed alignment failure comment on the work item issue, mark the node as failed, and halt the pipeline or escalate to a human

### REQ-ALIGN-022: Alignment check cost management

LLM alignment checks MUST be counted against the node's cost budget (REQ-CODE-004). The budget check MUST occur before the LLM alignment call is invoked — if the remaining budget is insufficient, the alignment check MUST fail with a structured budget-exceeded error rather than proceeding and exceeding the budget. When estimating node cost budgets, alignment check overhead MUST be accounted for — typically one additional LLM call per node execution. Deterministic alignment checks have negligible cost and are not counted. The pipeline configuration MUST support disabling LLM alignment checks per node while retaining deterministic checks.

### REQ-ALIGN-030: Traceability matrix

The end-to-end alignment check (REQ-ALIGN-015) MUST produce a traceability matrix as an artifact. The traceability matrix maps each requirement from the work item through the pipeline stages to the final deliverable. Each row traces: Requirement → Architecture spec section → Interface definition → Sub-work-item → Code file → Status (satisfied / not addressed / N/A).

The traceability matrix MUST be:

- Generated automatically from the alignment check results at each stage
- Posted as a comment on the work item issue at pipeline completion
- Included in the audit trail
- Available to the assurance review stages and to the gate service as evidence

### REQ-ALIGN-031: Traceability matrix accumulation

The traceability matrix MUST be built incrementally as the pipeline progresses. Each alignment check adds its columns:

- After Architecture alignment check: the "Architecture" column is populated
- After Interface alignment check: the "Interface" column is populated
- After Planning alignment check: the "Sub-Work-Item" column is populated
- After Code Generation alignment check: the "Code" column is populated
- After end-to-end alignment check: the "Status" column is computed

If a requirement is not addressed at a stage (e.g., a mechanical requirement in a firmware pipeline), the cell is marked "N/A" with a reason.

### REQ-ALIGN-040: Default alignment configuration

If a node has no explicit alignment configuration, the orchestrator MUST apply defaults appropriate to the node type:

| Stage type | Default behaviour |
| ---------- | ----------------- |
| `intake` | No alignment check (first step — nothing to align against) |
| `specify` | Deterministic + LLM alignment against the work item. Score threshold: 0.90. Max rework: 3. |
| `architect` | Deterministic + LLM alignment against the confirmed intent. Score threshold: 0.90. Max rework: 3. |
| `contract` | Deterministic + LLM alignment against the specification. Score threshold: 0.90. Max rework: 3. |
| `plan` | Deterministic alignment against the specification + contracts. LLM check: off. Score threshold: 0.85. Max rework: 2. |
| `unit-oracle`, `acceptance-oracle` | Deterministic alignment against the slice + contracts (requirement-trace completeness). LLM check: off. Max rework: 2. |
| `implement`, `refactor` | Deterministic + LLM alignment against the slice + contracts. Score threshold: 0.90. Max rework: 3. |
| `review-semantic`, `review-security` | No alignment check — these stages produce findings, not artifacts. |
| End-to-end (pipeline-level) | Deterministic + LLM alignment against the original work item. Score threshold: 0.90. Not a graph node — runs as a pipeline post-completion step (REQ-ALIGN-015). |

Where a stage's resolved profile declares its own alignment configuration, that configuration takes precedence over this table.

### REQ-ALIGN-041: Tier impact on alignment

Alignment checks MUST become stricter as the work item's tier rises (REQ-TIER-001):

| Parameter | T0–T1 | T2 | T3 |
| --------- | ----- | -- | -- |
| Score threshold | 0.90 | 0.95 | 0.95 |
| Max rework cycles | 3 | 5 | 5 |
| LLM alignment check | Configurable (default: on) | Always on | Always on |
| End-to-end alignment check | Default on | Always on | Always on |
| Traceability matrix | Generated | Generated | Generated and requires human sign-off |
| Blocking finding tolerance | 0 (any blocking finding fails) | 0, and more than 3 warnings also fails | 0, and any warning fails |

Where the resolved profile for a stage declares stricter values than this table, the profile's values apply. A tier's values MUST NOT be weakened by a profile or by any repository-level configuration.
