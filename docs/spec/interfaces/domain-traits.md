# Domain Service, Knowledge & LLM Provider Traits — Interface Specification

**Architectural Layer**: Domain trait definitions (in `pipeline`) + Infrastructure stubs
**Module Paths**:

- `crates/pipeline/src/domain_services.rs` — `DomainServiceClient`, `InterfaceRegistryLoader` + data types
- `crates/pipeline/src/harness.rs` — `HarnessLoader`, `RiskMapLoader` + data types
- `crates/pipeline/src/tools.rs` — `LlmProvider` + LLM call types
- `crates/extension-api/src/lib.rs` — `ExtensionApiClient` implementing `DomainServiceClient`
- `crates/llm/src/lib.rs` — `ModelGatewayProvider` implementing `LlmProvider`

**Specification Version**: 1.0

---

## Overview

This document specifies all traits and supporting data types.
These traits complete the "port" layer in `pipeline`: the full set of abstractions
the orchestration business logic depends on.

```
pipeline (trait definitions)
    DomainServiceClient ←──── ExtensionApiClient  (extension-api)
    InterfaceRegistryLoader ← (config adapter; wired in cli)
    HarnessLoader ←──────────── PinnedHarnessLoader  (wired in cli)
    RiskMapLoader ←──────────── GithubRiskMapLoader  (wired in cli)
    InterfaceRegistryLoader ←─── TomlInterfaceRegistryLoader  (wired in cli)
    AdapterSpecLoader ←──────────── (JSON file reader; wired in cli)
    LlmProvider ←────────────────── ModelGatewayProvider  (llm)
```

**Architectural rules** (from `docs/spec/constraints.md`):

- `pipeline` declares traits; it never depends on `extension-api`, `llm`, or any I/O crate.
- Infrastructure crates implement traits but must not add domain rules.
- `cli` is the only crate that constructs concrete instances and wires them together.
- Domain services are external processes — CogWorks MUST NOT contain domain-specific logic.

---

## Dependencies

| This module uses | From spec |
|-----------------|-----------|
| `ArtifactPath`, `DomainServiceName`, `InterfaceId` | `shared-types.md` |
| `NodeId`, `StageTypeId`, `ProfileId`, `HarnessVersion`, `DisciplineId` | `shared-types.md` |
| `CommitSha`, `TokenCount`, `AlignmentScore`, `ApiVersion` | `shared-types.md` |
| `Diagnostic`, `DiagnosticSeverity` | `shared-types.md` |
| `RetryPolicy` | `shared-types.md` |
| `PipelineConfiguration`, `PipelineGraph` | `pipeline-graph.md` |

---

## Part 1 — Domain Service Traits (`pipeline/src/domain_services.rs`)

### Diagnostics

A collection of [`Diagnostic`] items returned by domain service operations.

```rust
pub struct Diagnostics {
    pub items: Vec<Diagnostic>,
}
```

Helper methods: `empty()`, `is_empty()`, `len()`, `has_blocking() -> bool`.

### NormaliseResult

Result of a domain-service normalisation pass.

```rust
pub struct NormaliseResult {
    pub modified_files: Vec<ArtifactPath>,
    pub diagnostics: Diagnostics,
}
```

### SimulationResults

Result of running a domain service's tests or simulations.

```rust
pub struct SimulationResults {
    pub executed: u32,
    pub passed: u32,
    pub diagnostics: Diagnostics,
    pub detail: serde_json::Value,  // domain-specific; treat as opaque
}
```

These results inform a stage attempt. They are not a gate verdict: whether a
change passes is computed by the gate service from a commit SHA.

### DependencyResult

Result of dependency validation.

```rust
pub struct DependencyResult {
    pub satisfied: bool,
    pub missing_deps: Vec<String>,
    pub diagnostics: Diagnostics,
}
```

### InterfaceMap

Contract definitions extracted from generated artifacts.

```rust
pub struct InterfaceMap {
    pub entries: Vec<InterfaceDefinition>,
}
```

### DependencyGraph

Directed graph of domain dependencies. Nodes represent artifact paths
(module/package identifiers as repo-relative paths); edges represent
`(dependency, dependent)` pairs.

```rust
pub struct DependencyGraph {
    pub nodes: Vec<ArtifactPath>,
    pub edges: Vec<(ArtifactPath, ArtifactPath)>,  // (dependency, dependent)
}
```

