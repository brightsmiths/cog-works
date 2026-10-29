//! Context assembly, context pack selection, and classification result types.
//!
//! This module provides:
//!
//! - [`ClassificationResult`] / [`TaskType`] — the Intake node's classification
//!   of the work item. Drives context pack selection and safety gating throughout
//!   the rest of the pipeline run.
//! - [`ContextPriority`] / [`ContextItem`] / [`ContextPackage`] — the data
//!   structures representing assembled context for an LLM node invocation.
//! - [`ContextPackTrigger`] / [`ContextPack`] / [`MergedGuidance`] /
//!   [`LoadedContextPacks`] — the Context Pack type hierarchy.
//! - [`ContextAssemblyRequest`] — parameters for one context assembly call.
//! - [`select_context_packs`], [`merge_pack_guidance`], [`assemble_context`],
//!   [`apply_priority_truncation`], [`enforce_scenario_holdout`] — context
//!   assembly functions.
//!
//! ## Scenario Holdout Constraint
//!
//! [`enforce_scenario_holdout`] **must** be called before any context package
//! is used for code generation. Scenario files must never appear in a code
//! generation context (see `docs/spec/constraints.md` §Module Boundaries).
//! `assemble_context` calls this automatically; direct callers of
//! `apply_priority_truncation` must also call it first.
//!
//! ## Architectural Layer
//!
//! **Business logic.** No infrastructure dependencies.
//! `assemble_context` is `async` because it queries [`SummaryCache`], which is
//! an async port trait defined in `pipeline` and implemented in infrastructure.
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/context.md` for the full contract, merge semantics,
//! and assembly algorithm.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use globset::Glob;

use crate::{
    domain_services::InterfaceDefinition,
    graph::NodeType,
    identifiers::{ArtifactPath, ContextPackId},
    knowledge::{PyramidSummary, SummaryCache, SummaryLevel},
    types::{SatisfactionScore, TokenCount},
};

// ─── Classification result ───────────────────────────────────────────────────

/// The category of work the pipeline has been asked to perform, as determined
/// by the Intake node's classification step.
///
/// See `docs/spec/interfaces/context.md` §`TaskType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskType {
    /// Implementing new functionality.
    Feature,
    /// Correcting defective behaviour.
    BugFix,
    /// Updating or adding documentation only.
    Documentation,
    /// Restructuring code without changing observable behaviour.
    Refactoring,
    /// Changing configuration files or build settings.
    Configuration,
    /// Adding or updating tests without changing production code.
    Testing,
    /// Addressing a security vulnerability or hardening.
    Security,
    /// Could not be determined with sufficient confidence.
    Unknown,
}

// ---------------------------------------------------------------------------

/// The Intake node's classification of the work item.
///
/// All downstream pipeline nodes consume this value to customise their
/// behaviour — context pack selection, safety gating, scope checks.
///
/// Note: the processing functions that operate on `ClassificationResult`
/// (`apply_safety_override`, `check_scope_threshold`) are defined in
/// `classification.rs` (PR 7).
///
/// See `docs/spec/interfaces/context.md` §`ClassificationResult`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationResult {
    /// What category of work this is.
    pub task_type: TaskType,

    /// `true` if any affected module belongs to the safety-critical registry.
    ///
    /// Safety-affecting tasks require human approval before any PR is merged.
    /// This field may be overridden to `true` by `apply_safety_override` in
    /// `classification.rs` when the registry match happens post-classification.
    pub safety_affecting: bool,

    /// Magnitude estimate of the change (1 = trivial, 10 = very large).
    ///
    /// Used by `check_scope_threshold` (PR 7) to detect over-scoped tasks
    /// that should be split into smaller work items before proceeding.
    pub estimated_scope: u32,

    /// Repo-relative paths of modules the change is expected to touch.
    pub affected_modules: Vec<ArtifactPath>,
}

// ─── Context priority ────────────────────────────────────────────────────────

