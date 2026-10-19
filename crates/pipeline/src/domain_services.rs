//! Domain service traits and supporting types.
//!
//! This module defines every trait that the pipeline business logic uses to
//! interact with external domain services (e.g. Rust, KiCad, FreeCAD domain
//! service processes reached through the Extension API), the human-authored
//! interface registry, the scenario executor, and the digital-twin provisioner.
//!
//! ## Architectural Layer
//!
//! **Trait definitions (port layer).** All traits in this module live in `pipeline`
//! so business logic can depend on them without importing infrastructure. Concrete
//! implementations live in `extension-api` and other infrastructure crates.
//!
//! ## Key Design Decisions
//!
//! - Domain services are external processes — CogWorks never contains
//!   domain-specific validation logic (see `docs/spec/constraints.md`
//!   §Module Boundaries).
//! - `DomainServiceClient::handshake` must be called before any other method;
//!   implementations assert this invariant.
//! - `InterfaceRegistryLoader` loads human-authored contracts; CogWorks never
//!   creates or modifies interface definitions autonomously.
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/domain-traits.md` §Domain Service Traits for the
//! full contract.

use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ArtifactPath, DomainServiceName, InterfaceId,
    types::{ApiVersion, Diagnostic, SatisfactionScore},
};

// ─── Supporting data types ──────────────────────────────────────────────────

/// A collection of structured diagnostic messages from a domain service operation.
///
/// Wraps `Vec<Diagnostic>` to allow future addition of operation-level metadata
/// (e.g. total count, severity summary) without breaking callers.
///
/// See `docs/spec/interfaces/domain-traits.md` §Diagnostics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Diagnostics {
    /// Individual diagnostic items, ordered as returned by the domain service.
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    /// Creates an empty [`Diagnostics`] value.
    pub fn empty() -> Self {
        Self { items: Vec::new() }
    }

    /// Returns `true` if there are no diagnostic items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns the number of diagnostic items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` if any item has `DiagnosticSeverity::Blocking`.
    pub fn has_blocking(&self) -> bool {
        use crate::types::DiagnosticSeverity;
        self.items
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Blocking)
    }
}

// ---------------------------------------------------------------------------

/// Result of a normalisation pass performed by a domain service.
///
/// Normalisation re-formats and canonicalises source files according to the
/// domain's style rules (e.g. `rustfmt`, KiCad net-list normalisation).
///
/// See `docs/spec/interfaces/domain-traits.md` §NormaliseResult.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormaliseResult {
    /// Repo-relative paths of files that were modified during normalisation.
    pub modified_files: Vec<ArtifactPath>,
    /// Any warnings or informational messages produced during normalisation.
    ///
    /// Blocking diagnostics are not expected here — normalisation is not
    /// a validation pass. If the service cannot normalise a file it should
    /// return a `DomainServiceError` instead.
    pub diagnostics: Diagnostics,
}

// ---------------------------------------------------------------------------

/// Result of a simulation run against a digital twin.
///
/// Returned by `DomainServiceClient::simulate` after executing a set of
/// scenarios against a domain twin.
///
/// See `docs/spec/interfaces/domain-traits.md` §SimulationResults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResults {
    /// Total number of scenario trajectories that were executed.
    pub scenarios_executed: u32,
    /// Number of scenario trajectories that passed all acceptance criteria.
    pub scenarios_passed: u32,
    /// Diagnostic messages produced during simulated execution.
    pub diagnostics: Diagnostics,
    /// Domain-specific detail payload; schema matches the originating service's
    /// API spec. Callers should treat this as opaque JSON.
    pub detail: serde_json::Value,
}

// ---------------------------------------------------------------------------

/// Result of dependency validation against a set of artifacts.
///
/// Returned by `DomainServiceClient::validate_deps`.
///
/// See `docs/spec/interfaces/domain-traits.md` §DependencyResult.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyResult {
    /// `true` if all declared dependencies are satisfied at the validated version
    /// constraints.
    pub satisfied: bool,
    /// Human-readable descriptions of unsatisfied dependencies, if any.
    pub missing_deps: Vec<String>,
    /// Warnings or informational notes from the dependency check.
    pub diagnostics: Diagnostics,
}

// ---------------------------------------------------------------------------

/// Map of interface definitions extracted from a set of artifacts.
///
/// Returned by `DomainServiceClient::extract_interfaces`. The orchestrator
/// validates these extracted definitions against the human-authored registry
/// to detect cross-domain constraint violations.
///
/// See `docs/spec/interfaces/domain-traits.md` §InterfaceMap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceMap {
    /// All interface definitions found in the analysed artifacts.
    pub entries: Vec<InterfaceDefinition>,
}

// ---------------------------------------------------------------------------

/// Domain dependency graph for a set of artifacts.
///
/// Returned by `DomainServiceClient::dependency_graph`. Edges represent
/// `(dependency, dependent)` pairs; the first element is the lib/module
/// that is depended on and the second is the one that depends on it.
///
/// Nodes are repo-relative artifact paths (module/package identifiers).
///
/// See `docs/spec/interfaces/domain-traits.md` §DependencyGraph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyGraph {
    /// Dependency graph nodes (module/package artifact paths, repo-relative).
    pub nodes: Vec<ArtifactPath>,
    /// Directed edges: `(dependency, dependent)`.
    pub edges: Vec<(ArtifactPath, ArtifactPath)>,
}

// ---------------------------------------------------------------------------

/// Health status reported by a domain service.
///
/// Returned by `DomainServiceClient::health_check`.
///
/// See `docs/spec/interfaces/domain-traits.md` §HealthStatus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// The service is fully operational.
    Healthy,
    /// The service is operational but experiencing non-critical issues.
    Degraded {
        /// Human-readable description of the degraded condition.
        message: String,
    },
    /// The service is not able to serve requests.
    Unhealthy {
        /// Human-readable description of the failure condition.
        message: String,
    },
}

