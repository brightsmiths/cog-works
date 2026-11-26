# Architecture: Boundaries and Data Flow

This document defines the clean architecture boundaries — what is business logic, what are external system abstractions, and what are infrastructure implementations. Business logic depends only on abstractions, never on concrete infrastructure.

---

## Layered View

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        Business Logic                                   │
│  (Pure domain concepts — no I/O, no external dependencies)              │
│                                                                         │
│  Pipeline state machine    │  Stage-type registry                       │
│  Stage graph (DAG+cycles)  │  Profile selection                         │
│  Graph execution engine    │  Stage-graph resolution                    │
│  Edge condition evaluation │  Context bundle assembly                   │
│  Node gate logic           │  Verdict interpretation                    │
│  Budget enforcement        │  Rework & escalation policy                │
│  Retry budget tracking     │  Failure-class detection                   │
│  Scope threshold evaluation│  Tier derivation                           │
│  Label parsing/generation  │  Cross-domain constraint validation        │
│  Interface registry valid. │  Computed constraint evaluation            │
│  Context Pack selection    │  Constitutional rules enforcement          │
│  Injection detection       │  Scope enforcement                         │
│  Protected path validation │  Alignment verification                    │
└─────────────────────┬───────────────────────────────────────────────────┘
                      │ depends on (abstractions only)
                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                   External System Abstractions                          │
│  (Traits/interfaces — define what external systems provide)             │
│                                                                         │
│  LLM Provider              │  Issue Tracker                             │
│  Code Repository (read)    │  Pull Request Manager                      │
│  Domain Service Client     │  Attestation Provider                      │
│  Template Engine           │  Credential Provider                       │
│  Harness Loader            │  Stage Runner                              │
│  Check-Run Reader          │  Mount-Plan Provider                       │
│  Risk-Map Loader           │  Interface Registry Loader                 │
│  Audit Store               │                                            │
└─────────────────────┬───────────────────────────────────────────────────┘
                      │ implemented by
                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                   Infrastructure Implementations                        │
