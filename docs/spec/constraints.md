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
- **Stage drivers are orchestration, not business logic**: A stage driver coordinates calls between business logic, abstractions, and other components. It contains sequencing logic, not domain rules.
- **Trust-zone barriers are enforced during bundle assembly**: The Context Bundle Builder MUST exclude holdout content and acceptance source from any bundle built for a build-zone stage. This is a hard constraint, not advisory, and it is enforced by the allowlist rather than by a list of exclusions — an input the allowlist does not name is absent from the bundle.
- **Business logic never computes or publishes a verdict**: No business-logic module may decide that an artifact passes, author a check run, or satisfy a required check. Verdicts are read from the gate service's check runs. CogWorks' own deterministic pre- and post-checks bound an attempt and are never published as a check run.
- **The harness is read-only and pinned**: No module may write to the harness, and no module may resolve a floating harness reference. Every read resolves through the version pinned by the target repository.
- **A stage contract is never visible to the agent it governs**: The contract names the agent's allowlist, writable paths, limits and identity role. It MUST NOT be placed in the bundle of the stage it governs.
- **Bundle hashing is mandatory**: Every context bundle MUST be hashed and the hash recorded before the attempt that consumes it starts. An attempt whose bundle was not hashed MUST NOT be requested.
- **The harness loader, stage runner, check-run reader, mount-plan provider and risk-map loader are abstractions**: Business logic depends on their traits, never on a harness source, a runner API, the Checks API or a YAML parser.
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