// ---------------------------------------------------------------------------

/// A single interface contract definition.
///
/// Used both by the human-authored interface registry
/// (`InterfaceRegistryLoader::load_definitions`) and by the extraction output
/// (`DomainServiceClient::extract_interfaces`). Cross-domain constraint
/// validation compares registry definitions against extracted definitions.
///
/// See `docs/spec/interfaces/domain-traits.md` §InterfaceDefinition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceDefinition {
    /// Unique identifier for this interface contract.
    pub id: InterfaceId,
    /// The domain service that owns or produced this interface definition.
    pub domain: DomainServiceName,
    /// The interface contract as a JSON Schema value. The schema version and
    /// dialect are set by the owning domain service.
    pub schema: serde_json::Value,
    /// Artifact path patterns (glob syntax) describing which files participate
    /// in this interface.
    pub artifact_types: Vec<String>,
    /// Version of the interface definition schema used to parse this entry.
    pub version: ApiVersion,
}

// ---------------------------------------------------------------------------

/// Result of validating an [`InterfaceDefinition`] against its expected schema.
///
/// Returned by `InterfaceRegistryLoader::validate_schema`.
///
/// ## Invariant
///
/// `valid` is `true` if and only if `diagnostics` contains no blocking items.
/// Use [`ValidationResult::passed`] or [`ValidationResult::failed`] — never
/// construct directly — to ensure the invariant is always upheld.
///
/// See `docs/spec/interfaces/domain-traits.md` §ValidationResult.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// `true` if the definition conforms to its declared schema and all
    /// required fields are present.
    valid: bool,
    /// Diagnostics describing any schema violations found.
    ///
    /// Empty when `valid` is `true`; non-empty (and may include blocking items)
    /// when `valid` is `false`.
    diagnostics: Diagnostics,
}

impl ValidationResult {
    /// Creates a passing result with empty diagnostics.
    #[must_use]
    pub fn passed() -> Self {
        Self {
            valid: true,
            diagnostics: Diagnostics::empty(),
        }
    }

    /// Creates a failing result with the supplied diagnostics.
    ///
    /// # Panics
    ///
    /// Panics if `diagnostics` is empty — a failing result must carry at least
    /// one diagnostic explaining why validation failed. This invariant is
    /// enforced in both debug and release builds.
    #[must_use]
    pub fn failed(diagnostics: Diagnostics) -> Self {
        assert!(
            !diagnostics.is_empty(),
            "ValidationResult::failed requires at least one diagnostic"
        );
        Self {
            valid: false,
            diagnostics,
        }
    }

    /// Returns `true` if the definition passed schema validation.
    pub fn valid(&self) -> bool {
        self.valid
    }

    /// Returns the diagnostics produced during validation.
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }
}

// ---------------------------------------------------------------------------
// Scenario types
// ---------------------------------------------------------------------------

/// A single acceptance scenario definition.
///
/// Scenarios describe a specific trajectory through the generated artifact set.
/// The orchestrator uses them as holdout during context assembly and as inputs
/// to `ScenarioExecutor::execute_trajectory` after code generation completes.
///
/// See `docs/spec/interfaces/domain-traits.md` §Scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    /// Unique scenario identifier within the work item's scenario directory.
    pub id: String,
    /// Human-readable description of what this scenario tests.
    pub description: String,
    /// Repo-relative paths of artifacts that must be available to run this
    /// scenario (e.g. input fixtures).
    pub input_artifacts: Vec<ArtifactPath>,
    /// Repo-relative paths of artifacts that must be withheld from code
    /// generation context (the "holdout" set) to prevent leakage.
    pub hold_out_artifacts: Vec<ArtifactPath>,
    /// Acceptance threshold and behavioural constraints for this scenario.
    pub acceptance_criteria: AcceptanceCriteria,
}

// ---------------------------------------------------------------------------

/// Result of executing a single [`Scenario`] trajectory.
///
/// One `TrajectoryResult` is produced per `ScenarioExecutor::execute_trajectory`
/// call. The aggregator (`compute_satisfaction`) combines multiple
/// trajectory results into a `ScenarioSatisfactionResult`.
///
/// ## Explicit-failure scenarios
///
/// When `expected_failure` is `true`, this trajectory is part of an
/// explicit-failure scenario (one designed to verify graceful degradation).
/// Satisfaction is determined differently: the trajectory "passed" when the
/// expected failure was observed — i.e. `passed == true` here means the system
/// correctly failed. If the system did *not* fail when it should have, that is
/// recorded as an explicit-failure violation in
/// `ScenarioSatisfactionResult::explicit_failure_violations`.
///
/// See `docs/spec/interfaces/domain-traits.md` §TrajectoryResult.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryResult {
    /// The `id` of the [`Scenario`] that produced this result.
    pub scenario_id: String,
    /// `true` if the trajectory satisfied all acceptance criteria.
    ///
    /// For explicit-failure trajectories (`expected_failure == true`), `true`
    /// means the system correctly produced the expected failure.
    pub passed: bool,
    /// Numeric satisfaction score in `[0.0, 1.0]` for this trajectory.
    pub satisfaction_score: SatisfactionScore,
    /// `true` when this trajectory belongs to an explicit-failure scenario.
    ///
    /// Explicit-failure scenarios verify safety properties: that the system
    /// fails gracefully in the face of specific inputs. The aggregator treats
    /// these differently from normal scenarios (see module docs in
    /// `crates/pipeline/src/scenarios.rs`).
    pub expected_failure: bool,
    /// Diagnostics produced during trajectory execution.
    pub diagnostics: Diagnostics,
}