/// Priority rank for a context item, ordinal from highest to lowest.
///
/// Items with a lower discriminant value have higher priority and are retained
/// first when the token budget is exhausted. `Ord` is derived by declaration
/// order (smallest = most important).
///
/// See `docs/spec/interfaces/context.md` §`ContextPriority`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ContextPriority {
    /// Interface definitions for the artifact(s) the current node operates on.
    /// Most important — always included if it fits at all.
    CurrentInterfaceDefinition = 0,
    /// Output artifacts produced by nodes that the current node depends on.
    DirectDependencyOutput = 1,
    /// Constraints and principles from architectural decision records and
    /// constraint documents.
    ArchitecturalConstraints = 2,
    /// Domain knowledge provided by matched Context Packs.
    ContextPackKnowledge = 3,
    /// Coding standards, style guides, and linting rules.
    CodingStandards = 4,
    /// Summaries of modules transitively (but not directly) depended on.
    TransitiveDependency = 5,
}

// ─── Context item and package ─────────────────────────────────────────────────

/// A single unit of knowledge included in a node's context window.
///
/// See `docs/spec/interfaces/context.md` §`ContextItem`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItem {
    /// The verbatim text delivered to the LLM at this position in the context.
    pub content: String,

    /// The pyramid granularity level at which this entry was included.
    ///
    /// Used for observability: logging which levels were selected indicates
    /// how tight the token budget is.
    pub summary_level: SummaryLevel,

    /// Priority tier used for truncation ordering.
    pub priority: ContextPriority,

    /// Pre-computed approximate token count for `content`.
    ///
    /// The context assembler uses this for budget arithmetic. Must match the
    /// LLM provider's tokenisation (approximate counts are acceptable for
    /// budget enforcement; precision is not guaranteed).
    pub token_count: TokenCount,

    /// `true` if this item must be included regardless of the token budget.
    ///
    /// Set to `true` for artifacts listed in
    /// [`MergedGuidance::required_artifacts`]. [`apply_priority_truncation`]
    /// will include required items even when the budget is already exceeded
    /// by higher-priority non-required items.
    #[serde(default)]
    pub required: bool,

    /// The artifact this item was derived from; `None` for synthesised items.
    ///
    /// `None` examples: merged pack guidance text, sub-work-item description.
    /// `enforce_scenario_holdout` filters by this field.
    pub source_path: Option<ArtifactPath>,
}

// ---------------------------------------------------------------------------

/// The complete assembled context for a single LLM node invocation.
///
/// Produced by [`assemble_context`] and [`apply_priority_truncation`].
///
/// See `docs/spec/interfaces/context.md` §`ContextPackage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPackage {
    /// Context items, ordered high-priority first.
    ///
    /// Within the same priority tier, items are ordered alphabetically by
    /// `source_path` for reproducibility.
    pub items: Vec<ContextItem>,

    /// Sum of `item.token_count` across all retained items.
    pub total_token_count: TokenCount,

    /// `true` if one or more items were dropped to fit within the token budget,
    /// or if a single required artifact's item exceeds the budget on its own
    /// (budget overflow).
    ///
    /// Does **not** indicate cache fetch errors; see [`assembly_errors`] for
    /// that. Splitting the two signals allows callers to distinguish expected
    /// budget pressure from unexpected upstream failures.
    pub truncation_applied: bool,

    /// Descriptions of artifacts that were skipped because [`crate::SummaryCache`]
    /// returned an error during assembly.
    ///
    /// An empty vec means the context is complete (no fetch failures). A
    /// non-empty vec means the context may be missing information; callers
    /// should log or surface these errors for observability but may still
    /// proceed with the assembled context.
    pub assembly_errors: Vec<String>,
}

// ─── Holdout-filtered items newtype ──────────────────────────────────────────

/// A [`Vec<ContextItem>`] that has passed through [`enforce_scenario_holdout`].
///
/// This newtype exists solely to make the scenario holdout a **compile-time
/// constraint**: [`apply_priority_truncation`] accepts only
/// `HoldoutFilteredItems`, so it is impossible to call `apply_priority_truncation`
/// without first calling `enforce_scenario_holdout`. The compiler rejects the
/// wrong call order.
///
/// `assemble_context` handles this automatically. Direct callers that bypass
/// `assemble_context` must call `enforce_scenario_holdout` first and unwrap
/// the result via [`HoldoutFilteredItems::into_inner`].
///
/// See `docs/spec/interfaces/context.md` §`HoldoutFilteredItems`.
#[derive(Debug, Clone)]
pub struct HoldoutFilteredItems(Vec<ContextItem>);

