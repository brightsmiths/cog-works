# Behavioral Assertions

This document defines testable behavioral assertions for CogWorks. Each assertion follows a Given/When/Then structure and maps to specific requirements from the spec. These assertions guide the interface designer on error types, inform the planner on test coverage, and give the coder clear implementation targets.

---

## Pipeline State Machine

### ASSERT-PSM-001: Ready work item enters intake

- **Given**: The EventSource reports a work item ready for processing, and the work item has no active-node state
- **When**: The step function processes this work item
- **Then**: The Task Classifier is invoked, and `cogworks:node:intake` label is applied
- **Traces to**: REQ-PIPE-001, REQ-PIPE-002

### ASSERT-PSM-002: Completed node activates downstream nodes

- **Given**: A work item at node N whose outgoing edge conditions are satisfied
- **When**: The step function processes this work item
- **Then**: All downstream nodes whose edge conditions are satisfied and inputs are available are activated; `cogworks:node:<downstream>` labels are applied
- **Traces to**: REQ-PIPE-003, REQ-EDGE-001

### ASSERT-PSM-003: Human-gated node waits for approval

- **Given**: A work item at node N with `human-gated` configuration, and the gate has not been approved
- **When**: The step function processes this work item
- **Then**: The step function exits without advancing; `cogworks:awaiting-review` label is present
- **Traces to**: REQ-PIPE-006

### ASSERT-PSM-004: Safety-critical work item forces human gates

- **Given**: A work item classified as safety-affecting, with auto-proceed configured for node N (a code-producing node)
- **When**: The step function evaluates the gate for node N
- **Then**: The gate behaves as human-gated regardless of configuration
- **Traces to**: REQ-PIPE-006, REQ-CLASS-002

### ASSERT-PSM-005: Processing lock prevents concurrent processing

- **Given**: A work item with `cogworks:processing` label already applied
- **When**: A second CLI invocation attempts to process this work item
- **Then**: The second invocation backs off without taking action
- **Traces to**: REQ-PIPE-007

### ASSERT-PSM-006: Failed node reports and halts

- **Given**: A node that fails (unrecoverable error, budget exceeded, or max escalation)
- **When**: The failure is reported
- **Then**: `cogworks:node:failed` label is applied, a structured failure report is posted as an issue comment, and the step function exits
- **Traces to**: REQ-PIPE-005, REQ-AUDIT-003

### ASSERT-PSM-007: Status update posted on node entry

- **Given**: A work item where a new node is activated
- **When**: The new node begins
- **Then**: A status comment is posted on the work item issue with the node name and summary
- **Traces to**: REQ-PIPE-005

### ASSERT-PSM-008: Default linear pipeline used when no configuration exists

- **Given**: A repository with no `.cogworks/pipeline.toml` file
- **When**: The pipeline executor loads the pipeline configuration
- **Then**: The default 7-node linear pipeline (Intake → Architecture → Interface Design → Planning → Code Generation → Review → Integration) is used
- **Traces to**: REQ-GRAPH-003, REQ-PIPE-003

### ASSERT-PSM-009: Pipeline resumes from failed node on re-trigger

- **Given**: A pipeline run that failed at node N with pipeline state persisted to GitHub
- **When**: The pipeline is re-triggered through the EventSource or manual CLI
- **Then**: The pipeline reconstructs state from GitHub and resumes from node N without re-executing completed nodes
- **Traces to**: REQ-PIPE-009, REQ-EXEC-002

### ASSERT-PSM-010: Pipeline cancellation terminates active nodes

- **Given**: A pipeline running with node N active
- **When**: The step function re-reads work-item state and the configured adapter mapping reports cancellation
- **Then**: Active node execution is terminated, current state is persisted, a summary is posted, and active implementation containers are cleaned up
- **Traces to**: REQ-EXEC-005

### ASSERT-PSM-011: Duplicate pipeline prevention

- **Given**: A pipeline already running for work item #42
- **When**: A new trigger event arrives for work item #42
- **Then**: The new trigger is rejected with a comment explaining the conflict (or queued, per configuration)
- **Traces to**: REQ-PIPE-008

### ASSERT-JOB-001: Implementation job uses one ephemeral container and cleans it up

- **Given**: An implementation job is requested and reaches success, failure, or cancellation
- **When**: The Pipeline Executor coordinates the job through the runtime provider
- **Then**: Exactly one ephemeral container is requested for the job, retry attempts within that active job reuse it, and cleanup is requested on every terminal path
- **Traces to**: REQ-PIPE-012, REQ-EXEC-001, REQ-EXEC-008

### ASSERT-JOB-003: Persistence is attempted before container cleanup

- **Given**: An implementation job reaches success, failure, or cancellation and final state or deliverables are not yet known to be durable
- **When**: The orchestrator terminates the job
- **Then**: It attempts persistence before requesting cleanup; if persistence fails, cleanup is still requested and only the last successfully persisted state is used for recovery
- **Traces to**: REQ-EXEC-008, REQ-EXEC-009

### ASSERT-JOB-002: External approval does not replace a CogWorks node gate

- **Given**: An external factory/GateKeeper approval or work-item readiness status is reported through the work-item interface
- **When**: CogWorks evaluates its configured node gate
- **Then**: CogWorks applies its own gate configuration and safety override independently; the provider-specific status mapping is not inferred by the gate logic
- **Traces to**: REQ-PIPE-006, REQ-BOUND-004

---

## Task Classification

### ASSERT-CLASS-001: LLM classification produces valid structured output

- **Given**: An issue body and list of repository modules
- **When**: The classification LLM call completes
- **Then**: The output matches the classification schema (type, affected_modules, estimated_scope, safety_affecting, rationale)
- **Traces to**: REQ-CLASS-001

### ASSERT-CLASS-002: Safety override applies when registry matches

- **Given**: An LLM classification that says `safety_affecting: false`, but one affected module is in the safety-critical registry
- **When**: The classification is cross-validated
- **Then**: The final classification has `safety_affecting: true`
- **Traces to**: REQ-CLASS-002

### ASSERT-CLASS-003: Safety override does not apply when no registry match

- **Given**: An LLM classification that says `safety_affecting: false`, and no affected modules are in the safety-critical registry
- **When**: The classification is cross-validated
- **Then**: The final classification has `safety_affecting: false`
- **Traces to**: REQ-CLASS-002

### ASSERT-CLASS-004: Scope exceeding threshold triggers escalation

- **Given**: An LLM classification with estimated scope exceeding the configured threshold
- **When**: The scope is evaluated
- **Then**: An escalation result is produced (not a proceed result)
- **Traces to**: REQ-CLASS-003

---

## Architecture (Specification)

### ASSERT-ARCH-001: Valid spec references pass validation

- **Given**: A generated specification that references only modules that exist in the repository (or are explicitly marked new)
- **When**: Deterministic validation runs
- **Then**: Validation passes
- **Traces to**: REQ-ARCH-005

### ASSERT-ARCH-002: Invalid module references fail validation

- **Given**: A generated specification that references a module that does not exist and is not marked new
- **When**: Deterministic validation runs
- **Then**: Validation fails with a specific error identifying the missing module
- **Traces to**: REQ-ARCH-005

### ASSERT-ARCH-003: Constraint-violating dependency changes fail validation

- **Given**: A generated specification that proposes a dependency change violating the project's architectural constraints
- **When**: Deterministic validation runs
- **Then**: Validation fails with a specific error identifying the violated constraint
- **Traces to**: REQ-ARCH-005

### ASSERT-ARCH-004: Validation failure triggers LLM retry

- **Given**: A specification that fails validation
- **When**: The validation error is processed
- **Then**: The LLM is re-invoked with the specific error messages appended to context
- **And**: Retry count is incremented
- **Traces to**: REQ-ARCH-005, REQ-CODE-003

---

## Planning

### ASSERT-PLAN-001: Acyclic dependencies produce valid topological order

- **Given**: A set of sub-work-items with acyclic dependencies
- **When**: Dependency ordering is computed
- **Then**: A valid topological ordering is produced where every sub-work-item appears after all its dependencies
- **Traces to**: REQ-PLAN-003

### ASSERT-PLAN-002: Cyclic dependencies are rejected

- **Given**: A set of sub-work-items with a circular dependency (A → B → C → A)
- **When**: Dependency ordering is attempted
- **Then**: An error is returned identifying the cycle
- **And**: The error is fed back to the LLM for replanning
- **Traces to**: REQ-PLAN-003

