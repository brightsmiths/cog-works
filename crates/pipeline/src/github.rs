//! GitHub-facing trait definitions for the CogWorks pipeline domain.
//!
//! This module defines:
//!
//! - [`EventSource`]: abstraction over *how* a pipeline step is triggered. Two
//!   infrastructure implementations exist in the `listener` crate
//!   ([`GitHubWebhookEventSource`][listener] and [`QueueEventSource`][listener]);
//!   the CLI also synthesises a one-shot event for manual runs.
//! - [`IssueTracker`]: GitHub Issues API — reading issues, managing sub-issues,
//!   typed links, labels, comments, milestones.
//! - [`PullRequestManager`]: GitHub Pull Request API — creating PRs, fetching
//!   reviews, posting review comments.
//! - [`CodeRepository`]: read-only access to repository file contents and tree.
//! - [`ProjectBoard`]: optional, non-blocking GitHub Projects V2 synchronisation.
//!
//! Supporting data types for all five traits are also declared here.
//!
//! ## Architectural Layer
//!
//! This module expresses what the pipeline domain *needs* from GitHub in terms
//! that the domain understands. The `github` and `listener` infrastructure crates
//! implement these traits; the `pipeline` crate never sees API details.
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/github-traits.md` for the full contract, error
//! conditions, and the SDK gap table.
//!
//! [listener]: ../../listener/index.html

use std::net::SocketAddr;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use thiserror::Error;

use crate::{
    BranchName, CommitSha, GitObjectSha, MilestoneId, PullRequestId, RepositoryId, SubWorkItemId,
    WorkItemId,
};

// ─── Event trigger abstraction ─────────────────────────────────────────────

/// A GitHub event that may trigger or advance a pipeline step.
///
/// The `cli` crate's event loop calls [`EventSource::next_event`] in a loop.
/// Each variant carries just enough information for the pipeline state machine
/// to determine whether to act and, if so, how.
///
/// ## Specification
///
/// See `docs/spec/interfaces/github-traits.md` §GitHubEvent for the full
/// variant descriptions and delivery-order guarantees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitHubEvent {
    /// A label was applied to a work-item issue.
    ///
    /// The primary pipeline trigger: `cogworks:run` causes the Intake node to start.
    /// Other label variants (e.g. `cogworks:restart`) also arrive via this event.
    LabelApplied {
        /// The issue to which the label was applied.
        work_item_id: WorkItemId,
        /// The exact label string that was applied (e.g. `"cogworks:run"`).
        label: String,
    },

    /// A comment was posted on a work-item issue.
    ///
    /// Used at human-gated nodes: the pipeline resumes when an authorised reviewer
    /// posts an approval comment (e.g. `"/cogworks approve"`).
    CommentPosted {
        /// The issue on which the comment appeared.
        work_item_id: WorkItemId,
        /// GitHub login of the comment author.
        author: String,
        /// Full text of the comment body.
        body: String,
    },

    /// The state of a sub-issue changed (open → closed or closed → reopened).
    ///
    /// Used by the Planning node's fan-in gate to detect when all sub-work-items
    /// from a spawning node have completed.
    SubIssueStateChanged {
        /// The sub-issue whose state changed.
        sub_work_item_id: SubWorkItemId,
        /// New state of the sub-issue.
        new_state: IssueState,
    },

    /// A pull-request review was submitted.
    ///
    /// Used by the Review gate to detect APPROVED / CHANGES_REQUESTED decisions
    /// from human reviewers at the final integration step.
    PullRequestReviewed {
        /// The pull request that received the review.
        pr_id: PullRequestId,
        /// The reviewer's decision.
        decision: ReviewDecision,
    },
}

/// Errors that can be returned by an [`EventSource`] implementation.
///
/// All variants except [`EventSourceError::Timeout`] indicate that the source
/// is in a degraded or unrecoverable state and should be reported to the
/// operator.
///
/// ## Specification
///
/// See `docs/spec/interfaces/github-traits.md` §EventSourceError.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EventSourceError {
    /// The poll window elapsed with no event arriving. This is *not* an error
    /// condition: callers should loop and call `next_event` again.
    #[error("event source poll timed out")]
    Timeout,

    /// The underlying connection to the event source was lost.
    ///
    /// Infrastructure implementations should attempt reconnection internally
    /// before returning this error.
    #[error("event source connection lost: {message}")]
    ConnectionLost {
        /// Human-readable description of the connectivity failure.
        message: String,
    },

    /// An incoming event payload could not be parsed into a [`GitHubEvent`].
    ///
    /// The raw payload is preserved so it can be written to the audit log.
    #[error("failed to parse incoming event payload")]
    ParseError {
        /// The raw bytes / string that could not be deserialised.
        raw: String,
    },

    /// The event source rejected the request due to an authentication failure.
    ///
    /// Typically means the HMAC secret or queue credential is wrong.
    /// Non-retryable without operator intervention.
    #[error("event source authentication failure")]
    AuthError,

    /// A cloud queue operation failed.
    #[error("event source queue error from {provider}")]
    QueueError {
        /// Identifier of the queue provider (e.g. `"azure_service_bus"`).
        provider: String,
    },
}

/// Configuration for a GitHub-webhook-based [`EventSource`] implementation.
///
/// Passed to `GitHubWebhookEventSource::new` in the `listener` crate.
///
/// ## Specification
///
/// See `docs/spec/interfaces/github-traits.md` §WebhookConfig.
#[derive(Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// The local address on which the HTTP server should bind
    /// (e.g. `"0.0.0.0:3000"`).
    pub bind_address: SocketAddr,

    /// URL path prefix for the webhook endpoint (e.g. `"/hooks"`).
    /// The full endpoint is `{bind_address}{path_prefix}/github`.
    pub path_prefix: String,

    /// HMAC-SHA256 secret used to verify the `X-Hub-Signature-256` header on
    /// every incoming webhook. Must match the secret configured in the GitHub
    /// webhook settings.
    ///
    /// ## Security
    ///
    /// This field is intentionally excluded from the `Debug` impl to prevent
    /// accidental exposure in logs or tracing spans.
    pub secret: String,
}

impl std::fmt::Debug for WebhookConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebhookConfig")
            .field("bind_address", &self.bind_address)
            .field("path_prefix", &self.path_prefix)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}

/// Configuration for a cloud-queue-based [`EventSource`] implementation.
///
/// Passed to `QueueEventSource::new` in the `listener` crate.
///
/// `provider_config` is an opaque JSON value that the `listener` crate
/// deserialises into `queue_runtime::ProviderConfig`. Keeping it as
/// [`JsonValue`] avoids a `queue-runtime` dependency in the `pipeline` crate.
///
/// ## Specification
///
/// See `docs/spec/interfaces/github-traits.md` §QueueEventConfig.
#[derive(Clone, Serialize, Deserialize)]
pub struct QueueEventConfig {
    /// Serialised `queue_runtime::ProviderConfig` (Azure Service Bus or AWS SQS).
    /// Format is provider-specific; see `queue-runtime` documentation.
    ///
    /// **Security**: This field can contain SAS tokens, connection strings, or
    /// other credentials. It is redacted in the `Debug` impl — never print
    /// `provider_config` directly.
    pub provider_config: JsonValue,

    /// The name of the queue (or topic subscription) to consume from.
    pub queue_name: String,

    /// Whether to use session-based ordering with [`WorkItemId`] as the session key.
    ///
    /// When `true`, all events for a single work item are processed in sequence
    /// even under concurrent load. Requires session support from the queue
    /// provider (Azure Service Bus: sessions; AWS SQS: FIFO + message groups).
    pub use_session_ordering: bool,

    /// Maximum number of delivery attempts before a message is dead-lettered.
    pub max_retry_attempts: u32,
}

/// Redacts `provider_config` to prevent credential leakage in tracing output.
///
/// `provider_config` is an opaque JSON value that will contain Azure Service Bus
/// connection strings or SAS tokens in production. It must never appear in logs.
impl std::fmt::Debug for QueueEventConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueueEventConfig")
            .field("provider_config", &"[REDACTED]")
            .field("queue_name", &self.queue_name)
            .field("use_session_ordering", &self.use_session_ordering)
            .field("max_retry_attempts", &self.max_retry_attempts)
            .finish()
    }
}

/// The single interface all pipeline trigger sources satisfy.
///
/// The `cli` event loop calls [`EventSource::next_event`] in a tight loop.
/// - A return of `Ok(Some(event))` causes the loop to dispatch the event.
/// - A return of `Ok(None)` is equivalent to a timeout: the loop ticks again.
/// - A return of `Err(EventSourceError::Timeout)` is treated identically to
///   `Ok(None)` for caller convenience; implementations may return either.
/// - Any other error is logged and surfaced to the operator.
///
/// ## Implementations
///
/// | Struct | Crate | Trigger mode |
/// |--------|-------|--------------|
/// | `GitHubWebhookEventSource` | `listener` | Webhook (direct or smee.io) |
/// | `QueueEventSource` | `listener` | Azure Service Bus / AWS SQS |
/// | synthesised one-shot | `cli` | Manual CLI invocation |
///
/// ## Specification
///
/// See `docs/spec/interfaces/github-traits.md` §EventSource.
#[async_trait]
pub trait EventSource: Send {
    /// Wait for the next event, blocking for at most `timeout`.
    ///
    /// # Returns
    ///
    /// - `Ok(Some(event))` — an event is available.
    /// - `Ok(None)` — no event arrived within `timeout`; the caller should loop.
    /// - `Err(EventSourceError::Timeout)` — equivalent to `Ok(None)`; callers
    ///   treat both the same way.
    /// - `Err(e)` — the source is in an error state; see [`EventSourceError`] for
    ///   recovery semantics.
    async fn next_event(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<GitHubEvent>, EventSourceError>;
}

// ─── GitHub Issues data types ───────────────────────────────────────────────

/// The open/closed lifecycle state of a GitHub Issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IssueState {
    /// The issue is open and active.
    Open,
    /// The issue was closed (completed or won't-fix).
    Closed,
}

/// A GitHub label as seen by the pipeline domain.
///
/// The pipeline only reads and applies labels; it never creates or deletes
/// label definitions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Label {
    /// The exact label name string (e.g. `"cogworks:run"`).
    pub name: String,
    /// Optional CSS-hex colour code (e.g. `"0075ca"`), without the `#` prefix.
    pub color: Option<String>,
}

