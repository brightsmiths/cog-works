# ISO 9001 Traceability — Context Pack Source Material

> **Status:** Skeleton — to be completed by domain expert
> **Pack ID:** `iso9001-traceability`
> **Trigger:** `safety_affecting: true`

> **Relationship to Context Packs:** This document is the *source material* for the corresponding Context Pack in the harness's software discipline pack. It lives in `docs/standards/` so it can be reviewed and maintained independently, and moves to the harness once that repository exists. Its content is summarised into the pack's knowledge, safe-pattern and anti-pattern files; the pack loader does **not** follow references back to `docs/standards/` — only the pack's own files reach a context bundle.

This document provides the domain knowledge reference for the `iso9001-traceability` Context Pack. It informs the LLM about required traceability artefacts, linkage requirements, and documentation standards for safety-affecting work items.

---

## Domain Knowledge

*To be completed. Sections to cover:*

- Required traceability chain: requirement → design decision → implementation → test evidence
- Document control requirements (version, date, author, review status)
- Change management traceability (what changed, why, who approved)
- Non-conformance recording and corrective action requirements
- Design review evidence requirements

---

## Safe Patterns

*To be completed. Sections to cover:*

- Explicit requirement IDs referenced in code comments and test names
- Design decision rationale documented in ADRs linked to requirements
- Test evidence linked to specific requirements (coverage matrix)
- Change impact analysis documented before implementation

---

## Anti-Patterns

*To be completed. Each entry should explain **why** the pattern is unsafe.*

- Implementation without traceable requirement (uncontrolled scope creep)
- Test without requirement linkage (untraceable coverage claim)
- Design decision without documented rationale (audit failure)
- Missing change impact analysis (undetected side effects on other requirements)

---

## Required Artefacts

*To be completed. Each entry defines an artefact that must be present in pipeline output.*

- Requirement traceability matrix (requirement ID → implementation file → test file)
- Design decision rationale (ADR or inline documentation)
- Test evidence with requirement linkage
- Change impact analysis for modifications to existing safety-affecting modules