- **Every cycle must have a termination condition**: The graph validator must reject any resolved graph containing a cycle without an explicit `max_traversals` on at least one edge in the cycle. There must be no path to infinite execution.
- **Cost budget is shared across parallel nodes**: When nodes execute concurrently, budget acquisition must be atomic. Two nodes must not both be approved if their combined estimated cost exceeds the remaining budget.
- **Pipeline state is recoverable from GitHub**: The pipeline working directory is a performance optimisation. All pipeline state must be reconstructable from GitHub artifacts (issue comments, labels, PRs). Loss of the working directory must not require restarting the pipeline from scratch.
- **Stage attempt sandboxes are not durable state**: Sandbox-local files are transient. Durable pipeline state and deliverables must be persisted through the work-item and repository interfaces before the sandbox is torn down. One fresh sandbox per attempt; no sandbox is reused across attempts.
- **Node inputs must be verified before execution**: The graph execution engine must verify that all declared inputs for a node are available before starting the node. Missing inputs must produce a clear error identifying the node and the missing input.
- **Stage types are validated against the registry at load**: Every node in a resolved graph must declare a stage type present in the pinned harness's stage-type registry. A node declaring an unknown stage type must fail graph validation before any dispatch is attempted.
- **The stage graph comes from the harness**: The graph governing a work item is resolved from the pinned harness for its `(tier, discipline)` pair. No repository-local file may add a stage type, remove a stage the tier requires, or change a gate.
- **Edge conditions are deterministic**: An edge condition is a deterministic expression over pipeline state and verdicts. No condition may be evaluated by a model: routing is code, so which edge is taken never depends on model output.
- **Edge condition evaluation is audited**: Every edge condition evaluation (deterministic or composite) must be recorded in the audit trail with the condition definition, the evaluation input, and the result.
- **The resolved graph is validated at load time**: The resolved stage graph must be fully validated when loaded, before any stage executes. An invalid graph must produce clear errors and prevent the run from starting.

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
- **Minimum-privilege GitHub credential**: The credential must carry only what CogWorks needs: issues (read/write), pull requests (read/write), contents (**read only**), labels (read/write), checks (**read only**). No admin access.
- **No push credential**: CogWorks MUST NOT hold a credential capable of pushing commits to a target repository. Commits are pushed by the stage runner under the stage's identity role. A `contents:write` grant on this process is a defect.
- **No model API key**: CogWorks MUST NOT hold a model provider API key, and neither may any sandbox. Model credentials are held by the model gateway, and every model call reaches a provider through it.
- **No check-run authorship**: CogWorks MUST NOT hold a credential capable of creating or updating a check run. Verdicts are authored by the gate service alone.
- **No secrets in context bundles**: Credentials of any kind must never appear in a context bundle, an audit trail, or generated code.
- **No secrets in generated code**: Code generation must use placeholder values for secrets, with documentation on what needs to be configured.
- **Domain service isolation**: Domain services run as separate processes. CogWorks does not pass secrets to domain services. Domain services receive only the Extension API context (work item info, node, repository path, relevant interface contracts).
- **Prompt injection awareness**: Issue bodies and repository content are untrusted input. They enter a bundle as data, never as instructions, and event or work-item fields are never interpolated into a prompt. Screening incoming work items belongs to the upstream intake service, whose signed approval CogWorks verifies; sanitising model-sourced text before reusing it in a later prompt remains CogWorks' own defence in depth.
- **Constitutional rules are mandatory**: Constitutional rules MUST be resolved from the pinned harness and loaded before any model call on every run. No configuration option may disable this. Failure to resolve or verify them halts the run. This is not a security option — it is a hard constraint.
- **Protected paths**: CogWorks MUST NOT create or modify files matching protected path patterns through the normal pipeline, and no mount plan may make one writable. Protected paths include at minimum: the pin file, the risk map, locked tests and their manifest, gate configuration, and the Extension API schemas. The protected path list is version-controlled and configurable, and these defaults cannot be removed by configuration.
- **Protected-path matching is case-sensitive**: Glob matching for protected paths and approved-scope patterns (`security::is_protected`, `security::validate_scope`, `alignment::run_deterministic_alignment`) is case-sensitive. On a case-insensitive filesystem (default Windows, default macOS), a declared protected path such as `"SECURITY.md"` does not match an artifact key that differs only in case (e.g. `"Security.md"`), even though both resolve to the same file at write time. Deployments on case-insensitive filesystems MUST either enumerate case variants explicitly in the protected-path list, or run CogWorks only on case-sensitive filesystems (the recommended default for CI/production runners). This is a pre-existing constraint of the underlying glob engine, not a bug in any single check.
- **Rate limit respect**: The system must respect GitHub API rate limits (5000/hr for authenticated requests). Track remaining budget from response headers, back off proactively.
- **Model rate limiting is the gateway's**: CogWorks does not track or enforce model provider rate limits. The model gateway holds the credentials and sees all factory traffic, so it is the only component positioned to enforce them. A gateway response indicating throttling is surfaced as a retriable condition.
- **Extension API authentication**: The Unix domain socket transport is the default, and file system permissions on the socket provide its access control. The HTTP/gRPC transport MUST only be used where the network path between CogWorks and the domain service lies inside the deployment's trust boundary. Its transport design MUST NOT preclude adding an authentication mechanism (for example bearer tokens or mutual TLS) without a breaking change.

---

## Configuration

