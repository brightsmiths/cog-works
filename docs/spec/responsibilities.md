# Component Responsibilities

This document uses CRC-style (Class-Responsibility-Collaborator) notes to define what each component knows, what it does, and who it delegates to. These responsibilities inform the interface designer on what types and functions to create.

---

## Pipeline Executor (Graph Executor)

The central coordinator. Knows the pipeline graph structure and executes it by traversing nodes and evaluating edge conditions.

**Responsibilities:**

- Knows: Pipeline graph definition (nodes, edges, conditions), node gate configuration (auto/human per node), current pipeline state (reconstructed from GitHub), constitutional rules file path, shift work boundary per classification, maximum concurrent LLM calls limit
- Does: Loads constitutional rules at the start of every run (unconditional, before any other action), loads pipeline configuration (or falls back to default linear pipeline), determines the next action(s) for a given work item by traversing the graph, delegates to the appropriate node implementations, evaluates edge conditions to determine downstream activation, manages parallel fan-out and fan-in synchronisation, enforces CogWorks node gate rules, manages the processing lock, requests one ephemeral container per implementation job through the runtime provider and requests cleanup on every terminal path, writes pipeline state through the work-item interface at each node boundary, supports resume from failed node

**Collaborators:**

- Constitutional Rules Loader (loads constitutional rules from well-known path)
- Pipeline Configuration Manager (loads and validates pipeline graph definition)
- Edge Condition Evaluator (evaluates edge conditions to determine downstream node activation)
- GitHub Client (reads current state: labels, issues, PRs; writes pipeline state comments)
- Node Implementations (delegates execution of each node)
- Audit Recorder (logs node transitions, edge evaluations, pipeline state)
- Configuration Manager (reads gate configuration, shift work boundary)

**Roles:**

- Orchestrator: Coordinates pipeline flow through graph traversal
- State machine: Enforces valid node transitions based on edge conditions and input availability
- Parallel coordinator: Manages concurrent execution of independent nodes as async tasks
- Lock manager: Applies/removes processing lock
- Constitutional enforcer: Ensures constitutional rules are loaded before any LLM interaction
- Recovery manager: Reconstructs pipeline state from GitHub and resumes from failed node

**Key behavior:**

- Load constitutional rules before any other action (REQ-CONST-001) — failure to load is a pipeline-halting error
- Load pipeline configuration from `.cogworks/pipeline.toml` or fall back to default linear pipeline
- Given a work item, reconstruct its pipeline state from GitHub (labels, sub-issues, PR status, state comments)
- Determine which node(s) are eligible for execution (inputs available, edge conditions satisfied)
- For each eligible node: check gate configuration; if auto-proceed, delegate to node implementation; if human-gated, do nothing (exit and wait)
- When multiple nodes are eligible simultaneously, execute them concurrently (respecting max concurrent LLM calls)
- On node completion: evaluate outgoing edge conditions, activate eligible downstream nodes
- Write pipeline state to GitHub after each node boundary (crash-recovery point)
- Handle fan-in: a node requiring inputs from multiple upstream nodes stays pending until all are complete
- Track per-cycle traversal counts for rework edges and enforce termination conditions

---

## Edge Condition Evaluator

Evaluates edge conditions to determine which downstream nodes to activate after a node completes.

**Responsibilities:**

- Knows: Edge condition types (deterministic, LLM-evaluated, composite), expression language for deterministic conditions, fallback behavior for LLM failures
- Does: Evaluates deterministic conditions against pipeline state, invokes LLM for natural-language conditions (with fallback), combines results for composite conditions (AND/OR/NOT), records all evaluations in audit trail

**Collaborators:**

- LLM Gateway (for LLM-evaluated conditions)
- Audit Recorder (records all edge condition evaluations)
- Pipeline Executor (receives evaluation results to determine next nodes)

**Roles:**

- Expression evaluator: Evaluates deterministic conditions against pipeline state
- LLM condition evaluator: Assesses natural-language conditions via LLM
- Combiner: Evaluates composite conditions from boolean combinations
- Fallback handler: Applies deterministic fallback when LLM is unavailable or ambiguous
- Audit recorder: Ensures every evaluation is recorded for traceability

---

## Pipeline Configuration Manager

Loads, validates, and provides access to pipeline graph definitions.

**Responsibilities:**

- Knows: Pipeline configuration schema, configuration file location (`.cogworks/pipeline.toml`), default linear pipeline definition, graph validation rules (cycle termination, reachability, node uniqueness)
- Does: Loads pipeline configuration from the repository, validates graph structure (no cycles without termination conditions, all nodes reachable, all edges have valid source/target), selects the correct named pipeline based on classification output, falls back to default linear pipeline when no configuration exists

**Collaborators:**

- GitHub Client (reads configuration file from repository)
- Configuration Manager (reads configuration file path)

**Roles:**

- Loader: Reads and parses pipeline configuration from TOML
- Validator: Ensures graph structure is well-formed (no orphan nodes, cycles have termination, all node names unique)
- Selector: Chooses the correct named pipeline based on work item classification
- Default provider: Supplies the built-in 7-node linear pipeline when no configuration file exists

---

## Spawning Node Executor

Executes spawning nodes that create derivative work items without blocking the pipeline.

**Responsibilities:**

- Knows: Issue creation template, label configuration, linking rules
- Does: Analyzes pipeline context (via LLM prompt or deterministic script) to determine what derivative work items to create, creates GitHub Issues for each, links them to the parent work item, applies configured labels, records created issues in audit trail

**Collaborators:**

