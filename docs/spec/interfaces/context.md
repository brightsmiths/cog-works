# Context Assembly & Knowledge — Interface Specification

**Architectural Layer**: Business logic (pure functions + async orchestration, no I/O infrastructure)
**Module Paths**:

- `crates/pipeline/src/context.rs` — context assembly, context packs, classification result
- `crates/pipeline/src/labels.rs` — pipeline label enum and parse/generate functions

**Specification Version**: 1.0

---

## Overview

This document specifies two closely related subsystems:

1. **Context Assembly** — selecting and assembling the right knowledge into a
   `ContextPackage` that fits within a node's token budget. This includes:
   - Context priority ranking
   - Context Pack selection based on classification
   - Pyramid summary selection (coarsest level that fits at each priority tier)
   - Scenario holdout enforcement (prevents leakage into code generation context)

2. **Pipeline Label Parsing** — mapping configured GitHub label strings (for example, the default `"cogworks:run"` readiness mapping)
   to typed `PipelineLabel` values and back. Labels drive the pipeline state machine.

The context assembly functions are the primary bridge between classification results,
knowledge summaries (via `SummaryCache`), and the LLM input for each node.

---

## Dependencies

| This module uses | From |
| ----------------- | ------ |
| `ArtifactPath`, `ContextPackId` | `crates/pipeline/src/identifiers.rs` |
| `SatisfactionScore`, `TokenCount` | `crates/pipeline/src/types.rs` |
| `SummaryCache` (trait), `SummaryLevel`, `PyramidSummary` | `crates/pipeline/src/knowledge.rs` |
| `InterfaceDefinition` | `crates/pipeline/src/domain_services.rs` |
| `NodeType` | `crates/pipeline/src/graph.rs` |

---

## Part 1 — Classification Result

Classification is the first step of every pipeline run (Intake node). The result
drives context pack selection, scope threshold checks, and safety gating throughout
the rest of the run. The types are defined here because context assembly is their
primary consumer; processing functions (safety override, scope threshold) will be
added in `classification.rs` in PR 7.

### TaskType

The classification of the work the pipeline has been asked to do.

```rust
pub enum TaskType {
    Feature,
    BugFix,
    Documentation,
    Refactoring,
    Configuration,
    Testing,
    Security,
    Unknown,
}
```

### ClassificationResult

The Intake node's classification of the work item.

```rust
pub struct ClassificationResult {
    pub task_type: TaskType,
    pub safety_affecting: bool,
    pub estimated_scope: u32,
    pub affected_modules: Vec<ArtifactPath>,
}
```

**Fields**:

- `task_type` — what category of work this is.
- `safety_affecting` — `true` if any affected module is in the safety-critical registry
  (see `SafetyCriticalRegistry` in PR 7). Safety-affecting tasks require human approval
  before any PR is merged.
- `estimated_scope` — a magnitude estimate `1..=10` (1 = trivial, 10 = large).
  Used by `check_scope_threshold` (PR 7) to detect over-scoped tasks.
- `affected_modules` — repo-relative paths of modules the change is expected to touch.

---

## Part 2 — Context Item and Package

### ContextPriority

Priority rank for a context item. Items with higher priority (lower numeric value)
are retained first when the token budget is exhausted.

The variants are declared in descending priority order (highest first):

```rust
pub enum ContextPriority {
    CurrentInterfaceDefinition = 0,
    DirectDependencyOutput     = 1,
    ArchitecturalConstraints   = 2,
    ContextPackKnowledge       = 3,
    CodingStandards            = 4,
    TransitiveDependency       = 5,
}
```

`Ord` is derived from declaration order. `apply_priority_truncation` sorts by `Ord`
then fills the budget from the front.

### ContextItem

A single unit of knowledge assembled for a node's context window.

```rust
pub struct ContextItem {
    pub content: String,
    pub summary_level: SummaryLevel,
    pub priority: ContextPriority,
    pub token_count: TokenCount,
    pub source_path: Option<ArtifactPath>,
}
```

- `content` — the verbatim text to be delivered to the LLM at this position.
- `summary_level` — the pyramid level (`OneLine` through `Source`) at which this
  entry was included. `None` for items with no corresponding pyramid summary
  (e.g. interface definitions or pack guidance text).
- `priority` — determines drop order when the token budget is tight.
- `token_count` — pre-computed count used for budget arithmetic. Must match
  `content.len()` in tokens at the LLM's tokenisation.
- `source_path` — the artifact this item was derived from; `None` for
  synthesised items such as merged pack guidance.

### ContextPackage

The complete assembled context for a single LLM call.

