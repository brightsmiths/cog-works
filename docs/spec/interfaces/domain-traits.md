# Domain Service, Knowledge & LLM Provider Traits — Interface Specification

**Architectural Layer**: Domain trait definitions (in `pipeline`) + Infrastructure stubs
**Module Paths**:

- `crates/pipeline/src/domain_services.rs` — `DomainServiceClient`, `InterfaceRegistryLoader`, `ScenarioExecutor`, `TwinProvisioner` + data types
- `crates/pipeline/src/knowledge.rs` — `SummaryCache`, `PipelineConfigurationLoader`, `ToolProfileStore`, `AdapterSpecLoader` + data types
- `crates/pipeline/src/tools.rs` — `LlmProvider` + LLM call types
- `crates/extension-api/src/lib.rs` — `ExtensionApiClient` implementing `DomainServiceClient`
- `crates/llm/src/lib.rs` — `AnthropicProvider` implementing `LlmProvider`

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
    ScenarioExecutor ←──────── (pipeline-internal impl or infra crate; wired in cli)
    TwinProvisioner ←────────── ExtensionApiClient  (extension-api)
    SummaryCache ←────────────── (GitHub comment cache; wired in cli)
    PipelineConfigurationLoader ← (TOML reader; wired in cli)
    ToolProfileStore ←─────────── (TOML reader; wired in cli)
    AdapterSpecLoader ←──────────── (JSON file reader; wired in cli)
    LlmProvider ←────────────────── AnthropicProvider  (llm)
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
| `NodeId`, `PipelineName`, `ProfileName`, `SkillName`, `ToolName` | `shared-types.md` |
| `CommitSha`, `TokenCount`, `SatisfactionScore`, `ApiVersion` | `shared-types.md` |
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

Result of running scenarios against a digital twin.

```rust
pub struct SimulationResults {
    pub scenarios_executed: u32,
    pub scenarios_passed: u32,
    pub diagnostics: Diagnostics,
    pub detail: serde_json::Value,  // domain-specific; treat as opaque
}
```

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

### Scenario / TrajectoryResult / AcceptanceCriteria / SatisfactionDetermination

Used by `ScenarioExecutor`.

```rust
pub struct Scenario {
    pub id: String,
    pub description: String,
    pub input_artifacts: Vec<ArtifactPath>,
    pub hold_out_artifacts: Vec<ArtifactPath>,
    pub acceptance_criteria: AcceptanceCriteria,
}

pub struct TrajectoryResult {
    pub scenario_id: String,
    pub passed: bool,
    pub satisfaction_score: SatisfactionScore,
    /// When `true` this trajectory was expected to fail (explicit-failure scenario).
    /// A failed trajectory with `expected_failure: true` does not reduce overall
    /// satisfaction — it is counted as passing for scoring purposes.
    pub expected_failure: bool,
    pub diagnostics: Diagnostics,
}

pub struct AcceptanceCriteria {
    pub min_satisfaction_score: SatisfactionScore,
    pub required_behaviors: Vec<String>,
    pub prohibited_behaviors: Vec<String>,
}

pub enum SatisfactionDetermination {
    Satisfied { score: SatisfactionScore },
    NotSatisfied { score: SatisfactionScore, failing_criteria: Vec<String> },
}
```

### TwinHandle / TwinSpec / FailureProfile / FailureInjection

Used by `TwinProvisioner`.

