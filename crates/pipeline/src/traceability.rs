//! Traceability matrix — tracks requirement coverage across pipeline stages.
//!
//! The traceability matrix provides an audit trail linking each requirement
//! from the original GitHub work item to the pipeline artifacts that address
//! it, across four progressive stages:
//!
//! 1. **Architecture** — the architecture document addresses the requirement.
//! 2. **Interface** — the interface specification addresses the requirement.
//! 3. **SubWorkItem** — a planning sub-work-item is created to implement it.
//! 4. **Code** — the generated code addresses the requirement.
//!
//! ## Requirement Extraction
//!
//! Requirements are extracted from the GitHub issue body by
//! [`extract_requirements`]. The expected format is lines containing tags of
//! the form `REQ-NNN:` followed by a description (the Intake node's LLM prompt
//! instructs the issue author to use this convention).
//!
//! ## Matrix Lifecycle
//!
//! The matrix is initialised with all requirements extracted from the work item
//! body after the Intake node runs. Each subsequent node (Architecture,
//! Interface Design, Planning, Code Generation) calls
//! [`update_traceability_matrix`] with its alignment findings to advance
//! coverage flags. The final matrix is committed to GitHub via the
//! [`crate::AuditStore`] as part of the pipeline summary.
//!
//! ## Pure Business Logic
//!
//! Both functions are pure (no I/O, no async). The `nodes` crate is responsible
//! for calling them and persisting the results.
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/advanced-features.md` §Traceability Matrix for
//! the full contract and examples.

use serde::{Deserialize, Serialize};

use crate::{
    alignment::TraceabilityEntry,
    identifiers::{PipelineRunId, WorkItemId},
};

// ─── Requirement ─────────────────────────────────────────────────────────────

/// A single requirement extracted from a GitHub work item body.
///
/// Requirements are identified by a `REQ-NNN:` tag in the issue body.
/// They are extracted by [`extract_requirements`] and tracked through the
/// pipeline in a [`TraceabilityMatrix`].
///
/// See `docs/spec/interfaces/advanced-features.md` §Requirement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    /// Short identifier, e.g. `"REQ-001"`.
    ///
    /// Extracted verbatim from the `REQ-NNN` tag in the issue body.
    pub id: String,

    /// Full text of the requirement, from the remainder of the `REQ-NNN: …`
    /// line in the issue body.
    pub description: String,

    /// The GitHub work item from which this requirement was extracted.
    ///
    /// [`extract_requirements`] returns requirements with this field set to
    /// `WorkItemId::new(0)` (sentinel zero value); callers must set it to the
    /// relevant [`WorkItemId`] after calling the function.
    pub source_work_item: WorkItemId,
}

// ─── Stage ───────────────────────────────────────────────────────────────────

/// The four pipeline stages at which requirement coverage is tracked.
///
/// Stages progress from `Architecture` through `Code` in pipeline order.
/// Coverage flags in [`RequirementRow`] are set to `true` as each stage
/// produces alignment findings that reference a requirement.
///
/// See `docs/spec/interfaces/advanced-features.md` §TraceabilityStage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TraceabilityStage {
    /// Architecture document stage (Architecture node output).
    Architecture,
    /// Interface specification stage (Interface Design node output).
    Interface,
    /// Sub-work-item planning stage (Planning node output).
    SubWorkItem,
    /// Code generation stage (Code Generation node output).
    Code,
}

impl std::fmt::Display for TraceabilityStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Architecture => write!(f, "architecture"),
            Self::Interface => write!(f, "interface"),
            Self::SubWorkItem => write!(f, "sub_work_item"),
            Self::Code => write!(f, "code"),
        }
    }
}

// ─── Row status ──────────────────────────────────────────────────────────────

/// Coverage status of one requirement row in the traceability matrix.
///
/// See `docs/spec/interfaces/advanced-features.md` §TraceabilityStatus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TraceabilityStatus {
    /// All four pipeline stages have recorded coverage for this requirement.
    Complete,
    /// At least one but not all stages have recorded coverage.
    Partial,
    /// No stage has yet recorded coverage for this requirement.
    Missing,
}

// ─── Row ─────────────────────────────────────────────────────────────────────

/// One row of the traceability matrix — one requirement's coverage level
/// across all four pipeline stages.
///
/// See `docs/spec/interfaces/advanced-features.md` §RequirementRow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementRow {
    /// The requirement being tracked.
    pub requirement: Requirement,

    /// `true` after the Architecture node's alignment findings reference this
    /// requirement.
    pub architecture_covered: bool,

    /// `true` after the Interface Design node's alignment findings reference
    /// this requirement.
    pub interface_covered: bool,

    /// `true` after the Planning node creates a sub-work-item that references
    /// this requirement.
    pub sub_work_item_covered: bool,

    /// `true` after the Code Generation node's alignment findings reference
    /// this requirement.
    pub code_covered: bool,

    /// Aggregate coverage status derived from the four flags above.
    ///
    /// Recomputed by [`update_traceability_matrix`] after every stage update.
    pub status: TraceabilityStatus,
}

impl RequirementRow {
    /// Computes the [`TraceabilityStatus`] from the four coverage flags.
    ///
    /// Returns `Complete` if all four flags are `true`, `Missing` if all are
    /// `false`, and `Partial` otherwise.
    #[must_use]
    pub fn compute_status(&self) -> TraceabilityStatus {
        let covered = [
            self.architecture_covered,
            self.interface_covered,
            self.sub_work_item_covered,
            self.code_covered,
        ];
        let count = covered.iter().filter(|&&b| b).count();
        // IMPORTANT: the literal `4` matches the four stage fields above.
        // If `TraceabilityStage` gains a fifth variant, add the corresponding
        // `bool` field to `RequirementRow` AND update this match arm.
        match count {
            0 => TraceabilityStatus::Missing,
            4 => TraceabilityStatus::Complete,
            _ => TraceabilityStatus::Partial,
        }
    }
}

// ─── Matrix ──────────────────────────────────────────────────────────────────

/// The full traceability matrix for one pipeline run.
///
/// The matrix is initialised after the Intake node and updated after each
/// subsequent node. The final matrix is committed to the GitHub audit record
/// via [`crate::AuditStore`].
///
/// See `docs/spec/interfaces/advanced-features.md` §TraceabilityMatrix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceabilityMatrix {
    /// One row per requirement extracted from the work item.
    pub rows: Vec<RequirementRow>,

    /// Identifies which pipeline run produced this matrix.
    pub pipeline_run_id: PipelineRunId,

    /// The work item being implemented.
    pub work_item_id: WorkItemId,

    /// `true` when every row has [`TraceabilityStatus::Complete`].
    ///
    /// Recomputed by [`update_traceability_matrix`] after every stage update.
    pub complete: bool,
}

// ─── Core functions ───────────────────────────────────────────────────────────

/// Parses requirement declarations from a GitHub issue body.
///
/// Scans `work_item_body` for lines containing `REQ-NNN:` tags (case-insensitive
/// prefix, colon separator). Returns one [`Requirement`] per matching line.
///
/// Lines that do not match the pattern are silently ignored. The
/// `source_work_item` field of every returned requirement is initialised to the
/// zero UUID — callers must set it after calling this function.
///
/// # Arguments
///
/// * `work_item_body` — the raw Markdown body of the GitHub issue.
///
/// # Returns
///
/// A `Vec<Requirement>`. Empty if no requirements are found; never returns an
/// error.
///
/// # Examples
///
/// ```no_run
/// use pipeline::traceability::extract_requirements;
///
/// let body = "## Requirements\nREQ-001: System must validate inputs\nREQ-002: Errors must be logged";
/// let reqs = extract_requirements(body);
/// assert_eq!(reqs.len(), 2);
/// assert_eq!(reqs[0].id, "REQ-001");
/// assert_eq!(reqs[1].description, "Errors must be logged");
/// ```
///
/// See `docs/spec/interfaces/advanced-features.md` §extract_requirements.
pub fn extract_requirements(work_item_body: &str) -> Vec<Requirement> {
    work_item_body
        .lines()
        .filter_map(find_first_requirement_in_line)
        .collect()
}

