# GitHub Traits — Interface Specification

**Architectural Layer**: Domain trait definitions (in `pipeline`) + Infrastructure stubs (`github`, `listener`)
**Module Paths**:

- `crates/pipeline/src/github.rs` — `EventSource`, `IssueTracker`, `PullRequestManager`, `CodeRepository`, `ProjectBoard` + data types
- `crates/pipeline/src/templates.rs` — `TemplateEngine`
- `crates/pipeline/src/audit.rs` — `AuditStore`, `AuditEvent`, `PipelineSummary`
- `crates/github/src/lib.rs` — `GithubClient` implementing all GitHub traits + `AuditStore`
- `crates/listener/src/lib.rs` — `GitHubWebhookEventSource`, `QueueEventSource` implementing `EventSource`

**Specification Version**: PR 3

---

## Overview

This document specifies all traits and supporting data types that connect the
`pipeline` business logic to GitHub and to external trigger sources.

```
pipeline (trait definitions)
    EventSource ←──────── GitHubWebhookEventSource  (listener)
                 ←──────── QueueEventSource           (listener)
    IssueTracker ←──────── GithubClient               (github)
    PullRequestManager ←── GithubClient               (github)
    CodeRepository ←─────── GithubClient               (github)
    ProjectBoard ←────────── GithubClient               (github)
    AuditStore ←──────────── GithubClient               (github)
    TemplateEngine ←───────── (implemented in PR 10)
```

**Architectural rules** (from `docs/spec/constraints.md`):

- `pipeline` declares traits; it never depends on `github`, `listener`, or any I/O crate.
- Infrastructure crates implement traits but must not add domain rules.
- `cli` is the only crate that constructs concrete instances and wires them together.

---

## Dependencies

| This module uses | From spec |
|-----------------|-----------|
| `WorkItemId`, `SubWorkItemId`, `PullRequestId`, `MilestoneId` | `shared-types.md` |
| `RepositoryId`, `BranchName`, `CommitSha` | `shared-types.md` |
| `NodeId`, `PipelineRunId` | `shared-types.md` |
| `TokenCost`, `TokenCount`, `ArtifactPath` | `shared-types.md` |
| `EdgeEvaluationRecord`, `NodeStatus` | `pipeline-graph.md` |

---

## Part 1 — `pipeline/src/github.rs`

### GitHubEvent

Enum of events that may trigger or advance a pipeline step. The `cli` event loop
calls `EventSource::next_event` and dispatches each variant.

```rust
pub enum GitHubEvent {
    WorkItemReady { work_item_id: WorkItemId },
    LabelApplied { work_item_id: WorkItemId, label: String },
    CommentPosted { work_item_id: WorkItemId, author: String, body: String },
    SubIssueStateChanged { sub_work_item_id: SubWorkItemId, new_state: IssueState },
    PullRequestReviewed { pr_id: PullRequestId, decision: ReviewDecision },
}
```

#### Variant Contracts

| Variant | When delivered | Pipeline action |
|---------|----------------|-----------------|
| `WorkItemReady` | EventSource determines that the work item meets the configured readiness condition | Intake node start |
| `LabelApplied` | EventSource reports a label change | Update provider-specific work-item state; a particular label is not defined as the trigger |
| `CommentPosted` | EventSource reports a comment | Process a supported work-item command or approval indication |
| `SubIssueStateChanged` | EventSource reports a sub-work-item state change | Fan-in readiness evaluation |
| `PullRequestReviewed` | EventSource reports a PR review update | Evaluate the relevant CogWorks node gate |

**Readiness mapping**: `WorkItemReady` is the normalized event presented to the
pipeline. The EventSource/GitHub adapter determines readiness from its provider
configuration and current work-item state. This interface does not prescribe a
label, project status, webhook event/action mapping, or GateKeeper/factory
approval encoding. Such external approvals are distinct from CogWorks node
gates and are passed through the work-item interface.

---

### EventSourceError

```rust
pub enum EventSourceError {
    Timeout,
    ConnectionLost { message: String },
    ParseError { raw: String },
    AuthError,
    QueueError { provider: String },
}
```

**Retry semantics**:

| Variant | Retryable? | Notes |
|---------|-----------|-------|
| `Timeout` | Yes (loop again) | Not an error; callers treat `Ok(None)` and `Timeout` identically |
| `ConnectionLost` | Yes (after back-off) | Implementation should reconnect internally first |
| `ParseError` | No (drop event) | Log raw payload to audit; dead-letter message |
| `AuthError` | No | Operator intervention required (wrong HMAC secret or queue credential) |
| `QueueError` | Depends | Log and retry up to `max_retry_attempts` |