impl HoldoutFilteredItems {
    /// Returns the inner `Vec<ContextItem>`.
    #[must_use]
    pub fn into_inner(self) -> Vec<ContextItem> {
        self.0
    }
}

// ─── Context pack types ──────────────────────────────────────────────────────

/// The conditions under which a Context Pack is selected for a pipeline run.
///
/// Pack selection uses OR semantics across fields: the pack is selected if **any**
/// matching criterion evaluates to `true`.
///
/// See `docs/spec/interfaces/context.md` §`ContextPackTrigger`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPackTrigger {
    /// Glob patterns matched against GitHub label strings on the work item.
    ///
    /// The pack fires if any pattern matches any label. This field is evaluated
    /// by [`select_context_packs`] when the caller passes `active_labels`.
    /// Unlike `component_tag_patterns`, label information is not available in
    /// [`ClassificationResult`] and must be passed separately.
    pub label_patterns: Vec<String>,

    /// Glob patterns matched against each path in
    /// [`ClassificationResult::affected_modules`].
    ///
    /// The pack fires if any pattern matches any affected module path.
    pub component_tag_patterns: Vec<String>,

    /// If `true`, this pack is only selected when
    /// [`ClassificationResult::safety_affecting`] is `true`.
    pub requires_safety_critical: bool,
}

// ---------------------------------------------------------------------------

/// A domain-specific knowledge bundle loaded from `.cogworks/context-packs/`.
///
/// Each Context Pack provides domain knowledge text, coding pattern guidance,
/// artifact requirements, and optionally a tighter scenario threshold for its
/// domain. Multiple packs may be active for a single run; their guidance is
/// merged by [`merge_pack_guidance`].
///
/// See `docs/spec/interfaces/context.md` §`ContextPack`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPack {
    /// The pack identifier, matching its directory name in
    /// `.cogworks/context-packs/`.
    pub id: ContextPackId,

    /// The conditions under which this pack is selected.
    pub trigger: ContextPackTrigger,

    /// Domain knowledge text included at the [`ContextPriority::ContextPackKnowledge`]
    /// tier. Verbatim text; not subject to pyramid truncation.
    pub domain_knowledge: String,

    /// Code or design patterns that are correct for this domain.
    ///
    /// Merged into [`MergedGuidance::safe_patterns`] with union semantics.
    pub safe_patterns: Vec<String>,

    /// Code or design patterns that must be avoided in this domain.
    ///
    /// Merged into [`MergedGuidance::anti_patterns`] with union semantics.
    pub anti_patterns: Vec<String>,

    /// Artifacts that must be included in the context regardless of priority
    /// truncation.
    ///
    /// Merged into [`MergedGuidance::required_artifacts`] with
    /// deduplication-by-path union semantics.
    pub required_artifacts: Vec<ArtifactPath>,

    /// Per-pack scenario satisfaction threshold override.
    ///
    /// When multiple packs are active, the strictest (lowest) threshold wins.
    /// `None` defers to the pipeline-level default.
    pub scenario_threshold_override: Option<SatisfactionScore>,
}

// ---------------------------------------------------------------------------

/// Merged Context Pack guidance produced by [`merge_pack_guidance`].
///
/// `safe_patterns` and `anti_patterns` are union-merged across all active packs.
/// `required_artifacts` are union-merged with deduplication by path.
///
/// See `docs/spec/interfaces/context.md` §`MergedGuidance`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MergedGuidance {
    /// Union of `safe_patterns` from all matched packs.
    pub safe_patterns: Vec<String>,
    /// Union of `anti_patterns` from all matched packs.
    pub anti_patterns: Vec<String>,
    /// Deduplicated union of `required_artifacts` from all matched packs.
    pub required_artifacts: Vec<ArtifactPath>,
}

// ---------------------------------------------------------------------------

/// Output of context pack selection and guidance merging for one pipeline run.
///
/// Passed to [`assemble_context`] to inject pack knowledge and required artifacts.
///
/// See `docs/spec/interfaces/context.md` §`LoadedContextPacks`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedContextPacks {
    /// All packs selected for this run. May be empty.
    pub matched_packs: Vec<ContextPack>,

    /// Pre-merged guidance from all matched packs.
    pub merged_guidance: MergedGuidance,

    /// Strictest scenario threshold across matched packs.
    ///
    /// If no pack overrides the threshold, this should be set to the
    /// pipeline-level default by the caller of [`merge_pack_guidance`].
    pub strictest_threshold: SatisfactionScore,
}

