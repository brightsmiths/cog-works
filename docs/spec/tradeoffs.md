# Design Tradeoffs

This document records the significant design alternatives considered and the rationale for each decision.

---

## 1. GitHub as Sole Durable State vs. Local Database

**Decision**: GitHub is the only durable state store.

| Factor | GitHub-Only | Local Database |
| -------- | ------------- | ---------------- |
| **Crash recovery** | Trivial — restart and read GitHub | Requires DB recovery, migration, backup |
| **Observability** | Full state visible in GitHub UI | State split between GitHub and DB |
| **Deployment** | Single binary, no DB dependency | Binary + database (Postgres/SQLite) |
| **API cost** | High — state reconstruction requires many API calls per invocation | Low — fast local reads |
| **Concurrency** | Labels as locks (race condition window) | Proper transactions |
| **Query capability** | Limited — GitHub search and label filtering | Full SQL queries |

**Rationale**: The simplicity and observability benefits outweigh the API cost. CogWorks processes a small number of work items (not thousands), so the API budget is sufficient. The race condition window for label-based locks is acceptable at expected concurrency levels (< 10 simultaneous pipelines). If API costs become a problem, ephemeral in-memory caching within a single invocation can reduce calls without adding persistent state.

**Risk**: GitHub API rate limit (5000 req/hr authenticated) could become a bottleneck with many concurrent pipelines. Mitigation: efficient API usage, batched reads, and proactive rate limit tracking from response headers.

---

## 2. CLI-First vs. Service-First Execution Model

**Decision**: CLI-first. Each invocation is a stateless step function.

| Factor | CLI-First | Service-First |
| -------- | ----------- | --------------- |
| **Simplicity** | Each invocation is independent | Needs connection pools, graceful shutdown, health checks |
| **Testability** | Test one step at a time | Must test lifecycle, concurrency, state management |
| **Crash recovery** | Restart and re-invoke | Must handle in-flight requests, reconnection |
| **Resource efficiency** | Reconstruction cost per invocation | Persistent connections, amortized setup |
| **Latency** | Cold start on each invocation | Hot paths for frequent operations |
| **Future service mode** | Add loop/event handler around step function | Already done |

**Rationale**: CLI-first is simpler to build, test, and debug. The reconstruction cost (reading GitHub state) is a few API calls — negligible compared to LLM latency. Service mode is additive: poll mode wraps the step function in a timer loop; webhook mode wraps it in an HTTP handler. The core logic is identical in all modes.

**Constraint**: Component construction must be explicit (dependency injection, not global state) so that a service wrapper can construct long-lived components with connection pools.

---

## 3. Single CogWorks Binary + External Domain Services vs. Microservices

**Decision**: Single Rust binary for CogWorks orchestrator, with domain services as separate external processes.

| Factor | Single Binary + External Services | Full Microservices |
| -------- | -------------------------------------- | --------------------- |
| **Deployment** | CogWorks = one binary, domain services = separate binaries (one per domain) | Multiple artifacts with complex orchestration |
| **Type safety** | Shared types within CogWorks; JSON Schema at Extension API boundary | Serialization at every boundary |
| **Extensibility** | New domains added by implementing Extension API, no CogWorks changes | New services added but require orchestration changes |
| **Latency** | Function calls within CogWorks; socket/HTTP to domain services | Network calls everywhere |
| **Independent evolution** | Domain services evolve independently via versioned API | All services evolve independently |
| **Complexity** | Moderate — Extension API protocol + health checking | High — service discovery, circuit breakers, distributed tracing |

**Rationale**: CogWorks does not need to be split into microservices internally — it's bounded by LLM API and domain service throughput, not internal compute. The Extension API provides the extensibility boundary: domain services are external processes that can be written in any language and deployed independently. This gives the extensibility benefits of microservices (add new domains without changing CogWorks) with the simplicity of a monolithic orchestrator.

**Key constraint**: The Rust domain service MUST use the Extension API like any third-party service. No built-in privileged path. If the API is insufficient, improve the API.

---

## 4. Sequential vs. Parallel Sub-Work-Item Processing

**Decision**: Topological dependency order with optional parallel fan-out for independent sub-work-items.