// ---------------------------------------------------------------------------

/// Acceptance thresholds and behavioural constraints for a [`Scenario`].
///
/// See `docs/spec/interfaces/domain-traits.md` §AcceptanceCriteria.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptanceCriteria {
    /// Minimum satisfaction score required for the scenario to pass.
    pub min_satisfaction_score: SatisfactionScore,
    /// Behaviours that the generated artifact MUST exhibit. Each string is a
    /// human-readable description checked by the `ScenarioExecutor`.
    pub required_behaviors: Vec<String>,
    /// Behaviours that the generated artifact MUST NOT exhibit. Each string
    /// is a human-readable prohibition checked by the `ScenarioExecutor`.
    pub prohibited_behaviors: Vec<String>,
}

// ---------------------------------------------------------------------------

/// Overall determination from evaluating a set of [`TrajectoryResult`]s
/// against the [`AcceptanceCriteria`].
///
/// Returned by `ScenarioExecutor::evaluate_acceptance`.
///
/// See `docs/spec/interfaces/domain-traits.md` §SatisfactionDetermination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SatisfactionDetermination {
    /// All acceptance criteria were satisfied.
    Satisfied {
        /// Final aggregated satisfaction score.
        score: SatisfactionScore,
    },
    /// One or more acceptance criteria were not satisfied.
    NotSatisfied {
        /// Final aggregated satisfaction score (below threshold or zero).
        score: SatisfactionScore,
        /// Human-readable descriptions of each failing criterion.
        failing_criteria: Vec<String>,
    },
}

// ---------------------------------------------------------------------------
// Twin types
// ---------------------------------------------------------------------------

/// An opaque handle to a running digital twin process.
///
/// Returned by `TwinProvisioner::start_twin`. Callers must pass this handle to
/// `stop_twin`, `configure_failure_injection`, and `reset_twin_state`. Dropping
/// the handle without calling `stop_twin` is a resource leak — implementations
/// should log a warning if this occurs.
///
/// The fields are private to prevent accidental construction. However,
/// `TwinHandle::new` is `pub` so that the `extension-api` crate (a separate
/// crate) can create handles from provisioner responses. The restriction is
/// therefore caller convention rather than a type-system guarantee: only
/// [`TwinProvisioner::start_twin`] implementations should call
/// [`TwinHandle::new`]. Do not construct handles with fabricated values.
///
/// See `docs/spec/interfaces/domain-traits.md` §TwinHandle.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TwinHandle {
    /// Internal correlation ID assigned by the provisioner.
    id: String,
    /// The domain service that manages this twin instance.
    service: DomainServiceName,
}

impl TwinHandle {
    /// Creates a new handle. Should only be called by [`TwinProvisioner`]
    /// implementations.
    #[must_use]
    pub fn new(id: String, service: DomainServiceName) -> Self {
        Self { id, service }
    }

    /// Returns the internal correlation ID assigned by the provisioner.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the domain service that manages this twin instance.
    pub fn service(&self) -> &DomainServiceName {
        &self.service
    }
}

// ---------------------------------------------------------------------------

/// Specification for launching a digital twin process.
///
/// Passed to `TwinProvisioner::start_twin`.
///
/// See `docs/spec/interfaces/domain-traits.md` §TwinSpec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwinSpec {
    /// The domain service responsible for provisioning this twin.
    pub service: DomainServiceName,
    /// Domain-specific configuration payload. The schema is defined by the
    /// service's Extension API spec; callers treat this as opaque JSON.
    pub config: serde_json::Value,
}

// ---------------------------------------------------------------------------

/// Specification for fault injection into a running digital twin.
///
/// Passed to `TwinProvisioner::configure_failure_injection`.
///
/// See `docs/spec/interfaces/domain-traits.md` §FailureProfile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureProfile {
    /// Individual fault injections to apply to the twin.
    pub inject_failures: Vec<FailureInjection>,
}

// ---------------------------------------------------------------------------

/// A single fault-injection directive.
///
/// ## Invariant
///
/// `failure_rate` is always within `[0.0, 1.0]`. Use [`FailureInjection::new`]
/// which returns `None` for out-of-range values.
///
/// See `docs/spec/interfaces/domain-traits.md` §FailureInjection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureInjection {
    /// The domain operation name to inject faults into (service-specific).
    operation: String,
    /// Probability of injecting a failure on each invocation, in `[0.0, 1.0]`.
    ///
    /// `0.0` disables injection; `1.0` causes every invocation to fail.
    failure_rate: f32,
    /// Error message to use for injected failures.
    error_message: String,
}

impl FailureInjection {
    /// Creates a new fault-injection directive.
    ///
    /// Returns `None` if `failure_rate` is outside `[0.0, 1.0]`.
    #[must_use]
    pub fn new(operation: String, failure_rate: f32, error_message: String) -> Option<Self> {
        if !(0.0_f32..=1.0_f32).contains(&failure_rate) {
            return None;
        }
        Some(Self {
            operation,
            failure_rate,
            error_message,
        })
    }

    /// Returns the domain operation name targeted by this injection.
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// Returns the failure probability, guaranteed to be in `[0.0, 1.0]`.
    pub fn failure_rate(&self) -> f32 {
        self.failure_rate
    }

    /// Returns the error message used for injected failures.
    pub fn error_message(&self) -> &str {
        &self.error_message
    }
}

// ---------------------------------------------------------------------------
// Handshake
// ---------------------------------------------------------------------------