/// A GitHub Milestone associated with a work item.
///
/// CogWorks reads milestone information but never creates or modifies milestones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Milestone {
    /// The numeric GitHub milestone ID.
    pub id: MilestoneId,
    /// The milestone title string.
    pub title: String,
    /// Optional due-on date (UTC).
    pub due_on: Option<DateTime<Utc>>,
}

/// The kind of typed link between two work items.
///
/// Mapped to/from the GitHub GraphQL `issueLink` type. CogWorks uses
/// `Blocks`/`IsBlockedBy` to model sub-task dependencies created by the
/// Planning node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypedLinkKind {
    /// The source issue blocks progress on the target issue.
    Blocks,
    /// The source issue is blocked by the target issue.
    IsBlockedBy,
}

/// A typed link between two GitHub issues.
///
/// Created by the Planning node when it establishes dependency relationships
/// between sub-work-items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedLink {
    /// The issue that owns this link (the "from" side).
    pub source_id: WorkItemId,
    /// The issue the link points to (the "to" side).
    pub target_id: WorkItemId,
    /// The semantic relationship.
    pub kind: TypedLinkKind,
}

/// A GitHub Issue as returned by the [`IssueTracker`] trait.
///
/// Contains the fields the pipeline domain needs; not an exhaustive mirror of
/// the GitHub API response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    /// GitHub issue number (the numeric ID).
    pub id: WorkItemId,
    /// Repository that contains this issue.
    pub repository: RepositoryId,
    /// Issue title.
    pub title: String,
    /// Issue body (Markdown).
    pub body: String,
    /// Current lifecycle state.
    pub state: IssueState,
    /// Labels currently applied to the issue.
    pub labels: Vec<Label>,
    /// The milestone this issue is assigned to, if any.
    pub milestone: Option<Milestone>,
    /// When the issue was created (UTC).
    pub created_at: DateTime<Utc>,
    /// When the issue was last updated (UTC).
    pub updated_at: DateTime<Utc>,
}

