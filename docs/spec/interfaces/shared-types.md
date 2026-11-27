# Shared Types — Interface Specification

**Architectural Layer**: Core domain (`pipeline` crate)
**Source files**: `crates/pipeline/src/lib.rs`, `crates/pipeline/src/identifiers.rs`,
`crates/pipeline/src/types.rs`, `crates/pipeline/src/errors.rs`
**Introduced in**: PR 1 (workspace foundation)

---

## Purpose

This document specifies every newtype domain identifier, shared value type, and
cross-cutting error type defined in the `pipeline` crate. These are the primitive
vocabulary on which all other interface definitions build.

Types here are used by every crate in the workspace. Changing them is a breaking
change across the whole codebase and requires all downstream modules to be reviewed.

---

## Identifiers

All domain identifiers use the newtype pattern to prevent accidental interchange
between — for example — a `WorkItemId` and a `PullRequestId` even though both
wrap a `u64`. See `docs/spec/constraints.md` §Type System.

### Integer-backed identifiers (GitHub-assigned)

These wrap `u64` and are `Copy`.

| Type | Wraps | Represents |
|------|-------|-----------|
| `WorkItemId` | `u64` | GitHub Issue number — the unit of work for CogWorks |
| `SubWorkItemId` | `u64` | GitHub Issue number for a slice sub-issue created from an approved plan |
| `MilestoneId` | `u64` | GitHub Milestone number (CogWorks inherits; never creates) |
| `PullRequestId` | `u64` | GitHub Pull Request number CogWorks opened or updated |
| `CheckRunId` | `u64` | GitHub check run carrying a gate verdict (read-only to CogWorks) |

**Constructor**: `T::new(value: u64) -> T`
**Accessor**: `T::as_u64(self) -> u64`

### UUID-backed identifiers (internally generated)

| Type | Wraps | Represents |
|------|-------|-----------|
| `PipelineRunId` | `uuid::Uuid` | Unique identifier for one CLI invocation (step-function run) |

**Constructors**: `PipelineRunId::new_random() -> Self`, `PipelineRunId::from_uuid(Uuid) -> Self`
**Accessor**: `PipelineRunId::as_uuid(self) -> Uuid`

A fresh `PipelineRunId` is generated per invocation. It is propagated through
all `tracing` spans and audit events for correlation.

### String-backed identifiers (configured / Git names)

These wrap `String` and are `Clone` (not `Copy`). Construction rejects empty strings.

| Type | Format | Represents |
|------|--------|-----------|
| `NodeId` | Free text | Node name within a resolved stage graph |
| `EdgeId` | Free text | Edge name within a resolved stage graph |
| `StageTypeId` | Free text | Stage type a node declares, drawn from the harness registry (e.g. `"implement"`) |
| `ProfileId` | Free text | Agent profile resolved for a stage from the harness |
| `HarnessVersion` | Free text | Harness version resolved from `.factory/pin.yaml` |
| `DisciplineId` | Free text | Discipline pack selected for the work (e.g. `"software"`) |
| `SliceId` | Free text | Slice within an approved `plan.yaml` |
| `IdentityRole` | Free text | Role a stage attempt's commits are pushed under (e.g. `"oracle"`, `"builder"`) |
| `BundleHash` | 64-char hex | SHA-256 of an assembled context bundle |
| `BranchName` | Git branch name | Git branch (e.g. `"cogworks/42/slice-3"`) |
| `CommitSha` | 40-char hex | Git commit SHA |
| `RepositoryId` | `"owner/repo"` | GitHub repository |
| `DomainServiceName` | Free text | Domain service key in CogWorks' process configuration |
| `ArtifactPath` | Repo-relative path | File produced or consumed by a stage |
| `InterfaceId` | Free text | Interface contract in the human-authored registry |
| `ContextPackId` | Directory name | Context Pack supplied by the harness |

**Constructor**: `T::new(value: impl Into<String>) -> Option<T>` — returns `None` on empty input.
**Accessor**: `T::as_str(&self) -> &str`

`NodeId` and `StageTypeId` are deliberately separate. A `NodeId` names an instance
with a position in the graph; a `StageTypeId` names the kind of work that node does
and is the key by which its agent profile is resolved (REQ-PROFSEL-001). Several
nodes in one graph may share a stage type — a graph with three `implement` nodes,
one per slice, has three distinct `NodeId` values and one `StageTypeId`.

### Tier

`Tier` is an enumeration, not a string newtype: `T0`, `T1`, `T2`, `T3`. It is ordered,
so the tier of a piece of work is the maximum over the tiers of the paths it touches
(REQ-TIER-001), and a derived tier can be compared to enforce raise-only semantics
(REQ-TIER-002).

---

## Value Types

### Token and Cost Types

#### `TokenCount`

Wraps `u64`. Represents the number of tokens consumed or budgeted in an LLM API
call. Implements `Add`, `AddAssign`, `Ord`.

```rust
pub fn new(count: u64) -> TokenCount
pub fn as_u64(self) -> u64
pub fn is_zero(self) -> bool
```

#### `TokenCost`

Wraps `f64` (US dollars). Represents the monetary cost of LLM token usage.
Implements `Add`, `AddAssign`, `PartialOrd`.

```rust
pub fn new(value: f64) -> Option<TokenCost>   // None if negative, infinite, or NaN
pub fn zero() -> TokenCost                    // infallible zero cost
pub fn as_f64(self) -> f64
pub fn is_zero(self) -> bool
```

**Display**: `"$0.000042"` (6 decimal places).

#### `CostBudget`