```rust
pub struct TwinHandle {
    pub id: String,
    pub service: DomainServiceName,
}

pub struct TwinSpec {
    pub service: DomainServiceName,
    pub config: serde_json::Value,  // domain-specific; treat as opaque
}

pub struct FailureProfile {
    pub inject_failures: Vec<FailureInjection>,
}

pub struct FailureInjection {
    pub operation: String,
    pub failure_rate: f32,  // [0.0, 1.0]
    pub error_message: String,
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
| `ScenarioError` | `LoadFailed`, `ExecutionFailed` |
| `TwinError` | `StartFailed`, `StopFailed`, `ConfigurationFailed`, `NotRunning` |

All infrastructure error types implement `retry_policy() -> RetryPolicy`.

### DomainServiceClient Trait

```rust
#[async_trait]
pub trait DomainServiceClient: Send + Sync {
    async fn handshake(&self) -> Result<HandshakeResult, DomainServiceError>;
    async fn validate(&self, artifacts: &[ArtifactPath]) -> Result<ValidationResult, DomainServiceError>;
    async fn normalise(&self, artifacts: &[ArtifactPath]) -> Result<NormaliseResult, DomainServiceError>;
    async fn review_rules(&self, artifacts: &[ArtifactPath]) -> Result<Diagnostics, DomainServiceError>;
    async fn simulate(&self, spec: &TwinSpec, scenarios: &[Scenario]) -> Result<SimulationResults, DomainServiceError>;
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

### ScenarioExecutor Trait

```rust
#[async_trait]
pub trait ScenarioExecutor: Send + Sync {
    async fn load_scenarios(&self, scenario_dir: &Path) -> Result<Vec<Scenario>, ScenarioError>;
    async fn execute_trajectory(
        &self,
        scenario: &Scenario,
        generated_artifacts: &[(ArtifactPath, String)],
    ) -> Result<TrajectoryResult, ScenarioError>;
    fn evaluate_acceptance(
        &self,
        results: &[TrajectoryResult],
        criteria: &AcceptanceCriteria,
    ) -> SatisfactionDetermination;
}
```

**Context holdout**: Callers (the context assembler) MUST withhold
`scenario.hold_out_artifacts` from code generation context.

### TwinProvisioner Trait

```rust
#[async_trait]
pub trait TwinProvisioner: Send + Sync {
    async fn start_twin(&self, spec: &TwinSpec) -> Result<TwinHandle, TwinError>;
    async fn stop_twin(&self, handle: &TwinHandle) -> Result<(), TwinError>;
    async fn configure_failure_injection(&self, handle: &TwinHandle, profile: &FailureProfile) -> Result<(), TwinError>;
    async fn reset_twin_state(&self, handle: &TwinHandle) -> Result<(), TwinError>;
}
```

---

## Part 2 — Knowledge & Configuration Traits (`pipeline/src/knowledge.rs`)

### SummaryLevel

```rust
pub enum SummaryLevel {
    OneLine = 0,
    Paragraph = 1,
    FullInterface = 2,
    Source = 3,
}
```

Ordinal values allow comparison: a higher value means more detail.

### PyramidSummary

```rust
pub struct PyramidSummary {
    pub path: ArtifactPath,
    pub level: SummaryLevel,
    pub content: String,
    pub commit_sha: CommitSha,
    pub token_count: TokenCount,
}
```

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

### ToolProfile

```rust
pub struct ToolProfile {
    pub name: ProfileName,
    pub node_id: Option<NodeId>,
    pub allowed_tools: Vec<ToolName>,
    pub allowed_skills: Vec<SkillName>,
    pub scope_parameters: ScopeParameters,
}
```

### ToolOverrides

Layered on top of a base `ToolProfile` for node-specific customisation.

```rust
pub struct ToolOverrides {
    pub additional_tools: Vec<ToolName>,
    pub removed_tools: Vec<ToolName>,
    pub scope_overrides: Option<ScopeParameters>,
}
```

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
| `ProfileError` | `LoadFailed`, `NotFound` |
| `SpecError` | `LoadFailed`, `NotFound` |

### SummaryCache Trait

```rust
#[async_trait]
pub trait SummaryCache: Send + Sync {
    async fn get_summary(&self, path: &ArtifactPath, level: SummaryLevel) -> Result<Option<PyramidSummary>, CacheError>;
    async fn is_stale(&self, path: &ArtifactPath, current_sha: &CommitSha) -> Result<bool, CacheError>;
    async fn invalidate(&self, path: &ArtifactPath) -> Result<(), CacheError>;
}
```

**Staleness**: `is_stale` compares `current_sha` against the SHA stored in the
cached `PyramidSummary`. A missing entry is always treated as stale.

### PipelineConfigurationLoader Trait

```rust
#[async_trait]
pub trait PipelineConfigurationLoader: Send + Sync {
    async fn load_pipeline_config(&self, working_dir: &Path) -> Result<PipelineConfiguration, ConfigError>;
    fn get_named_pipeline<'a>(&self, config: &'a PipelineConfiguration, name: &PipelineName) -> Option<&'a PipelineGraph>;
    fn get_default_pipeline(&self, config: &PipelineConfiguration) -> PipelineGraph;
}
```

**Fallback**: `ConfigError::NotFound` is expected for repos with no
`.cogworks/pipeline.toml`. Callers should call `get_default_pipeline` in that case.

**Validation**: Implementations MUST call `validate_pipeline_graph` after parsing.
An invalid graph produces `ConfigError::InvalidConfiguration`.

### ToolProfileStore Trait

```rust
#[async_trait]
pub trait ToolProfileStore: Send + Sync {
    async fn load_profiles(&self) -> Result<Vec<ToolProfile>, ProfileError>;
    async fn get_node_profile(&self, node_id: &NodeId) -> Result<ToolProfile, ProfileError>;
    async fn get_node_overrides(&self, node_id: &NodeId) -> Result<Option<ToolOverrides>, ProfileError>;
    async fn get_default_profiles(&self) -> Result<Vec<ToolProfile>, ProfileError>;
}
```

### AdapterSpecLoader Trait

```rust
#[async_trait]
pub trait AdapterSpecLoader: Send + Sync {
    async fn load_spec(&self, service_name: &DomainServiceName) -> Result<ApiSpec, SpecError>;
    async fn list_specs(&self) -> Result<Vec<SpecInfo>, SpecError>;
}
```

---

## Part 3 — LLM Provider Trait (`pipeline/src/tools.rs`)

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
}
```

**Retry policy**: `RateLimited` and `NetworkError` and `ApiError(5xx)` are
`Retryable`; all others are `NonRetryable`.

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

**Transport selection**: configured per domain service in `.cogworks/services.toml`.
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

### `llm/src/lib.rs` — AnthropicProvider

```rust
pub struct AnthropicConfig {
    api_key: String,   // NEVER logged; Debug impl redacts this field
    pub base_url: String,
}