/// A GitHub Issue that was created as a sub-task of a parent work item.
///
/// Sub-issues are created by the Planning node; their state is monitored by the
/// Spawning node's fan-in gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubIssue {
    /// The sub-issue's own numeric ID.
    pub id: SubWorkItemId,
    /// The parent work item this sub-issue belongs to.
    pub parent_id: WorkItemId,
    /// Title of the sub-issue.
    pub title: String,
    /// Current lifecycle state.
    pub state: IssueState,
    /// When the sub-issue was created (UTC).
    pub created_at: DateTime<Utc>,
}

/// Errors returned by [`IssueTracker`] operations.
///
/// ## Specification
///
/// See `docs/spec/interfaces/github-traits.md` §GitHubOperationError.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GitHubOperationError {
    /// The requested resource was not found.
    #[error("GitHub resource not found: {resource}")]
    NotFound {
        /// Human-readable description of the resource that was not found.
        resource: String,
    },

    /// The request was denied due to insufficient permissions.
    #[error("GitHub permission denied: {action}")]
    PermissionDenied {
        /// Description of the action that was denied.
        action: String,
    },

    /// The GitHub API rate limit was exhausted.
    ///
    /// The `reset_at` field gives the UTC time when the limit resets; callers
    /// should not retry before then.
    #[error("GitHub API rate limit exhausted; resets at {reset_at}")]
    RateLimitExhausted {
        /// When the rate limit window resets (UTC).
        reset_at: DateTime<Utc>,
    },

    /// A transient network or server error occurred.
    ///
    /// May be retried after a back-off delay.
    #[error("GitHub API transient error: {message}")]
    Transient {
        /// Human-readable description of the failure.
        message: String,
    },

    /// The GitHub API returned a response that could not be parsed.
    #[error("GitHub API response parse failure: {message}")]
    ParseFailure {
        /// Human-readable description of the parse failure.
        message: String,
    },

    /// An operation that requires a pending SDK addition was called.
    ///
    /// Produced by `todo!()` stubs until `github-bot-sdk` gains the required
    /// capability. See `docs/spec/interfaces/github-traits.md` §SDK Gap Table.
    #[error("GitHub SDK capability not yet available: {capability}")]
    SdkCapabilityMissing {
        /// Name of the missing SDK capability.
        capability: String,
    },
}