### ASSERT-PLAN-003: Granularity limit enforced

- **Given**: A plan with more sub-work-items than the configured maximum
- **When**: The plan is validated
- **Then**: An escalation result is produced (not proceed)
- **Traces to**: REQ-PLAN-004

### ASSERT-PLAN-004: Interface coverage verified

- **Given**: A plan where some interface from the Interface Design node is not covered by any sub-work-item
- **When**: The plan is validated
- **Then**: Validation fails with a specific error identifying the uncovered interface
- **Traces to**: REQ-PLAN-006

### ASSERT-PLAN-005: All interfaces covered

- **Given**: A plan where every interface from the Interface Design node is covered by at least one sub-work-item
- **When**: Interface coverage is checked
- **Then**: Validation passes
- **Traces to**: REQ-PLAN-006

---

## Code Generation

### ASSERT-CODE-001: Validation failure feeds structured errors back

- **Given**: Generated artifacts that fail domain service validation
- **When**: The domain service returns structured diagnostics
- **Then**: The diagnostics (artifact, location, severity, message) are appended to context and the LLM is re-invoked
- **Traces to**: REQ-CODE-002

### ASSERT-CODE-002: Simulation failure with self-explanatory error feeds directly

- **Given**: Generated artifacts that pass validation but fail simulation, where the failure type is in the "self-explanatory" heuristic set
- **When**: The simulation result is analyzed
- **Then**: The failure output is fed directly to the LLM without additional interpretation
- **Traces to**: REQ-CODE-002

### ASSERT-CODE-003: Simulation failure with non-obvious error triggers LLM interpretation

- **Given**: Generated artifacts that fail simulation, where the failure type is NOT in the "self-explanatory" heuristic set
- **When**: The simulation result is analyzed
- **Then**: An LLM is invoked to interpret the failure, and the interpretation is included in the retry context
- **Traces to**: REQ-CODE-002

### ASSERT-CODE-004: Retry budget exhaustion triggers escalation

- **Given**: A sub-work-item that has failed its maximum number of retries
- **When**: The retry budget check runs
- **Then**: Escalation is triggered with a summary of all attempts and their failure reasons
- **Traces to**: REQ-CODE-003

### ASSERT-CODE-005: Cost budget exhaustion halts pipeline

- **Given**: A pipeline where accumulated token cost plus the next call's estimated cost exceeds the budget
- **When**: The budget check runs before the LLM call
- **Then**: The call is refused, and the pipeline halts with a cost report
- **And**: The cost report includes per-node and per-sub-work-item breakdown
- **Traces to**: REQ-CODE-004

### ASSERT-CODE-006: Context truncation follows priority order

- **Given**: A context package that exceeds the model's context window
- **When**: Truncation is applied
- **Then**: Items are removed starting from the lowest priority tier; higher-priority items are retained
- **And**: The current sub-work-item's interface definition is never removed
- **Traces to**: REQ-CODE-005

### ASSERT-CODE-007: Sub-work-item receives prior outputs

- **Given**: Sub-work-item N that depends on sub-work-items A and B (both complete)
- **When**: Context is assembled for sub-work-item N
- **Then**: The outputs (code) of sub-work-items A and B are included in the context package
- **Traces to**: REQ-CODE-001

### ASSERT-CODE-008: Test-first implementation uses domain-specific acceptance checks

- **Given**: An implementation job for a software or other engineering work item
- **When**: The job starts its implementation loop
- **Then**: Tests or equivalent domain acceptance checks are established before implementation, followed by configured validation and iteration on structured feedback
- **Traces to**: REQ-CODE-007

---

## Scenario Validation

### ASSERT-SCEN-001: Applicable scenarios are selected

- **Given**: A sub-work-item that implements interfaces I1 and I2, where scenarios S1, S2, and S3 cover I1, and scenario S4 covers I2
- **When**: Scenario validation begins
- **Then**: Scenarios S1, S2, S3, and S4 are selected for execution; other scenarios are not run
- **Traces to**: REQ-SCEN-007

### ASSERT-SCEN-002: Scenario specifications excluded from code generation context

- **Given**: Context assembly for code generation node
- **When**: The context package is built
- **Then**: No scenario specification files are included, even if they are relevant to the affected modules
- **Traces to**: REQ-SCEN-002 (holdout principle)

### ASSERT-SCEN-003: Multiple trajectories executed per scenario

- **Given**: Scenario S with configuration for 10 trajectories
- **When**: Scenario validation executes scenario S
- **Then**: Exactly 10 independent trajectory executions occur, each with fresh state
- **Traces to**: REQ-SCEN-003

### ASSERT-SCEN-004: Satisfaction score computed correctly

- **Given**: Scenario S with 10 trajectories where 9 satisfy acceptance criteria and 1 does not
- **When**: Satisfaction scoring occurs
- **Then**: The satisfaction score for scenario S is 0.9
- **Traces to**: REQ-SCEN-004

### ASSERT-SCEN-005: Threshold enforcement

- **Given**: Overall satisfaction score of 0.96 with threshold configured at 0.95
- **When**: Scenario validation decision is made
- **Then**: Validation passes
- **Traces to**: REQ-SCEN-004

### ASSERT-SCEN-006: Below-threshold score triggers remediation

- **Given**: Overall satisfaction score of 0.92 with threshold configured at 0.95
- **When**: Scenario validation decision is made
- **Then**: Validation fails, and failing scenario details are fed back to Code Generator
- **Traces to**: REQ-SCEN-006

### ASSERT-SCEN-007: Explicit failure criterion overrides score

- **Given**: Overall satisfaction score of 0.98 (above threshold), but one trajectory triggered an explicit failure criterion
- **When**: Scenario validation decision is made
- **Then**: Validation fails immediately
- **Traces to**: REQ-SCEN-004

### ASSERT-SCEN-008: No applicable scenarios skips validation

- **Given**: A sub-work-item with no applicable scenarios
- **When**: Scenario validation node is reached
- **Then**: Validation is skipped (not failed), and the pipeline proceeds to review gate
- **Traces to**: REQ-SCEN-007

### ASSERT-SCEN-009: Twin provisioning for scenarios

- **Given**: Scenario S requires Digital Twin T, and T is registered
- **When**: Scenario S is executed
- **Then**: Twin T is started before execution and stopped after execution
- **Traces to**: REQ-DTU-005

### ASSERT-SCEN-010: Scenario results in audit trail

- **Given**: Scenario validation completes
- **When**: Audit trail is written
- **Then**: Overall satisfaction score, per-scenario scores, trajectory count, and any failure details are included
- **Traces to**: REQ-SCEN-009

---

## Review Gate

### ASSERT-REVIEW-001: Blocking finding prevents PR creation

- **Given**: A review pass that produces at least one `blocking` finding
- **When**: Review results are aggregated
- **Then**: The aggregate result is `remediate` (not `proceed`)
- **And**: PR creation does not occur
- **Traces to**: REQ-REVIEW-004

### ASSERT-REVIEW-002: Non-blocking findings are preserved

- **Given**: Review passes with only `warning` and `informational` findings
- **When**: Review results are aggregated
- **Then**: The aggregate result is `proceed`
- **And**: All non-blocking findings are collected for PR comments
- **Traces to**: REQ-REVIEW-004

### ASSERT-REVIEW-003: All four review passes execute

- **Given**: Generated artifacts ready for review
- **When**: The review gate runs
- **Then**: Cross-domain constraint validation, code quality, architecture compliance, and security reviews all execute
- **And**: Results from all four are included in the aggregate
- **Traces to**: REQ-REVIEW-002, REQ-XVAL-001

### ASSERT-REVIEW-004: Blocking finding feeds back to code generation

- **Given**: A blocking review finding
- **When**: The review result is processed
- **Then**: The finding (file, line, severity, explanation) is fed back to the Code Generator for remediation
- **Traces to**: REQ-REVIEW-005

### ASSERT-REVIEW-005: Unresolved finding triggers escalation

- **Given**: A blocking finding that persists after the maximum number of remediation cycles
- **When**: The remediation cycle count is checked
- **Then**: Escalation is triggered
- **Traces to**: REQ-REVIEW-005

### ASSERT-REVIEW-006: Safety-critical PR requires human approval

- **Given**: A sub-work-item PR for a safety-affecting work item
- **When**: The PR is created
- **Then**: The system does NOT auto-approve; human approval is required before merge
- **Traces to**: REQ-REVIEW-006

