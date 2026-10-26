# Functional Requirements

This document defines the functional requirements for CogWorks. Every requirement has a unique ID that is referenced in `assertions.md` to establish traceability between requirements and testable behavioral assertions.

Requirements are organized by functional area. Each requirement states what the system **MUST** or **MUST NOT** do. Implementation constraints (how it must be done) are in `constraints.md`.

---

## REQ-PIPE: Pipeline Execution

### REQ-PIPE-001: Trigger on work-item readiness

CogWorks MUST initiate the pipeline when its EventSource reports that a work item is ready for processing and the work item has no current active-node state. The readiness signal and its mapping from provider events or work-item state MUST be handled behind the GitHub/EventSource interface. This specification does not select a particular label, project status, or webhook mapping.

### REQ-PIPE-002: Node label tracking

CogWorks MUST apply `cogworks:node:<name>` labels to track the currently active node(s) of each work item. Multiple node labels may be present simultaneously when nodes execute in parallel.

### REQ-PIPE-003: Node advancement

CogWorks MUST evaluate outgoing edge conditions when a node completes and activate all downstream nodes whose edge conditions are satisfied and whose other input requirements are met. When no configuration file exists, the default linear pipeline (Intake → Architecture → Interface Design → Planning → Code Generation → Review → Integration) is used.

### REQ-PIPE-004: Idempotent operations

All pipeline operations MUST be idempotent. Re-invoking the step function for the same GitHub state MUST produce the same outcome or detect prior completion and take no action.

### REQ-PIPE-005: Audit comments

CogWorks MUST post a structured status comment on the work item issue when entering each node, when a node completes successfully, and when a failure occurs.

### REQ-PIPE-006: Node gate enforcement

CogWorks MUST respect node gate configuration (`auto-proceed` or `human-gated`). Safety-critical work items MUST override all code-producing node gates to `human-gated` regardless of repository configuration.

These are CogWorks pipeline-advancement gates. External factory or GateKeeper approvals are not CogWorks gate mechanisms; their results are conveyed through the work-item interface and do not change the configured CogWorks gates.

**Exception**: Constitutional rules loading (REQ-CONST-001) is unconditional and not subject to gate configuration. It runs on every pipeline invocation regardless of gate settings.

### REQ-PIPE-007: Processing lock

CogWorks MUST apply a `cogworks:processing` label before processing a work item and MUST back off (exit without taking action) if that label is already present when a new invocation begins.

### REQ-PIPE-008: Duplicate pipeline prevention

The orchestrator MUST prevent duplicate pipeline runs for the same work item. If a pipeline is already running for an issue, a new trigger MUST either be rejected (with a comment explaining the conflict) or queue for execution after the current run completes (configurable).

### REQ-PIPE-009: Pipeline resumption

Re-triggering a failed pipeline MUST support resuming from the failed node (using the persisted pipeline state from REQ-EXEC-002) rather than restarting from the beginning. A full restart MUST be supported only on explicit request (`cogworks:restart` label or `/cogworks restart` comment).

### REQ-PIPE-010: Configurable pipeline graph

The pipeline MUST support configurable directed graphs where nodes are processing steps and edges are transitions with conditions. The default configuration MUST be the existing linear pipeline. Edge conditions MUST support both deterministic evaluation (expression checks against pipeline state) and LLM evaluation (natural language conditions assessed against pipeline context). Graph cycles MUST have termination conditions (maximum traversals, cost budget) to prevent infinite loops.

### REQ-PIPE-011: Shift work boundary

Each work item classification MUST define an explicit shift work boundary — the pipeline node after which CogWorks proceeds non-interactively. Nodes before the boundary MAY require human approval (configurable). Nodes after the boundary run autonomously with gate enforcement as the quality control. The boundary MUST be visible in the GitHub issue (as a label or comment) so humans know when to engage and when to let the system run.

### REQ-PIPE-012: Implementation job workspace

For each implementation job, CogWorks MUST request one ephemeral container through an infrastructure provider interface. CogWorks' orchestrator owns the job lifecycle and MUST request cleanup on success, failure, or cancellation. The container runtime and provider (for example Docker, Kubernetes, or a cloud runtime) are infrastructure choices behind that interface. Durable pipeline state and deliverables MUST remain recoverable from the work-item system and repository; container-local state is an execution aid, not a durability mechanism.

---

## REQ-GRAPH: Pipeline Graph Structure

### REQ-GRAPH-001: Directed graph with controlled cycles

The pipeline MUST be a directed graph where nodes represent processing steps and edges represent transitions. The graph MUST support sequential execution, parallel fan-out, fan-in synchronisation, and conditional edges. The graph MAY contain cycles (for retry and rework loops). Every cycle MUST have an explicit termination condition that guarantees eventual exit. The orchestrator MUST enforce termination conditions and halt the pipeline with a clear error if a cycle would exceed its limit.

### REQ-GRAPH-002: Node identity and ordering

Each node in the graph MUST have a unique name within the pipeline configuration. The orchestrator MUST compute the execution order from the edge definitions using topological sorting. For nodes that are not connected by any path, the orchestrator MUST support concurrent execution (see REQ-EXEC-006).

### REQ-GRAPH-003: Default linear pipeline

