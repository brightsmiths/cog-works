//! Steps 1-4 of `run_step_with_runner`: load constitutional rules, resolve the
//! pipeline graph, reconstruct state from GitHub, and detect graph-hash drift
//! on resume.
//!
//! Split out of `executor/mod.rs` to keep that file within the project's
//! file-length standard; see `executor/mod.rs`'s module doc for the full
//! 8-step sequence this module contributes steps 1-4 to.

use std::collections::HashMap;

use pipeline::{
    ApprovedBranches, ConfigError, ConstitutionalRules, EscalationReason, GateConfig,
    PipelineConfiguration, PipelineError, PipelineGraph, PipelineRunId, PipelineState,
    PipelineStateComment, PromptAssembly, TokenCost, WorkItemId, validate_constitutional_prompt,
};

use crate::node::CliConfig;

use super::{PipelineExecutor, StepContext, StepResult, infra_halt, sentinel_node_id, sha256_hex};

/// Path (relative to the repository root) from which constitutional rules are
/// loaded at the start of every pipeline step.
const CONSTITUTIONAL_RULES_PATH: &str = ".cogworks/constitutional-rules.md";

// ─── Step 1: constitutional rules ────────────────────────────────────────────

/// Builds a [`StepResult::Halted`] with [`PipelineError::ConstitutionalRulesLoadFailed`].
fn constitutional_halt(message: String) -> StepResult {
    StepResult::Halted(PipelineError::ConstitutionalRulesLoadFailed { message })
}

/// Loads and validates the constitutional rules document (step 1).
///
/// Reads `.cogworks/constitutional-rules.md` from `config.repository` at
/// `config.approved_branch`, computes its content hash, and validates it via
/// [`validate_constitutional_prompt`]. Any failure halts immediately, before
/// any other infrastructure call is made.
pub(super) async fn load_constitutional_rules(
    executor: &PipelineExecutor,
    config: &CliConfig,
) -> Result<ConstitutionalRules, StepResult> {
    let file = executor
        .code_repository
        .read_file(
            &config.repository,
            CONSTITUTIONAL_RULES_PATH,
            config.approved_branch.as_str(),
        )
        .await
        .map_err(|err| {
            constitutional_halt(format!("failed to read constitutional rules: {err}"))
        })?;

    let content = String::from_utf8(file.content).map_err(|err| {
        constitutional_halt(format!(
            "constitutional rules file is not valid UTF-8: {err}"
        ))
    })?;

    let rules = ConstitutionalRules {
        source_hash: sha256_hex(content.as_bytes()),
        content,
        source_branch: config.approved_branch.clone(),
    };

    let approved = ApprovedBranches::new(vec![config.approved_branch.clone()]);
    let prompt = PromptAssembly {
        system_prompt: String::new(),
        user_content: String::new(),
    };
    validate_constitutional_prompt(&rules, prompt, &approved)
        .map_err(|err| constitutional_halt(err.to_string()))?;

    Ok(rules)
}

// ─── Step 2: pipeline configuration ──────────────────────────────────────────

/// Resolves the active [`PipelineGraph`] for this run (step 2).
///
/// `ConfigError::NotFound` falls back to the built-in default pipeline (per
/// that variant's documented contract); any other `ConfigError` halts with
/// [`PipelineError::GraphInvalid`], since those indicate a genuinely broken
/// configuration file rather than a simple absence.
pub(super) async fn resolve_pipeline_graph(
    executor: &PipelineExecutor,
    config: &CliConfig,
) -> Result<PipelineGraph, StepResult> {
    let configuration = match executor
        .config_loader
        .load_pipeline_config(&config.working_dir)
        .await
    {
        Ok(cfg) => cfg,
        Err(ConfigError::NotFound { .. }) => PipelineConfiguration {
            pipelines: HashMap::new(),
        },
        Err(err) => {
            return Err(StepResult::Halted(PipelineError::GraphInvalid {
                message: err.to_string(),
            }));
        }
    };

    let graph = match &config.pipeline_name {
        Some(name) => executor
            .config_loader
            .get_named_pipeline(&configuration, name)
            .cloned()
            .unwrap_or_else(|| executor.config_loader.get_default_pipeline(&configuration)),
        None => executor.config_loader.get_default_pipeline(&configuration),
    };
    Ok(graph)
}

// ─── Step 3: reconstruct state ───────────────────────────────────────────────