---

## Idempotency

### ASSERT-IDEM-001: Duplicate PR creation is prevented

- **Given**: A specification PR already exists for work item #42
- **When**: The step function re-processes work item #42 at the architecture node
- **Then**: No duplicate PR is created; the existing PR is detected and used
- **Traces to**: REQ-PIPE-004

### ASSERT-IDEM-002: Duplicate sub-issue creation is prevented

- **Given**: Sub-work-item issues already exist for work item #42
- **When**: The step function re-processes work item #42 at the planning node
- **Then**: No duplicate issues are created; the existing issues are detected and used
- **Traces to**: REQ-PIPE-004

### ASSERT-IDEM-003: Label transitions are safe to repeat

- **Given**: Work item #42 already has `cogworks:node:architecture` label
- **When**: The step function attempts to set `cogworks:node:architecture` again
- **Then**: No error occurs; the operation is a no-op
- **Traces to**: REQ-PIPE-004

---

## Integration

### ASSERT-INT-001: PR references work item and sub-work-item

- **Given**: A sub-work-item PR being created
- **When**: The PR description is generated
- **Then**: It includes references to the sub-work-item issue, parent work item, specification PR, and interface PR
- **Traces to**: REQ-INT-002

### ASSERT-INT-002: Non-blocking findings appear as PR comments

- **Given**: A review gate that passed with warning and informational findings
- **When**: The PR is created
- **Then**: Each non-blocking finding is posted as an inline review comment at the relevant file and line
- **Traces to**: REQ-REVIEW-004, REQ-INT-001

### ASSERT-INT-003: Code PRs are never merged by CogWorks

- **Given**: A sub-work-item PR that has been created and approved
- **When**: CogWorks processes the work item
- **Then**: CogWorks does NOT merge the PR
- **Traces to**: REQ-BOUND-002

### ASSERT-INT-004: External approval and release actions remain outside CogWorks

- **Given**: A work item reports external approval or a PR becomes eligible for merge
- **When**: CogWorks records or reports the pipeline outcome
- **Then**: CogWorks passes status through the work-item interface but does not approve, merge, release, or publish artifacts
- **Traces to**: REQ-BOUND-002, REQ-BOUND-004

---

## Schema Validation

### ASSERT-SCHEMA-001: Invalid LLM output triggers retry

- **Given**: An LLM response that does not match the expected output schema
- **When**: Schema validation runs
- **Then**: The response is rejected, the validation error is appended to the prompt context, and the LLM is re-invoked
- **Traces to**: Design philosophy (Structured I/O at every boundary)

### ASSERT-SCHEMA-002: Valid LLM output proceeds

- **Given**: An LLM response that matches the expected output schema
- **When**: Schema validation runs
- **Then**: The validated, typed data is returned to the caller
- **Traces to**: Design philosophy (Structured I/O at every boundary)

---

## Cross-Domain Constraint Validation

### ASSERT-XVAL-001: Hard constraint violation blocks review

- **Given**: Generated artifacts where an extracted value (e.g., CAN bus load 0.55) exceeds the interface contract's maximum (0.50)
- **When**: Cross-domain constraint validation runs
- **Then**: A blocking finding is produced with the interface ID, parameter, expected range, actual value, and violating domain
- **Traces to**: REQ-XVAL-003

### ASSERT-XVAL-002: Nominal deviation produces warning

- **Given**: Generated artifacts where an extracted value deviates from nominal but stays within min/max bounds
- **When**: Cross-domain constraint validation runs
- **Then**: A warning finding is produced (not blocking)
- **Traces to**: REQ-XVAL-003

### ASSERT-XVAL-003: Constraint validation runs before LLM reviews

- **Given**: Generated artifacts ready for review
- **When**: The review gate begins
- **Then**: Cross-domain constraint validation executes first (deterministic, no tokens), before any LLM review pass
- **Traces to**: REQ-XVAL-001

### ASSERT-XVAL-004: Architecture node checks cross-domain constraints

- **Given**: A proposed architecture that would push CAN bus load over the declared maximum
- **When**: Architecture node validation runs
- **Then**: The constraint violation is caught before implementation begins
- **Traces to**: REQ-XVAL-005

### ASSERT-XVAL-005: Validation works with single domain service

- **Given**: Only the firmware domain service is registered, but the interface registry defines contracts between firmware and electrical
- **When**: Cross-domain constraint validation runs for firmware artifacts
- **Then**: Firmware artifacts are validated against the registry contracts without requiring the electrical domain service
- **Traces to**: REQ-XVAL-004

### ASSERT-XVAL-006: Computed constraints evaluated correctly

- **Given**: An interface contract defining max bus load of 0.50, and generated artifacts containing three CAN messages with known sizes and cycle times
- **When**: The constraint validator computes total bus load from message parameters
- **Then**: The computed bus load is compared against the max_bus_load contract parameter
- **Traces to**: REQ-XDOM-003

---

## Interface Registry

### ASSERT-XDOM-001: Registry validates on every pipeline run

- **Given**: A pipeline run starts
- **When**: Registry validation runs (before any node)
- **Then**: All interface definitions are checked against schema, cross-references validated, conflicts detected
- **Traces to**: REQ-XDOM-005

### ASSERT-XDOM-002: Invalid interface definition halts pipeline

- **Given**: An interface definition that doesn't conform to the schema
- **When**: Registry validation runs
- **Then**: Validation fails with a clear error identifying the malformed definition
- **And**: No pipeline nodes execute
- **Traces to**: REQ-XDOM-005

### ASSERT-XDOM-003: Version mismatch detected

- **Given**: A domain service declaring compatibility with interface v2, but the registry has v3
- **When**: Registry validation runs
- **Then**: A blocking finding is produced identifying the version mismatch
- **Traces to**: REQ-XDOM-004

---

## Extension API

### ASSERT-EXT-001: Domain service unavailability for primary domain halts pipeline

- **Given**: The primary domain service (covering the artifacts being generated) is registered but unavailable
- **When**: CogWorks attempts to invoke a domain service method
- **Then**: The pipeline halts with a clear error identifying the unavailable service
- **Traces to**: REQ-EXT-007

### ASSERT-EXT-002: Domain service unavailability for secondary domain continues with warning

- **Given**: A secondary domain service (would only participate in cross-domain validation) is registered but unavailable
- **When**: CogWorks processes a sub-work-item
- **Then**: The pipeline continues, but cross-domain validation for that domain is skipped
- **And**: A warning is reported in the PR and audit trail
- **Traces to**: REQ-EXT-007

### ASSERT-EXT-003: API version incompatibility rejected

- **Given**: A domain service implementing API v1, but CogWorks requires v2
- **When**: Health check / handshake occurs
- **Then**: The domain service is rejected with a clear version mismatch error
- **Traces to**: REQ-EXT-009

### ASSERT-EXT-004: Unsupported method produces clear error

- **Given**: A domain service that supports `validate` and `extract_interfaces` but not `normalise`
- **When**: CogWorks invokes `normalise` on that service
- **Then**: A clear error is returned indicating the method is not supported
- **And**: This is treated as a non-retryable error
- **Traces to**: REQ-EXT-002

---

## Context Pack System

### ASSERT-CPACK-001: Matched pack is always loaded

- **Given**: A work item whose classification labels match Context Pack P's trigger definition
- **When**: Context Pack selection runs at the Architecture node
- **Then**: Pack P is loaded (domain knowledge, safe patterns, anti-patterns, required artefacts)
- **And**: The loaded pack is recorded in the audit trail
- **Traces to**: REQ-CPACK-001, REQ-CPACK-005, REQ-CPACK-007

### ASSERT-CPACK-002: Unmatched pack is not loaded

- **Given**: A work item whose classification does not match Context Pack P's trigger definition
- **When**: Context Pack selection runs
- **Then**: Pack P is not loaded
- **And**: Pack P's content does not appear in any subsequent LLM context
- **Traces to**: REQ-CPACK-001

### ASSERT-CPACK-003: Multiple packs loaded simultaneously

- **Given**: A work item whose classification matches packs P1 and P2
- **When**: Context Pack selection runs
- **Then**: Both P1 and P2 are loaded and their combined content is incorporated into context assembly
- **Traces to**: REQ-CPACK-003

### ASSERT-CPACK-004: Conflicting guidance uses the more restrictive rule