- LLM Gateway (for LLM-based analysis of what issues to create)
- GitHub Client (creates issues, applies labels, links to parent)
- Audit Recorder (logs spawned issues)

**Roles:**

- Analyzer: Determines what derivative work items to create from pipeline context
- Issue creator: Creates well-structured GitHub Issues with proper templates
- Linker: Links spawned issues to the parent work item
- Non-blocking executor: Pipeline continues regardless of spawning outcome

---

## Task Classifier (Intake Node Implementation)

Analyzes a work item to determine its type, scope, and safety impact.

**Responsibilities:**

- Knows: Classification output schema, safety-critical module registry, scope thresholds
- Does: Invokes LLM to classify the work item, cross-validates safety classification against registry, checks scope against threshold, posts classification summary

**Collaborators:**

- LLM Gateway (sends classification prompt, receives structured response)
- GitHub Client (reads issue body, posts classification comment, applies labels)
- Configuration Manager (reads safety registry path, scope thresholds)

**Roles:**

- Classifier: Determines task type and scope
- Safety validator: Enforces safety classification override rules
- Escalation trigger: Raises scope escalation when threshold exceeded

**Key behavior:**

- Read the issue body and any structured fields
- Load the list of repository modules and the safety-critical registry
- Invoke LLM with classification prompt and validate response against schema
- If LLM says "not safety-affecting" but affected modules include a registered safety-critical module → override to safety-affecting
- If estimated scope exceeds threshold → escalate (don't proceed)
- Post classification summary as issue comment, apply node and safety labels

---

## Specification Generator (Architecture Node Implementation)

Produces a technical specification document from the classified work item.

**Responsibilities:**

- Knows: Specification document structure, architectural constraint rules, Context Pack trigger rules
- Does: Loads applicable Context Packs based on classification labels/tags, assembles context (including loaded pack knowledge), invokes LLM to generate spec, validates references and dependency changes, runs cross-domain constraint validation against interface registry at architecture node, creates specification PR

**Collaborators:**

- Context Pack Loader (loads domain knowledge packs based on classification labels and component tags)
- Context Assembler (builds context package from repo files, ADRs, constraints, and loaded Context Pack knowledge)
- LLM Gateway (sends spec generation prompt, receives Markdown)
- Constraint Validator (checks proposed architecture against cross-domain interface contracts)
- GitHub Client (creates branch, commits spec, creates PR, updates labels)

**Roles:**

- Generator: Produces the specification document
- Validator: Ensures referenced modules exist and dependency changes are valid
- Constraint checker: Verifies proposed architecture doesn't violate cross-domain contracts
- Retry coordinator: Feeds validation errors back to LLM on failure
- Pack loader: Triggers deterministic Context Pack loading based on classification

---

## Interface Generator (Interface Design Node Implementation)

Produces concrete interface definitions in the target domain's artifact format.

**Responsibilities:**

- Knows: Interface definition conventions for the target domain
- Does: Reads approved spec, assembles context, invokes LLM to generate interfaces, validates via domain service, creates interface PR

**Collaborators:**

- Context Assembler (builds context from spec, existing interfaces, domain patterns)
- LLM Gateway (sends interface generation prompt, receives source files)
- Domain Service Client (validates generated interfaces via domain service's `validate` method)
- GitHub Client (creates branch, commits interfaces, creates PR, updates labels)

**Roles:**

- Generator: Produces interface source files
- Validator: Ensures interfaces are syntactically and semantically correct per domain
- Retry coordinator: Feeds domain service errors back to LLM on failure

---

## Work Planner (Planning Node Implementation)

Decomposes the approved spec and interfaces into sub-work-items.

**Responsibilities:**

- Knows: Sub-work-item schema, granularity limits, dependency graph rules
- Does: Invokes LLM to decompose work, validates dependency graph (topological sort, cycle detection), checks granularity limits, verifies interface coverage, creates sub-work-item issues

**Collaborators:**

- LLM Gateway (sends planning prompt, receives structured plan)
- GitHub Client (creates sub-work-item issues with labels and dependencies)
- Configuration Manager (reads granularity limits)

**Roles:**

- Decomposer: Breaks work into discrete sub-work-items
- Graph validator: Ensures dependency graph is a DAG (no cycles)
- Coverage checker: Verifies every interface is covered by at least one sub-work-item
- Escalation trigger: Raises granularity escalation if too many sub-work-items

---

## Code Generator (Code Generation Node Implementation)

Generates implementation artifacts for a single sub-work-item through an iterative refinement loop.

**Responsibilities:**

- Knows: Test-first iterative refinement loop (establish checks → implement → validate → iterate), retry budget, and implementation-job inputs/outputs
- Does: Assembles context (spec + interfaces + prior SWI outputs + sub-work-item description), requests an implementation job from the orchestrator, establishes tests or equivalent domain acceptance checks before implementation, invokes the LLM to implement the requested change, runs deterministic tools and domain-specific validation, feeds structured errors back to the LLM, and iterates until pass or budget exhaustion

**Collaborators:**

- Context Assembler (builds context with full SWI history)
- LLM Gateway (sends code generation prompt, receives source files)
- Domain Service Client (domain-specific normalise → validate → review_rules → simulate)
- Implementation Job Runtime Provider (provider-backed ephemeral container execution)
- Budget Tracker (checks cost budget before each LLM call)

**Roles:**

- Generator: Produces implementation artifacts and tests
- Feedback loop coordinator: Routes deterministic errors back to LLM with structured context
- Budget enforcer: Halts when retry or cost budget exceeded

**Key behavior (the refinement loop):**

1. Assemble context package
2. Invoke LLM to generate artifacts
3. Run domain service: normalise → validate → review_rules
4. If deterministic checks fail → feed structured errors to LLM, retry from step 2
5. If deterministic checks pass → run simulation/tests via domain service `simulate`
6. If simulation fails → assess if failure is self-explanatory (deterministic heuristic); if yes, feed directly to LLM; if no, invoke LLM to interpret failure, then retry from step 2
7. If simulation passes → node completes, outputs available for downstream edges
8. If retry budget exceeded → escalate

---

## Review Executor (Review Node Implementation)

Performs multi-dimensional review of generated artifacts.

**Responsibilities:**

- Knows: Four review dimensions (cross-domain constraint validation, quality, architecture compliance, security), severity levels (blocking/warning/informational), aggregation rules, required artefacts declared by loaded Context Packs
- Does: Runs deterministic cross-domain constraint validation first, verifies all required artefacts declared by loaded Context Packs are present (missing artefacts produce blocking findings), then three independent LLM review passes, aggregates results, determines overall pass/fail, feeds blocking findings back to Code Generator for remediation

**Collaborators:**

- Constraint Validator (deterministic cross-domain constraint checking)
- LLM Gateway (sends three review prompts, receives structured review results)
- Audit Recorder (logs review results)

**Roles:**

- Constraint checker: Runs deterministic cross-domain constraint validation before LLM reviews (cheapest check first)
- Artefact checker: Verifies required artefacts from Context Packs are present (blocking if missing)
- Reviewer: Executes three independent LLM review passes
- Aggregator: Combines results deterministically (any blocking = fail)
- Feedback provider: Routes blocking findings back to Code Generator with context

**Key behavior:**

- Cross-Domain Constraint Validation (deterministic, runs first): Validates generated artifacts against interface registry contracts using domain service's `extract_interfaces` method
- Code Quality Review (LLM): coding standards, idioms, error handling, naming, documentation
- Architecture Compliance Review (LLM): matches spec, respects boundaries, no unplanned dependencies, interfaces implemented correctly
- Security Review (LLM): input validation, auth boundaries, unsafe code, vulnerability patterns, dependency security

---

## Integration Manager (Integration Node Implementation)

Creates Pull Requests for completed sub-work-items.

**Responsibilities:**

- Knows: PR structure (description template, required references), commit conventions
- Does: Creates PR with generated code, posts non-blocking review findings as PR comments, links PR to sub-work-item and parent work item

**Collaborators:**

- GitHub Client (creates PR, posts comments)
- Working Copy Manager (commits files to branch)
- Audit Recorder (logs PR creation)

**Roles:**

- PR creator: Produces well-structured Pull Requests
- Comment poster: Attaches non-blocking findings as review comments
- Linker: Ensures full traceability (sub-work-item → parent → spec PR → interface PR)

---

## LLM Gateway

Thin abstraction over LLM API calls with validation, cost tracking, budget enforcement, and constitutional rules injection.

**Responsibilities:**

- Knows: Model capabilities (context window size, token limits), cost per token per model, output schemas for each node type, rate limits (per-provider request quotas), loaded constitutional rules, maximum concurrent LLM calls limit, rate-limit state from provider response headers
- Does: Injects constitutional rules as a privileged, non-overridable component of the system prompt before any other context, sends prompts to LLM API, validates responses against output schemas, tracks token usage and cost, enforces pipeline cost budget (atomic across parallel nodes), retries on API failures and schema validation failures, routes to different models per node configuration, tracks rate-limit state per provider and proactively throttles requests to avoid exhausting quotas, queues pending calls when rate-limited, halts with retriable exit code if required wait exceeds threshold

**Collaborators:**

- LLM API provider (external: Anthropic initially)
- Budget Tracker (reports token usage per call)
- Configuration Manager (reads model selection per node)

**Roles:**

- API abstraction: Hides provider-specific details behind a consistent interface
- Schema enforcer: Rejects invalid outputs and retries automatically
- Cost tracker: Accumulates token usage per call
- Budget gate: Refuses calls that would exceed pipeline budget
- Rate limiter: Tracks per-provider request budgets from response headers, proactively throttles when budget is low, applies backoff on HTTP 429, coordinates across parallel nodes

---

## Context Assembler

Deterministic service that builds context packages for LLM calls. Contains **zero** LLM logic.

**Responsibilities:**

- Knows: Pyramid summary levels (1-4), context priority order (per vocabulary.md), token budget per model, file relevance rules, scenario separation rules, loaded Context Pack content
- Does: Identifies relevant files based on affected modules, loads constraint documents (ADRs, standards, architectural rules), incorporates loaded Context Pack knowledge (domain knowledge, safe patterns, anti-patterns) as high-priority context, computes transitive dependencies via domain service dependency graph, selects appropriate summary level per module based on dependency distance, applies priority-based truncation when context exceeds window, enforces scenario holdout (never includes scenarios in code generation context), includes relevant interface registry entries for cross-domain context, reads intermediate artifacts from pipeline working directory when available

**Collaborators:**

- GitHub Client (reads file contents via API for lightweight access)
- Domain Service Client (computes dependency graph for dependency-based relevance)
- Configuration Manager (reads constraint file paths, ADR locations, scenario directory exclusions)
- Summary Cache (reads cached pyramid summaries)
- Interface Registry Manager (provides relevant cross-domain interface contracts)

**Roles:**

- File selector: Determines which files are relevant to a given node/sub-work-item
- Constraint injector: Loads and includes project rules as hard requirements
- Pack injector: Includes loaded Context Pack domain knowledge, safe patterns, and anti-patterns
- Summary selector: Chooses appropriate detail level per module (Level 1/2/3/4 based on dependency distance)
- Truncator: Applies deterministic priority-based truncation with progressive level demotion to fit context window
- Holdout enforcer: Ensures scenario specifications are never included in code generation context

**Key behavior (pyramid-based assembly):**

1. Direct targets (files being modified): Include full source (Level 4)
2. Direct dependencies: Include Level 3 (full interface detail)
3. Transitive dependencies: Include Level 2 (paragraph summary)
4. Broader context (affected area): Include Level 1 (one-line summary)
5. If budget exceeded: Progressively demote distant modules to lower levels before excluding entirely

---

## Domain Service (External Process)

An external process providing domain-specific tooling capabilities. Domain services communicate with CogWorks through the Extension API.

**Responsibilities:**

- Knows: Domain-specific tooling commands, how to parse structured output from those tools, how to manage its own working copy
- Does: Validates artifacts, normalises formatting, applies domain rules, executes simulations/tests, validates dependencies, extracts public interfaces from artifacts, computes dependency graphs, manages its own local clone of the repository

**Collaborators:**

- Local toolchain (subprocess execution within the domain service process)
- Shared clone management library (optional, provided by CogWorks for common git operations)

**Roles:**

- Validator: Invokes domain validation and returns structured diagnostics
- Normaliser: Applies canonical formatting and reports changes
- Rule reviewer: Runs domain-specific rules and returns structured findings
- Simulator: Executes tests/simulations and returns structured results
- Interface extractor: Parses artifacts and extracts public interface surface
- Graph builder: Computes dependency relationships between artifacts
- Scenario runner: Executes scenario trajectories with given environment setup

**Extension API methods:**

| Method | Purpose | Invoked during |
|---|---|---|
| `validate(artifacts)` | Check domain rules (compile, DRC, tolerance analysis, etc.) | Code generation refinement, architecture validation |
| `normalise(artifacts)` | Apply canonical formatting | Code generation (before validate) |
| `review_rules(artifacts)` | Best practices and style rules | Code generation (after validate passes) |
| `simulate(filter)` | Run tests/simulations | Test execution, scenario validation |
| `validate_deps()` | Check dependency validity | Architecture validation, planning |
| `extract_interfaces(artifacts)` | Parse and extract public interface definitions | Context assembly, cross-domain constraint validation, pyramid summaries |
| `dependency_graph()` | Compute artifact dependency relationships | Context assembly, planning (topological sort) |

**All outputs are structured data** — artifact, location, severity, category, message — in a common diagnostic format. CogWorks does not interpret results beyond the structured format.

---

## Domain Service Client

CogWorks-side client that communicates with external domain service processes via the Extension API.

**Responsibilities:**

- Knows: Extension API protocol (request/response envelopes, JSON schemas, handshake protocol), service socket/URL, API version compatibility, standardised diagnostic categories and error codes
- Does: Performs handshake to discover service capabilities and negotiate API version, sends method invocations to domain services, receives and validates structured responses against Extension API schemas, handles connection failures with backoff

**Collaborators:**

- Domain Service Registry (resolves which service handles which artifacts/domains)
- Domain Service processes (external, via Unix socket or HTTP/gRPC)

**Roles:**

- Protocol handler: Serialises requests (with envelope: request_id, api_version, method, caller, repository, params, interface_contracts) and deserialises responses per Extension API schema
- Handshake coordinator: Performs handshake to discover capabilities, artifact types, interface types, domain, and negotiate API version before method calls
- Error mapper: Maps transport errors (connection refused, timeout) and Extension API error codes (tool_not_found, tool_failed, etc.) to domain-level errors with retryability information

---

## Domain Service Registry

Manages registered domain services and routes operations to the correct one.

**Responsibilities:**

- Knows: Registered domain services (from configuration: name + connection endpoint), their dynamically discovered capabilities (from handshake: domain, artifact types, interface types, supported methods)
- Does: Performs handshake on each registered service to discover capabilities, selects the appropriate domain service for given artifacts or operations, determines primary vs. secondary domain services for a sub-work-item, caches handshake results and re-queries on error, reports unavailable services

**Collaborators:**

- Configuration Manager (reads `[[services]]` entries from config)
- Domain Service Client (routes invocations to selected service)

**Roles:**

- Selector: Routes operations to the correct domain service based on artifact types and domains
- Capability resolver: Determines if a domain service supports a requested method
- Availability reporter: Reports which services are primary (required) vs. secondary (optional for cross-domain validation)

---

## Interface Registry Manager

Loads, validates, and provides access to the cross-domain interface registry.

**Responsibilities:**

- Knows: Interface definition schema, registry location (`.cogworks/interfaces/`), validation rules
- Does: Loads all interface definitions from the registry directory, validates schema conformance, checks cross-references between interfaces, detects conflicting constraints, verifies all referenced domains have registered services (or are marked external)

**Collaborators:**

- GitHub Client (reads interface definition files from repository)
- Domain Service Registry (verifies referenced domains have registered services)
- Configuration Manager (reads registry directory path)

**Roles:**

- Loader: Reads and parses interface definition files (TOML)
- Schema validator: Ensures all definitions conform to the interface definition schema
- Cross-reference checker: Validates all references between interfaces resolve
- Conflict detector: Ensures no two interfaces define conflicting constraints for the same physical parameter
- Version checker: Detects domain service / interface version mismatches

**Key behavior:**

- Registry validation is deterministic (no LLM involved)
- Runs before any pipeline node on every pipeline invocation
- CogWorks MUST NOT create or modify interface definitions autonomously
- CogWorks MAY suggest interface additions as recommendations for human review

---

## Constraint Validator

Performs deterministic cross-domain constraint validation against the interface registry.

**Responsibilities:**

- Knows: Interface registry contents, constraint comparison rules, computed constraint formulas
- Does: Identifies relevant cross-domain interfaces for the current sub-work-item, extracts actual values from generated artifacts via domain service, compares extracted values against registry contracts, reports violations as structured findings, validates computed constraints (e.g., total bus load = sum of message rates × sizes ÷ bandwidth)

**Collaborators:**

- Interface Registry Manager (provides relevant interface contracts)
- Domain Service Client (invokes `extract_interfaces` to get actual values from artifacts)
- Audit Recorder (logs constraint validation results)

**Roles:**

- Interface matcher: Determines which cross-domain interfaces are relevant based on modified artifacts and participating domains
- Value extractor: Uses domain service to extract actual interface values from artifacts
- Constraint checker: Compares extracted values against contract parameters (bounds, enumerations, booleans)
- Computed constraint checker: Evaluates derived constraints deterministically (no formulas in registry; computed in validator)
- Violation reporter: Produces structured findings with interface ID, parameter, expected vs. actual, owning vs. violating domain

**Key behavior:**

- Runs during review gate (first pass, before LLM reviews) and during architecture node validation
- Validates against registry contracts only — does not require other domain services to be running
- Hard constraint violations (min/max bounds) are blocking; nominal value violations are warnings
- Works even with a single domain service registered (validates that domain's artifacts against declared contracts)

---

## GitHub Client

Handles all GitHub API interaction. The system's sole interface to durable state.

**Responsibilities:**

- Knows: GitHub API (REST and GraphQL), authentication, rate limit headers, sub-issue API, typed issue links API, Projects V2 API
- Does: CRUD on issues (including native sub-issues via the sub-issue API), labels, comments; creates and reads PRs; reads file contents; creates branches; commits files; reads PR review status; creates and reads typed issue links (`blocks` / `is blocked by`) for dependency tracking; optionally syncs pipeline state to GitHub Projects V2 boards; reads and preserves milestone assignments

**Collaborators:**

- GitHub REST/GraphQL API (external)

**Roles:**

- State reader: Reconstructs pipeline state from GitHub (labels, sub-issues, typed issue links, PR status, state comments)
- State writer: Updates labels, creates issues (as native sub-issues), posts comments, creates PRs, creates typed issue links for dependencies
- File reader: Reads repository file contents via Contents API
- Rate limit manager: Tracks remaining API budget, applies backoff when needed
- Project syncer: Optionally updates GitHub Projects V2 board fields at pipeline boundaries (non-blocking, fire-and-forget)
- Milestone propagator: Reads parent work item milestone and applies it to sub-work-items

---

## Implementation Job Runtime Provider (Infrastructure Interface)

Provides the runtime capability requested by the Pipeline Executor for implementation jobs. CogWorks owns orchestration and cleanup decisions; this provider owns runtime-specific container operations.

**Responsibilities:**

- Knows: The selected container runtime/provider's job and cleanup capabilities
- Does: Creates and operates the requested ephemeral container and performs provider-specific cleanup when requested by the orchestrator

**Collaborators:**

- Pipeline Executor (implementation-job lifecycle requests)
- Selected container runtime or hosting provider

**Roles:**

- Runtime adapter: Translates the provider-neutral job request to the selected runtime
- Container lifecycle adapter: Performs runtime-specific cleanup when requested

**Note**: Exactly one ephemeral container is requested per implementation job. This provider does not define domain-specific validation and does not own CogWorks pipeline gates or pipeline state.

---

## Configuration Manager

Loads and provides typed access to repository configuration.

**Responsibilities:**

- Knows: Configuration schema, default values for all settings
- Does: Loads configuration from `.cogworks/config.toml` in the target repository, validates against schema, provides typed access to settings. Loads pipeline configuration from `.cogworks/pipeline.toml` (delegating to Pipeline Configuration Manager for graph validation)

**Collaborators:**

- GitHub Client (reads config file from repository)

**Roles:**

- Loader: Reads and parses configuration file
- Validator: Ensures configuration is complete and valid
- Default provider: Supplies sensible defaults for optional settings

---

## Budget Tracker

Tracks LLM token consumption and enforces cost limits.

**Responsibilities:**

- Knows: Token costs per model, pipeline budget, current accumulated cost, parallel execution context
- Does: Records token usage per LLM call, computes cumulative cost, checks budget before each call (atomic across concurrent nodes), produces cost reports (per-node, per-sub-work-item)

**Collaborators:**

- LLM Gateway (reports token counts per call)
- Audit Recorder (includes cost data in audit trail)

**Roles:**

- Accumulator: Tracks running total of tokens consumed and cost
- Gate: Prevents LLM calls that would exceed budget
- Reporter: Produces structured cost breakdowns

---

## Scenario Validator (Scenario Validation Node Implementation)

Executes scenario validation after deterministic checks and tests pass, before the review gate.

**Responsibilities:**

- Knows: Scenario specifications (loaded from holdout location), satisfaction threshold, trajectory count per scenario, twin provisioning requirements
- Does: Loads applicable scenarios for the current sub-work-item, provisions required Digital Twins, executes each scenario multiple times (trajectories), evaluates acceptance criteria (deterministic assertions, LLM-as-judge, or statistical checks), computes satisfaction scores, feeds failures back to Code Generator for remediation

**Collaborators:**

- Configuration Manager (reads scenario directory, satisfaction threshold, judge model)
- Domain Service Client (executes scenario trajectories via domain service's `simulate` method)
- LLM Gateway (for LLM-as-judge evaluation of acceptance criteria)
- Twin Provisioner (starts/stops Digital Twin instances)
- Audit Recorder (logs scenario results)

**Roles:**

- Scenario selector: Determines which scenarios apply to the current sub-work-item based on interface coverage
- Twin orchestrator: Provisions and manages Digital Twin instances for scenario execution
- Trajectory executor: Runs each scenario multiple times with fresh state
- Judge coordinator: Evaluates acceptance criteria using deterministic assertions, LLM-as-judge, or statistical checks as appropriate
- Scorer: Computes per-scenario and overall satisfaction scores
- Feedback provider: Feeds failing scenarios and observed behaviors back to Code Generator

**Key behavior:**

- Only runs for sub-work-items with applicable scenarios (others skip this node)
- Each trajectory runs in isolation (fresh twin state)
- Satisfaction score must meet threshold (default 0.95) to proceed
- Any explicit failure criterion violation fails the sub-work-item immediately

---

## Audit Recorder

Records all pipeline activity for traceability and debugging.

**Responsibilities:**

- Knows: Audit event schema, formatting conventions, constitutional event types
- Does: Records LLM calls (model, input hash, output, tokens, latency), validation results, state transitions, cost data, scenario validation results (satisfaction scores, trajectory outcomes), constitutional layer events (INJECTION_DETECTED, SCOPE_UNDERSPECIFIED, SCOPE_AMBIGUOUS, PROTECTED_PATH_VIOLATION), Context Pack loading events (which packs loaded, trigger matches); writes audit trail to GitHub

**Collaborators:**

- GitHub Client (posts audit comments or creates artifacts)

**Roles:**

- Logger: Records every significant event in the pipeline
- Formatter: Produces human-readable summaries from structured data
- Writer: Persists audit trail to GitHub (issue comments or artifacts)
- Safety event recorder: Records all constitutional layer events with full context for post-hoc review

---

## Observability Instrumentation

Records stable structured pipeline lifecycle fields through the existing `tracing` instrumentation. The CLI configures console logging and OpenTelemetry export; external systems own metric derivation and aggregation.

**Responsibilities:**

- Knows: The stable tracing field names and values defined by `pipeline::observability`
- Does: Records node start/completion, retry, rework, and cost fields on tracing spans

**Collaborators:**

- Pipeline Executor and node implementations (create spans and invoke instrumentation hooks)
- CLI composition root (configures the tracing subscriber and optional OTLP exporter)

**Roles:**

- Instrumentation: Records canonical structured fields on spans
- Export integration: Routes spans through the configured tracing subscriber

**Key behavior:**

- No custom metric-sink trait or metric data-point computation is defined
- The tracing helpers record only fields declared on the caller's span; callers must declare the expected fields
- External monitoring systems derive metrics from the span fields actually exported

---

## Context Pack Loader

Loads domain knowledge packs based on work item classification. Contains zero LLM logic.

**Responsibilities:**

- Knows: Context Pack directory structure, trigger file schema, well-known pack path (default: `.cogworks/context-packs/`), trigger matching rules
- Does: Scans available packs, evaluates each pack's trigger definition against the work item's classification labels, component tags, and safety classification, loads matching packs (domain knowledge, safe patterns, anti-patterns, required artefacts), reports loaded packs to audit trail

**Collaborators:**

- Configuration Manager (reads pack directory path)
- GitHub Client (reads pack files from repository)
- Audit Recorder (records which packs were loaded and why)

**Roles:**

- Scanner: Discovers available Context Packs from the configured directory
- Trigger evaluator: Matches trigger rules against classification output deterministically
- Loader: Reads and parses pack contents into structured domain knowledge
- Reporter: Reports loaded packs for audit and PR description inclusion

**Key behavior:**

- Loading is deterministic: same classification always loads same packs
- Multiple packs may match simultaneously
- A matched pack is always loaded (no option to skip)
- Trigger evaluation is a pure function: classification data in, list of matched packs out

---

## Constitutional Rules Loader

Loads and validates the constitutional rules file. Contains zero LLM logic.

**Responsibilities:**

- Knows: Constitutional rules file path (default: `.cogworks/constitutional-rules.md`), file format expectations, minimum required rule set
- Does: Reads constitutional rules from the well-known path, validates that the file exists and contains the required core rules, validates that the file comes from a reviewed/merged branch (not an unreviewed branch), produces the rules payload to be injected into the LLM system prompt

**Collaborators:**

- GitHub Client (reads constitutional rules file, verifies branch/merge status)
- Configuration Manager (reads constitutional rules file path)

**Roles:**

- Loader: Reads the constitutional rules document
- Validator: Ensures required rules are present and the source is from a reviewed branch
- Formatter: Produces the system prompt component for injection by LLM Gateway

**Key behavior:**

- Loading is unconditional — runs on every pipeline invocation
- Failure to load (file missing, validation failed, unreviewed source) halts the pipeline immediately
- The loaded rules are treated as immutable for the duration of the pipeline run
- No content in the context package can override the rules

---

## Injection Detector

Analyzes external content for prompt injection patterns. May use heuristic and/or LLM-based detection.

**Responsibilities:**

- Knows: Known injection patterns (persona overrides, instruction injections, behavioral modifications), detection heuristics, LLM-based detection prompt (if used)
- Does: Scans external content (issue bodies, specs, dependency docs, API responses) for injection patterns, emits `INJECTION_DETECTED` event when patterns found, triggers pipeline halt and hold state

**Collaborators:**

- Constitutional Rules Loader (provides the boundary definition between instructions and data)
- Audit Recorder (records injection detection events with full context)
- Pipeline Executor (receives halt signal)

**Roles:**

- Pattern scanner: Checks external content against known injection patterns
- Event emitter: Produces structured INJECTION_DETECTED events with source document, offending text
- Halt trigger: Signals the pipeline to stop and put the work item into hold state

**Key behavior:**

- Invoked before external content is included in any LLM prompt
- Detection triggers immediate pipeline halt (not a warning)
- Work item enters hold state — no automatic requeue
- False positive resolution requires explicit human review with justification recorded

---

## Scope Enforcer

Validates that generated artifacts stay within the approved specification scope.

**Responsibilities:**

- Knows: Approved specification scope, interface document, authorised file set (derived from spec and interface documents), protected path patterns
- Does: Validates generated artifacts against the authorised file set, checks for unauthorized capabilities (network calls, file system access, IPC, etc.), validates generated files do not match protected path patterns, emits SCOPE_UNDERSPECIFIED or SCOPE_AMBIGUOUS events when scope issues are detected

**Collaborators:**

- Configuration Manager (reads protected path patterns)
- Audit Recorder (records scope enforcement events)
- Pipeline Executor (receives halt signal for scope violations)

**Roles:**

- File set validator: Checks generated files against the authorised file set
- Capability scanner: Detects unauthorized capabilities in generated artifacts
- Protected path checker: Ensures no generated file matches protected patterns (pre-PR validation)
- Event emitter: Produces structured scope violation events

**Key behavior:**

- Runs before PR creation to catch scope violations
- SCOPE_UNDERSPECIFIED: specification incomplete for the work item's needs — halt and request human input
- SCOPE_AMBIGUOUS: safety-affecting specification is ambiguous — halt and request human clarification
- PROTECTED_PATH_VIOLATION: generated artifacts touch protected paths — fail pre-PR validation

---

## Alignment Verifier

Verifies that a pipeline node's output matches the semantic intent of its upstream inputs — the semantic correctness layer for the pipeline.

**Responsibilities:**

- Knows: Alignment check configuration per node (threshold, rework budget, LLM check enabled/disabled), deterministic check rules per stage, LLM alignment prompt templates, finding type taxonomy (missing, extra, modified, ambiguous, scope_exceeded), severity levels (blocking, warning, informational)
- Does: Runs deterministic alignment checks (structural comparisons), invokes LLM for semantic alignment assessment (adversarial prompt, different model from generator), merges findings from both check types, computes alignment score, triggers rework on alignment failure, constructs traceability matrix entries per stage

**Collaborators:**

- LLM Gateway (invokes LLM alignment check with different model from generating node)
- Pipeline Executor (receives rework signal with misalignment findings as additional context; distinct from retry signal)
- Audit Recorder (records alignment check results, findings, score, rework cycles)
- Configuration Manager (reads per-node alignment thresholds, rework budgets, check type enablement)
- Pipeline State (reads and writes traceability matrix entries accumulated across alignment checks)

**Roles:**

- Deterministic checker: Runs structural comparisons between input and output (function signatures present, requirements addressed, scope respected)
- Semantic checker: Invokes LLM to assess whether output addresses the intent of the input, not merely its structure
- Finding merger: Combines deterministic and LLM findings into a unified result, deduplicating where both detected the same issue
- Rework trigger: When alignment fails, signals the executor to re-enter the node with specific misalignment findings (distinct from retry, which re-enters due to technical failures)
- Traceability builder: Contributes per-stage entries to the requirement-to-implementation traceability matrix

**Key behavior:**

- Runs after schema validation and domain service validation (step 4 of node execution lifecycle)
- Deterministic checks run first; their results are included in the LLM check context
- LLM alignment check uses an adversarial prompt: focused on finding misalignment, not assessing quality
- LLM alignment check SHOULD use a different model from the generating node (reduces correlated bias — CW-R02)
- Alignment failure triggers rework (re-entry with misalignment findings), not retry (re-entry without additional context)
- Rework budget is separate from retry budget; exhaustion of rework budget escalates to pipeline failure
- For safety-classified work items: stricter thresholds (0.95 vs 0.90), LLM alignment check cannot be disabled, traceability matrix requires human sign-off

---

## Tool System Components

### Tool Registry

**Responsibilities:**

- Knows: All registered tool definitions (built-in, adapter-generated, skill-derived), each tool's name, parameter schema, scope parameter declarations, and implementation
- Does: Resolves tool names to implementations for invocation, validates tool existence before execution, manages tool lifecycle (registration, deregistration), provides tool listing and search capabilities

**Collaborators:**

- Tool Profile Manager (provides subset selection criteria for node-specific tool lists)
- Adapter Generator (registers adapter-generated tools)
- Skill Executor (registers skill-derived tools as `skill.list`, `skill.run`, `skill.inspect`)
- Audit Recorder (records tool registration events)

**Roles:**

- Catalog: Maintains the authoritative list of all available tools
- Resolver: Maps tool names to callable implementations
- Validator: Rejects invocations of nonexistent tools

**Key behavior:**

- Tools from three sources: built-in (filesystem, git, domain service, shell, network), adapter-generated (from OpenAPI/EAB specs), and skill-derived (from crystallised skills)
- Tool names are globally unique; registration of a duplicate name is rejected
- Tool schemas are validated at registration time, not at invocation time

---

### Tool Profile Manager

**Responsibilities:**

- Knows: Tool profile definitions from `.cogworks/pipeline.toml`, per-node profile assignments, per-node tool overrides, template variables pending resolution
- Does: Resolves a node's effective tool list by combining the base profile with any per-node overrides, resolves template variable placeholders (e.g., `{{affected_modules}}`) from pipeline state at node activation time, provides the filtered tool list to the LLM Gateway for inclusion in LLM calls

**Collaborators:**

- Tool Registry (queries available tools to validate profile contents against registry)
- Pipeline Configuration Loader (reads profile definitions from `pipeline.toml`)
- LLM Gateway (receives the resolved tool list for a specific node invocation)
- Pipeline State (provides runtime values for template variable resolution)

**Roles:**

- Resolver: Computes effective tool list for a given node at a given point in the pipeline
- Validator: Ensures profiles reference only tools that exist in the registry; warns on unknown tool names
- Template resolver: Substitutes `{{variable}}` placeholders in scope parameters with values from pipeline state

**Key behavior:**

- Default profiles are provided for all 7 core pipeline nodes when no explicit configuration exists
- Profile resolution is deterministic: same configuration + same pipeline state = same effective tool list
- Template variables that cannot be resolved produce a clear error and prevent node activation (do not silently substitute empty values)
- Profile resolution result is recorded in the audit trail for reproducibility

---

### Tool Scope Enforcer

**Responsibilities:**

- Knows: The calling node's resolved scope parameters for a specific tool (allowed paths, branch patterns, permitted services, etc.)
- Does: Validates every tool invocation against the calling node's scope parameters before executing the tool, rejects out-of-scope operations with structured error messages, records scope violations in the audit trail

**Collaborators:**

- Tool Profile Manager (provides the resolved scope parameters for the calling node's tool)
- Tool Registry (provides the tool implementation to be invoked after scope validation passes)
- Audit Recorder (records scope violations as `SCOPE_VIOLATION` events)
- OS-Level Sandbox (third layer of defence; constrains the process environment even if tool-level enforcement fails)

**Roles:**

- Gate: Prevents tool execution when scope parameters are violated (second layer of defence in depth, after orchestrator-level filtering)
- Reporter: Provides clear, actionable error messages to the LLM explaining why a tool call was rejected
- Monitor: Tracks scope violation counts per node execution; excessive violations (threshold default: 5) trigger a warning

**Key behavior:**

- Defence in depth: the scope enforcer operates independently of orchestrator-level tool filtering. Even if a tool appears in the node's profile, scope parameters constrain how it may be used.
- Scope enforcement is synchronous — validation completes before the tool executes
- Protected path enforcement at the tool level: `fs.write`, `fs.create` refuse operations targeting protected paths regardless of profile configuration
- Scope violations are counted per node execution; exceeding the threshold (default: 5) triggers an audit warning and MAY halt the node

---

### Skill Executor

**Responsibilities:**

- Knows: Registered skill definitions (TOML manifest + script), skill lifecycle states, skill parameter schemas, skill execution history (success/failure rates)
- Does: Validates skill parameters against the manifest schema, executes skill scripts with parameter substitution, validates every tool call within a skill against the calling node's tool profile (skills do not bypass scope enforcement), records skill execution results in the audit trail

**Collaborators:**

- Tool Scope Enforcer (validates each tool call within the skill against the calling node's profile)
- Tool Registry (skill tools `skill.list`, `skill.run`, `skill.inspect` are registered here)
- Audit Recorder (records skill execution events: invocation, parameters, result, duration)
- Configuration Manager (reads skill lifecycle state, activation/deprecation status)

**Roles:**

- Executor: Runs parameterised scripts that replay proven tool invocation patterns
- Gate: Validates skill parameters before execution; rejects invalid parameter types or missing required parameters
- Monitor: Tracks per-skill success/failure rates; auto-deprecates skills falling below success threshold (configurable, default: 90% over last 20 runs)

**Key behavior:**

- Skills are deterministic: the same parameters produce the same sequence of tool invocations
- Skills CANNOT bypass scope enforcement — each tool call within a skill is validated against the calling node's tool profile
- Skill lifecycle follows: Proposed → Reviewed → Active → Deprecated → Retired
- Skills are exposed to LLMs as `skill.list`, `skill.run`, `skill.inspect` tools
- In progressive discovery mode, skills rank above raw tools in search results

---

### Adapter Generator

**Responsibilities:**

- Knows: Supported API specification formats (OpenAPI 3.0/3.1 JSON/YAML, EAB JSON Schema), adapter output format (TOML tool definitions), namespace conventions
- Does: Parses API specifications, generates per-operation tool definitions with parameter schemas derived from the spec, creates adapter index files, detects specification drift (signature changes, removed endpoints)

**Collaborators:**

- Tool Registry (registers generated adapter tools)
- CI/CD Pipeline (drift detection runs as a CI check; regeneration is a CI-triggered action)

**Roles:**

- Generator: Transforms API specs into CogWorks tool definitions
- Drift detector: Compares generated adapters against their source specs; flags mismatches

**Key behavior:**

- Adapters supersede MCP servers for most external integrations (see ADR-0005)
- Generated adapters are stored in `.cogworks/adapters/<name>/` and version-controlled as derived artifacts
- Source spec is the source of truth; adapters can be regenerated at any time
- CI check verifies no drift between source specs and generated adapters
- MCP integration is retained only as a fallback for bidirectional/streaming protocols that generated adapters cannot represent
