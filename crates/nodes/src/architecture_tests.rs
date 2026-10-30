//! Adversarial test suite for `ArchitectureNode::execute` in `architecture.rs`.
//!
//! ## Phase: RED
//!
//! `ArchitectureNode::execute` is currently `todo!()`. Every test below is
//! expected to **compile** but **panic at runtime** until the Coder lands the
//! GREEN implementation.
//!
//! ## Spec gaps surfaced by this suite (see test-plan report for full detail)
//!
//! The interface contract for `ArchitectureNode::execute` (both the task
//! brief and `docs/spec/interfaces/nodes.md` §`ArchitectureNode`) leaves several
//! cross-cutting concerns unthreaded through the function signature. This
//! suite makes an explicit, documented decision for each so that it can
//! produce an executable contract; the Tech Lead / Architect should confirm
//! or correct each decision before/while the Coder implements against it.
//!
//! 1. **LLM response schema (generation call)** — no `OutputSchema` is given
//!    for the architecture-generation call. This suite fixes the contract at
//!    `{"architecture_document": "<markdown string>"}` and expects the node
//!    to copy that field verbatim into the `architecture_doc` output slot.
//! 2. **LLM response schema (semantic alignment call)** — likewise undefined.
//!    This suite fixes it at `{"aligned": bool, "score": number,
//!    "findings": [...], "traceability_entries": [{"requirement_ref":...,
//!    "output_ref":...}, ...]}`, with `aligned` as the authoritative
//!    pass/fail bit (not re-derived from `score`).
//! 3. **`AlignmentConfig` sourcing** — not a parameter of `execute`. This
//!    suite assumes `AlignmentCheckType::LlmSemantic` is enabled if and only
//!    if `ClassificationResult::safety_affecting` is `true` (satisfying
//!    ASSERT-ALIGN-005's "cannot disable for safety-critical" half, and
//!    giving the "conditional" wording in the task's step 6 a concrete,
//!    testable trigger for the non-safety half).
//! 4. **Source of the "extracted" `InterfaceMap`** for cross-domain
//!    validation — `ArchitectureNode` has no `DomainServiceClient` dependency
//!    (unlike `InterfaceDesignNode`/`CodeGenerationNode`), so it cannot call
//!    `extract_interfaces`. This suite assumes a fail-closed default: when no
//!    other source is available, the extracted map used for comparison is
//!    empty. This makes the "happy path" (empty registry contracts) and
//!    "blocking violation" (non-empty registry contracts) scenarios
//!    extraction-source-agnostic for the empty case, and fail-closed for the
//!    non-empty case.
//! 5. **`ConstitutionalRules` sourcing** — not available via `NodeInput` or
//!    the `execute` signature (this is a cross-cutting gap affecting every
//!    `*Node::execute`, not unique to this task). Tests therefore do not
//!    assert on *how* the node builds its `ConstitutionallyWrappedPrompt` —
//!    only that the mock LLM provider is reached, in the right order, the
//!    right number of times. `gateway_tests.rs` already covers
//!    `assemble_constitutional_prompt`/`LlmGateway::call` mechanics.
//! 6. **ASSERT-ALIGN-012** (different-model-for-critic requirement) is not
//!    independently testable at this boundary (no `ModelConfig`/model
//!    registry parameter is threaded through) and is omitted.
//! 7. **ASSERT-IDEM-001** ("specification PR already exists") is out of
//!    scope for `ArchitectureNode::execute`: it has no `PullRequestManager`
//!    dependency (confirmed via `crates/nodes/src/executor/runner.rs`
//!    `run_architecture`), and the 7-step sequence has no PR-existence check
//!    step. Omitted; recommend confirming this belongs to a higher-level
//!    orchestration task instead.
//! 8. **Alignment-failure signalling** — `NodeOutput` has no dedicated
//!    "alignment result" slot in the documented output table, so this suite
//!    assumes a blocking alignment finding (from either check type) causes
//!    `NodeExecutionResult::Failure`. This is the most defensible mapping
//!    given the types available, but is a judgment call.
//! 9. **`DeclaredNodeInputs`** (`required_output_slots`/`approved_scope`/
//!    `protected_paths`) is not parameterized through `execute` either, so it
//!    must be node-internal constants not independently controllable from
//!    this boundary. `run_deterministic_alignment`'s own behaviour
//!    (ASSERT-ALIGN-001's missing-slot/scope/protected-path paths) is already
//!    exhaustively covered at the pure-function level in
//!    `pipeline/src/alignment_tests.rs`; this suite does not re-derive a
//!    forced blocking-finding scenario for the deterministic check and
//!    instead verifies only that a well-formed happy-path response succeeds.
//! 10. **Context-pack loading plumbing** — the TOML file layout under
//!     `.cogworks/context-packs/` is unspecified. This suite assumes the node
//!     consumes `NodeInput.loaded_context_packs` directly for context
//!     assembly (consistent with `pipeline::assemble_context`'s
//!     `&LoadedContextPacks` parameter, and with `NodeInput`'s own field of
//!     that exact type) rather than re-deriving pack selection from
//!     `ContextPackLoader`'s raw repository access. If the real
//!     implementation re-derives packs from `pack_loader` instead, the
//!     CPACK-series tests below will need review.
//! 11. **ASSERT-ARCH-001..004** ("spec references"/"dependency changes"/
//!     "retry with error context") read as a more general description of the
//!     *same* `constraint_validator` cross-domain check covered by
//!     ASSERT-XVAL-001..004 (step 2) rather than a distinct validation pass.
//!     This suite maps ARCH-001/002/003 directly onto the cross-domain tests
//!     below. ARCH-004's "retry... error messages appended to context" loop
//!     is an executor-level behaviour (via `NodeState.current_error`/
//!     `attempt_count` on a *future* invocation) outside what a single
//!     `execute()` call can observe, so it is not separately re-tested here.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use pipeline::{
    ApiVersion, ArtifactPath, CacheError, ChatMessage, ClassificationResult, CodeRepository,
    CommitSha, ContextPack, ContextPackId, ContextPackTrigger, DirectoryEntry, DomainServiceName,
    FileContent, GitHubOperationError, InterfaceDefinition, InterfaceId, InterfaceRegistryLoader,
    LlmError, LlmProvider, LoadedContextPacks, MergedGuidance, ModelConfig, NodeOutput,
    OutputSchema, PipelineRunId, PipelineState, PyramidSummary, RepositoryId, Requirement,
    RequirementRow, RetryPolicy, SatisfactionScore, StructuredResponse, SummaryCache, SummaryLevel,
    TaskType, TokenCount, TraceabilityMatrix, TraceabilityStatus, ValidationResult, WorkItemId,
    merge_pack_guidance,
};

