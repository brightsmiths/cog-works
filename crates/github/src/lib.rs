//! CogWorks GitHub infrastructure adapter.
//!
//! Implements the GitHub-facing traits defined in the [`pipeline`] crate
//! using [`github_bot_sdk`](https://github.com/pvandervelde/github-bot-sdk):
//!
//! | Trait | Implemented by |
//! |-------|---------------|
//! | [`pipeline::IssueTracker`] | [`GithubClient`] |
//! | [`pipeline::PullRequestManager`] | [`GithubClient`] |
//! | [`pipeline::CodeRepository`] | [`GithubClient`] |
//! | [`pipeline::ProjectBoard`] | [`GithubClient`] |
//! | [`pipeline::AuditStore`] | [`GithubClient`] |
//!
//! ## SDK Gap Tracking
//!
//! Several trait methods require GitHub API capabilities not yet in
//! `github-bot-sdk`. Until those additions land, the affected methods return
//! `Err(GitHubOperationError::SdkCapabilityMissing { ... })`. See
//! `docs/spec/interfaces/github-traits.md` §SDK Gap Table for the full list.
//!
//! | Trait method | SDK addition required |
//! |---|---|
//! | `IssueTracker::list_sub_issues` | Sub-issues REST endpoint |
//! | `IssueTracker::create_sub_issue` | Sub-issues REST endpoint |
//! | `IssueTracker::add_typed_link` | GraphQL `issueLink` mutation |
//! | `IssueTracker::get_typed_links` | GraphQL `issueLink` query |
//! | `IssueTracker::set_milestone` | PATCH issue milestone field |
//! | `IssueTracker::list_comments` | `issues().list_comments(owner, repo, number)` |
//! | `PullRequestManager::find_pull_requests` | List PRs with filter params |
//! | `PullRequestManager::post_review_comment` | Create inline PR review comment |
//! | `CodeRepository::read_file` | GitHub Contents API |
//! | `CodeRepository::list_directory` | GitHub Contents API |
//! | `CodeRepository::file_exists` | GitHub Contents API HEAD check |
//! | `CodeRepository::read_tree` | GitHub Trees API recursive |
//!
//! ## Architectural Layer
//!
//! **Infrastructure.** This crate must not contain domain rules.
//! All GitHub API details (rate limiting, pagination, authentication) are
//! handled here; the [`pipeline`] crate never sees them.
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/github-traits.md` for the full contract.
//!
//! *This crate is a skeleton. Method bodies are filled in during PR 10.*

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value as JsonValue;
use tracing::instrument;

use pipeline::{
    BranchName, CommitSha, MilestoneId, PipelineRunId, PullRequestId, RepositoryId, WorkItemId,
    audit::{AuditEvent, AuditStore, AuditStoreError, PipelineSummary},
    github::{
        CodeRepository, DirectoryEntry, FileContent, GitHubOperationError, Issue, IssueComment,
        IssueState, IssueTracker, Label, Milestone, ProjectBoard, PullRequest, PullRequestFilter,
        PullRequestManager, ReviewStatus, SubIssue, TypedLink, TypedLinkKind,
    },
};

// ─── Client struct ───────────────────────────────────────────────────────────

/// GitHub infrastructure adapter.
///
/// Wraps the `github-bot-sdk` client and installation handle. A single
/// `GithubClient` is constructed once in `cli` and held behind an `Arc` so
/// that all pipeline nodes share the same authenticated connection.
///
/// ## Construction
///
/// ```rust,ignore
/// let client = GithubClient::new(sdk_client);
/// let shared: Arc<GithubClient> = Arc::new(client);
/// // Pass `shared.clone()` to each node.
/// ```
///
/// ## Specification
///
/// See `docs/spec/interfaces/github-traits.md` §GithubClient.
pub struct GithubClient {
    // TODO: replace with the real fields when method bodies are implemented:
    //   inner: Arc<github_bot_sdk::GitHubClient>
    //   audit_sender: tokio::sync::mpsc::Sender<(PipelineRunId, WorkItemId, AuditEvent)>
    //   _flush_handle: tokio::task::JoinHandle<()>
    // `_private` keeps the skeleton compilable; see SdkClientPlaceholder below.
    _private: (),
}