/// Capabilities and metadata returned by a domain service during the initial
/// handshake.
///
/// CogWorks performs a handshake before any other domain service operation to
/// verify compatibility and discover capabilities. The result is cached in
/// `DomainServiceRegistration` (defined in `ServiceRegistry`).
///
/// See `docs/spec/interfaces/domain-traits.md` §HandshakeResult.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResult {
    /// The domain service's own name (must match the registration key).
    pub domain: DomainServiceName,
    /// Extension API version supported by this service.
    pub api_version: ApiVersion,
    /// Capability identifiers supported by this service
    /// (e.g. `"validate"`, `"simulate"`, `"dependency_graph"`).
    pub capabilities: Vec<String>,
    /// Artifact type identifiers this service can process
    /// (e.g. `"rust/source"`, `"kicad/schematic"`).
    pub artifact_types: Vec<String>,
    /// Interface type identifiers this service can extract or validate
    /// (e.g. `"rust/trait"`, `"openapi/path"`).
    pub interface_types: Vec<String>,
}

// ─── Error types ───────────────────────────────────────────────────────────

/// Errors returned by [`DomainServiceClient`] operations.
///
/// See `docs/spec/interfaces/domain-traits.md` §DomainServiceError.
#[derive(Debug, Error, Serialize, Deserialize)]
pub enum DomainServiceError {
    /// Transport-level connection failure (socket not found, refused, etc.).
    #[error("Connection to domain service failed: {message}")]
    ConnectionFailed {
        /// Description of the connection failure.
        message: String,
    },

    /// The service accepted the connection but returned an error response.
    #[error("Domain service request '{operation}' failed: {message}")]
    RequestFailed {
        /// The operation name that failed (e.g. `"validate"`).
        operation: String,
        /// Error message from the service.
        message: String,
    },

    /// The Extension API protocol framing was violated (unexpected envelope,
    /// malformed JSON, unsupported API version, etc.).
    #[error("Extension API protocol error: {message}")]
    ProtocolError {
        /// Description of the protocol violation.
        message: String,
    },

    /// The handshake failed (API version incompatible, domain name mismatch,
    /// required capability missing).
    #[error("Handshake with domain service '{service}' failed: {message}")]
    HandshakeFailed {
        /// The service that failed the handshake.
        service: DomainServiceName,
        /// Reason for handshake failure.
        message: String,
    },

    /// The service is registered but currently not reachable.
    #[error("Domain service '{service}' is unavailable")]
    ServiceUnavailable {
        /// The service that is unavailable.
        service: DomainServiceName,
    },
}

impl DomainServiceError {
    /// Whether this error is likely transient and safe to retry.
    pub fn retry_policy(&self) -> crate::errors::RetryPolicy {
        use crate::errors::RetryPolicy;
        match self {
            Self::ConnectionFailed { .. } | Self::ServiceUnavailable { .. } => {
                RetryPolicy::Retryable { after: None }
            }
            Self::RequestFailed { .. }
            | Self::ProtocolError { .. }
            | Self::HandshakeFailed { .. } => RetryPolicy::NonRetryable,
        }
    }
}

// ---------------------------------------------------------------------------

/// Errors returned by [`InterfaceRegistryLoader`] operations.
///
/// See `docs/spec/interfaces/domain-traits.md` §RegistryError.
#[derive(Debug, Error, Serialize, Deserialize)]
pub enum RegistryError {
    /// The registry file could not be read or parsed.
    #[error("Failed to load interface registry from '{path}': {message}")]
    LoadFailed {
        /// The file or directory path that could not be read.
        path: String,
        /// Reason for the load failure.
        message: String,
    },

    /// A registry entry violates the expected JSON schema.
    #[error("Interface definition '{id}' has an invalid schema: {message}")]
    SchemaInvalid {
        /// The interface ID with the invalid schema.
        id: InterfaceId,
        /// Description of the schema violation.
        message: String,
    },

    /// The requested interface definition was not found in the registry.
    #[error("Interface definition '{id}' not found in registry")]
    NotFound {
        /// The interface ID that was not found.
        id: InterfaceId,
    },
}

// ---------------------------------------------------------------------------

/// Errors returned by [`ScenarioExecutor`] operations.
///
/// See `docs/spec/interfaces/domain-traits.md` §ScenarioError.
#[derive(Debug, Error, Serialize, Deserialize)]
pub enum ScenarioError {
    /// Scenario files could not be read or parsed from the scenario directory.
    #[error("Failed to load scenarios from '{path}': {message}")]
    LoadFailed {
        /// The directory path that could not be read.
        path: String,
        /// Reason for the load failure.
        message: String,
    },

    /// Trajectory execution encountered a fatal error (not a scenario failure).
    #[error("Scenario '{scenario_id}' trajectory execution failed: {message}")]
    ExecutionFailed {
        /// The ID of the scenario whose execution failed.
        scenario_id: String,
        /// Reason for the execution failure.
        message: String,
    },
}

// ---------------------------------------------------------------------------

/// Errors returned by [`TwinProvisioner`] operations.
///
/// See `docs/spec/interfaces/domain-traits.md` §TwinError.
#[derive(Debug, Error, Serialize, Deserialize)]
pub enum TwinError {
    /// The twin process could not be started.
    #[error("Failed to start twin: {message}")]
    StartFailed {
        /// Reason the twin could not be started.
        message: String,
    },

    /// The twin process could not be cleanly stopped.
    #[error("Failed to stop twin: {message}")]
    StopFailed {
        /// Reason the twin could not be stopped.
        message: String,
    },

    /// Failure injection configuration could not be applied.
    #[error("Failed to configure failure injection for twin: {message}")]
    ConfigurationFailed {
        /// Reason the configuration could not be applied.
        message: String,
    },

