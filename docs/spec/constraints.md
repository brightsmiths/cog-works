# Implementation Constraints

These constraints MUST be enforced during implementation. They inform the interface designer on type-level decisions and give the coder hard rules.

---

## Type System

- **Branded/newtype identifiers**: All domain identifiers must use distinct types that cannot be accidentally interchanged. `WorkItemId`, `SubWorkItemId`, `NodeId`, `BranchName`, `PipelineName`, `EdgeId`, etc. must not be bare strings or integers.
- **Result-based error handling**: All domain operations must return `Result<T, E>`. Business errors are values in the `Err` variant, not panics or exceptions.
- **Exhaustive error types**: Each component's error type must be an enum covering all failure modes. No catch-all `Other(String)` variants in domain error types. Infrastructure error types may have a catch-all for truly unexpected failures.
- **No `unwrap()` in production code**: All `Option` and `Result` values must be explicitly handled. `unwrap()`, `expect()` are allowed only in tests.
- **Structured data over strings**: All data that crosses a component boundary must be a typed struct or enum, not a serialized string. Parsing happens at the boundary (infrastructure layer), not in business logic.

---

## Module Boundaries

- **Business logic is pure**: Business logic modules must not import I/O crates (`tokio`, `reqwest`, `octocrab`, `std::fs`, `std::process`). They operate on data passed in as arguments.
- **Abstractions are traits**: External system interfaces are defined as Rust traits. Business logic depends on these traits via generics or trait objects.
- **Infrastructure implements traits**: Each infrastructure module implements one or more abstraction traits. Only infrastructure modules import external crates.
- **No circular dependencies**: Module dependency graph must be a DAG. Business logic → abstractions ← infrastructure. Abstractions never depend on business logic or infrastructure.
- **Node implementations are orchestration, not business logic**: Node implementations coordinate calls between business logic, abstractions, and other components. They contain sequencing logic, not domain rules.
- **Scenario holdout enforcement**: The Context Assembler MUST enforce that scenario specifications are never included in code generation context packages. This is a hard constraint, not advisory. The configuration must explicitly list scenario directories to exclude.
- **Domain services are external processes**: CogWorks MUST NOT contain domain-specific code. All domain operations (validate, normalise, review_rules, simulate, validate_deps, extract_interfaces, dependency_graph) are delegated to external domain services via the Extension API.
- **Engineering workflows are domain-neutral**: CogWorks MUST support software and other engineering work through generic test-first, implementation, and validation/iteration orchestration. Test formats, toolchains, and domain validation remain behind the domain-service interface.
- **No built-in privileged path**: The Rust domain service MUST use the Extension API like any other domain service. If the API is insufficient for the Rust domain service, the API must be improved, not bypassed.
- **Interface registry is human-authored**: CogWorks MUST NOT create or modify interface definitions autonomously. It MAY suggest additions as recommendations for human review.
- **Domain services do not communicate directly**: CogWorks mediates all cross-domain interactions. Domain services do not invoke or depend on each other.

---

## Workspace Architecture

These rules govern the physical crate structure and inter-crate dependency
direction. See `docs/spec/interfaces/README.md` for the full dependency graph.

- **Crate names are domain-meaningful**: Crates are named after their domain
  concern (`pipeline`, `nodes`, `github`, `llm`, `extension-api`, `listener`,
  `cli`). Architectural names like `core`, `adapters`, `ports` are forbidden.
- **`pipeline` has no I/O dependencies**: The `pipeline` crate must not declare
  `tokio`, `reqwest`, `std::fs`, `std::process`, or any other I/O crate as a
  dependency. Trait definitions that involve async are the only exception — they
  are defined in `pipeline` but implemented in infrastructure crates.
- **Infrastructure crates depend on `pipeline`, not on each other**: The `github`,
  `llm`, `extension-api`, and `listener` crates each depend on `pipeline`.
  They must not depend on each other. Cross-cutting infrastructure concerns
  (e.g. shared HTTP client configuration) must be resolved by composition in
  `cli`, not by introducing inter-infrastructure dependencies.