/// Placeholder type for the SDK client until the real type is wired in.
///
/// Replaced with `github_bot_sdk::GitHubClient` when method bodies are
/// implemented. The `Arc<dyn Any>` keeps the skeleton compilable without
/// pulling the full SDK type into the stub constructor signature.
type SdkClientPlaceholder = Arc<dyn std::any::Any + Send + Sync>;

impl GithubClient {
    /// Construct a new [`GithubClient`].
    ///
    /// `_sdk_client` is a placeholder — see [`SdkClientPlaceholder`].
    pub fn new(_sdk_client: SdkClientPlaceholder) -> Self {
        Self { _private: () }
    }
}

// ─── IssueTracker ────────────────────────────────────────────────────────────

#[async_trait]
impl IssueTracker for GithubClient {
    #[instrument(skip(self))]
    async fn get_issue(&self, _id: WorkItemId) -> Result<Issue, GitHubOperationError> {
        todo!("IssueTracker::get_issue — implemented in PR 10")
    }

    #[instrument(skip(self))]
    async fn list_sub_issues(
        &self,
        _parent: WorkItemId,
    ) -> Result<Vec<SubIssue>, GitHubOperationError> {
        // SDK gap: sub-issues REST endpoint not yet in github-bot-sdk.
        Err(GitHubOperationError::SdkCapabilityMissing {
            capability: "sub_issues_rest_endpoint".to_string(),
        })
    }

    #[instrument(skip(self))]
    async fn create_sub_issue(
        &self,
        _parent: WorkItemId,
        _title: &str,
        _body: &str,
    ) -> Result<SubIssue, GitHubOperationError> {
        // SDK gap: sub-issues REST endpoint not yet in github-bot-sdk.
        Err(GitHubOperationError::SdkCapabilityMissing {
            capability: "sub_issues_rest_endpoint".to_string(),
        })
    }

    #[instrument(skip(self))]
    async fn add_typed_link(
        &self,
        _source: WorkItemId,
        _target: WorkItemId,
        _kind: TypedLinkKind,
    ) -> Result<TypedLink, GitHubOperationError> {
        // SDK gap: GraphQL issueLink mutation not yet in github-bot-sdk.
        Err(GitHubOperationError::SdkCapabilityMissing {
            capability: "graphql_issue_link_mutation".to_string(),
        })
    }

    #[instrument(skip(self))]
    async fn get_typed_links(
        &self,
        _id: WorkItemId,
    ) -> Result<Vec<TypedLink>, GitHubOperationError> {
        // SDK gap: GraphQL issueLink query not yet in github-bot-sdk.
        Err(GitHubOperationError::SdkCapabilityMissing {
            capability: "graphql_issue_link_query".to_string(),
        })
    }

    #[instrument(skip(self))]
    async fn get_labels(&self, _id: WorkItemId) -> Result<Vec<Label>, GitHubOperationError> {
        todo!("IssueTracker::get_labels — implemented in PR 10")
    }

    #[instrument(skip(self))]
    async fn add_label(&self, _id: WorkItemId, _label: &Label) -> Result<(), GitHubOperationError> {
        todo!("IssueTracker::add_label — implemented in PR 10")
    }

    #[instrument(skip(self))]
    async fn remove_label(
        &self,
        _id: WorkItemId,
        _label: &Label,
    ) -> Result<(), GitHubOperationError> {
        todo!("IssueTracker::remove_label — implemented in PR 10")
    }

    #[instrument(skip(self))]
    async fn post_comment(&self, _id: WorkItemId, _body: &str) -> Result<(), GitHubOperationError> {
        todo!("IssueTracker::post_comment — implemented in PR 10")
    }

