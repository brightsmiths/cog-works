# Architecture: Boundaries and Data Flow

This document defines the clean architecture boundaries — what is business logic, what are external system abstractions, and what are infrastructure implementations. Business logic depends only on abstractions, never on concrete infrastructure.

---

## Layered View

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        Business Logic                                   │
│  (Pure domain concepts — no I/O, no external dependencies)              │
│                                                                         │
│  Pipeline state machine    │  Classification rules                      │
│  Pipeline graph (DAG+cycles)│  Review aggregation                       │
│  Graph execution engine    │  Context priority & truncation             │
│  Budget enforcement        │  Scope threshold evaluation                │
│  Node gate logic           │  Retry budget tracking                     │
│  Edge condition evaluation │  Cross-domain constraint validation        │
│  Label parsing/generation  │  Computed constraint evaluation            │
│  Interface registry valid. │  Scenario satisfaction                     │
│  Context Pack selection    │  Constitutional rules enforcement          │
│  Injection detection       │  Scope enforcement                         │
│  Required artefact check   │  Protected path validation                 │
│  Tool scope enforcement    │  Tool profile resolution                   │
│  Skill execution           │  Progressive discovery index               │
└─────────────────────┬───────────────────────────────────────────────────┘
                      │ depends on (abstractions only)
                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                   External System Abstractions                          │
│  (Traits/interfaces — define what external systems provide)             │
│                                                                         │
│  LLM Provider              │  Issue Tracker                             │
│  Code Repository           │  Pull Request Manager                      │
│  Domain Service Client     │  Template Engine                           │
│  Interface Registry Loader │  Audit Store                               │
│  Scenario Executor         │  Twin Provisioner                          │
│  Summary Cache             │  Pipeline Configuration Loader             │
│  Tool Profile Store        │  Adapter Spec Loader                       │
│  Project Board (optional)  │                                            │
└─────────────────────┬───────────────────────────────────────────────────┘
                      │ implemented by
                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                   Infrastructure Implementations                        │
