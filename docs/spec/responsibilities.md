# Component Responsibilities

This document uses CRC-style (Class-Responsibility-Collaborator) notes to define what each component knows, what it does, and who it delegates to. These responsibilities inform the interface designer on what types and functions to create.

---

## Pipeline Executor (Graph Executor)

The central coordinator. Knows the pipeline graph structure and executes it by traversing nodes and evaluating edge conditions.

**Responsibilities:**

- Knows: Pipeline graph definition (nodes, edges, conditions), node gate configuration (auto/human per node), current pipeline state (reconstructed from GitHub), constitutional rules file path, shift work boundary per classification, maximum concurrent LLM calls limit
- Does: Resolves the harness at the repository's pinned version, loads constitutional rules from it at the start of every run (unconditional, before any other action), resolves and validates the stage graph for the derived tier and discipline, determines the next action(s) by traversing the graph, delegates each stage to its driver, evaluates edge conditions deterministically to determine downstream activation, manages parallel fan-out and fan-in synchronisation, enforces node gate rules, manages the processing lock, requests one fresh sandbox per stage attempt through the stage runner and requests cancellation on every terminal path, writes pipeline state through the work-item interface at each stage boundary, and supports resume from a failed stage

**Collaborators:**

- Constitutional Rules Loader (loads constitutional rules from well-known path)
- Pipeline Configuration Manager (loads and validates pipeline graph definition)
- Edge Condition Evaluator (evaluates edge conditions to determine downstream node activation)
- GitHub Client (reads current state: labels, issues, PRs; writes pipeline state comments)
- Stage Drivers (delegates execution of each stage)
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
- Resolve the stage graph from the pinned harness for the derived tier and discipline, and validate it against the stage-type registry
- Given a work item, reconstruct its pipeline state from GitHub (labels, sub-issues, PR status, state comments)
- Determine which node(s) are eligible for execution (inputs available, edge conditions satisfied)
- For each eligible node: check gate configuration; if auto-proceed, delegate to the stage driver; if human-gated, do nothing (exit and wait)
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

## Intake Step

Reads a work item and establishes the facts the rest of the run depends on: its tier, its scope,
and whether the upstream approvals it requires are present.

**Responsibilities:**

- Knows: The structured fields a work item carries, the scope threshold, and which upstream attestations the configured trust policy requires
- Does: Reads the work item and its structured fields, derives the tier from the risk map, checks scope against the threshold, verifies required upstream attestations, and records what it found
- Does not: Parse prose to infer triage or affected areas, scan the work item for injection, or assign priority

**Collaborators:**

- GitHub Client (reads the work item, its labels and its signed comments; posts the intake record)
- Risk-Map Loader (supplies the path-glob-to-tier mapping)
- Tier Deriver (derives the tier, raise-only)
- Attestation Provider (verifies upstream signatures)

**Roles:**

- Tier deriver: Establishes the criticality that selects stages and thresholds
- Escalation trigger: Raises a scope escalation when the threshold is exceeded
- Trust checkpoint: Fails closed when a required upstream attestation is missing or invalid

**Key behaviour:**

- Read the work item body and its machine-readable blocks. Never parse prose for triage or affected areas
- Verify every upstream attestation the trust policy requires, and halt if one is missing, unsigned or mismatched
- Derive the tier from the risk map over the affected paths, raising but never lowering a recorded tier
- If the estimated scope exceeds the threshold, escalate rather than proceed
- Record the derived tier, the scope finding and the verified attestations on the work item

Screening untrusted text for injection belongs to the upstream intake service, which signs its
approval. CogWorks verifies that signature rather than repeating the scan.

---

## Architect Stage Driver (`architect`)

Drives the stage that produces the architectural decision, the threat-model delta and the
risk-map patch for a work item. **Level: L4.**

**Responsibilities:**

- Knows: The stage contract for `architect`, the specification paths it may write, and the deterministic constraint checks its output must satisfy
- Does: Resolves the architect profile, assembles the bundle, requests the attempt, and runs the deterministic constraint and alignment checks over the result as stage post-checks
- Does not: Decide whether its own output passes, write outside the specification paths, or lower a tier

**Collaborators:**