| Factor | Sequential Only | Topological + Parallel Fan-Out |
| -------- | ---------------- | ------------------------------- |
| **Context quality** | Each SWI has all prior outputs | Dependent SWIs get all dependency outputs; independent SWIs get own chain only |
| **Complexity** | Simple loop | Dependency graph scheduling, shared cost budget, fan-in synchronisation |
| **Cost** | Optimal per-SWI context | May generate slightly less optimal code for concurrent SWIs |
| **Speed** | Slower for independent SWIs | Faster when SWIs have no mutual dependency |
| **Safety** | Inherently safe ordering | Must enforce topological constraints; parallel nodes share budget atomically |

**Rationale**: REQ-CODE-001 requires dependency-ordered processing. Sub-work-items with no mutual dependency path MAY execute concurrently when the pipeline configuration allows it. The default pipeline remains sequential for simplicity; custom pipelines can enable parallel fan-out. This gives repositories the choice between simplicity (default) and speed (explicit opt-in), while ensuring dependency ordering is always respected.

---

## 5. Labels as Concurrency Locks vs. External Coordination

**Decision**: GitHub labels as lightweight processing locks.

| Factor | Labels | External (Redis, etcd) |
| -------- | -------- | ------------------------ |
| **Infrastructure** | None — uses existing GitHub | Additional service to deploy and maintain |
| **Correctness** | Race condition window between check and set | Proper atomic compare-and-set |
| **Observability** | Visible in GitHub UI | Separate monitoring |
| **Failure mode** | Stale lock if process crashes | Lock TTL with automatic expiry |

**Rationale**: At expected concurrency levels (< 10 pipelines), the probability of a label race condition is negligible. The check-and-set window is milliseconds. If a process crashes with the lock held, a human (or a subsequent invocation with a staleness check) can remove the label. External coordination adds infrastructure complexity that is not justified by the risk.

**Mitigation for stale locks**: The processing label should include a timestamp (e.g., in a corresponding issue comment). If a lock is older than a configurable timeout (default: 30 minutes), it can be considered stale and overridden.

---

## 6. Working Copy Management: Domain Service-Owned vs. CogWorks-Owned

**Decision**: Domain services manage their own working copies. CogWorks provides shared libraries.

| Factor | Domain Service-Owned | CogWorks-Owned | Hybrid (old) |
| -------- | --------------------- | ---------------- | --------------- |
| **Toolchain support** | Full support (service controls its own FS) | Full, but CogWorks must know file paths | Full |
| **Remote services** | Works (service clones remotely) | Broken (CogWorks clone is local only) | Local only |
| **Isolation** | Each service has independent clone | Shared clone = potential conflicts | Shared clone |
| **Complexity** | Services handle their own cloning | CogWorks handles cloning centrally | Split responsibility |
| **Shared libraries** | CogWorks provides optional clone library | Required CogWorks logic | CogWorks logic |

**Rationale**: Since domain services are external processes, CogWorks cannot assume they have filesystem access to a CogWorks-managed clone. Especially for future remote domain services (HTTP/gRPC), the clone must be on the service's side. CogWorks provides repository information (URL, branch, ref) via the Extension API context, and domain services manage their own clones. A shared library for common git operations (shallow clone, branch management, cleanup) is published for convenience but is optional.

**Change from previous design**: Previously, CogWorks managed the working copy centrally with a hybrid GitHub API + git clone approach. The domain generalisation shifts clone responsibility to domain services, enabling remote services and better isolation.

---

## 7. LLM Output Validation: JSON Schema vs. Type-Driven Parsing

**Decision**: JSON Schema for validation, with typed parsing after validation.

| Factor | JSON Schema | Direct Type Parsing (serde) |
| -------- | ------------- | ---------------------------- |
| **Declarative** | Schema files are human-readable and auditable | Validation embedded in code |
| **Version control** | Schema changes are visible in diffs | Type changes require code review |
| **LLM guidance** | Schemas can be included in prompts to guide output | Types are internal to CogWorks |
| **Performance** | Two-pass (validate then parse) | Single-pass |
| **Flexibility** | Can express constraints beyond types (min/max, patterns) | Limited to what types express |

**Rationale**: JSON Schemas serve double duty — they validate LLM output and can be included in prompts to guide the LLM toward producing valid output. The two-pass overhead (validate against schema, then deserialize into types) is negligible compared to LLM latency.

---

## 8. Four Review Passes (One Deterministic + Three LLM) vs. One Combined Review

**Decision**: Four review passes: one deterministic (cross-domain constraint validation) + three LLM passes (quality, architecture, security).