- **`nodes` depends on `pipeline` only** (among workspace crates): The `nodes`
  crate depends on `pipeline` for types and trait objects. At runtime, concrete
  infrastructure implementations are injected via trait objects
  (`Arc<dyn IssueTracker>`, etc.) — `nodes` never imports `github`, `llm`,
  `extension-api`, or `listener` directly.
- **`cli` is the sole composition root**: Only `cli` imports all other workspace
  crates and constructs concrete infrastructure instances. This is the only place
  where `GithubClient`, `AnthropicProvider`, `ExtensionApiClient`,
  `GitHubWebhookEventSource`, and `QueueEventSource` are instantiated.
- **Readiness mapping is an infrastructure concern**: Trigger sources report
  work-item readiness through the `pipeline::EventSource` abstraction. The
  GitHub/event adapter owns mapping provider-specific labels, project states, or
  webhook payloads; business logic MUST NOT require a particular external signal.
  Manual CLI invocation remains a supported trigger mode.
- **Workspace dependency versions are pinned centrally**: All external dependency
  versions are declared in the `[workspace.dependencies]` table in the root
  `Cargo.toml`. Individual crate `Cargo.toml` files use `{ workspace = true }`.
  Introducing a version that is not in the workspace table is not permitted without
  an explicit justification in the PR description.
- **`docs/spec/shared-registry.md` is kept current**: Every PR that adds a new
  public type, trait, or pattern must add it to the shared registry before merge.

---

## Pipeline Graph

- **Every cycle must have a termination condition**: The pipeline configuration validator must reject any graph containing a cycle without an explicit `max_traversals` on at least one edge in the cycle. There must be no path to infinite execution.
- **Cost budget is shared across parallel nodes**: When nodes execute concurrently, budget acquisition must be atomic. Two nodes must not both be approved if their combined estimated cost exceeds the remaining budget.
- **Pipeline state is recoverable from GitHub**: The pipeline working directory is a performance optimisation. All pipeline state must be reconstructable from GitHub artifacts (issue comments, labels, PRs). Loss of the working directory must not require restarting the pipeline from scratch.
- **Implementation containers are not durable state**: Job-local files are transient. Durable pipeline state and deliverables must be persisted through the existing work-item and repository interfaces before container cleanup.
- **Node inputs must be verified before execution**: The graph execution engine must verify that all declared inputs for a node are available before starting the node. Missing inputs must produce a clear error identifying the node and the missing input.
- **Edge condition evaluation is audited**: Every edge condition evaluation (deterministic, LLM-evaluated, or composite) must be recorded in the audit trail with the condition definition, the evaluation input, and the result.
- **Pipeline configuration is validated at load time**: The pipeline configuration must be fully validated when loaded (before any node executes). Invalid configurations must produce clear errors and prevent the pipeline from starting.

---

## Error Handling

- **Expected errors are `Result` values**: Validation failures, budget exceeded, scope threshold exceeded, review blocking findings — these are all `Err` variants, not panics.
- **Retryable vs. non-retryable**: Error types must distinguish between retryable failures (API timeout, rate limit) and non-retryable failures (budget exceeded, invalid configuration). The LLM Gateway and GitHub Client must expose this distinction.
- **Error context**: All error types must include sufficient context for debugging — what operation was attempted, what input was provided, what went wrong. Use structured error types, not just strings.
- **Error propagation**: Errors flow upward through the call stack via `Result`. Each layer may enrich the error with additional context (what it was trying to do when the error occurred).
- **Escalation is an error kind**: When the system cannot resolve an issue within its budget, it produces an escalation result — a structured report of what was tried and what failed. This is a value, not an exception.

---

## Testing