│  (Concrete adapters — the only code that touches external systems)      │
│                                                                         │
│  Model-gateway client      │  GitHub REST/GraphQL API                   │
│  Vault credential adapter  │  Attestation signer/verifier adapter       │
│  Stage-runner client        │  GitHub check-runs adapter                │
│  Harness loader adapter    │  Risk-map loader adapter                   │
│  Extension API Client      │  Handlebars template engine                │
│  (Unix socket / HTTP)      │  GitHub issue comments (audit)             │
│  OpenTelemetry exporter    │  (Future: GitLab, other forges)            │
└─────────────────────────────────────────────────────────────────────────┘
```

Note: Domain services (Rust, KiCad, FreeCAD, etc.) are **external processes**, not part of CogWorks. They communicate through the Extension API and are not shown in the layered view above. CogWorks' Extension API Client is an infrastructure implementation that handles the transport layer.

CogWorks remains domain-agnostic across software and other engineering work. It decides which stage runs next and requests one fresh sandbox per stage attempt through the Stage Runner abstraction. Sandbox creation, hosting and the virtualisation privilege it requires belong to the stage runner service, not to CogWorks; domain-specific toolchain work remains behind the Domain Service Client.

Three capabilities deliberately sit outside every layer of this diagram, because CogWorks does not implement them at all: executing an agent, deciding whether an artifact passes, and pushing commits. They belong to the stage runner, the gate service and the stage runner's identity roles respectively. CogWorks reads the results of all three from GitHub.

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

### Stage Attempt Orchestration

CogWorks requests one fresh sandbox for each stage attempt and tracks that attempt to a terminal
outcome. A retry is a new attempt and therefore a new sandbox; no sandbox is reused across
attempts. The request carries the image, the context bundle, the mount plan, the network profile,
the resource limits and the identity role, all drawn from the resolved stage contract. The stage
runner creates and operates the sandbox; business logic depends on no container runtime,
orchestrator or cloud API. CogWorks holds no virtualisation privilege.

Completion is observed through GitHub state — the expected branch carries the attempt's commits
and its output record is present — so an attempt's outcome survives a restart of CogWorks.

### Stage-Type Registry

Resolves the set of stage types a graph may reference, from the pinned harness.

- **Input**: The resolved harness at the pinned version
- **Output**: The stage-type registry for that harness version
- **Rules**:
  - Every node in a resolved stage graph declares exactly one stage type
  - A stage type absent from the registry fails graph validation at load, before any dispatch
  - The registry is data, drawn from the harness; no stage-type name is compiled into CogWorks

### Profile Selection

Resolves the agent profile that will run a stage attempt.

- **Input**: Stage type, discipline, language or toolchain, tier, and the pinned harness
- **Output**: A profile reference and its stage contract
- **Rules**:
  - Selection is deterministic: identical inputs resolve to an identical profile at a given pin
  - No model participates in selection, and no model output is an input to it
  - The resolved profile and the harness version are recorded in the stage attestation
  - No matching profile is a halt, never a fallback to a default profile

### Stage-Graph Resolution

Resolves which stage graph governs a work item.

- **Input**: Derived tier, discipline, and the pinned harness
- **Output**: The resolved stage graph and its hash
- **Rules**:
  - The graph is drawn from the harness for the `(tier, discipline)` pair
  - No repository-local file may add a stage type, remove a stage the tier requires, or change a gate
  - The resolved graph's hash is recorded in pipeline state, so drift during a run is detectable

### Context Bundle Assembly

Builds the inputs a stage attempt is permitted to see.

- **Input**: The profile's input allowlist, pipeline state, work item content, specification paths, and the Context Packs the harness supplies
- **Output**: A context bundle and its hash
- **Rules**:
  - An input absent from the allowlist is absent from the bundle, not merely ranked lower
  - The bundle is hashed and the hash recorded before the attempt starts
  - A stage contract is never placed in the bundle of the stage it governs
  - Trust-zone barriers are enforced during assembly: an implementation stage's bundle contains no holdout content and no acceptance source

### Verdict Interpretation

Derives the pass or fail state of a commit from the gate service's check runs.

- **Input**: Check runs on a commit SHA, and the identity that authored each
- **Output**: A verdict state (passing, failing, pending, absent) with structured failure feedback
- **Rules**:
  - Only check runs authored by the gate service's identity are considered; any other author is ignored entirely
  - CogWorks computes no verdict of its own and authors no check run
  - A pending or absent verdict means wait, never assume
  - A verdict bound to a SHA other than the attempt's is treated as absent
  - Holdout feedback is limited to failing requirement IDs; holdout content never reaches a build stage

### Rework and Escalation Policy

Decides whether a failed attempt is retried, reworked, or escalated.

- **Input**: The failure's class, the attempt and rework counters, the stage contract's limits
- **Output**: One of retry, rework, escalate, or halt
- **Rules**:
  - A technically invalid output (did not compile, did not parse, schema mismatch) is a **retry** with the same context
  - A technically valid but misaligned or gate-failing output is a **rework**, carrying structured feedback
  - Exhausting the stage contract's attempt or rework limit is an **escalation**
  - An unrecoverable condition — an unverifiable pin, a missing profile, a malformed risk map — is a **halt**
  - Escalation produces a dossier issue carrying the attempt history, the failure classes seen, and the bundle hashes, so a human inherits the context rather than reconstructing it

### Failure-Class Detection

Recognises that attempts are failing the same way and stops spending budget on them.

- **Input**: The classified failure of each consecutive attempt for a stage or slice
- **Output**: Whether a repeating failure class has been reached
- **Rules**:
  - Failures are classified before being compared; the comparison is on class, not message text
  - The same class recurring for a configurable number of consecutive attempts (default: 3) triggers early escalation, before the attempt budget is exhausted
  - Distinct classes on successive attempts indicate progress and do not trigger escalation

### Tier Derivation

Derives the criticality tier of a work item from the paths it affects.

- **Input**: `risk-map.yaml` path globs, the affected paths, and any previously recorded tier
- **Output**: The derived tier
- **Rules**:
  - The tier is the maximum over the tiers of all affected paths
  - Derivation is **raise-only**: a newly derived tier may exceed a recorded one but never lower it
  - A tier is never read from a label, so a label edit cannot weaken the stringency applied to the work
  - A missing or malformed risk map is a halt, never a default to the lowest tier

### Edge Condition Evaluation

Evaluates edge conditions to determine which downstream nodes to activate. Conditions are
deterministic expressions over pipeline state and verdicts. No condition is evaluated by a model:
routing is code, so which edge is taken never depends on model output.

- **Input**: Edge condition definition (deterministic expression, LLM-evaluated condition, or composite), current pipeline state, node output
- **Output**: Boolean (edge taken or not taken), evaluation record for audit trail
- **Rules**:
  - Deterministic conditions: evaluated by the orchestrator against pipeline state via a simple expression language
  - LLM-evaluated conditions: natural-language conditions assessed by the LLM against pipeline context; must be recorded in the audit trail
  - Composite conditions: boolean combinations (AND/OR/NOT) of deterministic and LLM-evaluated conditions
  - LLM-evaluated conditions have a deterministic fallback when the LLM is unavailable or returns an ambiguous response

### Classification Rules

Deterministic post-processing of LLM classification output.

- **Input**: Model classification result, the derived tier, the scope threshold
- **Output**: Final classification
- **Rules**:
  - The tier is **not** taken from the classification. It is derived from `risk-map.yaml` over the affected paths, raise-only, so a model cannot argue its way to a lower tier
  - If estimated scope exceeds threshold → produce escalation result

### Dependency Graph

Topological sort and validation of sub-work-item dependencies.

- **Input**: List of sub-work-items with declared `depends_on` relationships
- **Output**: Ordered list (topological sort) or error (cycle detected)
- **Rules**:
  - Cycles are rejected with a specific error identifying the cycle
  - The topological ordering is deterministic (stable sort by sub-work-item ID within each tier)

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

- **Input**: Interface definition files from the target repository's specification paths, registered domain services
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
  - No generated files may match protected path patterns (the pin, the risk map, locked tests and their manifest, gate configuration)
  - Scope violations for safety-affecting work items require human clarification

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
  - Outbound: artifact paths, test/simulation filters, environment configuration, relevant interface registry entries
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

### Harness Loader

Resolves the harness at the version a target repository pins, and reads the agent-shaping
configuration out of it. This is the only source of stage graphs, stage contracts, agent
profiles, prompt templates, output schemas, Context Packs and constitutional rules.

- **Operations**:
  - `read_pin(repository) → HarnessPin` — Read `.factory/pin.yaml` from the target repository
  - `resolve(pin) → ResolvedHarness` — Resolve the harness at the pinned version, verifying the resolution
  - `stage_registry(harness) → StageTypeRegistry` — The stage types this harness version defines
  - `stage_graph(harness, tier, discipline) → PipelineGraph` — The graph governing a `(tier, discipline)` pair
  - `resolve_profile(harness, stage_type, discipline, toolchain, tier) → (ProfileRef, StageContract)` — Deterministic profile resolution
  - `context_packs(harness, triggers) → Vec<ContextPack>` — The packs matching a stage's triggers
  - `constitutional_rules(harness) → ConstitutionalRules` — The rules at this harness version
- **Data flowing across boundary**:
  - Inbound: the pin, the resolved harness content, registry, graphs, profiles, contracts, packs and rules
  - Outbound: repository identifier, pin reference, resolution keys
- **Access**: Read-only. CogWorks never writes to the harness.
- **Error cases**: Pin absent, malformed, or naming a withdrawn version; harness unreachable;
  resolution fails verification; no graph for the `(tier, discipline)` pair; no profile for the
  resolution key. Every one of these is a halt, never a fallback to a default.

### Stage Runner

Runs one stage attempt in a fresh sandbox, on CogWorks' behalf, and pushes the resulting commits
under the stage's identity role. CogWorks holds neither the virtualisation privilege nor the push
credential this requires.

- **Operations**:
  - `start_attempt(request) → AttemptHandle` — Request one stage attempt. The request carries the image, context bundle, mount plan, network profile, resource limits, identity role and branch
  - `cancel_attempt(handle)` — Request cancellation on a terminal path
- **Data flowing across boundary**:
  - Inbound: an attempt handle; thereafter the attempt's observable effects in GitHub
  - Outbound: the attempt request
- **Completion**: Observed through GitHub state — the branch carries the attempt's commits and
  its output record is present. A queue event may prompt an earlier read but is never the source
  of truth, because a signal that exists only in a queue cannot be reconstructed after a restart.
- **Error cases**: The runner is unreachable; the attempt is rejected; no completion is observed
  within the stage contract's wall-clock limit.

### Check-Run Reader

Reads gate verdicts for a commit. This is CogWorks' only source of pass or fail.

- **Operations**:
  - `check_runs_for(repository, sha) → Vec<CheckRun>` — The check runs on a commit
  - `structured_output(check_run) → Option<GateOutput>` — The gate's machine-readable output
- **Data flowing across boundary**:
  - Inbound: check runs with their authoring identity, conclusion and structured output
  - Outbound: repository identifier and commit SHA
- **Rules**: Only check runs authored by the gate service's identity are considered. CogWorks
  authors no check run and computes no verdict of its own.
- **Error cases**: No check run for the SHA; a check run still in progress; a check run authored
  by an unexpected identity. The first two mean wait; the third means ignore.

### Mount-Plan Provider

Translates a stage's writable paths into a mount plan the sandbox enforces, and is the authority
on where a given identity role may write.

- **Operations**:
  - `mount_plan(identity_role, writable_paths, repository) → MountPlan` — The plan for one attempt
- **Data flowing across boundary**:
  - Inbound: the mount plan
  - Outbound: identity role, the stage contract's writable paths, repository identifier
- **Rules**: CogWorks passes the plan to the stage runner and is never the only enforcement point
  for a write boundary. A path the plan does not make writable cannot be written by the attempt.
- **Error cases**: No plan can be produced for the role, or the requested paths exceed what the
  role may ever write. Both are a halt.

### Risk-Map Loader

Reads the human-owned criticality map from the target repository.

- **Operations**:
  - `load_risk_map(repository, git_ref) → RiskMap` — Read and parse `risk-map.yaml`
- **Data flowing across boundary**:
  - Inbound: parsed path-glob-to-tier mappings
  - Outbound: repository identifier and git reference
- **Rules**: The map is human-owned. CogWorks reads it and never writes it.
- **Error cases**: Absent or malformed map. Both are a halt — never a default to the lowest tier,
  since that would silently reduce the stringency applied to the work.

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

### Credential Provider

Resolves credentials for infrastructure clients through the Vault library. Authentication and key lifecycle are provider concerns behind this interface. CogWorks does not define Vault APIs or lifecycle policy, and key material is not available to prompts, context, logs, audit records, or generated artifacts.

### Attestation Provider

Verifies required upstream attestations against the content and trust policy presented by the pipeline, and signs CogWorks action attestations. It uses the credential provider for signing capability; private key material is never returned to business logic or exposed to the LLM. Signature format and key lifecycle remain implementation concerns behind the provider interfaces.

## Infrastructure Implementations

Each abstraction has one or more concrete implementations. These are the only modules that import external crates or perform I/O.

### Model-Gateway Client

- Implements: LLM Provider
- Uses: The deployment's model gateway endpoint (HTTP)
- Handles: Request formatting, response parsing, retry with exponential backoff
- Credentials: None. Model provider API keys are held by the gateway. This process never holds a
  model API key, and neither does any sandbox (REQ-BOUND-007)
- Rate limiting: Owned by the gateway, which holds the keys and sees all traffic across the
  factory. This client does not maintain rate-limit state
- Scope: Deterministic-stage helper calls only. An agent turn runs inside a stage attempt and
  reaches the gateway through the stage runner, not through this client
- Model neutrality: The gateway fronts whichever providers a deployment configures; no provider
  name appears in CogWorks

### GitHub Issue Tracker + PR Manager + Code Repository

- Implements: Issue Tracker, Pull Request Manager, Code Repository
- Uses: GitHub REST API and GraphQL API via `octocrab` or direct HTTP
- Handles: Authentication (GitHub App token or PAT), pagination, rate limiting (X-RateLimit headers), error mapping, native sub-issue API (REST), typed issue links (GraphQL), milestone read/write
- Note: One GitHub client implementation serves three abstractions. Internally organized by concern, but all share the authenticated HTTP client.

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

### Stage-Runner Client

- Implements: Stage Runner
- Uses: The stage runner service's API
- Handles: Serialising the stage-attempt request (image, bundle, mount plan, network profile,
  limits, identity role, branch), submitting it, and surfacing rejections
- Privilege: None. The virtualisation privilege, the sandbox hosts and the KVM capability they
  require belong to the stage runner service. This adapter is an API client and nothing more
- Lifecycle: One attempt per request, one fresh sandbox per attempt. Cancellation is requested on
  terminal paths; the service performs the teardown

### GitHub Check-Runs Adapter

- Implements: Check-Run Reader
- Uses: GitHub Checks API (REST)
- Handles: Listing check runs for a SHA, exposing each run's authoring App identity, conclusion
  and structured output
- Filtering: Runs not authored by the configured gate service identity are discarded at this
  boundary, so business logic cannot act on a check run from an unexpected author
- Write access: None. This adapter never creates or updates a check run

### Harness Loader Adapter

- Implements: Harness Loader
- Uses: The configured harness source, at the version resolved from the target repository's pin
- Handles: Reading the pin, resolving and verifying the harness at that version, parsing the
  stage-type registry, stage graphs, profiles, stage contracts, Context Packs, prompt templates,
  output schemas and constitutional rules
- Access: Read-only, and content-addressed where the source supports it, so a resolved version
  cannot change underneath a run

### Risk-Map Loader Adapter

- Implements: Risk-Map Loader
- Uses: The GitHub Contents API to read `risk-map.yaml` from the target repository
- Handles: Parsing path globs to tiers, and reporting a malformed map as a halt rather than an
  empty map

### Vault Credential Adapter

- Implements: Credential Provider
- Uses: The Vault library
- Handles: Credential resolution and authentication/key lifecycle according to the provider contract; concrete Vault calls and rotation policy are outside this specification

### Attestation Adapter

- Implements: Attestation Provider
- Uses: Credential Provider for signing capability
- Handles: Verification of required upstream attestations and signing of CogWorks action attestations
- Publishes: Signed attestation comments through the Issue Tracker abstraction
- Security: Does not expose private key material to business logic, prompts, logs, or audit records

### Tracing and OpenTelemetry Export

- Implements: Structured observability through the existing `tracing` instrumentation
- Uses: JSON console output and the CLI-configured OpenTelemetry OTLP exporter
- Handles: Export of available tracing spans and events; backend aggregation and dashboards remain external

## Data Flow: End-to-End Example (Oracle and Implementation)

This traces data flow across every boundary for one slice, from the oracle stage through
implementation, rework and the pull request. It is the L1 slice, and it shows where each of the
three delegations happens.

```
Pipeline State Machine
  │  "Slice #5 of work item #42 is ready; the stage graph says `unit-oracle` is next"
  ▼
