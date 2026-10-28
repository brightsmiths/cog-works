//! Intake node — work item classification and requirement extraction.
//!
//! The Intake node is the first node in every default pipeline. It:
//! 1. Fetches the GitHub issue and scans for injection.
//! 2. Classifies the work item (task type, safety flag, scope estimate).
//! 3. Applies the safety-critical registry override.
//! 4. Checks the scope threshold; escalates if the work item is over-scoped.
//! 5. Extracts requirements and initialises the traceability matrix.
//!
//! ## Output Artifacts
//!
//! | Slot | Content |
//! |------|---------|
//! | `classification` | [`pipeline::ClassificationResult`] (JSON) |
//! | `traceability_matrix` | Initial [`pipeline::TraceabilityMatrix`] with all requirements and zero coverage (JSON) |
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/nodes.md` §IntakeNode.

use pipeline::IssueTracker;

use crate::gateway::LlmGateway;
use crate::node::{NodeExecutionResult, NodeInput, PipelineConfig};

/// Intake node — classifies the work item and initialises the traceability matrix.
///
/// See `docs/spec/interfaces/nodes.md` §IntakeNode.
pub struct IntakeNode;

impl IntakeNode {
    /// Executes the Intake node for `input`.
    ///
    /// # Arguments
    ///
    /// * `input` — Node inputs (pipeline state, loaded context packs).
    /// * `gateway` — LLM gateway for classification calls.
    /// * `issues` — Issue tracker for fetching the work-item body and labels.
    /// * `config` — Resolved pipeline configuration for this run.
    ///
    /// # Returns
    ///
    /// `NodeExecutionResult::Success` with `classification` and
    /// `traceability_matrix` artifacts on success, or
    /// `NodeExecutionResult::Failure` on any error.
    ///
    /// See `docs/spec/interfaces/nodes.md` §IntakeNode.
    pub async fn execute(
        _input: NodeInput,
        _gateway: &LlmGateway,
        _issues: &dyn IssueTracker,
        _config: &PipelineConfig,
    ) -> NodeExecutionResult {
        todo!("See docs/spec/interfaces/nodes.md §IntakeNode")
    }
}

#[cfg(test)]
#[path = "intake_tests.rs"]
mod tests;