- **Business logic: 100% unit test coverage**: Every function in business logic modules must have tests covering happy paths, error cases, and edge conditions. No mocks needed — pure input/output.
- **Abstraction traits: contract tests**: Each trait must have a test suite that any implementation must pass. Written as generic test functions parameterized by the trait.
- **Infrastructure: integration tests**: Each infrastructure implementation must be tested against the real external system (or a faithful simulation). Use testcontainers, mock HTTP servers, or temporary git repos as appropriate.
- **Node implementations: integration tests with mocks**: Test the orchestration logic by providing mock implementations of abstractions. Verify the correct sequence of calls and state transitions.
- **Graph execution tests**: The graph execution engine must have tests covering: linear graph traversal, fan-out (parallel activation), fan-in (waiting for all upstreams), rework edges with traversal counting, cycle termination enforcement, edge condition evaluation (deterministic, LLM-evaluated with fallback, composite), partial failure handling (`abort_siblings_on_failure` true and false), pipeline state serialisation and resume from failure, configuration validation (unterminated cycles, orphan nodes).
- **Pipeline configuration validation tests**: The pipeline configuration loader must have tests for: valid TOML parsing, missing file (falls back to default), malformed TOML, cycles without termination conditions, unreachable nodes, duplicate node names, invalid edge source/target references.
- **Test naming**: `test_<function>_<scenario>_<expected>` (per `.tech-decisions.yml`).
- **No test pollution**: Tests must not depend on external state. Each test sets up its own context and tears it down.
- **Scenarios test generated code, not CogWorks**: Scenario validation tests the code generated by CogWorks. CogWorks' own correctness is tested through unit/integration/E2E tests as defined in testing.md.
- **Pyramid summary accuracy**: Summaries must be regenerated when source changes. Staleness check must be deterministic (file hash comparison). Stale summaries must never be used for context assembly decisions.
- **Extension API conformance tests**: A published conformance test suite must exist that any domain service can run to verify it correctly implements the Extension API. The Rust domain service must pass this suite.
- **Cross-domain constraint validation tests**: The constraint validator must have tests for all contract parameter types (numeric with tolerance, exact, enumerated, boolean, reference, computed).
- **Property-based testing for constitutional layer**: Injection detection and scope enforcement logic must be tested with property-based tests covering a broad range of injection patterns (persona overrides, instruction injections, behavioral modifications). Relying solely on example-based tests is insufficient given the adversarial nature of the problem.
- **Context Pack selection tests**: Pack trigger evaluation must be tested with fixtures covering: classification that matches zero packs, one pack, multiple packs, conflicting guidance between packs, and packs with safety-classification-specific triggers.

---

## Performance

- **Pipeline step function < 60s wall time**: A single CLI invocation (one step function execution) should complete within 60 seconds, excluding LLM API latency and domain service operation latency. If a step takes longer, it's likely doing too much.
- **LLM latency is external**: The system must not add significant overhead on top of LLM response time. Context assembly, schema validation, and result processing should each take < 1 second.
- **GitHub API efficiency**: Minimize API calls per invocation. Batch reads where possible (e.g., read all labels in one call, not one per label). Cache within a single invocation (not across invocations — stateless design).
- **Domain service operations are external**: Domain service latency (validation, simulation, etc.) is external to CogWorks. CogWorks must support progress polling for long-running operations.
- **Subprocess timeouts (domain service side)**: Domain services are responsible for their own subprocess timeouts. CogWorks enforces an overall operation timeout per domain service method call (configurable, default: 10 minutes for simulate, 5 minutes for validate/normalise/review_rules).

---

## Security