- **Configuration describes the host, not the repository**: CogWorks' own configuration carries the endpoints it talks to, its label mappings and its concurrency limits. It is loaded once per invocation. Everything that shapes an agent — stage graphs, stage contracts, profiles, prompt templates, output schemas, Context Packs and constitutional rules — comes from the pinned harness, and no repository-local file may override it.
- **Mandatory fields**: At minimum: a reference to a domain service registration file and at least one LLM model selection.
- **Domain service registration**: Domain services are declared in CogWorks' own process configuration. Each entry specifies name, transport, and connection endpoint (socket path or URL). Service capabilities, artifact types, interface types, and domain are discovered dynamically via the handshake — they are NOT statically configured, so the configuration cannot drift from what the service actually provides.
- **Interface registry configuration**: `[interfaces]` section specifying registry directory and startup validation flag.
- **Constraint validation configuration**: `[constraint_validation]` section specifying enabled flag and missing-service behavior.
- **Sensible defaults**: Every configurable value must have a sensible default. A minimal configuration file should be sufficient to run the pipeline.
- **Validation at load time**: Configuration must be fully validated when loaded. Invalid configuration produces a clear error message and halts the pipeline before any work begins.
- **No environment-variable-driven behavior in business logic**: Environment variables are read in the infrastructure/configuration layer only. Business logic receives typed configuration values, never raw strings from the environment.
- **Label configuration**: Workflow-semantic labels and signals (trigger, awaiting-review, hold, cancel, restart, slice) must be configurable in CogWorks' own process configuration. Duplicate mappings must be rejected at load time, including collisions with run-state labels. Business logic must not depend on a particular label string or project status. A work item's tier MUST NOT be read from a label.
- **Native sub-issues for work item hierarchy**: Sub-work-items must be created as native GitHub sub-issues (using the sub-issue API), not as standalone issues linked only by labels. Dependencies between sub-work-items must use native GitHub typed issue links (`blocks` / `is blocked by`).
- **Milestone inheritance**: Sub-work-items must inherit the parent work item's GitHub Milestone when one is set. CogWorks must not create, modify, or delete milestones.
- **Semantic stalling threshold**: The consecutive-same-class threshold (`semantic_stall_threshold`, default: 3) must be validated against the attempt budget the resolved stage contract declares. Where the stalling threshold exceeds that budget, a warning must be emitted and stalling detection is treated as disabled for that stage. This is a warning, not an error — the run still starts.

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
- **Pack content is trusted as harness configuration**: Loaded Context Pack content is treated as configuration-level input — higher trust than work-item or repository content, lower trust than the constitutional rules. Pack content cannot override constitutional rules.
- **Pack loading is deterministic**: The same classification input must always produce the same set of loaded packs. Pack selection must be a pure function with no randomness or LLM involvement.
- **Packs live in the harness**: Context Packs are part of the harness discipline pack, subject to its review and branch protection, and adopted by a repository only when its pin moves. A pack is prompt material; a repository-local pack would be prompt content an agent working in that repository could edit.
- **Pack content never includes executable code**: Pack files are Markdown and TOML documents. They must not contain scripts, compiled artifacts, or anything that could be executed by CogWorks.
- **Packs tighten, never loosen**: A Context Pack may declare stricter guidance or thresholds for its domain. It may never relax a global threshold, and it may never override constitutional rules.

---

## Constitutional Layer Constraints

- **Constitutional rules loading is unconditional**: There MUST NOT be a configuration option to skip constitutional rules loading. Any code path that calls the LLM without loading constitutional rules first is a defect.
- **Constitutional rules position is fixed**: The constitutional rules are always the first component of the system prompt. No business logic may prepend content before them or demote them in the prompt order. For a deterministic helper call made in CogWorks' own process, this is enforced at the type level, so no call can reach a model without passing through constitutional assembly. For an agent turn, the resolved profile places the rules and the sandbox boundary preserves that position.
- **Constitutional rules are not context items**: Constitutional rules are NOT subject to context truncation. They are not in the context priority queue. They are always present in full.
- **Constitutional rules change requires human approval**: Rules are changed by a reviewed pull request in the harness, and adopted by a repository only when its pin moves. CogWorks verifies the resolved rules against the pin before use, and halts rather than proceed with rules it cannot verify. A repository cannot change the rules that govern its own runs.
- **Injection detection is pre-prompt**: Injection detection runs before any external content is assembled into the LLM prompt. Detecting injection after prompt assembly is insufficient.
- **Hold state is enforced by label**: A work item in hold state carries the configured hold label. Every invocation must check for it and exit without action. The label can only be removed by a human.
- **Protected paths are configurable but non-empty**: At minimum the pin file, the risk map, locked tests and their manifest, gate configuration, and the Extension API schemas are always in the protected path set, regardless of configuration. These defaults cannot be removed via configuration.

---

## Alignment Verification Constraints

