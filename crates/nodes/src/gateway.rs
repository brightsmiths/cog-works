//! LLM gateway — constitutional prompt wrapping and shared rate-limit management.
//!
//! Every LLM call made during a pipeline step goes through [`LlmGateway::call`].
//! The gateway enforces two invariants:
//!
//! 1. **Constitutional position** — constitutional rules are always embedded at
//!    the privileged leading position of the system prompt. The opaque
//!    [`ConstitutionallyWrappedPrompt`] type makes it impossible to call the
//!    gateway with a prompt that skips this step.
//!
//! 2. **Shared rate-limit state** — all nodes in the same pipeline step share
//!    a single [`RateLimitState`] via `Arc<Mutex<RateLimitState>>`. This prevents
//!    any parallel node from sending requests during an active backoff window.
//!
//! ## Usage Pattern
//!
//! ```text
//! // In run_step (after constitutional rules are loaded):
//! let prompt = assemble_constitutional_prompt(&rules, NODE_SYSTEM_PROMPT);
//! let response = gateway.call(prompt, &context, &schema, &model).await?;
//! ```
//!
//! A new [`ConstitutionallyWrappedPrompt`] must be assembled for each LLM call
//! (the type is not `Clone`).
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/nodes.md` §LLM Gateway for the full contract.

use std::sync::{Arc, Mutex};

use pipeline::{
    ConstitutionalRules, ContextPackage, LlmError, LlmProvider, ModelConfig, OutputSchema,
    StructuredResponse,
};
use tracing::instrument;

// ─── ConstitutionallyWrappedPrompt ───────────────────────────────────────────

/// An LLM prompt that has been assembled with constitutional rules at the
/// privileged leading position of the system message.
///
/// The inner fields are private; the only way to construct this type is via
/// [`assemble_constitutional_prompt`]. This enforces at compile time that
/// every [`LlmGateway::call`] is preceded by constitutional rule inclusion.
///
/// The type is **not** `Clone` or `Copy` — it is consumed by `LlmGateway::call`
/// exactly once. Create a new one for each LLM call.
///
/// See `docs/spec/interfaces/nodes.md` §ConstitutionallyWrappedPrompt.
#[derive(Debug)]
pub struct ConstitutionallyWrappedPrompt {
    /// Constitutional rules content placed at the start of the system prompt.
    constitutional_content: String,
    /// Node-specific system prompt appended after the constitutional content.
    system_prompt: String,
}

impl ConstitutionallyWrappedPrompt {
    /// Returns a reference to the constitutional content portion of the prompt.
    ///
    /// Used internally by [`LlmGateway::call`] to build the system message.
    pub(crate) fn constitutional_content(&self) -> &str {
        &self.constitutional_content
    }

    /// Returns a reference to the node-specific system prompt text.
    ///
    /// Used internally by [`LlmGateway::call`] to complete the system message.
    pub(crate) fn system_prompt(&self) -> &str {
        &self.system_prompt
    }
}

// ─── RateLimitState ──────────────────────────────────────────────────────────

/// Runtime state of the LLM provider's rate-limit window.
///
/// Shared across all concurrent nodes in one pipeline step via
/// `Arc<Mutex<RateLimitState>>`. Updates are applied after every API response
/// (or 429 error) while the mutex is held.
///
/// See `docs/spec/interfaces/nodes.md` §RateLimitState.
#[derive(Debug, Clone)]
pub struct RateLimitState {
    /// Requests remaining before the current rate-limit window resets.
    ///
    /// Initialised to `u32::MAX` until the first response with rate-limit
    /// headers is received.
    pub(crate) remaining_requests: u32,

    /// Wall-clock time when the current window resets.
    ///
    /// `None` until a response with rate-limit headers is received.
    /// On a 429 response this is set to `Instant::now() + retry_after` from
    /// the `LlmError::RateLimited { retry_after }` payload.
    pub(crate) window_reset: Option<std::time::Instant>,

    /// `true` when exponential backoff is active after receiving a 429 response.
    ///
    /// While `true` and `now < window_reset`, `LlmGateway::call` returns
    /// `Err(LlmError::RateLimited { ... })` immediately without making a
    /// network call.
    pub(crate) backoff_active: bool,
}

impl Default for RateLimitState {
    fn default() -> Self {
        Self {
            remaining_requests: u32::MAX,
            window_reset: None,
            backoff_active: false,
        }
    }
}

// ─── LlmGateway ──────────────────────────────────────────────────────────────

/// Central LLM call coordinator for one pipeline step.
///
/// Wrap in `Arc` and share across parallel nodes so they all observe the same
/// rate-limit state. The gateway itself is not `Clone`; clone the `Arc`.
///
/// ## Construction
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use nodes::gateway::LlmGateway;
/// # use pipeline::LlmProvider;
/// # let provider: Arc<dyn LlmProvider> = unimplemented!();
///
/// // provider comes from the `llm` infrastructure crate
/// let gateway = Arc::new(LlmGateway::new(provider));
/// ```
///
/// See `docs/spec/interfaces/nodes.md` §LlmGateway.
pub struct LlmGateway {
    /// The LLM API provider (Anthropic, OpenAI, etc.).
    provider: Arc<dyn LlmProvider>,
    /// Shared rate-limit state across all concurrent nodes.
    rate_limit: Arc<Mutex<RateLimitState>>,
}