- **Credential lifecycle is provider-owned**: The Vault library behind the credential-provider interface resolves credentials and implements their authentication/key lifecycle. CogWorks MUST NOT prescribe Vault mechanics or expose key material to LLM prompts/context, audit records, logs, or generated artifacts.
- **Minimum-privilege GitHub token**: The token must have only the permissions needed: issues (read/write), pull requests (read/write), contents (read/write), labels (read/write). No admin access.
- **No secrets in context packages**: LLM API keys, GitHub tokens, and any other credentials must never appear in context packages, audit trails, or generated code.
- **No secrets in generated code**: Code generation must use placeholder values for secrets, with documentation on what needs to be configured.
- **Domain service isolation**: Domain services run as separate processes. CogWorks does not pass secrets to domain services. Domain services receive only the Extension API context (work item info, node, repository path, relevant interface contracts).
- **Prompt injection awareness**: Issue bodies and repository content are untrusted input. The constitutional layer is the primary defense; schema validation of LLM outputs is the secondary defense. Neither is sufficient alone.
- **Constitutional rules are mandatory**: Constitutional rules MUST be loaded before any LLM call on every pipeline run. No configuration option may disable this. Failure to load constitutional rules halts the pipeline. This is not a security option — it is a hard constraint.
- **Protected paths**: CogWorks MUST NOT create or modify files matching protected path patterns through the normal pipeline. Protected paths include at minimum: the constitutional rules file, prompt template files, scenario specification files, and Extension API schemas. The protected path list is version-controlled and configurable.
- **Protected-path matching is case-sensitive**: Glob matching for protected paths and approved-scope patterns (`security::is_protected`, `security::validate_scope`, `alignment::run_deterministic_alignment`) is case-sensitive. On a case-insensitive filesystem (default Windows, default macOS), a declared protected path such as `"SECURITY.md"` does not match an artifact key that differs only in case (e.g. `"Security.md"`), even though both resolve to the same file at write time. Deployments on case-insensitive filesystems MUST either enumerate case variants explicitly in the protected-path list, or run CogWorks only on case-sensitive filesystems (the recommended default for CI/production runners). This is a pre-existing constraint of the underlying glob engine, not a bug in any single check.
- **Rate limit respect**: The system must respect GitHub API rate limits (5000/hr for authenticated requests). Track remaining budget from response headers, back off proactively.
- **LLM API rate limit respect**: The LLM Gateway must respect provider-imposed rate limits (requests per time window). Rate-limit state must be tracked from provider response headers, shared across parallel nodes, and enforced proactively (throttle before hitting the limit, not just react to 429s). If the required wait time exceeds the configured halt threshold (default: 30 minutes), the step must halt with a retriable exit code rather than blocking indefinitely.
- **Extension API authentication**: For Unix domain sockets, file system permissions provide access control. For HTTP/gRPC transport, authentication mechanism is to be determined but the design must not preclude adding authentication later (e.g., bearer tokens, mutual TLS). (see ADR-0006)

---

## Configuration

- **Configuration file**: `.cogworks/config.toml` in the target repository. Loaded once per CLI invocation.
- **Mandatory fields**: At minimum: a reference to a domain service registration file and at least one LLM model selection.
- **Domain service registration file**: Domain services are declared in `.cogworks/services.toml` (overridable via `COGWORKS_DOMAIN_SERVICES_CONFIG`), not in `config.toml`. Each service entry (under `[[services]]`) specifies name, transport, and connection endpoint (socket path or URL). Service capabilities, artifact types, interface types, and domain are discovered dynamically via the handshake (health check) — they are NOT statically configured. This keeps the config minimal and ensures the config can't drift from what the service actually provides.
- **Interface registry configuration**: `[interfaces]` section specifying registry directory and startup validation flag.
- **Constraint validation configuration**: `[constraint_validation]` section specifying enabled flag and missing-service behavior.
- **Sensible defaults**: Every configurable value must have a sensible default. A minimal configuration file should be sufficient to run the pipeline.
- **Validation at load time**: Configuration must be fully validated when loaded. Invalid configuration produces a clear error message and halts the pipeline before any work begins.
- **No environment-variable-driven behavior in business logic**: Environment variables are read in the infrastructure/configuration layer only. Business logic receives typed configuration values, never raw strings from the environment.
- **Label configuration**: Workflow-semantic labels and signals (readiness/trigger, awaiting-review, safety-critical, hold, cancel, restart, sub-work-item) must be configurable through the `[labels]` section in `.cogworks/config.toml` or the corresponding provider adapter. Pipeline-internal labels (`cogworks:processing`, `cogworks:node:<name>`, `cogworks:node:failed`, `cogworks:status:<status>`) are never configurable. Duplicate mappings must be rejected at load time, including collisions with pipeline-internal labels. Business logic must not depend on a particular workflow label or project status.
- **GitHub Project integration is optional and non-blocking**: When a `[github_project]` section is present, pipeline state is synced to a GitHub Projects V2 board at node boundaries. Project board update failures are logged as warnings and must never block, slow, or fail the pipeline. CogWorks must operate identically with or without project configuration.
- **Native sub-issues for work item hierarchy**: Sub-work-items must be created as native GitHub sub-issues (using the sub-issue API), not as standalone issues linked only by labels. Dependencies between sub-work-items must use native GitHub typed issue links (`blocks` / `is blocked by`).
- **Milestone inheritance**: Sub-work-items must inherit the parent work item's GitHub Milestone when one is set. CogWorks must not create, modify, or delete milestones.
- **Semantic stalling threshold**: The consecutive-same-category threshold (`semantic_stall_threshold`, default: 3) must be validated against each node's retry budget (`max_retries`) at configuration load time. If a node's stalling threshold exceeds its retry budget, a warning must be emitted at load time and stalling detection is treated as disabled for that node. This is a warning, not an error — the pipeline still starts.

