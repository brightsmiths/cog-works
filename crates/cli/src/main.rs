//! CogWorks CLI entry point.
//!
//! This binary is the composition root for the entire system. Responsibilities:
//!
//! 1. **Parse configuration** — load `.cogworks/config.toml` and/or
//!    environment variables; validate required secrets are present.
//! 2. **Wire observability** — configure `tracing-subscriber` with a JSON
//!    console layer and an OpenTelemetry OTLP exporter. All `tracing` spans and
//!    structured events emitted by every crate in the workspace flow through
//!    this composite subscriber automatically.
//! 3. **Construct infrastructure** — create concrete instances of all
//!    infrastructure types (`GithubClient`, `AnthropicProvider`,
//!    `ExtensionApiClient`, event source) and inject them into
//!    [`nodes::PipelineExecutor`].
//! 4. **Select trigger mode** — based on `CogWorksConfig.trigger_mode`:
//!    - [`TriggerMode::SingleShot`] — parse the work item ID from
//!      `--issue-url` and call `run_step` directly (Phase 1 CLI).
//!    - [`TriggerMode::Webhook`] — construct a
//!      [`listener::GitHubWebhookEventSource`] and loop on `next_event`.
//!    - [`TriggerMode::Queue`] — construct a
//!      [`listener::QueueEventSource`] and loop on `next_event`.
//!
//! ## Observability
//!
//! `init_observability` configures a composite [`tracing_subscriber`]:
//!
//! - **JSON console layer** — structured JSON events to stderr; verbosity
//!   controlled by `RUST_LOG` (default: `info`).
//! - **OpenTelemetry OTLP layer** — exports spans and events to
//!   `CogWorksConfig.otlp_endpoint` via gRPC (tonic). The backend
//!   (Prometheus, Jaeger, Grafana Tempo, etc.) is an OTel Collector concern;
//!   `cli` only configures the OTLP endpoint.
//!
//! ## Composition Root Wiring Order
//!
//! ```text
//! 1. Parse CogWorksConfig
//! 2. init_observability(config.otlp_endpoint)
//! 3. Construct GithubClient (wraps github-bot-sdk)
//! 4. Construct AnthropicProvider (wraps reqwest + Anthropic API)
//! 5. Construct ExtensionApiClient(s) from .cogworks/services.toml
//! 6. Construct in-process knowledge/config trait impls (TOML readers)
//! 7. Assemble PipelineExecutor
//! 8. Select trigger mode and enter trigger loop
//! ```
//!
//! ## Security Notes
//!
//! `CogWorksConfig.github_private_key` and `CogWorksConfig.anthropic_api_key`
//! are NEVER written to log output, tracing events, or error messages. The
//! `Debug` impl for `CogWorksConfig` manually redacts both fields.
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/infrastructure.md` §cli for the full contract.

// Stub phase: config and observability helpers are unused until the entry point
// is implemented. These attributes are removed when stubs are implemented.
#![allow(dead_code)]

use std::path::PathBuf;

use anyhow::Result;
// In opentelemetry_sdk 0.28+ (workspace pins 0.32), the concrete tracer
// provider struct is `SdkTracerProvider` in the `trace` module (renamed from
// `TracerProvider` in earlier 0.24–0.27 versions). Update this import if the
// SDK is downgraded.
use opentelemetry_sdk::trace::SdkTracerProvider;

use pipeline::{BranchName, PipelineName, QueueEventConfig, WebhookConfig};

// ─── TriggerMode ─────────────────────────────────────────────────────────────

/// How the pipeline step-function loop is driven.
///
/// Set once at startup from config / CLI flags. Determines which
/// [`pipeline::EventSource`] implementation (if any) is constructed, and
/// whether the process exits after one step (single-shot) or loops forever
/// (service mode).
///
/// See `docs/spec/interfaces/infrastructure.md` §TriggerMode.
#[derive(Debug)]
pub enum TriggerMode {
    /// Phase 1 CLI: synthesise a single [`pipeline::GitHubEvent`] from the
    /// given GitHub issue URL and call `run_step` once, then exit.
    ///
    /// The issue URL is parsed to extract the owner, repository, and issue
    /// number, which are used to construct the work item ID.
    SingleShot {
        /// Full GitHub issue URL (e.g.
        /// `https://github.com/owner/repo/issues/123`).
        issue_url: String,
    },

    /// Service mode: bind a webhook HTTP server via
    /// [`listener::GitHubWebhookEventSource`] and loop on incoming events.
    ///
    /// Requires a public HTTPS endpoint (or smee.io proxy for development).
    Webhook(WebhookConfig),

    /// Service mode: consume from a cloud queue via
    /// [`listener::QueueEventSource`] and loop on incoming messages.
    ///
    /// Azure Service Bus is available today. AWS SQS is planned in
    /// `queue-runtime`.
    Queue(QueueEventConfig),
}