/// A single comment on a GitHub Issue.
///
/// Returned by [`IssueTracker::list_comments`]. Used for pipeline state
/// reconstruction (reading the latest [`crate::PipelineStateComment`] JSON)
/// and for locating processing-lock timestamp records (THREAT-007 mitigation).
///
/// See `docs/spec/interfaces/github-traits.md` §IssueComment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueComment {
    /// Numeric GitHub comment ID.
    pub id: u64,
    /// Login name of the comment author.
    pub author: String,
    /// Numeric GitHub user ID of the comment author — unforgeable, unlike
    /// `author` (login string). Used by
    /// `nodes::executor::steps::reconstruct_state` to verify comment
    /// authenticity (THREAT-022).
    pub author_id: u64,
    /// Comment body (Markdown).
    pub body: String,
    /// When the comment was created (UTC).
    pub created_at: DateTime<Utc>,
}

/// GitHub Issues API — the operations the pipeline domain needs to read and
/// update work items, sub-issues, labels, comments, and milestones.
///
/// All methods are `async` and return `Result<_, GitHubOperationError>`.
/// Implementations must not add domain rules; they translate between the
/// GitHub API and domain types.
///
/// ## SDK Gap Table
///
/// The following methods require additions to `github-bot-sdk` that are not
/// yet merged. Until those additions land, the `github` crate returns
/// `GitHubOperationError::SdkCapabilityMissing`. See PR 3 of the interface
/// design plan for the full gap table.
///
/// | Method | Required SDK capability |
/// |--------|------------------------|
/// | `list_sub_issues` | Sub-issues REST endpoint |
/// | `create_sub_issue` | Sub-issues REST endpoint |
/// | `add_typed_link` | GraphQL `issueLink` mutation |
/// | `get_typed_links` | GraphQL `issueLink` query |
/// | `set_milestone` | PATCH issue milestone field |
/// | `list_comments` | `issues().list_comments(owner, repo, number)` |
///
/// ## Specification
///
/// See `docs/spec/interfaces/github-traits.md` §IssueTracker.
#[async_trait]
pub trait IssueTracker: Send + Sync {
    /// Fetch the full details of a work-item issue by its numeric ID.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — issue does not exist.
    /// - [`GitHubOperationError::RateLimitExhausted`] — retry after `reset_at`.
    /// - [`GitHubOperationError::Transient`] — transient network failure.
    async fn get_issue(&self, id: WorkItemId) -> Result<Issue, GitHubOperationError>;