---

## Extension API

- **API version compatibility**: CogWorks declares supported API versions. Domain services declare their implemented version. Incompatible versions are rejected during health check handshake.
- **JSON Schema enforcement**: All Extension API messages must conform to published JSON Schemas in `schemas/extension-api/`. Schema changes follow semantic versioning (additive = minor, breaking = major).
- **Transport flexibility**: Default transport is Unix domain sockets. HTTP/gRPC support is configurable per domain service. The protocol layer must be transport-agnostic so new transports can be added without changing business logic.
- **Progress polling**: The v1 Extension API baseline is synchronous request-response with configurable timeouts. The protocol design must accommodate adding progress polling (via operation IDs) or streaming transport in a future API version without breaking existing domain services. Domain services are NOT required to implement progress polling in v1.
- **Domain services own their lifecycle**: CogWorks does not start, stop, or manage independently deployed domain service processes. Services are started independently; CogWorks checks health before invocation. This is separate from implementation-job containers, whose lifecycle is requested by CogWorks' orchestrator through the runtime-provider interface.
- **Graceful degradation**: Primary domain service unavailable = halt. Secondary domain service unavailable = continue with warning. This distinction is based on whether the service covers the domain of the artifacts being generated.
- **Standardised diagnostic categories**: Domain service diagnostics must use the standardised diagnostic category set (`syntax_error`, `type_error`, `constraint_violation`, `interface_mismatch`, `dependency_error`, `style_violation`, `safety_concern`, `performance_concern`, `test_failure`, `completeness`). Domain services may use additional domain-specific categories; consumers treat unknown categories as informational.
- **Standardised error codes**: Service-level errors (cannot process request) must use the standardised error code set (`tool_not_found`, `tool_failed`, `invalid_request`, `unsupported_method`, `api_version_mismatch`, `timeout`, `artifact_not_found`, `internal_error`). Each code has a defined recoverability (retryable or non-retryable). Consumers use the `recoverable` field to decide retry strategy.

---

## Observability

- **Structured logging**: All log output must be structured (JSON). Each log entry must include: `pipeline_id` (work item number), `node`, `sub_work_item` (if applicable), `action`, `result`, `duration_ms`.
- **Audit trail completeness**: Every LLM call, validation result, state transition, and cost event must appear in the audit trail. The trail must be sufficient to reconstruct the full decision history.
- **Cost visibility**: Token usage and cost must be tracked per-call, per-node, and per-pipeline. The final cost report must be posted as a comment on the work item.
- **Constitutional event visibility**: Every INJECTION_DETECTED, SCOPE_UNDERSPECIFIED, SCOPE_AMBIGUOUS, and PROTECTED_PATH_VIOLATION event must appear in the audit trail with full context (work item ID, source document, offending content or missing capability). These events are never silently suppressed.
- **Context Pack audit**: The names, version (git ref), and trigger match reasons for all loaded Context Packs must be recorded in the audit trail and included in the PR description.
- **Tracing export is non-blocking**: External observability exporter availability MUST NOT block, slow, or fail pipeline execution. CogWorks records the tracing fields defined by the observability interface; it does not promise a separate metric data-point model or fallback emission.
- **No metrics storage**: CogWorks MUST NOT implement its own metrics storage, aggregation, dashboarding, or alerting. These are delegated to external tools. External systems may derive measurements from tracing fields that are actually exported.
- **Tracing fields are stable**: Stable tracing event kinds and typed retry/rework categories MUST be used for fields defined by the observability interface. CogWorks does not compute a separate metric model.
- **Root cause categories are structured**: Retry and failure root cause categories MUST be structured enums (compilation error, test failure, review finding, constraint violation, timeout), not free-form strings. This enables reliable external aggregation.