/// Reconstructs [`PipelineState`] and [`GateConfig`] from the latest
/// `PipelineStateComment` on the work-item issue (step 3).
///
/// Scans comments **newest first** for the first one that passes three
/// checks, in order (THREAT-022):
///
/// 1. **Author verification**: `comment.author_id` must match the
///    authenticated bot identity returned by
///    [`pipeline::IssueTracker::authenticated_bot_user_id`] — comments from
///    any other author are untrusted and skipped.
/// 2. **Deserialisation**: the body must parse as a [`PipelineStateComment`];
///    non-JSON comments (human discussion) are skipped without error.
/// 3. **Self-consistency**: the deserialised `work_item_id` must match
///    `work_item_id` — defense in depth against a misscoped or mismatched
///    comment, even though `list_comments` is expected to already scope to
///    one issue.
///
/// A comment that fails any check is skipped (not treated as "no state
/// found") and the scan continues to older comments. No match after scanning
/// every comment means a fresh run: a new [`PipelineState`] and the default
/// [`GateConfig`] are returned, along with a freshly generated
/// [`PipelineRunId`].
pub(super) async fn reconstruct_state(
    executor: &PipelineExecutor,
    work_item_id: WorkItemId,
) -> Result<
    (
        PipelineState,
        GateConfig,
        PipelineRunId,
        Option<PipelineStateComment>,
    ),
    StepResult,
> {
    let bot_user_id = executor
        .issues
        .authenticated_bot_user_id()
        .await
        .map_err(|err| {
            infra_halt(
                "pipeline-state-reconstruction",
                format!("failed to resolve authenticated bot user id: {err}"),
            )
        })?;

    let comments = executor
        .issues
        .list_comments(work_item_id)
        .await
        .map_err(|err| {
            infra_halt(
                "pipeline-state-reconstruction",
                format!("failed to list issue comments: {err}"),
            )
        })?;

    let prior_comment = comments.iter().rev().find_map(|c| {
        if c.author_id != bot_user_id {
            return None;
        }
        let candidate = serde_json::from_str::<PipelineStateComment>(&c.body).ok()?;
        if candidate.work_item_id != work_item_id {
            return None;
        }
        Some(candidate)
    });

    let (state, gate_config, run_id) = match &prior_comment {
        Some(comment) => (
            comment.state.clone(),
            comment.gate_config.clone(),
            comment.pipeline_run_id,
        ),
        None => {
            let run_id = PipelineRunId::new_random();
            (
                PipelineState {
                    run_id,
                    node_states: HashMap::new(),
                    active_parallel_branches: vec![],
                    cost_accumulator: TokenCost::zero(),
                },
                GateConfig::default(),
                run_id,
            )
        }
    };

    Ok((state, gate_config, run_id, prior_comment))
}

// ─── Step 4: graph-hash drift ────────────────────────────────────────────────

/// Builds the [`EscalationReason`] for a detected `graph_hash` mismatch on
/// resume.
///
/// The mismatch is not attributable to any single node (it reflects a drift
/// between the persisted state and the freshly loaded graph as a whole), so
/// the node with the highest `attempt_count` is reported as representative;
/// absent any node states, a sentinel node id is used.
fn graph_hash_drift_escalation(
    comment: &PipelineStateComment,
    computed_hash: &str,
) -> EscalationReason {
    let (node_id, attempt_count, rework_count) = comment
        .state
        .node_states
        .iter()
        .max_by_key(|(_, ns)| ns.attempt_count)
        .map(|(id, ns)| (id.clone(), ns.attempt_count, ns.rework_count))
        .unwrap_or_else(|| (sentinel_node_id("pipeline-graph"), 0, 0));
    EscalationReason {
        description: format!(
            "Pipeline graph hash drift detected on resume: persisted state was \
             computed against a different graph (persisted {}, current {computed_hash})",
            comment.graph_hash
        ),
        node_id,
        attempt_count,
        rework_count,
        cost_spent: comment.state.cost_accumulator,
    }
}

/// Handles a detected `graph_hash` mismatch: escalates without executing any
/// node (Architectural Decision #4).
pub(super) async fn finalize_graph_hash_drift(
    ctx: &StepContext<'_>,
    comment: &PipelineStateComment,
) -> StepResult {
    let reason = graph_hash_drift_escalation(comment, &ctx.graph_hash);
    super::completion::persist_terminal_state(ctx, &comment.state, &comment.gate_config).await;
    StepResult::Escalated(reason)
}
