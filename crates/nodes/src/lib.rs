//! CogWorks pipeline node implementations and LLM gateway.
//!
//! This crate provides the seven default pipeline node implementations (Intake
//! through Integration), the Spawning node, the LLM gateway that wraps all LLM
//! calls with constitutional rules and rate-limit tracking, and the
//! `PipelineExecutor` that drives the step-function loop.
//!
//! ## Architectural Layer
//!
//! **Orchestration layer.** Nodes sequence calls between business logic in the
//! [`pipeline`] crate and infrastructure traits (GitHub, LLM, domain services).
//! They contain no domain rules of their own.
//!
//! ## Module Layout
//!
//! | Module | Contents |
//! |--------|----------|
//! | [`gateway`] | [`gateway::LlmGateway`], constitutional prompt wrapping, rate-limit state |
//! | [`node`] | Node I/O types, coordinator types, step configuration |
//! | [`intake`] | [`intake::IntakeNode`] |
//! | [`architecture`] | [`architecture::ArchitectureNode`] |
//! | [`interface_design`] | [`interface_design::InterfaceDesignNode`] |
//! | [`planning`] | [`planning::PlanningNode`] |
//! | [`code_generation`] | [`code_generation::CodeGenerationNode`] |
//! | [`review`] | [`review::ReviewNode`] |
//! | [`integration`] | [`integration::IntegrationNode`] |
//! | [`spawning`] | [`spawning::SpawningNode`] |
//! | [`executor`] | [`executor::PipelineExecutor`], [`executor::StepResult`], [`executor::run_step`] |
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/nodes.md` for the full contract.

// Stub phase: all method bodies are `todo!()` so fields and variables that
// will be used in the implementation appear unused to the compiler.
// These attributes are removed when the stubs are implemented (PR implementation phase).
#![allow(dead_code, unused_variables)]

use std::fmt::Write as _;
use std::sync::Mutex;

use pipeline::OutputSchema;
use sha2::{Digest, Sha256};

pub mod architecture;
pub mod code_generation;
pub mod executor;
pub mod gateway;
pub mod intake;
pub mod integration;
pub mod interface_design;
pub mod node;
pub mod planning;
pub mod review;
pub mod spawning;
pub mod templates;

// ─── Shared internal helpers ───────────────────────────────────────────────────

/// Locks `mutex`, recovering the inner guard if the lock is poisoned.
///
/// This crate's policy for production code is that mutex poisoning (a panic
/// while a prior holder had the lock) must not cascade into a second panic
/// on the next lock attempt — the recovered guard is used as-is. This mirrors
/// the approach already used across the workspace for shared mutable state
/// (see `crates/pipeline/src/budget.rs`, `crates/llm/src/lib.rs` module docs).
/// Test code that wants poisoning to fail loudly should use
/// `.lock().expect("...")` directly instead of this helper.
pub(crate) fn lock_or_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Computes the SHA-256 digest of `data` as a 64-character lowercase hex string.
///
/// Single `nodes`-crate-wide hashing helper, used wherever a content hash is
/// needed (e.g. `ConstitutionalRules::source_hash`, `PipelineStateComment::graph_hash`).
/// Mirrors `pipeline::security`'s own private hashing helper (same algorithm);
/// that duplication is a deliberate cross-crate boundary — see
/// `.llm/findings/architecture-node.md` under "Deferred Issues" — since
/// `pipeline::security`'s helper is not exported for reuse outside that crate.
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Builds an [`OutputSchema`] from a literal, hand-written JSON schema value.
///
/// Every call site in this crate passes a `serde_json::json!` literal that is
/// structurally guaranteed to be a valid JSON object, so `OutputSchema::new`
/// cannot actually fail in practice; this helper centralises the
/// `unwrap_or_else(|| unreachable!(..))` tail that was previously repeated at
/// every schema-builder call site (`architecture.rs`, `executor/completion.rs`).
pub(crate) fn literal_output_schema(schema: serde_json::Value) -> OutputSchema {
    OutputSchema::new(schema)
        .unwrap_or_else(|| unreachable!("literal schema is always a valid JSON object"))
}

// ─── Re-exports ───────────────────────────────────────────────────────────────

// Gateway
pub use gateway::{
    ConstitutionallyWrappedPrompt, LlmGateway, RateLimitState, assemble_constitutional_prompt,
};

// Common node types
pub use node::{
    BudgetTracker, CliConfig, ConstraintValidator, ContextAssembler, ContextPackLoader,
    NodeExecutionResult, NodeFailure, NodeInput, PipelineConfig, WorkingCopyManager,
};

// Node structs
pub use architecture::ArchitectureNode;
pub use code_generation::CodeGenerationNode;
pub use intake::IntakeNode;
pub use integration::IntegrationNode;
pub use interface_design::InterfaceDesignNode;
pub use planning::PlanningNode;
pub use review::ReviewNode;
pub use spawning::SpawningNode;

// Executor
pub use executor::{PipelineExecutor, StepResult, run_step};

// Template engine implementation
pub use templates::HandlebarsTemplateEngine;
