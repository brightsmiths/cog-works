# ADR-0008: Pipeline Executor State Trust, Dispatch Seam, and Config Sourcing

**Status:** Accepted
**Date:** 2026-10-26
**Deciders:** Tech Lead (task #57), Security Reviewer, PR #176 review

---

## Context

Implementing `PipelineExecutor`/`run_step` (`crates/nodes/src/executor/`, task #57) surfaced
four decisions not fully resolved by `docs/spec/interfaces/nodes.md` as originally written.
Each is recorded here so the reasoning is discoverable later, rather than only visible in
commit messages and PR discussion.

## Decision 1 — `CliConfig` needs a repository identifier

Step 1 of `run_step` (`steps::load_constitutional_rules`) calls
`CodeRepository::read_file(repository: &RepositoryId, path, git_ref)` to read
`.cogworks/constitutional-rules.md`. No parameter available to `run_step`
(`executor: &PipelineExecutor`, `work_item_id: WorkItemId`, `config: &CliConfig`) carried a
`RepositoryId` — the original `CliConfig` had only `working_dir`, `pipeline_name`, and
`approved_branch`.

**Decision**: add `pub repository: RepositoryId` to `CliConfig` (`crates/nodes/src/node.rs`).
This is additive to a CLI-level configuration struct (not part of `PipelineExecutor::new`'s
fixed constructor signature), consistent with `CliConfig`'s documented purpose ("CLI-level
launch configuration... held unchanged for the lifetime of a service-mode run"). Documented
in `docs/spec/interfaces/nodes.md` §Part 3.

## Decision 2 — `NodeRunner` test seam, not a hardcoded node-name allowlist

The seven concrete node bodies (`IntakeNode::execute`, etc.) remain `todo!()` stubs under
separate open issues (#58–#62), not dependencies of #57. `PipelineExecutor`'s field list and
constructor are fixed by spec with no node-dispatch-table parameter, and `NodeId` is a
free-form string newtype, not a fixed enum — the pipeline graph (and therefore node dispatch)
is intentionally data-driven, not hardcoded to a specific set of node modules.

**Decision**: a `pub(crate)` trait `NodeRunner` (`crates/nodes/src/executor/runner.rs`)
dispatches one node's `execute` call given its `NodeId`/`NodeInput`. Production code gets
`DefaultNodeRunner`, which matches `NodeId::as_str()` against the eight known production names
and calls the corresponding module — calls that will themselves panic via the node's own
`todo!()` until #58–#62 land (expected, out of scope for #57). Tests use a `MockNodeRunner`,
so `run_step`'s orchestration (budget checks, parallel dispatch, edge evaluation, state
persistence) is fully testable without depending on those still-open issues.

**Follow-up from PR #176 review**: `DefaultNodeRunner::run_node` originally `panic!`ed on an
unrecognised `NodeId` — reachable in practice, since node names ultimately come from
user-supplied `.cogworks/pipeline.toml` configuration, and a panic in a service-mode process
shared across concurrent pipeline runs risks taking down unrelated work items. Fixed: an
unrecognised id now returns `NodeExecutionResult::Failure` (`PipelineError::NodeFailed`,
`RetryPolicy::NonRetryable`) instead of panicking.

The review additionally suggested *also* validating node names against the known list in step
2 (graph load), so a bad name halts with `GraphInvalid` before any dispatch is attempted.
**This suggestion was evaluated and deliberately not implemented**: doing so would hardcode
the generic, data-driven orchestration layer (`run_step_with_runner`, tested via the
`NodeRunner` seam) against the specific set of eight production node names — exactly the
coupling Decision 2's `NodeRunner` design exists to avoid. It would also break the sixteen-odd
existing orchestration tests, which deliberately use arbitrary node-id fixtures (`"a"`,
`"solo"`, `"review-gate"`, etc.) decoupled from real node names, by design, to prove
orchestration logic independent of which concrete node modules exist. The graceful-failure fix
above already resolves the actual safety concern (no more pipeline-crashing panic); a
redundant allowlist only adds coupling cost for a marginal, non-safety-critical benefit
(distinguishing a `NodeFailed` halt from a `GraphInvalid` halt for what is, either way, an
operator configuration mistake).

## Decision 3 — Graph-hash drift on resume escalates, does not halt or silently resume

`PipelineStateComment::graph_hash` is compared, on resume, against a freshly computed hash of
the just-loaded `PipelineGraph` (`docs/spec/constraints.md` §Pipeline Graph implies state must
be recoverable from GitHub, but says nothing about drift). A mismatch means the pipeline
configuration changed between the run that wrote the persisted state and the run attempting to
resume it (e.g. `.cogworks/pipeline.toml` edited mid-run).

**Decision**: a mismatch returns `StepResult::Escalated`, not `Halted` and not a silent resume
against the new graph. Rationale: silently resuming against a changed graph risks acting on
stale per-node state (rework counts, gate approvals) that no longer corresponds to the actual
graph structure — a correctness and safety concern, not merely an inconvenience. But the
situation is also not an unrecoverable system failure (`Halted`) — it is exactly the kind of
"pipeline cannot continue without human intervention" case `StepResult::Escalated` exists for:
a human must look at the drift and decide whether to restart the run or adjust the
configuration back. No node is executed when drift is detected (`steps::finalize_graph_hash_drift`).

## Decision 4 — State-reconstruction authenticity (THREAT-022)

Covered in full in `docs/spec/security.md` §THREAT-022; summarised here for cross-reference.
`reconstruct_state` (step 3) originally trusted any GitHub comment that merely parsed as
`PipelineStateComment` JSON, with no author check — a Critical finding from the Security
Reviewer during #57's own review pass, fixed in the same PR. `IssueComment` now exposes a
numeric `author_id`; `IssueTracker` exposes `authenticated_bot_user_id()`; `reconstruct_state`
requires both the author-ID match and a `work_item_id` self-consistency check before trusting
any candidate comment as pipeline state.

**Deployment note (PR #176 review)**: `authenticated_bot_user_id()` trusts whatever identity
the configured GitHub credential resolves to. Operators MUST use a dedicated bot/App identity
— a credential shared with a human's own interactive GitHub use would make every comment that
person posts under their own account automatically trusted as pipeline state. See
`docs/spec/security.md` §THREAT-022 "Deployment assumption" for the full note.

**Performance note (PR #176 review)**: the trait method's doc comment now recommends
implementations cache the result (the identity is invariant for the credential's lifetime),
per `docs/spec/constraints.md` §Performance. No caching is implemented yet because the only
concrete implementation (`GithubClient`) is itself still a `todo!()` skeleton (#63) — there is
nothing yet to measure or cache. Revisit when #63 lands.

---

## Consequences

- **Positive**: `run_step`'s orchestration logic is fully unit-testable today, independent of
  seven still-open node-implementation issues. The state-reconstruction attack surface
  identified mid-implementation is closed in the same change, not deferred. `CliConfig`'s
  additive field and the escalate-on-drift behaviour are both documented at their canonical
  spec location (`docs/spec/interfaces/nodes.md`), not just in commit history.
- **Negative**: `DefaultNodeRunner`'s node-name matching (Decision 2) is a second, independent
  place (besides whatever structural graph validation `pipeline::validate_pipeline_graph`
  already does) where a misconfigured node name can surface — as a `NodeFailed` halt at
  dispatch time rather than a `GraphInvalid` halt at load time. This is a deliberate trade-off,
  not an oversight (see Decision 2's follow-up note).
- **Follow-up required**: when #58–#62 (the real node bodies) land, `DefaultNodeRunner` will
  need the real resolved `PipelineGraph` threaded through instead of `runner.rs`'s current
  placeholder (tracked inline in that file). When #63 (`GithubClient`) lands,
  `authenticated_bot_user_id()` should be revisited for caching per the performance note above.

---

## References

- `docs/spec/interfaces/nodes.md` §Pipeline Executor, §Part 3 (CliConfig)
- `docs/spec/security.md` §THREAT-022 — Pipeline State Comment Forgery
- `docs/spec/constraints.md` §Pipeline Graph, §Performance
- `crates/nodes/src/executor/{mod,steps,dispatch,completion,runner}.rs`
- `.llm/findings/pipeline-executor-57.md`
- PR #176 — https://github.com/pvandervelde/cog_works/pull/176
