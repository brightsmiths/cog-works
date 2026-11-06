# ADR-0009: Thread Real `ConstitutionalRules` into Node `execute` Signatures

**Status:** Accepted
**Date:** 2026-10-31
**Deciders:** Tech Lead (task #59 follow-up), Security Reviewer

---

## Context

A Security Reviewer audit of `ArchitectureNode::execute` (`crates/nodes/src/architecture.rs`,
task #57/#59 follow-up) found a High-severity finding (H1): the node built its own
`ConstitutionalRules` value internally (`node_constitutional_rules()`), with `content` fixed
to a hardcoded literal and `source_hash` computed from that same literal at call time.

`assemble_constitutional_prompt` → `validate_constitutional_prompt` exists specifically to
detect **tampering between load and use** — it recomputes the SHA-256 digest of
`ConstitutionalRules::content` and compares it against the `source_hash` recorded at load
time (see `docs/spec/interfaces/security.md` §`validate_constitutional_prompt`). A node that
manufactures its own rules value and then validates it against itself can never fail that
check: the hash is always self-consistent by construction, regardless of what the *real*
constitutional rules document (loaded once per pipeline step from the approved branch, via
`executor/steps.rs::load_constitutional_rules`) actually says. The constitutional layer's
entire security property — "the rules enforced at LLM-call time are the same, unmodified,
operator-reviewed rules loaded at the start of the step" — was unenforced for this node in
practice, even though the validation function itself was implemented correctly.

This gap existed because `docs/spec/interfaces/nodes.md` §`ArchitectureNode` (as originally
written) and `NodeRunner::run_node` (`crates/nodes/src/executor/runner.rs`) did not thread a
`ConstitutionalRules` parameter from the step-level load (`StepContext.rules`, held by
`run_step_with_runner`, see ADR-0008) down into any node's `execute` function. Every node
`execute` signature was effectively closed to this input.

## Decision

Thread the real, step-loaded `ConstitutionalRules` value end-to-end for `ArchitectureNode`:

1. **`ArchitectureNode::execute`** (`crates/nodes/src/architecture.rs`) gains a trailing
   parameter `rules: &pipeline::ConstitutionalRules`. The node-internal
   `node_constitutional_rules()` helper and its backing `CONSTITUTIONAL_RULES_CONTENT`
   constant are removed entirely. Every `assemble_constitutional_prompt(...)` call site
   inside the node (the architecture-generation call and the LLM-semantic alignment call)
   now uses the caller-supplied `rules` directly.
2. **`NodeRunner::run_node`** (`crates/nodes/src/executor/runner.rs`) gains a
   `rules: &ConstitutionalRules` parameter. `DefaultNodeRunner::run_architecture` forwards it
   to `ArchitectureNode::execute`; `DefaultNodeRunner::run_node`'s match arms for the other
   seven node ids simply ignore the new parameter today (their node bodies are still
   `todo!()` stubs under separate issues and do not yet accept a rules parameter — see
   "Residual scope" below).
3. **`executor/dispatch.rs`**: `execute_single`, `execute_parallel`, and
   `run_batch_concurrently` pass `&ctx.rules` — the genuinely step-loaded value already held
   on `StepContext` (see ADR-0008) — through to every `runner.run_node(...)` call site. No new
   loading or construction of rules happens in `dispatch.rs`; it only forwards what step 1 of
   `run_step_with_runner` already validated and loaded.
4. `docs/spec/interfaces/nodes.md` §`ArchitectureNode` is updated to document the widened
   signature and the provenance of `rules`.

With this change, a tampered or otherwise invalid `ConstitutionalRules` value reaching
`ArchitectureNode::execute` is rejected by `assemble_constitutional_prompt` before any LLM
call is made, exactly as the constitutional layer's tamper-detection design intends — because
the value being validated is now the one actually loaded from the approved branch, not a
value the node manufactured to always pass its own check.

## Consequences

- **Positive**: `ArchitectureNode`'s constitutional gate is now a genuine security control
  rather than a self-consistent no-op. A compromised or stale rules document loaded at step 1
  now visibly fails closed at every LLM call this node makes, instead of being silently
  bypassed by a node-internal substitute.
- **Positive**: The `NodeRunner` trait and `StepContext.rules` plumbing (ADR-0008) already
  existed for exactly this purpose; this change is additive wiring, not new architecture.
- **Negative / residual scope**: Only `ArchitectureNode` is wired today. The other seven node
  modules (`IntakeNode`, `InterfaceDesignNode`, `PlanningNode`, `CodeGenerationNode`,
  `ReviewNode`, `IntegrationNode`, `SpawningNode`) remain `todo!()` stubs under separate
  issues (#58–#62 and successors) and do not yet accept a `ConstitutionalRules` parameter on
  their own `execute` functions, even though `DefaultNodeRunner::run_node` now has the real
  value available to forward to them. Any node implementation that calls
  `assemble_constitutional_prompt`/`LlmGateway::call` is itself making an LLM call under
  constitutional rules, and therefore needs the same treatment this ADR gives
  `ArchitectureNode`.
- **Follow-up required (standing guidance, not a new task)**: when implementing any future
  node's `execute` body that makes an LLM call, add a `rules: &ConstitutionalRules` trailing
  parameter to that node's `execute` signature (mirroring `ArchitectureNode`'s), forward it
  from the corresponding `DefaultNodeRunner::run_*` method (the `rules` argument is already
  available in `run_node`'s scope — no further plumbing through `dispatch.rs` or
  `StepContext` is needed), and do **not** construct a node-internal "always valid"
  `ConstitutionalRules` value as a substitute. This is the same mistake H1 fixes here; it
  should not be repeated node by node.

---

## References

- `docs/spec/interfaces/nodes.md` §`ArchitectureNode`, §`LlmGateway::call`
- `docs/spec/interfaces/security.md` §`validate_constitutional_prompt`, §`ConstitutionalRules`
- `docs/spec/interfaces/domain-traits.md` §`LlmError` (Fix H2, recorded in this same change)
- `crates/nodes/src/architecture.rs`, `crates/nodes/src/executor/{runner,dispatch,mod}.rs`
- ADR-0008 — Pipeline Executor State Trust, Dispatch Seam, and Config Sourcing (`StepContext.rules`
  provenance, `NodeRunner` test seam)
- `crates/nodes/src/architecture_tests.rs` — Fix H1 test suite (`valid_constitutional_rules`,
  `tampered_constitutional_rules`, `distinctive_constitutional_rules`)
- `crates/nodes/src/executor_tests.rs` — Fix H1 executor-level test suite (`MockNodeRunner::rules_seen`)