### HealthStatus

Health response from a domain service.

```rust
pub enum HealthStatus {
    Healthy,
    Degraded { message: String },
    Unhealthy { message: String },
}
```

### InterfaceDefinition

A single interface contract definition. Used by both the human-authored registry
and the domain service extraction output.

```rust
pub struct InterfaceDefinition {
    pub id: InterfaceId,
    pub domain: DomainServiceName,
    pub schema: serde_json::Value,
    pub artifact_types: Vec<String>,
    pub version: ApiVersion,
}
```

### ValidationResult

Result of validating an `InterfaceDefinition` against its schema.

```rust
pub struct ValidationResult {
    pub valid: bool,
    pub diagnostics: Diagnostics,
}
```

### HandshakeResult

Capability metadata returned after the initial Extension API handshake.

```rust
pub struct HandshakeResult {
    pub domain: DomainServiceName,
    pub api_version: ApiVersion,
    pub capabilities: Vec<String>,    // e.g. "validate", "simulate"
    pub artifact_types: Vec<String>,  // e.g. "rust/rs"
    pub interface_types: Vec<String>, // e.g. "rust/trait"
}
```

### Error Types

| Type | Variants |
|------|---------|
| `DomainServiceError` | `ConnectionFailed`, `RequestFailed`, `ProtocolError`, `HandshakeFailed`, `ServiceUnavailable` |
| `RegistryError` | `LoadFailed`, `SchemaInvalid`, `NotFound` |

All infrastructure error types implement `retry_policy() -> RetryPolicy`.

### DomainServiceClient Trait

```rust
#[async_trait]
pub trait DomainServiceClient: Send + Sync {
    async fn handshake(&self) -> Result<HandshakeResult, DomainServiceError>;
    async fn validate(&self, artifacts: &[ArtifactPath]) -> Result<ValidationResult, DomainServiceError>;
    async fn normalise(&self, artifacts: &[ArtifactPath]) -> Result<NormaliseResult, DomainServiceError>;
    async fn review_rules(&self, artifacts: &[ArtifactPath]) -> Result<Diagnostics, DomainServiceError>;
    async fn simulate(&self, filter: &SimulationFilter) -> Result<SimulationResults, DomainServiceError>;
    async fn validate_deps(&self, artifacts: &[ArtifactPath]) -> Result<DependencyResult, DomainServiceError>;
    async fn extract_interfaces(&self, artifacts: &[ArtifactPath]) -> Result<InterfaceMap, DomainServiceError>;
    async fn dependency_graph(&self, artifacts: &[ArtifactPath]) -> Result<DependencyGraph, DomainServiceError>;
    async fn health_check(&self) -> Result<HealthStatus, DomainServiceError>;
}
```

**Preconditions**: `handshake` must succeed before any other method is called.
Implementations may enforce this with a runtime panic or internal state flag.

**Constraint** (from `docs/spec/constraints.md`): CogWorks MUST NOT contain
domain-specific logic. All domain operations are delegated here.

### InterfaceRegistryLoader Trait

```rust
#[async_trait]
pub trait InterfaceRegistryLoader: Send + Sync {
    async fn load_definitions(&self) -> Result<Vec<InterfaceDefinition>, RegistryError>;
    fn validate_schema(&self, definition: &InterfaceDefinition) -> Result<ValidationResult, RegistryError>;
}
```

**Constraint**: CogWorks MUST NOT create or modify interface definitions autonomously.
This trait is read-only.

### ScopeParameters

```rust
pub struct ScopeParameters {
    pub max_file_changes: u32,             // 0 = unlimited
    pub allowed_artifact_patterns: Vec<String>,
    pub prohibited_artifact_patterns: Vec<String>,
    pub max_new_files: u32,                // 0 = no new files allowed
}
```

**Security**: `prohibited_artifact_patterns` takes precedence over
`allowed_artifact_patterns` when both match the same path.

### SpecInfo / OperationSpec / ApiSpec

```rust
pub struct SpecInfo {
    pub title: String,
    pub version: ApiVersion,
    pub description: String,
    pub service_name: DomainServiceName,
}

pub struct OperationSpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
}

pub struct ApiSpec {
    pub service_name: DomainServiceName,
    pub info: SpecInfo,
    pub operations: Vec<OperationSpec>,
}
```