```rust
pub struct ContextPackage {
    pub items: Vec<ContextItem>,
    pub total_token_count: TokenCount,
    pub truncation_applied: bool,
    pub assembly_errors: Vec<String>,
}
```

- `items` — ordered from highest to lowest priority. Items at the same priority
  level are ordered by `source_path` (alphabetical) for reproducibility.
- `total_token_count` — sum of `item.token_count` for all items in the package.
  Must be ≤ the budget passed to `assemble_context`.
- `truncation_applied` — `true` if one or more items were dropped to fit the budget,
  or if a single required artifact exceeds the budget (budget overflow). Does **not**
  indicate cache fetch errors — see `assembly_errors`.
- `assembly_errors` — descriptions of artifacts skipped because `SummaryCache` returned
  an error. An empty vec means the context is complete. A non-empty vec signals
  incomplete data; callers should surface these errors for observability but may
  still proceed.

---

## Part 3 — Context Pack Types

Context Packs are domain-specific knowledge bundles stored in `.cogworks/context-packs/`.
They are selected at runtime based on the classification result and merged into a single
knowledge payload for the node.

### ContextPackTrigger

Determines when a Context Pack is activated for a given pipeline run.

```rust
pub struct ContextPackTrigger {
    pub label_patterns: Vec<String>,
    pub component_tag_patterns: Vec<String>,
    pub requires_safety_critical: bool,
}
```

- `label_patterns` — glob patterns matched against the work item's label strings.
  Evaluated by `select_context_packs` when the caller passes `active_labels`.
  Unlike `component_tag_patterns`, label information is not present in
  `ClassificationResult` and must be supplied separately by the caller.
- `component_tag_patterns` — glob patterns matched against module path strings in
  `ClassificationResult.affected_modules`. A pack is triggered if any pattern
  matches any affected module path.
- `requires_safety_critical` — if `true`, the pack is only selected when
  `ClassificationResult.safety_affecting == true`.

A pack is selected if **any** trigger criterion matches (OR semantics across fields).

### ContextPack

A domain-specific knowledge bundle.

```rust
pub struct ContextPack {
    pub id: ContextPackId,
    pub trigger: ContextPackTrigger,
    pub domain_knowledge: String,
    pub safe_patterns: Vec<String>,
    pub anti_patterns: Vec<String>,
    pub required_artifacts: Vec<ArtifactPath>,
    pub scenario_threshold_override: Option<SatisfactionScore>,
}
```

**Fields**:

- `id` — matches the Context Pack directory name in `.cogworks/context-packs/`.
- `trigger` — the conditions under which this pack is selected.
- `domain_knowledge` — verbatim text injected into the `ContextPackKnowledge`
  priority tier.
- `safe_patterns` — examples of correct code or design patterns for this domain.
  Merged with most-restrictive-wins semantics by `merge_pack_guidance`.
- `anti_patterns` — examples of patterns to avoid. Union-merged across all packs.
- `required_artifacts` — artifacts that must be included (at any summary level)
  regardless of priority truncation.
- `scenario_threshold_override` — per-pack scenario satisfaction threshold;
  the strictest across all matched packs is applied. `None` uses the pipeline default.

### MergedGuidance

The result of merging guidance from all matched Context Packs.

```rust
pub struct MergedGuidance {
    pub safe_patterns: Vec<String>,
    pub anti_patterns: Vec<String>,
    pub required_artifacts: Vec<ArtifactPath>,
}
```

**Merge semantics**:

- `safe_patterns` — **union** of all packs' `safe_patterns`.
- `anti_patterns` — **union** of all packs' `anti_patterns` (most restrictive wins).
- `required_artifacts` — **union**; deduplication by path.

### LoadedContextPacks

The output of pack selection and guidance merging for one pipeline run.

```rust
pub struct LoadedContextPacks {
    pub matched_packs: Vec<ContextPack>,
    pub merged_guidance: MergedGuidance,
    pub strictest_threshold: SatisfactionScore,
}
```

- `matched_packs` — all packs selected for this run (may be empty).
- `merged_guidance` — pre-merged guidance ready for injection into the context.
- `strictest_threshold` — the lowest `scenario_threshold_override` across all matched
  packs. If no pack overrides the threshold, this equals the pipeline-level default.

---

## Part 4 — Context Assembly Request

### ContextAssemblyRequest

All the parameters needed to assemble a `ContextPackage` for one node invocation.

```rust
pub struct ContextAssemblyRequest {
    pub node_type: NodeType,
    pub sub_work_item_description: String,
    pub affected_modules: Vec<ArtifactPath>,
    pub scenario_holdout_dirs: Vec<PathBuf>,
    pub pipeline_working_dir: PathBuf,
}
```

