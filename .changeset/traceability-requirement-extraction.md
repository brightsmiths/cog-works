---
"pipeline": patch
---

Implement `extract_requirements` and `update_traceability_matrix` in `crates/pipeline/src/traceability.rs`.

These complete the ISO 9001 traceability chain from requirement through architecture, interface, sub-work-item, and code:

- **`extract_requirements`**: scans a GitHub work item's Markdown body for lines containing `REQ-<digits>:` tags (case-insensitive) and returns one `Requirement` per match, with the trimmed line remainder as the description. Malformed or non-matching lines are silently skipped; the function never panics and never errors. Implemented as a hand-rolled linear byte scanner (no `regex` dependency) so it is safe against catastrophic backtracking on untrusted issue-body text.
- **`update_traceability_matrix`**: applies LLM-semantic-alignment `TraceabilityEntry` results for a given pipeline stage (Architecture / Interface / SubWorkItem / Code) to a `TraceabilityMatrix`, setting the matching requirement row's coverage flag, then recomputing each row's `Missing`/`Partial`/`Complete` status and the matrix-level `complete` flag. Takes and returns the matrix by value.

Both functions were previously `todo!()` stubs behind an already-published interface contract (`docs/spec/interfaces/advanced-features.md` §Traceability Matrix); their public signatures are unchanged. No new CLI command, config option, or externally visible API surface is introduced — the pipeline's intake and architecture nodes that will call these functions are tracked separately and not yet implemented.