    /// List all sub-issues created under a parent work item.
    ///
    /// Returns an empty `Vec` if the parent has no sub-issues.
    ///
    /// **SDK gap**: requires sub-issues endpoint addition to `github-bot-sdk`.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — parent issue does not exist.
    /// - [`GitHubOperationError::SdkCapabilityMissing`] — SDK addition pending.
    async fn list_sub_issues(
        &self,
        parent: WorkItemId,
    ) -> Result<Vec<SubIssue>, GitHubOperationError>;

    /// Create a new sub-issue under a parent work-item issue.
    ///
    /// The returned [`SubIssue`] reflects the state immediately after creation.
    ///
    /// **SDK gap**: requires sub-issues endpoint addition to `github-bot-sdk`.
    ///
    /// # Arguments
    ///
    /// * `parent` — the parent work-item issue.
    /// * `title` — the sub-issue title (non-empty).
    /// * `body` — the sub-issue body in Markdown (may be empty).
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — parent issue does not exist.
    /// - [`GitHubOperationError::PermissionDenied`] — insufficient write access.
    /// - [`GitHubOperationError::SdkCapabilityMissing`] — SDK addition pending.
    async fn create_sub_issue(
        &self,
        parent: WorkItemId,
        title: &str,
        body: &str,
    ) -> Result<SubIssue, GitHubOperationError>;

    /// Add a typed link between two issues.
    ///
    /// **SDK gap**: requires GraphQL `issueLink` mutation in `github-bot-sdk`.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — source or target issue not found.
    /// - [`GitHubOperationError::SdkCapabilityMissing`] — SDK addition pending.
    async fn add_typed_link(
        &self,
        source: WorkItemId,
        target: WorkItemId,
        kind: TypedLinkKind,
    ) -> Result<TypedLink, GitHubOperationError>;

    /// Return all typed links attached to an issue (both directions).
    ///
    /// **SDK gap**: requires GraphQL `issueLink` query in `github-bot-sdk`.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — issue does not exist.
    /// - [`GitHubOperationError::SdkCapabilityMissing`] — SDK addition pending.
    async fn get_typed_links(&self, id: WorkItemId)
    -> Result<Vec<TypedLink>, GitHubOperationError>;

    /// Return the current set of labels applied to an issue.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — issue does not exist.
    async fn get_labels(&self, id: WorkItemId) -> Result<Vec<Label>, GitHubOperationError>;

    /// Apply a label to an issue.
    ///
    /// Idempotent: if the label is already present, this is a no-op.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — issue does not exist.
    /// - [`GitHubOperationError::PermissionDenied`] — insufficient write access.
    async fn add_label(&self, id: WorkItemId, label: &Label) -> Result<(), GitHubOperationError>;

    /// Remove a label from an issue.
    ///
    /// Idempotent: if the label is not present, this is a no-op.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — issue does not exist.
    /// - [`GitHubOperationError::PermissionDenied`] — insufficient write access.
    async fn remove_label(&self, id: WorkItemId, label: &Label)
    -> Result<(), GitHubOperationError>;

    /// Post a comment on an issue.
    ///
    /// # Arguments
    ///
    /// * `id` — the issue to comment on.
    /// * `body` — the comment body in Markdown.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — issue does not exist.
    /// - [`GitHubOperationError::PermissionDenied`] — insufficient write access.
    async fn post_comment(&self, id: WorkItemId, body: &str) -> Result<(), GitHubOperationError>;

    /// List all comments on an issue, in ascending creation order.
    ///
    /// Used to:
    /// - Reconstruct [`crate::PipelineStateComment`] on resume (read the last
    ///   comment whose body is valid JSON with the `pipeline_run_id` field).
    /// - Locate processing-lock timestamp records for THREAT-007 stale-lock
    ///   detection (look for comments with body starting with
    ///   `COGWORKS_LOCK:`).
    ///
    /// **SDK gap**: requires `github-bot-sdk` `issues().list_comments(owner,
    /// repo, number)` support.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — issue does not exist.
    /// - [`GitHubOperationError::SdkCapabilityMissing`] — SDK addition pending.
    async fn list_comments(
        &self,
        id: WorkItemId,
    ) -> Result<Vec<IssueComment>, GitHubOperationError>;