---

### WebhookConfig

Configuration for `GitHubWebhookEventSource`.

| Field | Type | Description |
|-------|------|-------------|
| `bind_address` | `std::net::SocketAddr` | Local address to bind the HTTP server |
| `path_prefix` | `String` | URL path prefix (e.g. `"/hooks"`) |
| `secret` | `String` | HMAC-SHA256 secret matching GitHub webhook settings. Excluded from `Debug` (prints `"[REDACTED]"`). **Never logged.** |

---

### QueueEventConfig

Configuration for `QueueEventSource`.

| Field | Type | Description |
|-------|------|-------------|
| `provider_config` | `serde_json::Value` | Serialised `queue_runtime::ProviderConfig`; deserialized by `listener` |
| `queue_name` | `String` | Queue or topic subscription name |
| `use_session_ordering` | `bool` | When `true`, use `WorkItemId` as session key |
| `max_retry_attempts` | `u32` | Dead-letter after this many delivery failures |

---

### EventSource

```rust
#[async_trait]
pub trait EventSource: Send {
    async fn next_event(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<GitHubEvent>, EventSourceError>;
}
```

**Contract**:

- `Ok(Some(event))` — event is available; dispatch immediately.
- `Ok(None)` — no event within `timeout`; loop and call again.
- `Err(EventSourceError::Timeout)` — treated identically to `Ok(None)`.
- `Err(e)` — log and surface to operator. Do not halt the process; log and retry.

**Implementations**:

| Struct | Crate | Mode |
|--------|-------|------|
| `GitHubWebhookEventSource` | `listener` | Webhook (direct or smee.io) |
| `QueueEventSource` | `listener` | Azure Service Bus / AWS SQS |
| synthesised one-shot event | `cli` | Manual CLI invocation |

---

### IssueState, Label, Milestone

```rust
pub enum IssueState { Open, Closed }

pub struct Label {
    pub name: String,
    pub color: Option<String>,  // CSS hex, no '#'
}

pub struct Milestone {
    pub id: MilestoneId,
    pub title: String,
    pub due_on: Option<DateTime<Utc>>,
}
```

`IssueState` represents the provider's basic open/closed lifecycle only. Any
readiness stage or external approval status used by the pipeline is normalized
by the work-item/EventSource adapter; optional project-board fields are not a
required source of pipeline readiness.

---

### TypedLinkKind, TypedLink

```rust
pub enum TypedLinkKind { Blocks, IsBlockedBy }

pub struct TypedLink {
    pub source_id: WorkItemId,
    pub target_id: WorkItemId,
    pub kind: TypedLinkKind,
}
```

Maps to GitHub GraphQL `issueLink` type. CogWorks uses `Blocks`/`IsBlockedBy` for
sub-task dependency edges created by the Planning node.

---

### Issue, SubIssue

```rust
pub struct Issue {
    pub id: WorkItemId,
    pub repository: RepositoryId,
    pub title: String,
    pub body: String,
    pub state: IssueState,
    pub labels: Vec<Label>,
    pub milestone: Option<Milestone>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct SubIssue {
    pub id: SubWorkItemId,
    pub parent_id: WorkItemId,
    pub title: String,
    pub state: IssueState,
    pub created_at: DateTime<Utc>,
}
```

---

### GitHubOperationError

```rust
pub enum GitHubOperationError {
    NotFound { resource: String },
    PermissionDenied { action: String },
    RateLimitExhausted { reset_at: DateTime<Utc> },
    Transient { message: String },
    ParseFailure { message: String },
    SdkCapabilityMissing { capability: String },
}
```

`SdkCapabilityMissing` is returned by stub methods blocked on
`github-bot-sdk` additions. See SDK Gap Table below.

---

### IssueComment

A single comment posted on a GitHub Issue.

```rust
pub struct IssueComment {
    pub id: u64,
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}
```

Used for:
- **Pipeline state reconstruction on resume**: `run_step` step 3 calls
  `list_comments` and scans for a comment whose body parses as a valid
  `PipelineStateComment` JSON object. The latest such comment wins.