Risk-Map Loader (abstraction) → GitHub Contents API (infrastructure)
  │  Returns: path globs → tiers
  ▼
Tier Derivation (business logic)
  │  Max over affected paths; raise-only against the recorded tier → T1
  ▼
Harness Loader (abstraction) → harness source (infrastructure)
  │  Reads `.factory/pin.yaml`, resolves the harness at that version
  │  Returns: stage-type registry, the (T1, software) stage graph, profiles, contracts, rules
  ▼
Stage-Graph Resolution + Stage-Type Registry (business logic)
  │  Validates every node's stage type against the registry; records the graph hash
  ▼
Profile Selection (business logic)
  │  (unit-oracle, software, rust, T1) → profile@version + stage contract
  │  Deterministic. No model participates. Recorded in the attestation
  ▼
Context Bundle Assembly (business logic)
  │  Inputs strictly from the contract's allowlist: the slice, the contracts, the
  │  specification, the harness Context Packs. Everything else is absent, not deprioritised
  │  The stage contract itself is NOT placed in the bundle
  │  Bundle hashed; hash recorded
  ▼
Mount-Plan Provider (abstraction) → write-scope service (infrastructure)
  │  Sends: identity role `oracle`, the contract's writable paths
  │  Returns: mount plan making `tests/locked/` writable and `src/` not
  ▼
