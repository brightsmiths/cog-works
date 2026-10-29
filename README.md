# cog_works
An AI-powered development workshop where specialized agents forge GitHub issues into polished features - from initial specs through documentation, testing, and implementation.

## Status

CogWorks is under active development. The composition root (the `cogworks` CLI binary) is not
yet wired up — there is no end-user command to run today. The sections below document pipeline
*node* behavior as it lands, so repository operators and reviewers know what to expect once a
work item starts flowing through the pipeline.

## Pipeline Nodes

The pipeline is a directed graph of nodes that process a GitHub issue (a "work item") from intake
through to a reviewed Pull Request. See `docs/spec/overview.md` for the full architecture; this
section covers only the operator-visible behavior of nodes implemented so far.

### 1. Intake

The Intake node is the first node in every pipeline run. For a work item, it:

1. **Fetches the GitHub issue** identified by the pipeline's `work_item_id`.
2. **Scans the issue title and body for prompt-injection attempts**, before any LLM call is made.
   If either is flagged, CogWorks applies the `cogworks:security-hold` label to the issue and
   halts — the issue is **not** classified or processed further until a human investigates and
   removes the hold.
3. **Classifies the work item** (task type, whether it's safety-affecting, an estimated scope
   from 1–10, and the modules it's expected to touch) via an LLM call.
4. **Applies a one-way safety override**: if any of the modules the work item is expected to
   touch match the repository's safety-critical registry, the work item is always treated as
   safety-affecting, regardless of what the classification step produced. This flag can only be
   turned on by the override, never off.
5. **Checks the estimated scope against the repository's configured threshold.** Work items
   judged too large to safely proceed in one pass are escalated — the pipeline halts and surfaces
   the estimated scope and the configured threshold for a human to split the work item into
   smaller pieces, rather than proceeding automatically.
6. **Extracts requirement tags from the issue body** for traceability. Any line containing a
   `REQ-<number>:` tag (e.g. `REQ-001: the gateway must reject malformed responses`) is recorded
   as a tracked requirement; the rest of that line becomes its description. Issue authors who want
   requirement-level traceability through the pipeline (architecture → interfaces → sub-work-items
   → code) should write requirements this way in the issue body.

If none of the above halts the run, the Intake node hands off a classification result and an
initial (zero-coverage) traceability matrix to the next node in the pipeline.

<!-- TODO: once the `cogworks` CLI composition root (tracked separately) is implemented, document
     the actual end-user trigger (labeling an issue `cogworks:run`, CLI invocation, webhook, etc.)
     and which exit code / label an operator will observe for the scope-exceeded escalation above —
     today the Intake node halts the pipeline step and surfaces the error, but no higher layer that
     maps this to a GitHub-visible label (e.g. `cogworks:status:escalated`) exists yet. -->