If a repository has no pipeline configuration file, the orchestrator MUST use a default pipeline equivalent to the original 7-node linear sequence: Intake → Architecture → Interface Design → Planning → Code Generation → Review → Integration. Default properties: all edges unconditional, review-to-code-generation rework edge has `max_traversals: 3` (matching REQ-REVIEW-005's default remediation cycle limit), safety-classified work items require human approval at Architecture, Interface Design, and Review, no parallel execution.

### REQ-GRAPH-004: Pipeline configuration file

Pipeline graphs MUST be configurable per repository via a TOML configuration file at `.cogworks/pipeline.toml`. The configuration defines nodes, edges, and pipeline-level settings. Multiple named pipelines MAY be defined in the same file. The pipeline for a work item is selected by the Intake node's classification output.

---

## REQ-NODE: Node Types

### REQ-NODE-001: Node interface

Every node, regardless of type, MUST implement a common interface: name (unique identifier), type (`llm`, `deterministic`, or `spawning`), inputs (required artifacts or state), outputs (produced artifacts or state), validation (how success is determined), timeout (maximum wall-clock time), and cost budget (for LLM nodes). The orchestrator MUST verify all inputs are available before starting a node.

### REQ-NODE-002: LLM nodes

LLM nodes MUST specify a prompt template, context requirements, output schema, and retry behaviour. The orchestrator MUST assemble context, invoke the LLM gateway, validate output against the schema, retry on failure (up to the node's retry budget), and record the full prompt, response, validation result, and cost in the audit trail.

### REQ-NODE-003: Deterministic nodes

Deterministic nodes MUST specify an execution method (`script`, `domain_service`, or `builtin`). The orchestrator MUST execute the specified method, capture output, parse it according to the node's output specification, and record the invocation and result in the audit trail.

### REQ-NODE-004: Spawning nodes

Spawning nodes MUST specify a prompt template (for LLM-based analysis) or a script (for deterministic issue creation), an issue template, labels to apply, and whether to link new issues to the current work item. Spawning nodes MUST be non-blocking by default — the pipeline continues regardless of success. The orchestrator MUST create resulting issues, link them to the parent work item, and record them in the audit trail.

---

## REQ-EDGE: Edge Conditions

### REQ-EDGE-001: Edge condition types

The orchestrator MUST support three types of edge conditions: deterministic conditions (evaluated by the orchestrator against pipeline state via a simple expression language), LLM-evaluated conditions (natural-language conditions assessed by the LLM against current pipeline context), and composite conditions (boolean combinations AND/OR/NOT of deterministic and LLM-evaluated conditions). LLM-evaluated conditions MUST be recorded in the audit trail. LLM-evaluated conditions MUST have a deterministic fallback (edge either taken or not taken) when the LLM is unavailable or returns an ambiguous response.

### REQ-EDGE-002: Edge priority and mutual exclusion

When multiple edges leave the same source node, the orchestrator MUST evaluate them in declared order. The configuration MUST support: `all-matching` (all true edges taken — fan-out), `first-matching` (only first true edge taken — exclusive routing), and `explicit` (the pipeline configuration declares exactly which edges fire for nodes using this mode; the explicit edge list is fixed at configuration load time and stored in `PipelineGraph::explicit_edge_lists`). The evaluation mode MUST be declared per source node.

### REQ-EDGE-003: Rework edges

Edges that create cycles MUST specify a maximum traversal count, which node outputs to preserve vs. discard on re-entry, and whether to increment the retry or rework counter. The orchestrator MUST track traversal counts per cycle and enforce the maximum. When the maximum is reached, the pipeline MUST either halt with an error, escalate to a human, or take a configured overflow edge.

---

## REQ-EXEC: Pipeline Execution

### REQ-EXEC-001: Ephemeral implementation workspace

Each implementation job MUST execute in one ephemeral container requested by CogWorks' orchestrator through an infrastructure provider interface. The orchestrator MUST arrange cleanup when the job succeeds, fails, or is cancelled. The provider supplies the runtime; CogWorks does not prescribe a specific container platform. Pipeline state MUST be reconstructable from durable work-item and repository records, so loss of the container MUST NOT require restarting the pipeline from scratch.

### REQ-EXEC-002: Pipeline state machine

The orchestrator MUST maintain a state machine for each pipeline run tracking: current active nodes, completed nodes with outputs, pending nodes (inputs not yet available), blocked nodes (upstream dependency failed — inputs will never arrive), failed nodes with error info, per-cycle traversal counts, and cumulative cost. The state machine MUST be representable as a JSON document and MUST be written to a GitHub comment on the parent work item at each node boundary.

### REQ-EXEC-003: Node execution lifecycle

Each node execution MUST follow this lifecycle: (1) precondition check — verify all declared inputs are available, (2) announce — update pipeline state comment, (3) execute — run the node, (4) validate — check output against validation criteria (schema validation, domain service validation, and alignment verification for LLM nodes), (5) record — write outputs and audit trail, (6) announce — update GitHub state, (7) evaluate edges — evaluate outgoing edge conditions. If execution fails and retries are available, the node re-enters execute with error info in context. If alignment verification fails and rework cycles are available, the node re-enters execute with specific misalignment findings in context. If retries and rework cycles are both exhausted, the node enters `failed` state.

### REQ-EXEC-004: Pipeline triggering

The pipeline MUST be triggerable through the EventSource abstraction, including manual CLI invocation. Provider-specific readiness events and their mapping to a work item MUST remain behind the GitHub/EventSource interface; this specification does not mandate a particular label, project status, or webhook payload. The orchestrator MUST prevent duplicate pipeline runs for the same work item.

### REQ-EXEC-005: Pipeline cancellation

A running pipeline MUST be cancellable through the EventSource/work-item interface. The provider-specific cancellation signal is not prescribed here. On cancellation: active node executions are terminated (in-progress LLM calls are allowed to complete), current state is persisted through the work-item interface, a summary is posted, and active implementation containers are cleaned up.

### REQ-EXEC-006: Parallel node execution

When the graph has multiple nodes whose inputs are all available simultaneously, the orchestrator MUST support executing them concurrently as async tasks within the orchestrator process. Parallel execution MUST respect the pipeline's total cost budget (shared), the maximum concurrent LLM calls limit (configurable, default: 3), report progress for each node independently, and handle partial failure (other nodes continue unless a failed node is marked `abort_siblings_on_failure: true`). Fan-in occurs when a downstream node declares inputs from multiple upstream nodes; it stays pending until all inputs complete.

### REQ-EXEC-007: Sub-work-item execution within graph

Sub-work-items produced by a planning node MUST be processed as a sub-graph within the pipeline. Each sub-work-item follows a code-generation → review → integration sequence (or a configured sub-graph). Sub-work-items MUST be processed in topological dependency order. Sub-work-items with no mutual dependency path MAY execute concurrently when the pipeline configuration allows parallel fan-out.

### REQ-EXEC-008: Implementation job container lifecycle

For each implementation job, the orchestrator MUST request exactly one ephemeral container through an infrastructure provider interface and MUST own its lifecycle, including cleanup after success, failure, or cancellation. The provider interface MUST isolate CogWorks' orchestration contract from the selected runtime or hosting provider. The requirements do not prescribe provider-specific APIs or container configuration.

---

## REQ-CLASS: Task Classification

### REQ-CLASS-001: Classification schema

The Task Classifier MUST produce structured output conforming to the classification schema: task type (enum), affected modules (list), estimated scope, safety_affecting flag, and rationale.

### REQ-CLASS-002: Safety override

If any module in the `affected_modules` list is registered in the safety-critical module registry, the final classification MUST set `safety_affecting: true`, regardless of what the LLM produced.

### REQ-CLASS-003: Scope threshold

When the estimated scope exceeds the configured threshold, the Task Classifier MUST produce an escalation result and MUST NOT proceed to subsequent nodes.

---

## REQ-ARCH: Architecture and Specification

### REQ-ARCH-001: Architectural context

The Specification Generator MUST include relevant ADRs, coding standards, and architectural constraints from the repository in its LLM context.

### REQ-ARCH-002: Specification format

The generated specification MUST be a structured Markdown document containing: affected modules, design decisions with rationale, dependency changes, risk assessment, and required ADRs.

### REQ-ARCH-003: Specification delivered as PR

The specification document MUST be delivered via a Pull Request targeting the appropriate branch of the target repository.

### REQ-ARCH-004: Specification gate

The pipeline MUST NOT advance past the architecture node until the specification PR is merged or explicitly approved per the gate configuration.

### REQ-ARCH-005: Specification validation

The Specification Generator MUST validate that all referenced modules exist in the repository (or are explicitly marked new) and that proposed dependency changes do not violate the project's architectural constraints.

---

## REQ-IFACE: Interface Design

### REQ-IFACE-001: Context from approved specification

The Interface Generator MUST build its LLM context from the approved specification document, existing interface conventions in the repository, and any cross-domain interface registry entries relevant to the affected modules.

### REQ-IFACE-002: Structured interface output

The Interface Generator MUST instruct the LLM to produce complete, syntactically valid interface definition files in the target domain's artifact format. Partial or template-placeholder files MUST be rejected.

### REQ-IFACE-003: Domain service validation

Generated interface files MUST be validated via the domain service's `validate` method before a PR is created. Validation failures MUST be fed back to the LLM as structured diagnostics for correction.

### REQ-IFACE-004: Retry budget

Each interface generation attempt MUST be counted against a configurable retry budget (default: 5). Exceeding the retry budget MUST trigger escalation with a summary of all attempts and their failure diagnostics.

### REQ-IFACE-005: Delivered as a Pull Request

Validated interface files MUST be delivered via a Pull Request. The PR MUST reference the parent work item and the specification PR.

### REQ-IFACE-006: Interface design gate

The pipeline MUST NOT advance to the planning node until the interface design PR is merged or explicitly approved per the node gate configuration.

### REQ-IFACE-007: Cross-domain registry consistency

If the affected modules declare cross-domain interfaces in `.cogworks/interfaces/`, the Interface Generator MUST verify that generated interface definitions are consistent with the declared registry contracts. Inconsistencies MUST be reported before PR creation.

---

## REQ-PLAN: Work Planning

### REQ-PLAN-001: Planning schema

The Work Planner MUST produce structured output: a list of sub-work-items each with a title, description, file list, interface references, test specification, and dependency references.

### REQ-PLAN-002: Sub-work-item minimum

A plan MUST contain at least one sub-work-item. A plan with zero sub-work-items is invalid and MUST be fed back to the LLM.

### REQ-PLAN-003: Dependency graph validation

The Work Planner MUST compute a topological ordering of sub-work-items. Circular dependencies MUST be detected, and the specific cycle MUST be returned as structured feedback to the LLM for replanning.

### REQ-PLAN-004: Granularity limit

The Work Planner MUST enforce a configurable maximum number of sub-work-items per work item (default: 10). Exceeding the limit MUST trigger escalation.

### REQ-PLAN-005: Sub-work-item issues

Each sub-work-item MUST be created as a native GitHub sub-issue of the parent work item (using the sub-issue API), with all required labels applied. Dependencies between sub-work-items MUST be expressed via native GitHub typed issue links (`blocks` / `is blocked by`).

### REQ-PLAN-006: Interface coverage

The Work Planner MUST verify that every interface from the Interface Design node is covered by at least one sub-work-item.

### REQ-PLAN-007: Milestone inheritance

When the parent work item has a GitHub Milestone set, all sub-work-items created by the Planning node MUST inherit the same milestone.

---

## REQ-CODE: Code Generation

### REQ-CODE-001: Dependency-ordered processing with prior context

Sub-work-items MUST be processed in topological dependency order. Each sub-work-item MUST receive the implementation outputs of all prior completed sub-work-items that it depends on as part of its context. Sub-work-items with no mutual dependency path MAY execute concurrently when the pipeline configuration allows parallel fan-out; each concurrent sub-work-item receives the outputs of its own dependency chain.

### REQ-CODE-002: Structured feedback loop

CogWorks MUST feed structured diagnostics from the configured validation interface (artifact, location, severity, message) back to the LLM on failure. Validation may be domain-specific, but its integration MUST remain behind the domain-service interface. Failures that are not self-explanatory MUST be interpreted before being included in retry context.

### REQ-CODE-003: Retry budget

Each sub-work-item MUST have a configurable maximum retry count (default: 5). Exceeding the retry budget MUST trigger escalation with a summary of all attempts and their failure reasons.

### REQ-CODE-004: Cost budget

The pipeline MUST track accumulated LLM token cost and halt when the pipeline budget is exceeded. The halt MUST include a per-node, per-sub-work-item cost report.

### REQ-CODE-005: Context truncation

Context assembly MUST apply deterministic priority-based truncation when the assembled package would exceed the model's context window. The current sub-work-item's interface definition MUST never be removed regardless of truncation pressure.

### REQ-CODE-006: Semantic stalling detection

The retry logic MUST track diagnostic categories across consecutive retry attempts for each sub-work-item. If the same diagnostic category recurs for a configurable number of consecutive retries (default: 3), the system MUST escalate with a structured stalling report rather than consuming the remaining retry budget. The stalling report MUST include the recurring error category and a summary of all attempts. This is distinct from flaky test detection (EDGE-015), which covers identical code producing different results.

### REQ-CODE-007: Test-first implementation loop

For each implementation job, CogWorks MUST establish the job's tests or equivalent domain-specific acceptance checks from the approved work-item and specification inputs before implementation proceeds. The job then implements the requested change, runs the configured validation, and iterates on structured feedback within its retry and rework budgets. This contract applies to software and other engineering domains; the test and validation mechanisms are selected through the domain-service interface.

---

## REQ-SCEN: Scenario Validation

### REQ-SCEN-001: Human-authored scenarios

Scenario specifications MUST be authored and maintained by humans. CogWorks MUST NOT create or modify scenario specifications.

### REQ-SCEN-002: Holdout principle

Scenario specification files MUST NOT be included in any code generation context package, regardless of their relevance to affected modules.

### REQ-SCEN-003: Multiple trajectories

Each scenario MUST be executed for the configured number of independent trajectories (default: 10). Each trajectory MUST start with fresh state.

### REQ-SCEN-004: Satisfaction scoring

A satisfaction score MUST be computed as the fraction of trajectories that satisfy acceptance criteria. Any trajectory that triggers an explicit failure criterion MUST cause immediate failure of the entire validation, regardless of the overall score.

### REQ-SCEN-005: Threshold enforcement

Scenario validation MUST fail when the overall satisfaction score falls below the configured threshold (default: 0.95). Context Packs MAY declare a stricter satisfaction threshold for their domain; when present, the stricter threshold applies to scenarios covering that domain's interfaces.

### REQ-SCEN-006: Below-threshold remediation

When scenario validation fails, the failing scenario identifiers, trajectory observations, and failure details MUST be fed back to the Code Generator as structured context for remediation.

### REQ-SCEN-007: Applicable scenario selection

CogWorks MUST select only scenarios whose declared interface coverage overlaps with the interfaces implemented by the current sub-work-item. Sub-work-items with no applicable scenarios MUST skip scenario validation (not fail it).

### REQ-SCEN-008: Scenario availability is optional

The absence of scenario specifications for a sub-work-item MUST NOT be treated as a failure. Scenario validation is skipped silently when no applicable scenarios exist.

### REQ-SCEN-009: Scenario audit trail

Scenario validation results MUST be recorded in the audit trail, including: overall satisfaction score, per-scenario scores, trajectory count, failure details, and any explicit failure criteria that were triggered.

---

## REQ-REVIEW: Review Gate

### REQ-REVIEW-001: Review pass schema

Each LLM review pass MUST produce structured output: overall pass/fail, per-criterion findings, each with file reference, line number (where applicable), severity (blocking/warning/informational), and explanation.

### REQ-REVIEW-002: Four review passes

The review gate MUST execute four passes in order:

1. Deterministic cross-domain constraint validation (no LLM, no tokens)
2. Code quality LLM review (coding standards, idioms, error handling, naming, documentation)
3. Architecture compliance LLM review (matches spec, respects boundaries, no unplanned dependencies)
4. Security LLM review (input validation, auth boundaries, unsafe code, vulnerability patterns)

### REQ-REVIEW-003: Independent pass prompts

Each of the three LLM review passes MUST use a separate, focused prompt. Review passes MUST NOT be combined into a single LLM call.

### REQ-REVIEW-004: Blocking vs non-blocking aggregation

Any blocking finding in any review pass MUST prevent PR creation and trigger the remediation loop. Non-blocking findings (warning, informational) MUST be collected and posted as PR review comments. Missing required artefacts (declared by loaded Context Packs) MUST be treated as blocking findings identifying the pack and the missing artefact.

### REQ-REVIEW-005: Remediation loop

Blocking findings MUST be fed back to the Code Generator as structured context for remediation. If blocking findings persist after the configured maximum remediation cycles (default: 3), escalation MUST be triggered.

### REQ-REVIEW-006: Safety-critical human approval

PRs for safety-critical work items MUST require explicit human approval before merge. CogWorks MUST NOT take any action that would merge a PR.

---

## REQ-INT: Integration (PR Creation)

### REQ-INT-001: Non-blocking findings as inline comments

Non-blocking review findings MUST be posted as inline review comments on the PR at the relevant file and line number.

### REQ-INT-002: PR traceability references

Every sub-work-item PR created by CogWorks MUST include references to: the sub-work-item issue, the parent work item, the specification PR, and the interface design PR.

---

## REQ-AUDIT: Audit Trail

### REQ-AUDIT-001: LLM call recording

Every LLM call MUST be recorded in the audit trail with: model name, input token count, output token count, latency, node, and sub-work-item identifier (if applicable).

### REQ-AUDIT-002: State transition recording

Every pipeline state transition (node entry, node completion, gate evaluation) MUST be recorded in the audit trail.

### REQ-AUDIT-003: Failure reporting

When a node fails, CogWorks MUST post a structured failure report as a GitHub issue comment and apply the `cogworks:node:failed` label.

### REQ-AUDIT-004: Performance metric emission

CogWorks MUST record pipeline lifecycle, retry, rework, and cost fields through its structured tracing instrumentation. The CLI MUST route tracing output to structured console logging and MAY export spans through the configured OpenTelemetry exporter. This requirement does not mandate a custom metric-sink abstraction or guarantee derived metrics that are not represented by recorded tracing fields.

### REQ-AUDIT-005: Metric data completeness

Tracing fields MUST use stable names for the pipeline run identifier, node identifier, event kind, and any event-specific values defined by the observability interface. External systems MAY derive and aggregate metrics from available spans and events. CogWorks MUST NOT claim that dimensions or measurements absent from those records are emitted.

### REQ-AUDIT-006: Tracing-based observability

CogWorks MUST use its existing tracing instrumentation and OpenTelemetry exporter path for external observability. Backend selection and aggregation are handled by the configured OpenTelemetry collector/exporter and external monitoring systems; CogWorks MUST NOT introduce a separate metric-sink interface or implement metrics storage, aggregation, or dashboarding.

---

## REQ-BOUND: System Boundaries

### REQ-BOUND-001: No code execution within CogWorks

CogWorks MUST NOT execute generated code or LLM output within its own process. Code execution is delegated to domain services.

### REQ-BOUND-002: No PR merging

CogWorks MUST NOT merge, approve, close, or request changes on any Pull Request. CogWorks does not own releases or artifact publication. PR lifecycle decisions and any external factory/GateKeeper approvals belong outside CogWorks and are passed through the work-item interface.

### REQ-BOUND-003: No metrics storage

CogWorks MUST NOT implement its own metrics database, time-series store, or dashboarding. Metrics storage, aggregation, trend analysis, and dashboarding are delegated to external purpose-built tools.

### REQ-BOUND-004: External status and approval pass-through

CogWorks MUST consume readiness, implementation-stage, and external approval status through the work-item interface. The mapping of provider-specific states (including a possible ready-for-implementation stage) MUST remain behind that interface and MUST NOT be fixed by this specification. CogWorks MUST report pipeline outcomes through the same interface without performing external approval, merge, release, or artifact-publication actions.

### REQ-BOUND-005: Credential provider boundary

Authentication and key lifecycle MUST be handled by the Vault library behind the credential-provider interface. CogWorks MUST rely on that provider to resolve credentials for infrastructure clients and MUST NOT prescribe Vault authentication or rotation details. Key material MUST NOT be included in LLM prompts or context, audit records, generated artifacts, or logs.

### REQ-BOUND-006: Attestation verification and signing

Before acting on a work item, CogWorks MUST verify any upstream attestation required by the configured trust policy through an attestation-provider interface. Missing, invalid, or content-mismatched required attestations MUST fail closed: CogWorks MUST NOT process the work item and MUST record a non-secret failure outcome. CogWorks MUST sign its own action attestations through the same provider boundary and publish them through the work-item interface. Cryptographic implementation, key access, and key lifecycle are delegated to the provider; the specific signature algorithm and provider protocol are not prescribed here.

---

## REQ-LLM: LLM Gateway

### REQ-LLM-001: LLM API rate limit respect

The LLM Gateway MUST track rate-limit state per LLM provider using response headers (e.g., `retry-after`, `x-ratelimit-remaining`). When the remaining request budget is low, the gateway MUST proactively throttle subsequent calls to avoid exhausting the budget. When an HTTP 429 response is received, the gateway MUST wait for the duration specified in the provider's response headers before retrying.

### REQ-LLM-002: Rate limit coordination across parallel nodes

When multiple nodes execute concurrently, the LLM Gateway MUST coordinate rate-limit state across all parallel LLM calls within the same pipeline invocation. Concurrent calls MUST NOT collectively exceed the provider's rate limit.

### REQ-LLM-003: Rate limit halt threshold

If the required wait time to respect a rate limit exceeds a configurable maximum (default: 30 minutes), the current step MUST halt with a retriable exit code rather than blocking indefinitely. A structured report MUST be posted identifying the rate limit and the required wait time.

### REQ-LLM-004: Rate limit visibility

The LLM Gateway MUST emit structured log entries when rate limiting is active, including: provider name, remaining request budget, reset time, and whether calls are being queued or throttled.

---

## REQ-LABEL: Label Configuration

### REQ-LABEL-001: Configurable workflow labels

Workflow-semantic labels (trigger, awaiting-review, safety-critical, hold, cancel, sub-work-item) MUST have configurable names via the `[labels]` section of `.cogworks/config.toml`. Each label MUST have a sensible default using the `cogworks:` prefix.

### REQ-LABEL-002: Pipeline-internal labels are not configurable

Pipeline-internal labels (`cogworks:processing`, `cogworks:node:<name>`, `cogworks:node:failed`, `cogworks:status:<status>`, `cogworks:restart`) MUST always use the `cogworks:` prefix and MUST NOT be configurable. These labels are machine state and are not intended for human consumption.

### REQ-LABEL-003: Label configuration validation

Label configuration MUST be validated at load time. If two workflow labels are configured to the same string, the system MUST produce a clear error and halt. Additionally, if any workflow label is configured to the same string as a pipeline-internal label (e.g., `cogworks:processing`, `cogworks:node:*`), the system MUST produce a clear error and halt — the same GitHub label string carrying two distinct semantic meanings causes undefined pipeline behavior.

---

## REQ-PROJECT: GitHub Projects Integration

### REQ-PROJECT-001: Optional project board sync

CogWorks MUST support optionally syncing pipeline state to a GitHub Projects V2 board. When a `[github_project]` section is present in `.cogworks/config.toml`, CogWorks MUST add the work item to the configured project when a pipeline starts, update project fields (current node, safety classification, disposition) at each node boundary, and mark the item complete or failed on pipeline finish.

### REQ-PROJECT-002: Project sync is non-blocking

GitHub Project board updates MUST be fire-and-forget. Failures to update the project board MUST be logged as warnings but MUST NOT block, slow, or fail the pipeline. CogWorks MUST operate identically with or without a GitHub Project configuration.

### REQ-PROJECT-003: Project field mapping

The `[github_project]` configuration MUST support mapping between pipeline state properties (current node, safety classification, disposition, cost) and GitHub Project custom field names. Field mappings are repository-specific.

---

## REQ-CTX: Context Assembly

### REQ-CTX-001: Reference exemplars

The context assembly service MUST support including files from external repositories as reference exemplars. Exemplars are declared in the architecture specification and included in the context package for code generation. Exemplars are read-only context — CogWorks MUST NOT modify files in referenced repositories. Exemplar inclusion MUST respect the pyramid summary levels (Level 2 for distant references, Level 3 for closely related references).

---

## REQ-DTU: Digital Twin Utility

### REQ-DTU-001: Twin specification format

Digital Twin specifications MUST be structured documents (TOML/YAML) describing the external dependency being modelled, the expected behavioral contracts, and the fidelity requirements.

### REQ-DTU-002: Twin lifecycle management

Twins MUST be programmatically startable and stoppable. CogWorks MUST be able to start a twin before scenario execution and stop it cleanly after.

### REQ-DTU-003: Twin state isolation

Each trajectory execution MUST start with a fresh twin state. Twin state MUST NOT persist between trajectories.

### REQ-DTU-004: Twin conformance testing

Each Digital Twin MUST include a conformance test suite that validates its behavior against the specification.

### REQ-DTU-005: Twin provisioning for scenarios

When a scenario specification requires a Digital Twin, the twin MUST be started before the first trajectory and stopped after the last trajectory.

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

### REQ-XDOM-001: Human-authored only

Interface definitions in `.cogworks/interfaces/` MUST be authored and maintained by humans. CogWorks MUST NOT create or modify interface definitions autonomously. CogWorks MAY suggest additions as recommendations for human review.

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

### REQ-XVAL-001: Deterministic-first ordering

Cross-domain constraint validation MUST execute before any LLM review pass in the review gate. It MUST NOT consume LLM tokens.

### REQ-XVAL-002: Interface extraction

The constraint validator MUST use the domain service's `extract_interfaces` method to obtain actual interface values from generated artifacts before comparing against registry contracts.

### REQ-XVAL-003: Severity levels

Hard constraint violations (values outside declared min/max bounds) MUST produce blocking findings. Nominal deviations (within bounds but outside the declared nominal range) MUST produce warnings.

### REQ-XVAL-004: Single-service operation

Cross-domain constraint validation MUST be able to validate a single domain's artifacts against registry contracts without requiring other participating domains' services to be running.

### REQ-XVAL-005: Architecture-node validation

Cross-domain constraint validation MUST also run during the architecture node to catch violations before implementation begins.

---

## REQ-CPACK: Context Pack System

### REQ-CPACK-001: Pack trigger mechanism

Context Pack loading MUST be driven deterministically by the work item's classification labels, component tags, and safety classification. The LLM MUST NOT choose which packs to load.

### REQ-CPACK-002: Pack loading timing

Context Packs MUST be loaded at the Architecture node, before any LLM call in the Architecture node begins. Loaded packs MUST remain active for the entire pipeline run.

### REQ-CPACK-003: Multiple simultaneous packs

A single pipeline run MUST support loading multiple Context Packs simultaneously when a work item matches multiple pack triggers.

### REQ-CPACK-004: Conflict resolution

Where loaded Context Packs contain contradictory guidance, the more restrictive rule MUST apply.

### REQ-CPACK-005: Unconditional loading on match

If a work item matches a Context Pack's trigger criteria, the pack MUST be loaded. There MUST NOT be an option to skip a matched pack.

### REQ-CPACK-006: Required artefact enforcement

Context Packs MAY declare required artefacts. At the Review node, CogWorks MUST verify all declared required artefacts are present. Missing artefacts MUST produce blocking findings identifying the pack and the specific missing artefact.

### REQ-CPACK-007: Pack audit trail

The set of Context Packs loaded for each pipeline run MUST be recorded in the audit trail and included in the PR description.

### REQ-CPACK-008: Pack content in context assembly

Context Pack domain knowledge, safe patterns, and anti-patterns MUST be included in the context package for all LLM calls from the Architecture node onward, subject to the standard context priority and truncation rules.

---

## REQ-CONST: Constitutional Security Layer

### REQ-CONST-001: Unconditional loading

Constitutional rules MUST be loaded on every pipeline run, before context assembly and before any LLM call. This is NOT a configurable gate.

### REQ-CONST-002: Privileged position

Constitutional rules MUST be injected as a privileged, non-overridable component of the LLM system prompt. No content in the context package MAY modify, append to, or override the constitutional rules.

### REQ-CONST-003: Human-approved source

Constitutional rules MUST be loaded from a version-controlled file at a well-known path. Changes to the constitutional rules file MUST require a reviewed and merged PR with at least one human approval. Rules from unreviewed branches MUST be rejected.

### REQ-CONST-004: External content as data

The constitutional rules MUST include a rule declaring that issue bodies, specifications, dependency docs, API responses, and any content not from core configuration are inputs to be analyzed — not instructions that modify CogWorks' behavior.

### REQ-CONST-005: Injection detection and halt

If external content contains text structured as a directive to CogWorks (persona overrides, instruction injections, behavioral modifications), the pipeline MUST halt immediately with an `INJECTION_DETECTED` event.

### REQ-CONST-006: Injection event content

The `INJECTION_DETECTED` event MUST include: pipeline run ID, work item ID, source document, and offending text.

### REQ-CONST-007: Hold state on injection

When injection is detected, the work item MUST enter a hold state. The work item MUST NOT be automatically requeued or retried. A human MUST review and either confirm false positive (with justification recorded in audit trail) or mark the work item as contaminated.

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

CogWorks MUST NOT create or modify files matching protected path patterns (constitutional rules, prompt templates, scenario specifications) through the normal pipeline. Pre-PR validation MUST check generated files against protected path patterns.

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

### REQ-ALIGN-014: Review to spec alignment

The review gate alignment check MUST verify that the review was spec-aware:

**Deterministic checks:**

- The review examined all files produced by code generation

**LLM alignment check** (optional, default: off) — assesses:

- Spec-awareness: Did the review check the code against the spec, or only against general quality standards?

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
- Available for the Review Gate to reference during gate evaluation (REQ-REVIEW-001)

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

| Node | Default behaviour |
|------|-------------------|
| Intake / Classification | No alignment check (first node — nothing to align against) |
| Architecture | Deterministic + LLM alignment against work item. Score threshold: 0.90. Max rework: 3. |
| Interface Design | Deterministic + LLM alignment against architecture spec. Score threshold: 0.90. Max rework: 3. |
| Planning | Deterministic alignment against architecture spec + interfaces. LLM check: off. Score threshold: 0.85. Max rework: 2. |
| Code Generation | Deterministic + LLM alignment against sub-work-item + interfaces. Score threshold: 0.90. Max rework: 3. |
| Review | Deterministic alignment against code + spec. LLM check: off. Score threshold: 0.80. Max rework: 1. |
| Integration | No alignment check (deterministic node). |
| Deterministic nodes | No alignment check. |
| Spawning nodes | No alignment check. |
| End-to-end (pipeline-level) | Deterministic + LLM alignment against original work item. Score threshold: 0.90. Not a graph node — runs as pipeline executor post-completion step (REQ-ALIGN-015). |

### REQ-ALIGN-041: Safety classification impact on alignment

For safety-classified work items, alignment checks MUST be stricter:

| Parameter | Standard | Safety-Critical |
|-----------|----------|------------------|
| Score threshold | 0.90 | 0.95 |
| Max rework cycles | 3 | 5 |
| LLM alignment check | Configurable (default: on) | Always on (cannot be disabled) |
| End-to-end alignment check | Default on | Always on (cannot be disabled) |
| Traceability matrix | Generated | Generated and requires human sign-off |
| Blocking finding tolerance | 0 (any blocking finding fails) | 0 (additionally, more than 3 warnings also fails) |

---

## Tool Registry & Scoped Tools

> **Numbering convention**: Tool requirements use categorical groupings: 001–009 for registry-level concerns, 010–019 for filesystem tools, 020–029 for git tools, 030–039 for domain service tools, 040–049 for shell tools, 050–059 for search/network tools. New tool categories should use the next available decade range. Do not fill gaps within a category unless the new requirement belongs to that category.

### REQ-TOOL-001: Tool registry

The orchestrator MUST maintain a central registry of all available tools. Each tool entry contains: unique name, category, description, parameter JSON Schema, scope parameter declarations, and implementation reference. Tools from three sources (built-in, adapter-generated, skill-derived) all register through the same mechanism.

### REQ-TOOL-002: Scoped tool access

Each pipeline node MUST receive a tool list restricted by its assigned tool profile. Tools not in the node's profile MUST NOT appear in the LLM call's tool list. The LLM must not be able to discover or invoke unscoped tools.

---

## Tool Definitions — Filesystem

### REQ-TOOL-010: Filesystem read tool

The orchestrator MUST provide `fs.read` and `fs.list` filesystem read tools. Scope parameters: `allowed_read_paths` (glob patterns), `denied_read_paths` (glob patterns — takes precedence), `max_file_size_bytes`.

### REQ-TOOL-011: Filesystem write tool

The orchestrator MUST provide `fs.write`, `fs.create`, and `fs.delete` filesystem write tools. Scope parameters: `allowed_write_paths` (glob patterns), `denied_write_paths` (glob patterns — takes precedence). Write tools MUST enforce protected path patterns at the tool level — operations targeting protected paths are refused regardless of profile configuration.

### REQ-TOOL-012: Filesystem search tool

The orchestrator MUST provide `fs.search` (content search) and `fs.glob` (path matching) tools. Scope parameters: `allowed_search_paths` (inherits from `allowed_read_paths`).

### REQ-TOOL-013: Filesystem scope enforcement

Every filesystem tool invocation MUST validate its target path(s) against the calling node's scope parameters before executing. Violations produce a structured error returned to the LLM and a `SCOPE_VIOLATION` audit trail event.

---

## Tool Definitions — Git

### REQ-TOOL-020: Git tools

The orchestrator MUST provide git tools: `git.status`, `git.diff`, `git.commit`, `git.branch`, `git.log`. Scope parameters: `branch_pattern` (regex — commits only allowed to branches matching the pattern), `allowed_commit_paths` (glob patterns — only changes within these paths may be committed).

---

## Tool Definitions — Domain Service

### REQ-TOOL-030: Domain service tools

The orchestrator MUST provide `domain.validate`, `domain.simulate`, `domain.normalise`, `domain.review_rules`, `domain.extract_interfaces` tools that delegate to domain services via the Extension API. Scope parameters: `allowed_services` (list of domain service names the node may invoke), `allowed_methods` (list of domain service methods the node may call).

---

## Tool Definitions — Shell

### REQ-TOOL-040: Restricted shell tool

The orchestrator MUST provide a `shell.run_restricted` tool for executing pre-configured commands. Scope parameters: `allowed_commands` (explicit list of permitted command patterns), `working_directory` (path constraint), `max_execution_time_seconds`, `network_access` (boolean — whether the command may access the network). This tool executes infrastructure operations (build, test, lint), not arbitrary LLM-generated shell commands.

---

## Tool Definitions — Search & Network

### REQ-TOOL-050: Search tools

The orchestrator MUST provide `search.code` (code search across repository) and `search.docs` (documentation search) tools. Scope parameters: `allowed_search_scope` (directories or file patterns).

### REQ-TOOL-051: Network tools

The orchestrator MAY provide `net.fetch` for read-only HTTP requests to approved URLs. Scope parameters: `allowed_urls` (URL prefix patterns), `max_response_size_bytes`. Default: not enabled in any profile (opt-in only).

---

## Tool Profiles

### REQ-PROFILE-001: Per-node tool profiles

Every LLM-executing pipeline node MUST have an assigned tool profile. The profile is referenced by name in the pipeline configuration (`.cogworks/pipeline.toml`). If no explicit profile is assigned, the orchestrator MUST apply the built-in default profile appropriate to the node type.

### REQ-PROFILE-002: Profile overrides

Pipeline configuration MUST support per-node `tool_overrides` that modify scope parameters of the base profile without redefining the entire profile. Template variable placeholders (`{{variable}}`) MUST be resolved from pipeline state at node activation time.

### REQ-PROFILE-003: Default profiles

The orchestrator MUST provide default tool profiles for all 7 core pipeline nodes:

| Node | Default Profile | Key Tools | Key Scope Constraints |
|------|----------------|-----------|----------------------|
| Intake | `reader` | `fs.read`, `fs.list`, `search.code` | Read-only, full repo read access |
| Architecture | `reader` | `fs.read`, `fs.list`, `search.code`, `search.docs` | Read-only, full repo read access |
| Interface Design | `reader` | `fs.read`, `fs.list`, `search.code` | Read-only, full repo read access |
| Planning | `reader` | `fs.read`, `fs.list`, `search.code` | Read-only, full repo read access |
| Code Generation | `generator` | All `reader` tools + `fs.write`, `fs.create`, `fs.delete`, `git.commit`, `git.branch`, `domain.*`, `shell.run_restricted` | Write access limited to `{{affected_modules}}`; git commits to `cogworks/swi-*` branches only |
| Review | `reviewer` | `reader` tools + `domain.validate`, `domain.review_rules`, `domain.simulate` | Read-only filesystem; domain validation only |
| Integration | `integrator` | `git.status`, `git.diff`, `git.branch`, `git.log`, `fs.read` | Branch operations and status checks only; read-only filesystem; no `git.commit` (merges use GitHub API via Pull Request Manager abstraction) |

---

## Enforcement

### REQ-ENFORCE-001: Tool filtering (Layer 1)

The orchestrator MUST filter the tool list before every LLM call. Only tools present in the node's resolved profile are included in the LLM call's tool/function list. This is the first layer of defence.

### REQ-ENFORCE-002: Scope enforcement (Layer 2)

Each tool implementation MUST validate scope parameters before executing. This validation is independent of Layer 1 — even tools that pass filtering may have their specific invocations rejected based on scope parameters. This is the second layer of defence.

### REQ-ENFORCE-003: OS-level sandboxing (Layer 3)

For Code Generation nodes (the only core node with write access), the orchestrator SHOULD apply OS-level sandboxing (e.g., `landlock`, `seccomp-bpf`, containerised execution) to constrain filesystem and network access at the process level. This is the third layer of defence, independent of Layers 1 and 2. For safety-critical work items (REQ-PIPE-006) or work items from untrusted sources, OS-level sandboxing MUST be applied. SHOULD becomes MUST when the work item's safety classification is set.

---

## Tool Audit

### REQ-TOOL-AUDIT-001: Tool invocation audit

Every tool invocation MUST be recorded in the audit trail with: tool name, parameters (with scope parameters identified separately), result (success/error), duration, and the calling node's identity.

### REQ-TOOL-AUDIT-002: Scope violation audit

Every scope violation MUST be recorded as a `SCOPE_VIOLATION` event in the audit trail with: tool name, attempted parameters, violated scope constraint, calling node identity.

### REQ-TOOL-AUDIT-003: Tool usage report

The pipeline completion summary MUST include a tool usage report containing: per-tool call counts, scope violation counts, invocation type breakdown (raw tool / skill / progressive discovery).

---

## Custom Tools

### REQ-TOOL-CUSTOM-001: Repository-local tool definitions

Repository maintainers MAY define custom tools in `.cogworks/tools/` as TOML files following the tool definition schema. Custom tools are registered in the tool registry at pipeline startup.

### REQ-TOOL-CUSTOM-002: Custom tool scope binding

Custom tool definitions MUST declare their scope parameter schema. Tools without scope parameter declarations are rejected at registration time.

---

## Skill Crystallisation

### REQ-SKILL-001: Skill definition format

Skills MUST be defined as a TOML manifest (declaring name, parameters, scope, lifecycle state, description) paired with a shell script (containing the parameterised tool invocation sequence). Skills are stored in `.cogworks/skills/`.

### REQ-SKILL-002: Skill extraction

The orchestrator MUST support offline skill extraction from audit trail data. Extraction identifies repeating tool invocation patterns across successful pipeline runs and proposes them as candidate skills for human review.

### REQ-SKILL-003: Skill invocation by LLM

Active skills MUST be exposed to LLM nodes via `skill.list` (enumerate available skills with descriptions), `skill.run` (execute a named skill with parameters), and `skill.inspect` (view a skill's parameter schema and description).

### REQ-SKILL-004: Skill scope enforcement

Every tool call within a skill execution MUST be validated against the calling node's tool profile. A skill MUST NOT bypass scope restrictions. If a tool call within a skill violates scope, the entire skill execution fails with a structured error identifying the violating call.

### REQ-SKILL-005: Skill lifecycle management

Skills MUST follow a lifecycle: Proposed → Reviewed → Active → Deprecated → Retired. Only Active skills are available to LLM nodes. Deprecated skills remain available but produce a warning when invoked, referencing the preferred alternative. Retired skills are removed from tool lists.

Valid state transitions:

| From | To | Trigger | Approval |
|------|----|---------|----------|
| Proposed | Reviewed | Human reviews and approves the skill script and parameter schema | Human (required) |
| Reviewed | Active | Operator activates via `cogworks skill activate` | Human (required) |
| Reviewed | Proposed | Operator rejects and sends back for revision via `cogworks skill rework` | Human (required) |
| Reviewed | Retired | Operator abandons the skill via `cogworks skill retire` | Human (required) |
| Active | Deprecated | Manual deprecation or auto-deprecation via REQ-SKILL-006 (success rate below threshold) | Automatic (REQ-SKILL-006) or human |
| Deprecated | Active | Operator re-activates after fixing the underlying issue | Human (required) |
| Deprecated | Retired | Operator retires via `cogworks skill retire` | Human (required) |
| Active | Retired | Operator retires directly (e.g., skill no longer applicable) | Human (required) |

Transitions not listed above are invalid. The Proposed → Active transition (skipping review) is explicitly prohibited.

### REQ-SKILL-006: Skill success tracking

The orchestrator MUST track per-skill success/failure rates. When a skill's success rate drops below a configurable threshold (default: 90% over the last 20 invocations), the skill MUST be automatically flagged for review (status change to Deprecated with reason `success_rate_below_threshold`).

---

## Progressive Discovery

### REQ-DISC-001: Compact tool index

When a node's resolved tool list meets or exceeds a configurable threshold (default: 15 tools), the orchestrator MUST present a compact tool index (name + one-line description) instead of full schemas in the LLM tool list.

### REQ-DISC-002: Meta-tools for on-demand schema

When progressive discovery is active, the orchestrator MUST provide three meta-tools: `tools.search` (semantic search across tool names and descriptions), `tools.schema` (retrieve full JSON Schema for a named tool), and `tools.call` (invoke a tool by name with parameters). Tool invocations through `tools.call` MUST pass through the same scope enforcement as direct tool invocations (REQ-ENFORCE-002). The `tools.call` meta-tool MUST NOT provide a bypass of any enforcement layer.

### REQ-DISC-003: Tool index generation

The compact tool index MUST be regenerated whenever the node's resolved tool list changes (profile update, adapter regeneration, skill lifecycle transition). The index MUST be deterministic — same tool list produces the same index.

### REQ-DISC-004: Skill priority in search

In progressive discovery search results, skills MUST rank above raw tools when both match a query, to encourage reuse of proven patterns.

---

## Adapter Generation

### REQ-ADAPT-001: OpenAPI adapter generation

The orchestrator MUST provide a CLI command to generate CogWorks tool definitions from OpenAPI 3.0/3.1 specifications (JSON and YAML formats). Each API operation produces a separate tool definition.

### REQ-ADAPT-002: Tool definition output

Generated adapter tool definitions MUST be TOML files conforming to the tool definition schema. Each definition includes: namespaced tool name (e.g., `inventree.stock.list`), parameter schema derived from the API spec, and authentication/connection scope parameters.

### REQ-ADAPT-003: Scope integration

Generated adapter tools MUST participate in the same scope enforcement as all other tools. Adapter tool definitions MUST declare scope parameters for endpoint URL, authentication, and rate limiting.

### REQ-ADAPT-004: EAB schema format

The orchestrator MUST support adapter generation from EAB (Extension API Bridge) JSON Schema format, in addition to OpenAPI. The generated tool definitions follow the same schema and registration mechanism.

### REQ-ADAPT-005: Drift detection

The orchestrator MUST provide a CLI command to compare generated adapter definitions against their source API specifications and report differences (new endpoints, removed endpoints, parameter changes). This command SHOULD be runnable as a CI check.

### REQ-ADAPT-006: Supported specification formats

The adapter generator MUST support: OpenAPI 3.0 (JSON + YAML), OpenAPI 3.1 (JSON + YAML), and EAB JSON Schema. Additional formats MAY be added as extensions.

---

## MCP Integration (Superseded)

> **Note**: Requirement IDs REQ-MCP-001 through REQ-MCP-004 were defined in COGWORKS-TOOLSCOPE-001 §9 and are superseded by REQ-ADAPT-001 through REQ-ADAPT-006. See ADR-0005 for decision context. MCP integration is retained only as a fallback for bidirectional or streaming protocols that generated adapters cannot represent. No new requirements should use the REQ-MCP-* prefix.
