# Risk Register

This document catalogs identified risks to CogWorks operations, their assessed likelihood and impact, required mitigations, and residual risk after mitigation. It is a living document reviewed quarterly or after any incident.

---

## Risk Scoring

**Likelihood:** 1 (Rare) – 2 (Unlikely) – 3 (Possible) – 4 (Likely) – 5 (Almost Certain)

**Impact:** 1 (Negligible) – 2 (Minor) – 3 (Moderate) – 4 (Major) – 5 (Critical)

**Risk Score:** Likelihood × Impact. Scores ≥ 12 require active mitigation before the system is used in that context.

---

## Risk Summary

| ID | Risk | L | I | Score | Status |
| ---- | ------ | --- | --- | ------- | -------- |
| CW-R01 | Subtle logic error in safety-critical firmware | 4 | 5 | 20 | Mitigations required |
| CW-R02 | LLM reward-hacks the review process | 3 | 4 | 12 | Mitigations required |
| CW-R03 | Security vulnerability in generated code | 3 | 4 | 12 | Mitigations required |
| CW-R04 | Accumulated technical debt from generated code | 3 | 3 | 9 | Monitor |
| CW-R05 | CogWorks / external system race condition on GitHub state | 2 | 4 | 8 | Mitigations required |
| CW-R06 | Domain service crash or garbage response | 2 | 3 | 6 | Mitigations required |
| CW-R07 | Cross-domain constraint false negative | 3 | 4 | 12 | Mitigations required |
| CW-R08 | Digital twin diverges from real system | 3 | 3 | 9 | Mitigations required |
| CW-R09 | Digital twin insufficient fidelity | 3 | 3 | 9 | Mitigations required |
| CW-R10 | Uncontrolled LLM cost on stuck pipeline | 2 | 4 | 8 | Mitigations required |
| CW-R11 | Credential exposure or scope creep | 3 | 4 | 12 | Mitigations required |
| CW-R12 | Malicious or adversarial work item injection | 2 | 4 | 8 | Mitigations required |
| CW-R13 | Audit trail incomplete or mutable | 3 | 3 | 9 | Mitigations required |
| CW-R14 | Team loses understanding of generated codebase | 2 | 3 | 6 | Monitor |
| CW-R15 | Context assembly includes wrong or stale files | 3 | 3 | 9 | Mitigations required |
| CW-R16 | Prompt template drift degrades output quality | 2 | 3 | 6 | Monitor |
| CW-R17 | LLM provider API change or outage | 2 | 3 | 6 | Mitigations required |
| CW-R18 | CogWorks modifies its own prompts or scenarios | 4 | 4 | 16 | Mitigations required |

---

## Detailed Risk Entries

### CW-R01: Subtle Logic Error in Safety-Critical Firmware

**Description:** The LLM generates code with a subtle logic error (off-by-one, sign error, race condition, incorrect unit conversion) in a safety-critical module such as motor control, brake actuation, or slope stability calculation. The error passes compilation, unit tests, and LLM review, and is deployed to hardware.

**Likelihood:** 4 (Likely — LLMs reliably produce subtle errors in numerical and timing-sensitive code)

**Impact:** 5 (Critical — autonomous vehicle on a hillside could behave unpredictably, risk of property damage or injury)

**Risk Score:** 20

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R01-M1 | Safety-critical modules require mandatory human review regardless of CogWorks review results (REQ-REVIEW-006, REQ-CLASS-002). No configuration option to bypass. | Preventive | Designed |
| CW-R01-M2 | Maintain a catalogue of "LLM failure patterns" (sign errors, off-by-one, integer overflow at boundaries, unit confusion). Write specific scenarios targeting these patterns. Update catalogue from audit trail analysis. | Detective | Planned |
| CW-R01-M3 | Implement property-based testing (proptest) for all safety-critical modules. Properties include: angle never exceeds physical limits, velocity never exceeds safe threshold, watchdog always triggers on comms loss, brake always engages on fault. | Detective | Planned |
| CW-R01-M4 | Scenario validation with satisfaction threshold ≥ 0.99 for safety-critical modules (stricter than the default 0.95). Failure criteria violations are always blocking. | Detective | Planned |
| CW-R01-M5 | Formal invariant specifications for critical control loops, checked by model checking or static analysis tools, not by LLM. | Detective | Future |
| CW-R01-M6 | Constitutional scope-binding rules prevent the LLM from implementing capabilities not in the approved specification. This reduces the probability of the LLM introducing unspecified behaviour (e.g., adding undocumented control paths) that could manifest as a subtle logic error in safety-critical code. | Preventive | Planned |