- **Given**: Pack P1 sets satisfaction threshold at 0.97 and pack P2 sets it at 0.99 for overlapping domain interfaces
- **When**: Scenario validation runs for those interfaces
- **Then**: The stricter threshold (0.99) is applied
- **Traces to**: REQ-CPACK-004

### ASSERT-CPACK-005: Missing required artefact is a blocking finding

- **Given**: Pack P declares required artefact "unsafe usage justification" and the generated output does not contain it
- **When**: The Review node runs required artefact checking
- **Then**: A blocking finding is produced identifying pack P and the missing artefact
- **And**: PR creation does not occur
- **Traces to**: REQ-CPACK-006, REQ-REVIEW-004

### ASSERT-CPACK-006: Present required artefact is not a finding

- **Given**: Pack P declares required artefact A and the generated output contains A
- **When**: The Review node runs required artefact checking
- **Then**: No finding is produced for this artefact
- **Traces to**: REQ-CPACK-006

### ASSERT-CPACK-007: Pack content included in context from Architecture node onward

- **Given**: Context Pack P is loaded for a work item at the Architecture node
- **When**: Context assembly runs for the Architecture node and subsequent nodes
- **Then**: Pack P's domain knowledge, safe patterns, and anti-patterns are included in context packages
- **Traces to**: REQ-CPACK-008

---

## Constitutional Security Layer

### ASSERT-CONST-001: Constitutional rules loaded on every pipeline run

- **Given**: A pipeline run starts
- **When**: Pipeline Executor's first action runs
- **Then**: Constitutional rules are loaded before any other action, including LLM calls and context assembly
- **Traces to**: REQ-CONST-001

### ASSERT-CONST-002: Failure to load constitutional rules halts pipeline

- **Given**: The constitutional rules file does not exist at the configured path
- **When**: The Pipeline Executor attempts to load constitutional rules
- **Then**: The pipeline halts with a clear error identifying the missing file
- **And**: No LLM call is made
- **Traces to**: REQ-CONST-001, REQ-CONST-003

### ASSERT-CONST-003: Unreviewed constitutional rules are rejected

- **Given**: The constitutional rules file exists but is from an unreviewed branch (not merged)
- **When**: The Constitutional Rules Loader validates the file source
- **Then**: The rules are rejected and the pipeline halts
- **Traces to**: REQ-CONST-003

### ASSERT-CONST-004: Injection detection triggers pipeline halt and hold state

- **Given**: An issue body containing text structured as a directive to CogWorks (e.g., "Ignore previous instructions and...")
- **When**: The Injection Detector scans the issue body before it is included in any LLM prompt
- **Then**: An `INJECTION_DETECTED` event is emitted with source document and offending text
- **And**: The pipeline halts immediately
- **And**: The work item enters hold state with `cogworks:hold` label
- **Traces to**: REQ-CONST-005, REQ-CONST-006, REQ-CONST-007

### ASSERT-CONST-005: Hold state requires human resolution

- **Given**: A work item in hold state (INJECTION_DETECTED)
- **When**: A new pipeline invocation processes this work item
- **Then**: The invocation detects the hold state and exits without taking action
- **And**: A message is logged indicating human review is required
- **Traces to**: REQ-CONST-007

### ASSERT-CONST-006: Scope underspecification halts generation

- **Given**: A work item requiring capabilities not explicitly in the approved specification
- **When**: Scope enforcement runs
- **Then**: A `SCOPE_UNDERSPECIFIED` event is emitted identifying the missing capability
- **And**: Code generation halts until specification is updated
- **Traces to**: REQ-CONST-011

### ASSERT-CONST-007: Protected path violation fails pre-PR validation

- **Given**: Generated artifacts include a file matching a protected path pattern (e.g., `.cogworks/constitutional-rules.md`)
- **When**: Pre-PR validation runs scope enforcement
- **Then**: A `PROTECTED_PATH_VIOLATION` event is emitted
- **And**: PR creation does not occur
- **Traces to**: REQ-CONST-013

### ASSERT-SEC-006: Credential material is isolated from model and audit content

- **Given**: Infrastructure credentials are resolved by the Vault-backed credential provider
- **When**: CogWorks assembles model prompts, context, audit records, logs, or generated artifacts
- **Then**: Credential material is absent from all of those outputs and is used only by the infrastructure authentication path
- **Traces to**: REQ-BOUND-005

### ASSERT-SEC-007: Required attestations are verified and CogWorks actions are signed

- **Given**: A work item has a required upstream attestation, or CogWorks is completing an action that requires its own attestation
- **When**: The attestation provider verifies the upstream content or signs the CogWorks action
- **Then**: Processing proceeds only after successful verification, and CogWorks publishes its own signed attestation through the work-item interface; missing, invalid, or mismatched required attestations prevent processing without exposing key material
- **Traces to**: REQ-BOUND-006

---

## Graph Execution

### ASSERT-GRAPH-001: Edge conditions determine downstream activation

- **Given**: Node A completes with output O, and edges A→B (condition: `O.status == "pass"`) and A→C (condition: `O.status == "fail"`)
- **When**: Edge condition evaluation runs
- **Then**: Only the edge whose condition is satisfied activates its target node
- **And**: The non-matching edge's target node remains pending
- **Traces to**: REQ-EDGE-001, REQ-PIPE-003

### ASSERT-GRAPH-002: Fan-out activates multiple parallel nodes

- **Given**: Node A completes with edges to B and C (both unconditional, `all-matching` mode)
- **When**: Edge condition evaluation runs
- **Then**: Both B and C are activated and may execute concurrently
- **And**: Both B and C have `cogworks:node:<name>` labels applied
- **Traces to**: REQ-GRAPH-001, REQ-EXEC-006

### ASSERT-GRAPH-003: Fan-in waits for all upstream nodes

- **Given**: Node D declares inputs from nodes B and C; node B has completed but C has not
- **When**: The step function evaluates D's readiness
- **Then**: Node D remains pending (not activated)
- **And**: When C later completes, the next step function invocation activates D
- **Traces to**: REQ-EXEC-006

### ASSERT-GRAPH-004: Rework edge tracks traversal count

- **Given**: A rework edge from Review→CodeGen with max_traversals: 3, and 2 prior traversals
- **When**: Review fails and the rework edge condition is true
- **Then**: CodeGen is re-activated (traversal count becomes 3)
- **And**: On the next Review failure, the rework edge is NOT taken (max reached) and the overflow action (escalate) is triggered
- **Traces to**: REQ-EDGE-003

### ASSERT-GRAPH-005: Cycle without termination condition is rejected at configuration load

- **Given**: A pipeline configuration with node A→B→A (cycle) but no `max_traversals` on any edge in the cycle
- **When**: The Pipeline Configuration Manager validates the graph
- **Then**: Validation fails with an error identifying the unterminated cycle
- **And**: The pipeline does not start
- **Traces to**: REQ-GRAPH-001, REQ-GRAPH-004

### ASSERT-GRAPH-006: LLM-evaluated edge condition falls back on LLM failure

- **Given**: An LLM-evaluated edge condition with fallback `edge_taken: false`
- **When**: The LLM call fails (timeout, API error, etc.)
- **Then**: The fallback is applied (edge not taken)
- **And**: The fallback application is recorded in the audit trail
- **Traces to**: REQ-EDGE-001

### ASSERT-GRAPH-007: Parallel execution respects max concurrent LLM calls

- **Given**: Nodes B, C, D, and E are all eligible for concurrent execution (all are LLM nodes), and max concurrent LLM calls is configured as 3
- **When**: The Graph Execution Engine schedules node execution
- **Then**: At most 3 nodes execute LLM calls simultaneously; the 4th waits until one completes
- **Traces to**: REQ-EXEC-006

### ASSERT-GRAPH-008: Parallel cost budget is atomic

- **Given**: Nodes B and C executing in parallel, pipeline cost budget remaining is 100 tokens, B estimates 80 tokens, C estimates 80 tokens
- **When**: Both nodes attempt to acquire budget simultaneously
- **Then**: Only one node's LLM call is approved; the other is denied (budget exceeded)
- **And**: The denied node receives a budget exceeded error
- **Traces to**: REQ-EXEC-006, REQ-CODE-004

### ASSERT-GRAPH-009: Pipeline state written to GitHub at each node boundary