// ─── Context assembly request ─────────────────────────────────────────────────

/// All parameters needed to assemble a [`ContextPackage`] for one node invocation.
///
/// Constructed by node orchestration code in `nodes` and passed to
/// [`assemble_context`].
///
/// See `docs/spec/interfaces/context.md` §`ContextAssemblyRequest`.
#[derive(Debug, Clone)]
pub struct ContextAssemblyRequest {
    /// The type of node being executed, used to select appropriate detail levels.
    ///
    /// LLM nodes typically need finer-grained summaries; deterministic nodes
    /// may not need summaries at all.
    pub node_type: NodeType,

    /// Natural-language description of the sub-task being worked on.
    ///
    /// Included verbatim as user context within the assembled package.
    pub sub_work_item_description: String,

    /// Repo-relative paths of modules the node is expected to interact with.
    ///
    /// Per-module summaries are fetched from `SummaryCache` for each of these.
    pub affected_modules: Vec<ArtifactPath>,

    /// Repo-relative directories containing scenario specification files.
    ///
    /// Items derived from these paths are stripped by [`enforce_scenario_holdout`]
    /// before assembly. This enforces the hard holdout constraint.
    pub scenario_holdout_dirs: Vec<ArtifactPath>,

    /// Root of the pipeline's working directory checkout.
    ///
    /// Used to resolve relative paths when fetching summaries from the cache.
    pub pipeline_working_dir: PathBuf,
}

// ─── Context assembly functions ───────────────────────────────────────────────

/// Returns the IDs of all Context Packs whose triggers match `classification`
/// and `active_labels`.
///
/// A pack is selected if any of the following is true:
/// - Any `trigger.label_patterns` glob matches any string in `active_labels`.
/// - Any `trigger.component_tag_patterns` glob matches any path in
///   `classification.affected_modules`.
/// - `trigger.requires_safety_critical` is `true` and
///   `classification.safety_affecting` is `true`.
///
/// Returns an empty `Vec` if no packs match. This is valid; the node proceeds
/// with no pack guidance.
///
/// # Arguments
///
/// - `classification` — classification result from the Intake node.
/// - `active_labels` — the current GitHub label strings on the work item;
///   matched against `trigger.label_patterns`. Pass an empty slice if label
///   context is not available.
/// - `available` — all Context Packs loaded from the configuration.
///
/// **Infallible. Pure.**
///
/// See `docs/spec/interfaces/context.md` §`select_context_packs`.
#[must_use]
pub fn select_context_packs(
    classification: &ClassificationResult,
    active_labels: &[String],
    available: &[ContextPack],
) -> Vec<ContextPackId> {
    available
        .iter()
        .filter(|pack| pack_trigger_matches(pack, classification, active_labels))
        .map(|pack| pack.id.clone())
        .collect()
}

// ---------------------------------------------------------------------------

/// Merges the guidance fields from all selected Context Packs into a single
/// [`MergedGuidance`].
///
/// Returns an empty [`MergedGuidance`] for an empty `packs` slice.
///
/// **Merge semantics**:
/// - `safe_patterns` — union of all packs' safe patterns.
/// - `anti_patterns` — union of all packs' anti-patterns.
/// - `required_artifacts` — union with deduplication by path.
///
/// **Infallible. Pure.**
///
/// See `docs/spec/interfaces/context.md` §`merge_pack_guidance`.
#[must_use]
pub fn merge_pack_guidance(packs: &[ContextPack]) -> MergedGuidance {
    let safe_patterns = packs
        .iter()
        .flat_map(|p| p.safe_patterns.iter().cloned())
        .collect();
    let anti_patterns = packs
        .iter()
        .flat_map(|p| p.anti_patterns.iter().cloned())
        .collect();
    let mut required_artifacts: Vec<ArtifactPath> = packs
        .iter()
        .flat_map(|p| p.required_artifacts.iter().cloned())
        .collect();
    required_artifacts.sort();
    required_artifacts.dedup();
    MergedGuidance {
        safe_patterns,
        anti_patterns,
        required_artifacts,
    }
}