use crate::gateway::LlmGateway;
use crate::node::{
    ConstraintValidator, ContextAssembler, ContextPackLoader, NodeFailure, NodeInput,
};

use super::ArchitectureNode;

// ─── Fixtures ────────────────────────────────────────────────────────────────

fn work_item() -> WorkItemId {
    WorkItemId::new(42)
}

fn classification(safety_affecting: bool) -> ClassificationResult {
    ClassificationResult {
        task_type: TaskType::Feature,
        safety_affecting,
        estimated_scope: 3,
        affected_modules: vec![],
    }
}

fn requirement_row(id: &str) -> RequirementRow {
    RequirementRow {
        requirement: Requirement {
            id: id.to_string(),
            description: format!("{id} description"),
            source_work_item: work_item(),
        },
        architecture_covered: false,
        interface_covered: false,
        sub_work_item_covered: false,
        code_covered: false,
        status: TraceabilityStatus::Missing,
    }
}

fn initial_matrix(req_ids: &[&str]) -> TraceabilityMatrix {
    TraceabilityMatrix {
        rows: req_ids.iter().map(|id| requirement_row(id)).collect(),
        pipeline_run_id: PipelineRunId::new_random(),
        work_item_id: work_item(),
        complete: false,
    }
}

fn empty_packs() -> LoadedContextPacks {
    LoadedContextPacks {
        matched_packs: vec![],
        merged_guidance: MergedGuidance::default(),
        strictest_threshold: SatisfactionScore::new(0.8).expect("0.8 is valid"),
    }
}

fn pack_with_knowledge(name: &str, domain_knowledge: &str) -> ContextPack {
    ContextPack {
        id: ContextPackId::new(name).expect("non-empty pack id"),
        trigger: ContextPackTrigger {
            label_patterns: vec![],
            component_tag_patterns: vec![],
            requires_safety_critical: false,
        },
        domain_knowledge: domain_knowledge.to_string(),
        safe_patterns: vec![],
        anti_patterns: vec![],
        required_artifacts: vec![],
        scenario_threshold_override: None,
    }
}

fn packs_from(packs: Vec<ContextPack>) -> LoadedContextPacks {
    let merged_guidance = merge_pack_guidance(&packs);
    LoadedContextPacks {
        matched_packs: packs,
        merged_guidance,
        strictest_threshold: SatisfactionScore::new(0.8).expect("0.8 is valid"),
    }
}

fn node_input(
    classification: &ClassificationResult,
    matrix: &TraceabilityMatrix,
    packs: LoadedContextPacks,
) -> NodeInput {
    let mut artifacts = HashMap::new();
    artifacts.insert(
        "classification".to_string(),
        serde_json::to_value(classification).expect("ClassificationResult serialises"),
    );
    artifacts.insert(
        "traceability_matrix".to_string(),
        serde_json::to_value(matrix).expect("TraceabilityMatrix serialises"),
    );
    NodeInput {
        artifacts,
        pipeline_state: PipelineState {
            run_id: matrix.pipeline_run_id,
            node_states: HashMap::new(),
            active_parallel_branches: vec![],
            cost_accumulator: pipeline::TokenCost::zero(),
        },
        loaded_context_packs: packs,
    }
}

fn interface_definition(id: &str) -> InterfaceDefinition {
    InterfaceDefinition {
        id: InterfaceId::new(id).expect("non-empty id"),
        domain: DomainServiceName::new("test-domain").expect("non-empty"),
        schema: serde_json::json!({ "type": "object", "properties": { "x": { "type": "string" } } }),
        artifact_types: vec!["rust/rs".to_string()],
        version: ApiVersion::new(1, 0),
    }
}

fn structured_response(content: serde_json::Value) -> StructuredResponse {
    StructuredResponse::new(content, TokenCount::new(120), TokenCount::new(340), 85)
}