| Factor | Four Passes (1 deterministic + 3 LLM) | One Combined Pass |
| -------- | -------------------------------------- | ------------------ |
| **Focus** | Each pass has a narrow focus → higher quality findings | Broad prompt → may miss depth |
| **Cost** | 3x LLM calls per sub-work-item (1 deterministic pass is free) | 1x LLM call |
| **Prompt size** | Smaller, focused prompts | Larger prompt with multiple concerns |
| **Independent evolution** | Each pass can be tuned independently | One monolithic prompt |
| **Parallelism** | LLM passes can run in parallel (future) | N/A |

**Rationale**: Separate assurance stages per concern (REQ-ASSURE-001) produce better findings, because each stage's profile can carry checklists and examples specific to its domain. The cost is bounded, and because a review stage produces findings rather than a verdict, adding a perspective adds information rather than another thing that can veto a correct change.

---

## 9. Direct Provider Access vs. a Model Gateway

**Decision**: every model call goes through a model gateway. CogWorks holds no provider credential.

| Factor | Direct provider access | Model gateway |
| -------- | --------------------- | ------------- |
| **Credential surface** | A provider key in CogWorks and in every sandbox | No key in CogWorks or any sandbox |
| **Rate limiting** | Per-process, blind to the rest of the factory | One place that sees all traffic and can enforce coherently |
| **Provider choice** | A provider name appears in the codebase | Resolved by the gateway from a logical model name |
| **Cost attribution** | Per-process accounting | Factory-wide, per-service |
| **Dependency** | One fewer service on the critical path | The gateway is on the path of every model call |

**Rationale**: the decisive factor is the credential. A provider key held here would also have to
be reachable from every sandbox running an agent, which multiplies the places it can leak by the
number of attempts. With a gateway there is no key in this process to leak — a stronger property
than careful handling of one.

Rate limiting follows the same logic: per-process throttling cannot see the other processes
sharing the same quota, so it either over-throttles or fails to prevent exhaustion. Only the
component holding the credentials can enforce a quota coherently.

The cost is a service on the critical path of every model call. Accepted, because CogWorks already
cannot function without the stage runner and the gate service — one more is not a change in kind.

---

## 10. External Process Domain Services vs. In-Process Trait Implementations

**Decision**: Domain services are external processes communicating via Extension API.

| Factor | External Process | In-Process Trait |
| -------- | ----------------- | ----------------- |
| **Language freedom** | Domain services can be any language | Must be Rust (or use FFI) |
| **Extensibility** | New domains without CogWorks changes | Requires CogWorks rebuild/release |
| **Isolation** | Process-level isolation (no shared memory) | Shared address space |
| **Latency** | Socket/HTTP overhead (~1ms per call) | Function call overhead (~1μs) |
| **Security** | CogWorks doesn't need domain toolchains | CogWorks process has all toolchains |
| **Debugging** | Two processes to inspect | Single process |
| **Deployment** | Multiple binaries to distribute | Single binary |

**Rationale**: The extensibility benefit is decisive. CogWorks must support domains beyond software (electrical, mechanical, etc.) without source changes. External processes allow domain services to be written by different teams in different languages, evolve independently, and be deployed independently. The socket overhead is negligible compared to the operations domain services perform (compilation, simulation, etc. take seconds to minutes). The Rust domain service serves as the reference implementation that validates the Extension API is complete and usable.

**Risk**: The Extension API must be sufficient for all domain service needs. If the API is missing capabilities, domain services are blocked. Mitigation: the Rust domain service is built first and uses the same API, surfacing gaps early.

---

## 11. Unix Domain Socket vs. HTTP/gRPC as Default Transport

**Decision**: Unix domain socket as default, HTTP/gRPC as optional alternative.

| Factor | Unix Socket | HTTP/gRPC |
| -------- | ------------ | ----------- |
| **Latency** | ~0.1ms per message | ~1-5ms per message |
| **Remote services** | Local only | Works across machines |
| **Authentication** | File permissions | Need auth mechanism (tokens, mTLS) |
| **Tooling** | Limited debugging tools | curl, Postman, grpcurl |
| **Ecosystem** | Standard on Linux/macOS, limited on Windows | Universal |
| **Complexity** | Simple | More infrastructure (ports, TLS, etc.) |