    /// An operation was attempted on a twin handle that is no longer active.
    #[error("Twin '{handle_id}' is not running")]
    NotRunning {
        /// The handle ID that is not active.
        handle_id: String,
    },
}

// ─── Traits ────────────────────────────────────────────────────────────────

/// Client interface for interacting with a domain service over the Extension API.
///
/// Each method corresponds to one Extension API operation. Callers must call
/// `handshake` successfully before any other method. Implementations may
/// enforce this as a runtime assertion or by tracking state.
///
/// ## Architectural Layer
///
/// Trait defined in `pipeline`; implemented in `extension-api`.
///
/// ## See Also
///
/// - `docs/spec/interfaces/domain-traits.md` §DomainServiceClient
/// - `docs/spec/constraints.md` §Module Boundaries — domain services are
///   external processes; CogWorks never contains domain-specific logic.
#[async_trait]
pub trait DomainServiceClient: Send + Sync {
    /// Performs the initial handshake to verify compatibility and discover
    /// capabilities.
    ///
    /// # Returns
    /// [`HandshakeResult`] containing capabilities and API version on success.
    ///
    /// # Errors
    /// - [`DomainServiceError::HandshakeFailed`] — API version incompatible or
    ///   required capability missing.
    /// - [`DomainServiceError::ConnectionFailed`] — transport layer failure.
    async fn handshake(&self) -> Result<HandshakeResult, DomainServiceError>;

    /// Validates a set of artifacts against the domain's rules.
    ///
    /// # Returns
    /// [`Diagnostics`] listing all findings. The caller checks for blocking
    /// items and decides whether to halt or continue the pipeline.
    ///
    /// # Errors
    /// - [`DomainServiceError::RequestFailed`] — service returned an error.
    /// - [`DomainServiceError::ServiceUnavailable`] — service not reachable.
    async fn validate(
        &self,
        artifacts: &[ArtifactPath],
    ) -> Result<ValidationResult, DomainServiceError>;

    /// Normalises the formatting of a set of artifacts using the domain's
    /// canonical style rules (e.g. `rustfmt` for Rust).
    ///
    /// # Returns
    /// [`NormaliseResult`] listing modified files and any warnings.
    ///
    /// # Errors
    /// - [`DomainServiceError::RequestFailed`] — normalisation failed for at
    ///   least one file and the service chose to return an error.
    async fn normalise(
        &self,
        artifacts: &[ArtifactPath],
    ) -> Result<NormaliseResult, DomainServiceError>;

    /// Runs the domain's rule-review pass (e.g. Clippy, lint, design-rule check).
    ///
    /// # Returns
    /// [`Diagnostics`] listing all rule findings. Blocking items indicate
    /// violations that must be resolved before the pipeline can continue.
    ///
    /// # Errors
    /// - [`DomainServiceError::RequestFailed`] — rule-review could not complete.
    async fn review_rules(
        &self,
        artifacts: &[ArtifactPath],
    ) -> Result<Diagnostics, DomainServiceError>;

    /// Runs a set of scenarios against a domain digital twin.
    ///
    /// # Arguments
    /// - `spec` — configuration for the twin process to provision.
    /// - `scenarios` — the scenarios to execute against the twin.
    ///
    /// # Returns
    /// [`SimulationResults`] with per-scenario pass/fail and diagnostics.
    ///
    /// # Errors
    /// - [`DomainServiceError::RequestFailed`] — twin launch or scenario
    ///   execution failed at the infrastructure level.
    async fn simulate(
        &self,
        spec: &TwinSpec,
        scenarios: &[Scenario],
    ) -> Result<SimulationResults, DomainServiceError>;

    /// Validates that all declared dependencies are available and satisfy
    /// their version constraints.
    ///
    /// # Returns
    /// [`DependencyResult`] with a satisfied flag and list of missing
    /// dependencies.
    ///
    /// # Errors
    /// - [`DomainServiceError::RequestFailed`] — dependency check failed.
    async fn validate_deps(
        &self,
        artifacts: &[ArtifactPath],
    ) -> Result<DependencyResult, DomainServiceError>;

    /// Extracts interface definitions from generated artifacts.
    ///
    /// The orchestrator uses the extracted map to validate cross-domain
    /// constraints against the human-authored interface registry.
    ///
    /// # Returns
    /// [`InterfaceMap`] containing every interface definition found.
    ///
    /// # Errors
    /// - [`DomainServiceError::RequestFailed`] — extraction failed.
    async fn extract_interfaces(
        &self,
        artifacts: &[ArtifactPath],
    ) -> Result<InterfaceMap, DomainServiceError>;

    /// Returns the full dependency graph for a set of artifacts.
    ///
    /// # Returns
    /// [`DependencyGraph`] with nodes and directed edges.
    ///
    /// # Errors
    /// - [`DomainServiceError::RequestFailed`] — graph extraction failed.
    async fn dependency_graph(
        &self,
        artifacts: &[ArtifactPath],
    ) -> Result<DependencyGraph, DomainServiceError>;

    /// Checks whether the domain service is healthy.
    ///
    /// Used by the pipeline to determine whether to route work to this
    /// service or to report `ServiceUnavailable`.
    ///
    /// # Returns
    /// [`HealthStatus`] indicating the current service health.
    ///
    /// # Errors
    /// - [`DomainServiceError::ConnectionFailed`] — service not reachable at all.
    async fn health_check(&self) -> Result<HealthStatus, DomainServiceError>;
}

// ---------------------------------------------------------------------------