- **Given**: Node A completes successfully
- **When**: The pipeline executor processes node A's completion
- **Then**: The full pipeline state (active/completed/pending/failed nodes, traversal counts, cumulative cost) is written as a JSON document to a GitHub comment
- **Traces to**: REQ-EXEC-002

### ASSERT-GRAPH-010: Spawning node is non-blocking

- **Given**: A spawning node S in the pipeline graph with a downstream edge to node T
- **When**: Spawning node S completes (whether issues were created or not)
- **Then**: Downstream node T is activated; spawning node completion does not depend on created issue outcomes
- **Traces to**: REQ-NODE-004

### ASSERT-GRAPH-011: Pipeline configuration selects named pipeline from classification

- **Given**: Pipeline configuration defines pipelines named "standard" and "hotfix", and intake classification outputs `pipeline: "hotfix"`
- **When**: The Pipeline Configuration Manager selects the pipeline
- **Then**: The "hotfix" pipeline graph is used for the remainder of the run
- **Traces to**: REQ-GRAPH-004

### ASSERT-GRAPH-012: Partial parallel failure continues other nodes

- **Given**: Nodes B and C executing in parallel; node B fails with `abort_siblings_on_failure: false`
- **When**: Node B reports failure
- **Then**: Node C continues executing; node B enters `failed` state
- **And**: Downstream nodes that depend only on C can still proceed
- **Traces to**: REQ-EXEC-006

### ASSERT-GRAPH-013: Abort-siblings parallel failure stops sibling nodes

- **Given**: Nodes B and C executing in parallel; node B fails with `abort_siblings_on_failure: true`
- **When**: Node B reports failure
- **Then**: Node C is terminated (in-progress LLM calls allowed to complete)
- **And**: The pipeline enters an error state for the failed sub-graph
- **Traces to**: REQ-EXEC-006

### ASSERT-GRAPH-014: Dead-end node triggers pipeline halt with clear error

- **Given**: Node A completes successfully
- **When**: All outgoing edges from A have conditions that evaluate to false
- **Then**: No downstream nodes are activated
- **And**: The pipeline posts a warning comment identifying A as a dead-end node
- **And**: The pipeline halts with a structured error (not silently stalls)
- **Traces to**: REQ-EXEC-003, EDGE-064

### ASSERT-GRAPH-015: Graph traversal state is fully restored on resume

- **Given**: A pipeline run is interrupted mid-graph and has persisted its state to GitHub
- **When**: The pipeline is reinvoked for the same work item
- **Then**: The orchestrator reads the persisted pipeline state JSON from the GitHub issue
- **And**: Per-node traversal counts (rework cycle counts) are restored
- **And**: Fan-out completion state is restored (which upstream nodes have finished)
- **And**: Execution resumes from the failed or incomplete node, not from the start
- **Traces to**: REQ-EXEC-004, ASSERT-PSM-009

---

## Context Assembly

### ASSERT-CTX-001: Reference exemplar files loaded from external repository

- **Given**: The architecture specification declares reference exemplar files from an external repository
- **When**: Context assembly prepares the context package for code generation
- **Then**: The specified exemplar files are fetched from the external repository
- **And**: Exemplar files are included in the context package at the appropriate pyramid summary level (Level 2 for distant references, Level 3 for closely related references)
- **And**: Exemplar files are read-only context — CogWorks does not modify files in referenced repositories
- **Traces to**: REQ-CTX-001

---

## Tracing-Based Observability

### ASSERT-OBS-001: Node lifecycle hooks record stable fields

- **Given**: A tracing span declares the fields required by the observability hook
- **When**: `record_node_start` and `record_node_complete` are invoked
- **Then**: The span records `pipeline.run_id`, `pipeline.node_id`, and the corresponding `pipeline.event`; completion also records `pipeline.token_cost.total_usd`
- **Traces to**: REQ-AUDIT-004, REQ-AUDIT-005

### ASSERT-OBS-002: Retry and rework categories are structured

- **Given**: A retry or rework occurs with a typed root cause or misalignment category
- **When**: The corresponding tracing hook is invoked
- **Then**: The span records the stable event kind and its structured category field
- **Traces to**: REQ-AUDIT-004, REQ-AUDIT-005

### ASSERT-OBS-003: Export uses the configured tracing path

- **Given**: The CLI initializes observability
- **When**: Pipeline spans and events are emitted
- **Then**: They flow through structured console logging and the configured OpenTelemetry exporter without a separate metric-sink abstraction
- **Traces to**: REQ-AUDIT-006

---

## Alignment Verification Assertions

### ASSERT-ALIGN-001: Deterministic check detects missing output element

- **Given**: An architecture specification requires three components (A, B, C)
- **When**: The interface design node produces interfaces for only A and B
- **Then**: The deterministic alignment check produces a finding with type `missing`, severity `blocking`, and a reference to component C
- **And**: The alignment check fails regardless of the alignment score
- **Traces to**: REQ-ALIGN-001, REQ-ALIGN-002

### ASSERT-ALIGN-002: LLM check detects semantic drift

- **Given**: A work item requests a retry mechanism with exponential backoff
- **When**: The code generation node produces code with a fixed-delay retry
- **Then**: The LLM alignment check produces a finding with type `modified`, describing the substitution of fixed delay for exponential backoff
- **And**: The finding includes references to both the input requirement and the output code
- **Traces to**: REQ-ALIGN-001, REQ-ALIGN-004

### ASSERT-ALIGN-003: Alignment failure triggers rework with findings in context

- **Given**: An alignment check fails with two blocking findings
- **When**: The pipeline executor processes the alignment failure
- **Then**: The node is re-entered with rework context that includes the two specific misalignment findings
- **And**: The rework counter is incremented (not the retry counter)
- **And**: The alignment findings appear in the audit trail
- **Traces to**: REQ-ALIGN-005, REQ-ALIGN-021

### ASSERT-ALIGN-004: Rework budget exhaustion escalates to pipeline failure

- **Given**: A node has exhausted its rework budget (e.g., 3 rework cycles completed)
- **When**: The alignment check fails again after the final rework attempt
- **Then**: The pipeline fails with a structured error including the alignment findings from the final attempt
- **And**: The error distinguishes rework exhaustion from retry exhaustion
- **Traces to**: REQ-ALIGN-006

### ASSERT-ALIGN-005: Safety-classified items use stricter alignment thresholds

- **Given**: A work item that is classified as safety-critical
- **When**: The alignment verifier loads the alignment configuration for a node
- **Then**: The threshold is at least 0.95 (cannot be configured lower)
- **And**: The LLM alignment check is enabled (cannot be disabled)
- **Traces to**: REQ-ALIGN-041

### ASSERT-ALIGN-006: Traceability matrix is built incrementally

- **Given**: A pipeline with nodes A → B → C, each with alignment checks
- **When**: Node B’s alignment check completes
- **Then**: The traceability matrix contains entries from both node A and node B
- **And**: Requirements not addressed at node B are marked N/A with a reason
- **Traces to**: REQ-ALIGN-030, REQ-ALIGN-031

### ASSERT-ALIGN-007: Traceability matrix published on pipeline completion

- **Given**: A pipeline completes successfully with alignment checks at each stage
- **When**: The pipeline executor processes pipeline completion
- **Then**: The traceability matrix is posted as a comment on the work item issue
- **And**: The matrix is included in the audit trail
- **And**: For safety-classified work items, the matrix is flagged as requiring human sign-off
- **Traces to**: REQ-ALIGN-030, REQ-ALIGN-031

### ASSERT-ALIGN-008: End-to-end alignment check catches accumulated drift

- **Given**: A pipeline where each stage passes its per-stage alignment check, but the final output has drifted from the original work item intent through accumulated small changes
- **When**: The end-to-end alignment check runs after pipeline completion
- **Then**: The check detects the accumulated drift and produces findings referencing the original work item
- **And**: The pipeline disposition reflects the end-to-end alignment failure
- **Traces to**: REQ-ALIGN-015

### ASSERT-ALIGN-009: Retry and rework counters are independent

- **Given**: A node that has retried twice (technical failures) and reworked once (alignment failure)
- **When**: The pipeline executor evaluates budget limits
- **Then**: The retry counter shows 2 and the rework counter shows 1
- **And**: Each counter is compared only against its own budget
- **And**: Metrics emit retry and rework counts separately with their respective root causes
- **Traces to**: REQ-ALIGN-005, REQ-ALIGN-006

### ASSERT-ALIGN-010: Deterministic check results included in LLM check context