fn architecture_response(markdown: &str) -> StructuredResponse {
    structured_response(serde_json::json!({ "architecture_document": markdown }))
}

fn semantic_response(aligned: bool, score: f64, entries: &[(&str, &str)]) -> StructuredResponse {
    let entries_json: Vec<serde_json::Value> = entries
        .iter()
        .map(|(r, o)| serde_json::json!({ "requirement_ref": r, "output_ref": o }))
        .collect();
    structured_response(serde_json::json!({
        "aligned": aligned,
        "score": score,
        "findings": [],
        "traceability_entries": entries_json,
    }))
}

// ─── Mock: InterfaceRegistryLoader ──────────────────────────────────────────

enum RegistryBehavior {
    Success(Vec<InterfaceDefinition>),
    Failure(String),
}

struct MockRegistryLoader {
    behavior: Mutex<Option<RegistryBehavior>>,
    call_count: Arc<AtomicUsize>,
    trace: Arc<Mutex<Vec<String>>>,
}

impl MockRegistryLoader {
    fn new(behavior: RegistryBehavior, trace: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            behavior: Mutex::new(Some(behavior)),
            call_count: Arc::new(AtomicUsize::new(0)),
            trace,
        }
    }

    fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl InterfaceRegistryLoader for MockRegistryLoader {
    async fn load_definitions(
        &self,
    ) -> Result<Vec<InterfaceDefinition>, pipeline::domain_services::RegistryError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.trace
            .lock()
            .expect("trace mutex poisoned")
            .push("registry_load".to_string());
        match self
            .behavior
            .lock()
            .expect("behavior mutex poisoned")
            .take()
        {
            Some(RegistryBehavior::Success(defs)) => Ok(defs),
            Some(RegistryBehavior::Failure(message)) => {
                Err(pipeline::domain_services::RegistryError::LoadFailed {
                    path: "test-registry".to_string(),
                    message,
                })
            }
            None => Ok(vec![]),
        }
    }

    fn validate_schema(
        &self,
        _definition: &InterfaceDefinition,
    ) -> Result<ValidationResult, pipeline::domain_services::RegistryError> {
        Ok(ValidationResult::passed())
    }
}

// ─── Mock: SummaryCache ──────────────────────────────────────────────────────

struct MockSummaryCache;

#[async_trait]
impl SummaryCache for MockSummaryCache {
    async fn get_summary(
        &self,
        _path: &ArtifactPath,
        _level: SummaryLevel,
    ) -> Result<Option<PyramidSummary>, CacheError> {
        Ok(None)
    }

    async fn is_stale(
        &self,
        _path: &ArtifactPath,
        _current_sha: &CommitSha,
    ) -> Result<bool, CacheError> {
        Ok(false)
    }

    async fn invalidate(&self, _path: &ArtifactPath) -> Result<(), CacheError> {
        Ok(())
    }
}

// ─── Mock: CodeRepository ───────────────────────────────────────────────────

struct MockCodeRepository;

#[async_trait]
impl CodeRepository for MockCodeRepository {
    async fn read_file(
        &self,
        _repository: &RepositoryId,
        path: &str,
        _git_ref: &str,
    ) -> Result<FileContent, GitHubOperationError> {
        Err(GitHubOperationError::NotFound {
            resource: path.to_string(),
        })
    }

    async fn list_directory(
        &self,
        _repository: &RepositoryId,
        _path: &str,
        _git_ref: &str,
    ) -> Result<Vec<DirectoryEntry>, GitHubOperationError> {
        Ok(vec![])
    }

    async fn file_exists(
        &self,
        _repository: &RepositoryId,
        _path: &str,
        _git_ref: &str,
    ) -> Result<bool, GitHubOperationError> {
        Ok(false)
    }

    async fn read_tree(
        &self,
        _repository: &RepositoryId,
        _git_ref: &str,
    ) -> Result<Vec<DirectoryEntry>, GitHubOperationError> {
        Ok(vec![])
    }
}

// ─── Mock: LlmProvider ───────────────────────────────────────────────────────

type Captured = (String, Vec<ChatMessage>, String);

struct MockLlmProvider {
    responses: Mutex<VecDeque<Result<StructuredResponse, LlmError>>>,
    captured: Arc<Mutex<Vec<Captured>>>,
    call_count: Arc<AtomicUsize>,
    trace: Arc<Mutex<Vec<String>>>,
}

impl MockLlmProvider {
    fn new(
        responses: Vec<Result<StructuredResponse, LlmError>>,
        trace: Arc<Mutex<Vec<String>>>,
    ) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            captured: Arc::new(Mutex::new(Vec::new())),
            call_count: Arc::new(AtomicUsize::new(0)),
            trace,
        }
    }

    fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }

    fn captured(&self) -> Vec<Captured> {
        self.captured
            .lock()
            .expect("captured mutex poisoned")
            .clone()
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn complete(
        &self,
        system_prompt: &str,
        messages: &[ChatMessage],
        _schema: &OutputSchema,
        model: &ModelConfig,
    ) -> Result<StructuredResponse, LlmError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.trace
            .lock()
            .expect("trace mutex poisoned")
            .push("llm_call".to_string());
        self.captured
            .lock()
            .expect("captured mutex poisoned")
            .push((
                system_prompt.to_string(),
                messages.to_vec(),
                model.model_id.clone(),
            ));

        let mut queue = self.responses.lock().expect("responses mutex poisoned");
        match queue.pop_front() {
            Some(result) => result,
            None => Ok(architecture_response(
                "unexpected fallback call — queue exhausted",
            )),
        }
    }
}

