# Context Assembly & Knowledge — Interface Specification

**Architectural Layer**: Business logic (pure functions + async orchestration, no I/O infrastructure)
**Module Paths**:

- `crates/pipeline/src/context.rs` — context assembly, context packs, classification result
- `crates/pipeline/src/labels.rs` — pipeline label enum and parse/generate functions

**Specification Version**: 1.0

---

## Overview

This document specifies two closely related subsystems:

1. **Context Bundle Assembly** — admitting exactly the inputs a stage's contract
   allows, and fitting them within the stage's token budget. This includes:
   - Resolving the input allowlist from the resolved stage contract
   - Context Pack selection from the pinned harness
   - Context priority ranking and deterministic truncation
   - Trust-zone barrier enforcement, as defence in depth behind the allowlist
   - Hashing the result so what the agent saw is recorded and provable

2. **Pipeline Label Parsing** — mapping configured GitHub label strings to typed
   `PipelineLabel` values and back. Label names are configuration; this module
   handles the mapping, never a hardcoded name.

Assembly is **deterministic and takes no model input**. What an agent is shown
decides what it produces, so that decision is made by code from a recorded
allowlist, not by a model and not by whatever happened to be nearby.

---

## Dependencies

| This module uses | From |
| ----------------- | ------ |
| `ArtifactPath`, `ContextPackId` | `crates/pipeline/src/identifiers.rs` |
| `TokenCount` | `crates/pipeline/src/types.rs` |
| `InterfaceDefinition` | `crates/pipeline/src/domain_services.rs` |
| `StageTypeId` | `crates/pipeline/src/graph.rs` |
| `StageContract`, `TrustZone`, `BundleHash` | `crates/pipeline/src/harness.rs` |

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
    pub priority: ContextPriority,
    pub token_count: TokenCount,
    pub source_path: Option<ArtifactPath>,
}
```

- `content` — the verbatim text to be delivered to the LLM at this position.
- `allowlist_entry` — the input allowlist entry that admitted this item. Every
  item has one: an item with no admitting entry is a defect, not a default.
- `priority` — determines drop order when the token budget is tight.
- `token_count` — pre-computed count used for budget arithmetic. Must match
  `content.len()` in tokens at the LLM's tokenisation.
- `source_path` — the artifact this item was derived from; `None` for
  synthesised items such as merged pack guidance.

### ContextBundle

The complete, allowlisted set of inputs one stage attempt is permitted to see.

| Field | Type | Description |
|-------|------|-------------|
| `items` | `Vec<ContextItem>` | The admitted inputs, in assembled order |
| `allowlist` | `Vec<String>` | The stage contract's input allowlist, recorded so what *could* have been admitted is provable, not just what was |
| `bundle_hash` | `BundleHash` | SHA-256 over the canonical serialisation of `items` |
| `trust_zone` | `TrustZone` | The zone this bundle was built for; determines which barriers applied |
| `total_tokens` | `u32` | Estimated token count of `items` |
| `truncated` | `Vec<String>` | Non-mandatory inputs dropped to fit the budget, named for the audit trail |

**Allowlist, not denylist.** An input the allowlist does not name is **absent**
from `items` — not ranked lower, not truncated away, absent. A denylist fails
open: anything nobody thought to exclude is included. An allowlist fails closed.

This is also what makes the trust-zone barrier real. A build-zone bundle contains
no holdout content because its allowlist does not name holdouts, not because
something remembered to strip them.

**The stage contract is never an item.** The contract names the allowlist, the
writable paths, the limits and the identity role. Placing it in the bundle it
governs would let the agent reason about its own constraints.

**Hashed before use.** `bundle_hash` is computed over a canonical serialisation
and recorded before the attempt is requested. An attempt whose bundle was not
hashed is not requested. The hash goes into the stage attestation, so an attempt
is reproducible: the same profile at the same pin over the same bundle.

**Canonical serialisation** is required for the hash to be meaningful: items in
assembled order, object keys sorted, no insignificant whitespace. Two assemblies
of identical content must produce an identical hash.

---

## Part 3 — Context Pack Types

Context Packs are domain-specific knowledge bundles stored in `the harness discipline pack at `.
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
}
```

**Fields**:

- `id` — the pack's identifier within the harness discipline pack.
- `trigger` — the conditions under which this pack is selected.
- `domain_knowledge` — verbatim text injected into the `ContextPackKnowledge`
  priority tier.
- `safe_patterns` — examples of correct code or design patterns for this domain.
  Merged with most-restrictive-wins semantics by `merge_pack_guidance`.
