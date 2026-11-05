//! Architecture node — architecture document generation and constraint validation.
//!
//! The Architecture node takes the Intake output and produces an architecture
//! document that describes the approach for implementing the work item. It:
//! 1. Loads context packs for the classified work item.
//! 2. Validates cross-domain constraints (interface registry check).
//! 3. Assembles context (pyramid summaries, interface definitions).
//! 4. Calls the LLM to produce the architecture document.
//! 5. Runs deterministic alignment.
//! 6. If `LlmSemantic` alignment is enabled: runs it and updates the traceability
//!    matrix from `AlignmentResult::traceability_entries`.
//!
//! ## Output Artifacts
//!
//! | Slot | Content |
//! |------|---------|
//! | `architecture_doc` | Markdown architecture document (string) |
//! | `traceability_matrix` | Updated [`pipeline::TraceabilityMatrix`] (JSON) |
//!
//! ## Implementation Notes (spec gaps)
//!
//! `docs/spec/interfaces/nodes.md` §`ArchitectureNode` and the task brief leave
//! several cross-cutting concerns unthreaded through `execute`'s signature.
//! The concrete decisions made here are documented exhaustively in the module
//! doc comment of `architecture_tests.rs` (the adversarial test suite this
//! implementation satisfies); in summary:
//!
//! - `AlignmentCheckType::LlmSemantic` runs iff `ClassificationResult::safety_affecting`.
//! - The cross-domain "extracted" `InterfaceMap` is a fail-closed empty map
//!   (this node has no `DomainServiceClient::extract_interfaces` source).
//! - A blocking finding from either alignment check maps to `NodeExecutionResult::Failure`.
//! - `update_traceability_matrix` is only called with the `LlmSemantic` result's
//!   `traceability_entries` — never with the deterministic result's (always-empty) entries.
//! - `execute` takes the real, operator-reviewed `rules: &pipeline::ConstitutionalRules`
//!   value loaded once per pipeline step by `executor/steps.rs::load_constitutional_rules`
//!   and forwarded via `NodeRunner::run_node` (Fix H1, task #59 security follow-up — see
//!   `docs/adr/ADR-0009-thread-constitutional-rules-into-node-execute.md`). This node
//!   previously built its own self-consistent, always-valid `ConstitutionalRules` value
//!   internally, which defeated the tamper-detection purpose of the constitutional layer.
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/nodes.md` §`ArchitectureNode`.

use std::collections::HashMap;
use std::path::PathBuf;

use pipeline::{
    AlignmentCheckType, AlignmentFinding, ApprovedScope, ClassificationResult, CodeRepository,
    ConstitutionalError, ConstitutionalRules, ConstraintFinding, ContextAssemblyRequest,
    ContextItem, ContextPackage, ContextPriority, DeclaredNodeInputs, DiagnosticSeverity,
    InterfaceDefinition, InterfaceMap, LlmError, MisalignmentType, ModelConfig, NodeId, NodeOutput,
    NodeType, OutputSchema, PipelineError, RegistryError, RetryPolicy, ScopeParameters,
    StructuredResponse, SummaryLevel, TokenCost, TokenCount, TraceabilityEntry, TraceabilityMatrix,
    TraceabilityStage, run_deterministic_alignment, update_traceability_matrix,
    validate_cross_domain_constraints,
};

use crate::gateway::{LlmGateway, assemble_constitutional_prompt};
use crate::node::{
    ConstraintValidator, ContextAssembler, ContextPackLoader, NodeExecutionResult, NodeFailure,
    NodeInput,
};

// ─── Input/output artifact slot names ────────────────────────────────────────

const CLASSIFICATION_SLOT: &str = "classification";
const ARCHITECTURE_DOC_SLOT: &str = "architecture_doc";
const TRACEABILITY_MATRIX_SLOT: &str = "traceability_matrix";

// ─── System prompts ───────────────────────────────────────────────────────────

const ARCHITECTURE_GENERATION_SYSTEM_PROMPT: &str = "You are the CogWorks Architecture node. Produce a Markdown architecture document that \
     describes the implementation approach for the classified work item, using only the \
     assembled context and matched Context Pack guidance. Respond with a JSON object of the \
     exact shape {\"architecture_document\": \"<markdown string>\"}.";

const SEMANTIC_ALIGNMENT_SYSTEM_PROMPT: &str = "You are the CogWorks Architecture alignment critic. Given the generated architecture \
     document and the original requirements context, determine whether the document is fully \
     aligned with the requirements. Respond with a JSON object of the exact shape \
     {\"aligned\": bool, \"score\": number, \"findings\": [...], \"traceability_entries\": \
     [{\"requirement_ref\": \"...\", \"output_ref\": \"...\"}]}.";

/// Architecture node — generates the architecture document and validates constraints.
///
/// See `docs/spec/interfaces/nodes.md` §`ArchitectureNode`.
pub struct ArchitectureNode;

impl ArchitectureNode {
    /// Executes the Architecture node for `input`.
    ///
    /// # Arguments
    ///
    /// * `input` — Node inputs including the `classification` artifact from Intake.
    /// * `gateway` — LLM gateway for architecture generation and semantic alignment.
    /// * `context` — Context assembler for building the node's context package.
    /// * `constraint_validator` — Validates cross-domain interface constraints.
    /// * `_pack_loader` — Unused: context packs are consumed directly from
    ///   `input.loaded_context_packs` (see module-level spec-gap notes).
    /// * `_github` — Unused by this node: the interface registry definitions
    ///   loaded via `constraint_validator` double as the `interface_entries`
    ///   context input, so no direct repository read is needed.
    /// * `rules` — The real, operator-reviewed constitutional rules loaded
    ///   once per pipeline step (Fix H1, task #59 security follow-up: this
    ///   node previously built its own self-consistent, always-valid
    ///   `ConstitutionalRules` value internally, defeating the tamper-
    ///   detection purpose of the constitutional layer). See ADR-0009.
    ///
    /// # Returns
    ///
    /// `NodeExecutionResult::Success` with `architecture_doc` and updated
    /// `traceability_matrix` artifacts, or `NodeExecutionResult::Failure`.
    ///
    /// See `docs/spec/interfaces/nodes.md` §`ArchitectureNode`.
    pub async fn execute(
        input: NodeInput,
        gateway: &LlmGateway,
        context: &ContextAssembler,
        constraint_validator: &ConstraintValidator,
        _pack_loader: &ContextPackLoader,
        _github: &dyn CodeRepository,
        rules: &ConstitutionalRules,
    ) -> NodeExecutionResult {
        let generated = match generate_architecture_document(
            &input,
            gateway,
            context,
            constraint_validator,
            rules,
        )
        .await
        {
            Ok(g) => g,
            Err(failure) => return NodeExecutionResult::Failure(failure),
        };

        let mut final_matrix = generated.matrix;
        let mut cost_delta = generated.cost_delta;
        if generated.classification.safety_affecting {
            let (updated_matrix, semantic_cost) = match maybe_run_semantic_alignment(
                gateway,
                rules,
                &generated.model,
                &generated.context_package,
                &generated.architecture_doc,
                final_matrix,
            )
            .await
            {
                Ok(pair) => pair,
                Err(failure) => return NodeExecutionResult::Failure(failure),
            };
            final_matrix = updated_matrix;
            cost_delta += semantic_cost;
        }

        NodeExecutionResult::Success(build_output(
            &generated.architecture_doc,
            &final_matrix,
            cost_delta,
        ))
    }
}

// ─── Steps 1-5: generate the architecture document ───────────────────────────

/// Intermediate state produced by steps 1-5 (input extraction through
/// deterministic alignment), carried forward into the conditional step 6
/// (LLM-semantic alignment) and the final output assembly.
struct GeneratedArchitecture {
    classification: ClassificationResult,
    matrix: TraceabilityMatrix,
    context_package: ContextPackage,
    model: ModelConfig,
    architecture_doc: String,
    cost_delta: TokenCost,
}

/// Runs steps 1-5 of the node spec: input extraction, cross-domain
/// constraint validation, context assembly, architecture generation, and
/// deterministic alignment. Returns `Err` on the first failure, matching
/// `execute`'s early-return-on-failure contract.
async fn generate_architecture_document(
    input: &NodeInput,
    gateway: &LlmGateway,
    context: &ContextAssembler,
    constraint_validator: &ConstraintValidator,
    rules: &ConstitutionalRules,
) -> Result<GeneratedArchitecture, NodeFailure> {
    let classification = extract_classification(input)?;
    let matrix = extract_traceability_matrix(input)?;

    let contracts = load_registry_contracts(constraint_validator).await?;
    let blocking_constraints = cross_domain_blocking_findings(&contracts);
    if !blocking_constraints.is_empty() {
        return Err(cross_domain_failure(&blocking_constraints));
    }

    let context_package = assemble_node_context(context, input, &classification, &contracts).await;

    let model = node_model_config();
    let generation_schema = architecture_output_schema();

    let response = call_architecture_llm(
        gateway,
        rules,
        ARCHITECTURE_GENERATION_SYSTEM_PROMPT,
        &context_package,
        &generation_schema,
        &model,
    )
    .await?;

    let cost_delta = response_cost(&response, &model);
    let architecture_doc = extract_architecture_document(&response);

    let candidate_output = build_output(&architecture_doc, &matrix, cost_delta);
    let det_findings = run_deterministic_alignment(&declared_node_inputs(), &candidate_output);
    if det_findings.iter().any(|f| f.blocking) {
        return Err(alignment_failure(&det_findings));
    }

    Ok(GeneratedArchitecture {
        classification,
        matrix,
        context_package,
        model,
        architecture_doc,
        cost_delta,
    })
}

// ─── Step 1/2: input extraction and cross-domain constraint validation ──────

fn extract_classification(input: &NodeInput) -> Result<ClassificationResult, NodeFailure> {
    input
        .artifacts
        .get(CLASSIFICATION_SLOT)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .ok_or_else(|| missing_artifact_failure(CLASSIFICATION_SLOT))
}

fn extract_traceability_matrix(input: &NodeInput) -> Result<TraceabilityMatrix, NodeFailure> {
    input
        .artifacts
        .get(TRACEABILITY_MATRIX_SLOT)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .ok_or_else(|| missing_artifact_failure(TRACEABILITY_MATRIX_SLOT))
}

async fn load_registry_contracts(
    validator: &ConstraintValidator,
) -> Result<Vec<InterfaceDefinition>, NodeFailure> {
    validator
        .registry_loader()
        .load_definitions()
        .await
        .map_err(|err| registry_failure(&err))
}

/// Validates the registry contracts against a fail-closed empty extracted
/// `InterfaceMap` (see module-level spec-gap notes: this node has no
/// `DomainServiceClient::extract_interfaces` source), returning only the
/// blocking findings.
///
/// This is "fail-closed" only in the sense that it never fabricates a
/// comparison; the practical effect is either vacuous (empty registry) or an
/// unconditional blocking failure on any populated registry — not a genuine
/// cross-domain check. Tracked as Medium security finding #1 and Spec Gap #3
/// in `.llm/findings/architecture-node.md`; an Architect decision on the real
/// `InterfaceMap` source is tracked in beads issue `architecture-node-p0k`.
fn cross_domain_blocking_findings(contracts: &[InterfaceDefinition]) -> Vec<ConstraintFinding> {
    let extracted = InterfaceMap { entries: vec![] };
    validate_cross_domain_constraints(contracts, &extracted)
        .into_iter()
        .filter(|finding| finding.severity == DiagnosticSeverity::Blocking)
        .collect()
}

// ─── Step 3: context assembly ────────────────────────────────────────────────

/// Default token budget for the architecture node's assembled context.
///
/// No per-node budget configuration is threaded through `execute` today (the
/// same cross-cutting gap as `AlignmentConfig`/`ModelConfig`); this generous
/// fixed default is large enough that none of the small context items used in
/// practice are ever truncated away.
const CONTEXT_TOKEN_BUDGET: u64 = 100_000;

async fn assemble_node_context(
    context: &ContextAssembler,
    input: &NodeInput,
    classification: &ClassificationResult,
    contracts: &[InterfaceDefinition],
) -> ContextPackage {
    let request = ContextAssemblyRequest {
        node_type: NodeType::Llm,
        sub_work_item_description: format!(
            "Produce the architecture document for a {:?} task (estimated scope {}).",
            classification.task_type, classification.estimated_scope
        ),
        affected_modules: classification.affected_modules.clone(),
        scenario_holdout_dirs: vec![],
        pipeline_working_dir: PathBuf::new(),
    };
    pipeline::assemble_context(
        &request,
        context.summary_cache(),
        &input.loaded_context_packs,
        contracts,
        TokenCount::new(CONTEXT_TOKEN_BUDGET),
    )
    .await
}

// ─── Step 4: LLM calls ────────────────────────────────────────────────────────

fn architecture_output_schema() -> OutputSchema {
    crate::literal_output_schema(serde_json::json!({
        "type": "object",
        "properties": { "architecture_document": { "type": "string" } },
        "required": ["architecture_document"]
    }))
}

fn semantic_alignment_output_schema() -> OutputSchema {
    crate::literal_output_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "aligned": { "type": "boolean" },
            "score": { "type": "number" },
            "findings": { "type": "array" },
            "traceability_entries": { "type": "array" }
        },
        "required": ["aligned"]
    }))
}

/// Placeholder model configuration for both LLM calls made by this node.
///
/// No model-selection configuration is threaded through `execute` today (the
/// same cross-cutting gap noted for `AlignmentConfig`); ASSERT-ALIGN-012's
/// different-model-for-critic requirement is therefore not independently
/// testable at this boundary and is intentionally omitted (see
/// `architecture_tests.rs` module doc, spec-gap #6).
fn node_model_config() -> ModelConfig {
    ModelConfig {
        model_id: "architecture-node-default".to_string(),
        context_window_size: 200_000,
        max_output_tokens: 8_192,
        input_cost_per_token: 0.000_003,
        output_cost_per_token: 0.000_015,
    }
}

async fn call_architecture_llm(
    gateway: &LlmGateway,
    rules: &ConstitutionalRules,
    system_prompt: &str,
    context_package: &ContextPackage,
    schema: &OutputSchema,
    model: &ModelConfig,
) -> Result<StructuredResponse, NodeFailure> {
    let prompt = assemble_constitutional_prompt(rules, system_prompt)
        .map_err(|err| constitutional_failure(&err))?;
    gateway
        .call(prompt, context_package, schema, model)
        .await
        .map_err(|err| llm_failure(&err))
}

#[allow(clippy::cast_precision_loss)] // token counts are far below f64's exact-integer limit
fn response_cost(response: &StructuredResponse, model: &ModelConfig) -> TokenCost {
    let input_cost = response.input_tokens().as_u64() as f64 * model.input_cost_per_token;
    let output_cost = response.output_tokens().as_u64() as f64 * model.output_cost_per_token;
    TokenCost::new(input_cost + output_cost).unwrap_or_else(TokenCost::zero)
}

/// Extracts `architecture_document` from the generation response.
///
/// Returns an empty string (never fabricates content) if the field is absent
/// or not a string — a malformed or schema-mismatched response must not be
/// reported as a successfully populated `architecture_doc`.
///
/// In practice this fallback is defensive-only, not reachable through a
/// conformant provider: `LlmProvider::complete`'s contract (`pipeline::tools`)
/// guarantees that an `Ok(StructuredResponse)` already satisfies `schema`
/// (here, `architecture_output_schema()`'s `required: ["architecture_document"]`
/// with `type: "string"`), so a non-conformant provider response would
/// surface as `Err(LlmError::SchemaValidationFailed)` before reaching this
/// function. A schema-valid *empty* string (the LLM legitimately returning
/// `""`) is a distinct, allowed case this function intentionally passes
/// through rather than treating as an error.
fn extract_architecture_document(response: &StructuredResponse) -> String {
    response
        .content()
        .get("architecture_document")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

// ─── Step 5: deterministic alignment ─────────────────────────────────────────

/// Node-internal declared inputs for `run_deterministic_alignment`.
///
/// `DeclaredNodeInputs` is not parameterized through `execute` (see
/// `architecture_tests.rs` module doc, spec-gap #9), so the required output
/// slots, approved scope, and protected paths are fixed constants here.
fn declared_node_inputs() -> DeclaredNodeInputs {
    let scope_params = ScopeParameters {
        max_file_changes: None,
        allowed_artifact_patterns: vec!["**".to_string()],
        prohibited_artifact_patterns: vec![],
        max_new_files: u32::MAX,
    };
    DeclaredNodeInputs {
        required_output_slots: vec![
            ARCHITECTURE_DOC_SLOT.to_string(),
            TRACEABILITY_MATRIX_SLOT.to_string(),
        ],
        approved_scope: ApprovedScope::from_scope_parameters(&scope_params),
        protected_paths: vec![],
    }
}

fn build_output(
    architecture_doc: &str,
    matrix: &TraceabilityMatrix,
    cost_delta: TokenCost,
) -> NodeOutput {
    let mut artifacts = HashMap::new();
    artifacts.insert(
        ARCHITECTURE_DOC_SLOT.to_string(),
        serde_json::Value::String(architecture_doc.to_string()),
    );
    artifacts.insert(
        TRACEABILITY_MATRIX_SLOT.to_string(),
        matrix_to_value(matrix),
    );
    NodeOutput {
        artifacts,
        cost_delta,
        state_updates: vec![],
    }
}

fn matrix_to_value(matrix: &TraceabilityMatrix) -> serde_json::Value {
    serde_json::to_value(matrix).unwrap_or_else(|err| {
        tracing::error!(error = %err, "failed to serialise TraceabilityMatrix; using null placeholder");
        serde_json::Value::Null
    })
}

// ─── Step 6: conditional LLM-semantic alignment ─────────────────────────────

/// Runs the LLM-semantic alignment check and, on success, folds its
/// traceability entries into `matrix`.
///
/// Only called when `ClassificationResult::safety_affecting` is `true` — see
/// the module-level spec-gap notes: there is no `AlignmentConfig` parameter
/// on `execute`, so safety-affecting status is the only available trigger for
/// the "conditional" semantic check described in the node spec's step 6.
async fn maybe_run_semantic_alignment(
    gateway: &LlmGateway,
    rules: &ConstitutionalRules,
    model: &ModelConfig,
    base_context: &ContextPackage,
    architecture_doc: &str,
    matrix: TraceabilityMatrix,
) -> Result<(TraceabilityMatrix, TokenCost), NodeFailure> {
    let semantic_context = semantic_alignment_context(base_context, architecture_doc);
    let schema = semantic_alignment_output_schema();
    let response = call_architecture_llm(
        gateway,
        rules,
        SEMANTIC_ALIGNMENT_SYSTEM_PROMPT,
        &semantic_context,
        &schema,
        model,
    )
    .await?;

    let cost = response_cost(&response, model);
    let outcome = parse_semantic_response(&response);
    if !outcome.aligned {
        return Err(semantic_alignment_failure(&outcome.findings));
    }

    let updated = update_traceability_matrix(
        matrix,
        TraceabilityStage::Architecture,
        &outcome.traceability_entries,
    );
    Ok((updated, cost))
}

/// Builds the semantic-alignment call's context: the original assembled
/// context plus a synthesised item carrying the just-generated architecture
/// document, so the critic call's context genuinely includes the
/// deterministic stage's output (ASSERT-ALIGN-010 proxy).
fn semantic_alignment_context(base: &ContextPackage, architecture_doc: &str) -> ContextPackage {
    let content = format!("Generated architecture document under review:\n\n{architecture_doc}");
    let token_count = TokenCount::new(estimate_token_len(&content));
    let mut items = base.items.clone();
    items.push(ContextItem {
        content,
        summary_level: SummaryLevel::Source,
        priority: ContextPriority::DirectDependencyOutput,
        token_count,
        required: true,
        source_path: None,
    });
    ContextPackage {
        items,
        total_token_count: base.total_token_count + token_count,
        truncation_applied: base.truncation_applied,
        assembly_errors: base.assembly_errors.clone(),
    }
}

/// Approximates a token count for `content` (~4 bytes per token), mirroring
/// `pipeline::context`'s own private estimator since that function is not
/// exported for reuse outside the `pipeline` crate.
fn estimate_token_len(content: &str) -> u64 {
    let len = u64::try_from(content.len()).unwrap_or(u64::MAX);
    len / 4 + 1
}

struct SemanticOutcome {
    aligned: bool,
    traceability_entries: Vec<TraceabilityEntry>,
    findings: Vec<AlignmentFinding>,
}

/// Parses the semantic-alignment response, failing closed (`aligned = false`,
/// empty entries/findings) on any malformed or missing field rather than
/// panicking or fabricating a pass.
fn parse_semantic_response(response: &StructuredResponse) -> SemanticOutcome {
    let content = response.content();
    let aligned = content
        .get("aligned")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let traceability_entries = content
        .get("traceability_entries")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(parse_traceability_entry)
                .collect()
        })
        .unwrap_or_default();
    let findings = content
        .get("findings")
        .and_then(serde_json::Value::as_array)
        .map(|entries| entries.iter().filter_map(parse_alignment_finding).collect())
        .unwrap_or_default();
    SemanticOutcome {
        aligned,
        traceability_entries,
        findings,
    }
}

/// Reads a string-valued field from a JSON object, returning `None` if the
/// field is absent or not a string.
///
/// Shared by every `parse_*` helper below that extracts a required string
/// field from an LLM response's parsed JSON value — the fail-skip style
/// (`None` propagates via `?` to skip the malformed entry, never panics or
/// fabricates a value) is otherwise repeated verbatim at each call site.
fn get_str_field<'a>(value: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    value.get(field)?.as_str()
}

fn parse_traceability_entry(value: &serde_json::Value) -> Option<TraceabilityEntry> {
    let requirement_ref = get_str_field(value, "requirement_ref")?.to_string();
    let output_ref = get_str_field(value, "output_ref")?.to_string();
    Some(TraceabilityEntry {
        requirement_ref,
        output_ref,
    })
}

/// Parses a single `findings` array entry from the semantic-alignment critic
/// response into an [`AlignmentFinding`] (Fix H3: embed full finding detail,
/// not just a count, in failure messages). Returns `None` (skip, don't
/// panic or fabricate) on any missing or unrecognised field — mirrors
/// [`parse_traceability_entry`]'s fail-skip style.
fn parse_alignment_finding(value: &serde_json::Value) -> Option<AlignmentFinding> {
    let check_type = parse_alignment_check_type(get_str_field(value, "check_type")?)?;
    let misalignment = parse_misalignment_type(get_str_field(value, "misalignment")?)?;
    let description = get_str_field(value, "description")?.to_string();
    let blocking = value.get("blocking")?.as_bool()?;
    Some(AlignmentFinding {
        check_type,
        misalignment,
        description,
        blocking,
    })
}

fn parse_alignment_check_type(value: &str) -> Option<AlignmentCheckType> {
    match value {
        "deterministic" => Some(AlignmentCheckType::Deterministic),
        "llm_semantic" => Some(AlignmentCheckType::LlmSemantic),
        _ => None,
    }
}

fn parse_misalignment_type(value: &str) -> Option<MisalignmentType> {
    match value {
        "missing" => Some(MisalignmentType::Missing),
        "extra" => Some(MisalignmentType::Extra),
        "modified" => Some(MisalignmentType::Modified),
        "ambiguous" => Some(MisalignmentType::Ambiguous),
        "scope_exceeded" => Some(MisalignmentType::ScopeExceeded),
        _ => None,
    }
}

// ─── Failure constructors ────────────────────────────────────────────────────

fn node_id() -> NodeId {
    NodeId::new("architecture").unwrap_or_else(|| unreachable!("non-empty literal"))
}

fn node_failed(error: String, retry_policy: RetryPolicy) -> NodeFailure {
    NodeFailure {
        error: PipelineError::NodeFailed {
            node_id: node_id(),
            error,
            retry_policy: retry_policy.clone(),
        },
        retry_policy,
    }
}

fn missing_artifact_failure(slot: &str) -> NodeFailure {
    node_failed(
        format!("required input artifact slot '{slot}' is missing or malformed"),
        RetryPolicy::NonRetryable,
    )
}

fn registry_failure(err: &RegistryError) -> NodeFailure {
    node_failed(
        format!("interface registry load failed: {err}"),
        RetryPolicy::NonRetryable,
    )
}

fn cross_domain_failure(findings: &[ConstraintFinding]) -> NodeFailure {
    node_failed(
        format!(
            "cross-domain constraint validation found {} blocking violation(s): {}",
            findings.len(),
            findings_detail(findings)
        ),
        RetryPolicy::NonRetryable,
    )
}

fn alignment_failure(findings: &[AlignmentFinding]) -> NodeFailure {
    node_failed(
        format!(
            "deterministic alignment check found {} blocking finding(s): {}",
            findings.len(),
            findings_detail(findings)
        ),
        RetryPolicy::NonRetryable,
    )
}

fn semantic_alignment_failure(findings: &[AlignmentFinding]) -> NodeFailure {
    node_failed(
        format!(
            "LLM-semantic alignment check reported the architecture document as not aligned: {}",
            findings_detail(findings)
        ),
        RetryPolicy::NonRetryable,
    )
}

/// Serialises `findings` in full for inclusion in a `NodeFailed.error`
/// message (Fix H3, task #59 security follow-up): a human or automated
/// retry/rework loop reading the error text needs the actual finding
/// detail, not merely a count. Falls back to a human-readable placeholder
/// (never panics) if serialisation itself fails.
fn findings_detail<T: serde::Serialize>(findings: &[T]) -> String {
    serde_json::to_string(findings).unwrap_or_else(|err| {
        tracing::error!(error = %err, "failed to serialise findings for error message");
        format!("{} finding(s) (serialisation failed)", findings.len())
    })
}

fn llm_failure(err: &LlmError) -> NodeFailure {
    let retry_policy = err.retry_policy();
    node_failed(format!("LLM call failed: {err}"), retry_policy)
}

fn constitutional_failure(err: &ConstitutionalError) -> NodeFailure {
    NodeFailure {
        error: PipelineError::ConstitutionalRulesLoadFailed {
            message: err.to_string(),
        },
        retry_policy: RetryPolicy::NonRetryable,
    }
}

#[cfg(test)]
#[path = "architecture_tests.rs"]
mod tests;