fn captured_text(captured: &Captured) -> String {
    let mut s = captured.0.clone();
    for m in &captured.1 {
        s.push('\n');
        s.push_str(m.content());
    }
    s
}

// ─── Test harness ────────────────────────────────────────────────────────────

struct Outcome {
    result: crate::node::NodeExecutionResult,
    llm: Arc<MockLlmProvider>,
    registry: Arc<MockRegistryLoader>,
    trace: Arc<Mutex<Vec<String>>>,
}

async fn run(
    classification: ClassificationResult,
    matrix: TraceabilityMatrix,
    packs: LoadedContextPacks,
    registry_behavior: RegistryBehavior,
    llm_responses: Vec<Result<StructuredResponse, LlmError>>,
) -> Outcome {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let registry = Arc::new(MockRegistryLoader::new(
        registry_behavior,
        Arc::clone(&trace),
    ));
    let llm = Arc::new(MockLlmProvider::new(llm_responses, Arc::clone(&trace)));

    let gateway = LlmGateway::new(Arc::clone(&llm) as Arc<dyn LlmProvider>);
    let summary_cache: Arc<dyn SummaryCache> = Arc::new(MockSummaryCache);
    let context = ContextAssembler::new(summary_cache);
    let constraint_validator =
        ConstraintValidator::new(Arc::clone(&registry) as Arc<dyn InterfaceRegistryLoader>);
    let code_repo: Arc<dyn CodeRepository> = Arc::new(MockCodeRepository);
    let pack_loader = ContextPackLoader::new(Arc::clone(&code_repo));

    let input = node_input(&classification, &matrix, packs);

    let result = ArchitectureNode::execute(
        input,
        &gateway,
        &context,
        &constraint_validator,
        &pack_loader,
        code_repo.as_ref(),
    )
    .await;

    Outcome {
        result,
        llm,
        registry,
        trace,
    }
}

fn as_success(result: &crate::node::NodeExecutionResult) -> &NodeOutput {
    match result {
        crate::node::NodeExecutionResult::Success(output) => output,
        crate::node::NodeExecutionResult::Failure(NodeFailure { error, .. }) => {
            panic!("expected NodeExecutionResult::Success, got Failure({error:?})")
        }
    }
}

fn as_failure(result: &crate::node::NodeExecutionResult) -> &NodeFailure {
    match result {
        crate::node::NodeExecutionResult::Failure(failure) => failure,
        crate::node::NodeExecutionResult::Success(output) => {
            panic!("expected NodeExecutionResult::Failure, got Success({output:?})")
        }
    }
}

// ─── Tier 1: Specification tests ────────────────────────────────────────────

/// Happy path: empty registry (no cross-domain contracts to violate), a
/// single generation call succeeds, and the node writes both documented
/// output artifact slots (acceptance criterion: "architecture artifact is
/// written as the terminal output of this node").
#[tokio::test]
async fn test_execute_happy_path_returns_success_with_populated_output_artifacts() {
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![Ok(architecture_response("# Architecture\n\nSome design."))],
    )
    .await;

    let output = as_success(&outcome.result);

    let doc = output
        .artifacts
        .get("architecture_doc")
        .expect("architecture_doc slot must be populated")
        .as_str()
        .expect("architecture_doc must be a JSON string");
    assert!(!doc.is_empty(), "architecture_doc must not be empty");

    let matrix_json = output
        .artifacts
        .get("traceability_matrix")
        .expect("traceability_matrix slot must be populated");
    let _: TraceabilityMatrix = serde_json::from_value(matrix_json.clone())
        .expect("traceability_matrix must deserialise as a TraceabilityMatrix");
}

/// ASSERT-XVAL-003: constraint validation (deterministic, no tokens) must run
/// before any LLM review pass — the registry load must be recorded in the
/// trace log before the first LLM call.
#[tokio::test]
async fn test_execute_constraint_validation_runs_before_any_llm_call() {
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![Ok(architecture_response("# Architecture"))],
    )
    .await;

    as_success(&outcome.result);
    let trace = outcome.trace.lock().expect("trace mutex poisoned").clone();
    let registry_pos = trace.iter().position(|e| e == "registry_load");
    let llm_pos = trace.iter().position(|e| e == "llm_call");

    assert!(
        registry_pos.is_some(),
        "registry_load must have been recorded, got trace {trace:?}"
    );
    assert!(
        llm_pos.is_some(),
        "llm_call must have been recorded, got trace {trace:?}"
    );
    assert!(
        registry_pos < llm_pos,
        "constraint validation must run before any LLM call; trace was {trace:?}"
    );
}

/// ASSERT-ARCH-002: a registry load failure must propagate as a `Failure`
/// (not panic, not silently swallowed), and the LLM must never be reached
/// (fail before generation).
#[tokio::test]
async fn test_execute_registry_load_failure_returns_failure_and_never_calls_llm() {
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Failure("registry file unreadable".to_string()),
        vec![],
    )
    .await;

    as_failure(&outcome.result);
    assert_eq!(
        outcome.llm.call_count(),
        0,
        "the LLM provider must never be invoked when the interface registry fails to load"
    );
}