    #[instrument(skip(self))]
    async fn list_comments(
        &self,
        _id: WorkItemId,
    ) -> Result<Vec<IssueComment>, GitHubOperationError> {
        // SDK gap: REST list issue comments not yet in github-bot-sdk.
        // Returns Err (not todo!()) so that run_step step 3 (state
        // reconstruction) falls through gracefully to a fresh-run
        // initialisation rather than panicking. Callers treat
        // SdkCapabilityMissing on list_comments as "no prior state found"
        // and proceed with a new run. This is intentional until the SDK
        // adds the REST endpoint.
        Err(GitHubOperationError::SdkCapabilityMissing {
            capability: "list_issue_comments".to_string(),
        })
    }

    #[instrument(skip(self))]
    async fn authenticated_bot_user_id(&self) -> Result<u64, GitHubOperationError> {
        todo!("IssueTracker::authenticated_bot_user_id — implemented in PR 10")
    }

    #[instrument(skip(self))]
    async fn get_issue_state(&self, _id: WorkItemId) -> Result<IssueState, GitHubOperationError> {
        todo!("IssueTracker::get_issue_state — implemented in PR 10")
    }

    #[instrument(skip(self))]
    async fn get_milestone(&self, _id: MilestoneId) -> Result<Milestone, GitHubOperationError> {
        todo!("IssueTracker::get_milestone — implemented in PR 10")
    }

    #[instrument(skip(self))]
    async fn set_milestone(
        &self,
        _id: WorkItemId,
        _milestone: Option<MilestoneId>,
    ) -> Result<(), GitHubOperationError> {
        // SDK gap: PATCH issue milestone not yet in github-bot-sdk.
        // Returns Err (not todo!()) so that callers on the write path receive a
        // propagatable error rather than a panic. Milestone assignment is
        // non-critical for pipeline correctness; callers log the
        // SdkCapabilityMissing error and continue rather than aborting the run.
        // This is intentional until the SDK adds the PATCH endpoint.
        Err(GitHubOperationError::SdkCapabilityMissing {
            capability: "patch_issue_milestone".to_string(),
        })
    }
}

// ─── PullRequestManager ──────────────────────────────────────────────────────

#[async_trait]
impl PullRequestManager for GithubClient {
    #[instrument(skip(self))]
    async fn create_pull_request(
        &self,
        _repository: &RepositoryId,
        _title: &str,
        _body: &str,
        _head: &BranchName,
        _base: &BranchName,
    ) -> Result<PullRequest, GitHubOperationError> {
        todo!("PullRequestManager::create_pull_request — implemented in PR 10")
    }

    #[instrument(skip(self))]
    async fn get_pull_request(
        &self,
        _repository: &RepositoryId,
        _id: PullRequestId,
    ) -> Result<PullRequest, GitHubOperationError> {
        todo!("PullRequestManager::get_pull_request — implemented in PR 10")
    }

    #[instrument(skip(self))]
    async fn find_pull_requests(
        &self,
        _repository: &RepositoryId,
        _filter: &PullRequestFilter,
    ) -> Result<Vec<PullRequest>, GitHubOperationError> {
        // SDK gap: list PRs with filter params not yet in github-bot-sdk.
        Err(GitHubOperationError::SdkCapabilityMissing {
            capability: "list_prs_with_filter".to_string(),
        })
    }

    #[instrument(skip(self))]
    async fn post_review_comment(
        &self,
        _repository: &RepositoryId,
        _id: PullRequestId,
        _commit_sha: &CommitSha,
        _path: &str,
        _line: u32,
        _body: &str,
    ) -> Result<(), GitHubOperationError> {
        // SDK gap: create inline PR review comment not yet in github-bot-sdk.
        Err(GitHubOperationError::SdkCapabilityMissing {
            capability: "create_pr_review_comment".to_string(),
        })
    }

    #[instrument(skip(self))]
    async fn get_review_status(
        &self,
        _repository: &RepositoryId,
        _id: PullRequestId,
    ) -> Result<ReviewStatus, GitHubOperationError> {
        todo!("PullRequestManager::get_review_status — implemented in PR 10")
    }
}