// ---------------------------------------------------------------------------

/// Assembles a [`ContextPackage`] for one LLM node invocation.
///
/// Fetches summaries for required and affected artifacts from `summaries`,
/// combines them with interface definitions and pack guidance, enforces the
/// scenario holdout constraint, and truncates to the token budget.
///
/// # Arguments
///
/// - `req` — describes the node type, affected modules, and holdout directories.
/// - `summaries` — the pyramid summary cache; queried for each artifact path.
/// - `packs` — pre-loaded and merged Context Pack guidance and required artifacts.
/// - `interface_entries` — current interface definitions from the registry.
/// - `token_budget` — the maximum total token count for the returned package.
///
/// # Algorithm (in order)
///
/// 1. Convert `packs.merged_guidance.required_artifacts` + `req.affected_modules`
///    into `ContextItem` values. The finest **available** summary level is selected
///    (`Source` first, then `FullInterface`, `Paragraph`, `OneLine`); budget-fitting
///    is deferred to step 5.
/// 2. Add `interface_entries` as `CurrentInterfaceDefinition` items.
/// 3. Add merged pack guidance as a `ContextPackKnowledge` item.
/// 4. Call `enforce_scenario_holdout` with `req.scenario_holdout_dirs`.
/// 5. Call `apply_priority_truncation` with `token_budget`.
///
/// # Error policy
///
/// If `SummaryCache::get_summary` returns an error for an artifact, that
/// artifact is **skipped** (not returned as an error). The resulting package
/// has `truncation_applied = true` to signal incomplete data.
///
/// See `docs/spec/interfaces/context.md` §`assemble_context`.
pub async fn assemble_context(
    req: &ContextAssemblyRequest,
    summaries: &dyn SummaryCache,
    packs: &LoadedContextPacks,
    interface_entries: &[InterfaceDefinition],
    token_budget: TokenCount,
) -> ContextPackage {
    let mut items = Vec::new();
    let mut assembly_errors: Vec<String> = Vec::new();

    // Steps 1–2: collect unique artifact paths and fetch summaries.
    // Required artifacts (from merged_guidance) are marked so apply_priority_truncation
    // can include them even when the budget is exhausted by higher-priority items.
    let required_set: std::collections::HashSet<&ArtifactPath> =
        packs.merged_guidance.required_artifacts.iter().collect();
    let unique_paths = collect_unique_artifact_paths(packs, req);
    for path in &unique_paths {
        let is_required = required_set.contains(path);
        match fetch_best_summary(summaries, path).await {
            Ok(Some(item)) => items.push(summary_to_context_item(item, is_required)),
            Ok(None) => {}
            Err(msg) => assembly_errors.push(msg),
        }
    }

    // Step 3: add interface definitions as CurrentInterfaceDefinition items.
    for iface in interface_entries {
        items.push(interface_to_context_item(iface));
    }

    // Step 4: add combined pack domain knowledge as a single item.
    let knowledge = combine_domain_knowledge(packs);
    if !knowledge.is_empty() {
        let tc = estimate_token_count(&knowledge);
        items.push(ContextItem {
            content: knowledge,
            summary_level: SummaryLevel::Paragraph,
            priority: ContextPriority::ContextPackKnowledge,
            token_count: tc,
            required: false,
            source_path: None,
        });
    }

    // Step 5: enforce scenario holdout.
    let filtered = enforce_scenario_holdout(items, &req.scenario_holdout_dirs);

    // Step 6: priority-ordered greedy truncation.
    let mut package = apply_priority_truncation(filtered, token_budget);

    // Merge accumulated cache errors into the returned package.
    package.assembly_errors.extend(assembly_errors);
    if !package.assembly_errors.is_empty() {
        package.truncation_applied = true;
    }

    package
}

// ---------------------------------------------------------------------------

