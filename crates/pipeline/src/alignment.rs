//! Alignment verification — checks that a node's output addresses its inputs.
//!
//! Alignment verification detects cases where LLM output has drifted from the
//! work it was asked to do: implementing the wrong function, adding files
//! outside the approved scope, or failing to produce a declared output slot.
//!
//! Two check types are supported:
//!
//! - [`AlignmentCheckType::Deterministic`] — structural checks performed
//!   purely from the data in [`DeclaredNodeInputs`] and [`crate::NodeOutput`].
//!   No LLM is involved. [`run_deterministic_alignment`] implements these.
//! - [`AlignmentCheckType::LlmSemantic`] — semantic checks delegated to a
//!   (optionally different) LLM. These are orchestrated by the `nodes` crate
//!   using [`AlignmentConfig::use_different_model`]; the function signatures
//!   and result types are defined here but the LLM call itself lives in
//!   `nodes`.
//!
//! ## Blocking vs. Non-Blocking Findings
//!
//! An [`AlignmentFinding`] with `blocking: true` causes the Alignment check
//! to fail and triggers a rework cycle (subject to the rework budget from
//! [`AlignmentConfig::rework_budget`]). Non-blocking findings are recorded in
//! the audit trail but do not block progression.
//!
//! All [`MisalignmentType::ScopeExceeded`] findings are always blocking.
//! [`MisalignmentType::Missing`] findings are blocking when the missing item
//! is a declared required output slot.
//!
//! ## Pure Business Logic
//!
//! [`run_deterministic_alignment`] is pure (no I/O, no async). Semantic
//! alignment requires an LLM call and is orchestrated by the `nodes` crate.
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/advanced-features.md` §Alignment Verification for
//! the full contract, field semantics, and examples.

use serde::{Deserialize, Serialize};

use crate::{
    execution::NodeOutput,
    identifiers::ArtifactPath,
    security::{self, ApprovedScope, ProtectedPath},
    types::AlignmentScore,
};

// ─── Check type ───────────────────────────────────────────────────────────────

/// The kind of alignment check that produced a finding.
///
/// See `docs/spec/interfaces/advanced-features.md` §AlignmentCheckType.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AlignmentCheckType {
    /// Structural check: output slots, scope containment, protected-path
    /// compliance. No LLM involved.
    Deterministic,
    /// Semantic check: verifies that the implementation matches the
    /// specification intent. Delegated to an LLM in the `nodes` crate.
    LlmSemantic,
}

// ─── Misalignment category ────────────────────────────────────────────────────

/// Category of a misalignment discovered by an alignment check.
///
/// See `docs/spec/interfaces/advanced-features.md` §MisalignmentType.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MisalignmentType {
    /// An expected output, requirement, or declared output slot was not addressed.
    Missing,
    /// Output was produced for something not in the input scope.
    Extra,
    /// An artifact changed in a way that contradicts the input specification.
    Modified,
    /// The relationship between input and output is unclear or cannot be
    /// determined structurally.
    Ambiguous,
    /// Output touches artifacts outside the declared approved scope.
    ScopeExceeded,
}

impl std::fmt::Display for MisalignmentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "missing"),
            Self::Extra => write!(f, "extra"),
            Self::Modified => write!(f, "modified"),
            Self::Ambiguous => write!(f, "ambiguous"),
            Self::ScopeExceeded => write!(f, "scope_exceeded"),
        }
    }
}

// ─── Finding ─────────────────────────────────────────────────────────────────

/// A single misalignment discovered by one alignment check.
///
/// ## Blocking Rules
///
/// - All [`MisalignmentType::ScopeExceeded`] findings are always `blocking`.
/// - [`MisalignmentType::Missing`] findings are `blocking` when the missing
///   item was a declared required output slot.
/// - [`AlignmentCheckType::LlmSemantic`] findings: `blocking` is set by the
///   LLM's judgement in the response payload.
///
/// See `docs/spec/interfaces/advanced-features.md` §AlignmentFinding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlignmentFinding {
    /// Which kind of check produced this finding.
    pub check_type: AlignmentCheckType,

    /// The category of misalignment.
    pub misalignment: MisalignmentType,

    /// Human-readable explanation of the finding.
    ///
    /// For deterministic checks this describes the structural violation
    /// (e.g. "Output slot 'spec_doc' not populated"). For LLM-semantic checks
    /// this is the LLM's explanation.
    pub description: String,

    /// `true` if this finding must halt progression (triggers rework cycle).
    ///
    /// See blocking rules above.
    pub blocking: bool,
}

// ─── Traceability entry ───────────────────────────────────────────────────────

/// A single link between a requirement reference and an output artifact or
/// section, produced by the alignment check for use in traceability matrix updates.
///
/// See `docs/spec/interfaces/advanced-features.md` §TraceabilityEntry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceabilityEntry {
    /// Identifier of the requirement being addressed (e.g. `"REQ-001"`).
    pub requirement_ref: String,

    /// Identifier of the output that addresses the requirement.
    ///
    /// This is typically a repo-relative file path, a section header within a
    /// spec document, or a module name — whatever is most meaningful for the
    /// node type that produced the output.
    pub output_ref: String,
}

// ─── Aggregate result ─────────────────────────────────────────────────────────

/// The complete result of one alignment check pass.
///
/// See `docs/spec/interfaces/advanced-features.md` §AlignmentResult.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlignmentResult {
    /// Fraction of items checked that aligned, in `[0.0, 1.0]`.
    ///
    /// Computed as `(checked_items - blocking_findings) / checked_items`.
    /// When no items were checked this is `1.0` (vacuously aligned).
    pub score: AlignmentScore,

    /// All individual misalignments found during this check pass.
    pub findings: Vec<AlignmentFinding>,

    /// Requirement-to-output pairs for advancing the traceability matrix.
    ///
    /// Populated by LLM-semantic checks that identify which requirements each
    /// output section or file addresses. Deterministic checks do not populate
    /// this field.
    pub traceability_entries: Vec<TraceabilityEntry>,

    /// `true` when there are no blocking findings.
    ///
    /// The pipeline may proceed to the next node only when `aligned == true`.
    pub aligned: bool,
}

// ─── Configuration ────────────────────────────────────────────────────────────

/// Configuration for one alignment check pass.
///
/// Loaded from `.cogworks/pipeline.toml` and applied to each relevant node via
/// the `nodes` crate's `AlignmentChecker`. Default values are defined per-node
/// in the pipeline configuration.
///
/// See `docs/spec/interfaces/advanced-features.md` §AlignmentConfig.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlignmentConfig {
    /// Minimum score required for the alignment check to pass.
    ///
    /// Below this threshold the check fails even if `aligned` would otherwise
    /// be `true` (used to enforce a quantitative quality floor in addition to
    /// the binary blocking-findings gate).
    pub threshold: AlignmentScore,

    /// Which check types to run in this pass.
    ///
    /// Typically `[Deterministic, LlmSemantic]` for most nodes; the Code
    /// Generation node may run only `LlmSemantic` after the domain service
    /// validation has already covered structural slot-filling.
    pub enabled_checks: Vec<AlignmentCheckType>,

    /// Maximum number of rework cycles permitted before escalation.
    ///
    /// When the rework count reaches this limit and alignment still fails,
    /// the pipeline escalates rather than looping again.
    pub rework_budget: u32,

    /// If `true`, the `nodes` crate uses a separate critic model (rather than
    /// the main node model) for [`AlignmentCheckType::LlmSemantic`] checks.
    ///
    /// Using a different model for alignment helps avoid confirmation bias —
    /// the producing model's blind spots differ from the critic model's.
    pub use_different_model: bool,
}

// ─── Declared inputs (for deterministic alignment) ───────────────────────────

/// The node input declarations needed for deterministic alignment checking.
///
/// This is the subset of node input metadata that [`run_deterministic_alignment`]
/// requires. Full `NodeInput` (including context packs, artifact content, and
/// pipeline state) is defined in the `nodes` crate (PR 9); this struct carries
/// only the structural declarations used for scope and slot checking.
///
/// See `docs/spec/interfaces/advanced-features.md` §run_deterministic_alignment.
#[derive(Debug, Clone)]
pub struct DeclaredNodeInputs {
    /// Output slot names that the node was required to populate.
    ///
    /// Each name must appear as a key in [`NodeOutput::artifacts`] for the
    /// corresponding `Missing` check to pass.
    pub required_output_slots: Vec<String>,

    /// The scope boundary approved for this node's execution.
    ///
    /// Any artifact key in [`NodeOutput::artifacts`] that falls outside this
    /// scope generates a [`MisalignmentType::ScopeExceeded`] finding.
    pub approved_scope: ApprovedScope,

    /// Artifact paths that must not appear in the node's output.
    ///
    /// These are the globally-protected paths (from the safety registry plus
    /// pipeline configuration). Any match generates a
    /// [`MisalignmentType::ScopeExceeded`] finding.
    pub protected_paths: Vec<ArtifactPath>,
}

// ─── Core function ────────────────────────────────────────────────────────────

/// Performs structural alignment checks that do not require an LLM.
///
/// Checks:
/// 1. Every slot in `inputs.required_output_slots` is present in
///    `node_output.artifacts`.
/// 2. No artifact key in `node_output.artifacts` falls outside
///    `inputs.approved_scope`.
/// 3. No artifact key matches any path in `inputs.protected_paths`.
///
/// # Arguments
///
/// * `inputs` — declared input constraints for the node that produced the output.
/// * `node_output` — the output produced by the node.
///
/// # Returns
///
/// Zero or more [`AlignmentFinding`] values. An empty vec means all structural
/// checks passed.
///
/// # Examples
///
/// ```no_run
/// use pipeline::alignment::{run_deterministic_alignment, DeclaredNodeInputs};
/// use pipeline::execution::NodeOutput;
/// use pipeline::security::ApprovedScope;
/// use pipeline::knowledge::ScopeParameters;
/// use pipeline::types::TokenCost;
/// use std::collections::HashMap;
///
/// let scope_params = ScopeParameters {
///     allowed_artifact_patterns: vec!["**".to_string()],
///     max_file_changes: None,
///     max_new_files: u32::MAX,
///     prohibited_artifact_patterns: vec![],
/// };
/// let inputs = DeclaredNodeInputs {
///     required_output_slots: vec!["spec_doc".to_string()],
///     approved_scope: ApprovedScope::from_scope_parameters(&scope_params),
///     protected_paths: vec![],
/// };
/// let mut artifacts = HashMap::new();
/// artifacts.insert("spec_doc".to_string(), serde_json::json!("content"));
/// let output = NodeOutput { artifacts, cost_delta: TokenCost::zero(), state_updates: vec![] };
/// let findings = run_deterministic_alignment(&inputs, &output);
/// assert!(findings.is_empty(), "All declared slots populated, no scope violations");
/// ```
///
/// See `docs/spec/interfaces/advanced-features.md` §run_deterministic_alignment.
pub fn run_deterministic_alignment(
    inputs: &DeclaredNodeInputs,
    node_output: &NodeOutput,
) -> Vec<AlignmentFinding> {
    let mut findings = check_required_slots(inputs, node_output);
    findings.extend(check_scope_and_protected_paths(inputs, node_output));
    findings
}

