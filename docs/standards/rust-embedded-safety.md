# Rust Embedded Safety — Context Pack Source Material

> **Status:** Skeleton — to be completed by domain expert
> **Pack ID:** `rust-embedded-safety`
> **Trigger:** `component:firmware` or `safety_affecting: true` for Rust artifacts

> **Relationship to Context Packs:** This document is the *source material* for the corresponding Context Pack in the harness's software discipline pack. It lives in `docs/standards/` so it can be reviewed and maintained independently, and moves to the harness once that repository exists. Its content is summarised into the pack's knowledge, safe-pattern and anti-pattern files; the pack loader does **not** follow references back to `docs/standards/` — only the pack's own files reach a context bundle.

This document provides the domain knowledge reference for the `rust-embedded-safety` Context Pack. It informs the LLM about Rust `no_std` conventions, safety patterns, and anti-patterns specific to embedded targets.

---

## Domain Knowledge

*To be completed. Sections to cover:*

- `no_std` conventions and constraints for the target platform
- Error propagation patterns for embedded targets (no `Box<dyn Error>`, no heap allocation in error paths)
- Allocation constraints: stack budgets, arena allocators, static allocation patterns
- Interrupt safety: data structures, critical sections, volatile access
- Real-time constraints: bounded execution time, no blocking allocations in hot paths
- Watchdog and fault detection patterns

---

## Safe Patterns

*To be completed. Sections to cover:*

- Panic-free error handling (`Result` propagation, `defmt` for diagnostics)
- Bounded resource usage (fixed-capacity collections, compile-time size guarantees)
- Interrupt-safe data structures (atomic operations, SPSC queues)
- Type-state patterns for hardware peripherals (compile-time state machine enforcement)
- RTIC or Embassy patterns for concurrent access to shared resources

---

## Anti-Patterns

*To be completed. Each entry should explain **why** the pattern is unsafe.*

- `unwrap()` / `expect()` in production embedded code (causes panic → undefined recovery)
- Unbounded allocation in interrupt or real-time contexts (heap fragmentation, latency)
- Unguarded `unsafe` blocks without safety documentation (audit trail gap)
- Panic paths that reach production firmware (no panic handler, or panic handler that halts indefinitely)
- Floating-point arithmetic in contexts requiring deterministic timing (soft-float variability)
- Global mutable state without synchronization primitives

---

## Required Artefacts

*To be completed. Each entry defines an artefact that must be present in pipeline output.*

- Unsafe usage justification for each `unsafe` block
- Panic path analysis document (evidence that no panic path reaches production)
- Stack usage analysis or evidence of bounded allocation