- **Stale-lock detection** (THREAT-007): Look for comments with bodies
  starting with `COGWORKS_LOCK:` and parse the embedded timestamp. See
  `docs/spec/security.md §THREAT-007` and the `LockComment` format below.

---

### IssueTracker

```rust
#[async_trait]
pub trait IssueTracker: Send + Sync {
    async fn get_issue(&self, id: WorkItemId) -> Result<Issue, GitHubOperationError>;
    async fn list_sub_issues(&self, parent: WorkItemId) -> Result<Vec<SubIssue>, GitHubOperationError>;
    async fn create_sub_issue(&self, parent: WorkItemId, title: &str, body: &str) -> Result<SubIssue, GitHubOperationError>;
    async fn add_typed_link(&self, source: WorkItemId, target: WorkItemId, kind: TypedLinkKind) -> Result<TypedLink, GitHubOperationError>;
    async fn get_typed_links(&self, id: WorkItemId) -> Result<Vec<TypedLink>, GitHubOperationError>;
    async fn get_labels(&self, id: WorkItemId) -> Result<Vec<Label>, GitHubOperationError>;
    async fn add_label(&self, id: WorkItemId, label: &Label) -> Result<(), GitHubOperationError>;
    async fn remove_label(&self, id: WorkItemId, label: &Label) -> Result<(), GitHubOperationError>;
    async fn post_comment(&self, id: WorkItemId, body: &str) -> Result<(), GitHubOperationError>;
    async fn list_comments(&self, id: WorkItemId) -> Result<Vec<IssueComment>, GitHubOperationError>;
    async fn get_issue_state(&self, id: WorkItemId) -> Result<IssueState, GitHubOperationError>;
    async fn get_milestone(&self, id: MilestoneId) -> Result<Milestone, GitHubOperationError>;
    async fn set_milestone(&self, id: WorkItemId, milestone: Option<MilestoneId>) -> Result<(), GitHubOperationError>;
}
```

**Idempotency**: `add_label` and `remove_label` are idempotent (no-op if already in target state).

`list_comments` returns comments in ascending `created_at` order (oldest first). The
implementation calls `sdk.issues().list_comments(owner, repo, number)`.

`get_milestone` looks up a **Milestone by its own numeric ID** (not by the issue it
is attached to). It maps to the GitHub REST endpoint
`GET /repos/{owner}/{repo}/milestones/{milestone_number}`, implemented via
`sdk.milestones().get(owner, repo, milestone_number)` — **not** via
`sdk.issues().get_milestone(…)`. The intent is to resolve a `MilestoneId` to a
human-readable `Milestone` struct (title, due date) when the Planning node needs to
display or record milestone information. To find the milestone attached to a specific
issue, use `get_issue(id).milestone` instead.

#### SDK Gap Table

| Method | Required SDK capability |
|--------|------------------------|
| `list_sub_issues` | Sub-issues REST endpoint |
| `create_sub_issue` | Sub-issues REST endpoint |
| `add_typed_link` | GraphQL `issueLink` mutation |
| `get_typed_links` | GraphQL `issueLink` query |
| `set_milestone` | PATCH issue milestone field |
| `list_comments` | `issues().list_comments(owner, repo, number)` |

Until these capabilities land in `github-bot-sdk`, implementations return
`GitHubOperationError::SdkCapabilityMissing`.

---

### ReviewDecision, ReviewStatus, PullRequest, PullRequestFilter

```rust
pub enum ReviewDecision { Approved, ChangesRequested, Commented, Dismissed }

/// `approved` reflects GitHub's platform-level merge-readiness check
/// (branch protection rules satisfied), NOT a recomputation of `approvals > 0`.
/// Invariant: `approved == true` implies `changes_requested == false`.
/// The converse does not hold (e.g. required reviewer has not yet reviewed).
pub struct ReviewStatus {
    pub approvals: u32,
    pub changes_requested: bool,
    pub approved: bool,
}

pub struct PullRequest {
    pub id: PullRequestId,
    pub repository: RepositoryId,
    pub title: String,
    pub body: String,
    pub head_branch: BranchName,
    pub base_branch: BranchName,
    pub head_sha: CommitSha,
    pub is_open: bool,
    pub is_merged: bool,
    pub review_status: ReviewStatus,
    pub created_at: DateTime<Utc>,
}

pub enum PullRequestStateFilter { Open, Closed, All }

pub struct PullRequestFilter {
    pub base_branch: Option<BranchName>,
    pub head_branch: Option<BranchName>,
    /// `None` is equivalent to `PullRequestStateFilter::All`.
    pub state: Option<PullRequestStateFilter>,
}
```