- Profile Resolver, Context Bundle Builder, Mount-Plan Client, Stage Runner Client
- Constraint Validator (deterministic cross-domain constraint checks)
- Alignment Verifier (semantic alignment against the work item)
- Verdict Reader (reads the gate's conclusion on the specification pull request)

**Roles:**

- Architect driver: Produces the decision record, threat-model delta and risk-map patch
- Pre/post-check runner: Bounds the attempt on deterministic constraint results, without publishing a verdict

---

## Contract Stage Driver (`contract`)

Drives the stage that turns an approved specification into compilable contracts and a program
design. **Level: L2.**

**Responsibilities:**

- Knows: The stage contract for `contract`, and the specification paths it may write
- Does: Resolves the contract profile, assembles the bundle, requests the attempt, and opens a specification pull request carrying the result
- Does not: Write implementation source, or decide whether the contracts pass

**Collaborators:**

- Profile Resolver, Context Bundle Builder, Mount-Plan Client, Stage Runner Client
- Pull Request Manager (opens the specification pull request)
- Verdict Reader (reads the gate's conclusion on that pull request)
- Interface Registry Manager (registers cross-domain contracts)

**Roles:**

- Contract driver: Produces compilable stubs and the program design
- Specification author: Produces a pull request a human approves by review

---

## Plan Stage Driver (`plan`)

Drives the stage that decomposes approved contracts into slices, and creates the sub-issues those
slices become. **Level: L2.**

**Responsibilities:**

- Knows: The stage contract for `plan`, the `plan.yaml` shape (slices, sizes, requirement mapping, dependency DAG), and milestone inheritance
- Does: Resolves the plan profile, assembles the bundle, requests the attempt, opens a specification pull request carrying `plan.yaml`, and — once a human approves it — creates one sub-issue per slice in dependency order
- Does not: Order work across work items, assign priority, or create a sub-issue from an unapproved plan

**Collaborators:**

- Profile Resolver, Context Bundle Builder, Mount-Plan Client, Stage Runner Client
- Pull Request Manager (opens the specification pull request)
- GitHub Client (creates the sub-issues, inherits the parent milestone)
- Dependency Graph (topologically orders the slices)

**Roles:**

- Plan driver: Produces the slice decomposition and its dependency DAG
- Sub-issue creator: Realises an approved plan as sub-issues, in dependency order

Creating sub-issues is deterministic: the order comes from the DAG in the approved `plan.yaml`,
not from a model.

---

## Oracle Stage Driver (`unit-oracle`)

Drives the stage that writes the locked tests and their signed manifest for a slice. It is the
first half of the system's most important boundary: the identity that writes the tests is never
the identity that writes the implementation.

**Responsibilities:**

- Knows: The stage contract for `unit-oracle`, the trust zone it runs in, the locked-test paths it may write, and the manifest that seals them
- Does: Resolves the oracle profile, assembles a bundle from that profile's allowlist, requests a mount plan for the oracle identity role, requests one stage attempt, observes completion through GitHub state, and reads the gate verdict for the resulting SHA
- Does not: See the implementation source, write to `src/`, run the tests to judge them, or decide whether its own output passes

**Collaborators:**

- Profile Resolver (resolves the oracle profile and its stage contract)
- Context Bundle Builder (assembles the allowlisted bundle)
- Mount-Plan Client (writable paths for the oracle identity role)
- Stage Runner Client (runs the attempt; pushes under the oracle identity)
- Verdict Reader (reads the gate's conclusion on the oracle SHA)
- Stage Attestation Writer (records profile, harness version and bundle hash)

**Roles:**

- Oracle driver: Produces the locked tests and the manifest that seals them
- Barrier keeper: Ensures the manifest exists and is sealed before any implementation stage starts

**Key behaviour:**

1. Resolve the oracle profile for `(unit-oracle, discipline, toolchain, tier)`
2. Assemble a bundle carrying the slice, the contracts and the specification — never implementation source
3. Obtain a mount plan making the locked-test paths writable and `src/` not
4. Request one stage attempt; the runner pushes the tests under the oracle identity
5. Read the gate verdict on the resulting SHA
6. Confirm the manifest is present and sealed. Implementation may not start until it is

---

## Implement Stage Driver (`implement`)

Drives the stage that writes the implementation for a slice, against tests it cannot change.

**Responsibilities:**

- Knows: The stage contract for `implement`, the trust zone it runs in, the paths it may write (`src/` and its own tests), and the attempt and rework limits its contract declares
- Does: Resolves the implement profile, assembles a bundle from that profile's allowlist, requests a mount plan for the builder identity role, requests one fresh sandbox per attempt, reads gate verdicts, and applies the rework and escalation policy between attempts
- Does not: Write to the locked-test paths, see holdout content or acceptance source, resolve a dispute by editing a locked test, or decide whether its own output passes

**Collaborators:**

- Profile Resolver (resolves the implement profile and its stage contract)
- Context Bundle Builder (assembles the bundle; enforces the zone barrier)
- Mount-Plan Client (writable paths for the builder identity role; locked tests read-only)
- Stage Runner Client (one fresh sandbox per attempt)
- Verdict Reader (reads the gate's conclusion and structured output)
- Rework and Escalation Policy (decides retry, rework, escalate or halt)
- Budget Tracker (checks cost budget before each attempt)

**Roles:**

- Implementation driver: Produces the implementation for one slice
- Loop coordinator: Carries the gate's structured feedback into the next attempt
- Dispute raiser: Where a locked test appears wrong, raises a dispute for a human rather than editing it

**Key behaviour (the rework loop):**

1. Confirm the locked tests and sealed manifest exist for this slice
2. Resolve the implement profile; assemble a bundle carrying the locked tests as read-only
3. Obtain a mount plan; request one fresh sandbox for this attempt
4. Read the gate verdict on the resulting SHA
5. If the verdict is pending or absent, wait — never assume
6. If it fails and the output was technically invalid, retry with the same context
7. If it fails and the output was valid, rework with the gate's structured feedback. Feedback carries failing test identities and messages, never test source; for holdouts, never more than failing requirement IDs
8. If the same failure class recurs, escalate early rather than spending the remaining budget
9. If the attempt or rework limit is reached, escalate with a dossier
10. If it passes, run the stage's alignment checks and advance

---

## Pull Request Manager

Opens and updates the pull requests that carry a run's work, and attaches its non-blocking
findings.

**Responsibilities:**

- Knows: Pull request structure (description template, required references) and the branch scheme
- Does: Opens or updates the pull request for a branch, posts non-blocking findings as comments, and links the pull request to its slice and parent work item
- Does not: Push commits, author a check run, approve, or merge

**Collaborators:**

- GitHub Client (opens and updates pull requests, posts comments)
- Stage Runner Client (the runner pushed the commits this pull request describes)
- Audit Recorder (records pull request creation)

**Roles:**

- Pull request author: Produces well-structured pull requests over branches the runner pushed
- Comment poster: Attaches findings as non-blocking review comments
- Linker: Maintains traceability from slice to parent work item to specification pull requests

The commits on the branch were pushed by the stage runner under the stage's identity role.
CogWorks describes that work; it never pushes it, and it never merges the result.

---

## LLM Gateway

Thin abstraction over LLM API calls with validation, cost tracking, budget enforcement, and constitutional rules injection.

**Responsibilities:**

- Knows: Model capabilities (context window size, token limits), cost per token per model, output schemas for each node type, rate limits (per-provider request quotas), loaded constitutional rules, maximum concurrent LLM calls limit, rate-limit state from provider response headers
- Does: Injects constitutional rules as a privileged, non-overridable component of the system prompt before any other context, sends prompts to LLM API, validates responses against output schemas, tracks token usage and cost, enforces pipeline cost budget (atomic across parallel nodes), retries on API failures and schema validation failures, routes to different models per node configuration, tracks rate-limit state per provider and proactively throttles requests to avoid exhausting quotas, queues pending calls when rate-limited, halts with retriable exit code if required wait exceeds threshold

**Collaborators:**

- Model gateway (external; holds the provider credentials)
- Budget Tracker (reports token usage per call)
- Configuration Manager (reads model selection per node)

**Roles:**

- API abstraction: Hides provider-specific details behind a consistent interface
- Schema enforcer: Rejects invalid outputs and retries automatically
- Cost tracker: Accumulates token usage per call
- Budget gate: Refuses calls that would exceed pipeline budget
- Rate limiter: Tracks per-provider request budgets from response headers, proactively throttles when budget is low, applies backoff on HTTP 429, coordinates across parallel nodes

---

## Context Bundle Builder

Deterministic service that builds the context bundle a stage attempt is permitted to see.
Contains **zero** model logic.

**Responsibilities:**

- Knows: The resolved stage contract's input allowlist, the trust zone the stage runs in, the token budget, file relevance rules, and the Context Pack content the harness supplied
- Does: Collects exactly the inputs the allowlist names, incorporates harness Context Pack knowledge, computes transitive dependencies through the domain service dependency graph for relevance ranking, applies deterministic priority-based truncation when the result exceeds the window, enforces the trust-zone barrier, hashes the bundle, and records the hash
- Does not: Include an input the allowlist does not name, include the stage contract governing the stage it is building for, or let a model influence what is included

**Collaborators:**

- Harness Loader (supplies the allowlist, the stage contract and the Context Packs)
- GitHub Client (reads file contents through the Contents API)
- Domain Service Client (computes the dependency graph for relevance)
- Interface Registry Manager (supplies relevant cross-domain contracts)

**Roles:**

- Allowlist enforcer: An input absent from the allowlist is absent from the bundle, not ranked lower
- Pack injector: Includes the harness-supplied domain knowledge, safe patterns and anti-patterns
- Zone barrier: Ensures a build-zone bundle carries no holdout content and no acceptance source
- Truncator: Applies deterministic priority-based truncation to fit the context window
- Hasher: Hashes the assembled bundle so what the agent saw is recorded and provable

**Key behaviour:**

1. Read the resolved stage contract's input allowlist
2. Collect only the inputs it names, resolving each from the repository, the harness or pipeline state
3. Rank by relevance, using dependency distance where a dependency graph is available
4. Apply the trust-zone barrier for the stage's zone, removing anything the zone must not see
5. Truncate deterministically by priority if the result exceeds the token budget
6. Hash the bundle and record the hash before the attempt starts

Truncation never removes the allowlist's mandatory inputs. If the mandatory inputs alone exceed
the budget, that is an escalation, not a silent truncation.

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

**Extension API methods:**

| Method | Purpose | Invoked during |
|---|---|---|
| `validate(artifacts)` | Check domain rules (compile, DRC, tolerance analysis, etc.) | Code generation refinement, architecture validation |
| `normalise(artifacts)` | Apply canonical formatting | Code generation (before validate) |
| `review_rules(artifacts)` | Best practices and style rules | Code generation (after validate passes) |
| `simulate(filter)` | Run tests/simulations | Domain-specific test execution |
| `validate_deps()` | Check dependency validity | Architecture validation, planning |
| `extract_interfaces(artifacts)` | Parse and extract public interface definitions | Context bundle assembly, cross-domain constraint validation |
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

- Knows: Interface definition schema, the registry's location in the repository's specification paths, validation rules
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

## Stage Runner Client (Infrastructure Interface)

The boundary through which CogWorks asks for a stage attempt to be run. Everything that requires
privilege lives on the other side of it.

**Responsibilities:**

- Knows: The stage-attempt request shape — image, context bundle, mount plan, network profile, resource limits, identity role and branch
- Does: Submits one attempt request per attempt, surfaces rejections, and requests cancellation on terminal paths
- Does not: Hold a virtualisation or host privilege, hold a push credential, run an agent in this process, reuse a sandbox across attempts, or interpret what the attempt produced

**Collaborators:**

- Implement and Oracle Stage Drivers (request attempts)
- Mount-Plan Client (supplies the plan carried in the request)
- Context Bundle Builder (supplies the bundle carried in the request)

**Roles:**

- Delegation boundary: The single place where execution leaves CogWorks
- Privilege boundary: Holds no privilege itself; the stage runner service holds all of it

---

## Harness Loader

Resolves the harness at the version the target repository pins, and is the only component that
reads agent-shaping configuration.

**Responsibilities:**

- Knows: The pin file's location and shape, how to resolve a harness version, and what the harness contains — the stage-type registry, stage graphs, agent profiles, stage contracts, prompt templates, output schemas, Context Packs and constitutional rules
- Does: Reads the pin, resolves and verifies the harness at that version, and exposes its contents to the components that need them
- Does not: Write to the harness, resolve a floating reference, or fall back to a default when resolution fails

**Collaborators:**

- GitHub Client (reads the pin from the target repository)
- Profile Resolver, Stage-Graph Resolver, Context Bundle Builder, Constitutional Rules Loader (all read through this component)

**Roles:**

- Pin resolver: Turns a repository's pin into one specific, verified harness version
- Single source: The only path by which prompt material, contracts and graphs enter CogWorks

**Key behaviour:**

- An absent, malformed or withdrawn pin is a halt, never a default
- A resolution that fails verification is a halt
- A resolved version is stable for the whole run, so a harness change cannot alter behaviour mid-run

---

## Profile Resolver

Resolves which agent profile runs a stage, deterministically.

**Responsibilities:**

- Knows: The resolution key — stage type, discipline, language or toolchain, and tier — and the harness's profile index
- Does: Resolves the key to exactly one profile and its stage contract, and records the choice
- Does not: Consult a model, accept model output as an input, or fall back to a default profile

**Collaborators:**

- Harness Loader (supplies the profile index)
- Tier Deriver (supplies the tier component of the key)
- Stage Attestation Writer (records the resolved profile and harness version)

**Roles:**

- Deterministic selector: Identical inputs at a given pin resolve to an identical profile
- Recorder: The resolution is evidence, written into the attestation

No matching profile is a halt. Running a stage under an approximately-right profile would mean
an agent operating under a contract nobody chose for it.

---

## Stage-Graph Resolver

Resolves which stage graph governs a work item, and validates it before anything executes.

**Responsibilities:**

- Knows: How a graph is selected for a `(tier, discipline)` pair, the stage-type registry, and the graph's structural rules
- Does: Resolves the graph from the harness, validates every node's stage type against the registry, validates the graph's structure, and records the graph hash
- Does not: Accept a graph from the target repository, or execute a graph that failed validation

**Collaborators:**

- Harness Loader (supplies the graph and the registry)
- Tier Deriver (supplies the tier)
- Pipeline Executor (executes the validated graph)

**Roles:**

- Resolver: Selects the graph the tier and discipline require
- Validator: Rejects an invalid graph at load, before any stage is dispatched

**Key behaviour:**

- A node declaring a stage type absent from the registry fails validation at load
- Structural rules still apply to the resolved graph: no orphan nodes, every edge target exists, every cycle has a termination condition, and at least one terminal node is reachable
- The graph hash is recorded in pipeline state, so drift during a run is detected on resume

---

## Mount-Plan Client

Obtains the mount plan that bounds where a stage attempt may write.

**Responsibilities:**

- Knows: The identity role the stage runs under and the writable paths its contract declares
- Does: Requests a mount plan for that role and those paths, and passes the plan into the attempt request
- Does not: Decide the boundary itself, widen a plan, or act as the only enforcement point

**Collaborators:**

- Profile Resolver (supplies the contract's writable paths and identity role)
- Stage Runner Client (carries the plan into the attempt request)

**Roles:**

- Boundary requester: Asks a separate service where this role may write
- Defence in depth: CogWorks is never the only thing standing between an agent and a path it should not touch

---

## Verdict Reader

Reads gate verdicts. This is CogWorks' only source of pass or fail.

**Responsibilities:**

- Knows: The gate service's identity, how to read check runs for a commit, and how to interpret their structured output
- Does: Reads the check runs on an attempt's SHA, discards any not authored by the gate service, and derives a verdict state with its structured feedback
- Does not: Compute a verdict, author a check run, treat its own checks as a verdict, or act on a check run from an unexpected author

**Collaborators:**

- Check-Run Reader (the abstraction over the Checks API)
- Rework and Escalation Policy (consumes the verdict and its feedback)

**Roles:**

- Trust anchor: The gate service's identity is the only thing CogWorks trusts for pass or fail
- Interpreter: Turns check runs into a verdict state the state machine can act on

**Key behaviour:**

- A check run authored by any other identity is ignored entirely, not weighted
- A pending or absent verdict means wait, never assume
- A verdict bound to a SHA other than the attempt's is treated as absent
- Holdout feedback is limited to failing requirement identities

---

## Rework and Escalation Policy

Decides what happens after a failed attempt.

**Responsibilities:**

- Knows: The failure classes, the attempt and rework counters, and the limits the stage contract declares
- Does: Chooses retry, rework, escalate or halt, and composes the structured feedback a rework carries
- Does not: Exceed a contract's limits, place test source or holdout content in feedback, or retry an unrecoverable condition

**Collaborators:**

- Verdict Reader (supplies the verdict and its structured output)
- Failure-Class Detector (reports a repeating failure class)
- Context Bundle Builder (receives the feedback for the next attempt's bundle)
- GitHub Client (creates the escalation dossier issue)

**Roles:**

- Loop controller: Bounds how long a stage may keep trying
- Dossier author: Hands a human the attempt history, failure classes and bundle hashes rather than a bare failure

**Key behaviour:**

1. Technically invalid output — did not compile, did not parse, failed schema validation — is a **retry** with the same context
2. Technically valid output that fails a gate or an alignment check is a **rework** carrying structured feedback
3. A repeating failure class is an **early escalation**, before the budget is spent
4. Reaching the contract's attempt or rework limit is an **escalation** with a dossier
5. An unverifiable pin, a missing profile, a malformed risk map or a missing stage graph is a **halt**, not a retry

---

## Tier Deriver

Derives the criticality tier that selects stages, thresholds and human gates.

**Responsibilities:**

- Knows: The risk map's path-glob-to-tier mapping, the affected paths, and any previously recorded tier
- Does: Computes the maximum tier over the affected paths and compares it with the recorded tier
- Does not: Read a tier from a label, lower a recorded tier, or default when the map is absent

**Collaborators:**

- Risk-Map Loader (supplies the mapping)
- Profile Resolver and Stage-Graph Resolver (consume the derived tier)

**Roles:**

- Deriver: Establishes criticality from the paths the work touches
- Ratchet: Raise-only, so stringency can never be reduced by re-deriving

An absent or malformed risk map is a halt. Defaulting to the lowest tier would silently reduce
the stringency applied to work nobody has assessed.

---

## Stage Attestation Writer

Records what ran, under what, and over what inputs.

**Responsibilities:**

- Knows: The attestation shape — stage type, resolved profile, harness version, bundle hashes, identity role, verdicts read, and attempt outcome
- Does: Assembles the attestation, has it signed, and transports it as a hidden comment on the work item
- Does not: Hold private key material, or treat an attestation as workflow state

**Collaborators:**

- Attestation Provider (signs the attestation)
- Credential Provider (supplies the signing capability; key material never reaches business logic)
- GitHub Client (transports the signed attestation as a hidden comment)

**Roles:**

- Recorder: Makes each stage's inputs and choices provable after the fact
- Chain extender: Carries the chain of custody forward for the next service to verify

Attestations are evidence, not state. A run's position is always reconstructable from GitHub
without reading a single attestation.

---

## Configuration Manager

Loads and provides typed access to repository configuration.

**Responsibilities:**

- Knows: Configuration schema, default values for all settings
- Does: Loads CogWorks' own process configuration — the endpoints it talks to, its label mappings and its concurrency limits — validates it against schema, and provides typed access to it. Stage graphs, stage contracts and all agent-shaping configuration come from the harness, not from here

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

## Audit Recorder

Records all pipeline activity for traceability and debugging.

**Responsibilities:**

- Knows: Audit event schema, formatting conventions, constitutional event types
- Does: Records model calls (model, input hash, output, tokens, latency), resolved profiles and harness versions, context bundle hashes, verdicts read and their authoring identity, validation results, state transitions, cost data, constitutional layer events (SCOPE_UNDERSPECIFIED, SCOPE_AMBIGUOUS, PROTECTED_PATH_VIOLATION), and Context Pack loading events (which packs loaded, trigger matches); writes the audit trail to GitHub

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

- Pipeline Executor and stage drivers (create spans and invoke instrumentation hooks)
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

- Knows: Context Pack structure within the harness discipline pack, trigger file schema, trigger matching rules
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

- Knows: Where the constitutional rules live within the harness, file format expectations, minimum required rule set
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