│  (Concrete adapters — the only code that touches external systems)      │
│                                                                         │
│  Anthropic Messages API    │  GitHub REST/GraphQL/Projects API          │
│  Extension API Client      │  Handlebars template engine                │
│  (Unix socket / HTTP)      │  GitHub issue comments (audit)             │
│  (Future: gRPC transport)  │  (Future: OpenAI, GitLab, etc.)            │
│  TOML Profile Loader       │  OpenAPI/EAB Adapter Generator             │
└─────────────────────────────────────────────────────────────────────────┘
```

Note: Domain services (Rust, KiCad, FreeCAD, etc.) are **external processes**, not part of CogWorks. They communicate through the Extension API and are not shown in the layered view above. CogWorks' Extension API Client is an infrastructure implementation that handles the transport layer.

CogWorks remains domain-agnostic across software and other engineering work. The orchestrator owns implementation-job lifecycle decisions and requests one ephemeral container per implementation job through the runtime abstraction. Runtime creation and hosting are provider-specific infrastructure; domain-specific tests and validation remain behind the Domain Service Client.

---

## Business Logic

Business logic contains zero I/O. It operates on data structures passed in as arguments and returns data structures as results. It can be unit-tested with no mocks — only pure input/output assertions.

### Pipeline State Machine

Determines valid node transitions and next actions by traversing the pipeline graph.

- **Input**: Current pipeline state (active/completed/pending/failed nodes, edge conditions, sub-work-item statuses, PR statuses, gate configuration, pipeline graph definition)
- **Output**: Next action(s) to take (which node(s) to activate, or "wait", or "escalate")
- **Rules**:
  - A node cannot start until all its incoming edge conditions are satisfied and all declared inputs are available
  - Safety-critical work items force human gates for code-producing nodes
  - `cogworks:processing` label means another instance is active — back off
  - Multiple nodes may be activatable simultaneously (parallel fan-out) when the graph allows
  - Fan-in nodes stay pending until all upstream parallel nodes complete
  - Rework edges track traversal counts and enforce termination conditions
  - When no pipeline configuration exists, uses the default linear 7-node pipeline

### Graph Execution Engine

Coordinates the execution of pipeline graph nodes, evaluates edge conditions, and manages parallel execution.

- **Input**: Pipeline graph definition, current pipeline state, node implementations
- **Output**: Updated pipeline state after executing one graph traversal pass
- **Rules**:
  - Computes node execution order from edge definitions using topological sorting
  - Evaluates outgoing edge conditions when a node completes, activating all eligible downstream nodes
  - For `all-matching` edges: all true edges are taken (fan-out)
  - For `first-matching` edges: only the first true edge is taken (exclusive routing)
  - For `explicit` edges: node output names the edges to take
  - Parallel nodes execute as concurrent async tasks within the orchestrator process
  - Respects maximum concurrent LLM calls limit (configurable, default: 3)
  - Handles partial failure: other nodes continue unless failed node is marked `abort_siblings_on_failure`
  - Writes pipeline state to GitHub at each node boundary (crash-recovery point)

### Implementation Job Orchestration

The orchestrator requests and tracks one ephemeral container for each implementation job. It owns the job lifecycle, including requesting cleanup after success, failure, or cancellation. The runtime provider creates and operates the container; business logic does not depend on Docker, Kubernetes, or a cloud runtime. Domain-specific tests and validation remain behind the Domain Service Client.

### Edge Condition Evaluation

Evaluates edge conditions to determine which downstream nodes to activate.

- **Input**: Edge condition definition (deterministic expression, LLM-evaluated condition, or composite), current pipeline state, node output
- **Output**: Boolean (edge taken or not taken), evaluation record for audit trail
- **Rules**:
  - Deterministic conditions: evaluated by the orchestrator against pipeline state via a simple expression language
  - LLM-evaluated conditions: natural-language conditions assessed by the LLM against pipeline context; must be recorded in the audit trail
  - Composite conditions: boolean combinations (AND/OR/NOT) of deterministic and LLM-evaluated conditions
  - LLM-evaluated conditions have a deterministic fallback when the LLM is unavailable or returns an ambiguous response

### Classification Rules

Deterministic post-processing of LLM classification output.

- **Input**: LLM classification result, safety-critical module registry
- **Output**: Final classification (potentially with safety override)
- **Rules**:
  - If any affected module is in the safety-critical registry → override `safety_affecting` to true
  - If estimated scope exceeds threshold → produce escalation result

### Dependency Graph

Topological sort and validation of sub-work-item dependencies.

- **Input**: List of sub-work-items with declared `depends_on` relationships
- **Output**: Ordered list (topological sort) or error (cycle detected)
- **Rules**:
  - Cycles are rejected with a specific error identifying the cycle
  - The topological ordering is deterministic (stable sort by sub-work-item ID within each tier)

### Review Aggregation

Combines multiple review pass results into a single decision.

- **Input**: Three review results (quality, architecture, security), each with per-finding severity
- **Output**: Aggregate result (proceed / remediate / escalate) with categorized findings
- **Rules**:
  - Any `blocking` finding in any pass → aggregate result is `remediate`
  - If remediation cycle count exceeds limit → aggregate result is `escalate`
  - `warning` and `informational` findings are collected but don't block

### Budget Enforcement

Pure arithmetic on token usage and cost.

- **Input**: Current accumulated cost, proposed call's estimated tokens, model cost rates, budget limit, parallel execution context
- **Output**: Approved (proceed) or denied (budget exceeded)
- **Rules**:
  - If accumulated + estimated > budget → deny and produce cost report
  - Cost report includes per-node and per-sub-work-item breakdown
  - Parallel nodes share the pipeline's total cost budget; budget enforcement must be atomic across concurrent nodes

### Context Priority and Truncation

Deterministic selection of context items when the full package exceeds the model's window.

- **Input**: Full context package (all candidate items), target model's context window size, token counter
- **Output**: Truncated context package that fits within the window
- **Rules** (priority order, highest first):
  1. Current sub-work-item's interface definition
  2. Directly depended-upon sub-work-item outputs
  3. Architectural constraints document
  4. Context Pack domain knowledge (from loaded packs)
  5. Coding standards document
  6. Remaining items ranked by import-graph proximity to affected modules

### Label Parsing and Generation

Converts between structured pipeline state and GitHub label strings.

- **Input/Output**: Bidirectional mapping for CogWorks pipeline-state labels. Readiness signals and external status mappings are handled by the GitHub/EventSource adapter; no trigger label or project-status mapping is fixed by business logic.
- **Note**: Dependency and ordering information is read from native GitHub typed issue links (`blocks` / `is blocked by`) and the sub-issue API, not from labels.

### Interface Registry Validation

Deterministic validation of the cross-domain interface registry.

- **Input**: Interface definition files from `.cogworks/interfaces/`, registered domain services
- **Output**: Valid (all definitions conform to schema, no conflicts) or list of validation errors
- **Rules**:
  - All definitions must conform to the interface definition JSON Schema
  - All cross-references between interfaces must resolve
  - No two interfaces may define conflicting constraints for the same physical parameter
  - All referenced domains must have a registered domain service (or be marked `external`)
  - Domain service / interface version mismatches are flagged as blocking
  - Runs before any pipeline node on every invocation

### Cross-Domain Constraint Validation

Deterministic comparison of generated artifact values against interface registry contracts.

- **Input**: Relevant interface contracts, extracted interface values from generated artifacts, computed constraint definitions
- **Output**: List of structured findings (interface ID, parameter, expected, actual, owning domain, violating domain, severity)
- **Rules**:
  - Hard constraint violations (outside min/max bounds) are blocking
  - Nominal value deviations are warnings
  - Computed constraints (e.g., total bus load) are evaluated deterministically by the validator
  - Validates against registry contracts only — does not require other domain services to be running
  - Runs during review gate (first, before LLM reviews) and during architecture node

### Scenario Satisfaction Scoring

Computes satisfaction scores from trajectory results.

- **Input**: Per-scenario trajectory results (each trajectory has a boolean satisfaction determination), explicit failure criteria violations
- **Output**: Per-scenario satisfaction score (0.0-1.0), overall satisfaction score, pass/fail decision
- **Rules**:
  - Satisfaction score = (satisfied trajectories / total trajectories)
  - Overall score must meet threshold (default 0.95) to proceed
  - Any explicit failure criterion violation → fail immediately regardless of score
  - Missing applicable scenarios → skip validation (not an error)
  - Context Packs may declare stricter thresholds for their domain; the stricter threshold applies

### Context Pack Selection

Deterministic selection of Context Packs based on work item classification.

- **Input**: Work item classification (labels, component tags, safety classification), available Context Pack trigger definitions
- **Output**: List of matched Context Pack identifiers and their loaded content (domain knowledge, safe patterns, anti-patterns, required artefacts)
- **Rules**:
  - Trigger evaluation is a pure function: classification data in, matched packs out
  - A work item may match multiple packs simultaneously
  - A matched pack is always loaded (no option to skip)
  - Where packs contain contradictory guidance, the more restrictive rule applies
  - Pack loading occurs at Architecture node, before any LLM generation call

### Constitutional Rules Enforcement

Validation that constitutional rules are present and correctly positioned.

- **Input**: Constitutional rules document content, LLM prompt being assembled
- **Output**: Validated prompt with constitutional rules in privileged system prompt position
- **Rules**:
  - Rules must be loaded on every pipeline run (no exceptions)
  - Rules occupy a privileged position in the system prompt — no context content may override them
  - Required core rules must be present (external content as data, injection detection, scope binding, unauthorized capabilities prohibition, no credential generation)
  - Rules from unreviewed branches are rejected

### Injection Detection

Pattern matching and analysis of external content for prompt injection attempts.

- **Input**: External content (issue bodies, specifications, dependency docs, API responses)
- **Output**: Clean (no injection detected) or INJECTION_DETECTED event with source document and offending text
- **Rules**:
  - External content is scanned before inclusion in any LLM prompt
  - Detection of injection patterns triggers immediate pipeline halt
  - The work item enters hold state (no automatic requeue)
  - Detection includes: persona overrides, instruction injections, behavioral modifications, system prompt extraction attempts
  - False positive resolution requires explicit human review with justification

### Scope Enforcement

Validation that generated artifacts stay within the approved specification scope.

- **Input**: Generated artifact paths and content, approved specification scope, interface document, protected path patterns
- **Output**: Clean (within scope) or scope violation events (SCOPE_UNDERSPECIFIED, SCOPE_AMBIGUOUS, PROTECTED_PATH_VIOLATION)
- **Rules**:
  - Generated files must be within the authorised file set derived from spec and interface documents
  - No unauthorized capabilities (network calls, file system access, IPC, hardware access) unless explicitly specified
  - No generated files may match protected path patterns (constitutional rules, prompt templates, scenarios)
  - Scope violations for safety-affecting work items require human clarification

### Required Artefact Checking

Verification that all artefacts declared by loaded Context Packs are present.

- **Input**: Required artefact declarations from loaded Context Packs, generated pipeline output
- **Output**: Pass (all present) or blocking findings identifying the pack and missing artefact
- **Rules**:
  - Checked during the Review node
  - Missing artefacts produce blocking findings (preventing PR creation)
  - Each finding identifies which pack declared the requirement and what is missing

### Tool Profile Resolution

Determines the effective tool list for a pipeline node at activation time.

- **Input**: Node identifier, base profile name (from pipeline configuration), per-node tool overrides, pipeline state (for template variable resolution), tool registry contents
- **Output**: Resolved tool list with per-tool scope parameters (all template variables substituted)
- **Rules**:
  - Profile resolution is deterministic: same configuration + same pipeline state = same tool list
  - Unknown tool names in a profile produce a configuration warning (not a hard error — allows profile reuse across repos with different adapter sets)
  - Template variables (e.g., `{{affected_modules}}`) are resolved from pipeline state at activation time; unresolvable variables produce a clear error and prevent node activation
  - Default profiles exist for all 7 core nodes; no configuration needed for out-of-the-box operation
  - The resolved tool list is recorded in the audit trail

### Tool Scope Validation

Validates a tool invocation against the calling node's scope parameters before execution.

- **Input**: Tool name, tool invocation parameters (LLM-supplied), scope parameters (orchestrator-injected, from the resolved tool profile)
- **Output**: Pass (tool may execute) or scope violation (structured error identifying the violated constraint)
- **Rules**:
  - This is the second layer of defence in depth (after orchestrator-level tool filtering, before OS-level sandboxing)
  - Operates independently of orchestrator filtering — even if a tool is in the profile, scope parameters may reject a specific invocation
  - Protected path enforcement: `fs.write` and `fs.create` refuse operations targeting protected paths regardless of profile configuration
  - Scope violations are counted per node execution; exceeding the threshold (default: 5) produces an audit warning and MAY halt the node
  - Each violation is recorded as a `SCOPE_VIOLATION` event in the audit trail

### Skill Validation and Execution

Validates skill parameters and orchestrates skill script execution, enforcing scope constraints on every tool call within the skill.

- **Input**: Skill name, skill parameters (from LLM invocation), skill manifest (TOML), calling node's tool profile
- **Output**: Skill execution result (success with output, or failure with error)
- **Rules**:
  - Skill parameters are validated against the manifest schema before execution
  - Each tool call within a skill is validated against the calling node's tool profile — skills cannot bypass scope enforcement
  - Skill execution is deterministic: same parameters produce the same sequence of tool invocations
  - Skill lifecycle state is checked: only Active skills can be invoked (Proposed, Deprecated with alternatives, Retired are rejected)

### Progressive Discovery Index

Generates a compact tool index for nodes with many tools, and handles meta-tool requests for on-demand schema expansion.

- **Input**: Resolved tool list for a node, tool count threshold (default: 15), search query (for `tools.search`), tool name (for `tools.schema`)
- **Output**: Compact tool index (name + one-line description per tool), or full schema for a specific tool, or search results ranked by relevance
- **Rules**:
  - Activated automatically when tool count exceeds the threshold
  - Skills rank above raw tools in search results
  - Meta-tools (`tools.search`, `tools.schema`, `tools.call`) are always available when progressive discovery is active
  - The compact index replaces full schemas in the LLM tool list to reduce context token consumption

### Tracing-Based Observability

Pipeline lifecycle instrumentation records structured fields on tracing spans. The CLI configures structured console output and the OpenTelemetry exporter. External systems may derive metrics from available span attributes; CogWorks defines no metric-sink trait or separate metric data-point computation abstraction. The instrumentation contract is limited to the stable fields declared by the observability interface.

### Alignment Verification

Deterministic and LLM-based verification that a node's output matches the intent of its upstream inputs. This is the semantic correctness layer — distinct from schema validation (structural correctness) and domain service validation (technical correctness).

- **Input**: Node's upstream inputs (work item, architecture spec, interface definitions, sub-work-item description — depending on stage), node's output, alignment configuration (threshold, check types, rework budget)
- **Output**: Alignment result (aligned/not-aligned, score, findings list, traceability matrix entries)
- **Rules**:
  - Runs as part of step 4 of the node execution lifecycle (REQ-EXEC-003), after schema and domain service validation
  - Two check types: deterministic (structural comparison) and LLM (semantic comparison)
  - Deterministic checks run first (cheap, fast, no LLM bias) — their results are included in the LLM check context so the LLM can focus on what requires judgement
  - LLM alignment check uses an adversarial prompt focused on identifying misalignment, not assessing quality
  - LLM alignment check SHOULD use a different model from the generating node (reduces correlated bias — CW-R02)
  - Alignment failures trigger rework (not retry): the node re-enters with specific misalignment findings as additional context
  - Rework counter is separate from retry counter; both have independent budgets
  - Alignment score must meet configurable threshold (default: 0.90) AND have zero blocking findings to pass
  - Safety-classified work items use stricter thresholds (default: 0.95) and cannot disable LLM alignment checks
  - Each alignment check contributes entries to the incremental traceability matrix

### Traceability Matrix Construction

Incrementally builds a requirement-to-implementation traceability matrix across pipeline stages.

- **Input**: Work item requirements (extracted from the work item body during the first alignment check — the Architecture node's alignment check parses the work item into a structured requirements list to seed the matrix rows), per-stage alignment check results, pipeline completion status
- **Output**: Structured traceability matrix mapping each requirement through Architecture → Interface → Sub-Work-Item → Code → Status
- **Rules**:
  - Matrix is accumulated in the pipeline state — each alignment check adds its columns
  - Requirements not addressed at a stage are marked "N/A" with a reason (not treated as failures)
  - On pipeline completion, the matrix is posted as a comment on the work item and included in the audit trail
  - For safety-classified work items, the traceability matrix requires human sign-off
  - Matrix construction is deterministic given the same alignment check results

---

## External System Abstractions

These are traits (in Rust terms) that define what the business logic needs from external systems. Business logic only depends on these abstractions — never on their implementations.

### LLM Provider

Sends a prompt with a context package and receives a structured response.

- **Operations**:
  - `complete(prompt, context, output_schema, model_config) → StructuredResponse` — Send a prompt and receive a response validated against the output schema
- **Data flowing across boundary**:
  - Inbound: prompt text, context items (strings), output schema (JSON Schema), model identifier
  - Outbound: parsed response (validated against schema), token count (input + output), latency
- **Error cases**: API failure (retryable), rate limit (retryable with backoff), schema validation failure (retry with error appended), budget exceeded (non-retryable)
- **Note**: Constitutional rules are injected into the system prompt by the LLM Gateway before calling the LLM Provider. The provider receives the full prompt (including constitutional rules) but is not aware of the constitutional layer — it is a transport concern handled by the gateway.

### Issue Tracker

Reads and manages work items and sub-work-items.

- **Operations**:
  - `get_issue(id) → Issue` — Read an issue's full details
  - `list_sub_issues(parent_id) → Vec<Issue>` — List native sub-issues of a parent
  - `create_sub_issue(parent_id, details) → Issue` — Create a native sub-issue of the parent work item
  - `add_typed_link(source_id, target_id, link_type)` — Create a typed issue link (e.g., `blocks` / `is blocked by`) for dependency tracking
  - `get_typed_links(issue_id) → Vec<TypedLink>` — Read typed issue links for dependency resolution
  - `get_labels(issue_id) → Vec<Label>` — Read labels
  - `add_label(issue_id, label)` — Apply a label
  - `remove_label(issue_id, label)` — Remove a label
  - `post_comment(issue_id, body)` — Post a comment
  - `get_issue_state(issue_id) → IssueState` — Check if open/closed
  - `get_milestone(issue_id) → Option<Milestone>` — Read the milestone assigned to an issue
  - `set_milestone(issue_id, milestone_id)` — Set the milestone on an issue (used for inheritance from parent to sub-work-items)
- **Data flowing across boundary**:
  - Inbound (from external): issue details (title, body, labels, state, milestone), sub-issue lists, typed link lists
  - Outbound (to external): issue creation details, label changes, comments, typed link creation, milestone assignment

### Pull Request Manager

Creates and reads Pull Requests.

- **Operations**:
  - `create_pull_request(branch, base, title, body, references) → PullRequest` — Create a PR
  - `get_pull_request(id) → PullRequest` — Read PR details
  - `find_pull_requests(filters) → Vec<PullRequest>` — Search for PRs by branch, labels, etc.
  - `post_review_comment(pr_id, file, line, body)` — Post an inline review comment
  - `get_review_status(pr_id) → ReviewStatus` — Check approval/rejection status
- **Data flowing across boundary**:
  - Inbound: PR details (state, reviews, merge status)
  - Outbound: PR creation details, review comments

### Code Repository

Reads repository structure and file contents.

- **Operations**:
  - `read_file(path, ref) → FileContent` — Read a single file at a given ref
  - `list_directory(path, ref) → Vec<Entry>` — List directory contents
  - `file_exists(path, ref) → bool` — Check if a file exists
  - `read_tree(paths, ref) → Map<Path, FileContent>` — Batch read multiple files
- **Data flowing across boundary**:
  - Inbound: file contents, directory listings
  - Outbound: file paths, git refs

### Domain Service Client

Invokes domain service methods through the Extension API protocol.

- **Operations**:
  - `validate(artifacts, interfaces) → Diagnostics` — Check domain rules; return structured errors/warnings
  - `normalise(artifacts) → NormaliseResult` — Apply canonical formatting; report whether changes were needed
  - `review_rules(artifacts, rule_config) → Diagnostics` — Run domain-specific rules; return structured findings
  - `simulate(filter, environment) → SimulationResults` — Run tests/simulations (optionally filtered); return pass/fail per case with failure output
  - `validate_deps(declarations) → DependencyResult` — Check declared dependencies are valid
  - `extract_interfaces(artifacts) → InterfaceMap` — Parse artifacts and extract public interface definitions
  - `dependency_graph(artifact_list) → DependencyGraph` — Build artifact dependency graph
  - `health_check() → HealthStatus` — Verify service availability, negotiate API version, discover capabilities and supported artifact types/interface types via handshake protocol
- **Data flowing across boundary**:
  - Inbound: structured diagnostics (artifact, location, severity, message), simulation results (name, pass/fail, output), interface definitions, dependency graph, handshake results (capabilities, artifact types, interface types, domain, API version)
  - Outbound: artifact paths, test/simulation filters, scenario specifications, environment configuration, relevant interface registry entries
- **Error cases**: Service unavailable (retryable), API version mismatch (non-retryable), method not supported (non-retryable), transport error (retryable with backoff), tool not found (non-retryable), operation timeout (potentially retryable)

**Note**: The Domain Service Client is the CogWorks-side abstraction. It sends JSON messages over Unix socket (default) or HTTP/gRPC. The actual domain service is an external process that CogWorks does not manage. Capabilities, artifact types, interface types, and domain are discovered dynamically via the handshake — not statically configured. Future API versions may add progress polling or streaming for long-running operations.

### Interface Registry Loader

Loads cross-domain interface definitions from the repository.

- **Operations**:
  - `load_definitions(directory) → Vec<InterfaceDefinition>` — Load and parse all TOML files from the registry directory
  - `validate_schema(definition) → ValidationResult` — Check a definition against the interface definition JSON Schema
- **Data flowing across boundary**:
  - Inbound: parsed interface definitions (structured)
  - Outbound: directory path, raw file contents

### Scenario Executor

Executes scenario trajectories and evaluates acceptance criteria.

- **Operations**:
  - `load_scenarios(directory, module_filter) → Vec<Scenario>` — Load scenario specifications applicable to given modules
  - `execute_trajectory(scenario, twin_environment) → TrajectoryResult` — Run one trajectory of a scenario
  - `evaluate_acceptance(trajectory_result, acceptance_criteria, method) → SatisfactionDetermination` — Evaluate whether a trajectory satisfies criteria using specified method (deterministic assertion, LLM-as-judge, or statistical)
- **Data flowing across boundary**:
  - Inbound: scenario specifications (from holdout location), trajectory results, satisfaction determinations
  - Outbound: scenario loading requests, trajectory execution requests, evaluation requests

### Twin Provisioner

Manages Digital Twin instances for scenario execution.

- **Operations**:
  - `start_twin(twin_spec, isolated_state) → TwinHandle` — Start a twin instance with fresh state
  - `stop_twin(handle)` — Stop a running twin instance
  - `configure_failure_injection(handle, failure_profile)` — Configure failure modes for a twin
  - `reset_twin_state(handle)` — Reset twin to initial state for next trajectory
- **Data flowing across boundary**:
  - Inbound: twin specifications, state reset confirmations
  - Outbound: twin process handles, endpoint URLs/ports for scenario execution

### Summary Cache

Reads and manages pyramid summaries of modules.

- **Operations**:
  - `get_summary(module, level) → Summary` — Retrieve cached summary at specified level (1, 2, or 3)
  - `is_stale(module) → bool` — Check if cached summary is outdated relative to source
  - `invalidate(module)` — Mark a module's summaries as needing regeneration
- **Data flowing across boundary**:
  - Inbound: cached summaries (Level 1/2/3 text), staleness indicators
  - Outbound: module identifiers, level requests

### Pipeline Configuration Loader

Loads and validates pipeline graph definitions from the repository.

- **Operations**:
  - `load_pipeline_config(directory) → PipelineConfig` — Load and parse the `.cogworks/pipeline.toml` file
  - `get_named_pipeline(config, name) → PipelineGraph` — Retrieve a specific named pipeline graph definition
  - `get_default_pipeline() → PipelineGraph` — Return the built-in default linear pipeline when no configuration file exists
- **Data flowing across boundary**:
  - Inbound: parsed pipeline configuration (node definitions, edge definitions, pipeline-level settings, named pipeline maps)
  - Outbound: directory path, raw file contents
- **Error cases**: Missing configuration file (falls back to default), invalid TOML (non-retryable), schema violation (non-retryable with error details), graph validation failure (cycle without termination condition, unreachable nodes, etc.)

### Template Engine

Renders prompt templates with variable substitution.

- **Operations**:
  - `render(template_name, variables) → String` — Render a named template with the given variables
  - `list_required_variables(template_name) → Vec<String>` — List variables a template expects
- **Data flowing across boundary**:
  - Inbound: rendered prompt text
  - Outbound: template name, variable map

### Audit Store

Persists audit events.

- **Operations**:
  - `record_event(pipeline_id, event)` — Record a single audit event
  - `write_summary(pipeline_id, summary)` — Write a pipeline summary
- **Data flowing across boundary**:
  - Outbound: structured audit events (LLM call records, validation results, state transitions, cost data)

### Project Board (Optional)

Synchronises work item status to an external project board for human oversight.

- **Operations**:
  - `sync_item_status(issue_id, status_field, value) → Result` — Update a work item's status field on the project board
  - `sync_custom_field(issue_id, field_name, value) → Result` — Set a custom field value (e.g., current pipeline node, classification)
- **Data flowing across boundary**:
  - Outbound: issue identifiers, field names, field values
- **Error cases**: Board unavailable (non-fatal — logged, pipeline continues), field not found (non-fatal — logged), authentication failure (non-fatal — logged)
- **Constraint**: Project board operations are fire-and-forget. Failures MUST NOT block or slow pipeline execution. This abstraction is optional — if not configured, no calls are made.

### Implementation Job Runtime Provider

Provides the infrastructure capability used by the orchestrator to request one ephemeral container per implementation job. The provider owns runtime-specific operations; the orchestrator remains responsible for deciding when a job starts and requesting cleanup on all terminal paths. This specification does not prescribe provider methods, request fields, or runtime-specific configuration.

### Credential Provider

Resolves credentials for infrastructure clients through the Vault library. Authentication and key lifecycle are provider concerns behind this interface. CogWorks does not define Vault APIs or lifecycle policy, and key material is not available to prompts, context, logs, audit records, or generated artifacts.

### Tool Profile Store

Loads tool profile definitions and per-node tool overrides from the pipeline configuration file.

- **Operations**:
  - `load_profiles(config) → Map<ProfileName, ToolProfile>` — Load all tool profile definitions from `.cogworks/pipeline.toml`
  - `get_node_profile(node_id) → ProfileName` — Get the base profile name assigned to a node
  - `get_node_overrides(node_id) → ToolOverrides` — Get per-node scope parameter overrides
  - `get_default_profiles() → Map<ProfileName, ToolProfile>` — Return built-in default profiles for core nodes
- **Data flowing across boundary**:
  - Inbound: parsed profile definitions (tool lists, scope parameters, overrides)
  - Outbound: configuration file path, raw file contents
- **Error cases**: Missing configuration (falls back to defaults), invalid profile reference (non-retryable with error details), schema violation (non-retryable)

### Adapter Spec Loader

Loads API specification files for adapter generation.

- **Operations**:
  - `load_spec(path) → ApiSpec` — Load and parse an API specification file (OpenAPI 3.0/3.1 JSON/YAML, or EAB JSON Schema)
  - `list_specs(directory) → Vec<SpecInfo>` — List available API specification files in the adapters directory
- **Data flowing across boundary**:
  - Inbound: parsed API specification (endpoints, parameters, schemas)
  - Outbound: file path, raw file contents
- **Error cases**: Unsupported format (non-retryable), parse error (non-retryable with error details)

---

## Infrastructure Implementations

Each abstraction has one or more concrete implementations. These are the only modules that import external crates or perform I/O.

### Anthropic LLM Provider

- Implements: LLM Provider
- Uses: Anthropic Messages API (HTTP)
- Handles: Authentication, request formatting, response parsing, rate limit headers, retry with exponential backoff
- Future: Additional providers (OpenAI, local models) implement the same trait

### GitHub Issue Tracker + PR Manager + Code Repository

- Implements: Issue Tracker, Pull Request Manager, Code Repository
- Uses: GitHub REST API and GraphQL API via `octocrab` or direct HTTP
- Handles: Authentication (GitHub App token or PAT), pagination, rate limiting (X-RateLimit headers), error mapping, native sub-issue API (REST), typed issue links (GraphQL), milestone read/write
- Note: One GitHub client implementation serves three abstractions. Internally organized by concern, but all share the authenticated HTTP client.

### GitHub Projects V2 Board

- Implements: Project Board (optional)
- Uses: GitHub Projects V2 GraphQL API
- Handles: Authentication (same GitHub App token as Issue Tracker), project item lookup, status field updates, custom field mapping
- Non-blocking: All operations are fire-and-forget; failures are logged, not fatal to the pipeline
- Note: Requires the project number to be configured in `config.toml`. If not configured, this implementation is not instantiated.

### Extension API Client (Unix Socket / HTTP)

- Implements: Domain Service Client
- Uses: Unix domain socket (default) or HTTP client for transport, JSON serialisation/deserialisation
- Handles: Connection management, message envelope formatting, response validation against Extension API JSON Schemas, reconnection with backoff
- Transport: Configurable per domain service (socket path or URL)
- Message format: JSON request/response envelopes conforming to published schemas
- Future: gRPC transport may be added as an additional option; current design does not preclude this

### Handlebars Template Engine

- Implements: Template Engine
- Uses: `handlebars-rust` crate
- Handles: Template loading from repository, variable substitution, missing variable detection

### GitHub Audit Store

- Implements: Audit Store
- Uses: GitHub Client (posts issue comments or creates artifacts)
- Handles: Formatting audit events into readable Markdown, batching events into comments

### Container Runtime Adapter

- Implements: Implementation Job Runtime Provider
- Uses: A selected container runtime or hosting provider
- Handles: Provider-specific container job requests and runtime operations
- Lifecycle: The orchestrator requests cleanup after every implementation job; the adapter performs provider-specific cleanup

### Vault Credential Adapter

- Implements: Credential Provider
- Uses: The Vault library
- Handles: Credential resolution and authentication/key lifecycle according to the provider contract; concrete Vault calls and rotation policy are outside this specification

### Tracing and OpenTelemetry Export

- Implements: Structured observability through the existing `tracing` instrumentation
- Uses: JSON console output and the CLI-configured OpenTelemetry OTLP exporter
- Handles: Export of available tracing spans and events; backend aggregation and dashboards remain external

### TOML Profile Loader

- Implements: Tool Profile Store
- Uses: `toml` crate for parsing `.cogworks/pipeline.toml` `[tool_profiles.*]` sections
- Handles: Loading profile definitions, resolving profile inheritance/overrides, validating profile contents against tool registry

### OpenAPI / EAB Adapter Generator

- Implements: Adapter Spec Loader (for loading specs), generates tool definitions registered in Tool Registry
- Uses: OpenAPI 3.x parser (JSON/YAML), JSON Schema parser for EAB format
- Handles: Parsing API specs, generating per-operation TOML tool definitions, namespace assignment, drift detection between generated adapters and source specs
- Output: Generated tool definitions stored in `.cogworks/adapters/<name>/`, index file, and configuration file
- CI integration: Drift detection runs as a CI check; regeneration is a CLI command

---

## Data Flow: End-to-End Example (Code Generation)

This traces data flow across all boundaries for a single sub-work-item code generation cycle:

```
Graph Execution Engine
  │  "Execute code-generation node for sub-work-item #5 of work item #42"
  │  Resolves tool profile for this node (Tool Profile Resolution — business logic)
  │  Template variables (e.g., {{affected_modules}}) resolved from pipeline state
  │  Effective tool list recorded in audit trail
  ▼