/// Second, distinct registry-failure input (different message) — proves the
/// error path is a genuine propagation, not a hardcoded single-string match.
#[tokio::test]
async fn test_execute_registry_load_failure_second_distinct_message_also_returns_failure() {
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Failure("permission denied reading registry directory".to_string()),
        vec![],
    )
    .await;

    as_failure(&outcome.result);
    assert_eq!(outcome.llm.call_count(), 0);
}

/// ASSERT-XVAL-001 / ASSERT-XVAL-004 / ASSERT-ARCH-003: a non-empty registry
/// contract set with no corresponding extracted interface (fail-closed
/// default, see spec-gap #4) produces a blocking cross-domain finding, which
/// must prevent the LLM from ever being called — the violation is caught
/// "before implementation begins".
#[tokio::test]
async fn test_execute_cross_domain_blocking_violation_blocks_before_llm_call() {
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![interface_definition("iface-contract-1")]),
        vec![Ok(architecture_response("should never be read"))],
    )
    .await;

    as_failure(&outcome.result);
    assert_eq!(
        outcome.llm.call_count(),
        0,
        "a blocking cross-domain violation must prevent the generation LLM call entirely"
    );
}

/// ASSERT-ARCH-001: a registry with zero declared contracts has nothing to
/// violate — validation passes and generation proceeds (distinct input from
/// the blocking-violation case above, same registry mechanism).
#[tokio::test]
async fn test_execute_empty_registry_contracts_allows_generation_to_proceed() {
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![Ok(architecture_response("# Architecture"))],
    )
    .await;

    as_success(&outcome.result);
    assert_eq!(outcome.llm.call_count(), 1);
}

/// LLM rate-limited (with a `retry_after` hint) must propagate as a
/// `Failure` whose `retry_policy` matches `LlmError::retry_policy()`'s own
/// mapping (`Retryable { after: Some(..) }`) — not a hardcoded
/// `NonRetryable`.
#[tokio::test]
async fn test_execute_llm_rate_limited_propagates_retryable_with_after() {
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![Err(LlmError::RateLimited {
            retry_after: Some(Duration::from_secs(30)),
        })],
    )
    .await;

    let failure = as_failure(&outcome.result);
    match &failure.retry_policy {
        RetryPolicy::Retryable { after } => {
            assert_eq!(*after, Some(Duration::from_secs(30)));
        }
        other @ RetryPolicy::NonRetryable => {
            panic!("expected Retryable{{after: Some(30s)}}, got {other:?}")
        }
    }
}

/// LLM network error must propagate as `Retryable { after: None }` — a
/// distinct `LlmError` variant from the rate-limit case, proving the mapping
/// is not special-cased to a single variant.
#[tokio::test]
async fn test_execute_llm_network_error_propagates_retryable_without_after() {
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![Err(LlmError::NetworkError {
            message: "connection reset".to_string(),
        })],
    )
    .await;

    let failure = as_failure(&outcome.result);
    assert_eq!(failure.retry_policy, RetryPolicy::Retryable { after: None });
}

/// LLM schema-validation failure must propagate as `NonRetryable` (per
/// `LlmError::retry_policy()`), and must not panic despite the error
/// carrying raw, potentially malformed `content`.
#[tokio::test]
async fn test_execute_llm_schema_validation_failure_propagates_non_retryable() {
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![Err(LlmError::SchemaValidationFailed {
            content: serde_json::json!({ "garbage": true }),
            violations: vec!["missing required field".to_string()],
        })],
    )
    .await;

    let failure = as_failure(&outcome.result);
    assert_eq!(failure.retry_policy, RetryPolicy::NonRetryable);
}

/// LLM context-window-exceeded failure must also propagate as
/// `NonRetryable` — fourth distinct `LlmError` variant exercised.
#[tokio::test]
async fn test_execute_llm_context_window_exceeded_propagates_non_retryable() {
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![Err(LlmError::ContextWindowExceeded {
            requested: TokenCount::new(500_000),
            limit: TokenCount::new(100_000),
        })],
    )
    .await;

    let failure = as_failure(&outcome.result);
    assert_eq!(failure.retry_policy, RetryPolicy::NonRetryable);
}

/// ASSERT-ALIGN-005 / ASSERT-ALIGN-002 / ASSERT-ALIGN-006: a safety-affecting
/// classification must run the LLM-semantic alignment check (second LLM
/// call) and that check's `traceability_entries` must advance the
/// traceability matrix's `architecture_covered` flag for the matching
/// requirement.
#[tokio::test]
async fn test_execute_safety_affecting_runs_semantic_alignment_and_updates_traceability() {
    let outcome = run(
        classification(true),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![
            Ok(architecture_response("# Architecture")),
            Ok(semantic_response(
                true,
                0.97,
                &[("REQ-001", "architecture.md#req-001")],
            )),
        ],
    )
    .await;

    let output = as_success(&outcome.result);
    assert_eq!(
        outcome.llm.call_count(),
        2,
        "safety-affecting work items must run both the generation call and the \
         LLM-semantic alignment call"
    );

    let matrix_json = output
        .artifacts
        .get("traceability_matrix")
        .expect("traceability_matrix slot must be populated");
    let matrix: TraceabilityMatrix =
        serde_json::from_value(matrix_json.clone()).expect("valid TraceabilityMatrix JSON");
    let row = matrix
        .rows
        .iter()
        .find(|r| r.requirement.id == "REQ-001")
        .expect("REQ-001 row must be present");
    assert!(
        row.architecture_covered,
        "REQ-001's architecture_covered flag must be set true after a matching \
         traceability entry from the LLM-semantic check"
    );
}