**Residual Risk:** Moderate. No automated system can guarantee absence of subtle errors. Defence-in-depth (multiple independent detection mechanisms) reduces probability but does not eliminate it. Human review of safety-critical code remains essential. Physical validation by downstream external systems is expected before any safety-critical firmware change is deployed to production hardware.

---

### CW-R02: LLM Reward-Hacks the Review Process

**Description:** The same model family generates code and reviews it. Systematic biases in the model cause the reviewer to consistently miss the same error classes that the generator consistently produces. Alternatively, CogWorks could modify its own review prompts or scenario specifications through the normal pipeline (see CW-R18), making review more lenient over time.

**Likelihood:** 3 (Possible — correlated biases between generation and review are likely; self-modification requires specific circumstances)

**Impact:** 4 (Major — undetected systematic quality degradation across multiple PRs)

**Risk Score:** 12

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R02-M1 | Use a different LLM model (or provider) for review passes than for code generation. Configure via model selection per node. | Preventive | Designed |
| CW-R02-M2 | Prompt templates, scenario specifications, and the conformance test suite are in a protected path with CODEOWNERS requiring human approval for any change. CogWorks PRs touching these paths are automatically flagged. | Preventive | Planned |
| CW-R02-M3 | Deterministic checks (compilation, linting, property tests, static analysis) are immune to LLM bias. Maximise the coverage of deterministic validation. | Preventive | Designed |
| CW-R02-M4 | Periodic human audit of CogWorks review outputs — sample review findings and verify they're catching real issues, not rubber-stamping. Track review finding rates over time; a sudden drop indicates potential degradation. | Detective | Planned |
| CW-R02-M5 | CogWorks MUST NOT modify files matching protected path patterns (see CW-R18). Enforced by the constitutional layer. | Preventive | Planned |
| CW-R02-M6 | Alignment verification at each pipeline stage detects semantic drift between generator input and output. The LLM alignment check uses a different model from the generator (reducing correlated bias). Alignment failures trigger rework with specific misalignment findings, not retry. | Preventive | Designed |

**Residual Risk:** Low-moderate with mitigations. Different model for review significantly reduces correlated bias. Alignment verification adds a per-stage semantic check that catches drift the review pass might miss. Protected paths prevent self-modification.

---

### CW-R03: Security Vulnerability in Generated Code

**Description:** The LLM generates code containing a security vulnerability: command injection, path traversal, insecure deserialization, hardcoded credentials, CAN bus authentication bypass, OTA update signature bypass, or buffer overflow in unsafe blocks.