Code Generator (business logic)
  │  Requests context assembly
  ▼
Context Assembler (business logic)
  │  Needs: spec doc, interfaces, prior SWI outputs, constraints,
  │         relevant cross-domain interface contracts
  │  Reads from pipeline working directory + delegates to Code Repository abstraction
  ▼
Code Repository (abstraction) → GitHub API (infrastructure)
  │  Returns: file contents
  ▼
Interface Registry Manager (business logic)
  │  Provides: relevant interface contracts for cross-domain context
  ▼
Context Assembler
  │  Needs: dependency graph for relevance ranking
  │  Delegates to Domain Service Client abstraction
  ▼
Domain Service Client (abstraction) → Extension API Client (infrastructure) → Domain Service (external)
  │  Sends: dependency_graph() request over Unix socket
  │  Returns: DependencyGraph
  ▼
Context Assembler
  │  Applies priority truncation (business logic)
  │  Returns: context package (fits in window)
  ▼
Code Generator
  │  Needs: rendered prompt
  │  Delegates to Template Engine abstraction
  ▼
Template Engine (abstraction) → Handlebars (infrastructure)
  │  Returns: rendered prompt string
  ▼
Code Generator
  │  Checks budget (business logic: Budget Enforcement — atomic across parallel nodes)
  │  Sends prompt + context + schema
  │  Delegates to LLM Provider abstraction
  ▼