### Error Types

| Type | Variants |
|------|---------|
| `CacheError` | `Unavailable`, `SerialisationError` |
| `ConfigError` | `NotFound`, `ParseError`, `InvalidConfiguration` |

### HarnessLoader Trait

```rust
#[async_trait]
pub trait HarnessLoader: Send + Sync {
    async fn read_pin(&self, repository: &RepositoryId) -> Result<HarnessPin, HarnessError>;
    async fn resolve(&self, pin: &HarnessPin) -> Result<ResolvedHarness, HarnessError>;

    fn stage_registry(&self, harness: &ResolvedHarness) -> &StageTypeRegistry;
    fn stage_graph(&self, harness: &ResolvedHarness, tier: Tier, discipline: &DisciplineId)
        -> Result<PipelineGraph, HarnessError>;
    fn resolve_profile(
        &self,
        harness: &ResolvedHarness,
        stage_type: &StageTypeId,
        discipline: &DisciplineId,
        toolchain: &str,
        tier: Tier,
    ) -> Result<(ProfileRef, StageContract), HarnessError>;
    fn context_packs(&self, harness: &ResolvedHarness, triggers: &ContextPackTrigger)
        -> Vec<ContextPack>;
    fn constitutional_rules(&self, harness: &ResolvedHarness)
        -> Result<ConstitutionalRules, HarnessError>;
}
```

**The only source of agent-shaping configuration.** Stage graphs, stage
contracts, agent profiles, prompt templates, output schemas, Context Packs and
constitutional rules all come through here.

**Read-only.** There is no write method, and adding one would breach
`docs/spec/constraints.md` §Module Boundaries.

**Deterministic resolution.** `resolve_profile` is a pure function of its
arguments at a given resolved harness. No model participates, and no model output
is an argument.

**No fallback.** Every failure mode halts the run:

| Error | Condition |
|-------|-----------|
| `PinUnreadable` | Pin file absent or malformed |
| `VersionNotFound` | Pin names a version that does not resolve |
| `VerificationFailed` | Resolved content does not match what the pin names |
| `SourceUnavailable` | Harness source unreachable (retriable) |
| `NoGraphForTier` | No stage graph for the `(tier, discipline)` pair |
| `NoProfileForKey` | No profile for the resolution key |

There is deliberately no `get_default_graph` and no `get_default_profile`. A run
that cannot establish what governs it does not run, because running a stage under
an approximately-right profile means an agent operating under a contract nobody
chose for it.

### RiskMapLoader Trait

```rust
#[async_trait]
pub trait RiskMapLoader: Send + Sync {
    async fn load_risk_map(&self, repository: &RepositoryId, git_ref: &str)
        -> Result<RiskMap, RiskMapError>;
}
```

**Read-only.** The risk map is human-owned.

An absent or malformed map produces an error, never an empty map. An empty map
would derive the lowest tier for everything, silently applying the least rigour to
work nobody has assessed.

### ChatRole / ChatMessage

```rust
pub enum ChatRole { System, User, Assistant }

pub struct ChatMessage {
    role: ChatRole,     // private — immutable after construction
    content: String,    // private — immutable after construction
}
```

`ChatMessage` is intentionally immutable. Fields are private; mutation after
construction is not possible. This prevents callers from bypassing the injection
guard by modifying content after it has been validated.

Accessor methods: `role() -> &ChatRole`, `content() -> &str`.
Constructor helpers: `ChatMessage::system(content)`, `ChatMessage::user(content)`,
`ChatMessage::assistant(content)`.

### OutputSchema

JSON Schema wrapper. Only valid JSON objects are accepted.

```rust
pub struct OutputSchema(serde_json::Value);
// OutputSchema::new(value: serde_json::Value) -> Option<Self>
// OutputSchema::as_value(&self) -> &serde_json::Value
```

### ModelConfig

```rust
pub struct ModelConfig {
    pub model_id: String,
    pub context_window_size: u64,
    pub max_output_tokens: u64,
    pub input_cost_per_token: f64,
    pub output_cost_per_token: f64,
}
```

### StructuredResponse

```rust
pub struct StructuredResponse {
    pub content: serde_json::Value,   // validated against OutputSchema
    pub input_tokens: TokenCount,
    pub output_tokens: TokenCount,
    pub latency_ms: u64,
    pub schema_validated: bool,       // always true; retained for audit log
}
```