---

## Context Pack Constraints

- **Pack structure is enforced**: Context Pack directories must contain a valid `trigger.toml`. Packs without a valid trigger file are skipped with a warning, not ignored silently.
- **Pack content is trusted as configuration**: Loaded Context Pack content is treated as configuration-level input (higher trust than user-supplied content) but lower trust than CogWorks' own constitutional rules. Pack content cannot override constitutional rules.
- **Pack loading is deterministic**: The same classification input must always produce the same set of loaded packs. Pack selection must be a pure function with no randomness or LLM involvement.
- **Pack directory is version-controlled**: Context Pack files in `.cogworks/context-packs/` are subject to normal code review and branch protection. Changes are tracked in git.
- **Pack content never includes executable code**: Pack files are Markdown and TOML documents. They must not contain scripts, compiled artifacts, or anything that could be executed by CogWorks.
- **Required artefact declarations are validated at load time**: The `required-artefacts.toml` file must conform to the required artefacts schema. Malformed declarations are reported as configuration errors, not silently ignored.
- **Stricter thresholds are pack-declared**: Context Packs may declare stricter satisfaction thresholds for their domain. These override the global threshold for affected scenarios. They cannot relax the global threshold.

---

## Constitutional Layer Constraints

- **Constitutional rules loading is unconditional**: There MUST NOT be a configuration option to skip constitutional rules loading. Any code path that calls the LLM without loading constitutional rules first is a defect.
- **Constitutional rules position is fixed**: The constitutional rules are always the first component of the system prompt. No business logic may prepend content before them or demote them in the prompt order.
- **Constitutional rules are not context items**: Constitutional rules are NOT subject to context truncation. They are not in the context priority queue. They are always present in full.
- **Constitutional rules change requires human approval**: Changes to `.cogworks/constitutional-rules.md` must be made via a reviewed PR. Unreviewed changes must be detected and rejected at load time. This constraint is enforced by checking that the file's current content matches a reviewed commit.
- **Injection detection is pre-prompt**: Injection detection runs before any external content is assembled into the LLM prompt. Detecting injection after prompt assembly is insufficient.
- **Hold state is enforced by label**: Work items in hold state have `cogworks:hold` label. All pipeline invocations must check for this label and exit without action. The label can only be removed by a human.
- **Protected paths are configurable but non-empty**: At minimum, the constitutional rules file, prompt template directory, scenario directory, and Extension API schemas are always in the protected path set, regardless of configuration. These defaults cannot be removed via configuration.

---

## Alignment Verification Constraints

- **Alignment checker model separation**: The LLM alignment check SHOULD use a different model from the generating node. Using the same model risks correlated blind spots (a model is unlikely to detect its own semantic errors). When only one model is available, this constraint is relaxed with a warning logged, not silently ignored.
- **Rework and retry budgets are independent**: Rework (alignment failure) and retry (technical failure) MUST have separate counters and separate configurable budgets. Exhausting one does not affect the other. Both appear independently in the audit trail and metrics.
- **Rework budget is enforced**: When the rework budget is exhausted for a node, the pipeline MUST fail with a structured error including the alignment findings from the final rework attempt. The pipeline MUST NOT silently pass after rework exhaustion.
- **Finding types are structured enums**: Alignment finding types (missing, extra, modified, ambiguous, scope_exceeded) and severity levels (blocking, warning, informational) MUST be structured enums, not free-form strings. This enables reliable external aggregation and deterministic pass/fail evaluation.
- **Blocking findings always fail**: If any alignment finding has severity `blocking`, the alignment check MUST fail regardless of the alignment score. The score threshold is necessary but not sufficient.
- **Safety-critical alignment constraints cannot be relaxed**: For safety-classified work items: LLM alignment checks cannot be disabled, alignment threshold cannot be set below 0.95, and the traceability matrix requires human sign-off. These constraints are enforced at configuration load time, not at check time.
- **Traceability matrix is incremental**: The traceability matrix MUST be built incrementally during pipeline execution, not reconstructed after the fact. Each alignment check contributes its entries as they occur.
- **Alignment check cost is budgeted**: LLM alignment check token usage MUST be counted against the node's token budget. Alignment checks that would exceed the budget are reported as a structured error, not silently skipped.
- **Deterministic checks run first**: Deterministic alignment checks MUST complete before the LLM alignment check is invoked. Their results are included in the LLM check context to avoid redundant LLM work.

