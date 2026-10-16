---
"pipeline": minor
---

Implement the four observability hook functions (`record_node_start`, `record_node_complete`, `record_retry`, `record_rework`) in `crates/pipeline/src/observability.rs`, completing the pipeline's observability-hooks subsystem.

Each hook records OTel-aligned structured fields onto a caller-provided `tracing::Span` via `span.record(...)`:

- **`record_node_start`**: `pipeline.run_id`, `pipeline.node_id`, `pipeline.event = "node_start"`.
- **`record_node_complete`**: the above plus `pipeline.event = "node_complete"` and `pipeline.token_cost.total_usd`.
- **`record_retry`**: the above plus `pipeline.event = "node_retry"` and `pipeline.retry.root_cause`.
- **`record_rework`**: the above plus `pipeline.event = "node_rework"` and `pipeline.rework.misalignment_type`.

These are the canonical way the `nodes` crate will record pipeline-level events (node timing, retry counts, rework counts, token usage) so that OTel attribute names stay consistent across all node types, feeding the OTel SDK configured in the `cli` crate. All four functions are purely side-effecting — they return no errors and do not panic, including when called with an empty or default span.

No public API shapes changed — the function signatures were already fully specified in `docs/spec/interfaces/advanced-features.md` §Observability Hooks; this change fills in the previously-`todo!()` implementation bodies.

**Not yet user-visible**: these hooks are not wired into any caller yet — that wiring is tracked separately against the pipeline executor / step runner (blocked by #57). There is no new CLI flag, config option, or change to CLI output, metrics, or OTel data in this release; nothing calls these functions outside of their own unit tests.

Documentation correction: the spec's `record_node_complete` field table (and the function's doc comment) previously listed `pipeline.token_cost.input` and `pipeline.token_cost.output` fields. `TokenCost` is a monetary-only type (USD) with no input/output token-count breakdown, so those two fields were never implementable as specified and are not emitted. The spec and doc comment have been corrected to reflect only the field that is actually emitted (`pipeline.token_cost.total_usd`); adding a token-count breakdown is tracked as issue #162.