Wraps `f64` (US dollars). Represents a maximum token cost cap for a run, node,
or parallel budget window.

```rust
pub fn new(limit: f64) -> Option<CostBudget>  // None if not strictly positive/finite
pub fn as_f64(self) -> f64
pub fn is_exceeded_by(self, accumulated: TokenCost) -> bool
```

**Constraint**: Cost budget acquisition across parallel nodes **must be atomic**.
See `docs/spec/constraints.md` §Pipeline Graph.

### Score Types

#### `AlignmentScore`

Wraps `f64` in `[0.0, 1.0]`. Alignment verification score for both deterministic
and LLM-semantic checks.

```rust
pub fn new(value: f64) -> Option<AlignmentScore>      // None if out of range
pub fn as_f64(self) -> f64
```

**Critical**: A `DiagnosticSeverity::Blocking` finding always fails an alignment
check regardless of score. Score threshold is necessary but not sufficient.

### Diagnostic Types

#### `DiagnosticSeverity`

```rust
pub enum DiagnosticSeverity {
    Blocking,        // Blocks progression; check fails
    Warning,         // Should be addressed; does not block
    Informational,   // Context only; no progression impact
}
```

#### `DiagnosticCategory`

Wraps `String`. The standardised set is:

`syntax_error` · `type_error` · `constraint_violation` · `interface_mismatch` ·
`dependency_error` · `style_violation` · `safety_concern` · `performance_concern` ·
`test_failure` · `completeness`

Domain services may emit custom categories. Consumers treat unknown categories as
`Informational`.

```rust
pub fn new(category: impl Into<String>) -> Option<DiagnosticCategory>  // None if empty
pub fn as_str(&self) -> &str
```

#### `Diagnostic`

Structured finding produced by domain services, alignment checkers, and review
passes.

```rust
pub struct Diagnostic {
    pub artifact: Option<ArtifactPath>,  // None if not file-specific
    pub location: Option<String>,        // e.g. "line 42, column 5"; None = whole file
    pub severity: DiagnosticSeverity,
    pub category: DiagnosticCategory,
    pub message: String,
}
```

### Versioning

#### `ApiVersion`

Semantic version of the Extension API protocol. Used during health-check
handshake to negotiate compatibility.

```rust
pub struct ApiVersion { pub major: u32, pub minor: u32 }

pub fn new(major: u32, minor: u32) -> ApiVersion
pub fn is_compatible_with(self, other: ApiVersion) -> bool
// Compatible: same major, other.minor >= self.minor
```

**Display**: `"1.2"`.

### Time

#### `Timestamp`

Wraps `chrono::DateTime<Utc>`. Used in audit events, state records, and rate-limit
tracking.

```rust
pub fn now() -> Timestamp
pub fn from_utc(dt: DateTime<Utc>) -> Timestamp
pub fn as_datetime(self) -> DateTime<Utc>
```

**Display**: RFC 3339 format (e.g. `"2026-03-01T12:00:00Z"`).

---

## Error Types

### `RetryPolicy`

Cross-cutting indication of whether an error condition is safe to retry.

```rust
pub enum RetryPolicy {
    Retryable { after: Option<Duration> },
    NonRetryable,
}
```

**Rules**: Retryable = API timeouts, transient rate limits. NonRetryable = budget
exceeded, invalid configuration, injection detected, constitutional rules missing.

Infrastructure error types that participate in retry decisions must be able to
produce a `RetryPolicy` (typically via a method `retry_policy(&self) -> RetryPolicy`).

### `CogWorksError`

Top-level error type for conditions that halt or escalate the pipeline.

| Variant | When produced | Retry? |
|---------|---------------| -------|
| `PipelineHalt { reason }` | Scope enforcer, injection guard, human-gate abort | No |
| `BudgetExceeded { accumulated, limit }` | Budget enforcement during node execution | No |
| `InjectionDetected { source_document, offending_text }` | Constitutional layer pre-prompt check | No — hold state |
| `ConstitutionalRulesMissing` | Rules file missing or invalid at startup | No |
| `ProtectedPathViolation { path }` | Filesystem write tool against protected path | No |
| `ScopeViolation { description }` | Scope enforcer — capability not in approved spec | No |
| `ConfigurationError { message }` | Configuration load-time validation failure | No |

**None of these variants are retryable.** Human intervention is required in all cases.

---

## Usage Examples

```rust
use pipeline::{WorkItemId, PipelineRunId, TokenCost, CostBudget, CogWorksError};

// Create identifiers
let work_item = WorkItemId::new(42);
let run_id = PipelineRunId::new_random();

// Cost tracking — constructors return Option; the ? operator propagates None as a
// ConfigurationError in contexts where invalid values are programmer errors.
let call_cost = TokenCost::new(0.000_123).expect("literal is valid");
let budget = CostBudget::new(1.00).expect("literal is valid");
if budget.is_exceeded_by(call_cost) {
    return Err(CogWorksError::BudgetExceeded {
        accumulated: call_cost,
        limit: budget,
    });
}

// Zero cost (e.g. for accumulator initialisation) — use the infallible constructor:
let mut accumulated = TokenCost::zero();
```

---

## Implementation Notes

- Identifiers implement `Serialize`/`Deserialize` (serde) so they appear correctly
  in `PipelineStateComment` JSON written to GitHub.
- `TokenCost` and `CostBudget` use `f64`. The precision is adequate for USD amounts
  at LLM token granularity (sub-cent per call). Do not use these for financial
  reporting; they are operational cost tracking only.
- `AlignmentScore`'s constructor returns `Option` rather than `Result` because the
  range check is a pure invariant validation, not a recoverable failure mode.