---

## Tool Scoping Constraints

- **Tool profile is mandatory for LLM nodes**: Every pipeline node that invokes an LLM MUST have a resolved tool profile before execution. If no explicit profile is configured, the built-in default profile for the node type MUST be applied. A node MUST NOT execute with an undefined or empty tool profile.
- **Defence in depth is mandatory**: Tool access control MUST be enforced at all three layers independently: (1) orchestrator-level tool filtering (unscoped tools never appear in LLM tool lists), (2) tool-level scope enforcement (each tool validates parameters against scope constraints before executing), (3) OS-level sandboxing for write-capable nodes. Failure at one layer MUST NOT compromise the others.
- **Only Code Generation has core write access**: Among the 7 core pipeline nodes, only Code Generation has filesystem write and git commit tools in its default profile. All other core nodes are read-only by default. Pipeline configuration MAY grant write access to custom nodes, but this is an explicit opt-in recorded in the audit trail.
- **LLM-requested validation and Git actions are deterministic tools**: LLMs may request validation or Git operations only through scoped, allowlisted tools with validated parameters. Arbitrary LLM-generated shell or Git commands MUST NOT be executed.
- **Shell execution is restricted**: The `shell.run_restricted` tool executes infrastructure commands (build, test, lint) from an explicit allowlist — NOT arbitrary LLM-generated shell commands. The allowed command list is defined in the tool profile scope parameters, not by the LLM.
- **Network access is opt-in**: The `net.fetch` tool is NOT enabled in any default profile. Enabling network access requires explicit configuration in the tool profile. Network-capable profiles MUST specify URL allowlists.
- **Protected path enforcement at tool level**: Filesystem write tools (`fs.write`, `fs.create`, `fs.delete`) MUST refuse operations targeting protected paths (constitutional rules, prompt templates, scenario specs, Extension API schemas) regardless of the node's profile configuration. This is a hard constraint that cannot be overridden by profile configuration.
- **Scope parameter template resolution is strict**: Template variables (`{{variable}}`) in scope parameters MUST be resolved from pipeline state before node activation. Unresolvable variables MUST produce a clear error and prevent node activation. Empty-string substitution for missing variables is NOT permitted.
- **Skill scope enforcement is unconditional**: Tool calls within skill execution MUST be validated against the calling node's tool profile. A skill MUST NOT bypass scope enforcement. This constraint exists because skills are deterministic scripts — but the node's effective scope may change between runs.
- **Adapter tools are regular tools**: Adapter-generated tool definitions MUST participate in the same tool registry, profile scoping, and scope enforcement as built-in tools. No special bypass for adapter tools.
- **Tool profile validation at load time**: Tool profile definitions are validated when the pipeline configuration is loaded. Profiles that reference nonexistent tools produce warnings (not errors, to allow profile reuse across repos with different adapter sets). Profiles with invalid scope parameter schemas produce errors and prevent pipeline start.

---

## Code Style (Rust-Specific)

- **Follow Rust conventions**: `snake_case` for functions/variables, `PascalCase` for types, `SCREAMING_SNAKE_CASE` for constants (per `.tech-decisions.yml`).
- **Max function length: 50 lines** (per `.tech-decisions.yml`).
- **Max file length: 500 lines** (per `.tech-decisions.yml`).
- **Max cyclomatic complexity: 10** (per `.tech-decisions.yml`).
- **`clippy` clean**: Code must pass `clippy` with no warnings.
- **`rustfmt` formatted**: Code must be formatted with `rustfmt`.
- **Public API documentation**: All public types, functions, and traits must have `///` doc comments including purpose, parameters, return values, error conditions, and examples where non-trivial.
