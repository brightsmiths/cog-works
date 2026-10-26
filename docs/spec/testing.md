# Testing Strategy

This document defines how CogWorks itself is tested. Not how CogWorks tests generated artifacts (that's a domain service operation), but how we verify CogWorks is correct.

---

## Testing Pyramid

```
            ┌─────────┐
            │  E2E    │   Few: Full pipeline against real GitHub + LLM
            │  Tests  │
           ─┼─────────┼─
          │  Integration  │  Moderate: Node implementations with mock services
          │    Tests      │
         ─┼───────────────┼─
        │    Unit Tests       │  Many: Business logic, pure input/output
        │                     │
        └─────────────────────┘
```

---

## Unit Tests (Business Logic)

Business logic is pure — no I/O, no mocks needed. Test with direct input/output assertions.

### Pipeline State Machine

- EventSource-reported readiness enters intake without depending on a particular provider signal
- All valid node transitions via edge conditions
- Invalid transitions (node activated without inputs available)
- Human-gated vs. auto-proceed behavior
- Safety-critical gate override
- Processing lock detection (back off when another instance is active)
- Sub-work-item ordering within code-generation/review/integration nodes
- Default linear pipeline used when no configuration file exists
- Fan-out: multiple downstream nodes activated when conditions met
- Fan-in: downstream node waits until all upstream nodes complete
- Rework edge traversal count enforcement (max traversals)
- Cycle termination: pipeline halts when rework budget exhausted
- Pipeline resume from failed node using persisted state
- Pipeline cancellation: active nodes terminated, state written to GitHub

### Classification Rules

- Safety override: LLM says safe + registry match → override to unsafe
- Safety no-override: LLM says safe + no registry match → remains safe
- LLM says unsafe → always unsafe regardless of registry
- Scope within threshold → proceed
- Scope exceeds threshold → escalation

### Dependency Graph

- Empty graph → valid (no sub-work-items, edge case)
- Linear chain (A → B → C) → valid order [A, B, C]
- Diamond (A → B, A → C, B → D, C → D) → valid order respecting all edges
- Simple cycle (A → B → A) → error identifying cycle
- Complex cycle (A → B → C → A) → error identifying cycle
- Disconnected components → valid order (independent items in stable order)
- Self-dependency (A → A) → error

### Review Aggregation

- All passes clean → proceed
- One blocking finding in quality → remediate
- One blocking finding in security → remediate
- Blocking in multiple passes → remediate (all findings collected)
- Warnings only → proceed (warnings preserved for PR comments)
- Mixed blocking + warnings → remediate (warnings still preserved)
- Remediation cycle count exceeded → escalate

### Budget Enforcement

- Cost within budget → approved
- Cost exactly at budget → approved (boundary condition)
- Cost exceeding budget → denied with cost report
- Cost report structure (per-node, per-sub-work-item breakdown)
- Parallel budget atomicity: two concurrent nodes cannot both be approved if combined cost exceeds budget

### Context Priority and Truncation

- Context fits in window → no truncation
- Context exceeds window → lowest priority items removed first
- Current SWI interface is never removed (highest priority)
- Empty context → valid (edge case)
- Single item exceeding window → item truncated with warning

### Label Parsing and Generation

- Round-trip: structured type → label string → structured type = identity
- All label patterns: node, status, processing, safety-critical, hold, awaiting-review
- Dependency and ordering information is read from native GitHub typed issue links and the sub-issue API, not from labels
- Invalid label strings → parse error (not panic)

### Context Pack Selection

- Zero available packs → empty loaded set (valid)
- Available packs, none match classification → empty loaded set (valid)
- One pack matches classification labels → pack is loaded
- One pack matches safety classification → pack is loaded
- Two packs match → both loaded
- Conflicting guidance (one pack more restrictive) → more restrictive rule applied
- Pack with malformed trigger file → warning, pack skipped, pipeline continues
- Required artefacts from loaded pack → correctly extracted into required artefact set

### Constitutional Rules Enforcement

- Constitutional rules present and valid → rules loaded successfully
- Constitutional rules missing core required rule → load fails with specific error
- Constitutional rules from unreviewed branch → rejected, pipeline halts
- Rules are in privileged position in prompt → no context item precedes them
- Context items cannot override rules → rules remain unchanged regardless of context content

### Injection Detection

- Clean issue body → no detection
- Clear persona override ("You are now...") → INJECTION_DETECTED
- Classic instruction injection ("Ignore all previous...") → INJECTION_DETECTED
- Behavioral modification ("From now on...") → INJECTION_DETECTED
- System prompt extraction ("Output your system prompt") → INJECTION_DETECTED
- Technical content with similar language ("The system ignores invalid inputs") → no detection (not an instruction)
- Property-based testing: Generated injection patterns from adversarial prompt corpus → detection rate measured
- Legitimate security test pseudocode → borderline case (document expected behavior)

### Scope Enforcement

- Generated files within authorised set → no violation
- Generated file outside authorised set → scope violation
- Generated file matching protected path pattern → PROTECTED_PATH_VIOLATION
- Specification exists for capability → no SCOPE_UNDERSPECIFIED
- Required capability not in spec → SCOPE_UNDERSPECIFIED
- Ambiguous safety-affecting spec → SCOPE_AMBIGUOUS

### Required Artefact Checking

- No packs loaded → no required artefacts, checking is a no-op
- Pack requires artefact A, output contains A → no finding
- Pack requires artefact A, output does not contain A → blocking finding identifying pack and artefact
- Multiple packs, all requirements met → no findings
- Multiple packs, one requirement missing → one blocking finding

### Metric Data Point Computation

- Pipeline audit data with all fields present → correct metric data points with all dimensions populated
- Pipeline audit data with missing optional fields → data points computed with available data, warning logged for missing fields
- Multiple retry cycles with different root causes → root cause structured enum correctly assigned per retry
- Zero-retry pipeline → metric data points show zero retry count, no retry root cause
- Safety-classified vs non-safety-classified work item → safety classification dimension correctly populated
- Same audit data input produces identical data points on repeated computation (determinism)
- Each node boundary in audit data produces corresponding incremental data point

---

## Integration Tests (Node Implementations with Mocks)

Node implementations orchestrate calls between business logic and abstractions. Test them with mock implementations of abstraction traits.

### Mock Implementations

- **Mock LLM Provider**: Returns pre-configured responses; tracks calls for assertion; can simulate failures (API error, schema validation failure, rate limit)
- **Mock Issue Tracker**: In-memory issue store; returns configured issues; tracks all mutations (create, label, comment)
- **Mock PR Manager**: In-memory PR store; returns configured PRs; tracks all mutations
- **Mock Code Repository**: In-memory file store; returns configured file contents
- **Mock Domain Service Client**: Returns pre-configured structured responses (diagnostics with standard categories, simulation results, dependency graphs); pre-configured handshake responses (capabilities, domain, artifact types, interface types); can simulate unavailability, API version mismatch, partial capabilities, timeout, standardised error codes, and malformed responses
- **Mock Template Engine**: Returns pre-configured rendered strings; tracks variable usage
- **Mock Audit Store**: In-memory event log; allows assertions on recorded events
- **Mock Interface Registry Loader**: Returns pre-configured interface definitions; can simulate missing definitions, schema errors, or conflicts
- **Mock Constraint Validator**: Returns pre-configured constraint validation results; can simulate hard failures, warnings, and extraction errors
- **Mock Context Pack Loader**: Returns pre-configured pack sets; can simulate zero matches, single match, multiple matches, pack with contradictory guidance, and load failures (malformed trigger file, missing pack directory)
- **Mock Constitutional Rules Loader**: Returns pre-configured rules payload; can simulate missing file, invalid content (missing required rule), and unreviewed branch rejection
- **Mock Injection Detector**: Returns pre-configured detection results; can simulate detection with specific offending text, no detection, and borderline cases for testing pipeline halt behavior
- **Mock Scope Enforcer**: Returns pre-configured scope validation results; can simulate SCOPE_UNDERSPECIFIED, SCOPE_AMBIGUOUS, PROTECTED_PATH_VIOLATION, and clean pass
- **Mock Implementation Job Runtime Provider**: Tracks one container request per implementation job and can simulate startup, execution, cancellation, and cleanup failures
- **Mock Credential Provider**: Supplies test credentials to infrastructure adapters and allows assertions that credentials never enter model content, audit records, or logs

### Test Scenarios per Node

**Task Classifier (Intake Node):**

- Happy path: Issue body → LLM classification → safety cross-validation → labels applied
- LLM returns invalid schema → retry with error → valid response → proceed
- Safety override triggered → safety-critical label applied
- Scope exceeded → escalation produced

**Specification Generator (Architecture Node):**

- Happy path: Context assembled (with matching Context Pack) → LLM generates spec → validation passes → PR created
- Happy path (no packs): Context assembled (no matching packs) → LLM generates spec → validation passes → PR created
- Module reference validation fails → retry with error → passes on retry → PR created
- Constraint violation → retry with error → fails again → retry budget exceeded → escalation
- PR already exists (idempotency) → detected, no duplicate created
- Context Pack loaded → pack content included in LLM context (verified via Mock LLM Provider call capture)

**Interface Generator (Interface Design Node):**

- Happy path: Spec read → context assembled → LLM generates interfaces → domain service validates → PR created
- Validation fails → retry with error → passes on retry → PR created
- Parse error in generated code → retry with error

**Work Planner (Planning Node):**

- Happy path: Decomposition → validation → sub-issues created
- Cycle detected → retry with error → acyclic plan → sub-issues created
- Too many sub-work-items → escalation
- Missing interface coverage → retry with error

**Code Generator (Code Generation Node):**

- Happy path: Establish tests or domain acceptance checks → implement → normalise → validate → review_rules → simulate → pass
- Validation failure → feedback → retry → pass
- Simulation failure (self-explanatory) → direct feedback → retry → pass
- Simulation failure (non-obvious) → LLM interpretation → feedback → retry → pass
- Retry budget exhausted → escalation
- Cost budget exceeded mid-generation → halt with report
- Domain service unavailable → early failure with diagnostic

**Review Executor (Review Node):**

- Happy path: Four passes (1 deterministic + 3 LLM), all clean → proceed
- Required artefact from loaded pack is missing → blocking finding produced, no PR creation
- Required artefacts present → no artefact-related finding, review proceeds to LLM passes
- Blocking finding → feed back to code generator
- Multiple remediation cycles → finding resolved → proceed
- Remediation cycle limit exceeded → escalation

**Integration Manager (Integration Node):**

- Happy path: PR created with correct references and comments
- Non-blocking findings posted as inline comments
- PR already exists (idempotency) → detected

### Pipeline Executor (Graph Traversal)

- Fresh work item → constitutional rules loaded first, then progresses through all nodes
- Missing constitutional rules → pipeline halts before any node runs
- Injection detected in issue body → pipeline halts, work item enters hold state
- Work item in hold state → invocation detects hold state, exits without action
- Re-processing after crash at each node → resumes from failed node using persisted state
- Human-gated node → stops and waits
- Safety-critical work item → forced human gates
- Protected path violation in generated files → PR creation blocked
- Custom pipeline graph → correct traversal order
- Fan-out → parallel nodes activated and execute concurrently
- Fan-in → downstream node waits for all upstreams
- Rework edge → re-executes node with preserved/discarded outputs per configuration
- Rework max traversals reached → escalation or overflow edge
- Pipeline cancellation → active nodes terminated, state persisted
- Duplicate pipeline trigger → rejected with comment
- Implementation job requests exactly one ephemeral container and requests cleanup on success, failure, and cancellation
- External factory/GateKeeper status is passed through the work-item interface and does not replace CogWorks node gates

---

## Integration Tests (Infrastructure)

Each infrastructure implementation tested against the real external system or a faithful simulation.

### GitHub Client

- Use a dedicated test repository on GitHub
- Test: create issue, add label, remove label, post comment, create branch, commit file, create PR, read PR status, list sub-issues
- Test rate limit handling: verify backoff when rate limit headers indicate low budget
- Test pagination: verify correct handling of paginated responses

### Anthropic LLM Provider

- Use mock HTTP server (e.g., `wiremock`) simulating Anthropic API
- Test: successful completion, rate limit response, server error, malformed response
- Test token counting accuracy against known inputs
- Test retry behavior with exponential backoff

### Extension API Client

- Use a mock domain service process (test harness that speaks the Extension API protocol)
- Test: health_check (handshake), validate, normalise, review_rules, simulate, dependency_graph, extract_interface_surface
- Test: Unix domain socket transport — connection, message exchange, disconnection
- Test: HTTP transport — connection, message exchange, error responses
- Test: Handshake response parsing — capabilities, artifact types, interface types, domain, API version discovered correctly
- Test: Handshake result caching — re-query on error, periodic refresh
- Test: timeout enforcement — domain service that never responds is terminated after configured timeout
- Test: malformed response handling — invalid JSON, schema-violating response, unexpected message type
- Test: API version negotiation — handshake returns incompatible version → graceful rejection
- Test: domain service crash mid-operation → CogWorks detects disconnection and reports failure
- Test: standardised error code handling — each error code mapped to correct retryability
- Test: diagnostic category handling — known and unknown categories parsed correctly

### Interface Registry Loader

- Use fixture `.cogworks/interfaces/` directory with known TOML files
- Test: valid definitions loaded and parsed correctly
- Test: malformed TOML → clear error with file name and line
- Test: schema-violating definition → clear error identifying the violation
- Test: conflicting parameter definitions across files → detected and reported
- Test: missing interfaces directory → empty registry (not error)
- Test: version format validation

### Observability Hooks

- `record_node_start` records run ID, node ID, and `node_start` on a span that predeclares those fields
- `record_node_complete` records run ID, node ID, `node_complete`, and numeric total USD cost
- `record_retry` and `record_rework` record stable event kinds and typed categories
- Undeclared span fields remain absent; tests verify callers declare the required fields
- CLI subscriber configuration routes spans to JSON console output and the configured OTLP exporter

### Implementation Job Runtime Provider

- Exactly one container request is made per implementation job
- Cleanup is requested after success, failure, and cancellation
- Provider startup failure is surfaced without leaving a job active
- Runtime-specific behavior is tested through a faithful fake/provider contract, without requiring a specific runtime in business-logic tests

---

## Extension API Conformance Test Suite

CogWorks publishes a conformance test suite that domain service authors run against their implementations. This is separate from CogWorks' own tests.

### Purpose

- Verify a domain service correctly implements the Extension API protocol
- Catch protocol mismatches before deployment
- Serve as living documentation of the API contract

### Coverage

- **Health check / handshake**: Returns correct `name`, `service_version`, `api_version`, `domain`, `capabilities`, `artifact_types`, `interface_types`, and `status`
- **Validate**: Accepts artifact, returns structured diagnostics (with `severity`, `category`, `location`, `message`) in a common diagnostic format
- **Normalise**: Accepts artifact, returns normalised artifact content or no-change indicator
- **Review rules**: Accepts artifact, returns domain-specific rule findings
- **Simulate**: Accepts test specification, returns pass/fail with structured output
- **Dependency graph**: Accepts artifact set, returns adjacency list
- **Extract interface surface**: Accepts artifact, returns public interface description
- **Error responses**: Correct error codes for invalid requests, unsupported capabilities, internal failures
- **Progress polling** *(Future — v2)*: Long-running operations return operation ID; `poll_progress` returns correct status transitions. Not required in v1 (synchronous request/response only)
- **Graceful shutdown**: Domain service handles termination signals cleanly

### Distribution

- Published as a standalone binary or test harness alongside CogWorks releases
- Domain service authors integrate it into their CI pipeline

---

## End-to-End Tests

Full pipeline tests against real GitHub and real LLM API. These are expensive and slow — run sparingly (nightly, pre-release).

### Setup

- Dedicated test repository on GitHub with known structure
- Test issues with known expected outcomes
- LLM API key with cost budget for testing

### Scenarios

- **Happy path**: Create issue → configure the EventSource/work-item adapter to report readiness → verify the pipeline completes → verify PRs are created with correct content
- **Safety-critical path**: Create issue touching safety-critical module → verify human gates enforced
- **Escalation path**: Create issue with scope exceeding threshold → verify escalation comment posted
- **Re-trigger path**: Fail a sub-work-item intentionally → re-trigger → verify recovery

### Cost Control

- E2E tests have their own cost budget (separate from production)
- Tests that would exceed budget are skipped with a warning
- Use cheaper/faster LLM models for E2E testing where possible

---

## Test Fixtures

The test suite requires several fixture types:

- **Fixture domain projects**: Small, valid projects for each supported domain (e.g., Rust crate, KiCad schematic) with known validation results, simulation results, and interface surfaces. Stored in `tests/fixtures/domains/`.
- **Fixture interface registries**: Hand-crafted `.cogworks/interfaces/` directories with known valid, invalid, and conflicting TOML definitions. Stored in `tests/fixtures/interfaces/`.
- **GitHub API response fixtures**: Recorded or hand-crafted JSON responses for GitHub API calls. Stored in `tests/fixtures/github/`.
- **LLM response fixtures**: Recorded or hand-crafted LLM responses for each node type's output schema. Stored in `tests/fixtures/llm/`.
- **Extension API message fixtures**: Hand-crafted JSON request/response pairs for each Extension API method. Stored in `tests/fixtures/extension_api/`.
- **Configuration fixtures**: Various `.cogworks/config.toml` files exercising different configuration options (including domain service registration). Stored in `tests/fixtures/config/`.
- **Pipeline configuration fixtures**: Various `.cogworks/pipeline.toml` files exercising: default linear pipeline, custom graph with fan-out/fan-in, graph with rework loops, graph with conditional edges (deterministic and LLM-evaluated), graph with spawning nodes, multiple named pipelines, invalid graphs (unterminated cycles, orphan nodes, duplicate names). Stored in `tests/fixtures/pipelines/`.
- **Alignment check fixtures**: Hand-crafted alignment check inputs and expected results for each pipeline stage. Includes: deterministic check inputs (input/output pairs with known misalignments), LLM alignment check response fixtures (JSON responses with scores, findings, traceability entries), merged finding sets, and rework context fixtures. Stored in `tests/fixtures/alignment/`.

---

## Alignment Verification Unit Tests

The alignment verification subsystem requires targeted unit test scenarios:

### Deterministic Check Tests

- **Missing element detection**: Input specification lists functions A, B, C; output implements only A, B. Assert: finding with type `missing`, severity `blocking`, referencing C.
- **Extra element detection**: Input specification lists functions A, B; output implements A, B, C. Assert: finding with type `extra`, severity `warning`.
- **Scope exceeded detection**: Input specification scopes to module X; output modifies module Y. Assert: finding with type `scope_exceeded`, severity `blocking`.
- **Clean pass**: Input and output structurally match. Assert: no findings, deterministic check passes.

### LLM Alignment Check Tests

- **Semantic drift detection**: Fixture with input requiring "exponential backoff" and output implementing "fixed delay". Assert: LLM response parsed correctly, finding with type `modified` produced.
- **Ambiguity detection**: Fixture with ambiguous input and output making an assumption. Assert: finding with type `ambiguous`, severity `warning`.
- **Score parsing**: Various LLM response fixtures with scores. Assert: scores parsed correctly, threshold comparison correct.
- **Model separation verification**: Assert alignment check invokes LLM Gateway with a different model identifier than the generating node.

### Finding Merger Tests

- **Deduplication**: Deterministic and LLM checks both find the same missing function. Assert: merged result contains one finding, not two.
- **Severity aggregation**: Multiple findings with mixed severities. Assert: any `blocking` finding causes overall failure.
- **Empty merge**: Both check types return zero findings. Assert: alignment passes with score from LLM check.

### Rework Triggering Tests

- **Alignment failure triggers rework**: Alignment check fails. Assert: rework signal sent to executor with findings in context.
- **Rework budget exhaustion**: Rework counter reaches budget limit. Assert: pipeline failure with structured error.
- **Rework counter independence**: Assert rework counter and retry counter are tracked separately.

### Traceability Matrix Tests

- **Incremental construction**: Two sequential alignment checks. Assert: matrix contains entries from both.
- **N/A handling**: Requirement not addressed at a stage. Assert: matrix shows N/A with reason.
- **Safety sign-off flag**: Safety-classified work item. Assert: matrix flagged for human sign-off.

### Mock Alignment Verifier

For integration tests that do not need to exercise alignment logic:

- Returns configurable alignment results (pass/fail, findings, score)
- Records all invocations for assertion (inputs received, order of calls)
- Supports per-node configuration (different results per stage)
- Default behavior: always passes with score 1.0 and no findings

---

## Tool Scoping and Enforcement Tests

### Tool Profile Resolution Tests

- **Default profile application**: No `[tool_profiles]` in config. Assert: each core node receives its built-in default profile with expected tools.
- **Profile override**: Base profile is `reader`; node has `tool_overrides` adding `fs.write` with `allowed_write_paths`. Assert: resolved profile includes base tools plus `fs.write` with specified scope.
- **Template variable resolution**: Profile with `allowed_write_paths = ["src/{{affected_modules}}/**"]` and pipeline state `affected_modules = "auth"`. Assert: resolved path is `["src/auth/**"]`.
- **Unresolvable template variable**: Profile references `{{undefined_var}}`. Assert: profile resolution fails with structured error; node does not activate.
- **Unknown tool in profile**: Profile references `nonexistent.tool`. Assert: warning logged; profile resolves without the unknown tool.

### Tool Filtering Tests (Layer 1)

- **Only profiled tools in LLM call**: Node profile includes `fs.read`, `search.code`. Assert: LLM tool list contains exactly these tools and no others.
- **Empty profile**: Node has a profile with no tools. Assert: LLM call includes no tools (but the call itself still proceeds for nodes that may operate without tools).
- **Adapter tools in profile**: Profile includes an adapter-generated tool. Assert: tool appears in LLM tool list with correct schema.
- **Skill tools in profile**: Node has Active skills. Assert: `skill.list`, `skill.run`, `skill.inspect` appear in tool list.

### Scope Enforcement Tests (Layer 2)

- **Path within scope**: `fs.write` with path inside `allowed_write_paths`. Assert: operation proceeds.
- **Path outside scope**: `fs.write` with path outside `allowed_write_paths`. Assert: operation rejected, structured error returned, `SCOPE_VIOLATION` event in audit.
- **Denied path takes precedence**: Path matches both `allowed_write_paths` and `denied_write_paths`. Assert: operation rejected (deny takes precedence).
- **Protected path override attempt**: `fs.write` targeting `.cogworks/constitutional-rules.md` even though profile allows the path. Assert: operation rejected with `PROTECTED_PATH_VIOLATION`.
- **Branch pattern enforcement**: `git.commit` on branch not matching `branch_pattern`. Assert: commit rejected.
- **Service scope enforcement**: `domain.validate` with service name not in `allowed_services`. Assert: invocation rejected.
- **Violation threshold**: 5 scope violations in one node execution. Assert: audit warning emitted after 5th violation.

### Tool Audit Tests

- **Invocation recording**: Tool call succeeds. Assert: audit trail entry contains tool name, parameters, scope parameters, result, duration.
- **Violation recording**: Scope violation occurs. Assert: `SCOPE_VIOLATION` event in audit with tool name, attempted params, violated constraint.
- **Usage report**: Pipeline completes. Assert: pipeline summary includes tool usage report with per-tool counts, violation counts, invocation type breakdown.

---

## Skill Executor Tests

### Skill Invocation Tests

- **Valid invocation**: Active skill with valid parameters. Assert: skill executes, tool calls proceed through scope enforcement, result returned.
- **Missing required parameter**: Skill invoked without a required parameter. Assert: execution rejected with structured error before any tool calls.
- **Invalid parameter type**: Skill invoked with wrong parameter type. Assert: execution rejected with structured error.
- **Scope violation within skill**: Skill calls `fs.write` to path outside node's scope. Assert: entire skill execution fails, scope violation recorded.

### Skill Lifecycle Tests

- **Only Active skills visible**: Skills in Proposed, Reviewed, Deprecated, Retired states. Assert: `skill.list` returns only Active skills (and Deprecated with warning).
- **Deprecated skill warning**: Invoking a Deprecated skill. Assert: execution proceeds with warning referencing alternative.
- **Retired skill rejected**: Invoking a Retired skill. Assert: execution rejected with structured error.
- **Auto-deprecation**: Skill with 85% success rate (below 90% threshold). Assert: lifecycle state changes to Deprecated with `success_rate_below_threshold` reason.

### Mock Skill Executor

For integration tests that do not need to exercise skill logic:

- Returns configurable skill execution results (success/failure)
- Records all invocations for assertion
- Default behavior: always succeeds

---

## Adapter Generator Tests

### Generation Tests

- **OpenAPI 3.1 generation**: Valid OpenAPI spec with 3 endpoints. Assert: 3 TOML tool definition files generated with correct namespacing.
- **EAB schema generation**: Valid EAB JSON Schema. Assert: tool definitions generated with correct parameter schemas.
- **Schema conformance**: Generated tool definitions. Assert: all conform to the tool definition schema.

### Drift Detection Tests

- **No drift**: Generated adapters match source spec. Assert: drift check reports no differences.
- **Removed endpoint**: Source spec has fewer endpoints than generated adapters. Assert: drift report identifies removed endpoint.
- **Changed parameter**: Source spec has a different parameter type for an endpoint. Assert: drift report identifies the parameter change.

---

## Progressive Discovery Tests

### Activation Tests

- **Above threshold**: Node with 20 tools. Assert: compact index presented, meta-tools available.
- **Below threshold**: Node with 10 tools. Assert: full schemas presented, no meta-tools.
- **Exact threshold**: Node with exactly 15 tools (default threshold). Assert: compact index activated.

### Meta-Tool Tests

- **tools.search**: Search query "validate". Assert: returns matching tools with descriptions.
- **tools.schema**: Request schema for specific tool. Assert: returns full JSON Schema.
- **tools.call**: Invoke tool via meta-tool. Assert: tool executes with scope enforcement.
- **Skill priority**: Search matching both a skill and raw tool. Assert: skill ranked first.

---

## Coverage Requirements

| Layer | Coverage Target | Measured By |
|-------|----------------|-------------|
| Business logic | 100% line coverage | `cargo tarpaulin` or `grcov` |
| Node implementations | 90% line coverage | `cargo tarpaulin` or `grcov` |
| Infrastructure | 80% line coverage | `cargo tarpaulin` or `grcov` |
| E2E | Key scenarios covered | Manual checklist |

Mutation testing (per `.tech-decisions.yml` target of 70%) applied to business logic modules.