**Rationale**: For the initial deployment (single machine, co-located services), Unix sockets are simpler, faster, and naturally secured by file permissions. HTTP/gRPC support is configurable per domain service for future remote deployment scenarios. The transport layer is behind an abstraction, so adding new transports does not change business logic.

**Windows consideration**: Named pipes provide equivalent functionality to Unix domain sockets. The transport abstraction must accommodate platform differences.

---

## 12. External Metrics Backend vs. Built-in Metrics Store

**Decision**: CogWorks exports structured tracing through the configured OpenTelemetry path. External systems may derive metrics from exported fields; CogWorks has no built-in metrics storage, aggregation, or dashboarding.

| Factor | External Metrics Backend | Built-in Metrics Store |
| -------- | ------------------------ | ------------------------ |
| **Focus** | CogWorks stays focused on automated engineering pipelines | CogWorks takes on metrics infrastructure responsibility |
| **Aggregation** | Purpose-built tools (Prometheus, Mimir) handle time-series aggregation natively | Must implement aggregation logic across pipeline runs |
| **Dashboarding** | Grafana or equivalent provides rich, configurable dashboards out of the box | Must build and maintain a custom dashboard |
| **Alerting** | External alerting systems (Alertmanager, Grafana alerts) with mature notification channels | Must implement alerting logic and notification delivery |
| **GitHub-only principle** | GitHub remains sole *durable pipeline state*. Metrics are operational telemetry, not pipeline state — they belong in a metrics system | Violates the "no database" principle or requires awkward GitHub-based metrics storage |
| **Cross-pipeline queries** | Natural — time-series DBs are designed for multi-dimensional queries across data points | Requires scanning GitHub comments across many work items (expensive API usage) |
| **Operational maturity** | These tools have years of production hardening | Must build and harden a new metrics system |
| **Deployment complexity** | Adds external dependency (metrics backend + dashboard) | No additional infrastructure (but more CogWorks code) |

**Rationale**: CogWorks' core competency is automated engineering pipelines, not metrics infrastructure. Purpose-built tools (Prometheus/Mimir for storage, Grafana for dashboards, Alertmanager for alerts) are excellent at exactly this problem. CogWorks records structured tracing fields through the existing tracing and OpenTelemetry path; external systems own metric derivation, aggregation, storage, and dashboards. CogWorks does not define a separate metric-sink abstraction or promise measurements not represented by exported fields.

This also clarifies the boundary of the "GitHub as sole durable state" principle: GitHub is the durable state for *pipeline execution* (work item state, audit trail, PR lifecycle). Performance metrics are *operational telemetry* — a different concern that belongs in a different system.

**Risk**: Exporter or collector unavailability must not block or fail pipeline execution. CogWorks relies on the configured tracing/export behavior and does not provide a separate metric fallback.

---

## 13. Provider-Neutral Implementation Job Runtime

**Decision**: The CogWorks orchestrator owns implementation-job lifecycle decisions and requests one ephemeral container per job through an infrastructure provider interface. The provider owns runtime-specific execution and cleanup operations. Retries within one active job reuse that job's container; a new or resumed job requests a new one.

**Rationale**: Job isolation supports context reduction and keeps runtime choices (local Docker, Kubernetes, or a cloud provider) outside CogWorks business logic. Durable state remains in the work-item and repository systems, not in the container.

**Risk**: A provider or orchestrator failure can interrupt persistence or cleanup. The orchestrator attempts durable persistence before cleanup, requests cleanup even if persistence fails, and resumes only from the last successfully persisted state.

**ADR**: [ADR-0004: Provider-Neutral Implementation Job Runtime](../adr/ADR-0004-provider-neutral-stage-attempt-runtime.md)

---

## 14. Attestation Provider vs. Inline Cryptography

**Decision**: CogWorks verifies required upstream attestations and signs its own action attestations through an attestation-provider interface. The provider uses the Vault-backed credential boundary; private key material is not exposed to pipeline logic or the LLM.

**Rationale**: This preserves a testable trust boundary without coupling business logic to a Vault protocol, signature format, or key lifecycle implementation. Signed comments are transported through the work-item interface, which does not interpret their cryptographic contents.

**Risk**: The exact signed payload, trust policy, and attestation chain format must be defined by the concrete interface contract before implementation.

**ADR**: [ADR-0005: Vault-Backed Credential and Attestation Providers](../adr/ADR-0005-credential-and-attestation-provider-boundaries.md)

---

## 15. Polling vs. Streaming for Long-Running Domain Service Operations

