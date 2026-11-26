# AI Agent Guidelines

This file provides guidance and context for AI coding assistants working on this project.

## Project Overview

CogWorks is a deterministic foreman: a stateless step function that reads GitHub state, decides the next action, executes one step, writes the result back, and exits. It runs a graph of agent stages, resolving each stage's agent profile from a pinned harness, building the exact inputs that stage may see, and delegating the attempt to a separate service that runs it in a fresh sandbox.

Key traits:
- **Stage types, not built-in roles**: CogWorks contains no agent personas, prompt text or discipline-specific logic. A node declares a stage type; its profile, stage contract and the stage graph itself resolve from the harness version the target repository pins.
- **It does not run agents, decide pass/fail, or push commits**: agent turns happen in sandboxes owned by the stage runner; verdicts are computed by the gate service and read from its check runs; commits are pushed by the stage runner under the stage's identity role. These are boundaries, not gaps — see `docs/spec/architecture.md` §Boundary Rules.
- **Oracle separation**: the tests that judge an implementation are written by a different stage, under a different identity, in a different sandbox, and sealed before implementation starts.
- **Domain-service architecture**: all build, simulation, lint, test and interface-extraction operations are delegated to independently deployed domain services over the Extension API.
- **Tier-driven gates**: a work item's tier is derived from path globs in a human-owned `risk-map.yaml`, raise-only, and selects the stages, thresholds and human gates that apply. A tier is never read from a label.
- **Audit trail and attestations**: every model call, state transition and domain service interaction is recorded, and each stage attempt produces a signed attestation naming the profile, harness version and context bundle hash.
- **Cross-domain awareness**: a human-maintained interface registry is validated to detect cross-domain constraint violations early.

CogWorks is one service in a larger system; the system-level architecture lives in the [`brightsmiths/handbook`](https://github.com/brightsmiths/handbook) repository under `docs/`.

See [docs/spec/overview.md](docs/spec/overview.md) for the full system context and [docs/spec/README.md](docs/spec/README.md) for the spec index.

## Production Software Standards

**This is production-grade software.** All code must meet production quality standards:

- **Complete Implementation**: No TODOs, placeholders, or "demonstration" code. Every feature must be fully implemented.
- **Comprehensive Error Handling**: All error paths must be handled properly, with clear error messages and proper error propagation.
- **Full Test Coverage**: All functionality must have comprehensive tests covering happy paths, error cases, and edge conditions.
- **Production-Ready Documentation**: All public APIs must have complete rustdoc with examples, error conditions, and behavioral specifications.
- **Security First**: All security-sensitive operations must be implemented with production-grade security measures.
- **Performance Conscious**: Code must be optimized for production workloads, not just correctness.
- **Observability**: All operations must have appropriate logging, metrics, and tracing for production debugging.

When implementing features:

- Write production code from the start - no prototypes or demos
- Think about failure scenarios and edge cases
- Consider operational concerns (monitoring, debugging, maintenance)
- Implement complete functionality, not partial demonstrations

## Pre-Implementation Checklist

Before implementing features, verify:

1. **Read Specifications**: Check docs/spec/ for relevant documentation
2. Read [docs/spec/constraints.md](docs/spec/constraints.md) (implementation rules and tripwires)
3. Read [docs/spec/requirements.md](docs/spec/requirements.md) (what must be true — use to confirm scope before adding code)
4. Read relevant standards in [docs/standards/](docs/standards/) (language/domain specific; may be empty early in the project)
5. **Search Existing Code**: Use semantic_search to find similar implementations
6. **Check Module Structure**: Determine if code belongs in existing module or needs new one
7. **Security Review**: Identify sensitive data (tokens, secrets) requiring special handling
8. **Plan Tests**: Identify test scenarios before writing implementation

## Summary

Following these conventions ensures:

- **Consistency**: Codebase looks like one person wrote it
- **Maintainability**: Easy to find and understand code
- **Quality**: High test coverage and clear documentation
- **Security**: Sensitive data handled properly
- **Performance**: Conscious resource management

When in doubt, look at existing code in the repository as examples of these patterns in practice.

## Workflow

1. Check for existing ADRs related to your task
2. Follow coding standards in .tech-decisions.yml
3. Write tests before implementation (TDD preferred)
4. Run pre-commit hooks before committing
5. Include context in commit messages

## Task Management

This project uses Beads (bd) for AI-friendly task tracking.

### Before starting work

1. Check what's ready: `bd ready --json`
2. Pick a task: `bd show bd-abc --json`
3. Start work: `bd update bd-abc working`

### When creating new tasks

1. Create issue: `bd create "Task description" -p 1 -t feature`
2. Add dependencies: `bd update bd-xyz --blocks bd-abc`
3. The task will auto-appear in `bd ready` when blockers are done

### When finishing work

1. Commit with issue ID: `git commit -m "Fix auth bug (bd-abc)"`
2. Close issue: `bd close bd-abc --reason "Completed"`
3. Sync: `bd sync` (usually automatic)

### Integration with ADRs

- Link ADRs in task descriptions: "See ADR-NNNN for context"
- Create tasks for implementing ADR decisions
- Reference task IDs in ADR implementation notes

### Quick reference

``bash
bd ready              # Show tasks ready to work on
bd create "desc" -p 1 # Create new task (priority 1-5)
bd show bd-xyz        # Show task details
bd update bd-xyz working  # Mark task in progress
bd close bd-xyz       # Close completed task
bd search "keyword"   # Search tasks
bd doctor             # Check for orphaned work
``