/// Sorts context items by priority (highest first) and greedily fills the
/// token budget, returning a [`ContextPackage`].
///
/// Accepts a [`HoldoutFilteredItems`] (produced by [`enforce_scenario_holdout`])
/// rather than a raw `Vec<ContextItem>`. This makes the scenario holdout a
/// compile-time requirement: calling this function without first calling
/// `enforce_scenario_holdout` is a type error.
///
/// Items at the same priority tier are ordered alphabetically by `source_path`
/// for reproducibility.
///
/// An item is included in full or excluded entirely. If a single item exceeds
/// the budget, it is **still included** (required artifacts must never be
/// silently dropped). `truncation_applied` is set `true` whenever any item
/// is dropped **or** when budget overflow occurs.
///
/// **Infallible. Pure.**
///
/// See `docs/spec/interfaces/context.md` §`apply_priority_truncation`.
#[must_use]
pub fn apply_priority_truncation(
    items: HoldoutFilteredItems,
    budget: TokenCount,
) -> ContextPackage {
    let mut items = items.into_inner();
    sort_context_items(&mut items);

    let mut included = Vec::new();
    let mut total = TokenCount::new(0);
    let mut truncation_applied = false;

    for item in items {
        let new_total = total + item.token_count;
        if new_total <= budget {
            total = new_total;
            included.push(item);
        } else if item.required {
            // Required artifacts are always included, even on budget overflow.
            total = new_total;
            included.push(item);
            truncation_applied = true;
        } else {
            truncation_applied = true;
        }
    }

    ContextPackage {
        items: included,
        total_token_count: total,
        truncation_applied,
        assembly_errors: vec![],
    }
}

// ---------------------------------------------------------------------------

/// Removes any context item whose `source_path` is rooted under one of the
/// holdout directories, returning a [`HoldoutFilteredItems`] that can be
/// passed directly to [`apply_priority_truncation`].
///
/// This is a **hard constraint**: scenario specifications must never be present
/// in code generation context (see `docs/spec/constraints.md` §Module Boundaries).
/// The [`HoldoutFilteredItems`] return type makes it a compile-time error to
/// call [`apply_priority_truncation`] without first calling this function.
///
/// Items with `source_path == None` are never removed.
///
/// **Infallible. Pure.**
///
/// See `docs/spec/interfaces/context.md` §`enforce_scenario_holdout`.
#[must_use]
pub fn enforce_scenario_holdout(
    items: Vec<ContextItem>,
    holdout_dirs: &[ArtifactPath],
) -> HoldoutFilteredItems {
    let filtered = items
        .into_iter()
        .filter(|item| {
            let Some(path) = &item.source_path else {
                return true; // None-source items are never removed.
            };
            !holdout_dirs.iter().any(|h| path_is_under_holdout(path, h))
        })
        .collect();
    HoldoutFilteredItems(filtered)
}

// ─── Private helpers ──────────────────────────────────────────────────────────

/// Returns `true` if any glob in `patterns` matches any string in `values`.
///
/// Invalid glob patterns are skipped with a `tracing::warn!` rather than
/// panicking, to keep matching infallible.
///
/// `context_label` is included in the warning message to identify which
/// configuration source produced the invalid pattern.
pub(crate) fn glob_matches_any<'a>(
    patterns: &[String],
    values: impl Iterator<Item = &'a str>,
    context_label: &str,
) -> bool {
    let values: Vec<&str> = values.collect();
    for pattern in patterns {
        match Glob::new(pattern) {
            Ok(glob) => {
                let matcher = glob.compile_matcher();
                if values.iter().any(|v| matcher.is_match(v)) {
                    return true;
                }
            }
            Err(_) => {
                tracing::warn!(
                    pattern = %pattern,
                    context = %context_label,
                    "invalid glob pattern — skipped"
                );
            }
        }
    }
    false
}

/// Returns `true` if any trigger field in `pack` matches `classification` or
/// `active_labels`.  Uses OR semantics across all fields.
fn pack_trigger_matches(
    pack: &ContextPack,
    classification: &ClassificationResult,
    active_labels: &[String],
) -> bool {
    if pack.trigger.requires_safety_critical && classification.safety_affecting {
        return true;
    }
    if glob_matches_any(
        &pack.trigger.label_patterns,
        active_labels.iter().map(String::as_str),
        "context pack trigger",
    ) {
        return true;
    }
    glob_matches_any(
        &pack.trigger.component_tag_patterns,
        classification
            .affected_modules
            .iter()
            .map(ArtifactPath::as_str),
        "context pack trigger",
    )
}