/// Loads and validates the human-authored interface registry.
///
/// The interface registry is a directory of JSON files, each containing an
/// [`InterfaceDefinition`]. CogWorks reads this registry before every pipeline
/// run to detect cross-domain constraint violations early.
///
/// ## Architectural Rule
///
/// CogWorks MUST NOT create or modify interface definitions autonomously.
/// It MAY suggest additions as recommendations for human review.
///
/// ## Architectural Layer
///
/// Trait defined in `pipeline`; implemented in the TOML/JSON file adapter in
/// `extension-api` (or a dedicated config crate in a future PR).
///
/// See `docs/spec/interfaces/domain-traits.md` §InterfaceRegistryLoader.
#[async_trait]
pub trait InterfaceRegistryLoader: Send + Sync {
    /// Loads all interface definitions from the registry source.
    ///
    /// # Returns
    /// A `Vec` of all definitions. An empty vec means the registry exists but
    /// is empty; callers should treat this as a warning.
    ///
    /// # Errors
    /// - [`RegistryError::LoadFailed`] — registry files could not be read.
    async fn load_definitions(&self) -> Result<Vec<InterfaceDefinition>, RegistryError>;

    /// Validates a single [`InterfaceDefinition`] against its declared schema.
    ///
    /// This is a pure, synchronous operation: it validates the in-memory struct
    /// without any I/O.
    ///
    /// # Returns
    /// [`ValidationResult`] with `valid: true` and empty diagnostics on success.
    ///
    /// # Errors
    /// - [`RegistryError::SchemaInvalid`] — the definition contains a structural
    ///   error that prevents validation.
    fn validate_schema(
        &self,
        definition: &InterfaceDefinition,
    ) -> Result<ValidationResult, RegistryError>;
}

// ---------------------------------------------------------------------------

/// Executes acceptance scenarios against generated artifact sets.
///
/// The scenario executor loads scenario specifications, runs trajectory
/// evaluation on generated outputs, and determines whether acceptance criteria
/// are met. It is distinct from `DomainServiceClient::simulate` (which runs
/// domain-specific simulation against a digital twin).
///
/// ## Architectural Layer
///
/// Trait defined in `pipeline`. Implementations may delegate per-scenario
/// evaluation to an LLM (for semantic checks) or to a deterministic checker
/// (for structural checks).
///
/// See `docs/spec/interfaces/domain-traits.md` §ScenarioExecutor.
#[async_trait]
pub trait ScenarioExecutor: Send + Sync {
    /// Loads all scenario definitions from a scenario directory.
    ///
    /// The directory is withheld from code generation context so code is not
    /// contaminated by the scenarios it must satisfy.
    ///
    /// # Arguments
    /// - `scenario_dir` — repo-relative path to the scenario directory for
    ///   the work item.
    ///
    /// # Returns
    /// A `Vec` of all loaded scenarios. Empty vec means no scenarios are
    /// defined.
    ///
    /// # Errors
    /// - [`ScenarioError::LoadFailed`] — directory unreadable or YAML/JSON
    ///   parse error.
    async fn load_scenarios(&self, scenario_dir: &Path) -> Result<Vec<Scenario>, ScenarioError>;

    /// Executes a single scenario trajectory against a set of generated
    /// artifacts.
    ///
    /// # Arguments
    /// - `scenario` — the scenario to evaluate.
    /// - `generated_artifacts` — `(path, content)` pairs of the artifacts
    ///   produced by code generation for this sub-work-item.
    ///
    /// # Returns
    /// [`TrajectoryResult`] with pass/fail determination and satisfaction score.
    ///
    /// # Errors
    /// - [`ScenarioError::ExecutionFailed`] — execution infrastructure failed
    ///   (not a scenario-level failure; the scenario result is not populated).
    async fn execute_trajectory(
        &self,
        scenario: &Scenario,
        generated_artifacts: &[(ArtifactPath, String)],
    ) -> Result<TrajectoryResult, ScenarioError>;

    /// Aggregates a set of trajectory results against acceptance criteria.
    ///
    /// This is a pure, synchronous aggregation — it combines results already
    /// computed by `execute_trajectory` and determines the overall pass/fail.
    ///
    /// # Arguments
    /// - `results` — trajectory results from one or more `execute_trajectory`
    ///   calls for the same scenario set.
    /// - `criteria` — the acceptance thresholds to evaluate against.
    ///
    /// # Returns
    /// [`SatisfactionDetermination::Satisfied`] if all criteria are met;
    /// [`SatisfactionDetermination::NotSatisfied`] with failing criteria
    /// otherwise.
    fn evaluate_acceptance(
        &self,
        results: &[TrajectoryResult],
        criteria: &AcceptanceCriteria,
    ) -> SatisfactionDetermination;
}

// ---------------------------------------------------------------------------

/// Provisions and manages digital twin processes for scenario simulation.
///
/// Twin processes are started by `DomainServiceClient::simulate` via this trait
/// when the simulation mode is `TwinSpec::service`-based. The provisioner
/// handles lifecycle management of the twin subprocess.
///
/// ## Architectural Layer
///
/// Trait defined in `pipeline`; implemented in `extension-api`.
///
/// See `docs/spec/interfaces/domain-traits.md` §TwinProvisioner.
#[async_trait]
pub trait TwinProvisioner: Send + Sync {
    /// Starts a domain twin process according to `spec`.
    ///
    /// # Returns
    /// [`TwinHandle`] identifying the running twin.
    ///
    /// # Errors
    /// - [`TwinError::StartFailed`] — process could not be started.
    async fn start_twin(&self, spec: &TwinSpec) -> Result<TwinHandle, TwinError>;

    /// Stops and cleans up a running twin process.
    ///
    /// After this call the [`TwinHandle`] is invalid. Subsequent calls to
    /// `configure_failure_injection` or `reset_twin_state` with the same
    /// handle will return [`TwinError::NotRunning`].
    ///
    /// # Errors
    /// - [`TwinError::StopFailed`] — process could not be stopped cleanly.
    /// - [`TwinError::NotRunning`] — handle is already inactive.
    async fn stop_twin(&self, handle: &TwinHandle) -> Result<(), TwinError>;

