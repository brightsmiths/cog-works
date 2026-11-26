# AI Agent Guidelines

This file provides guidance and context for AI coding assistants working on this project.

## Project Overview

CogWorks is an AI-assisted software development pipeline that automates the cycle from GitHub work item to Pull Request. Given an issue labelled `cogworks:run`, it classifies the task, generates a specification, designs interfaces, plans sub-tasks, iterates code generation with domain service feedback, validates against scenarios, and runs a multi-pass review gate — all without human intervention except at explicitly gated checkpoints.

Key traits:
- **Domain-service architecture**: CogWorks itself contains no language- or toolchain-specific logic. All build, simulation, lint, test, and interface-extraction operations are delegated to independently deployed domain services over the Extension API.
- **Safety-first gates**: Work items touching safety-critical modules always require human approval before any PR can be merged.
- **Audit trail**: Every LLM call, state transition, and domain service interaction is recorded for post-hoc review.
- **Cross-domain awareness**: A human-maintained interface registry is validated before every run to detect cross-domain constraint violations early.

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