LLM Provider (abstraction) → Anthropic API (infrastructure)
  │  Returns: validated structured response + token count
  ▼
Code Generator
  │  Writes output to pipeline working directory
  │  Runs deterministic checks
  │  Delegates to Domain Service Client abstraction
  ▼
Domain Service Client → Extension API Client → Domain Service (external)
  │  normalise → validate → review_rules → simulate
  │  Each method: JSON request over socket → structured JSON response
  │  Returns: Diagnostics / SimulationResults
  ▼
Code Generator (business logic)
  │  If checks fail: compose error feedback, loop back to LLM call (rework edge)
  │  If checks pass: node completes, edge conditions evaluated
  ▼
Graph Execution Engine
  │  Evaluates outgoing edges, activates next eligible node(s)
  │  Writes pipeline state to GitHub (crash-recovery point)
  ▼
Review Executor (next node)
  │  First: Cross-domain constraint validation (deterministic)
  │  Delegates to Domain Service Client: extract_interfaces
  │  Compares extracted values against interface registry contracts
  │  Then: Three LLM review passes
  │  Records all events via Audit Store abstraction
  ▼
Audit Store (abstraction) → GitHub comments (infrastructure)
```

---

## Boundary Rules

1. **Business logic never performs I/O.** All external interaction flows through abstractions.
2. **Abstractions define data contracts.** The data types that cross abstraction boundaries are defined by the abstraction, not by the infrastructure.
3. **Infrastructure maps external formats.** GitHub API JSON, Anthropic API response format, compiler output text — all mapped to the abstraction's types by the infrastructure layer.
4. **No abstraction leakage.** Business logic does not handle HTTP status codes, JSON parsing, subprocess exit codes, or file system errors. These are mapped to domain-level errors by infrastructure.
5. **Dependency direction is inward.** Infrastructure depends on abstractions. Business logic depends on abstractions. Nothing depends on infrastructure directly.
6. **Testing follows boundaries.** Business logic is unit-tested with pure data. Abstractions are contract-tested with mocks/fakes. Infrastructure is integration-tested against real or simulated external systems.