**Decision**: Polling initially, designed for future streaming.

| Factor | Polling | Streaming |
| -------- | --------- | ----------- |
| **Simplicity** | Simple request/response + poll endpoint | Bidirectional stream management |
| **Progress visibility** | Periodic polling for updates | Real-time progress |
| **Transport compatibility** | Works with any transport | Requires streaming-capable transport |
| **Resource usage** | Polling wastes cycles when no progress | Efficient push-based |
| **Implementation** | Simpler domain service implementation | More complex event emitting |

**Rationale**: Most domain service operations complete in seconds to minutes. Polling with a reasonable interval (e.g., every 5 seconds) is sufficient. The protocol is designed with an `operation_id` concept that can be extended to support server-sent events or bidirectional streaming when needed. Domain services are not required to implement polling — synchronous request/response is the baseline.

---

## 16. CogWorks-Mediated vs. Direct Cross-Domain Communication

**Decision**: CogWorks mediates all cross-domain interactions. Domain services never communicate directly.

| Factor | CogWorks-Mediated | Direct Domain-to-Domain |
| -------- | ------------------- | ------------------------ |
| **Coupling** | Domain services independent | Services must discover each other |
| **Observability** | All interactions visible in CogWorks audit trail | Distributed tracing needed |
| **Control** | CogWorks enforces ordering and validation | Services manage their own coordination |
| **Scalability** | CogWorks is potential bottleneck | Services scale independently |
| **Complexity** | Simpler domain services | More complex service infrastructure |

**Rationale**: Domain services should not need to know about each other. The interface registry provides the shared vocabulary, and CogWorks provides the relevant contract parameters to each service via the Extension API context. This keeps domain services simple and focused on their domain, with CogWorks handling all orchestration.

---

## 17. Deterministic vs. LLM-Based Injection Detection

**Decision**: Heuristic-only injection detection (LLM-based secondary pass deferred to future enhancement).

| Factor | Heuristic / Regex | LLM-Based |
| -------- | ------------------ | ----------- |
| **Speed** | Fast (sub-millisecond) | Slow (LLM latency) |
| **Determinism** | Deterministic — same input, same result | Non-deterministic |
| **Coverage** | Limited to known patterns | Theoretically broader |
| **Adversarial resistance** | Can be evaded by novel patterns | Better generalization |
| **Cost** | Free | Token cost per call |
| **False positives** | Pattern-dependent | May have different false positive profile |

**Rationale**: Heuristic detection (regex + known pattern matching) is applied first: it is fast, deterministic, and catches the majority of obvious injection attempts. The constitutional rules themselves are the true primary defense — even if detection misses an injection, the behavioral rules limit what the LLM can be induced to do.

**Key constraint**: Heuristic detection does not provide guarantees. The constitutional layer (system-level behavioral rules) is the true primary defense; detection is an early-warning system that enables halting before generation, not a correctness guarantee.

**LLM-based secondary pass**: An LLM-based secondary detection pass for borderline cases is *deferred as a future enhancement*. Injection Detection is classified as pure Business Logic (zero I/O), and an LLM secondary pass would require I/O, violating this boundary. If introduced in a future version, it must be delegated through an `InjectionDetector` abstraction (not inline I/O in business logic) and its non-determinism must be explicitly accepted.

---

## 18. Context Pack Loading: Architecture Node vs. Every Node

**Decision**: Context Packs are loaded once at the Architecture node and their content persists for the entire pipeline run.

| Factor | Load Once at Architecture | Load at Every Node |
| -------- | -------------------------- | --------------------- |
| **Consistency** | Same packs throughout run | Could vary per node |
| **API calls** | One load per pipeline | Multiple loads |
| **Content currency** | Based on original classification | Could reflect updates |
| **Determinism** | Deterministic per run | Could change if packs are updated mid-run |
| **Auditability** | Single recorded pack set | Multiple pack sets to audit |

**Rationale**: Packs are loaded once and remain consistent for the entire pipeline run. This ensures that the code generator and reviewer see the same domain knowledge, anti-patterns, and required artefacts. Loading packs at each node would complicate auditing (which packs were active when?) and could introduce inconsistency if a pack is updated between nodes. The classification that triggers pack loading does not change during a pipeline run, so reloading would produce the same result anyway.

**Trade-off accepted**: Required artefact declarations from newly committed packs will not take effect for in-progress pipeline runs. This is acceptable — pack updates are rare and take effect on subsequent runs.