    /// Returns the numeric GitHub user ID that CogWorks itself is authenticated as.
    ///
    /// Used to verify the authorship of comments that are trusted as pipeline
    /// state (see `nodes::executor::steps::reconstruct_state`, THREAT-022) —
    /// the numeric ID is unforgeable, unlike a login string.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::Transient`] — transient network failure.
    /// - [`GitHubOperationError::PermissionDenied`] — the configured token
    ///   cannot be resolved to an authenticated identity.
    async fn authenticated_bot_user_id(&self) -> Result<u64, GitHubOperationError>;

    /// Return the current lifecycle state of an issue without fetching all fields.
    ///
    /// Cheaper than [`IssueTracker::get_issue`] when only the open/closed state
    /// is needed.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — issue does not exist.
    async fn get_issue_state(&self, id: WorkItemId) -> Result<IssueState, GitHubOperationError>;

    /// Fetch a milestone by its numeric ID.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — milestone does not exist.
    async fn get_milestone(&self, id: MilestoneId) -> Result<Milestone, GitHubOperationError>;

    /// Assign a milestone to an issue, or clear the milestone if `None`.
    ///
    /// **SDK gap**: requires PATCH issue milestone in `github-bot-sdk`.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — issue or milestone not found.
    /// - [`GitHubOperationError::PermissionDenied`] — insufficient write access.
    /// - [`GitHubOperationError::SdkCapabilityMissing`] — SDK addition pending.
    async fn set_milestone(
        &self,
        id: WorkItemId,
        milestone: Option<MilestoneId>,
    ) -> Result<(), GitHubOperationError>;
}

// ─── Pull Request data types ────────────────────────────────────────────────

/// A reviewer's decision on a pull request review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReviewDecision {
    /// The reviewer approved the changes.
    Approved,
    /// The reviewer requested changes before merge.
    ChangesRequested,
    /// The reviewer commented without a formal approve/reject decision.
    Commented,
    /// The review was dismissed.
    Dismissed,
}

/// The overall review status of a pull request.
///
/// Aggregated over all submitted reviews: once any reviewer requests changes,
/// the status is `ChangesRequested` regardless of other approvals.
///
/// ## Invariant
///
/// `approved` reflects the **platform-level merge-readiness check** as
/// determined by GitHub's branch protection rules (required reviewers met,
/// no outstanding change requests). It is *not* simply `approvals > 0`.
/// An implementation must derive `approved` from the GitHub API's merge-ready
/// state, not by recomputing it from `approvals` and `changes_requested`.
/// Concretely: `approved == true` implies `changes_requested == false`,
/// but the converse is not guaranteed (e.g. a required reviewer has not yet
/// reviewed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewStatus {
    /// Number of approvals received.
    pub approvals: u32,
    /// Whether any reviewer has requested changes (blocks merge).
    pub changes_requested: bool,
    /// Whether GitHub considers the PR ready to merge (branch-protection rules
    /// satisfied). See the invariant on [`ReviewStatus`] for details.
    pub approved: bool,
}

/// A GitHub Pull Request as seen by the pipeline domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    /// The numeric pull-request number.
    pub id: PullRequestId,
    /// Repository containing the pull request.
    pub repository: RepositoryId,
    /// PR title.
    pub title: String,
    /// PR body in Markdown.
    pub body: String,
    /// The branch being merged.
    pub head_branch: BranchName,
    /// The target branch (base).
    pub base_branch: BranchName,
    /// Commit SHA at the tip of `head_branch` at time of last fetch.
    pub head_sha: CommitSha,
    /// Whether the PR is currently open.
    pub is_open: bool,
    /// Whether the PR has been merged.
    pub is_merged: bool,
    /// The current review status.
    pub review_status: ReviewStatus,
    /// When the PR was created (UTC).
    pub created_at: DateTime<Utc>,
}

/// State selector for [`PullRequestFilter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PullRequestStateFilter {
    /// Include only open pull requests.
    Open,
    /// Include only closed or merged pull requests.
    Closed,
    /// Include pull requests in all states.
    All,
}

/// Parameters for [`PullRequestManager::find_pull_requests`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PullRequestFilter {
    /// Restrict to PRs targeting this base branch.
    pub base_branch: Option<BranchName>,
    /// Restrict to PRs sourced from this head branch.
    pub head_branch: Option<BranchName>,
    /// Lifecycle state filter. `None` is equivalent to [`PullRequestStateFilter::All`].
    pub state: Option<PullRequestStateFilter>,
}