// ─── CodeRepository ──────────────────────────────────────────────────────────

#[async_trait]
impl CodeRepository for GithubClient {
    #[instrument(skip(self))]
    async fn read_file(
        &self,
        _repository: &RepositoryId,
        _path: &str,
        _git_ref: &str,
    ) -> Result<FileContent, GitHubOperationError> {
        // SDK gap: GitHub Contents API not yet in github-bot-sdk.
        Err(GitHubOperationError::SdkCapabilityMissing {
            capability: "contents_api_read_file".to_string(),
        })
    }

    #[instrument(skip(self))]
    async fn list_directory(
        &self,
        _repository: &RepositoryId,
        _path: &str,
        _git_ref: &str,
    ) -> Result<Vec<DirectoryEntry>, GitHubOperationError> {
        // SDK gap: GitHub Contents API not yet in github-bot-sdk.
        Err(GitHubOperationError::SdkCapabilityMissing {
            capability: "contents_api_list_directory".to_string(),
        })
    }

    #[instrument(skip(self))]
    async fn file_exists(
        &self,
        _repository: &RepositoryId,
        _path: &str,
        _git_ref: &str,
    ) -> Result<bool, GitHubOperationError> {
        // SDK gap: GitHub Contents API HEAD check not yet in github-bot-sdk.
        Err(GitHubOperationError::SdkCapabilityMissing {
            capability: "contents_api_file_exists".to_string(),
        })
    }

    #[instrument(skip(self))]
    async fn read_tree(
        &self,
        _repository: &RepositoryId,
        _git_ref: &str,
    ) -> Result<Vec<DirectoryEntry>, GitHubOperationError> {
        // SDK gap: GitHub Trees API (recursive) not yet in github-bot-sdk.
        Err(GitHubOperationError::SdkCapabilityMissing {
            capability: "trees_api_recursive".to_string(),
        })
    }
}

// ─── ProjectBoard ─────────────────────────────────────────────────────────────

#[async_trait]
impl ProjectBoard for GithubClient {
    #[instrument(skip(self))]
    async fn sync_item_status(
        &self,
        _work_item_id: WorkItemId,
        _status: &str,
    ) -> Result<(), GitHubOperationError> {
        todo!("ProjectBoard::sync_item_status — implemented in PR 10")
    }

    #[instrument(skip(self))]
    async fn sync_custom_field(
        &self,
        _work_item_id: WorkItemId,
        _field_name: &str,
        _value: &JsonValue,
    ) -> Result<(), GitHubOperationError> {
        todo!("ProjectBoard::sync_custom_field — implemented in PR 10")
    }
}

// ─── AuditStore ──────────────────────────────────────────────────────────────

#[async_trait]
impl AuditStore for GithubClient {
    /// Records an audit event as a Markdown-formatted comment on the work-item issue.
    ///
    /// Format: a collapsible `<details>` block with the event's JSON body inside
    /// a fenced code block. Each event is a separate comment to preserve the
    /// audit trail even if earlier comments are edited.
    ///
    /// Implementation detail (PR 10): batches events and flushes on a timer to
    /// avoid GitHub API rate-limit exhaustion during parallel node execution.
    #[instrument(skip(self, _event))]
    async fn record_event(
        &self,
        _run_id: PipelineRunId,
        _work_item_id: WorkItemId,
        _event: AuditEvent,
    ) -> Result<(), AuditStoreError> {
        todo!("AuditStore::record_event — implemented in PR 10")
    }

    /// Writes the pipeline run summary as a Markdown collapsible section
    /// appended to the work-item issue body or as a pinned comment.
    #[instrument(skip(self, _summary))]
    async fn write_summary(&self, _summary: &PipelineSummary) -> Result<(), AuditStoreError> {
        todo!("AuditStore::write_summary — implemented in PR 10")
    }
}
