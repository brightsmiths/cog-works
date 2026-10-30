//! Architecture node — architecture document generation and constraint validation.
//!
//! The Architecture node takes the Intake output and produces an architecture
//! document that describes the approach for implementing the work item. It:
//! 1. Loads context packs for the classified work item.
//! 2. Validates cross-domain constraints (interface registry check).
//! 3. Assembles context (pyramid summaries, interface definitions).
//! 4. Calls the LLM to produce the architecture document.
//! 5. Runs deterministic alignment.
//! 6. If `LlmSemantic` alignment is enabled: runs it and updates the traceability
//!    matrix from `AlignmentResult::traceability_entries`.
//!
//! ## Output Artifacts
//!
//! | Slot | Content |
//! |------|---------|
//! | `architecture_doc` | Markdown architecture document (string) |
//! | `traceability_matrix` | Updated [`pipeline::TraceabilityMatrix`] (JSON) |
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/nodes.md` §ArchitectureNode.

use pipeline::CodeRepository;

use crate::gateway::LlmGateway;
use crate::node::{
    ConstraintValidator, ContextAssembler, ContextPackLoader, NodeExecutionResult, NodeInput,
};

/// Architecture node — generates the architecture document and validates constraints.
///
/// See `docs/spec/interfaces/nodes.md` §ArchitectureNode.
pub struct ArchitectureNode;

impl ArchitectureNode {
    /// Executes the Architecture node for `input`.
    ///
    /// # Arguments
    ///
    /// * `input` — Node inputs including the `classification` artifact from Intake.
    /// * `gateway` — LLM gateway for architecture generation and semantic alignment.
    /// * `context` — Context assembler for building the node's context package.
    /// * `constraint_validator` — Validates cross-domain interface constraints.
    /// * `pack_loader` — Loads context packs from the repository.
    /// * `github` — Repository access for reading ADRs and interface documents.
    ///
    /// # Returns
    ///
    /// `NodeExecutionResult::Success` with `architecture_doc` and updated
    /// `traceability_matrix` artifacts, or `NodeExecutionResult::Failure`.
    ///
    /// See `docs/spec/interfaces/nodes.md` §ArchitectureNode.
    pub async fn execute(
        _input: NodeInput,
        _gateway: &LlmGateway,
        _context: &ContextAssembler,
        _constraint_validator: &ConstraintValidator,
        _pack_loader: &ContextPackLoader,
        _github: &dyn CodeRepository,
    ) -> NodeExecutionResult {
        todo!("See docs/spec/interfaces/nodes.md §ArchitectureNode")
    }
}

#[cfg(test)]
#[path = "architecture_tests.rs"]
mod tests;