/// GitHub Pull Request API — operations the pipeline domain needs for PR
/// lifecycle management and review gating.
///
/// ## SDK Gap Table
///
/// | Method | Required SDK capability |
/// |--------|------------------------|
/// | `find_pull_requests` | List PRs with filter parameters |
/// | `post_review_comment` | Create inline PR review comment |
///
/// ## Specification
///
/// See `docs/spec/interfaces/github-traits.md` §PullRequestManager.
#[async_trait]
pub trait PullRequestManager: Send + Sync {
    /// Create a new pull request.
    ///
    /// # Arguments
    ///
    /// * `repository` — the repository to create the PR in.
    /// * `title` — pull request title.
    /// * `body` — pull request body in Markdown.
    /// * `head` — the branch containing the changes.
    /// * `base` — the target branch to merge into.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::PermissionDenied`] — insufficient write access.
    /// - [`GitHubOperationError::Transient`] — transient network failure.
    async fn create_pull_request(
        &self,
        repository: &RepositoryId,
        title: &str,
        body: &str,
        head: &BranchName,
        base: &BranchName,
    ) -> Result<PullRequest, GitHubOperationError>;

    /// Fetch a pull request by its numeric ID.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — pull request does not exist.
    async fn get_pull_request(
        &self,
        repository: &RepositoryId,
        id: PullRequestId,
    ) -> Result<PullRequest, GitHubOperationError>;

    /// Find pull requests matching the given filter criteria.
    ///
    /// Returns an empty `Vec` if no PRs match.
    ///
    /// **SDK gap**: requires list-PRs-with-filter-params in `github-bot-sdk`.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::SdkCapabilityMissing`] — SDK addition pending.
    async fn find_pull_requests(
        &self,
        repository: &RepositoryId,
        filter: &PullRequestFilter,
    ) -> Result<Vec<PullRequest>, GitHubOperationError>;

    /// Post an inline review comment on a specific line of a pull request diff.
    ///
    /// **SDK gap**: requires create-PR-review-comment in `github-bot-sdk`.
    ///
    /// # Arguments
    ///
    /// * `id` — the pull request to comment on.
    /// * `commit_sha` — the commit SHA the comment is anchored to.
    /// * `path` — repository-root-relative path to the file.
    /// * `line` — the line number in the diff.
    /// * `body` — comment body in Markdown.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — PR or commit not found.
    /// - [`GitHubOperationError::SdkCapabilityMissing`] — SDK addition pending.
    async fn post_review_comment(
        &self,
        repository: &RepositoryId,
        id: PullRequestId,
        commit_sha: &CommitSha,
        path: &str,
        line: u32,
        body: &str,
    ) -> Result<(), GitHubOperationError>;

    /// Return the aggregated review status of a pull request.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — pull request does not exist.
    async fn get_review_status(
        &self,
        repository: &RepositoryId,
        id: PullRequestId,
    ) -> Result<ReviewStatus, GitHubOperationError>;
}

// ─── Code repository data types ────────────────────────────────────────────

/// The content of a single file read from a GitHub repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileContent {
    /// Repository-root-relative path to the file.
    pub path: String,
    /// Raw byte content of the file.
    pub content: Vec<u8>,
    /// Git blob SHA of this file at the time of reading.
    ///
    /// This is the SHA-1 hash of the blob Git object for this file's content.
    /// Must not be passed to APIs that expect a commit ref.
    pub sha: GitObjectSha,
    /// MIME type as reported by the GitHub API (e.g. `"text/plain"`).
    /// `None` if the API did not return a content type.
    pub content_type: Option<String>,
}

impl FileContent {
    /// Attempt to interpret the file content as UTF-8 text.
    ///
    /// Returns `None` if the bytes are not valid UTF-8.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        std::str::from_utf8(&self.content).ok()
    }
}

/// The kind of entry in a repository directory listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DirectoryEntryKind {
    /// A regular file.
    File,
    /// A directory (subdirectory).
    Directory,
    /// A symbolic link.
    Symlink,
    /// A Git submodule.
    Submodule,
}

/// A single entry returned by [`CodeRepository::list_directory`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryEntry {
    /// The name of the entry (filename or directory name, not the full path).
    pub name: String,
    /// Repository-root-relative full path to the entry.
    pub path: String,
    /// The kind of entry.
    pub kind: DirectoryEntryKind,
    /// Git object SHA as returned by the GitHub Contents API: a blob SHA for
    /// [`DirectoryEntryKind::File`] entries, a tree SHA for
    /// [`DirectoryEntryKind::Directory`] entries. Not a commit SHA;
    /// must not be used as a commit ref.
    pub sha: GitObjectSha,
}