/// Returns `true` if `path` is rooted under `holdout` using directory-prefix
/// semantics: `"spec/scenarios"` matches `"spec/scenarios/foo.md"` but NOT
/// `"spec/scenarios-alt/foo.md"`.
fn path_is_under_holdout(path: &ArtifactPath, holdout: &ArtifactPath) -> bool {
    let p = path.as_str();
    let h = holdout.as_str();
    p == h || (p.starts_with(h) && p.as_bytes().get(h.len()) == Some(&b'/'))
}

/// Sorts `items` in-place: ascending by `priority` discriminant (lower = higher
/// priority = first), then alphabetically by `source_path` within the same tier.
/// Items with `source_path = None` sort before any path.
fn sort_context_items(items: &mut [ContextItem]) {
    items.sort_by(|a, b| {
        a.priority.cmp(&b.priority).then_with(|| {
            let a_path = a.source_path.as_ref().map_or("", ArtifactPath::as_str);
            let b_path = b.source_path.as_ref().map_or("", ArtifactPath::as_str);
            a_path.cmp(b_path)
        })
    });
}

/// Converts a [`PyramidSummary`] into a [`ContextItem`] at the
/// `DirectDependencyOutput` priority tier.
fn summary_to_context_item(summary: PyramidSummary, required: bool) -> ContextItem {
    ContextItem {
        priority: ContextPriority::DirectDependencyOutput,
        summary_level: summary.level,
        token_count: summary.token_count,
        source_path: Some(summary.path),
        content: summary.content,
        required,
    }
}

/// Converts an [`InterfaceDefinition`] into a [`ContextItem`] at the
/// `CurrentInterfaceDefinition` priority tier.
fn interface_to_context_item(iface: &InterfaceDefinition) -> ContextItem {
    let content = serde_json::to_string_pretty(&iface.schema).unwrap_or_default();
    let token_count = estimate_token_count(&content);
    ContextItem {
        content,
        summary_level: SummaryLevel::FullInterface,
        priority: ContextPriority::CurrentInterfaceDefinition,
        token_count,
        required: false,
        source_path: None,
    }
}

/// Estimates the token count for a text string (≈ 4 bytes per token).
///
/// A best-effort pre-call estimate only; exact counts are not required for
/// budget enforcement. Shared by any caller that needs to build a
/// [`ContextItem`] from raw text without an exact tokenizer count — e.g.
/// node `execute` functions that assemble an ad hoc [`ContextPackage`]
/// outside [`crate::assemble_context`] (see `nodes::intake::build_context_package`).
pub fn estimate_token_count(content: &str) -> TokenCount {
    let len = u64::try_from(content.len()).unwrap_or(u64::MAX);
    TokenCount::new(len / 4 + 1)
}

/// Returns the deduplicated union of `required_artifacts` and `affected_modules`.
fn collect_unique_artifact_paths(
    packs: &LoadedContextPacks,
    req: &ContextAssemblyRequest,
) -> Vec<ArtifactPath> {
    let mut paths: Vec<ArtifactPath> = packs
        .merged_guidance
        .required_artifacts
        .iter()
        .chain(req.affected_modules.iter())
        .cloned()
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// Concatenates non-empty `domain_knowledge` strings from all matched packs.
fn combine_domain_knowledge(packs: &LoadedContextPacks) -> String {
    packs
        .matched_packs
        .iter()
        .filter(|p| !p.domain_knowledge.is_empty())
        .map(|p| p.domain_knowledge.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Tries each [`SummaryLevel`] from finest to coarsest, returning the first
/// `Ok(Some(_))` result, `Ok(None)` if all levels miss, or `Err(String)` if
/// any level returns a cache error (subsequent levels are not tried after an
/// error).
async fn fetch_best_summary(
    summaries: &dyn SummaryCache,
    path: &ArtifactPath,
) -> Result<Option<PyramidSummary>, String> {
    const LEVELS: [SummaryLevel; 4] = [
        SummaryLevel::Source,
        SummaryLevel::FullInterface,
        SummaryLevel::Paragraph,
        SummaryLevel::OneLine,
    ];
    for level in LEVELS {
        match summaries.get_summary(path, level).await {
            Ok(Some(summary)) => return Ok(Some(summary)),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(
                    path = %path.as_str(),
                    error = %e,
                    "summary cache error — artifact skipped"
                );
                return Err(e.to_string());
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