    /// Configures the failure injection profile for a running twin.
    ///
    /// Replaces any previously configured failure profile.
    ///
    /// # Errors
    /// - [`TwinError::ConfigurationFailed`] — profile could not be applied.
    /// - [`TwinError::NotRunning`] — handle is inactive.
    async fn configure_failure_injection(
        &self,
        handle: &TwinHandle,
        profile: &FailureProfile,
    ) -> Result<(), TwinError>;

    /// Resets the twin to its initial state, clearing any accumulated state
    /// from previous trajectories.
    ///
    /// # Errors
    /// - [`TwinError::ConfigurationFailed`] — state reset failed.
    /// - [`TwinError::NotRunning`] — handle is inactive.
    async fn reset_twin_state(&self, handle: &TwinHandle) -> Result<(), TwinError>;
}

// ─── Domain service routing ───────────────────────────────────────────────────

/// The cached capabilities of a registered domain service, populated after a
/// successful handshake.
///
/// Derived from [`HandshakeResult`] at startup and cached in
/// [`DomainServiceRegistration`] to avoid repeated handshakes during a
/// pipeline run.
///
/// See `docs/spec/interfaces/advanced-features.md` §ServiceCapabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceCapabilities {
    /// The service's own name.
    pub domain: DomainServiceName,

    /// Artifact type identifiers the service can process
    /// (e.g. `"rust/source"`, `"kicad/schematic"`).
    ///
    /// Routing uses these to match artifact paths: the type identifier
    /// convention is `domain/extension` (e.g. `"rust/source"` matches `.rs`
    /// files). The convention is owned by the domain service;
    /// CogWorks treats these strings as opaque matchers.
    pub artifact_types: Vec<String>,

    /// Interface type identifiers the service can extract or validate
    /// (e.g. `"rust/trait"`, `"openapi/path"`).
    ///
    /// Used by [`resolve_primary_and_secondary`] to find secondary services for
    /// cross-domain interface validation.
    pub interface_types: Vec<String>,

    /// Capability identifiers supported by this service
    /// (e.g. `"validate"`, `"simulate"`, `"dependency_graph"`).
    pub supported_methods: Vec<String>,
}

impl ServiceCapabilities {
    /// Constructs `ServiceCapabilities` from a [`HandshakeResult`].
    #[must_use]
    pub fn from_handshake(result: &HandshakeResult) -> Self {
        Self {
            domain: result.domain.clone(),
            artifact_types: result.artifact_types.clone(),
            interface_types: result.interface_types.clone(),
            supported_methods: result.capabilities.clone(),
        }
    }
}

// ---------------------------------------------------------------------------

/// A single registered domain service entry.
///
/// Held in a [`ServiceRegistry`]. The `capabilities` field is `None` before
/// the initial handshake completes; routing functions return
/// [`RoutingError::HandshakePending`] if they encounter a `None` here.
///
/// See `docs/spec/interfaces/advanced-features.md` §DomainServiceRegistration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainServiceRegistration {
    /// Unique registration key for this service.
    pub service_name: DomainServiceName,

    /// Opaque transport configuration — socket path or HTTP URL.
    ///
    /// Parsed and consumed by the `extension-api` crate when constructing the
    /// concrete client. CogWorks treats this as an opaque JSON blob.
    pub transport_config: serde_json::Value,

    /// Cached capabilities; `None` until the initial handshake completes.
    pub capabilities: Option<ServiceCapabilities>,
}

// ---------------------------------------------------------------------------

/// The complete set of registered domain services for one pipeline run.
///
/// Constructed by `cli` at startup from `.cogworks/services.toml` and
/// populated with capabilities after the initial handshake round.
///
/// See `docs/spec/interfaces/advanced-features.md` §ServiceRegistry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceRegistry {
    /// All registered services.
    pub registrations: Vec<DomainServiceRegistration>,
}

impl ServiceRegistry {
    /// Creates an empty registry.
    pub fn empty() -> Self {
        Self::default()
    }
}

// ---------------------------------------------------------------------------

/// Errors returned by domain service routing functions.
///
/// See `docs/spec/interfaces/advanced-features.md` §RoutingError.
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum RoutingError {
    /// No registered service can handle the given artifact paths.
    #[error("No domain service found for artifact paths: {artifact_paths:?}")]
    NoServiceFound {
        /// The artifact paths that could not be matched.
        artifact_paths: Vec<ArtifactPath>,
    },

    /// Multiple services claim to handle the same artifact paths with equal
    /// specificity, and no tie-breaking rule applies.
    #[error(
        "Ambiguous domain service routing for artifact paths {artifact_paths:?}: \
         candidates are {candidates:?}"
    )]
    Ambiguous {
        /// The artifact paths that matched multiple services.
        artifact_paths: Vec<ArtifactPath>,
        /// The services that tied for the best match score.
        candidates: Vec<DomainServiceName>,
    },

    /// The best-matching service has not yet completed its handshake.
    #[error("Domain service '{service}' handshake is still pending")]
    HandshakePending {
        /// The service that has not completed its handshake.
        service: DomainServiceName,
    },
}

// ---------------------------------------------------------------------------