/// Read-only access to a GitHub repository's file contents and directory tree.
///
/// All reads are against a specific commit ref. The pipeline never writes
/// directly through this trait; writes go via git operations in `nodes`.
///
/// ## SDK Gap Table
///
/// | Method | Required SDK capability |
/// |--------|------------------------|
/// | `read_file` | GitHub Contents API |
/// | `list_directory` | GitHub Contents API |
/// | `file_exists` | GitHub Contents API (HEAD check) |
/// | `read_tree` | GitHub Trees API (recursive) |
///
/// ## Specification
///
/// See `docs/spec/interfaces/github-traits.md` §CodeRepository.
#[async_trait]
pub trait CodeRepository: Send + Sync {
    /// Read the content of a single file at the given ref.
    ///
    /// # Arguments
    ///
    /// * `repository` — owner/repo identifier.
    /// * `path` — repository-root-relative path to the file.
    /// * `git_ref` — the commit SHA or branch name to read from.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — path does not exist at this ref.
    /// - [`GitHubOperationError::SdkCapabilityMissing`] — SDK addition pending.
    async fn read_file(
        &self,
        repository: &RepositoryId,
        path: &str,
        git_ref: &str,
    ) -> Result<FileContent, GitHubOperationError>;

    /// List the immediate children of a directory at the given ref.
    ///
    /// Returns an empty `Vec` if the directory exists but is empty.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — path does not exist or is not a directory.
    /// - [`GitHubOperationError::SdkCapabilityMissing`] — SDK addition pending.
    async fn list_directory(
        &self,
        repository: &RepositoryId,
        path: &str,
        git_ref: &str,
    ) -> Result<Vec<DirectoryEntry>, GitHubOperationError>;

    /// Check whether a path exists in the repository at the given ref.
    ///
    /// Returns `true` for both files and directories.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::SdkCapabilityMissing`] — SDK addition pending.
    async fn file_exists(
        &self,
        repository: &RepositoryId,
        path: &str,
        git_ref: &str,
    ) -> Result<bool, GitHubOperationError>;

    /// Read the full recursive tree of a repository at the given ref.
    ///
    /// Returns a flat list of all entries in the tree. For large repositories
    /// this may return thousands of entries; callers should filter as needed.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — ref does not exist.
    /// - [`GitHubOperationError::SdkCapabilityMissing`] — SDK addition pending.
    async fn read_tree(
        &self,
        repository: &RepositoryId,
        git_ref: &str,
    ) -> Result<Vec<DirectoryEntry>, GitHubOperationError>;
}

// ─── Project board synchronisation ─────────────────────────────────────────

/// GitHub Projects V2 — status and custom-field synchronisation.
///
/// All methods are best-effort and non-blocking with respect to pipeline
/// progress. A failure here must be logged but must not halt the pipeline.
///
/// ## Specification
///
/// See `docs/spec/interfaces/github-traits.md` §ProjectBoard.
#[async_trait]
pub trait ProjectBoard: Send + Sync {
    /// Update the status column of a work-item card on the project board.
    ///
    /// Idempotent: if the item already has the desired status, this is a no-op.
    ///
    /// # Arguments
    ///
    /// * `work_item_id` — the issue to update.
    /// * `status` — the target status column name (e.g. `"In Progress"`).
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — item not present on the board.
    /// - [`GitHubOperationError::Transient`] — transient network failure.
    async fn sync_item_status(
        &self,
        work_item_id: WorkItemId,
        status: &str,
    ) -> Result<(), GitHubOperationError>;

    /// Update a custom Projects V2 field value for a work-item card.
    ///
    /// # Arguments
    ///
    /// * `work_item_id` — the issue to update.
    /// * `field_name` — the custom field name (must match the field configured
    ///   in the project).
    /// * `value` — the new field value as a JSON scalar or object.
    ///
    /// # Errors
    ///
    /// - [`GitHubOperationError::NotFound`] — item or field not found.
    /// - [`GitHubOperationError::Transient`] — transient network failure.
    async fn sync_custom_field(
        &self,
        work_item_id: WorkItemId,
        field_name: &str,
        value: &JsonValue,
    ) -> Result<(), GitHubOperationError>;
}
