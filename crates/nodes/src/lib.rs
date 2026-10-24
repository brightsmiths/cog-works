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

use std::sync::Mutex;

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
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
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
