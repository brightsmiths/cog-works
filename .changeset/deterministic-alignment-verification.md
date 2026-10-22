---
"pipeline": minor
---

Implement `run_deterministic_alignment` in `crates/pipeline/src/alignment.rs`, completing the deterministic (no-LLM) half of the alignment verification subsystem.

`run_deterministic_alignment` runs three sequential structural checks against a node's output artifacts, returning a `Vec<AlignmentFinding>`:

1. Every slot in `DeclaredNodeInputs::required_output_slots` must be present as a key in `NodeOutput::artifacts`, else a blocking `Missing` finding.
2. No artifact key may fall outside `DeclaredNodeInputs::approved_scope`, else a blocking `ScopeExceeded` finding.
3. No artifact key may match a protected path (reusing `security::is_protected`), else a blocking `ScopeExceeded` finding.

An empty result means all deterministic checks passed; any finding returned by this function is blocking.

**Security-relevant behaviour**: the function fails closed on malformed artifact keys. Any artifact key that does not parse as a valid `ArtifactPath` (for example, a key containing a `..` path-traversal segment) now immediately produces a blocking `ScopeExceeded` finding, rather than falling through to a raw-string scope match. This closes a bypass where a single path-traversal-shaped key could simultaneously evade both the scope check and the protected-path check. Callers that previously treated unparseable artifact keys as a non-issue should expect a blocking finding for them going forward.

No public API shapes changed — `run_deterministic_alignment` and its supporting types (`AlignmentFinding`, `AlignmentCheckType`, `MisalignmentType`, `DeclaredNodeInputs`) were already fully specified; this change fills in the previously-`todo!()` implementation body. No breaking change: nothing could successfully call this function before (it always panicked via `todo!()`).