/// Check 1: every declared required output slot must be a key in
/// `node_output.artifacts`. One [`MisalignmentType::Missing`] finding is
/// produced per absent slot (duplicates in `required_output_slots` are not
/// deduplicated — each absent occurrence produces its own finding).
fn check_required_slots(
    inputs: &DeclaredNodeInputs,
    node_output: &NodeOutput,
) -> Vec<AlignmentFinding> {
    inputs
        .required_output_slots
        .iter()
        .filter(|slot| !node_output.artifacts.contains_key(slot.as_str()))
        .map(|slot| {
            deterministic_finding(
                MisalignmentType::Missing,
                format!(
                    "Required output slot '{}' was not populated.",
                    sanitize_for_description(slot)
                ),
            )
        })
        .collect()
}

/// Maximum number of characters of a user- or LLM-influenced string (an
/// artifact key or slot name) that may appear verbatim in a finding
/// description. Longer values are truncated; this keeps descriptions bounded
/// even for adversarially long node output, since findings are documented as
/// feeding into rework-loop prompts sent back to an LLM.
const MAX_DESCRIPTION_KEY_LEN: usize = 200;

/// Escapes control characters (including newlines) and truncates `s` to
/// [`MAX_DESCRIPTION_KEY_LEN`] characters before it is embedded in an
/// [`AlignmentFinding::description`]. Prevents a crafted artifact key or slot
/// name from injecting raw newlines/control bytes or an unbounded amount of
/// text into a description that may later be replayed into an LLM prompt.
fn sanitize_for_description(s: &str) -> String {
    let total_chars = s.chars().count();
    let escaped: String = s
        .chars()
        .take(MAX_DESCRIPTION_KEY_LEN)
        .flat_map(char::escape_default)
        .collect();
    if total_chars > MAX_DESCRIPTION_KEY_LEN {
        format!("{escaped}...")
    } else {
        escaped
    }
}