### LlmError

```rust
pub enum LlmError {
    RateLimited { retry_after: Option<Duration> },
    ApiError { status_code: u16, message: String },
    SchemaValidationFailed { content: serde_json::Value, violations: Vec<String> },
    NetworkError { message: String },
    ContextWindowExceeded { requested: TokenCount, limit: TokenCount },
    InjectionDetected { source: String, offending_text: String, pattern: InjectionPattern },
}
```

**Retry policy**: `RateLimited` and `NetworkError` and `ApiError(5xx)` are
`Retryable`; all others — including `InjectionDetected` — are `NonRetryable`
(retrying would just re-submit the same malicious content and re-trigger the
same detection).

**`InjectionDetected`** (Security Reviewer finding H2, task #59 follow-up):
raised by `nodes::LlmGateway::call` when `pipeline::detect_injection` matches
a pattern in a `ContextPackage` item's content, before the provider is ever
called. Its `Display` impl deliberately omits `offending_text` so the error
message itself does not re-amplify the injected text. Because of this field
layout (a field literally named `source` that is a plain label, not a nested
error), `LlmError` implements `Display`/`std::error::Error` by hand rather
than via `#[derive(thiserror::Error)]` — see the doc comment on the type in
`crates/pipeline/src/tools.rs` for why.

### LlmProvider Trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(
        &self,
        system_prompt: &str,
        messages: &[ChatMessage],
        schema: &OutputSchema,
        model: &ModelConfig,
    ) -> Result<StructuredResponse, LlmError>;
}
```

**Security**: Never log `messages` content — it may contain proprietary code
or sensitive issue descriptions. Log only token counts, model ID, and latency.

**Constitutional rules**: The `system_prompt` MUST contain the constitutional rules
text. The `LlmGateway` ensures this at the call site.

---

## Part 4 — Infrastructure Stubs

### `extension-api/src/lib.rs` — ExtensionApiClient

```rust
pub enum TransportKind {
    UnixSocket { path: PathBuf },
    Http { base_url: String },
}

pub struct ServiceTransportConfig {
    pub service_name: DomainServiceName,
    pub transport: TransportKind,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
}

pub struct ExtensionApiClient {
    config: ServiceTransportConfig,
}
```

`ExtensionApiClient` implements `pipeline::DomainServiceClient`.

**Transport selection**: configured per domain service in CogWorks' own process
configuration, which describes the host it runs on rather than the repository it
is working on.
Default is `UnixSocket`; `Http` is configurable.

**Protocol envelope format**:

```json
{
  "version": "1.0",
  "operation": "<op>",
  "payload": { ... }
}
```

Response adds `"status": "ok" | "error"` and optionally `"error": { ... }`.

### `llm/src/lib.rs` — ModelGatewayProvider

```rust
pub struct ModelGatewayConfig {
    pub endpoint: String,
    pub model: String,
}