/// Selects the single domain service that should process a given set of
/// artifact paths.
///
/// Considers only services whose `capabilities` are `Some` (handshake
/// complete). Scores each candidate by the number of `artifact_types` that
/// match the file extensions of `artifact_paths`. Returns the unique
/// highest-scoring service.
///
/// # Arguments
///
/// * `registry` — the service registry to search.
/// * `artifact_paths` — the artifact paths to route.
///
/// # Returns
///
/// The [`DomainServiceName`] of the unique best-matching service.
///
/// # Errors
///
/// - [`RoutingError::NoServiceFound`] — no service matched any path.
/// - [`RoutingError::Ambiguous`] — multiple services tied at the best score.
/// - [`RoutingError::HandshakePending`] — the best-matching service has not
///   completed its handshake.
///
/// # Artifact Type Matching Convention
///
/// The convention for `artifact_types` is `domain/extension` (e.g.
/// `"rust/source"` matches `.rs` files). CogWorks strips the leading `domain/`
/// component and compares the remainder against the file extension extracted
/// from each artifact path.
///
/// See `docs/spec/interfaces/advanced-features.md` §select_service_for_artifacts.
pub fn select_service_for_artifacts(
    registry: &ServiceRegistry,
    artifact_paths: &[ArtifactPath],
) -> Result<DomainServiceName, RoutingError> {
    let path_extensions: Vec<String> = artifact_paths
        .iter()
        .filter_map(|path| file_extension(path.as_str()))
        .collect();

    let scored: Vec<(&DomainServiceName, usize)> = registry
        .registrations
        .iter()
        .filter_map(|registration| registration.capabilities.as_ref())
        .map(|capabilities| {
            let score = score_capabilities(capabilities, &path_extensions);
            (&capabilities.domain, score)
        })
        .collect();

    let highest_score = scored.iter().map(|(_, score)| *score).max().unwrap_or(0);

    if highest_score == 0 {
        return Err(RoutingError::NoServiceFound {
            artifact_paths: artifact_paths.to_vec(),
        });
    }

    let top_candidates: Vec<DomainServiceName> = scored
        .into_iter()
        .filter(|(_, score)| *score == highest_score)
        .map(|(name, _)| name.clone())
        .collect();

    match top_candidates.as_slice() {
        [single] => Ok(single.clone()),
        _ => Err(RoutingError::Ambiguous {
            artifact_paths: artifact_paths.to_vec(),
            candidates: top_candidates,
        }),
    }
}

/// Extracts the file extension (without the leading `.`) from a repo-relative
/// artifact path string, or `None` if the filename has no extension.
///
/// The extension is the substring after the last `.` in the filename, so
/// multi-dot filenames (e.g. `archive.tar.gz`) yield the final component
/// (`"gz"`).
fn file_extension(path: &str) -> Option<String> {
    let filename = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let dot_index = filename.rfind('.')?;
    let extension = &filename[dot_index + 1..];
    if extension.is_empty() {
        None
    } else {
        Some(extension.to_string())
    }
}

/// Scores a single candidate's capabilities against the distinct file
/// extensions present in a path set.
///
/// Counts how many of the candidate's own declared `artifact_types` entries
/// have a matching extension among `path_extensions` (type-centric scoring —
/// see `select_service_for_artifacts` docs).
fn score_capabilities(capabilities: &ServiceCapabilities, path_extensions: &[String]) -> usize {
    capabilities
        .artifact_types
        .iter()
        .filter(|artifact_type| {
            let declared_extension = artifact_type_extension(artifact_type);
            path_extensions
                .iter()
                .any(|path_ext| path_ext.eq_ignore_ascii_case(declared_extension))
        })
        .count()
}

/// Strips the leading `domain/` component from an `artifact_types` entry,
/// returning the remainder as the extension to match.
///
/// Only the first `/`-delimited segment is stripped (e.g. `"a/b/rs"` strips
/// to `"b/rs"`, not `"rs"`).
fn artifact_type_extension(artifact_type: &str) -> &str {
    match artifact_type.split_once('/') {
        Some((_, remainder)) => remainder,
        None => artifact_type,
    }
}

// ---------------------------------------------------------------------------

/// Resolves the primary domain service and any secondary services for a
/// sub-work-item's set of artifacts and interface types.
///
/// The primary service is selected via [`select_service_for_artifacts`] from
/// `artifact_paths`. Secondary services are those whose `interface_types`
/// intersect with `interface_type_identifiers` — used for cross-domain
/// interface extraction and validation.
///
/// # Arguments
///
/// * `registry` — the service registry to search.
/// * `artifact_paths` — the artifacts produced or modified by the sub-work-item.
/// * `interface_type_identifiers` — interface type identifiers relevant to the
///   sub-work-item (e.g. `"rust/trait"`, `"openapi/path"`).
///
/// # Returns
///
/// `(primary, secondaries)` where `secondaries` excludes the primary and is
/// guaranteed to be empty if no other service handles the sub-work-item's
/// interface types.
///
/// # Errors
///
/// Propagates any error from [`select_service_for_artifacts`].
///
/// See `docs/spec/interfaces/advanced-features.md` §resolve_primary_and_secondary.
pub fn resolve_primary_and_secondary(
    registry: &ServiceRegistry,
    artifact_paths: &[ArtifactPath],
    interface_type_identifiers: &[String],
) -> Result<(DomainServiceName, Vec<DomainServiceName>), RoutingError> {
    let primary = select_service_for_artifacts(registry, artifact_paths)?;

    let secondaries: Vec<DomainServiceName> = registry
        .registrations
        .iter()
        .filter(|registration| registration.service_name != primary)
        .filter_map(|registration| {
            let capabilities = registration.capabilities.as_ref()?;
            let intersects = capabilities
                .interface_types
                .iter()
                .any(|interface_type| interface_type_identifiers.contains(interface_type));
            intersects.then(|| registration.service_name.clone())
        })
        .collect();

    Ok((primary, secondaries))
}

#[cfg(test)]
#[path = "domain_services_tests.rs"]
mod tests;