- **Given**: A node output that has a deterministic alignment check finding (e.g., missing function signature)
- **When**: The LLM alignment check is invoked
- **Then**: The deterministic check findings are included in the LLM prompt context
- **And**: The LLM check focuses on semantic alignment issues not already caught by deterministic checks
- **Traces to**: REQ-ALIGN-004

### ASSERT-ALIGN-011: Alignment score and blocking findings both required to pass

- **Given**: An alignment check where the LLM produces a score of 0.95 (above threshold) but includes one finding with severity `blocking`
- **When**: The alignment verifier evaluates the result
- **Then**: The alignment check fails (blocking finding overrides score)
- **And**: The finding is included in the rework context
- **Traces to**: REQ-ALIGN-002, REQ-ALIGN-003

### ASSERT-ALIGN-012: Alignment check uses different LLM model from generator

- **Given**: A node configured with model X for code generation
- **When**: The alignment verifier invokes the LLM alignment check
- **Then**: The LLM call uses a model different from model X
- **Or**: If only one model is available, a warning is logged and the check proceeds with the same model
- **Traces to**: REQ-ALIGN-004, CW-R02

---

## Tool Scoping and Enforcement Assertions

### ASSERT-TOOL-001: Unscoped tools are invisible to LLM

- **Given**: A pipeline node with a tool profile that includes `fs.read`, `fs.list`, and `search.code` but NOT `fs.write`
- **When**: The LLM call is constructed for this node
- **Then**: The tool list in the LLM request contains only `fs.read`, `fs.list`, and `search.code`
- **And**: `fs.write` does not appear in any form (not in tool list, not in descriptions, not referenced)
- **Traces to**: REQ-TOOL-002, REQ-ENFORCE-001

### ASSERT-TOOL-002: Scope enforcement rejects out-of-scope path

- **Given**: A Code Generation node with `allowed_write_paths` set to `["src/modules/auth/**"]`
- **When**: The LLM invokes `fs.write` with path `src/modules/payment/handler.rs`
- **Then**: The tool rejects the invocation with a structured error explaining the path constraint
- **And**: A `SCOPE_VIOLATION` event is recorded in the audit trail
- **And**: The file is NOT modified
- **Traces to**: REQ-TOOL-011, REQ-ENFORCE-002, REQ-TOOL-AUDIT-002

### ASSERT-TOOL-003: Protected path enforcement at tool level

- **Given**: A Code Generation node whose `allowed_write_paths` glob pattern would technically include `.cogworks/constitutional-rules.md`
- **When**: The LLM invokes `fs.write` targeting `.cogworks/constitutional-rules.md`
- **Then**: The tool rejects the invocation regardless of the profile's path configuration
- **And**: A `PROTECTED_PATH_VIOLATION` event is recorded in the audit trail
- **Traces to**: REQ-TOOL-011, THREAT-017

### ASSERT-TOOL-004: Tool profile resolution with template variables

- **Given**: A tool profile with `allowed_write_paths = ["src/{{affected_modules}}/**"]` and pipeline state containing `affected_modules = "inventory"`
- **When**: The profile is resolved at node activation time
- **Then**: The effective `allowed_write_paths` is `["src/inventory/**"]`
- **And**: The resolved profile is recorded in the audit trail
- **Traces to**: REQ-PROFILE-002

### ASSERT-TOOL-005: Unresolvable template variable prevents node activation

- **Given**: A tool profile with `allowed_write_paths = ["src/{{affected_modules}}/**"]` but pipeline state does NOT contain `affected_modules`
- **When**: Profile resolution is attempted at node activation time
- **Then**: Node activation fails with a structured error identifying the unresolvable variable
- **And**: No LLM call is made for this node
- **Traces to**: REQ-PROFILE-002

### ASSERT-TOOL-006: Default profiles applied when no configuration exists

- **Given**: A repository with no `.cogworks/pipeline.toml` file (or a file without `[tool_profiles]`)
- **When**: The pipeline executes
- **Then**: Each core node uses its built-in default tool profile
- **And**: Code Generation has write access; all other nodes are read-only
- **Traces to**: REQ-PROFILE-003

### ASSERT-TOOL-007: Tool invocation recorded in audit trail

- **Given**: An LLM node that invokes `fs.read` and `domain.validate` during execution
- **When**: The node completes
- **Then**: The audit trail contains entries for both invocations with: tool name, parameters, scope parameters, result, and duration
- **Traces to**: REQ-TOOL-AUDIT-001

### ASSERT-TOOL-008: Scope violation threshold triggers warning

- **Given**: A node with scope violation threshold of 5
- **When**: The LLM makes 5 tool invocations that are scope violations within a single node execution
- **Then**: An audit warning is emitted after the 5th violation
- **And**: The warning includes the violation count and calling node identity
- **Traces to**: REQ-ENFORCE-002

### ASSERT-TOOL-009: Tool usage report in pipeline summary

- **Given**: A completed pipeline run where Code Generation invoked `fs.write` 12 times, `domain.validate` 3 times, `skill.run` 2 times, and had 1 scope violation
- **When**: The pipeline completion summary is generated
- **Then**: The tool usage report shows per-tool counts, 1 scope violation, and invocation type breakdown (12 raw + 3 raw + 2 skill)
- **Traces to**: REQ-TOOL-AUDIT-003

---

## Skill Assertions

### ASSERT-SKILL-001: Skill scope enforcement

- **Given**: A skill that internally calls `fs.write` to a path outside the calling node's `allowed_write_paths`
- **When**: `skill.run` is invoked by the LLM
- **Then**: The skill execution fails with a structured error identifying the violating tool call
- **And**: The scope violation is recorded in the audit trail
- **And**: No files are modified by the violating call
- **Traces to**: REQ-SKILL-004

### ASSERT-SKILL-002: Only active skills are invocable

- **Given**: Four skills — one Active, one Proposed, one Reviewed, one Retired
- **When**: The LLM calls `skill.list`
- **Then**: Only the Active skill appears in the list
- **And**: Attempting `skill.run` on the Proposed, Reviewed, or Retired skill returns a structured error stating the skill is not available
- **And**: The error does NOT reveal whether the skill exists (to prevent information leakage about unapproved skills)
- **Traces to**: REQ-SKILL-005, THREAT-020

### ASSERT-SKILL-003: Deprecated skill invocation produces warning

- **Given**: A skill with lifecycle state `Deprecated` and a specified alternative skill
- **When**: The LLM invokes `skill.run` on the deprecated skill
- **Then**: The skill executes but a warning is returned to the LLM referencing the preferred alternative
- **And**: The deprecation warning is recorded in the audit trail
- **Traces to**: REQ-SKILL-005

### ASSERT-SKILL-004: Skill auto-deprecation on low success rate

- **Given**: A skill with success rate tracking. The skill has failed 3 of its last 20 invocations (85% success rate, below the default 90% threshold).
- **When**: The orchestrator evaluates skill health
- **Then**: The skill's lifecycle state changes to Deprecated with reason `success_rate_below_threshold`
- **And**: The state change is recorded in the audit trail
- **Traces to**: REQ-SKILL-006

### ASSERT-SKILL-005: Skill parameter validation

- **Given**: A skill that declares parameters `{module: string, target: string required}` and the LLM invokes `skill.run` without providing `target`
- **When**: The skill executor validates parameters
- **Then**: Execution is rejected with a structured error identifying the missing required parameter
- **And**: No tool calls from the skill are executed
- **Traces to**: REQ-SKILL-001

### ASSERT-SKILL-006: Proposed-to-Active transition requires review

- **Given**: A skill in Proposed state
- **When**: An operator attempts to activate it directly (skipping the Reviewed state)
- **Then**: The activation is rejected with a structured error indicating review is required
- **And**: The skill remains in Proposed state
- **Traces to**: REQ-SKILL-005, THREAT-020

### ASSERT-SKILL-007: Reviewed skill can be rejected or abandoned

- **Given**: A skill in Reviewed state
- **When**: An operator invokes `cogworks skill rework` (reject/rework) or `cogworks skill retire` (abandon)
- **Then**: The skill transitions to Proposed (rework) or Retired (abandon) respectively
- **And**: The state change is recorded in the audit trail with the operator's reason
- **Traces to**: REQ-SKILL-005

---

## Progressive Discovery Assertions

### ASSERT-DISC-001: Progressive discovery activates at or above threshold