**Fields**:

- `node_type` — the kind of node being executed, used to select suitable detail levels.
  LLM nodes typically need finer-grained summaries; deterministic nodes may not need
  summaries at all.
- `sub_work_item_description` — the natural-language description of the sub-task.
  Included verbatim as user context.
- `affected_modules` — repo-relative paths the node is expected to interact with.
  Used to fetch per-module summaries from `SummaryCache`.
- `scenario_holdout_dirs` — repo-relative directories containing scenario files.
  Items derived from these paths are removed by `enforce_scenario_holdout` before
  context assembly. Uses `ArtifactPath` (not `PathBuf`) to stay platform-neutral.
- `pipeline_working_dir` — root of the pipeline's working directory checkout.
  Used to resolve relative paths when fetching summaries.

---

## Part 5 — Context Assembly Functions

### fn select_context_packs

```rust
pub fn select_context_packs(
    classification: &ClassificationResult,
    active_labels: &[String],
    available: &[ContextPack],
) -> Vec<ContextPackId>
```

Evaluates every available pack's trigger against `classification` and
`active_labels` and returns the IDs of all packs that match.

**Returns** an empty `Vec` if no packs match (valid; node proceeds with no pack guidance).

**Infallible. Pure (synchronous).**

Matching rules:

- `trigger.label_patterns`: compared against each string in `active_labels`.
  Pass an empty slice when label context is not available.
- `trigger.component_tag_patterns`: compared against each path in
  `classification.affected_modules`.
- `trigger.requires_safety_critical`: pack excluded if `!classification.safety_affecting`.

---

### fn merge_pack_guidance

```rust
pub fn merge_pack_guidance(packs: &[ContextPack]) -> MergedGuidance
```

Merges all selected packs' guidance fields into a single `MergedGuidance`.

Returns an empty `MergedGuidance` for an empty `packs` slice.

**Infallible. Pure.**

---

### fn assemble_context

```rust
pub async fn assemble_context(
    req: &ContextAssemblyRequest,
    summaries: &dyn SummaryCache,
    packs: &LoadedContextPacks,
    interface_entries: &[InterfaceDefinition],
    token_budget: TokenCount,
) -> ContextPackage
```

Assembles a `ContextPackage` by:

1. Converting every `LoadedContextPacks.merged_guidance.required_artifacts` plus
   `req.affected_modules` into `ContextItem` values. For each artifact, the finest
   available summary level is selected by querying `SummaryCache` (trying `Source`
   first, falling back to `FullInterface`, `Paragraph`, `OneLine`). Budget-fitting
   is handled later by `apply_priority_truncation`, not here.
2. Adding interface definitions as `CurrentInterfaceDefinition`-priority items
   (up to one item per `InterfaceDefinition` entry).
3. Adding merged pack guidance text as a single `ContextPackKnowledge` item.
4. Calling `enforce_scenario_holdout` to strip items derived from holdout directories.
5. Calling `apply_priority_truncation` to trim to `token_budget`.

**Async** — calls `SummaryCache::get_summary` for each artifact.

**Side effects**: none beyond the `SummaryCache` reads. Never writes.

**Error policy**: if `get_summary` returns an error for an artifact, that artifact
is skipped (not truncated) and the assembly continues. The resulting package
will have `truncation_applied = true` to signal that some data was unavailable.

---

### fn apply_priority_truncation

```rust
pub fn apply_priority_truncation(
    items: HoldoutFilteredItems,
    budget: TokenCount,
) -> ContextPackage
```

Accepts a [`HoldoutFilteredItems`] value produced by `enforce_scenario_holdout`.
The distinct type makes the call order a compile-time constraint: passing a raw
`Vec<ContextItem>` is a type error.

Sorts items by `ContextPriority` (highest first, then alphabetical by `source_path`
for stability), then greedily fills the budget.

An item is included in full or excluded entirely — partial inclusion is not supported.
If a required artifact's item exceeds the budget on its own, it is still included
(budget overflow; logged as a warning). This guarantees that required artifacts are
never silently dropped.

**Infallible. Pure.**

---

### fn enforce_scenario_holdout

```rust
pub fn enforce_scenario_holdout(
    items: Vec<ContextItem>,
    holdout_dirs: &[ArtifactPath],
) -> HoldoutFilteredItems
```

Removes any `ContextItem` whose `source_path` is rooted under one of the holdout
directories. This is a **hard constraint** (documented in `docs/spec/constraints.md`
§Module Boundaries): scenario specifications must never be present in code generation
context.

Items with `source_path == None` are never removed.