---

### PullRequestManager

```rust
#[async_trait]
pub trait PullRequestManager: Send + Sync {
    async fn create_pull_request(&self, repository: &RepositoryId, title: &str, body: &str, head: &BranchName, base: &BranchName) -> Result<PullRequest, GitHubOperationError>;
    async fn get_pull_request(&self, repository: &RepositoryId, id: PullRequestId) -> Result<PullRequest, GitHubOperationError>;
    async fn find_pull_requests(&self, repository: &RepositoryId, filter: &PullRequestFilter) -> Result<Vec<PullRequest>, GitHubOperationError>;
    async fn post_review_comment(&self, repository: &RepositoryId, id: PullRequestId, commit_sha: &CommitSha, path: &str, line: u32, body: &str) -> Result<(), GitHubOperationError>;
    async fn get_review_status(&self, repository: &RepositoryId, id: PullRequestId) -> Result<ReviewStatus, GitHubOperationError>;
}
```

---

### FileContent, DirectoryEntryKind, DirectoryEntry

```rust
pub struct FileContent {
    pub path: String,
    pub content: Vec<u8>,
    /// Git blob SHA of the file object. Not a commit SHA.
    pub sha: GitObjectSha,
    pub content_type: Option<String>,
}
impl FileContent {
    pub fn as_text(&self) -> Option<&str> { ... }
}

pub enum DirectoryEntryKind { File, Directory, Symlink, Submodule }

pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    pub kind: DirectoryEntryKind,
    /// Git object SHA (blob for files, tree for directories). Not a commit SHA.
    pub sha: GitObjectSha,
}
```

---

### CodeRepository

```rust
#[async_trait]
pub trait CodeRepository: Send + Sync {
    async fn read_file(&self, repository: &RepositoryId, path: &str, git_ref: &str) -> Result<FileContent, GitHubOperationError>;
    async fn list_directory(&self, repository: &RepositoryId, path: &str, git_ref: &str) -> Result<Vec<DirectoryEntry>, GitHubOperationError>;
    async fn file_exists(&self, repository: &RepositoryId, path: &str, git_ref: &str) -> Result<bool, GitHubOperationError>;
    async fn read_tree(&self, repository: &RepositoryId, git_ref: &str) -> Result<Vec<DirectoryEntry>, GitHubOperationError>;
}
```

**Read-only**: no write methods. All reads are at a specific `git_ref`
(commit SHA or branch name). Writing to the repository is done via git
CLI operations in the `nodes` crate.

---

### ProjectBoard

```rust
#[async_trait]
pub trait ProjectBoard: Send + Sync {
    async fn sync_item_status(&self, work_item_id: WorkItemId, status: &str) -> Result<(), GitHubOperationError>;
    async fn sync_custom_field(&self, work_item_id: WorkItemId, field_name: &str, value: &serde_json::Value) -> Result<(), GitHubOperationError>;
}
```

**Non-blocking**: failures must be logged at `WARN` but must not halt the pipeline.

---

## Part 2 — `pipeline/src/templates.rs`

### TemplateError

```rust
pub enum TemplateError {
    NotFound { name: String },
    MissingVariables { missing: Vec<String> },
    SyntaxError { name: String, message: String },
    ConstraintViolation { name: String, message: String },
}
```

---

### TemplateEngine

```rust
// Synchronous — templates are pre-loaded in memory at startup; no I/O needed.
pub trait TemplateEngine: Send + Sync {
    fn render(&self, name: &str, context: HashMap<String, String>) -> Result<String, TemplateError>;
    fn list_required_variables(&self, name: &str) -> Result<Vec<String>, TemplateError>;
}
```

**Variable naming convention**: snake_case keys (e.g. `work_item_id`, `pr_url`).
All values are `String`; the template engine performs further formatting.

Templates are pre-loaded at startup from `.cogworks/templates/`. The infrastructure
implementation validates all templates at startup and rejects startup if any
template has a syntax error.

---

## Part 3 — `pipeline/src/audit.rs`