---

## 19. Constitutional Rules: System Prompt vs. Context Injection

**Decision**: Constitutional rules are injected as a privileged system prompt component, not as a regular context item.

| Factor | System Prompt | Context Item |
| -------- | --------------- | -------------- |
| **Override resistance** | Non-overridable by design (model API) | Can be buried by subsequent context |
| **Position stability** | Always first, before any context | Depends on context assembly ordering |
| **Context window cost** | Fixed overhead per call | Counted against context budget |
| **Truncation resistance** | Not subject to truncation | Could be truncated under pressure |
| **Implementation** | Uses model API's system/user separation | Standard context assembly |

**Rationale**: The primary goal of the constitutional rules is that they cannot be overridden by external content. Placing them in the system prompt (separate from the user-role context) provides the strongest available boundary using the model API's own separation mechanism. A context-injected rule could theoretically be overridden or buried by subsequent items assembled from untrusted sources. System prompt placement is also not subject to context truncation — the rules are always present in full.

**Key constraint**: Constitutional rules token cost is not counted against the per-call context budget. They are overhead the system must absorb. This means effective context budget = model_context_window - constitutional_rules_tokens - output_reservation_tokens.

---

## 20. Repository-Configured Pipeline vs. Harness-Sourced Stage Graph

**Decision**: The stage graph is resolved from the pinned harness for the work item's `(tier, discipline)` pair. The target repository does not supply a pipeline configuration file and cannot define, extend or override the graph.

| Factor | Repository-Configured | Harness-Sourced |
| -------- | ---------------------- | ---------------- |
| **Flexibility** | Each repository shapes its own graph | Graph varies by tier and discipline, not by repository |
| **Rigor follows risk** | A repository can shape its graph independently of its tier | A tier's stages are a property of the tier |
| **Review surface** | Graph changes are reviewed per repository, by that repository's reviewers | Graph changes are reviewed once, by the harness maintainers |
| **Rollout** | Each repository adopts changes on its own schedule | Versioned and pinned; changes roll out by canary |
| **Validation cost** | Structural validation per repository configuration | Structural validation plus stage-type registry check on the resolved graph |
| **Testing surface** | Combinatorial across repository configurations | Bounded by the tier and discipline matrix |

**Rationale**: The stage graph decides which assurance stages run and which gates apply, so it is a risk control rather than a convenience. Sourcing it from the pinned harness keeps that control with the harness maintainers and makes a tier's stage set a property of the tier instead of a property of whoever configured the repository. A repository that could define its own graph could omit stages its tier requires, which matters most for the control-plane repositories that are mapped to the higher tiers — including this one. Pinning gives each repository a stable, explicitly adopted version, so a harness change cannot alter a repository's behaviour until its pin moves.

**Risk**: Every run depends on the harness being resolvable at the pinned version, and a repository with a genuine need the harness does not cover has to change the harness rather than its own configuration. The first is handled by failing closed — an unresolvable or withdrawn pin halts the run rather than falling back to a default. The second is intentional: it routes the need through the harness review that a risk control warrants.

---


**ADR**: [ADR-0006](../adr/ADR-0006-a-node-is-a-stage-type-profiles-and-graphs-from-the-harness.md)
## 21. Pipeline State Persistence: GitHub Issue Comments vs. Local File

**Decision**: Pipeline execution state is persisted as a structured JSON comment on the GitHub issue, not as a local file in the working directory.

| Factor | GitHub Issue Comment | Local File |
| -------- | --------------------- | ------------ |
| **Durability** | Survives process crash and host loss | Lost if host dies |
| **Visibility** | Readable by humans and other tools | Requires file system access |
| **Recoverability** | Any host can resume from last comment | Requires same host or shared storage |
| **Latency** | API call per state write | Local file write (faster) |
| **Rate limits** | Counts against GitHub API budget | No API cost |
| **Audit trail** | Built-in (comment history) | Must be separately captured |

**Rationale**: The system's design principle is "GitHub is the sole durable state store" (no external databases, no persistent local state that isn't reproducible). Writing pipeline state to the issue ensures that a crash-and-resume on a different host can pick up exactly where the previous run left off. The latency cost of an API call per node transition is acceptable because node execution itself (LLM calls, domain service operations) dominates wall-clock time. State writes are infrequent (once per node boundary) so rate-limit impact is minimal.