- **Given**: A node with 15 tools in its resolved profile (at the default threshold of 15)
- **When**: The LLM call is prepared
- **Then**: The tool list contains the compact index (name + one-line description) instead of full schemas
- **And**: Meta-tools `tools.search`, `tools.schema`, `tools.call` are available
- **Traces to**: REQ-DISC-001, REQ-DISC-002

### ASSERT-DISC-002: Progressive discovery not active below threshold

- **Given**: A node with 10 tools in its resolved profile (below the default threshold of 15)
- **When**: The LLM call is prepared
- **Then**: Full tool schemas are included in the tool list as normal
- **And**: Meta-tools are NOT added
- **Traces to**: REQ-DISC-001

### ASSERT-DISC-003: Skills rank above raw tools in search

- **Given**: A progressive discovery search query "validate code" that matches both a skill named "validate-rust-module" and a raw tool `domain.validate`
- **When**: `tools.search` returns results
- **Then**: The skill appears before the raw tool in the result list
- **Traces to**: REQ-DISC-004

### ASSERT-DISC-004: tools.call enforces scope identically to direct invocation

- **Given**: A Code Generation node with `allowed_write_paths` set to `["src/modules/auth/**"]` and progressive discovery active
- **When**: The LLM invokes `tools.call` with tool name `fs.write` and path `src/modules/payment/handler.rs`
- **Then**: The scope enforcer rejects the invocation with the same structured error as a direct `fs.write` call would produce
- **And**: A `SCOPE_VIOLATION` event is recorded in the audit trail
- **And**: The file is NOT modified
- **Traces to**: REQ-DISC-002, REQ-ENFORCE-002

---

## Adapter Generation Assertions

### ASSERT-ADAPT-001: OpenAPI adapter generation produces valid tool definitions

- **Given**: An OpenAPI 3.1 specification with 5 endpoints
- **When**: The adapter generator CLI command is run
- **Then**: 5 TOML tool definition files are generated in `.cogworks/adapters/<name>/`
- **And**: Each file conforms to the tool definition schema
- **And**: Tool names are namespaced (e.g., `inventree.stock.list`)
- **Traces to**: REQ-ADAPT-001, REQ-ADAPT-002

### ASSERT-ADAPT-002: Adapter tools participate in scope enforcement

- **Given**: An adapter-generated tool `inventree.stock.list` registered in the tool registry, and a node profile that includes this tool with scope parameter `allowed_urls = ["https://inventory.example.com"]`
- **When**: The LLM invokes `inventree.stock.list` targeting a different URL
- **Then**: The scope enforcer rejects the invocation
- **And**: A `SCOPE_VIOLATION` event is recorded
- **Traces to**: REQ-ADAPT-003, REQ-ENFORCE-002

### ASSERT-ADAPT-003: Drift detection identifies removed endpoint

- **Given**: A generated adapter set based on a specification with 5 endpoints, but the source specification has been updated to remove one endpoint
- **When**: The drift detection CLI command is run
- **Then**: The report identifies the removed endpoint and the adapter tool definition that no longer has a source
- **Traces to**: REQ-ADAPT-005

---

## Per-Category Tool Assertions

### ASSERT-TOOL-010: Filesystem read respects read scope

- **Given**: A node with `allowed_read_paths = ["src/**"]` and `denied_read_paths = ["src/secrets/**"]`
- **When**: The LLM invokes `fs.read` targeting `src/secrets/config.toml`
- **Then**: The tool rejects the read with a structured error (denied path takes precedence)
- **And**: A `SCOPE_VIOLATION` event is recorded
- **Traces to**: REQ-TOOL-010, REQ-TOOL-013

### ASSERT-TOOL-020: Git commit respects branch pattern

- **Given**: A Code Generation node with `branch_pattern = "cogworks/swi-.*"`
- **When**: The LLM invokes `git.commit` on branch `main`
- **Then**: The commit is rejected with a structured error explaining the branch constraint
- **And**: No commit is created
- **And**: A `SCOPE_VIOLATION` event is recorded
- **Traces to**: REQ-TOOL-020, REQ-ENFORCE-002

### ASSERT-TOOL-030: Domain service tool respects service scope

- **Given**: A node with `allowed_services = ["rust"]`
- **When**: The LLM invokes `domain.validate` specifying service `kicad`
- **Then**: The invocation is rejected with a structured error identifying the service scope violation
- **And**: A `SCOPE_VIOLATION` event is recorded
- **Traces to**: REQ-TOOL-030, REQ-ENFORCE-002

### ASSERT-TOOL-040: Shell tool restricts to allowed commands

- **Given**: A node with `allowed_commands = ["cargo build", "cargo test"]`
- **When**: The LLM invokes `shell.run_restricted` with command `rm -rf /`
- **Then**: The command is rejected with a structured error (command not in allowlist)
- **And**: The command is NOT executed
- **And**: A `SCOPE_VIOLATION` event is recorded
- **Traces to**: REQ-TOOL-040, REQ-ENFORCE-002

### ASSERT-TOOL-050: Network tool restricts to allowed URLs

- **Given**: A node with `net.fetch` enabled and `allowed_urls = ["https://docs.example.com"]`
- **When**: The LLM invokes `net.fetch` targeting `https://evil.example.com/exfiltrate`
- **Then**: The fetch is rejected with a structured error (URL not in allowlist)
- **And**: No network request is made
- **And**: A `SCOPE_VIOLATION` event is recorded
- **Traces to**: REQ-TOOL-051, REQ-ENFORCE-002

---

## LLM Rate Limiting

### ASSERT-LLM-001: Rate limit headers are respected

- **Given**: An LLM API response includes rate limit headers indicating 5 remaining requests and a reset time 30 seconds in the future
- **When**: The LLM Gateway processes the response
- **Then**: The rate limit state is updated with the remaining count and reset time
- **And**: Subsequent requests are paced so that the remaining budget is not exhausted before the reset time
- **Traces to**: REQ-LLM-001

### ASSERT-LLM-002: Proactive throttling delays requests before 429

- **Given**: The LLM Gateway rate limit state shows 2 remaining requests with a reset time 60 seconds in the future
- **When**: A pipeline node requests an LLM call
- **Then**: The request is delayed (throttled) to spread remaining capacity across the window
- **And**: No HTTP 429 response is received
- **Traces to**: REQ-LLM-001, REQ-LLM-002

### ASSERT-LLM-003: Reactive backoff on 429 response

- **Given**: The LLM API returns an HTTP 429 response with a `Retry-After` header of 10 seconds
- **When**: The LLM Gateway receives this response
- **Then**: The request is retried after at least 10 seconds
- **And**: Other queued requests are paused until the retry succeeds
- **Traces to**: REQ-LLM-001

### ASSERT-LLM-004: Parallel nodes share a single rate limit budget

- **Given**: Two pipeline nodes executing in parallel, both requesting LLM calls
- **When**: The shared rate limit state shows 1 remaining request
- **Then**: Only one node's request proceeds; the other is queued until the rate limit resets
- **And**: No race condition causes both requests to fire simultaneously
- **Traces to**: REQ-LLM-002

### ASSERT-LLM-005: Halt threshold triggers pipeline abort

- **Given**: The LLM API has been continuously rate-limited (all requests throttled or returning 429) for 30 minutes (default threshold)
- **When**: The halt-threshold timer expires
- **Then**: The pipeline is aborted with a clear error indicating prolonged rate limiting
- **And**: The abort reason is recorded in the audit trail
- **And**: The work item is updated with the abort status
- **Traces to**: REQ-LLM-003

### ASSERT-LLM-006: Rate limit events are logged with visibility

- **Given**: An LLM API response triggers a rate limit state change (remaining count drops below 20% of window capacity)
- **When**: The LLM Gateway updates its state
- **Then**: A structured log entry is emitted at warning level containing: remaining count, reset time, throttle delay applied
- **Traces to**: REQ-LLM-004

---

## Label Configuration

### ASSERT-LABEL-001: Pipeline-internal labels are always prefixed and not configurable

- **Given**: A configuration file that attempts to override a pipeline-internal `cogworks:node:*` label or maps a workflow signal such as readiness to the same value
- **When**: The configuration is loaded
- **Then**: The configuration is rejected with a clear error message and the pipeline halts
- **And**: Pipeline-internal labels retain their `cogworks:` prefix
- **Traces to**: REQ-LABEL-002

### ASSERT-LABEL-002: Workflow-semantic labels use configured names

