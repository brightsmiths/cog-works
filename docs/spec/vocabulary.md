# Domain Vocabulary

This document defines the precise meaning of every domain concept in CogWorks. These definitions establish the language that all subsequent design, implementation, and documentation must use consistently.

---

## Pipeline Concepts

### Pipeline

A configurable directed graph of nodes that takes a work item from intake to PR creation.

- Identified by: The parent work item's GitHub Issue number
- Contains: A directed graph of nodes connected by edges, defined in `.cogworks/pipeline.toml` or the built-in default
- State: Represented by GitHub labels on the parent work item and a structured state comment (JSON) updated at each node boundary
- Lifespan: From the EventSource/work-item adapter reporting readiness to terminal node completion or pipeline failure
- Default: Repositories with no `.cogworks/pipeline.toml` get the default linear pipeline (Intake → Architecture → Interface Design → Planning → Code Generation → Review → Integration)
- See also: Named Pipeline, Pipeline Configuration

### Pipeline Graph

The directed graph structure defining a pipeline's execution flow.

- Contains: Nodes (processing steps) and edges (transitions with optional conditions)
- Supports: Sequential execution, parallel fan-out, fan-in synchronisation, conditional routing, and controlled cycles (rework loops)
- Cycles: Permitted but every cycle MUST have an explicit termination condition (max traversals, cost budget, or deterministic exit condition)
- Ordering: Execution order computed from edge definitions via topological sorting; unconnected nodes may execute concurrently

### Node

A processing step in the pipeline graph. Every node has a common interface regardless of type.

- Identified by: Unique name within the pipeline configuration
- Type: One of `llm`, `deterministic`, or `spawning`
- Inputs: Named artifacts or state that must exist before the node can execute
- Outputs: Named artifacts or state that the node produces for downstream nodes
- Validation: How to determine success — schema-based, exit-code-based, or domain-service-based
- Timeout: Maximum wall-clock time; exceeded → node failure
- Cost budget: For LLM nodes, maximum token spend; exceeded → node halt
- Gate: `auto-proceed` or `human-gated` (configurable per node, overridable by shift work boundary and safety classification)

### LLM Node

A node that invokes the LLM gateway with a prompt template and context to produce artifacts.

- Specifies: Prompt template path, context requirements, output schema, retry behaviour
- The original 7 pipeline steps (Intake through Integration) are LLM nodes in the default pipeline
- The orchestrator assembles context, invokes the LLM, validates output against schema, retries on failure, and records everything in the audit trail

### Deterministic Node

A node that executes a script or invokes a domain service without LLM involvement.

- Execution methods: `script` (shell command), `domain_service` (Extension API method), or `builtin` (orchestrator-provided function like PR creation or label management)
- Examples: compile check, binary size check, licence scan, PR creation, label update, constraint validation, code normalisation
- The orchestrator captures stdout/stderr/exit-code, parses output per the node's specification, and records in the audit trail

### Spawning Node

A node that analyses pipeline state and creates new GitHub Issues for follow-up work. Does not produce artifacts for the current pipeline run.

- Non-blocking by default: Pipeline continues regardless of spawning node success
- May be configured as blocking (pipeline waits for issue creation, but never waits for issue completion)
- Examples: refactoring analysis, tech debt detection, follow-up work identification, documentation gap detection
- Created issues are linked to the parent work item and recorded in the audit trail

### Edge

A transition between two nodes in the pipeline graph.

