---
"nodes": minor
---

Implement `IntakeNode::execute` — the first node in every CogWorks pipeline run (`crates/nodes/src/intake.rs`), previously a `todo!()` stub.

- **Fail-closed injection screening**: both the GitHub issue title and body are scanned for prompt-injection patterns via `detect_injection` *before* any LLM call is made. If either is flagged, the work item is labeled `cogworks:security-hold` and the node halts without ever calling the LLM — classification never runs on a work item under suspicion of injection.
- **Classification**: the issue is classified (task type, safety-affecting flag, estimated scope 1–10, affected modules) via an LLM call wrapped in the constitutional-rules prompt (`assemble_constitutional_prompt`), so constitutional rules are enforced on this call like every other LLM-calling node.
- **One-way safety override**: if any affected module matches the repository's safety-critical registry, `safety_affecting` is forced to `true` regardless of what the LLM produced — the override can only turn the flag on, never off.
- **Scope escalation**: work items whose estimated scope exceeds the configured threshold are escalated (the pipeline halts, surfacing the estimated scope and the configured threshold) rather than proceeding automatically; this is intended to prompt a human to split the work item into smaller pieces.
- **Requirement extraction and traceability**: `REQ-<number>:` tags in the issue body are extracted and used to build the initial (zero-coverage) `TraceabilityMatrix`, which flows to downstream nodes as an output artifact alongside the classification result.

**Breaking change**: `PipelineConfig` (`crates/nodes/src/node.rs`) gained three new required fields — `safety_registry: SafetyCriticalRegistry`, `scope_threshold: u32`, and `constitutional_rules: ConstitutionalRules` — needed by the steps above. Anyone constructing `PipelineConfig` directly must now supply these. Today `PipelineConfig` is constructed in exactly one place in the workspace (`crates/nodes/src/executor/runner.rs`, updated here with a conservative default scope threshold and a deliberately-empty placeholder `ConstitutionalRules` until the executor is restructured to load real constitutional rules before constructing the runner), so there is no known external break today — but the struct's public shape did change, which matters for anyone building against this crate directly.

This crate is still not wired up to the `cli` binary (`crates/cli/src/main.rs` remains `todo!()`) — there is no new end-user command-line surface in this change. `IntakeNode::execute` remains an internal node body invoked by the pipeline executor.