- **Given**: A configuration that maps the `safety` workflow label to `priority:safety-critical`
- **When**: The pipeline applies the safety label to a work item
- **Then**: The label applied is `priority:safety-critical`, not the default name
- **Traces to**: REQ-LABEL-001

### ASSERT-LABEL-003: Duplicate label names are rejected at startup

- **Given**: A configuration where two different workflow labels map to the same GitHub label name
- **When**: The configuration is loaded
- **Then**: An error is emitted identifying the duplicate
- **And**: The pipeline refuses to start
- **Traces to**: REQ-LABEL-003

### ASSERT-LABEL-004: Workflow label matching a pipeline-internal label is rejected at startup

- **Given**: A configuration that maps a workflow label to a pipeline-internal label string (e.g., `trigger = "cogworks:processing"`)
- **When**: The configuration is loaded
- **Then**: The configuration is rejected with a clear error identifying the conflict with the named pipeline-internal label
- **And**: The pipeline refuses to start
- **Traces to**: REQ-LABEL-003

---

## GitHub Projects Integration

### ASSERT-PROJECT-001: Status is synced to project board on node transition

- **Given**: A work item configured with a GitHub Projects V2 board, and the pipeline moves the item from node A to node B
- **When**: The node transition completes
- **Then**: The project board item's status field is updated to reflect the new node
- **Traces to**: REQ-PROJECT-001, REQ-PROJECT-003

### ASSERT-PROJECT-002: Project board failure does not block pipeline

- **Given**: The GitHub Projects V2 API is unavailable (returns HTTP 500)
- **When**: CogWorks attempts to sync a work item status to the project board
- **Then**: The failure is logged at warning level
- **And**: The pipeline continues execution without delay
- **Traces to**: REQ-PROJECT-002

### ASSERT-PROJECT-003: No project board calls when not configured

- **Given**: The `config.toml` does not contain a `[github_project]` section
- **When**: A pipeline run executes
- **Then**: No GitHub Projects V2 API calls are made
- **And**: No errors or warnings are logged about project board integration
- **Traces to**: REQ-PROJECT-001

---

## Milestone Inheritance

### ASSERT-MILESTONE-001: Sub-work-items inherit parent milestone

- **Given**: A parent work item assigned to milestone "v2.1"
- **When**: CogWorks creates a sub-work-item from the parent
- **Then**: The sub-work-item is automatically assigned to milestone "v2.1"
- **Traces to**: REQ-PLAN-007

### ASSERT-MILESTONE-002: No milestone on parent means no milestone on child

- **Given**: A parent work item with no milestone assigned
- **When**: CogWorks creates a sub-work-item from the parent
- **Then**: The sub-work-item has no milestone assigned
- **And**: No error or warning is logged
- **Traces to**: REQ-PLAN-007, EDGE-084

---

## Semantic Stalling Detection

### ASSERT-STALL-001: Repeated same-category diagnostics trigger early escalation

- **Given**: A code generation node that has retried 3 times, and all 3 retries produced diagnostics in the same error category (e.g., `type_error`)
- **When**: The retry loop evaluates the latest diagnostics
- **Then**: The node escalates early (before exhausting the full retry budget)
- **And**: The escalation reason includes the stalling category and repetition count
- **Traces to**: REQ-CODE-006

### ASSERT-STALL-002: Different diagnostic categories do not trigger stalling

- **Given**: A code generation node that has retried 3 times, with categories `type_error`, `interface_mismatch`, and `syntax_error`
- **When**: The retry loop evaluates the latest diagnostics
- **Then**: No stalling is detected
- **And**: The node continues retrying normally using its standard retry budget
- **Traces to**: REQ-CODE-006

### ASSERT-STALL-003: Stalling detection tracks categories across retries

- **Given**: A code generation node that retries with diagnostics: retry 1 = `type_error`, retry 2 = `interface_mismatch`, retry 3 = `type_error`, retry 4 = `type_error`, retry 5 = `type_error`
- **When**: The retry loop evaluates after retry 5
- **Then**: Stalling is detected because 3 consecutive retries (3, 4, 5) produced the same category
- **And**: The stalling metric `cogworks_semantic_stalling_total` is incremented
- **Traces to**: REQ-CODE-006

---

## Native Sub-Issues and Typed Links

### ASSERT-SUBISSUE-001: Sub-work-items created via native sub-issue API

- **Given**: A parent work item that needs to be broken into sub-work-items during planning
- **When**: The planner creates sub-work-items
- **Then**: Each sub-work-item is created using the GitHub native sub-issue API (not just a label reference)
- **And**: The parent-child relationship is visible in the GitHub UI
- **Traces to**: REQ-PLAN-005

### ASSERT-SUBISSUE-002: Dependencies expressed via typed issue links

- **Given**: Sub-work-item B depends on sub-work-item A
- **When**: The planner records this dependency
- **Then**: A typed issue link of type `blocks` is created **from A to B** (the canonical direction: dependency source blocks the dependent target)
- **And**: No `cogworks:depends-on:*` label is used
- **Traces to**: REQ-PLAN-005

### ASSERT-STALL-004: Stalling threshold exceeding retry budget produces a warning at startup

- **Given**: A pipeline configuration where the stalling threshold (`semantic_stall_threshold`) is set to 5 but the node's `max_retries` is 3
- **When**: The pipeline configuration is loaded
- **Then**: A warning is emitted noting that stalling detection cannot fire for this node (threshold 5 > retry budget 3)
- **And**: Stalling detection is treated as disabled for that node (pipeline still starts — this is a warning, not an error)
- **Traces to**: REQ-CODE-006

---

## Security — Constitutional Layer, Injection Detection, Scope Enforcement

### ASSERT-SEC-001: Constitutional rules from non-approved branch are rejected

- **Given**: A `ConstitutionalRules` document loaded from a feature branch (e.g. `"feature/my-work"`)
- **When**: `validate_constitutional_prompt` is called with those rules
- **Then**: The call returns `Err(ConstitutionalError::InvalidSourceBranch { branch })` where `branch` matches the feature branch name
- **And**: No prompt is assembled
- **Traces to**: THREAT-003, THREAT-015

### ASSERT-SEC-002: Tampered constitutional rules (hash mismatch) are rejected before rule-presence check

- **Given**: A `ConstitutionalRules` document whose `content` has been modified after load (so the computed SHA-256 no longer matches `source_hash`), and whose content is also missing required rule signatures
- **When**: `validate_constitutional_prompt` is called with those rules on an approved branch
- **Then**: The call returns `Err(ConstitutionalError::HashMismatch { expected, computed })` — **not** `Err(ConstitutionalError::MissingRules)`
- **And**: The hash check is proved to run **before** the rule-presence check
- **Traces to**: THREAT-015

### ASSERT-SEC-003: Constitutional rules missing required rule signatures are rejected with full missing list

- **Given**: A `ConstitutionalRules` document on an approved branch with a correct hash, but with one or more required rule signatures absent from `content`
- **When**: `validate_constitutional_prompt` is called
- **Then**: The call returns `Err(ConstitutionalError::MissingRules { missing })` where `missing` lists every absent `RequiredRule` variant — no absent variant is omitted
- **And**: No prompt is assembled
- **Traces to**: THREAT-001, THREAT-003

### ASSERT-SEC-004: Injection detected in external content halts pipeline

- **Given**: External content (issue body, repository file, or domain service response) that contains a known injection trigger phrase (e.g. "ignore all previous instructions")
- **When**: `detect_injection` is called with that content
- **Then**: The call returns `InjectionDetectionResult::InjectionDetected { source, offending_text, pattern }` where `pattern` identifies the highest-severity matching `InjectionPattern` class and `offending_text` is non-empty
- **And**: `source` equals the `source_label` argument passed to `detect_injection`
- **Traces to**: THREAT-001, THREAT-002, THREAT-006

### ASSERT-SEC-005: Artifact matching a protected path violates scope regardless of approved patterns

- **Given**: An `ApprovedScope` whose `artifact_patterns` include a pattern that would match an artifact path, but that artifact path also matches a `ProtectedPath` entry in `protected_paths`
- **When**: `validate_scope` is called with that artifact
- **Then**: The call returns `Err(violations)` where exactly one violation of kind `ProtectedPathViolation` is present for that artifact
- **And**: No additional `UnauthorizedCapability` violation is produced for the same artifact
- **Traces to**: THREAT-004, THREAT-006