pub struct ModelGatewayProvider {
    config: ModelGatewayConfig,
    client: reqwest::Client,
}
```

`ModelGatewayProvider` implements `pipeline::LlmProvider`.

**No credential field.** The gateway holds the provider keys. This type cannot
reach a model provider directly even if misconfigured, because it has nothing to
authenticate with.

**No provider name.** Which concrete model a logical name resolves to is the
gateway's decision, so no provider appears in this codebase.

**Rate-limit headers**: `x-ratelimit-requests-remaining` and
`x-ratelimit-requests-reset` are read from each response to produce
`LlmError::RateLimited` with an accurate `retry_after` duration.

---

## Implementation Notes

- All method bodies in infrastructure stubs are `todo!()`.
- `StageContract` is consumed by the `nodes` crate to bound a stage attempt. It
  is never placed in the context bundle of the stage it governs.
- The `Diagnostics.has_blocking()` helper avoids callers needing to import and
  match on `DiagnosticSeverity` directly.

---

## Related Documents

- `docs/spec/interfaces/shared-types.md` — shared type primitives
- `docs/spec/interfaces/pipeline-graph.md` — graph model resolved by `HarnessLoader`
- `docs/spec/interfaces/github-traits.md` — GitHub traits
- `docs/spec/interfaces/security.md` — trust zones, identity roles, constitutional rules
- `docs/spec/interfaces/context.md` — `ContextBundle` assembly and Context Packs
- `docs/spec/interfaces/pipeline-execution.md` — will reference `DomainServiceClient`
- `docs/spec/interfaces/infrastructure.md` — full infrastructure implementation contracts

---

## Module: `domain_services` — Routing

These types and functions are added to `crates/pipeline/src/domain_services.rs`.

### ServiceCapabilities

The cached capabilities of a registered domain service, populated after a
successful handshake.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `domain` | `DomainServiceName` | The service's own name |
| `artifact_types` | `Vec<String>` | Artifact type identifiers the service can process |
| `interface_types` | `Vec<String>` | Interface type identifiers the service can extract or validate |
| `supported_methods` | `Vec<String>` | Capability identifiers (e.g. `"validate"`, `"simulate"`) |

Derived from the [`HandshakeResult`] at startup. Cached in
`DomainServiceRegistration` to avoid repeated handshakes.

### DomainServiceRegistration

A single registered domain service entry.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `service_name` | `DomainServiceName` | Unique registration key |
| `transport_config` | `serde_json::Value` | Opaque transport configuration (socket path or HTTP URL); parsed by `extension-api` |
| `capabilities` | `Option<ServiceCapabilities>` | `None` before handshake, `Some` after |

### ServiceRegistry

The complete set of registered domain services for this pipeline run.

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `registrations` | `Vec<DomainServiceRegistration>` | All registered services |

### RoutingError

Errors returned by routing functions.

```rust
pub enum RoutingError {
    NoServiceFound { artifact_paths: Vec<ArtifactPath> },
    Ambiguous { artifact_paths: Vec<ArtifactPath>, candidates: Vec<DomainServiceName> },
    HandshakePending { service: DomainServiceName },
}
```

| Variant | Meaning |
|---------|---------|
| `NoServiceFound` | No registered service can handle the given artifact paths |
| `Ambiguous` | Multiple services claim to handle the same artifact paths with equal specificity |
| `HandshakePending` | The best-matching service has not yet completed its handshake |

### `fn select_service_for_artifacts`

```rust
pub fn select_service_for_artifacts(
    registry: &ServiceRegistry,
    artifact_paths: &[ArtifactPath],
) -> Result<DomainServiceName, RoutingError>
```

**Purpose**: Selects the single domain service that should process a given set
of artifact paths.

**Behaviour**:

1. Filters to services whose `capabilities` are `Some` (handshake complete).
2. For each candidate, scores it by the number of `artifact_types` that match
   the file extensions of `artifact_paths`.
3. If exactly one service has the highest score: returns its name.
4. If multiple services tie at the highest score: returns
   `Err(RoutingError::Ambiguous)`.
5. If no service matches: returns `Err(RoutingError::NoServiceFound)`.
6. `RoutingError::HandshakePending` is never returned by this function: step 1
   filters out `capabilities == None` candidates before scoring (step 2), so a
   service that hasn't completed its handshake can never be "the best match."
   If the only services whose `artifact_types` would otherwise match have not
   completed their handshake, the result is `NoServiceFound`, not
   `HandshakePending`. The variant remains defined on `RoutingError` for other
   potential call sites.

### `fn resolve_primary_and_secondary`

```rust
pub fn resolve_primary_and_secondary(
    registry: &ServiceRegistry,
    artifact_paths: &[ArtifactPath],
    interface_type_identifiers: &[String],
) -> Result<(DomainServiceName, Vec<DomainServiceName>), RoutingError>
```

**Purpose**: Resolves the primary domain service (for validation and normalisation)
and any secondary services (for interface extraction and dependency checks) for
a sub-work-item's artifact and interface set.

**Behaviour**:

1. Calls [`select_service_for_artifacts`] with `artifact_paths` to find the primary service.
2. Finds all other services whose `interface_types` intersect with
   `interface_type_identifiers` (for cross-domain validation).
3. Returns `(primary, secondaries)` where `secondaries` excludes the primary.

---

---

### Artifact Type Matching

Service routing matches `artifact_paths` against `artifact_types` by comparing
the file extension of each path against each artifact type identifier. The
convention is `domain/extension` (e.g. `"rust/rs"` matches `.rs` files,
`"kicad/kicad_sch"` matches `.kicad_sch` files). This convention is owned by
the domain service; CogWorks treats type identifiers as opaque strings for
matching.
