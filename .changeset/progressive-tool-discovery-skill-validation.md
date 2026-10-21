---
"pipeline": minor
---

Implement `build_compact_index`, `search_tools`, and `validate_skill_invocation` in `crates/pipeline/src/tools.rs`, completing the progressive tool discovery and skill validation subsystems.

`build_compact_index` turns a tool list and the set of `ToolProfile`s into a `CompactToolIndex` ranked for LLM-driven progressive discovery: entries whose name appears in any profile's `allowed_skills` are marked `is_skill: true` and sorted ahead of raw tools, alphabetically within each group. Entry `description` is currently always empty — enrichment from richer tool/skill metadata is deferred to the forthcoming `nodes` crate.

`search_tools` performs a case-insensitive substring search over a `CompactToolIndex`'s `tool_name` and `description` fields, preserving the index's existing order so that skills continue to rank ahead of raw tools when the index was built with `build_compact_index`.

`validate_skill_invocation` is the gate that must run before any skill is invoked. It checks, in order: the skill's lifecycle is `Active` (rejecting `Proposed`, `Reviewed`, `Deprecated`, or `Retired` with `SkillError::LifecycleInactive`); the skill is listed in the active `ToolProfile.allowed_skills` (otherwise `SkillError::ProfileProhibited`); and the caller-supplied invocation parameters conform to the skill manifest's `parameter_schema` (otherwise `SkillError::SchemaValidationFailed`). On success it returns an opaque `ValidatedSkillInvocation` — constructible only by this function — carrying the manifest's `tool_call_sequence` for the caller to execute.

Schema conformance checking is a small hand-rolled subset of JSON Schema, supporting only the `type`, `required`, and `properties` keywords (no `enum`, `pattern`, `additionalProperties`, `items`, or numeric bounds), consistent with this project's minimal-dependencies policy. Skill manifest authors relying on unsupported keywords for security-relevant constraints will not have those constraints enforced by this gate.

`SkillLifecycle` now implements `std::fmt::Display`, replacing an internal-only free function, for consistency with other enums in the crate (`RootCause`, `SummaryLevel`, etc.).

No public API shapes changed — `build_compact_index`, `search_tools`, `validate_skill_invocation`, and their supporting types were already fully specified; this change fills in the previously-`todo!()` implementation bodies. No breaking change: nothing could successfully call these functions before (they always panicked via `todo!()`).
