# Architecture Decision Records

This folder holds the architecture decision records (ADRs) for CogWorks. An ADR captures one
architecturally significant decision: the forces that bound it, what is decided, what that
costs, and what was rejected.

System-level decisions that apply across all Brightsmiths services are not recorded here. They
live in the [`brightsmiths/handbook`](https://github.com/brightsmiths/handbook) repository under
`docs/adr/`. An ADR in this folder may follow a handbook decision, but it does not restate one.

`.tech-decisions.yml` lists the categories of change that require an ADR: architectural
decisions, technology choices, security model changes, and Extension API changes.

## Index

| ADR | Title | Level |
| --- | ----- | ----- |
| [ADR-0001](ADR-0001-dynamic-capability-discovery-via-handshake.md) | Dynamic Capability Discovery via Handshake | L1 |
| [ADR-0002](ADR-0002-context-packs-feed-the-context-bundle.md) | Context Packs Feed the Context Bundle | L1 |
| [ADR-0003](ADR-0003-pipeline-state-trust-and-stage-dispatch.md) | Pipeline State Trust and Stage Dispatch | L1 |
| [ADR-0004](ADR-0004-provider-neutral-stage-attempt-runtime.md) | Provider-Neutral Stage Attempt Runtime | L1 |
| [ADR-0005](ADR-0005-credential-and-attestation-provider-boundaries.md) | Credential and Attestation Provider Boundaries | L1 |
| [ADR-0006](ADR-0006-a-node-is-a-stage-type-profiles-and-graphs-from-the-harness.md) | A Node Is a Stage Type; Profiles and Stage Graphs Come From the Pinned Harness | L1 |
| [ADR-0007](ADR-0007-oracle-and-implementation-are-separate-stages.md) | Oracle and Implementation Are Separate Stages, Identities and Sandboxes | L1 |
| [ADR-0008](ADR-0008-the-gate-service-is-the-sole-source-of-pass-or-fail.md) | The Gate Service Is the Sole Source of Pass or Fail | L1 |
| [ADR-0009](ADR-0009-assurance-review-produces-findings-not-verdicts.md) | Assurance Review Produces Findings, Not Verdicts | L1 |
| [ADR-0010](ADR-0010-context-bundles-are-allowlisted-hashed-and-attested.md) | Context Bundles Are Allowlisted, Hashed and Attested | L1 |
| [ADR-0011](ADR-0011-prompt-assets-load-from-the-pinned-harness.md) | Prompt Templates, Output Schemas and Constitutional Rules Load From the Pinned Harness | L1 |
| [ADR-0012](ADR-0012-tier-derivation-from-the-risk-map-raise-only.md) | Tier Derivation From the Risk Map, Raise-Only | L2 |
| [ADR-0013](ADR-0013-stage-attestations-are-signed-and-transported-through-github.md) | Stage Attestations Are Signed and Transported Through GitHub | L2 |
| [ADR-0014](ADR-0014-upstream-attestations-are-verified-not-recomputed.md) | Upstream Attestations Are Verified, Not Recomputed | L3 |
| [ADR-0015](ADR-0015-specification-implementation-and-acceptance-repository-split.md) | Specification, Implementation and Acceptance Repository Split, and Holdouts | L4 |

The **Level** column gives the increment at which a decision comes into force. Levels correspond
to the increments of CogWorks' role in the wider factory, described in the
[`brightsmiths/handbook`](https://github.com/brightsmiths/handbook) repository under
`docs/roadmap.md`. A decision at a later level is recorded now and implemented when that level is
built.

## Writing an ADR

Copy [ADR_TEMPLATE.md](ADR_TEMPLATE.md) and fill it in. The house rules:

- **Fill every section.** An ADR with an empty Context, or with no alternatives considered, is
  a statement rather than a decision record.
- **ADRs are forward-looking.** They describe the product as it will be, never as it was. No
  migration narrative, no "this replaces", no account of how the design arrived here, what an
  earlier revision did, or what a review found. Git history is the record of how the design
  moved.
- **No `Follow-up required` section.** Open work belongs in a GitHub issue, where it can be
  scheduled and closed. An ADR that carries its own to-do list goes stale silently.
- **References carry specification paths and handbook pages only** — no pull request links, no
  issue links, no review findings, and nothing under `.llm/` (that folder is git-ignored and
  temporary, so a reference to it cannot be followed).
- **One decision per ADR.** If a document needs two unrelated Decision sections, it is two
  ADRs.

## Numbering

- Numbers are four digits, zero-padded, and assigned sequentially: `ADR-NNNN`.
- The file name is the number followed by a lowercase, hyphenated form of the title:
  `ADR-0004-provider-neutral-implementation-job-runtime.md`.
- **The sequence is contiguous.** There are no gaps and no duplicates.

## Superseded decisions are deleted

When a decision stops describing the product, its ADR is **removed from this folder** and the
remaining ADRs are renumbered to close the gap. It is not marked `Superseded`, and no
breadcrumb is left behind.

This is deliberate, and it is the opposite of what many projects do. The reasoning: a reader
of this folder is trying to understand the system that exists. A record of a decision that no
longer holds does not help them do that, and a folder of tombstones makes the live decisions
harder to find. Git history is the record of how the design moved, and it is a better one —
it carries the diff, the date, the author and the surrounding change.

Renumbering means every citation of an ADR number has to move with it. Before finishing a
change that deletes or renumbers an ADR, search the repository for `ADR-00` and confirm that
every result points at a file that exists.