/// Builds a blocking [`AlignmentFinding`] for [`AlignmentCheckType::Deterministic`].
///
/// All deterministic findings produced by this module are blocking (see
/// module-level "Blocking vs. Non-Blocking Findings" docs), so this is the
/// single construction point for both check 1 ([`check_required_slots`]) and
/// checks 2/3 ([`scope_exceeded_finding`]).
fn deterministic_finding(misalignment: MisalignmentType, description: String) -> AlignmentFinding {
    AlignmentFinding {
        check_type: AlignmentCheckType::Deterministic,
        misalignment,
        description,
        blocking: true,
    }
}

/// Checks 2 and 3, combined per artifact so that an artifact that is both
/// out of scope and protected yields exactly one finding (protected-path
/// precedence, mirroring `security::validate_scope`'s `ASSERT-SEC-005` rule).
///
/// For each artifact key, [`classify_artifact_key`] decides the outcome. Keys
/// that fail `ArtifactPath::new` are rejected fail-closed (see that
/// function's docs) rather than falling back to the raw-string scope check.
fn check_scope_and_protected_paths(
    inputs: &DeclaredNodeInputs,
    node_output: &NodeOutput,
) -> Vec<AlignmentFinding> {
    let approved_set = security::build_glob_set(&inputs.approved_scope.artifact_patterns);
    let protected: Vec<ProtectedPath> = inputs
        .protected_paths
        .iter()
        .map(|path| ProtectedPath {
            // `ProtectedPath.pattern` is compiled as a glob by
            // `security::is_protected`/`build_glob_set`, but a declared
            // protected path is a concrete artifact location, not a glob —
            // it must match itself literally even when it contains glob
            // metacharacters (e.g. `src/[id].rs`). Escaping preserves `/` so
            // the existing anchored-vs-any-depth logic in
            // `normalize_glob_pattern`/`build_glob_set` still applies.
            pattern: globset::escape(path.as_str()),
            reason: "declared protected path".to_string(),
        })
        .collect();

    node_output
        .artifacts
        .keys()
        .filter_map(|key| classify_artifact_key(key, &approved_set, &protected))
        .collect()
}