Returns a [`HoldoutFilteredItems`] wrapper. Pass it directly to
`apply_priority_truncation`. Call `into_inner()` only when working outside the
normal `assemble_context` flow.

**Infallible. Pure.**

---

### HoldoutFilteredItems

```rust
pub struct HoldoutFilteredItems(Vec<ContextItem>);

impl HoldoutFilteredItems {
    pub fn into_inner(self) -> Vec<ContextItem>;
}
```

A newtype wrapper over `Vec<ContextItem>` that witnesses that
`enforce_scenario_holdout` has been applied. `apply_priority_truncation` requires
this type rather than a raw `Vec<ContextItem>`, so swapping the call order is
caught at compile time.

`into_inner` is provided for callers that need the raw items outside the
standard flow (e.g., tests). Normal production code passes the value directly
to `apply_priority_truncation` without unwrapping.

---

## Part 6 — Pipeline Labels

Labels are GitHub label strings that drive the pipeline state machine. The pipeline
uses them to signal state, lock concurrent runs, and receive human decisions.

### PipelineLabel

```rust
pub enum PipelineLabel {
    // ── Trigger labels ──────────────────────────────────────────
    /// Default adapter mapping "cogworks:run" — starts the pipeline for a ready work item.
    Run,
    /// Default adapter mapping "cogworks:restart" — restarts the pipeline from the last checkpoint.
    Restart,

    // ── Status labels ────────────────────────────────────────────
    /// Default adapter mapping "cogworks:status:running" — pipeline is currently executing.
    Running,
    /// Default adapter mapping "cogworks:status:done" — pipeline completed successfully.
    Done,
    /// Default adapter mapping "cogworks:status:failed" — pipeline failed and requires attention.
    Failed,
    /// Default adapter mapping "cogworks:status:escalated" — pipeline escalated to a human.
    Escalated,

    // ── Gate labels ──────────────────────────────────────────────
    /// Default adapter mapping "cogworks:gate:pending" — a human-gated node is awaiting approval.
    HumanGatePending,
    /// Default adapter mapping "cogworks:gate:approved" — human reviewer approved a gated node.
    HumanGateApproved,
    /// Default adapter mapping "cogworks:gate:rejected" — human reviewer rejected a gated node.
    HumanGateRejected,

    // ── Processing lock ──────────────────────────────────────────
    /// Default adapter mapping "cogworks:lock" — mutual-exclusion lock; prevents concurrent pipeline runs.
    Lock,

    // ── Security and safety ──────────────────────────────────────
    /// Default adapter mapping "cogworks:security-hold" — injection or scope violation detected; halt.
    SecurityHold,
    /// Default adapter mapping "cogworks:safety-critical" — issue touches safety-critical modules.
    SafetyCritical,
}
```

### fn parse_label

```rust
pub fn parse_label(s: &str) -> Option<PipelineLabel>
```

Parses a GitHub label string into a `PipelineLabel` variant. Returns `None` for
any label string that does not correspond to a known pipeline label (i.e.
non-CogWorks labels are silently ignored).

**Infallible. Pure.**

### fn generate_label

```rust
pub fn generate_label(label: &PipelineLabel) -> &'static str
```

Returns the canonical GitHub label string for a pipeline label as a `&'static str`
(no allocation).

`parse_label(generate_label(label))` must always equal `Some(label.clone())`.

**Infallible. Pure.**

---

## Error Handling

| Function | Error / fallibility |
| ---------- | ------------------- |
| `select_context_packs` | Infallible |
| `merge_pack_guidance` | Infallible |
| `assemble_context` | Async; `SummaryCache` errors cause affected artifacts to be skipped, not propagated |
| `apply_priority_truncation` | Infallible |
| `enforce_scenario_holdout` | Infallible |
| `parse_label` | Returns `None` for unrecognised labels |
| `generate_label` | Infallible |

---

## Implementation Notes

- `apply_priority_truncation` must use a deterministic sort so the assembled context
  is reproducible; same inputs → same output.
- The `assemble_context` error-swallowing policy (skip on cache error, set
  `truncation_applied = true`) must be recorded in the tracing span for observability.
- `enforce_scenario_holdout` is applied **before** `apply_priority_truncation` to
  ensure holdout items never consume budget space.
- Context Packs live in `.cogworks/context-packs/<pack-id>/pack.toml`.
  Loading them is the responsibility of an infrastructure loader (wired in `cli`);
  this module only consumes already-loaded `ContextPack` values.
- `PathBuf` from `std::path` is intentionally used for `scenario_holdout_dirs` and
  `pipeline_working_dir` — these are local filesystem paths in the working checkout,
  not repo-relative artifact paths.