pub struct AnthropicProvider {
    config: AnthropicConfig,
    client: reqwest::Client,
}
```

`AnthropicProvider` implements `pipeline::LlmProvider`.
All method bodies are `todo!()`.

**Rate-limit headers**: `x-ratelimit-requests-remaining` and
`x-ratelimit-requests-reset` are read from each response to produce
`LlmError::RateLimited` with an accurate `retry_after` duration.

---

## Implementation Notes

- All method bodies in infrastructure stubs are `todo!()`.
- `ScopeParameters` is also consumed by `validate_tool_scope`;
  it is defined here in `knowledge.rs` and re-exported from `pipeline`.
- `ToolProfile` and the profile store are consumed by the `nodes` crate
  for per-node capability gating.
- `PyramidSummary.token_count` is used by the context assembler to account
  for tokens before choosing the finest summary level that fits the budget.
- The `Diagnostics.has_blocking()` helper avoids callers needing to import and
  match on `DiagnosticSeverity` directly.

---

## Related Documents

- `docs/spec/interfaces/shared-types.md` — shared type primitives
- `docs/spec/interfaces/pipeline-graph.md` — graph model referenced by `PipelineConfigurationLoader`
- `docs/spec/interfaces/github-traits.md` — GitHub traits
- `docs/spec/interfaces/security.md` — will reference `ScopeParameters`, `ToolProfile`
- `docs/spec/interfaces/context.md` — will reference `SummaryCache`, `PyramidSummary`, `SummaryLevel`
- `docs/spec/interfaces/pipeline-execution.md` — will reference `DomainServiceClient`
- `docs/spec/interfaces/infrastructure.md` — full infrastructure implementation contracts