### AuditEvent

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuditEvent {
    LlmCall(LlmCallRecord),
    Validation(ValidationRecord),
    StateTransition(StateTransitionRecord),
    CostSnapshot(CostSnapshot),
    EdgeEvaluation(EdgeEvaluationRecord),   // from pipeline-graph.md
    InjectionDetected(InjectionDetectionRecord),
    ScopeViolation(ScopeViolationRecord),
}
```

All variants are `Serialize + Deserialize`. The `kind` tag enables typed
deserialization from the persisted JSON.

#### Variant Payloads

| Variant | Key fields |
|---------|-----------|
| `LlmCall` | `node_id`, `model_id`, `prompt_tokens`, `completion_tokens`, `cost`, `latency`, `schema_validated` |
| `Validation` | `node_id`, `validation_kind`, `passed`, `diagnostics: Vec<String>` |
| `StateTransition` | `node_id`, `from_status`, `to_status`, `reason` |
| `CostSnapshot` | `node_id`, `accumulated`, `budget`, `budget_exceeded` |
| `EdgeEvaluation` | see `pipeline-graph.md` §EdgeEvaluationRecord |
| `InjectionDetected` | `node_id`, `source_label`, `offending_text`, `pattern` |
| `ScopeViolation` | `node_id`, `artifact_path`, `description`, `violation_kind` |

**Note on forward references**: `InjectionDetected.pattern` and
`ScopeViolation.violation_kind` are `String` until PR 5 (`security.rs`)
defines the `InjectionPattern` and `ScopeViolationKind` enums. PR 5 will
update these fields to use the typed enums.

---

### PipelineOutcome, PipelineSummary

```rust
pub enum PipelineOutcome { Completed, Failed, HumanGated, Escalated }

pub struct PipelineSummary {
    pub run_id: PipelineRunId,
    pub work_item_id: WorkItemId,
    pub outcome: PipelineOutcome,
    pub total_cost: TokenCost,
    pub duration: Duration,
    pub nodes_completed: u32,
    pub nodes_failed: u32,
    pub total_rework_count: u32,
    pub terminal_message: String,
    pub completed_at: DateTime<Utc>,
}
```

---

### AuditStoreError

```rust
pub enum AuditStoreError {
    Unavailable { message: String },
    SerialisationError { message: String },
}
```

**Non-fatal**: callers must log at `WARN` and continue. Audit failures must
never halt the pipeline.

---

### AuditStore

```rust
#[async_trait]
pub trait AuditStore: Send + Sync {
    async fn record_event(
        &self,
        run_id: PipelineRunId,
        work_item_id: WorkItemId,
        event: AuditEvent,
    ) -> Result<(), AuditStoreError>;