/// Wraps validated constitutional rules and a node-specific system prompt into
/// the opaque [`ConstitutionallyWrappedPrompt`] token required by
/// [`LlmGateway::call`].
///
/// # Arguments
///
/// * `rules` — Constitutional rules loaded from the approved branch. Must have
///   been loaded at the start of `run_step` (Step 1).
/// * `system_prompt` — Node-specific system prompt text. Appended after the
///   constitutional content in the assembled system message.
///
/// # Returns
///
/// An opaque [`ConstitutionallyWrappedPrompt`] that can only be consumed by
/// [`LlmGateway::call`].
///
/// See `docs/spec/interfaces/nodes.md` §assemble_constitutional_prompt.
pub fn assemble_constitutional_prompt(
    _rules: &ConstitutionalRules,
    _system_prompt: &str,
) -> ConstitutionallyWrappedPrompt {
    todo!("See docs/spec/interfaces/nodes.md §assemble_constitutional_prompt")
}

impl LlmGateway {
    /// Creates a new `LlmGateway` wrapping `provider`.
    ///
    /// Initialises `RateLimitState` at default (no rate limit observed yet).
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            rate_limit: Arc::new(Mutex::new(RateLimitState::default())),
        }
    }

    /// Returns a clone of the shared rate-limit state handle.
    ///
    /// The [`PipelineExecutor`](crate::executor::PipelineExecutor) uses this to
    /// share the same `RateLimitState` across all nodes in a parallel batch
    /// without cloning the gateway itself.
    pub fn rate_limit_handle(&self) -> Arc<Mutex<RateLimitState>> {
        Arc::clone(&self.rate_limit)
    }

    /// Sends the constitutional prompt with assembled context to the LLM provider.
    ///
    /// # Arguments
    ///
    /// * `prompt` — Constitutional wrapped prompt. Consumed by this call; build
    ///   a new one via [`assemble_constitutional_prompt`] for each LLM call.
    /// * `context` — Assembled context package for this node invocation.
    /// * `schema` — JSON Schema the completion must conform to.
    /// * `model` — Model configuration (model ID, context window, costs).
    ///
    /// # Errors
    ///
    /// * `LlmError::RateLimited` — Backoff is active and the window has not yet reset.
    /// * `LlmError::Timeout` — Provider did not respond within the configured deadline.
    /// * `LlmError::InvalidSchema` — The completion did not conform to `schema`.
    /// * `LlmError::ProviderError` — Unrecoverable provider-side error.
    ///
    /// See `docs/spec/interfaces/nodes.md` §LlmGateway::call.
    #[instrument(skip_all, fields(model_id = %_model.model_id))]
    pub async fn call(
        &self,
        _prompt: ConstitutionallyWrappedPrompt,
        _context: &ContextPackage,
        _schema: &OutputSchema,
        _model: &ModelConfig,
    ) -> Result<StructuredResponse, LlmError> {
        todo!("See docs/spec/interfaces/nodes.md §LlmGateway::call")
    }
}

#[cfg(test)]
#[path = "gateway_tests.rs"]
mod tests;