// ─── CogWorksConfig ──────────────────────────────────────────────────────────

/// Top-level CLI / service configuration.
///
/// Parsed once at startup from environment variables and/or
/// `.cogworks/config.toml`. Never mutated after construction.
///
/// ## Security
///
/// `github_private_key` and `anthropic_api_key` MUST NOT be logged or
/// included in error messages. The `Debug` impl for this type is manually
/// implemented to redact those fields.
///
/// See `docs/spec/interfaces/infrastructure.md` §CogWorksConfig.
pub struct CogWorksConfig {
    /// How pipeline runs are triggered.
    pub trigger_mode: TriggerMode,

    /// Working directory for this pipeline run.
    ///
    /// Must be the root of the repository being processed. All relative paths
    /// in pipeline configuration are resolved against this directory.
    pub working_dir: PathBuf,

    /// Named pipeline to use; the default pipeline is used when `None`.
    pub pipeline_name: Option<PipelineName>,

    /// Repository branch approved for loading constitutional rules.
    ///
    /// Only the default branch (or an explicitly approved branch) is
    /// accepted. Feature branches and forks are rejected.
    pub approved_branch: BranchName,

    /// OTLP exporter endpoint for OpenTelemetry spans/events.
    ///
    /// Example: `http://localhost:4317` (gRPC). The OTel Collector must be
    /// running at this address, forwarding to the desired backend.
    pub otlp_endpoint: String,

    /// GitHub App ID.
    pub github_app_id: u64,

    /// GitHub App private key (PEM format).
    ///
    /// **Security**: NEVER log this value. Sourced from an environment
    /// variable or a secrets manager; never from a version-controlled file.
    pub github_private_key: String,

    /// Anthropic API key.
    ///
    /// **Security**: NEVER log this value. Sourced from an environment
    /// variable or a secrets manager; never from a version-controlled file.
    pub anthropic_api_key: String,

    /// Anthropic API base URL.
    ///
    /// Defaults to `https://api.anthropic.com`. Overridable to point at a
    /// proxy or test endpoint.
    pub anthropic_base_url: String,

    /// LLM model identifier (e.g. `"claude-3-5-sonnet-20241022"`).
    pub llm_model_id: String,
}

/// Redacts secrets in debug output to prevent accidental credential leakage.
impl std::fmt::Debug for CogWorksConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CogWorksConfig")
            .field("trigger_mode", &self.trigger_mode)
            .field("working_dir", &self.working_dir)
            .field("pipeline_name", &self.pipeline_name)
            .field("approved_branch", &self.approved_branch)
            .field("otlp_endpoint", &self.otlp_endpoint)
            .field("github_app_id", &self.github_app_id)
            .field("github_private_key", &"[REDACTED]")
            .field("anthropic_api_key", &"[REDACTED]")
            .field("anthropic_base_url", &self.anthropic_base_url)
            .field("llm_model_id", &self.llm_model_id)
            .finish()
    }
}

// ─── Observability ───────────────────────────────────────────────────────────