/// Spec-gap decision #3: a non-safety-affecting classification does not run
/// the LLM-semantic check by default — only the generation call is made, and
/// the traceability matrix's coverage flags are left unchanged (the
/// documented Traceability Update Rule: "if only Deterministic ran ... stage
/// coverage flags stay unchanged").
#[tokio::test]
async fn test_execute_non_safety_affecting_skips_semantic_alignment_leaves_traceability_uncovered()
{
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![Ok(architecture_response("# Architecture"))],
    )
    .await;

    let output = as_success(&outcome.result);
    assert_eq!(
        outcome.llm.call_count(),
        1,
        "a non-safety-affecting work item must not run the LLM-semantic check by default"
    );

    let matrix: TraceabilityMatrix = serde_json::from_value(
        output
            .artifacts
            .get("traceability_matrix")
            .expect("traceability_matrix slot must be populated")
            .clone(),
    )
    .expect("valid TraceabilityMatrix JSON");
    let row = matrix
        .rows
        .iter()
        .find(|r| r.requirement.id == "REQ-001")
        .expect("REQ-001 row must be present");
    assert!(
        !row.architecture_covered,
        "without an LLM-semantic pass, no traceability_entries exist, so coverage \
         must remain honestly uncovered — not fabricated as covered"
    );
}

/// ASSERT-ALIGN-003: a blocking semantic-alignment result (`aligned: false`)
/// must cause the node to report `Failure` (see spec-gap decision #8).
#[tokio::test]
async fn test_execute_semantic_alignment_blocking_returns_failure() {
    let outcome = run(
        classification(true),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![
            Ok(architecture_response("# Architecture")),
            Ok(semantic_response(false, 0.40, &[])),
        ],
    )
    .await;

    as_failure(&outcome.result);
}

/// ASSERT-ALIGN-011: a high score (0.99) combined with `aligned: false` must
/// still fail — the explicit `aligned` bit overrides the score, proving the
/// node does not re-derive pass/fail purely from a threshold comparison.
#[tokio::test]
async fn test_execute_semantic_alignment_high_score_but_not_aligned_still_fails() {
    let outcome = run(
        classification(true),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![
            Ok(architecture_response("# Architecture")),
            Ok(semantic_response(false, 0.99, &[])),
        ],
    )
    .await;

    as_failure(&outcome.result);
}

/// ASSERT-ALIGN-010 (proxy): the LLM-semantic alignment call's context must
/// include the content generated by the preceding architecture-generation
/// call — proving the two calls are not independent/stateless, and that
/// deterministic-stage output genuinely flows into the semantic check's
/// context.
#[tokio::test]
async fn test_execute_semantic_alignment_context_includes_generated_architecture_document() {
    const MARKER: &str = "UNIQUE_ARCHITECTURE_MARKER_0xC0FFEE";
    let outcome = run(
        classification(true),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![
            Ok(architecture_response(MARKER)),
            Ok(semantic_response(true, 0.9, &[])),
        ],
    )
    .await;

    as_success(&outcome.result);
    let captured = outcome.llm.captured();
    assert_eq!(captured.len(), 2, "exactly two LLM calls expected");
    let second_call_text = captured_text(&captured[1]);
    assert!(
        second_call_text.contains(MARKER),
        "the semantic-alignment call's context must reference the generated \
         architecture document; got: {second_call_text:?}"
    );
}

// ─── Tier 2: Adversarial / stub-killing tests ───────────────────────────────

/// A syntactically valid but schema-mismatched LLM response (no
/// `architecture_document` key) must not be silently accepted as a populated
/// architecture document, and must not panic.
#[tokio::test]
async fn test_execute_malformed_llm_response_missing_expected_key_does_not_fabricate_content() {
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![Ok(structured_response(
            serde_json::json!({ "unexpected_field": "shape" }),
        ))],
    )
    .await;

    // No panic reaching this line is itself part of the assertion.
    let has_fabricated_content = matches!(
        &outcome.result,
        crate::node::NodeExecutionResult::Success(output)
            if output
                .artifacts
                .get("architecture_doc")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty())
    );
    assert!(
        !has_fabricated_content,
        "a response missing the expected content field must not be reported as a \
         successfully populated architecture_doc"
    );
}