/// Scans a single line for the first `REQ-<digits>:` tag and, if found,
/// builds the corresponding [`Requirement`].
///
/// Matching is case-insensitive on the `REQ` prefix, but the `id` field
/// preserves the exact bytes matched in `line` (verbatim case). Matching is
/// byte-oriented over ASCII delimiters only (`R`, `E`, `Q`, `-`, ASCII
/// digits, `:`), so it never slices on a non-UTF-8-boundary: every slice
/// point returned by [`match_tag_at`] immediately follows one of those
/// single-byte ASCII characters, which is always a valid `char` boundary.
fn find_first_requirement_in_line(line: &str) -> Option<Requirement> {
    let bytes = line.as_bytes();
    for start in 0..bytes.len() {
        if let Some(colon_idx) = match_tag_at(bytes, start) {
            let id = line[start..colon_idx].to_string();
            let description = line[colon_idx + 1..].trim().to_string();
            return Some(Requirement {
                id,
                description,
                source_work_item: WorkItemId::new(0),
            });
        }
    }
    None
}

/// Attempts to match a `REQ-<digits>:` tag starting at byte offset `start`
/// in `bytes`. Returns the byte offset of the matching `:` on success.
///
/// Runs in `O(1)` amortised time per call site: the prefix check fails fast
/// on the first mismatched byte, so scanning a whole line is `O(n)` with no
/// backtracking — this is a hand-rolled bounded matcher, not a backtracking
/// regex engine, so pathological inputs (e.g. long runs of `"REQ-"`) cannot
/// cause super-linear behaviour.
fn match_tag_at(bytes: &[u8], start: usize) -> Option<usize> {
    const PREFIX_LEN: usize = 4; // "REQ-"
    if start + PREFIX_LEN > bytes.len() {
        return None;
    }
    let prefix_matches = bytes[start].eq_ignore_ascii_case(&b'R')
        && bytes[start + 1].eq_ignore_ascii_case(&b'E')
        && bytes[start + 2].eq_ignore_ascii_case(&b'Q')
        && bytes[start + 3] == b'-';
    if !prefix_matches {
        return None;
    }

    let digits_start = start + PREFIX_LEN;
    let mut idx = digits_start;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == digits_start {
        return None; // no digits
    }

    if idx < bytes.len() && bytes[idx] == b':' {
        Some(idx)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------

/// Applies structured traceability entries from one pipeline stage to update
/// coverage flags in the traceability matrix.
///
/// For each [`TraceabilityEntry`] in `entries`, matches `requirement_ref`
/// against the `id` field of each row in `matrix.rows`. When a match is found
/// the corresponding stage coverage flag is set to `true`.
///
/// After updating flags, recomputes each row's `status` field and the
/// matrix-level `complete` flag.
///
/// ## Source of entries
///
/// `entries` must come from [`AlignmentResult::traceability_entries`], which is
/// only populated by the [`AlignmentCheckType::LlmSemantic`] check path. The
/// deterministic alignment path does not produce traceability entries; if only
/// deterministic checks were run for a stage, pass an empty slice and the stage
/// coverage flags will remain unchanged (recording an honest *uncovered* status
/// rather than fabricating coverage).
///
/// # Arguments
///
/// * `matrix` — the current traceability matrix (consumed and returned as
///   updated).
/// * `stage` — the pipeline stage whose traceability entries are being applied.
/// * `entries` — structured requirement-to-output pairs from the LLM-semantic
///   alignment check for this stage.
///
/// # Returns
///
/// The updated matrix (value semantics — no in-place mutation).
///
/// # Examples
///
/// ```no_run
/// use pipeline::traceability::{
///     extract_requirements, update_traceability_matrix, RequirementRow,
///     Requirement, TraceabilityMatrix, TraceabilityStage, TraceabilityStatus,
/// };
/// use pipeline::alignment::TraceabilityEntry;
/// use pipeline::identifiers::{PipelineRunId, WorkItemId};
///
/// let reqs = extract_requirements("REQ-001: Must validate inputs");
/// let rows: Vec<RequirementRow> = reqs
///     .into_iter()
///     .map(|r| RequirementRow {
///         requirement: r,
///         architecture_covered: false,
///         interface_covered: false,
///         sub_work_item_covered: false,
///         code_covered: false,
///         status: TraceabilityStatus::Missing,
///     })
///     .collect();
/// let matrix = TraceabilityMatrix {
///     rows,
///     pipeline_run_id: PipelineRunId::new_random(),
///     work_item_id: WorkItemId::new(42),
///     complete: false,
/// };
/// let entries = vec![TraceabilityEntry {
///     requirement_ref: "REQ-001".to_string(),
///     output_ref: "docs/spec/architecture.md#input-validation".to_string(),
/// }];
/// let updated = update_traceability_matrix(matrix, TraceabilityStage::Architecture, &entries);
/// assert!(updated.rows[0].architecture_covered);
/// ```
///
/// See `docs/spec/interfaces/advanced-features.md` §update_traceability_matrix.
pub fn update_traceability_matrix(
    mut matrix: TraceabilityMatrix,
    stage: TraceabilityStage,
    entries: &[TraceabilityEntry],
) -> TraceabilityMatrix {
    for entry in entries {
        for row in &mut matrix.rows {
            if row.requirement.id == entry.requirement_ref {
                set_stage_covered(row, stage);
            }
        }
    }

    for row in &mut matrix.rows {
        row.status = row.compute_status();
    }
    matrix.complete = matrix
        .rows
        .iter()
        .all(|row| row.status == TraceabilityStatus::Complete);

    matrix
}

/// Sets the coverage flag on `row` that corresponds to `stage`.
fn set_stage_covered(row: &mut RequirementRow, stage: TraceabilityStage) {
    match stage {
        TraceabilityStage::Architecture => row.architecture_covered = true,
        TraceabilityStage::Interface => row.interface_covered = true,
        TraceabilityStage::SubWorkItem => row.sub_work_item_covered = true,
        TraceabilityStage::Code => row.code_covered = true,
    }
}

#[cfg(test)]
#[path = "traceability_tests.rs"]
mod tests;