/// Initialise the global `tracing` subscriber with a JSON console layer and
/// an OpenTelemetry OTLP exporter.
///
/// Must be called **once**, before any other infrastructure is constructed,
/// so that all crates in the workspace emit spans and events through the
/// composite subscriber from the very first operation.
///
/// ## Layers
///
/// 1. **JSON console layer** (`tracing-subscriber`) — structured JSON events
///    to stderr. Verbosity is controlled by `RUST_LOG` (default: `info`).
/// 2. **OpenTelemetry OTLP layer** (`tracing-opentelemetry`) — exports spans
///    to `otlp_endpoint` using gRPC (tonic). The span exporter runs on the
///    Tokio runtime started by `#[tokio::main]`.
///
/// ## Return Value
///
/// Returns the [`SdkTracerProvider`] so the caller can invoke
/// `.shutdown()` on clean exit to flush the final batch of spans.
///
/// # Errors
///
/// Returns an error if the OTLP pipeline cannot be built (typically a
/// misconfigured `otlp_endpoint` or a missing `opentelemetry-otlp` feature).
///
/// See `docs/spec/interfaces/infrastructure.md` §Observability Wiring.
fn init_observability(_otlp_endpoint: &str) -> Result<SdkTracerProvider> {
    todo!("init_observability — see docs/spec/interfaces/infrastructure.md §Observability Wiring")
}

// ─── Configuration loading ───────────────────────────────────────────────────

/// Load and validate `CogWorksConfig` from environment variables and/or
/// `.cogworks/config.toml`.
///
/// ## Environment Variables
///
/// | Variable | Required | Description |
/// |---|---|---|
/// | `COGWORKS_GITHUB_APP_ID` | Yes | GitHub App numeric ID |
/// | `COGWORKS_GITHUB_PRIVATE_KEY` | Yes | PEM private key (never logged) |
/// | `COGWORKS_ANTHROPIC_API_KEY` | Yes | Anthropic API key (never logged) |
/// | `COGWORKS_OTLP_ENDPOINT` | No | OTLP gRPC endpoint (default: `http://localhost:4317`) |
/// | `COGWORKS_ANTHROPIC_BASE_URL` | No | API base URL (default: `https://api.anthropic.com`) |
/// | `COGWORKS_LLM_MODEL_ID` | No | Model ID (default: `claude-3-5-sonnet-20241022`) |
/// | `RUST_LOG` | No | Log verbosity filter (default: `info`) |
///
/// ## Config File
///
/// Optional `.cogworks/config.toml` provides non-secret configuration such as
/// `trigger_mode`, `working_dir`, `pipeline_name`, and `approved_branch`.
/// Environment variables take precedence over config file values for secrets.
///
/// ## Errors
///
/// Returns an error if any required environment variable is absent or if
/// the config file exists but contains invalid TOML.
///
/// See `docs/spec/interfaces/infrastructure.md` §CogWorksConfig.
fn load_config() -> Result<CogWorksConfig> {
    todo!("load_config — see docs/spec/interfaces/infrastructure.md §CogWorksConfig")
}

// ─── Entry point ─────────────────────────────────────────────────────────────

/// CogWorks entry point.
///
/// ## Startup Sequence
///
/// 1. Load [`CogWorksConfig`] via [`load_config`].
/// 2. Initialise observability via [`init_observability`].
/// 3. Construct infrastructure instances and assemble [`nodes::PipelineExecutor`].
/// 4. Select trigger mode and enter the event loop (or single-shot and exit).
///
/// ## Exit Codes
///
/// | Code | Meaning |
/// |------|---------|
/// | 0 | Pipeline step completed successfully or is waiting at a human gate |
/// | 1 | Pipeline escalated; human intervention required |
/// | 2 | Unrecoverable pipeline error |
/// | 3 | Configuration or startup failure |
///
/// See `docs/spec/interfaces/infrastructure.md` §cli.
#[tokio::main]
async fn main() -> Result<()> {
    todo!("CLI entry point — see docs/spec/interfaces/infrastructure.md §cli")
}