/// Classifies a single artifact key against the protected-path and
/// approved-scope rules, returning the finding (if any) for that key.
///
/// Fail-closed rule: if `key` does not parse as a valid [`ArtifactPath`]
/// (e.g. it contains a `..` traversal component), it is rejected immediately
/// with a blocking `ScopeExceeded` finding. It is never matched against the
/// raw-string approved-scope glob, because an unresolved string cannot be
/// trusted to reflect the artifact's true, path-resolved location — doing so
/// previously allowed a `..`-containing key to defeat both the protected-path
/// check (which requires a parsed `ArtifactPath`) and the scope check
/// (which matched the raw string) simultaneously.
///
/// Only when `key` parses successfully does the existing protected-path-then-
/// scope precedence apply (protected match wins over, and suppresses, a
/// redundant scope violation for the same artifact).
fn classify_artifact_key(
    key: &str,
    approved_set: &globset::GlobSet,
    protected: &[ProtectedPath],
) -> Option<AlignmentFinding> {
    let Some(path) = ArtifactPath::new(key) else {
        return Some(scope_exceeded_finding(
            key,
            "is not a valid artifact path (e.g. contains an invalid component such as '..')",
        ));
    };
    if security::is_protected(&path, protected) {
        return Some(scope_exceeded_finding(key, "matches a protected path"));
    }
    // Match the normalized, parsed path (not the raw `key`) so that both
    // checks agree on the same canonical form — e.g. a leading `./` is
    // stripped by `ArtifactPath::new` and must be stripped consistently
    // before either check runs.
    if !approved_set.is_match(path.as_str()) {
        return Some(scope_exceeded_finding(
            key,
            "falls outside the approved scope",
        ));
    }
    None
}

/// Builds a `ScopeExceeded` finding for an out-of-scope or protected artifact
/// key. `ScopeExceeded` findings are always blocking.
///
/// `key` is sanitized (control characters escaped, length bounded) before
/// being embedded in the description, since it may be an untrusted,
/// LLM-influenced artifact key that later feeds into a rework-loop prompt.
fn scope_exceeded_finding(key: &str, reason: &str) -> AlignmentFinding {
    deterministic_finding(
        MisalignmentType::ScopeExceeded,
        format!("Artifact '{}' {reason}.", sanitize_for_description(key)),
    )
}

#[cfg(test)]
#[path = "alignment_tests.rs"]
mod tests;