- Contains: Source node, target node, optional condition
- Unconditional edges: Always taken (condition omitted)
- Conditional edges: Taken only when the condition evaluates to true
- Evaluation modes (per source node): `all-matching` (fan-out to all true edges), `first-matching` (exclusive routing), or `explicit` (node output includes a `next_edges` field — a list of edge IDs — that the orchestrator uses to determine exactly which edges to activate; the node's output schema must declare this field)

### Edge Condition

A rule that determines whether an edge is taken after its source node completes.

- **Deterministic condition**: Expression evaluated against pipeline state using a simple expression language (e.g., `review.result.passed == true`, `code_generation.retry_count < 5`)
- **LLM-evaluated condition**: Natural-language condition assessed by the LLM against current pipeline context (e.g., "The review findings indicate an architectural issue, not just an implementation bug"). MUST have a deterministic fallback. MUST be recorded in the audit trail.
- **Composite condition**: Boolean combination (AND, OR, NOT) of deterministic and LLM-evaluated conditions

### Rework Edge

An edge that creates a cycle (loop) in the pipeline graph for retry or rework scenarios.

- MUST specify: Maximum traversal count, which node outputs to preserve vs. discard on re-entry, retry vs. rework semantics
- Retry: Same input, try again (e.g., LLM schema validation failure)
- Rework: Modified input, different approach (e.g., review findings fed back to code generation)
- Overflow behaviour: When max traversals reached — halt with error, escalate to human, or take a configured overflow edge

### Node Gate

A configurable decision point after a node completes where the pipeline may pause for human approval.

- Configuration: `auto-proceed` (continue immediately) or `human-gated` (wait for explicit approval)
- Scope: Configurable per node, per repository
- Override: Safety-critical work items force `human-gated` for all code-producing nodes regardless of configuration
- Shift work boundary: Sets a default gate policy per work item classification — nodes before the boundary default to `human-gated`, nodes after default to `auto-proceed`
- Represented by: `cogworks:awaiting-review` label when waiting
- Boundary: This is a CogWorks pipeline-advancement mechanism. External factory or GateKeeper approvals are separate statuses passed through the work-item interface; they do not replace or redefine configured CogWorks gates.

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
- State labels: `cogworks:node:<node-name>`, `cogworks:safety-critical`, `cogworks:processing`, `cogworks:awaiting-review`
- Milestone: If the work item has a GitHub Milestone set, it is preserved and inherited by sub-work-items (see Milestone)

### Sub-Work-Item

A GitHub Issue created by CogWorks during the Planning node, representing one implementation task within a larger work item.

- Identified by: GitHub Issue number
- Created by: The Planning node
- Created as: A native GitHub sub-issue of the parent work item (using the sub-issue API)
- Contains: Title, description, file list, interface references, test specification, dependency references
- State labels: `cogworks:sub-work-item`, `cogworks:status:<status>`
- Dependencies: Expressed via native GitHub typed issue links (`blocks` / `is blocked by`), not via labels
- Milestone: Inherits the parent work item's GitHub Milestone, if one is set
- Constraint: Maximum configurable count per work item (default: 10)
- Constraint: Must represent logical changes, not individual files
- Ordering: Topologically sorted by declared dependencies (read from GitHub typed issue links)

### Classification

The result of analyzing a work item to determine its type, scope, and safety impact.

- Contains: Task type (enum), affected modules (list), estimated scope, safety-affecting flag, rationale
- Task types: new feature, bug fix, refactor, configuration change, documentation, dependency update
- Determines: Which pipeline nodes execute, which constraints apply, which gates are enforced

### Safety Classification

A determination of whether a work item touches safety-critical code paths.

- Determined by: Cross-referencing affected modules against the safety-critical module registry
- Override rule: If *any* affected module is in the registry, the work item is safety-affecting regardless of LLM classification
- Consequence: Forces human-gated transitions for all code-producing nodes

### Scope Estimate

A rough measure of work item size used to decide whether human confirmation is needed.

- Contains: Estimated file count, estimated interface count, complexity rating
- Threshold: Configurable; exceeding triggers escalation

---

## Validation Concepts

### Scenario

A structured specification describing an end-to-end behavior or user story to validate against generated code.

- Contains: Natural-language description, preconditions, action sequence, acceptance criteria, optional failure criteria
- Format: Structured (TOML/YAML with schema)
- Stored: Separate from codebase (`.cogworks/scenarios/`), excluded from code generation context (holdout principle)
- Applicability: Each scenario declares which modules/interfaces it covers
- Purpose: Probabilistic validation across realistic situations, preventing overfitting to deterministic tests

### Trajectory

A single execution run of a scenario against generated code.

- Contains: Observed outputs, timing information, captured logs/metrics, satisfaction determination
- Multiple trajectories: Each scenario runs multiple times (configurable, default 10) to capture non-deterministic behavior
- Evaluation: Each trajectory is evaluated against scenario acceptance criteria

### Satisfaction Score

The fraction of scenario trajectories that satisfy acceptance criteria.

- Range: 0.0 to 1.0
- Threshold: Configurable minimum (default 0.95) required to pass scenario validation
- Failure criteria override: A single trajectory triggering an explicit failure criterion fails the entire validation regardless of score
- Reported: Per-scenario and overall, included in PR description and audit trail

### Digital Twin

A high-fidelity behavioral clone of an external dependency (API, hardware, network protocol) built and maintained by CogWorks.

- Purpose: Enable high-volume integration testing without rate limits, cost, or safety concerns of testing against real services
- Built via: Standard CogWorks pipeline (twins are work items)
- Contains: Conformance test suite validating twin behavior against specification
- Lifecycle: Versioned, maintained, updated when real dependency changes
- Used by: Scenario validation node provisions twins when scenarios reference external dependencies
- Properties: Programmatically startable/stoppable, stateless between runs, supports failure injection
- Conformance status: Whether a twin's conformance tests still pass against the real system. Stale twins (failed conformance) produce "unverified" scenario results (see risk-register.md CW-R08)
- Fidelity boundary: Each twin specification documents what behaviors are replicated vs. simplified or omitted. Scenarios are tagged with required fidelity level; those requiring higher fidelity than available trigger physical test validation (see risk-register.md CW-R09)

### Pyramid Summary Levels

Multi-level summaries of modules enabling efficient context assembly.

- **Level 1 (one-line)**: Module name + single-sentence purpose (10-20 tokens)
- **Level 2 (paragraph)**: Purpose, public interface summary, key dependencies, constraints (100-300 tokens)
- **Level 3 (full interface detail)**: Complete public interfaces with type signatures and documentation
- **Level 4 (source code)**: Full file contents (implicit level, used for files being directly modified)
- Cached: Stored in `.cogworks/summaries/`, regenerated when source changes
- Usage: Context Assembler selects appropriate level based on dependency distance

---

## Artifact Concepts

### Specification Document

A Markdown document produced by the Architecture node describing what will be built and why.

- Contains: Affected modules, design decisions with rationale, dependency changes, risk assessment, required ADRs
- Delivered via: Pull Request (referencing the work item)
- Validated: All referenced modules must exist (or be marked new); dependency changes must not violate constraints

### Interface Definition

Source code files containing type signatures, trait definitions, and function signatures produced by the Interface Design node.

- Language: Target language (Rust initially), not pseudocode
- Delivered via: Pull Request (referencing work item and spec PR)
- Validated: Must parse and type-check via language service

### Sub-Work-Item Plan

The set of sub-work-items produced by the Planning node, with their dependency graph.

- Contains: One GitHub Issue per sub-work-item, each with file list, interface references, test specification, and dependency links
- Validated: Topological sort must succeed (no cycles), all interfaces must be covered, granularity limits respected
- Delivered via: GitHub Issues (linked to parent work item)

### Implementation Output

The code and tests produced by the Code Generation node for a single sub-work-item.

- Contains: Source files (new or modified), test files, all passing deterministic checks
- Delivered via: Pull Request (referencing sub-work-item and parent work item)
- Validated: Compilation, type checking, formatting, linting, test execution (all via language service)

### Review Result

The structured output of the Review Gate for a single sub-work-item.

- Contains: Three separate review pass results (quality, architecture compliance, security)
- Per-pass: Overall pass/fail, per-criterion pass/fail, file/line references, severity, explanation
- Severity levels: Blocking, warning, informational
- Aggregation rule: Any blocking finding in any pass prevents PR creation

---

## Context and LLM Concepts

### Context Package

The assembled set of files, documentation, and constraints provided as input to an LLM call.

- Contents vary by node but may include: specification, interface definitions, prior SWI outputs, ADRs, coding standards, architectural constraints, relevant source code, Context Pack domain knowledge
- Constraint: Must fit within target model's context window
- Truncation: Deterministic priority-based strategy when content exceeds window
- Note: Constitutional Rules are NOT part of the context package. They are injected separately as a privileged system prompt component. Context Pack content IS included in context packages via the Context Assembler.

### Context Priority Order

The deterministic ranking used to select context when the full package exceeds the model's window.

1. Current sub-work-item's interface definition (highest priority)
2. Directly depended-upon sub-work-item outputs
3. Architectural constraints
4. Context Pack domain knowledge (from loaded packs)
5. Coding standards
6. Remaining context by import-graph proximity (lowest priority)

### Prompt Template

A version-controlled Markdown file with variable placeholders that defines the instructions given to an LLM at a specific node.

- Format: Markdown with `{{variable}}` placeholders
- Stored: In repository (version-controlled)
- Constraint: Never hardcoded in source code
- Contract: Each template declares its required variables and expected output schema

### Output Schema

A JSON Schema definition that specifies the structure of an LLM's response for a given node.

- Purpose: Enables deterministic validation of LLM output before the pipeline proceeds
- Stored: In repository (version-controlled)
- Enforcement: Invalid outputs trigger automatic retry with validation error appended to context

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

A temporary local clone of the target repository used for domain service operations.

- Created: Shallow clone to a temporary directory when toolchain operations are needed
- Branch convention: `cogworks/<work-item-number>/<node-slug>`
- Lifecycle: Created and destroyed within a single CLI invocation
- Not used for: Lightweight file reads (those use GitHub API)
- Management: Domain services are responsible for creating and managing working copies. CogWorks provides shared libraries that domain services can use for clone management and other common operations.

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

### Implementation Job Container

One ephemeral container requested by CogWorks' orchestrator for a single implementation job through an infrastructure runtime-provider interface.

- Ownership: The orchestrator requests the job and requests cleanup on success, failure, or cancellation; the provider performs runtime-specific operations
- Provider neutrality: Docker, Kubernetes, and cloud runtimes are possible implementations, not CogWorks contract choices
- Durability: Container-local state is temporary; pipeline state and deliverables remain recoverable through repository and work-item interfaces
- Validation: Tests and domain-specific validation are selected through the Domain Service Client

### Implementation Job

A single test-first engineering task executed by one ephemeral container.

- Sequence: Establish tests or equivalent domain acceptance checks, implement, validate, then iterate on structured feedback within the configured budgets
- Scope: Applies to software and other engineering domains
- Tool access: Validation and Git operations requested by an LLM use deterministic, scoped tools rather than arbitrary shell commands

### Pipeline Configuration

The TOML file (`.cogworks/pipeline.toml`) that defines the pipeline graph for a repository.

- Optional: Repositories without this file get the default linear pipeline
- Contents: Node definitions, edge definitions, pipeline-level settings (cost budget, max concurrent LLM calls)
- Multiple pipelines: May define multiple named pipelines (e.g., `feature`, `bugfix`, `documentation-only`); pipeline for a work item selected by the Intake node's classification output
- Validated at load time: No orphan nodes, all edge targets exist, every cycle has a termination condition, at least one terminal node reachable from start

### Named Pipeline

A specific pipeline graph configuration defined within `.cogworks/pipeline.toml`.

- Identified by: Human-readable name (e.g., `feature-development`, `bugfix`, `documentation-only`)
- Selection: The Intake node's classification output determines which named pipeline is used for a work item
- Default: If no named pipeline matches, or no configuration file exists, the default linear pipeline is used

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

### Shift Work Boundary

The pipeline node after which CogWorks proceeds non-interactively for a given work item classification.

- Purpose: Makes the human/autonomous boundary explicit and configurable
- Per-classification: Different work item types have different boundaries (e.g., safety-critical: after Review; standard: after Interface Design; low-risk: after Intake)
- Effect: Nodes before the boundary default to `human-gated`; nodes after default to `auto-proceed`
- Override: Per-node gate configuration and safety-critical override still apply
- Visibility: Represented by a label or comment on the GitHub Issue so humans know when to engage

### Reference Exemplar

A file from an external repository included as read-only context for code generation.

- Purpose: Enable pattern reuse across repositories ("implement this feature following the pattern shown in the reference")
- Declared in: Architecture specification
- Included by: Context Assembler, at the appropriate pyramid summary level (Level 2 for distant references, Level 3 for closely related)
- Constraint: Read-only — CogWorks MUST NOT modify files in referenced repositories
- Example: Extension API handler pattern in `cogworks-domain-rust` referenced when building a new domain service

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

A version-controlled repository of cross-domain interface definitions, stored in `.cogworks/interfaces/`.

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

- Declared in: `.cogworks/services.toml` under `[[services]]` (separate from the main `.cogworks/config.toml`)
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

### Capability Profile

A machine-readable definition of what a domain service for a specific engineering domain must provide.

- Defines: Required and optional Extension API methods, required validation checks, interface types the domain declares and validates against, supported artifact types
- Purpose: Domain service developers use profiles to know what to implement; conformance tests verify a service meets its profile’s requirements
- Examples: firmware profile (requires `validate`, `simulate`, `extract_interfaces`; optional `normalise`, `review_rules`), electrical profile, mechanical profile
- Published: In the CogWorks repository alongside schemas (`schemas/capability-profiles/`)
- CogWorks does not enforce profiles at runtime — capability discovery via handshake is the runtime mechanism
- Profiles are documentation and conformance-testing artifacts, not runtime configuration

---

## Knowledge and Safety Concepts

### Context Pack

A structured directory containing domain knowledge, safe patterns, anti-patterns, and required artefact definitions for a specific technical domain.

- Located at: `.cogworks/context-packs/<pack-name>/` (configurable)
- Contains: trigger definition file, domain knowledge document, safe patterns document, anti-patterns document (with explanations of why each pattern is unsafe), required artefacts declaration
- Loading: Deterministic, driven by work item's component tags, issue labels, and safety classification — not by LLM inference
- Timing: Loaded at the Architecture node, before any code generation begins
- Multiple packs: A single pipeline run may load multiple packs simultaneously
- Conflict resolution: Where packs contain contradictory guidance, the more restrictive rule applies
- Versioned: Version-controlled alongside the source code they inform; changes traceable to pipeline runs
- Extensible: New packs addable without changes to the CogWorks pipeline

### Required Artefact

A specific document section, evidence item, or output element declared by a Context Pack that must be present in the pipeline's output for the pack's domain requirements to be satisfied.

- Declared in: Context Pack's `required-artefacts.toml`
- Checked at: Review node (blocking finding if missing)
- Failure output: Identifies which pack declared the requirement and what artefact is missing (actionable, not generic)

### Constitutional Rules

A set of non-overridable behavioral constraints loaded into CogWorks at the start of every pipeline run, before context assembly and before any LLM call.

- Located at: `.cogworks/constitutional-rules.md` (configurable)
- Loading: Unconditional on every pipeline run — NOT a configurable gate (exception to general gate configurability)
- Position: Injected as a privileged, non-overridable component of the LLM system prompt
- Separation: No content in the context package (issue bodies, specs, external docs) may modify, append to, or override the constitutional rules
- Change control: Requires a reviewed and merged PR with at least one human approval before taking effect
- Format: Plain language that a non-specialist reviewer can evaluate

### Prompt Injection

The presence of text in external content (issue bodies, documentation, dependency READMEs) structured to influence LLM behavior as if it were an instruction.

- Examples: Persona override instructions, behavioral modification directives, instruction overrides embedded in code comments
- Detection: Constitutional layer scans external content for injection patterns
- Consequence: Pipeline halt with `INJECTION_DETECTED` event

### Scope Violation

CogWorks generating code that implements capabilities, touches files, or introduces dependencies not present in the approved specification and interface documents.

- Types: Unauthorized network calls, file system access, IPC mechanisms, external process invocations, hardware access not in the interface document
- Detection: Constitutional layer scope enforcement
- Consequence: Pipeline halt with `SCOPE_UNDERSPECIFIED` or `SCOPE_AMBIGUOUS` event

### Authorised File Set

The set of source files a specific work item is permitted to create or modify, derived from the interface document and specification.

- Source: Interface Design node output + Specification document
- Enforcement: Scope enforcer validates generated artifacts against this set
- Violation: Generating files outside this set is a scope violation

### Hold State

A work item state entered after injection detection. The work item is suspended from all automated processing.

- Entry: `INJECTION_DETECTED` event
- Behavior: Work item is NOT automatically requeued or retried
- Exit: Human must explicitly review the flagged content and either confirm false positive (with justification recorded in audit trail) or mark the work item as contaminated
- Label: `cogworks:hold` (distinct from `cogworks:node:failed`)

### Alignment Verification

The process of verifying that a pipeline node's output matches the intent of its upstream input — semantic correctness, distinct from structural correctness (schema validation) and technical correctness (domain service validation).

- Purpose: Catches cases where the output is valid and correct code but doesn't address what was requested
- Examples: Architecture spec solves a different problem than the work item described; interfaces add methods not in the spec; code implements a different algorithm than specified
- Timing: Runs as part of step 4 of the node execution lifecycle, after schema and domain service validation
- Two check types: Deterministic (structural comparison — fast, cheap, no LLM bias) and LLM (semantic comparison — adversarial prompt, ideally different model)
- Output: Alignment result with score, findings, and traceability matrix entries
- Failure triggers: Rework (not retry) with specific misalignment findings in context

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
- Threshold: Configurable per node (default: 0.90; safety-critical: 0.95)
- Pass criteria: Score ≥ threshold AND zero blocking findings
- Tracked in: Audit trail and performance metrics (per stage)
- Related: Satisfaction Score (which measures scenario validation, not stage-to-stage alignment)

### Rework

Re-execution of a node with modified context because the previous output was technically valid but semantically misaligned with its input.

- Distinguished from: Retry (re-execution because the output was technically invalid — didn't compile, didn't parse)
- Context: Rework context includes the specific misalignment findings from the alignment check, giving the LLM targeted feedback on what to fix
- Budget: Separate from retry budget; configurable per node (default: 3 cycles)
- Counter: Tracked independently from retry counter in pipeline state
- Audit: Each rework cycle is recorded with its alignment findings and the resulting output

### Traceability Matrix

A structured artifact mapping each requirement from the work item through the pipeline stages to the final deliverable.

- Built: Incrementally as the pipeline progresses — each alignment check adds its columns
- Columns: Requirement → Architecture → Interface → Sub-Work-Item → Code → Status
- Status values: ✅ (satisfied), ⚠️ N/A (not applicable at this stage, with reason), ❌ (not addressed)
- Published: Posted as a comment on the work item issue at pipeline completion
- ISO 9001: Serves as evidence that requirements flow through to implementation
- Safety: For safety-classified work items, requires human sign-off

### Pipeline Events (Safety)

Structured events emitted by the constitutional layer and scope enforcer when behavioral boundaries are violated.

- **INJECTION_DETECTED**: External content contains text structured as a directive to CogWorks. Includes: pipeline run ID, work item ID, source document, offending text. Triggers pipeline halt and hold state.
- **SCOPE_UNDERSPECIFIED**: Fulfilling the work item would require capabilities not in the approved specification. Includes: missing capability description, relevant spec section. Triggers generation halt.
- **SCOPE_AMBIGUOUS**: Specification is ambiguous for a safety-affecting work item. Includes: ambiguous section, conflicting interpretations. Triggers generation halt and human clarification request.
- **PROTECTED_PATH_VIOLATION**: Generated artifacts match protected path patterns (constitutional rules, prompt templates, scenarios). Triggers pre-PR validation failure.

### Extraction Completeness

A domain service's assessment of whether it could confidently extract all relevant values when computing actual values for cross-domain constraint validation.

- Reported as: `status: "incomplete"` in the `extract_interfaces` response
- Consequence: Constraint validation produces a warning requiring human review, not a silent pass
- Related risk: CW-R07 (Cross-domain constraint false negative)

### Protected Path

A file path pattern identifying files that CogWorks must never create or modify through the normal pipeline.

- Examples: Constitutional rules file, prompt templates, scenario specifications, conformance test suite, output schemas, Extension API schemas
- Enforcement: Pre-PR validation checks generated files against protected path patterns; tool-level scope enforcement prevents writes to protected paths (REQ-TOOL-011)
- Change control: Protected paths require human-approved PR via CODEOWNERS or equivalent

---

## Label Categories

CogWorks uses GitHub labels for two distinct purposes. Labels are split into categories to support mixed human/AI development workflows.

### Pipeline-Internal Labels

Labels that represent CogWorks-internal machine state. These are managed exclusively by CogWorks and should not be manually modified by humans (except during troubleshooting).

- `cogworks:processing` — Concurrency lock indicating an active CLI invocation
- `cogworks:node:<name>` — Tracks the currently active pipeline node(s)
- `cogworks:node:failed` — Indicates a node has failed
- `cogworks:status:<status>` — Tracks sub-work-item processing status

These labels always use the `cogworks:` prefix and are not configurable.

### Workflow-Semantic Labels

Labels that represent workflow state meaningful to both human and AI developers. These use configurable names so teams can reuse their existing label conventions.

- Readiness signal (default label mapping: `cogworks:run`) — Initiates the pipeline through the configured EventSource/work-item adapter
- Restart signal (default label mapping: `cogworks:restart`) — Requests a full pipeline restart through the configured adapter
- Awaiting review label (default: `cogworks:awaiting-review`) — A gate is waiting for human approval
- Safety-critical label (default: `cogworks:safety-critical`) — Work item touches safety-critical code
- Hold label (default: `cogworks:hold`) — Work item suspended pending human review (injection detection)
- Cancellation signal (default label mapping: `cogworks:cancel`) — Signals pipeline cancellation through the configured EventSource/work-item adapter
- Sub-work-item label (default: `cogworks:sub-work-item`) — Identifies sub-work-item issues

Label mappings are configurable via the `[labels]` section in `.cogworks/config.toml` or the corresponding provider adapter. These values are defaults, not fixed business-logic signals; teams may map readiness and cancellation to their own work-item conventions.

---

## Milestone

A GitHub Milestone associated with a work item, representing a development phase, release target, or software version.

- Optional: Work items are not required to have a milestone
- Inherited: When CogWorks creates sub-work-items, each sub-work-item inherits the parent work item's milestone (if set)
- Filtering: The `cogworks run-all` command supports filtering by milestone (e.g., `cogworks run-all --repo <owner/repo> --milestone "v2.0"`) to process only work items in a specific release phase
- Not modified: CogWorks MUST NOT create, modify, or delete milestones. Milestone management is a human responsibility
- Visibility: Milestones provide natural grouping for human oversight, showing which work items belong to which release or development phase

---

## LLM Rate Limiting

The mechanism by which the LLM Gateway respects provider-imposed API request quotas.

- Scope: Per-provider, per-API-key rate limits (e.g., 100 calls per 5 hours, or requests-per-minute)
- Tracking: The LLM Gateway tracks rate-limit state from provider response headers (`retry-after`, `x-ratelimit-remaining`, etc.)
- Proactive throttling: When the remaining request budget is low, the gateway queues pending calls and spaces them to avoid exhausting the budget
- Reactive backoff: When an HTTP 429 is received, the gateway waits for the duration specified in the `retry-after` header before retrying
- Coordination: Rate-limit state is shared across parallel nodes executing within the same pipeline invocation to prevent concurrent calls from collectively exceeding the limit
- Halt threshold: If the required wait time exceeds a configurable maximum (default: 30 minutes), the current step halts with a retriable exit code rather than blocking indefinitely
- Distinguished from cost budget: Rate limiting controls request frequency; cost budget controls total token spend. Both are enforced independently

---

## Semantic Stalling

A condition where LLM retries produce novel but equally incorrect output, consuming retry budget without making progress.

- Detection: The LLM Gateway or retry logic tracks diagnostic categories (e.g., `type_error`, `constraint_violation`) across consecutive retry attempts. If the same diagnostic category recurs for a configurable number of consecutive retries (default: 3), the system flags semantic stalling
- Distinguished from: Flaky tests (identical code, different results — EDGE-015) and normal retries (different errors on each attempt suggesting progress)
- Response: Escalation with a structured report identifying the recurring error category and all attempts, rather than consuming the remaining retry budget
- Rationale: An LLM that produces different code but hits the same category of error is unlikely to resolve the issue with more retries — the problem is likely a misunderstanding of requirements, not a transient failure

---

## GitHub Projects Integration

Optional integration with GitHub Projects V2 for human oversight dashboards.

- Purpose: Provides a visual overview of all active work items, their pipeline stage, safety classification, and blocked status via a GitHub Project board
- Configuration: Optional `[github_project]` section in `.cogworks/config.toml` specifying project ID and field mappings
- Behaviour: When configured, CogWorks updates the project board at pipeline boundaries (work item added on pipeline start, status field updated at node transitions, removed or marked complete on pipeline finish)
- Non-blocking: Project board updates are fire-and-forget. Failures to update the project board are logged as warnings but MUST NOT block, slow, or fail the pipeline
- Not required: CogWorks operates identically with or without a GitHub Project configuration. The project board is a visibility tool, not a state store
- Fields: Configurable mapping between pipeline state (current node, safety classification, disposition) and project board custom fields
- Related risk: CW-R18 (CogWorks modifies its own prompts or scenarios)

---

## Tool System Concepts

### Tool

A function available to an LLM node during pipeline execution that allows it to interact with the environment (read files, write files, run git commands, invoke domain services).

- Identified by: Unique name in the tool registry (e.g., `fs.read`, `git.commit`, `domain.validate`)
- Contains: Name, category, description, parameter schema (JSON Schema), scope parameters, implementation
- Scope parameters: Parameters injected by the orchestrator (not visible to the LLM) that enforce boundaries (allowed paths, branch patterns, etc.)
- Constraint: A tool not in the node's profile does not exist from the LLM's perspective — it is never offered in the LLM call's tool list

### Tool Registry

The orchestrator's central registry of all available tools, including built-in tools, skill-derived tools, and adapter-generated tools.

- Contains: Tool definitions with schemas, scope parameter declarations, and implementations
- Sources: Built-in tools (filesystem, git, domain service), generated adapter tools (from OpenAPI/EAB specs), registered skills
- Resolution: Maps tool names to implementations for invocation; validates tool existence before execution
- Access control: Tool profiles select subsets of the registry for each node

### Tool Profile

A named collection of tools with their scope parameters, assigned to pipeline nodes to control what the LLM can access during execution.

- Identified by: Human-readable name (e.g., `reader`, `generator`, `reviewer`, `architect`)
- Contains: List of allowed tool names, per-tool scope parameter values (allowed paths, branch patterns, etc.)
- Configuration: Defined in `.cogworks/pipeline.toml` under `[tool_profiles.*]`
- Inheritance: Nodes reference a base profile and MAY override specific scope parameters via `tool_overrides`
- Default profiles: The orchestrator provides default profiles for all 7 core nodes (Intake, Architecture, Interface Design, Planning, Code Generation, Review, Integration)
- Template variables: Scope parameters support `{{variable}}` placeholders resolved at runtime (e.g., `{{affected_modules}}` from architecture output)

### Scope Parameter

A parameter injected by the orchestrator into a tool invocation that constrains the tool's behavior, invisible to the LLM.

- Examples: `allowed_write_paths`, `denied_read_paths`, `branch_pattern`, `allowed_services`, `max_file_size_bytes`
- Enforcement: Validated by the tool implementation before executing (defence in depth — independent of orchestrator filtering)
- Violation handling: Refused operation, clear error message to LLM, audit trail entry

### Tool Scope Violation

An event where a tool call is rejected because it violates the calling node's scope parameters.

- Examples: Writing to a path outside `allowed_write_paths`, committing to a branch not matching `branch_pattern`
- Handling: Tool refuses the operation, returns an error explaining the constraint, logs the violation in the audit trail
- Monitoring: High scope violation rates may indicate prompt injection, tool profile misconfiguration, or poor prompt instructions
- Threshold: Repeated violations within a single node execution (default: 5) trigger a warning and may halt the node

### Skill

A parameterised, deterministic script that replays a proven tool invocation pattern without LLM involvement.

- Stored: `.cogworks/skills/` as a TOML manifest + bash script
- Extracted from: Successful pipeline runs by analysing the audit trail (offline, human-reviewed)
- Parameters: Named, typed parameters replacing values that vary between runs (e.g., module paths, compilation targets)
- Scope enforcement: Every tool call within a skill is validated against the calling node's tool profile — a skill cannot bypass scope restrictions
- Exposed as tools: `skill.list`, `skill.run`, `skill.inspect` appear in the LLM's tool list
- Lifecycle: Proposed → Reviewed → Active → Deprecated → Retired

### Skill Lifecycle State

The current state of a skill in its lifecycle.

- **Proposed**: Extracted from audit trail analysis; not yet available to pipeline nodes
- **Reviewed**: Human has reviewed and approved; committed to `.cogworks/skills/`. May transition to Active (activate), back to Proposed (rework), or to Retired (abandon).
- **Active**: Available to pipeline nodes; included in tool lists
- **Deprecated**: Marked as outdated (e.g., success rate dropped below threshold); still available but alternatives are preferred
- **Retired**: Removed from tool lists; retained in version control for reference

### Adapter

A set of auto-generated tool definitions produced from an external API specification (OpenAPI, EAB schema) that register in the tool registry.

- Sources: OpenAPI 3.0/3.1 specs (JSON/YAML), EAB schema format (JSON Schema)
- Produces: Per-operation tool definition (TOML), index file, configuration file
- Namespaced: `inventree.stock.list`, `domain.validate`, etc.
- Scope integration: Adapter tools are subject to the same tool profile scoping as all other tools
- Storage: `.cogworks/adapters/<name>/`, version-controlled as derived artifacts (spec is source of truth)
- Regeneration: Regenerable at any time from the source spec; CI check verifies no drift

### Progressive Discovery

A mode of tool presentation where the LLM receives a compact tool index (name + one-line description) instead of full schemas, and requests full schemas on demand via meta-tools.

- Trigger: Automatically activated when the node's tool count exceeds a threshold (default: 15)
- Meta-tools: `tools.search` (semantic search), `tools.schema` (get full schema), `tools.call` (invoke tool)
- Skills priority: Skills rank above raw tools in search results
- Token savings: Reduces context usage when nodes have access to many tools (especially with adapters)

### Tool Usage Report

A summary of tool invocations included in the pipeline completion summary.

- Contains: Per-tool call counts, scope violation counts, invocation type breakdown (raw/skill/discovery)
- Purpose: Identifies tool usage patterns — excessive calls may indicate poor context assembly or prompt instructions
- Audit: Every individual tool call is recorded in the audit trail with parameters, scope, result, and duration