- **Alignment checker model separation**: The LLM alignment check SHOULD use a different model from the generating node. Using the same model risks correlated blind spots (a model is unlikely to detect its own semantic errors). When only one model is available, this constraint is relaxed with a warning logged, not silently ignored.
- **Rework and retry budgets are independent**: Rework (alignment failure) and retry (technical failure) MUST have separate counters and separate configurable budgets. Exhausting one does not affect the other. Both appear independently in the audit trail and metrics.
- **Rework budget is enforced**: When the rework budget declared by the stage contract is exhausted, the run MUST escalate with a structured error including the findings from the final rework attempt. It MUST NOT silently pass after rework exhaustion.
- **Finding types are structured enums**: Alignment finding types (missing, extra, modified, ambiguous, scope_exceeded) and severity levels (blocking, warning, informational) MUST be structured enums, not free-form strings. This enables reliable external aggregation and deterministic pass/fail evaluation.
- **Blocking findings always fail**: If any alignment finding has severity `blocking`, the alignment check MUST fail regardless of the alignment score. The score threshold is necessary but not sufficient.
- **An alignment result is not a verdict**: An alignment check bounds a stage attempt. CogWorks may fail, retry or rework the attempt on its result, but MUST NOT publish it as a check run or treat it as a gate verdict. An attempt that passes every alignment check still has to pass the gate service.
- **Tier alignment constraints cannot be relaxed**: At the higher tiers the LLM alignment check cannot be disabled, the alignment threshold cannot be set below the tier's floor, and the traceability matrix requires human sign-off. These constraints are enforced when configuration is loaded, not at check time, and neither a profile nor any repository-level configuration may weaken them.
- **Traceability matrix is incremental**: The traceability matrix MUST be built incrementally during pipeline execution, not reconstructed after the fact. Each alignment check contributes its entries as they occur.
- **Alignment check cost is budgeted**: LLM alignment check token usage MUST be counted against the stage's token budget. Alignment checks that would exceed the budget are reported as a structured error, not silently skipped.
- **Deterministic checks run first**: Deterministic alignment checks MUST complete before the LLM alignment check is invoked. Their results are included in the LLM check context to avoid redundant LLM work.

---

## Stage Attempt Constraints

- **One fresh sandbox per attempt**: Every stage attempt runs in a sandbox created for it and torn down after it. A retry is a new attempt and therefore a new sandbox. No sandbox is reused across attempts, and no state carries between them except through GitHub.
- **No agent runs in CogWorks' process or container**: Every agent turn happens in a sandbox owned by the stage runner. CogWorks holds no virtualisation privilege and no host privilege.
- **Every attempt carries a mount plan**: An attempt request MUST carry a mount plan obtained from the mount-plan provider for the stage's identity role. CogWorks MUST NOT be the only enforcement point for a write boundary.
- **Identity roles are per stage, not per run**: The identity an attempt's commits are pushed under comes from the stage contract. The oracle and implement stages MUST run under different identities, and the implement identity MUST NOT be able to write the locked-test paths.
- **Locked tests exist before implementation**: An `implement` attempt MUST NOT be requested until the locked tests and their signed manifest exist for the slice. A build-zone stage MUST NOT be able to modify them.
- **Disputes are raised, not resolved**: Where an agent judges a locked test to be wrong, the outcome is a dispute recorded for a human. A locked test is never edited by the stage it constrains.
- **Rework feedback is bounded**: Feedback into a rework attempt carries the gate's structured output — failing test identities and messages — never test source. For a holdout, feedback carries no more than the failing requirement identities.
- **Network access is the profile's and the sandbox's**: A stage attempt's network reachability is declared by the network profile in its stage contract and enforced at the sandbox boundary, deny-by-default. CogWorks does not filter an agent's tool list at run time, because the agent does not run in CogWorks.
- **Attempt limits come from the stage contract**: The attempt and rework limits for a stage are declared by its resolved contract, not by repository-level configuration. Exceeding either is an escalation, never a silent pass.

---

## Code Style (Rust-Specific)

- **Follow Rust conventions**: `snake_case` for functions/variables, `PascalCase` for types, `SCREAMING_SNAKE_CASE` for constants (per `.tech-decisions.yml`).
- **Max function length: 50 lines** (per `.tech-decisions.yml`).
- **Max file length: 500 lines** (per `.tech-decisions.yml`).
- **Max cyclomatic complexity: 10** (per `.tech-decisions.yml`).
- **`clippy` clean**: Code must pass `clippy` with no warnings.
- **`rustfmt` formatted**: Code must be formatted with `rustfmt`.
- **Public API documentation**: All public types, functions, and traits must have `///` doc comments including purpose, parameters, return values, error conditions, and examples where non-trivial.