/// Stub-killer: the number of LLM calls must depend on `safety_affecting`,
/// not be a hardcoded constant (one trivial stub could always call once;
/// another could always call twice regardless of input).
#[tokio::test]
async fn test_execute_llm_call_count_differs_by_safety_affecting_flag() {
    let non_safety = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![Ok(architecture_response("# Architecture"))],
    )
    .await;
    as_success(&non_safety.result);

    let safety = run(
        classification(true),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![
            Ok(architecture_response("# Architecture")),
            Ok(semantic_response(true, 0.9, &[])),
        ],
    )
    .await;
    as_success(&safety.result);

    assert_eq!(non_safety.llm.call_count(), 1);
    assert_eq!(safety.llm.call_count(), 2);
    assert_ne!(
        non_safety.llm.call_count(),
        safety.llm.call_count(),
        "call count must vary with classification.safety_affecting"
    );
}

/// Stub-killer: `cost_delta` must reflect real LLM token usage, not a
/// hardcoded `TokenCost::zero()`.
#[tokio::test]
async fn test_execute_output_cost_delta_is_not_hardcoded_zero() {
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![Ok(architecture_response("# Architecture"))],
    )
    .await;

    let output = as_success(&outcome.result);
    assert!(
        output.cost_delta.as_f64() > 0.0,
        "cost_delta must be positive after a real LLM call with nonzero token \
         counts (input_tokens=120, output_tokens=340); got {}",
        output.cost_delta
    );
}

/// Adversarial: with two requirement rows, a semantic alignment result that
/// references only one of them must leave the other row's coverage
/// untouched — kills a stub that marks every row covered indiscriminately.
#[tokio::test]
async fn test_execute_traceability_matrix_preserves_unrelated_requirement_rows() {
    let outcome = run(
        classification(true),
        initial_matrix(&["REQ-001", "REQ-002"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![
            Ok(architecture_response("# Architecture")),
            Ok(semantic_response(
                true,
                0.95,
                &[("REQ-001", "architecture.md#one")],
            )),
        ],
    )
    .await;

    let output = as_success(&outcome.result);
    let matrix: TraceabilityMatrix = serde_json::from_value(
        output
            .artifacts
            .get("traceability_matrix")
            .expect("traceability_matrix slot must be populated")
            .clone(),
    )
    .expect("valid TraceabilityMatrix JSON");

    let req1 = matrix
        .rows
        .iter()
        .find(|r| r.requirement.id == "REQ-001")
        .expect("REQ-001 present");
    let req2 = matrix
        .rows
        .iter()
        .find(|r| r.requirement.id == "REQ-002")
        .expect("REQ-002 present");

    assert!(req1.architecture_covered, "REQ-001 must be covered");
    assert!(
        !req2.architecture_covered,
        "REQ-002 must remain uncovered — only REQ-001 was referenced by the \
         semantic alignment's traceability_entries"
    );
    assert!(
        !matrix.complete,
        "matrix must not be reported complete while REQ-002 has zero coverage"
    );
}

/// ASSERT-CPACK-003 / ASSERT-CPACK-007: multiple matched context packs'
/// domain knowledge must be combined and reach the generation LLM call's
/// context.
#[tokio::test]
async fn test_execute_multiple_context_packs_domain_knowledge_reaches_generation_context() {
    let packs = packs_from(vec![
        pack_with_knowledge("pack-alpha", "PACK_ALPHA_GUIDANCE_TEXT"),
        pack_with_knowledge("pack-beta", "PACK_BETA_GUIDANCE_TEXT"),
    ]);

    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        packs,
        RegistryBehavior::Success(vec![]),
        vec![Ok(architecture_response("# Architecture"))],
    )
    .await;

    as_success(&outcome.result);
    let captured = outcome.llm.captured();
    let first_call_text = captured_text(&captured[0]);
    assert!(
        first_call_text.contains("PACK_ALPHA_GUIDANCE_TEXT"),
        "pack-alpha's domain knowledge must reach the LLM context; got {first_call_text:?}"
    );
    assert!(
        first_call_text.contains("PACK_BETA_GUIDANCE_TEXT"),
        "pack-beta's domain knowledge must reach the LLM context; got {first_call_text:?}"
    );
}

/// ASSERT-CPACK-002 (differential pair with the test above): when no context
/// packs are matched, no pack guidance text appears in the LLM context.
#[tokio::test]
async fn test_execute_without_context_packs_no_pack_guidance_text_reaches_context() {
    let outcome = run(
        classification(false),
        initial_matrix(&["REQ-001"]),
        empty_packs(),
        RegistryBehavior::Success(vec![]),
        vec![Ok(architecture_response("# Architecture"))],
    )
    .await;

    as_success(&outcome.result);
    let captured = outcome.llm.captured();
    let first_call_text = captured_text(&captured[0]);
    assert!(
        !first_call_text.contains("PACK_ALPHA_GUIDANCE_TEXT")
            && !first_call_text.contains("PACK_BETA_GUIDANCE_TEXT"),
        "no pack guidance sentinel text may appear when no packs are matched; \
         got {first_call_text:?}"
    );
}

/// Mock fidelity check: `MockRegistryLoader::validate_schema` must behave per
/// the `InterfaceRegistryLoader` contract (an always-valid stub) so that any
/// incidental call to it from the node body does not itself cause a false
/// failure in the tests above.
#[test]
fn test_mock_registry_loader_validate_schema_reports_passed() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let loader = MockRegistryLoader::new(RegistryBehavior::Success(vec![]), trace);
    let result = loader
        .validate_schema(&interface_definition("iface-x"))
        .expect("mock validate_schema is infallible");
    assert!(result.valid());
}