Stage Runner (abstraction) → stage runner service (infrastructure)
  │  Sends: image, bundle, mount plan, network profile, limits, identity role, branch
  │  One fresh sandbox. The agent turn happens there — never in this process
  │  The service pushes the commits under the `oracle` identity. CogWorks holds no push token
  ▼
Pipeline State Machine
  │  Observes completion through GitHub state: branch carries commits, output record present
  │  Writes pipeline state to GitHub (crash-recovery point)
  ▼
Check-Run Reader (abstraction) → GitHub Checks API (infrastructure)
  │  Returns: the gate service's check runs on the oracle SHA
  │  Runs from any other author are discarded at the boundary
  ▼
Verdict Interpretation (business logic)
  │  Oracle gates pass: the locked tests and their signed manifest now exist
  │  The manifest is locked BEFORE implementation starts — this is the oracle barrier
  ▼
Profile Selection → (implement, software, rust, T1) → a DIFFERENT profile and contract
  ▼
Context Bundle Assembly (business logic)
  │  A different allowlist, and a different trust zone. The bundle carries the slice, the
  │  contracts and the locked tests as read-only. It carries no holdout content and no
  │  acceptance source
  ▼
Mount-Plan Provider → mount plan making `src/` and `tests/own/` writable,
  │  and the locked test paths read-only. A different identity role: `builder`
  ▼