**Likelihood:** 3 (Possible — LLMs produce insecure patterns, especially in areas they've seen less training data for, such as embedded security)

**Impact:** 4 (Major — security breach in safety-critical autonomous vehicle firmware)

**Risk Score:** 12

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R03-M1 | Dedicated security review pass in the review gate (REQ-REVIEW-002) with a domain-specific security checklist covering: OTA authentication, CAN bus message authentication, bootloader integrity, panic paths in no-std, unbounded allocations, unsafe block justification. | Detective | Designed |
| CW-R03-M2 | Deterministic security scanning as part of the Rust domain service: `cargo audit` (dependency CVEs), `cargo clippy` security lints, `cargo-geiger` (unsafe code auditing). These run before LLM review. | Detective | Planned |
| CW-R03-M3 | Generated code must never contain credentials, tokens, or secrets. The constitutional layer explicitly prohibits this. CI checks scan for high-entropy strings and known credential patterns. | Preventive | Planned |
| CW-R03-M4 | Add a `security_scan` capability to the domain service interface for domain-specific static analysis tools beyond standard linting. | Detective | Future |
| CW-R03-M5 | Security-relevant modules (authentication, cryptography, network communication) are classified as safety-critical and require human review per CW-R01-M1. | Preventive | Planned |

**Residual Risk:** Moderate. Static analysis catches known patterns; LLM review catches some novel patterns; human review of security-critical modules is the final defence. Novel vulnerability classes remain a risk.

---

### CW-R04: Accumulated Technical Debt from Generated Code

**Description:** Code generated by CogWorks works and passes review but is overly complex, poorly abstracted, inconsistently structured, or doesn't follow established codebase patterns. This accumulates as technical debt, making future changes harder and degrading CogWorks' own output quality (poor context → poor generation).

**Likelihood:** 3 (Possible — LLMs tend toward verbose, over-engineered solutions)

**Impact:** 3 (Moderate — increased maintenance cost, degraded CogWorks effectiveness over time)

**Risk Score:** 9

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R04-M1 | Architecture compliance review pass checks pattern consistency, not just interface compliance. Existing code patterns are surfaced in context via pyramid summaries. | Detective | Designed |
| CW-R04-M2 | Track code quality metrics (complexity, coupling, consistency) per CogWorks pipeline run. Alert on declining trends. | Detective | Planned |
| CW-R04-M3 | Periodic human-driven refactoring work items to clean up accumulated debt. Schedule quarterly. | Corrective | Planned |
| CW-R04-M4 | CogWorks' code quality review prompt explicitly penalises unnecessary complexity and rewards consistency with existing patterns. | Preventive | Designed |

**Residual Risk:** Low-moderate. Technical debt is inevitable in any codebase; the mitigations ensure it's detected and managed rather than silently accumulating.

---

### CW-R05: CogWorks / External System Race Condition on GitHub State

**Description:** Both CogWorks and external gate enforcement systems read and write GitHub labels and comments on the same issues. If they race (CogWorks moves a label forward while an external system is evaluating a gate), the issue enters an inconsistent state. GitHub labels are not transactional.

**Likelihood:** 2 (Unlikely — events are sequential in practice, but concurrent pipeline runs could trigger this)

**Impact:** 4 (Major — inconsistent state could lead to skipped gates or blocked pipelines)

**Risk Score:** 8

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R05-M1 | Define clear label ownership: CogWorks owns `cogworks:*` labels. External systems own their own label namespaces (e.g., `state:*`, `process:*`). Neither system modifies the other's labels. | Preventive | Planned |
| CW-R05-M2 | Use GitHub's event system for coordination: external systems react to PR creation webhooks, not polling. CogWorks acts, GitHub emits event, external system reacts — clear sequence. | Preventive | Planned |
| CW-R05-M3 | Collision detection: if either system detects inconsistent label state, halt processing, post a warning comment, and escalate to human. Do not auto-resolve. | Detective | Planned |
| CW-R05-M4 | Use `cogworks:processing` label as a lightweight lock (REQ-PIPE-007). Check-and-set with short hold time. | Preventive | Designed |

**Residual Risk:** Low. Race conditions require specific timing; mitigations make them detectable and recoverable.

---

### CW-R06: Domain Service Crash or Garbage Response

**Description:** A domain service crashes, times out, hangs, or returns a response that passes schema validation but contains nonsensical content (e.g., `passed: true` with blocking diagnostics, or zero tests run).

**Likelihood:** 2 (Unlikely — conformance tests catch most issues, but production edge cases exist)

**Impact:** 3 (Moderate — pipeline stalls or produces incorrect validation results)

**Risk Score:** 6

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R06-M1 | Extension API conformance test suite run in CI for every domain service release. | Preventive | Planned |
| CW-R06-M2 | Sanity checks in the orchestrator: validate response consistency (pass=true with blocking diagnostics is a contradiction), check for suspicious results (zero tests run, zero artifacts checked). | Detective | Planned |
| CW-R06-M3 | Circuit breaker: if a domain service fails N consecutive times, stop retrying and escalate. | Preventive | Planned |
| CW-R06-M4 | Process isolation and timeouts for all domain service invocations. Timeout kills the request; orchestrator retries or escalates. | Preventive | Designed |

**Residual Risk:** Low. Crash recovery is handled by the stateless architecture; garbage responses are harder to detect but sanity checks catch the most dangerous cases.

---

### CW-R07: Cross-Domain Constraint False Negative

**Description:** The cross-domain constraint validator reports a pass, but the actual value violates the constraint. The most likely cause is incomplete extraction — the domain service misses some relevant artifacts or values when computing the actual value for a constraint parameter.

**Likelihood:** 3 (Possible — extraction from source code is inherently fragile)

**Impact:** 4 (Major — physical interface violation could cause hardware damage or safety incident)

**Risk Score:** 12

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R07-M1 | Domain services report `status: "incomplete"` when they cannot confidently extract all relevant values. Incomplete extraction is a warning requiring human review, not a silent pass. | Detective | Planned |
| CW-R07-M2 | For critical constraints (bus load, power budget, thermal limits), require explicit declaration annotations in source code (e.g., `// @cogworks bus_load: 0.48`). The constraint validator checks the declaration against the contract, and the domain service's `review_rules` checks that the declaration matches reality. | Preventive | Planned |
| CW-R07-M3 | Downstream manufacturing gates (external to CogWorks) require physical test results before production release. Physical test validation is the final line of defence for cross-domain constraints. | Detective | Planned |
| CW-R07-M4 | Track constraint validation accuracy over time by comparing constraint results against physical test outcomes. If constraint validation consistently misses issues that physical testing catches, investigate and improve extraction. | Detective | Future |

**Residual Risk:** Moderate. Extraction from arbitrary source code will never be perfect. The annotation approach (CW-R07-M2) and physical testing (CW-R07-M3) provide defence-in-depth.

---

### CW-R08: Digital Twin Diverges from Real System

**Description:** A DTU twin is built against a specific version of an external system (hardware revision, firmware version, protocol specification). The real system changes (updated firmware, hardware ECO, protocol revision) and the twin is not updated. Scenarios pass against the stale twin but fail against the real system.

**Likelihood:** 3 (Possible — systems change, twin maintenance is easy to neglect)

**Impact:** 3 (Moderate — false confidence in validation results, bugs escape to physical testing or field)

**Risk Score:** 9

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R08-M1 | Each twin has a conformance test suite that runs against the real system (REQ-DTU-004). Schedule monthly and on every hardware/firmware revision. | Detective | Planned |
| CW-R08-M2 | Track twin conformance status. If conformance tests fail, mark the twin as stale. Scenario validation results from stale twins are flagged as unverified. | Detective | Planned |
| CW-R08-M3 | Downstream gate systems check twin conformance status before accepting scenario validation results for production releases. | Preventive | Planned |
| CW-R08-M4 | Twin specifications include the version of the real system they target. Version mismatch between twin spec and current system version triggers a warning. | Detective | Planned |

**Residual Risk:** Low with regular conformance testing. The risk increases if conformance testing is neglected — hence the downstream gate enforcement (CW-R08-M3).

---

### CW-R09: Digital Twin Insufficient Fidelity

**Description:** A twin replicates the basic API or protocol behaviour but doesn't simulate important real-world characteristics: timing jitter, bus contention, error frames, partial failures, environmental effects. Code that works against the idealised twin fails under real-world conditions.

**Likelihood:** 3 (Possible — there's always a fidelity trade-off in simulation)

**Impact:** 3 (Moderate — bugs escape to physical testing, potential field failures)

**Risk Score:** 9

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R09-M1 | Twin specifications document fidelity boundaries: what behaviours are replicated and what is simplified or omitted. | Preventive | Planned |
| CW-R09-M2 | Scenarios are tagged with required fidelity level. Scenarios requiring higher fidelity than the available twin are flagged as requiring physical test validation. | Preventive | Planned |
| CW-R09-M3 | Twins support failure injection (REQ-DTU-003) to test degraded-mode behaviour even when base fidelity is limited. | Detective | Planned |
| CW-R09-M4 | Prioritise twin development for the dependencies with the most complex failure modes and highest safety impact. Don't twin everything. | Preventive | Planned |

**Residual Risk:** Moderate. Simulation can never fully replace physical testing for safety-critical systems. Twins are a pre-filter, not a replacement.

---

### CW-R10: Uncontrolled LLM Cost on Stuck Pipeline

**Description:** A code generation loop can't converge — the LLM keeps producing code that fails validation, burning tokens on every retry. The retry budget eventually halts the loop, but if the budget is too high or cost tracking has a bug, the bill is large.

**Likelihood:** 2 (Unlikely — retry budget is designed to prevent this)

**Impact:** 4 (Major — unexpected cloud bill, potentially thousands of dollars)

**Risk Score:** 8

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R10-M1 | Per-pipeline cost budget with automatic halt (REQ-CODE-004). | Preventive | Designed |
| CW-R10-M2 | Per-call token limit so no single LLM invocation consumes a disproportionate share of the budget. | Preventive | Planned |
| CW-R10-M3 | Hard billing alert at the LLM provider level (independent of CogWorks' own tracking). Defence-in-depth against bugs in cost tracking. | Detective | Planned |
| CW-R10-M4 | Per-node cost breakdown in the audit trail (REQ-AUDIT-002). Review weekly for anomalies. | Detective | Designed |
| CW-R10-M5 | Semantic stalling detection (REQ-CODE-006). If the same error category recurs across consecutive retries (default: 3), escalate early instead of burning remaining retry budget on a problem the LLM cannot solve. | Preventive | Designed |

**Residual Risk:** Low. Multiple independent cost controls make undetected runaway spending unlikely.

---

### CW-R11: Credential Exposure or Scope Creep

**Description:** CogWorks' runtime environment has access to credentials beyond what it needs (production database, manufacturing API, cloud services). Generated code could accidentally or deliberately access production systems during testing. Or CogWorks' GitHub token has excessive permissions.

**Likelihood:** 3 (Possible — credential scoping is frequently under-implemented in practice)

**Impact:** 4 (Major — production data corruption, unauthorised system access)

**Risk Score:** 12

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R11-M1 | CogWorks runs in an isolated environment with access only to: its GitHub token (minimum required scopes), LLM API key, and domain service sockets. No production database, manufacturing, or cloud service credentials. | Preventive | Planned |
| CW-R11-M2 | GitHub token scoped to minimum permissions: repo contents read/write, issues read/write, pull requests read/write. No admin, no org-level access. | Preventive | Planned |
| CW-R11-M3 | Domain service test execution runs in a network-restricted sandbox — no outbound network except to DTU twins on localhost. | Preventive | Planned |
| CW-R11-M4 | Container network policies enforce isolation. CogWorks container can reach: LLM API (outbound HTTPS), GitHub API (outbound HTTPS), domain service sockets (local). Nothing else. | Preventive | Planned |
| CW-R11-M5 | Separate credential store for CogWorks with only CogWorks-relevant secrets. No shared credential store with production systems. | Preventive | Planned |
| CW-R11-M6 | Tool profile scoping limits each pipeline node to the minimum set of tools it needs. Network access (`net.fetch`) is opt-in and requires explicit URL allowlists. Shell execution is restricted to pre-configured command allowlists. | Preventive | Designed |
| CW-R11-M7 | OS-level sandboxing (Layer 3) for Code Generation constrains process-level filesystem and network access independently of tool-level enforcement. | Preventive | Designed |

**Residual Risk:** Low with proper isolation. Requires discipline during deployment setup to not take shortcuts.

---

### CW-R12: Malicious or Adversarial Work Item Injection

**Description:** An attacker (or an innocent user with a cleverly worded issue) crafts a work item description that causes the LLM to generate code with a backdoor, exfiltrate data via PR descriptions, or modify security-critical paths.

**Likelihood:** 2 (Unlikely — requires repo access and knowledge of CogWorks' pipeline)

**Impact:** 4 (Major — backdoor in firmware, data exfiltration)

**Risk Score:** 8

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R12-M1 | CogWorks only processes issues from authorised users (configurable allowlist or restricted to repository collaborators). | Preventive | Planned |
| CW-R12-M2 | The configured readiness signal (default label mapping: `cogworks:run`) can only be applied by authorised users, as enforced by the work-item provider's permissions. | Preventive | Planned |
| CW-R12-M3 | Constitutional layer treats all issue body content as data, not instructions. Injection attempts are detected and halt the pipeline (see CW-R18 mitigations). | Preventive | Planned |
| CW-R12-M4 | Security review pass in the review gate checks for common backdoor patterns, unauthorized network calls, and data exfiltration indicators. | Detective | Designed |
| CW-R12-M5 | Scope enforcement prevents generation of capabilities not in the approved specification. | Preventive | Planned |
| CW-R12-M6 | Tool scope enforcement limits the LLM's runtime capabilities: filesystem writes constrained to `allowed_write_paths`, git commits to `branch_pattern`, domain service calls to `allowed_services`. Even if the LLM is manipulated by a crafted issue, it cannot write outside its scoped paths. | Preventive | Designed |

**Residual Risk:** Low-moderate. The combination of authorised user filtering, injection detection, scope enforcement, and security review provides defence-in-depth. Subtle social engineering attacks against human reviewers remain a risk beyond CogWorks' scope.

---

### CW-R13: Audit Trail Incomplete or Mutable

**Description:** Audit events are posted as GitHub issue comments, which can be edited or deleted by anyone with write access to the repository. If the audit trail is incomplete (events dropped due to API failures) or mutable (events edited after the fact), post-hoc review and ISO 9001 traceability are compromised.

**Likelihood:** 3 (Possible — API failures during comment posting are normal; editing is always possible)

**Impact:** 3 (Moderate — compromised traceability, inability to reconstruct decision history)

**Risk Score:** 9

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R13-M1 | Audit trail completeness check at pipeline completion: verify all expected events were recorded. Missing events trigger a warning in the final cost/status comment. | Detective | Planned |
| CW-R13-M2 | Include a content hash in each audit comment linking to the previous comment's hash, creating a lightweight tamper-detection chain. Editing a comment breaks the hash chain. | Detective | Planned |
| CW-R13-M3 | Retry audit comment posting with backoff on API failure. Buffer events in memory and batch-post if individual posts fail. | Preventive | Planned |
| CW-R13-M4 | For ISO 9001 traceability requirements, consider exporting audit trails to an append-only external store (future enhancement). | Preventive | Future |

**Residual Risk:** Low-moderate. GitHub comments are inherently mutable. The hash chain detects tampering but cannot prevent it. An append-only external store would reduce residual risk further.

---

### CW-R14: Team Loses Understanding of Generated Codebase

**Description:** As CogWorks generates more code, human team members review less of it carefully. Over time, the team's understanding of the codebase degrades to the point where they cannot effectively maintain, debug, or extend the system without CogWorks.

**Likelihood:** 2 (Unlikely in near term; 4 Likely in long term without intervention)

**Impact:** 3 (Moderate — reduced ability to respond to incidents, audit effectively, or evolve architecture)

**Risk Score:** 6

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R14-M1 | Safety-critical modules require human review (REQ-REVIEW-006). This forces engagement with the most important code. | Preventive | Designed |
| CW-R14-M2 | CogWorks-generated specifications and PR descriptions include rationale and design decisions, making the generated code's intent understandable. | Preventive | Designed |
| CW-R14-M3 | Periodic human-driven refactoring work items (see CW-R04-M3). Refactoring requires understanding the code, maintaining comprehension. | Corrective | Planned |
| CW-R14-M4 | Track which modules have not been human-reviewed in the last N months. Flag stale modules for human review. | Detective | Future |

**Residual Risk:** Moderate long-term. This is an organisational risk that mitigations can slow but not eliminate. Active engagement through required reviews and periodic refactoring is the primary defence.

---

### CW-R15: Context Assembly Includes Wrong or Stale Files

**Description:** The context assembler includes files that are stale (cached pyramid summaries not regenerated after source changes), irrelevant (wrong dependency graph path), or missing (relevant files excluded by truncation). The LLM generates code based on incorrect or incomplete context.

**Likelihood:** 3 (Possible — staleness and relevance are hard to get right consistently)

**Impact:** 3 (Moderate — generated code has wrong dependencies, uses deprecated APIs, or misses constraints)

**Risk Score:** 9

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R15-M1 | Staleness detection is mandatory before using any cached summary. File hash comparison detects changes. Stale summaries are never used (see constraints.md). | Preventive | Designed |
| CW-R15-M2 | Context packages are logged in the audit trail (file list and summary levels used). Post-hoc review can identify context quality issues. | Detective | Designed |
| CW-R15-M3 | Deterministic priority-based truncation ensures the most relevant files are always included. Truncation decisions are logged. | Preventive | Designed |
| CW-R15-M4 | Periodic validation of context assembly accuracy: compare context packages against expected file sets for known work items. | Detective | Future |

**Residual Risk:** Low-moderate. Staleness detection and deterministic truncation handle the most common cases. Edge cases in dependency graph traversal remain a risk.

---

### CW-R16: Prompt Template Drift Degrades Output Quality

**Description:** Prompt templates evolve over time (to handle edge cases, support new features, or improve output quality) but without systematic regression testing. A template change that fixes one issue may degrade output quality in other scenarios.

**Likelihood:** 2 (Unlikely in early stages; 3 Possible as template count and complexity grow)

**Impact:** 3 (Moderate — degraded output quality across pipeline runs using the affected template)

**Risk Score:** 6

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R16-M1 | Prompt templates are version-controlled and subject to code review (existing design). | Preventive | Designed |
| CW-R16-M2 | Protected path status: prompt templates require human-approved PR to change. CogWorks cannot modify its own templates (see CW-R18). | Preventive | Planned |
| CW-R16-M3 | Track output quality metrics per template version. A quality regression after a template change indicates the change should be reverted or refined. | Detective | Planned |
| CW-R16-M4 | Maintain a regression test suite for prompt templates: known inputs with expected output characteristics. Run on template changes. | Detective | Future |

**Residual Risk:** Low. Version control and human review are sufficient for the current template count. Regression testing becomes more important as the template library grows.

---

### CW-R17: LLM Provider API Change or Outage

**Description:** The LLM provider (Anthropic initially) changes their API (deprecates a model, changes response format, alters rate limits), or experiences an extended outage. CogWorks pipeline cannot function without LLM access.

**Likelihood:** 2 (Unlikely for breaking changes — providers usually give notice; 3 Possible for outages)

**Impact:** 3 (Moderate — pipeline blocked until provider recovers or CogWorks adapts)

**Risk Score:** 6

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R17-M1 | LLM Provider abstraction trait enables switching providers without changing business logic (existing design). | Preventive | Designed |
| CW-R17-M2 | Pin model versions in configuration. Do not use "latest" model aliases. Test against specific model versions. | Preventive | Planned |
| CW-R17-M3 | Multi-provider support planned (see tradeoffs.md). Adding a second provider (e.g., OpenAI) provides failover capability. | Preventive | Future |
| CW-R17-M4 | Monitor provider status pages and API changelog. Proactively test against new API versions before migration. | Detective | Planned |
| CW-R17-M5 | LLM rate limit handling with proactive throttling and reactive backoff (REQ-LLM-001). Prevents CogWorks from triggering provider-side blocks due to excessive request rates. | Preventive | Designed |
| CW-R17-M6 | Halt threshold (REQ-LLM-003, default 30 min). If rate limiting persists, pipeline aborts cleanly rather than waiting indefinitely for provider recovery. | Preventive | Designed |

**Residual Risk:** Low with single provider; very low with multi-provider failover. The abstraction trait ensures the migration path exists; implementing a second provider is the key remaining mitigation.

---

### CW-R18: CogWorks Modifies Its Own Prompts or Scenarios

**Description:** A work item processed by CogWorks results in changes to files that govern CogWorks' own behaviour: prompt templates, scenario specifications, constitutional rules, the conformance test suite, or CogWorks' own source code. If these changes make generation or review more lenient, subsequent pipeline runs produce lower-quality output with less effective review — a self-reinforcing degradation loop.

**Likelihood:** 4 (Likely — without explicit protection, a work item that touches CogWorks-adjacent files could trigger this)

**Impact:** 4 (Major — systematic, compounding quality degradation that may not be detected until significant damage is done)

**Risk Score:** 16

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R18-M1 | Define protected path patterns: constitutional rules, prompt templates, scenario specifications, conformance test suite, output schemas, Extension API schemas. CogWorks MUST NOT create or modify files matching these patterns. | Preventive | Planned |
| CW-R18-M2 | CODEOWNERS rules require human approval for any change to protected paths. CogWorks-generated PRs touching protected paths are automatically flagged and require additional human review. | Preventive | Planned |
| CW-R18-M3 | Constitutional layer rule: specification scope is binding. CogWorks cannot implement capabilities not in the approved spec. A work item would need to explicitly specify changes to protected files, which human reviewers would catch. | Preventive | Planned |
| CW-R18-M4 | Audit trail records all files modified by each pipeline run. Periodic review of modified file lists against protected path patterns detects any violation of CW-R18-M1. | Detective | Planned |
| CW-R18-M5 | Pre-PR validation: before creating a PR, CogWorks checks whether any generated files match protected path patterns. If they do, the pipeline halts with a `PROTECTED_PATH_VIOLATION` diagnostic rather than creating the PR. | Preventive | Planned |
| CW-R18-M6 | Tool-level protected path enforcement: `fs.write`, `fs.create`, and `fs.delete` tools refuse operations targeting protected paths regardless of the calling node's profile configuration. This is enforced at the tool implementation level, independent of the pre-PR validation. | Preventive | Designed |

**Residual Risk:** Low with mitigations. The combination of constitutional rules, protected path enforcement, CODEOWNERS, and pre-PR validation creates multiple independent barriers. The highest residual risk is a novel path that is functionally equivalent to a protected path but doesn't match the pattern (e.g., a new config file that influences behaviour).

---

### CW-R19: Graph Misconfiguration Causes Runaway Rework or Dead-End Pipeline

**Description:** A misconfigured `.cogworks/pipeline.toml` results in a pipeline graph with an inadvertent cycle that evades the `max_traversals` limit, a dead-end node (no forward edge and not a terminal), or edge conditions that can never evaluate to true — causing CogWorks to spin, stall, or consume cost budget without making progress.

**Likelihood:** 3 (Possible — graph configuration is complex and teams may introduce subtle errors)

**Impact:** 3 (Moderate — pipeline stuck or cost wasted; no data loss, but work item blocked until human intervention)

**Risk Score:** 9

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R19-M1 | Load-time DAG validation: cycle detection (with `max_traversals` as the only allowed back-edges), reachability analysis (every node has a path to a terminal node). Configuration that fails validation is rejected before any execution. | Preventive | Designed |
| CW-R19-M2 | Hard cap on rework loop traversals enforced in executor code regardless of what the configuration specifies (code-level guard cannot be overridden by configuration). | Preventive | Designed |
| CW-R19-M3 | LLM-evaluated edge conditions MUST have a deterministic fallback. A condition that returns no parseable result defaults to the fallback, not to infinite retry. | Preventive | Designed |
| CW-R19-M4 | Cost budget applies across the entire pipeline run, including rework iterations. A misconfigured loop that approaches the budget limit triggers a cost-budget halt before complete exhaustion. | Detective | Designed |
| CW-R19-M5 | Pipeline configuration linting tool (separate from CogWorks runtime) validates pipeline files in CI before merge, providing early feedback to engineers. | Preventive | Planned |

**Residual Risk:** Low-moderate. Load-time validation catches structural errors; cost budget catches runaway execution. A semantically valid but logically incorrect pipeline (correct structure, impossible edge conditions) may still stall, but cost budget and processing lock TTL ensure eventual cleanup.

---

### CW-R20: Tracing Field Coverage Degradation

**Description:** If tracing field coverage degrades (e.g., missing node lifecycle or retry/rework fields), external systems may derive incomplete measurements and produce misleading process-improvement conclusions.

**Likelihood:** 2 (Unlikely — tracing field coverage is specified by ASSERT-OBS-001 through ASSERT-OBS-003 and constraints)

**Impact:** 3 (Moderate — incorrect improvement decisions waste effort but don't affect pipeline correctness)

**Risk Score:** 6

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R20-M1 | Tracing hooks record the stable fields defined by the observability interface; absent span fields are not claimed as emitted measurements. | Detective | Designed |
| CW-R20-M2 | Tracing field coverage is specified by ASSERT-OBS-001 through ASSERT-OBS-003 and constraints (Observability section). | Preventive | Designed |
| CW-R20-M3 | External dashboards should include data quality indicators for the fields actually exported. | Detective | Recommendation |

**Residual Risk:** Low. The tracing field contract is covered by focused observability assertions; external systems remain responsible for measuring data completeness.

---

### CW-R21: Alert Fatigue from External Monitoring

**Description:** Overly aggressive alerting thresholds configured in external monitoring systems (Grafana, Prometheus alertmanager) desensitize operators to alerts, causing genuine performance degradation to go unnoticed.

**Likelihood:** 2 (Unlikely — this is an operational configuration concern, not a CogWorks defect)

**Impact:** 2 (Minor — delayed response to degradation trends, but pipeline continues to function)

**Risk Score:** 4

**Mitigations:**

| ID | Mitigation | Type | Status |
| ---- | ----------- | ------ | -------- |
| CW-R21-M1 | Operations documentation provides recommended alert thresholds with graduated severity levels (info/warning/critical). | Preventive | Designed |
| CW-R21-M2 | Review cadence process (weekly/monthly/quarterly) ensures metrics are reviewed by humans, not only by automated alerts. | Preventive | Designed |

**Residual Risk:** Low. This is primarily an operational concern managed through review cadence discipline.

---

## Cross-Reference to Spec

The following spec documents contain mitigations or design decisions informed by entries in this risk register:

| Risk | Primary Spec Reference |
| ------ | ---------------------- |
| CW-R01 | constraints.md (testing), requirements.md (REQ-REVIEW-006, REQ-CLASS-002, REQ-CONST) |
| CW-R02 | constraints.md (security), requirements.md (REQ-CONST, REQ-ALIGN-001, REQ-ALIGN-004), assertions.md (ASSERT-ALIGN-012) |
| CW-R03 | security.md (THREAT-001, THREAT-002, THREAT-014), requirements.md (REQ-REVIEW-002) |
| CW-R05 | vocabulary.md (Processing Lock), operations.md (runbook) |
| CW-R06 | edge-cases.md (EDGE-043, EDGE-044), testing.md (conformance tests) |
| CW-R07 | architecture.md (Cross-Domain Constraint Validation), requirements.md (REQ-XVAL) |
| CW-R08–09 | vocabulary.md (Digital Twin), operations.md (twin maintenance) |
| CW-R10 | requirements.md (REQ-CODE-004, REQ-CODE-006), operations.md (cost management, semantic stalling runbook) |
| CW-R11 | security.md (THREAT-003, THREAT-004, THREAT-019), constraints.md (security, tool scoping) |
| CW-R12 | security.md (THREAT-001, THREAT-015, THREAT-019), requirements.md (REQ-CONST, REQ-ENFORCE) |
| CW-R13 | requirements.md (REQ-AUDIT), operations.md (audit trail) |
| CW-R15 | constraints.md (pyramid summary accuracy), vocabulary.md (Pyramid Summary Levels) |
| CW-R18 | requirements.md (REQ-CONST, REQ-TOOL-011, REQ-ENFORCE), security.md (THREAT-015, THREAT-017, THREAT-019) |
| CW-R17 | requirements.md (REQ-LLM-001 through REQ-LLM-004), security.md (THREAT-021), operations.md (LLM rate limit runbook) |
| CW-R19 | edge-cases.md (EDGE-006a, EDGE-006b, EDGE-057, EDGE-058, EDGE-064), constraints.md (Pipeline Graph), assertions.md (ASSERT-GRAPH-004, ASSERT-GRAPH-005, ASSERT-GRAPH-006, ASSERT-GRAPH-014), security.md (THREAT-018) |
| CW-R20 | constraints.md (Observability), assertions.md (ASSERT-OBS-001 through ASSERT-OBS-003), operations.md (Tracing and external analysis views) |
| CW-R21 | operations.md (Review Cadence, Improvement Loop) |