// ─── Tier 3: Property-based tests ───────────────────────────────────────────

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build a current-thread tokio runtime for a proptest case")
        .block_on(fut)
}

proptest::proptest! {
    /// ASSERT-XVAL-001 / ASSERT-XVAL-004 generalised across arbitrary
    /// registry sizes: with a fail-closed empty extracted map (spec-gap #4),
    /// *any* non-empty set of registry contracts must block generation, and
    /// an empty registry must always allow it to proceed — this must hold
    /// regardless of how many contracts are declared or what their schemas
    /// contain.
    #[test]
    fn test_execute_cross_domain_gate_matches_registry_emptiness_invariant(
        contract_count in 0usize..5,
    ) {
        let contracts: Vec<InterfaceDefinition> = (0..contract_count)
            .map(|i| interface_definition(&format!("iface-{i}")))
            .collect();
        let expect_block = contract_count > 0;

        let outcome = block_on(run(
            classification(false),
            initial_matrix(&["REQ-001"]),
            empty_packs(),
            RegistryBehavior::Success(contracts),
            vec![Ok(architecture_response("# Architecture"))],
        ));

        if expect_block {
            proptest::prop_assert!(
                matches!(outcome.result, crate::node::NodeExecutionResult::Failure(_)),
                "contract_count={contract_count} > 0 must block generation, got {:?}",
                outcome.result
            );
            proptest::prop_assert_eq!(outcome.llm.call_count(), 0);
        } else {
            proptest::prop_assert!(
                matches!(outcome.result, crate::node::NodeExecutionResult::Success(_)),
                "contract_count=0 must allow generation to proceed, got {:?}",
                outcome.result
            );
            proptest::prop_assert_eq!(outcome.llm.call_count(), 1);
        }
    }

    /// ASSERT-ALIGN-006 generalised: for an arbitrary subset of requirement
    /// rows referenced by the semantic alignment's `traceability_entries`,
    /// exactly that subset's `architecture_covered` flag must become `true`
    /// in the output matrix, and no other row's flag must change.
    #[test]
    fn test_execute_traceability_coverage_matches_arbitrary_referenced_subset(
        covered_mask in proptest::collection::vec(proptest::bool::ANY, 1..6),
    ) {
        let req_ids: Vec<String> = (1..=covered_mask.len())
            .map(|i| format!("REQ-{i:03}"))
            .collect();
        let req_id_refs: Vec<&str> = req_ids.iter().map(String::as_str).collect();

        let referenced: Vec<(&str, &str)> = req_id_refs
            .iter()
            .zip(covered_mask.iter())
            .filter(|(_, covered)| **covered)
            .map(|(id, _)| (*id, "architecture.md#section"))
            .collect();

        let outcome = block_on(run(
            classification(true),
            initial_matrix(&req_id_refs),
            empty_packs(),
            RegistryBehavior::Success(vec![]),
            vec![
                Ok(architecture_response("# Architecture")),
                Ok(semantic_response(true, 0.9, &referenced)),
            ],
        ));

        let output = match &outcome.result {
            crate::node::NodeExecutionResult::Success(output) => output,
            other @ crate::node::NodeExecutionResult::Failure(_) => {
                return Err(proptest::test_runner::TestCaseError::fail(format!(
                    "expected Success, got {other:?}"
                )));
            }
        };
        let matrix: TraceabilityMatrix = serde_json::from_value(
            output
                .artifacts
                .get("traceability_matrix")
                .expect("traceability_matrix slot must be populated")
                .clone(),
        )
        .expect("valid TraceabilityMatrix JSON");

        for (id, expected_covered) in req_id_refs.iter().zip(covered_mask.iter()) {
            let row = matrix
                .rows
                .iter()
                .find(|r| r.requirement.id == *id)
                .unwrap_or_else(|| panic!("row {id} must be present"));
            proptest::prop_assert_eq!(
                row.architecture_covered,
                *expected_covered,
                "row {} coverage mismatch: expected {}, got {}",
                id,
                expected_covered,
                row.architecture_covered
            );
        }
    }

    /// Robustness / no-panic property: an arbitrary, schema-unrelated JSON
    /// object returned by the LLM for the generation call must never cause
    /// `execute` to panic — it must resolve to some `NodeExecutionResult`
    /// (Success or Failure), never an unwinding panic.
    #[test]
    fn test_execute_arbitrary_llm_response_shape_never_panics(
        keys in proptest::collection::vec("[a-zA-Z_]{1,12}", 0..5),
        values in proptest::collection::vec("[a-zA-Z0-9 _#\\-]{0,40}", 0..5),
    ) {
        let mut obj = serde_json::Map::new();
        for (k, v) in keys.into_iter().zip(values.into_iter()) {
            obj.insert(k, serde_json::Value::String(v));
        }
        let content = serde_json::Value::Object(obj);

        let outcome = block_on(run(
            classification(false),
            initial_matrix(&["REQ-001"]),
            empty_packs(),
            RegistryBehavior::Success(vec![]),
            vec![Ok(structured_response(content))],
        ));

        // Reaching this point without a panic is the property under test.
        proptest::prop_assert!(matches!(
            outcome.result,
            crate::node::NodeExecutionResult::Success(_) | crate::node::NodeExecutionResult::Failure(_)
        ));
    }
}