Stage Runner → a second, separate sandbox under the `builder` identity
  │  Two stages, two identities, two sandboxes. The builder cannot edit the oracle's tests
  ▼
Check-Run Reader → Verdict Interpretation (business logic)
  │  The build gate fails: three locked tests fail
  ▼
Rework and Escalation Policy (business logic)
  │  Output was technically valid, so this is rework, not retry
  │  Feedback is the gate's structured output — failing test identities and messages,
  │  never the test source, and for holdouts never more than failing requirement IDs
  │  Rework counter incremented against the contract's limit
  ▼
Failure-Class Detection (business logic)
  │  Classifies this failure; compares with prior attempts. A new class, so continue
  ▼
Context Bundle Assembly → a new bundle carrying the rework feedback, hashed anew
  ▼
Stage Runner → attempt 2 in a THIRD fresh sandbox. No sandbox is reused across attempts
  ▼
Check-Run Reader → Verdict Interpretation
  │  The build gate passes
  ▼
Alignment Verification (business logic)
  │  Deterministic and LLM checks against the slice and contracts
  │  Bounds the attempt. Never published as a check run, never a gate verdict
  ▼
Pull Request Manager (abstraction) → GitHub API (infrastructure)
  │  CogWorks opens or updates the PR for the branch the runner pushed
  │  It does not push the commits, and it does not merge
  ▼