    async fn write_summary(&self, summary: &PipelineSummary) -> Result<(), AuditStoreError>;
}
```

**Durability contract**: `record_event` implementations **may buffer events**
and are **not** required to provide per-call durability guarantees. Events may
be lost if the process crashes between a `record_event` call and the next
scheduled flush. Callers that require at-least-once delivery of an event must
call a flush checkpoint (e.g., before writing `PipelineStateComment`). The
`write_summary` call at the end of a pipeline run acts as an implicit flush
for the `GithubClient` implementation.

Trait documentation must make this buffering behaviour visible rather than
implying immediate durability with simple write semantics.

**GitHub implementation format** (PR 10):

- `record_event`: Events are batched internally and flushed either every 30
  seconds or when the batch reaches 10 events, whichever comes first. Each
  flush posts a `<details>` collapsible Markdown block as a GitHub issue
  comment. This batching avoids rate-limit exhaustion.
- `write_summary`: A Markdown table summarising the run, posted as the final
  comment on the work-item issue. Triggers an immediate flush of any pending
  batched events before posting the summary.

---

## Part 4 — SDK Gap Table

Methods requiring additions to `pvandervelde/github-bot-sdk` before they can
be implemented in PR 10. Until those additions land, the `github` crate returns
`Err(GitHubOperationError::SdkCapabilityMissing { capability: "..." })`.

| Method | SDK addition | GitHub API |
|--------|-------------|------------|
| `IssueTracker::list_sub_issues` | Sub-issues REST endpoint | `GET /repos/{owner}/{repo}/issues/{issue_number}/sub_issues` |
| `IssueTracker::create_sub_issue` | Sub-issues REST endpoint | `POST /repos/{owner}/{repo}/issues/{issue_number}/sub_issues` |
| `IssueTracker::add_typed_link` | GraphQL `issueLink` mutation | `mutation { createIssueLink(...) }` |
| `IssueTracker::get_typed_links` | GraphQL `issueLink` query | `query { issue { issueLinks { ... } } }` |
| `IssueTracker::set_milestone` | PATCH issue milestone | `PATCH /repos/{owner}/{repo}/issues/{issue_number}` |
| `PullRequestManager::find_pull_requests` | List PRs with filter | `GET /repos/{owner}/{repo}/pulls?head=...&base=...` |
| `PullRequestManager::post_review_comment` | Create PR review comment | `POST /repos/{owner}/{repo}/pulls/{pull_number}/reviews` |
| `CodeRepository::read_file` | GitHub Contents API | `GET /repos/{owner}/{repo}/contents/{path}?ref={ref}` |
| `CodeRepository::list_directory` | GitHub Contents API | `GET /repos/{owner}/{repo}/contents/{path}?ref={ref}` |
| `CodeRepository::file_exists` | GitHub Contents API | `HEAD /repos/{owner}/{repo}/contents/{path}?ref={ref}` |
| `CodeRepository::read_tree` | GitHub Trees API (recursive) | `GET /repos/{owner}/{repo}/git/trees/{sha}?recursive=1` |

**Already covered by existing SDK**: issue CRUD, labels, comments, PR CRUD
(non-filter), Projects V2, branch ops, rate limiting, auth, pagination,
webhook HMAC verification.

---

## Part 5 — Infrastructure Structs

### GithubClient (`github` crate)

```rust
pub struct GithubClient { /* wraps github-bot-sdk client — filled in PR 10 */ }
impl GithubClient {
    pub fn new(sdk_client: Arc<dyn Any + Send + Sync>) -> Self;
}
impl IssueTracker for GithubClient { ... }
impl PullRequestManager for GithubClient { ... }
impl CodeRepository for GithubClient { ... }
impl ProjectBoard for GithubClient { ... }
impl AuditStore for GithubClient { ... }
```

Constructed once in `cli` and shared as `Arc<GithubClient>` across all nodes.
Rate limiting is delegated to the SDK's built-in handling.

---

### GitHubWebhookEventSource (`listener` crate)

```rust
pub struct GitHubWebhookEventSource { config: WebhookConfig, /* server handle — PR 10 */ }
impl GitHubWebhookEventSource {
    pub fn new(config: WebhookConfig) -> Self;
}
impl EventSource for GitHubWebhookEventSource { ... }
```

Binds an HTTP server on `config.bind_address`. Validates every POST with
HMAC-SHA256 using `github-bot-sdk`'s webhook responder. Forwards parsed
events via an internal channel to `next_event`.

**Development proxy**: Use smee.io — run `smee --url <channel> --port <port>`
and set `bind_address` to the local port.

---

### QueueEventSource (`listener` crate)

```rust
pub struct QueueEventSource { config: QueueEventConfig, /* queue client — PR 10 */ }
impl QueueEventSource {
    pub fn new(config: QueueEventConfig) -> Self;
}
impl EventSource for QueueEventSource { ... }
```

Consumers from Azure Service Bus (default) or AWS SQS (planned in
`queue-runtime`). Each message body is a JSON-encoded GitHub webhook payload.
Session ordering keyed on `WorkItemId` when `use_session_ordering = true`.

---

## Implementation Notes

1. **`async_trait`**: All traits use `#[async_trait]` from the `async_trait`
   crate (`v0.1`), which is declared in workspace dependencies. This is required
   for `dyn Trait` dispatch with async methods (native async-in-traits do not
   support `dyn` in Rust 1.82).

2. **`pipeline` has no I/O**: `pipeline/src/github.rs` uses only `std::net::SocketAddr`,
   `std::time::Duration`, `serde`, `serde_json`, `chrono`, and `async_trait`.
   No `tokio`, `reqwest`, or network crates.

3. **SDK gap methods return `Err`, not `todo!()`**: Methods blocked on SDK
   additions return `Err(GitHubOperationError::SdkCapabilityMissing { ... })`
   so callers receive a typed error rather than a panic.

4. **PR 6 label refinement**: `GitHubEvent::LabelApplied.label` is a raw
   `String` now. PR 6 introduces `PipelineLabel` enum; the `cli` dispatcher
   will be updated to parse and filter labels at that point.

5. **PR 5 security type refinement**: `InjectionDetectionRecord.pattern` and
   `ScopeViolationRecord.violation_kind` are `String` until PR 5 defines
   `InjectionPattern` and `ScopeViolationKind` enums.