- `anti_patterns` — examples of patterns to avoid. Union-merged across all packs.
- `required_artifacts` — artifacts that must be included regardless of priority
  truncation. Where these alone exceed the budget, assembly reports a structured
  error rather than dropping one silently.

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
}
```

- `matched_packs` — all packs selected for this run (may be empty).
- `merged_guidance` — pre-merged guidance ready for injection into the bundle.
  Where two packs disagree, the more restrictive rule applies: a pack may tighten
  guidance for its domain and may never relax a global threshold.

---

## Part 4 — Context Assembly Request

### ContextAssemblyRequest

All the parameters needed to assemble a `ContextBundle` for one node invocation.

```rust
pub struct ContextAssemblyRequest {
    pub node_type: NodeType,
    pub sub_work_item_description: String,
    pub affected_modules: Vec<ArtifactPath>,
    pub zone_barrier_paths: Vec<PathBuf>,
    pub pipeline_working_dir: PathBuf,
}
```

**Fields**:

- `node_type` — the kind of node being executed, used to select suitable detail levels.
  LLM nodes typically need finer-grained summaries; deterministic nodes may not need
  summaries at all.
- `sub_work_item_description` — the natural-language description of the sub-task.
  Included verbatim as user context.
- `affected_modules` — repo-relative paths the stage is expected to interact with.
  Used for relevance ranking, and to read the artifacts the allowlist admits.
- `zone_barrier_paths` — repo-relative paths the stage's trust zone must not see.
  Items derived from these paths are removed by `enforce_trust_zone` before
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

### fn assemble_bundle

```rust
pub async fn assemble_bundle(
    req: &ContextAssemblyRequest,
    packs: &LoadedContextPacks,
    interface_entries: &[InterfaceDefinition],
    token_budget: TokenCount,
) -> ContextBundle
```

Assembles a `ContextBundle` by:

1. Resolving the stage contract's input allowlist into candidate `ContextItem`
   values, drawing from `LoadedContextPacks.merged_guidance.required_artifacts`,
   `req.affected_modules`, the repository and the harness. An input the allowlist
   does not name produces no candidate. Budget-fitting is handled later by
   `apply_priority_truncation`, not here.
2. Adding interface definitions as `CurrentInterfaceDefinition`-priority items
   (up to one item per `InterfaceDefinition` entry).
3. Adding merged pack guidance text as a single `ContextPackKnowledge` item.
4. Calling `enforce_trust_zone` as defence in depth, stripping anything the
   stage's zone must not see that the allowlist nonetheless admitted.
5. Calling `apply_priority_truncation` to trim to `token_budget`.
6. Hashing the result and recording `bundle_hash`.

**Async** — reads admitted artifacts through the repository and harness loaders.

**Side effects**: reads only. Never writes.

**Error policy**: if `get_summary` returns an error for an artifact, that artifact
is skipped (not truncated) and the assembly continues. The resulting package
will have `truncation_applied = true` to signal that some data was unavailable.

---

### fn apply_priority_truncation

```rust
pub fn apply_priority_truncation(
    items: ZoneFilteredItems,
    budget: TokenCount,
) -> ContextBundle
```

Accepts a [`ZoneFilteredItems`] value produced by `enforce_trust_zone`.
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

### fn enforce_trust_zone

```rust
pub fn enforce_trust_zone(
    items: Vec<ContextItem>,
    zone: TrustZone,
) -> ZoneFilteredItems
```

Removes from a candidate item set anything the stage's trust zone must not see.

This is defence in depth, not the primary mechanism. The primary mechanism is the
allowlist: a build-zone contract does not name holdouts, so they never become
candidates. This function is the second line, catching the case where an
allowlist entry resolves more broadly than its author intended.

**Barriers by zone**:

| Zone | Contains | Must not see |
|------|----------|--------------|
| Design | `specify`, `architect`, `contract`, `plan` | implementation source |
| Oracle | `unit-oracle`, `acceptance-oracle` | implementation source |
| Build | `implement`, `refactor`, `debt-fix` | holdout content, acceptance source |
| Review | `review-semantic`, `review-security` | — (read-only, sees the change under review) |

The oracle barrier matters as much as the build barrier and is easier to forget.
An oracle that can see the implementation writes tests describing what was built
rather than what was asked for, which is a quieter failure than a build stage
reading a holdout but defeats the oracle just as completely.

### ZoneFilteredItems

| Field | Type | Description |
|-------|------|-------------|
| `retained` | `Vec<ContextItem>` | Items admitted for this zone |
| `removed` | `Vec<(String, TrustZone)>` | Item identifiers removed, with the zone barrier that removed each |

`removed` is **non-empty only when the allowlist let something through that the
zone forbids**, which means an allowlist needs correcting. A non-empty `removed`
is therefore recorded as a warning in the audit trail, not silently discarded —
the filter worked, but it should not have had to.

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
| `assemble_bundle` | Async; a read failure on an optional artifact skips it with a recorded warning, a read failure on a mandatory one is an error |
| `apply_priority_truncation` | Infallible |
| `enforce_trust_zone` | Infallible |
| `parse_label` | Returns `None` for unrecognised labels |
| `generate_label` | Infallible |

---

## Implementation Notes

- `apply_priority_truncation` must use a deterministic sort so the assembled context
  is reproducible; same inputs → same output.
- The `assemble_bundle` error-swallowing policy (skip on cache error, set
  `truncation_applied = true`) must be recorded in the tracing span for observability.
- `enforce_trust_zone` is applied **before** `apply_priority_truncation` to
  ensure holdout items never consume budget space.
- Context Packs live in `the harness discipline pack at <pack-id>/pack.toml`.
  Loading them is the responsibility of an infrastructure loader (wired in `cli`);
  this module only consumes already-loaded `ContextPack` values.
- `PathBuf` from `std::path` is intentionally used for `zone_barrier_paths` and
  `pipeline_working_dir` — these are local filesystem paths in the working checkout,
  not repo-relative artifact paths.