Attestation Provider (abstraction) → signer (infrastructure)
  │  Signs the stage attestation: profile, harness version, bundle hashes, verdicts read
  ▼
Audit Store (abstraction) → GitHub issue comments (infrastructure)
  │  The attestation and the run record land in GitHub, the only system of record
  ▼
Pipeline State Machine
  │  Evaluates outgoing edges deterministically, activates the next eligible stage
```

Three things in that trace are the whole point of the architecture, and each is a thing CogWorks
deliberately does **not** do:

- The agent ran in a sandbox CogWorks does not own, reached through the Stage Runner abstraction.
- Pass and fail came from check runs CogWorks cannot author, read through the Check-Run Reader.
- The commits were pushed by an identity CogWorks does not hold, under a mount plan a different
  service authored.

What CogWorks did was decide what runs next, decide what the agent may see, and record what
happened.

## Boundary Rules

1. **Business logic never performs I/O.** All external interaction flows through abstractions.
2. **Abstractions define data contracts.** The data types that cross abstraction boundaries are defined by the abstraction, not by the infrastructure.
3. **Infrastructure maps external formats.** GitHub API JSON, model gateway response format, compiler output text — all mapped to the abstraction's types by the infrastructure layer.
4. **No abstraction leakage.** Business logic does not handle HTTP status codes, JSON parsing, subprocess exit codes, or file system errors. These are mapped to domain-level errors by infrastructure.
5. **Dependency direction is inward.** Infrastructure depends on abstractions. Business logic depends on abstractions. Nothing depends on infrastructure directly.
6. **Testing follows boundaries.** Business logic is unit-tested with pure data. Abstractions are contract-tested with mocks/fakes. Infrastructure is integration-tested against real or simulated external systems.

The next three are absolute. They are not layering preferences but the boundaries that make
CogWorks safe to run unattended, and no layer may breach them.

7. **CogWorks never computes or publishes a verdict.** Whether an artifact passes is computed by
   the gate service from a commit SHA, in its own sandbox, under its own identity. CogWorks reads
   those check runs and authors none. Its own deterministic pre- and post-checks bound an
   attempt — it may fail, retry or rework on them — but they are never published as a check run
   and never constitute a gate verdict.
8. **CogWorks never runs an agent in its own process or container.** Every agent turn happens in
   a fresh sandbox owned by the stage runner. CogWorks holds no virtualisation privilege and no
   host privilege.
9. **CogWorks holds no push token and no model API key.** Commits are pushed by the stage runner
   under the stage's identity role. Model credentials are held by the model gateway. A credential
   that could push to a target repository, or call a model provider directly, must not be
   reachable from this process.
