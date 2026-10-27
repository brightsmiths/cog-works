# Infrastructure Implementation Contracts — Interface Specification

**Architectural Layer**: Infrastructure adapters + CLI composition root
**Specification Version**: PR 10

---

## Overview

This document specifies the concrete implementation contracts for every
infrastructure crate in the CogWorks workspace. It is the authoritative
reference for the **shape** of each adapter: which SDK calls map to which
trait methods, how transports are layered, how the CLI wires everything
together, and what production concerns (HMAC validation, backoff, session
ordering) each crate owns.

All method bodies in these crates remain `todo!()` at the end of PR 10.
This document and the source stubs together constitute the complete structural
definition on which the coder phase builds.

---

## Dependency Rules (reminder)

```
            pipeline  (domain types + traits)
            /  |  \  \
           /   |   \  \
       nodes  github llm  extension-api  listener
           \   |   /  /
            \  |  /  /
              cli  (composition root)
```

- **Infrastructure crates** implement `pipeline` traits. They MUST NOT contain domain rules.
- **`cli`** is the only crate that imports all others, constructs instances, and wires them.
- **Rate limits, HMAC, backoff, connection management** are infrastructure concerns; they never leak into `pipeline` or `nodes`.

---

## Section 1 — `github` Crate

### Module Layout

```
crates/github/src/
  lib.rs          GithubClient + all trait impls
```

Sub-module splitting is deferred until crate complexity warrants it. At current
scale, a single `lib.rs` with clearly delineated `// ─── Trait ───` sections
is easier to review than multiple sub-modules.

### GithubClient

```rust
pub struct GithubClient {
    inner: Arc<github_bot_sdk::GitHubClient>,
}
```

The SDK is configured with credentials resolved by the credential-provider
interface. The Vault library behind that interface owns authentication and key
lifecycle. CogWorks does not define Vault calls, credential refresh, or rotation
details. The SDK exposes per-installation API handles and manages its documented
rate-limit behavior.

**Construction** occurs in `cli` after the credential provider resolves the
credentials required by the SDK. The concrete acquisition and key lifecycle
remain outside this interface contract.

### Trait → SDK Call Mapping

| Trait method | SDK API call | Notes |
| --- | --- | --- |
| `IssueTracker::get_issue` | `sdk.issues().get(owner, repo, number)` | |
| `IssueTracker::list_sub_issues` | *(SDK gap — see below)* | Returns `SdkCapabilityMissing` |
| `IssueTracker::create_sub_issue` | *(SDK gap)* | Returns `SdkCapabilityMissing` |
| `IssueTracker::add_typed_link` | *(SDK gap — GraphQL issueLink)* | Returns `SdkCapabilityMissing` |
| `IssueTracker::get_typed_links` | *(SDK gap — GraphQL issueLink)* | Returns `SdkCapabilityMissing` |
| `IssueTracker::get_labels` | `sdk.issues().list_labels(owner, repo, number)` | |
| `IssueTracker::add_label` | `sdk.issues().add_labels(owner, repo, number, &[name])` | |
| `IssueTracker::remove_label` | `sdk.issues().remove_label(owner, repo, number, name)` | |
| `IssueTracker::post_comment` | `sdk.issues().create_comment(owner, repo, number, body)` | |
| `IssueTracker::get_issue_state` | `sdk.issues().get(owner, repo, number)` — inspect `.state` | |
| `IssueTracker::get_milestone` | `sdk.milestones().get(owner, repo, milestone_number)` — GitHub REST `GET /repos/{owner}/{repo}/milestones/{milestone_number}` | |
| `IssueTracker::list_comments` | *(SDK gap — `issues().list_comments(owner, repo, number)`)* | Returns `SdkCapabilityMissing` |
| `IssueTracker::set_milestone` | *(SDK gap — PATCH issue .milestone)* | Returns `SdkCapabilityMissing` |
| `PullRequestManager::create_pull_request` | `sdk.pull_requests().create(owner, repo, body)` | |
| `PullRequestManager::get_pull_request` | `sdk.pull_requests().get(owner, repo, number)` | |
| `PullRequestManager::find_pull_requests` | *(SDK gap — list with filters)* | Returns `SdkCapabilityMissing` |
| `PullRequestManager::post_review_comment` | *(SDK gap — inline PR review comment)* | Returns `SdkCapabilityMissing` |
| `PullRequestManager::get_review_status` | `sdk.pull_requests().list_reviews(owner, repo, number)` | |
| `CodeRepository::read_file` | *(SDK gap — Contents API)* | Returns `SdkCapabilityMissing` |
| `CodeRepository::list_directory` | *(SDK gap — Contents API)* | Returns `SdkCapabilityMissing` |
| `CodeRepository::file_exists` | *(SDK gap — Contents API HEAD)* | Returns `SdkCapabilityMissing` |
| `CodeRepository::read_tree` | *(SDK gap — Trees API recursive)* | Returns `SdkCapabilityMissing` |
| `ProjectBoard::sync_item_status` | `sdk.projects().update_item(project_id, item_id, status)` | Non-blocking; errors are logged, not propagated |
| `ProjectBoard::sync_custom_field` | `sdk.projects().update_field_value(...)` | Non-blocking |
| `AuditStore::record_event` | `sdk.issues().create_comment(...)` (Markdown `<details>`) | Batched, flushed on timer |
| `AuditStore::write_summary` | `sdk.issues().create_comment(...)` (Markdown section) | |

### SDK Gaps

These capabilities are absent from `pvandervelde/github-bot-sdk` as of the
commit pinned in `Cargo.toml`. The affected methods return
`GitHubOperationError::SdkCapabilityMissing { capability: "<name>" }` until
the SDK additions land.

Formal SDK addition requests must be filed against
`https://github.com/pvandervelde/github-bot-sdk`:

| Capability | PR description |
| --- | --- |
| Sub-issues REST endpoint | Support `POST /repos/{owner}/{repo}/issues/{number}/sub_issues` |
| GraphQL `issueLink` mutation | Expose typed issue link create/delete/query |
| GitHub Contents API (read_file, list_directory, file_exists) | `GET /repos/{owner}/{repo}/contents/{path}` |
| GitHub Trees API recursive | `GET /repos/{owner}/{repo}/git/trees/{tree_sha}?recursive=1` |
| PATCH issue milestone | `PATCH /repos/{owner}/{repo}/issues/{number}` with `milestone` field |
| List PRs with filter params | `GET /repos/{owner}/{repo}/pulls` with state, head, base params |
| Inline PR review comment | `POST /repos/{owner}/{repo}/pulls/{pull_number}/comments` |

### Rate Limiting

Fully delegated to `github-bot-sdk`'s built-in rate-limit handling.
`GithubClient` does not implement its own back-off. When the SDK signals rate
exhaustion it returns an error; `nodes` handles this by applying
`RetryPolicy::Retryable { after: Some(retry_after) }` from the `NodeFailure`.

### Audit Store: Markdown Formatting

`AuditStore::record_event` formats each `AuditEvent` variant as a GitHub
issue comment using a collapsible `<details>` block:

```markdown
<details>
<summary>🔍 Audit: LlmCallRecord — node_id=architecture, 2026-03-27T14:23:01Z</summary>

```json
{
  "node_id": "architecture",
  "model_id": "claude-3-5-sonnet-20241022",
  "prompt_tokens": 8432,
  ...
}
```

</details>
```

Events are batched in an in-memory queue (bounded channel) and flushed
periodically (every N events or on a timer) to avoid rate-limit exhaustion
during parallel node execution. The flush timer fires every 30 seconds; the
batch limit is 10 events.

---

## Section 2 — `llm` Crate

### Module Layout

```
crates/llm/src/
  lib.rs          AnthropicConfig, AnthropicProvider, LlmProvider impl
```

### AnthropicProvider

```rust
pub struct AnthropicProvider {
    config: AnthropicConfig,
    client: reqwest::Client,
}
```

The `reqwest::Client` is initialised once at construction and reused across
calls (connection pooling). The client is built with `rustls-tls`; no
native TLS dependency.

### LlmProvider::complete — Request Flow

```
1.  Build RequestBody:
      POST https://api.anthropic.com/v1/messages
      Headers:
        anthropic-version: 2023-06-01
        x-api-key: <provider-resolved credential>          ← NEVER logged or included in model content
        content-type: application/json
      Body (JSON):
        { "model": model.model_id,
          "max_tokens": model.max_tokens,
          "system": system_prompt,
          "messages": [ { "role": "user", "content": "..." }, ... ] }

2.  Execute via self.client.post(...).json(&body).send().await.

3.  Extract rate-limit headers from response:
        x-ratelimit-requests-remaining  → u32
        x-ratelimit-requests-reset      → RFC 3339 timestamp → Instant

4.  On HTTP 200: parse response body → StructuredResponse.
    On HTTP 429: return Err(LlmError::RateLimited { retry_after }).
    On HTTP 4xx: return Err(LlmError::ProviderError { status, body }).
    On HTTP 5xx: return Err(LlmError::ProviderError { status, body }).
    On network error: return Err(LlmError::Timeout).

5.  Schema validation: validate the completion JSON against OutputSchema.
    On failure: return Err(LlmError::InvalidSchema { reason }).
```

**Security**: The provider obtains credential material through the credential-
provider interface backed by the Vault library. This document does not prescribe
the provider's API or credential lifecycle. Credential material is used only
for outbound authentication and MUST NOT be included in prompts, context,
structured events, audit records, logs, or error messages.

### Multi-Provider Extension Point

Adding a second provider (e.g. OpenAI `gpt-4o`) is additive:

```rust
// In crates/llm/src/lib.rs  (new struct, no changes to pipeline)
pub struct OpenAiProvider { ... }

#[async_trait]
impl LlmProvider for OpenAiProvider { ... }
```

`cli` selects the active provider from `LlmConfig.provider` and constructs
the appropriate concrete type.

---

## Section 3 — `extension-api` Crate

### Module Layout

```
crates/extension-api/src/
  lib.rs            ExtensionApiClient + DomainServiceClient/TwinProvisioner impls
                    TransportKind, ServiceTransportConfig, envelope types
  unix_socket.rs    UnixSocketTransport (internal transport implementation)
  http.rs           HttpTransport (internal transport implementation)
```

### JSON Envelope Protocol

All Extension API communication uses a JSON envelope format. Every request
carries:

```json
{
  "version": "1.0",
  "operation": "validate",
  "payload": { "artifact_paths": ["/path/to/file.rs"] }
}
```

Every response carries:

```json
{
  "version": "1.0",
  "status": "ok",
  "payload": { ... }
}
```

Or on error:

```json
{
  "version": "1.0",
  "status": "error",
  "error": "domain service reported build failure: ..."
}
```

The `RequestEnvelope` and `ResponseEnvelope` types are defined in `lib.rs`
as `pub(crate)` and used by both transport modules.

### ExtensionApiClient Internal Architecture

```
ExtensionApiClient
    config: ServiceTransportConfig
         └── transport: TransportKind
               ├── UnixSocket { path: PathBuf }  → unix_socket::UnixSocketTransport
               └── Http { base_url: String }     → http::HttpTransport
```

The `ExtensionApiClient` selects the transport implementation based on
`config.transport` at the start of each operation call. Both transports expose
the same internal method signature:

```rust
async fn send(
    &self,
    envelope: &RequestEnvelope,
) -> Result<ResponseEnvelope, TransportError>
```

`TransportError::ConnectionFailed` and `TransportError::Timeout` are retried
up to `config.max_retries` times with `config.retry_delay_ms` delay between
attempts. `TransportError::ResponseParseError` is not retried.

### DomainServiceClient → Operation Mapping

| Trait method | `operation` field | `payload` contents |
| --- | --- | --- |
| `handshake` | `"handshake"` | `{}` |
| `validate` | `"validate"` | `{ "artifact_paths": [...] }` |
| `normalise` | `"normalise"` | `{ "artifact_paths": [...] }` |
| `review_rules` | `"review_rules"` | `{ "artifact_paths": [...] }` |
| `simulate` | `"simulate"` | `{ "spec": {...}, "scenarios": [...] }` |
| `validate_deps` | `"validate_deps"` | `{ "artifact_paths": [...] }` |
| `extract_interfaces` | `"extract_interfaces"` | `{ "artifact_paths": [...] }` |
| `dependency_graph` | `"dependency_graph"` | `{ "artifact_paths": [...] }` |
| `health_check` | `"health_check"` | `{}` |

`TwinProvisioner` methods use the same envelope format with operations
`"start_twin"`, `"stop_twin"`, `"configure_failure_injection"`,
`"reset_twin_state"`.

### UnixSocketTransport (`unix_socket.rs`)

Internal type — not part of the public API of this crate.

```rust
pub(crate) struct UnixSocketTransport {
    path: PathBuf,
    max_retries: u32,
    retry_delay_ms: u64,
}
```

**Protocol**: Write the JSON-serialised `RequestEnvelope` as a length-prefixed
frame (4-byte big-endian length header, then UTF-8 body), then read the
length-prefixed response frame.

**Access control**: The Unix socket file's permissions are set by the domain
service process. `UnixSocketTransport` does not set permissions — it only
connects as a client. Operators configure the socket path so only the
CogWorks process user can write to it.

**Connection lifecycle**: A new `UnixStream` is opened per operation call.
Connection multiplexing within a single call is not used; keeping the
implementation simple is preferred over connection reuse at this stage.

### HttpTransport (`http.rs`)

Internal type — not part of the public API of this crate.

```rust
pub(crate) struct HttpTransport {
    base_url: String,
    client: reqwest::Client,
    max_retries: u32,
    retry_delay_ms: u64,
}
```

**Protocol**: `POST {base_url}/api/v1` with `Content-Type: application/json`,
body is the JSON-serialised `RequestEnvelope`. Response body is the
JSON-deserialised `ResponseEnvelope`.

**Authentication**: TBD. The HTTP transport is intended for remote domain
services. Authentication mechanism (mutual TLS, bearer token, or HMAC header)
is to be decided in a future ADR. Until then, the transport sends unauthenticated
requests. Do not deploy the HTTP transport on a network boundary without
adding authentication.

---

## Section 4 — `listener` Crate

### Module Layout

```
crates/listener/src/
  lib.rs        module declarations, re-exports
  webhook.rs    GitHubWebhookEventSource
  queue.rs      QueueEventSource
```

### GitHubWebhookEventSource (`webhook.rs`)

Implements `EventSource` using `github-bot-sdk`'s webhook responder.

**HMAC validation**: Every incoming HTTP POST is verified before parsing.
The `X-Hub-Signature-256` header is compared to `HMAC-SHA256(config.secret,
body)` using a **constant-time comparison** (`subtle::ConstantTimeEq` or
equivalent). Requests that fail verification return `EventSourceError::AuthError`
and are **never** parsed. The raw body is discarded.

**Security**: The webhook credential is resolved through the credential-provider
interface; it is not a field in `WebhookConfig`. The Vault library owns its
authentication and key lifecycle. The listener receives the credential only
for request authentication and MUST NOT log or expose it. Any transient
credential wrapper MUST redact its `Debug` representation.

**Event normalization**: After authentication, the adapter parses provider
events and maps configured work-item readiness into the normalized pipeline
event contract. Exact webhook event/action mappings and readiness criteria are
provider-specific and are not fixed by this specification. Unsupported events
may be ignored by the adapter.

**Internal channel**: The webhook server runs in a background task that sends
parsed events to an `mpsc` channel. `next_event` reads from the receiver side
with the specified timeout.

**smee.io for local development**: When `config.bind_address` is
`127.0.0.1:<port>`, GitHub cannot reach the service directly. Use smee.io as a
proxy:

```bash
smee --url https://smee.io/<channel-id> --port <port>
```

No code changes are required; the webhook server is unaware of the proxy.

### QueueEventSource (`queue.rs`)

Implements `EventSource` using `queue-runtime`'s `QueueClient`.

**Provider support**:

| Provider | Status |
| --- | --- |
| Azure Service Bus | Available in `queue-runtime` |
| AWS SQS | Planned in `queue-runtime`; not yet available |

**Session ordering**: When `config.use_session_ordering` is `true`, the
`WorkItemId` extracted from each message's `session_key` metadata is used to
maintain FIFO ordering for all events belonging to that work item. The
`queue-runtime` session API guarantees exclusive delivery of messages with the
same session key.

**Message lifecycle**:

1. Receive message from queue (blocking receive with timeout).
2. Deserialise message body (JSON GitHub webhook payload) → `GitHubEvent`.
3. On success: complete (acknowledge) the message, return `Ok(Some(event))`.
4. On parse failure: call `abandon_message` to trigger `queue-runtime`'s
   retry and dead-letter logic. Return `EventSourceError::ParseError`.
5. On connectivity failure: return `EventSourceError::QueueError`.
6. On timeout: return `Ok(None)`.

After `config.max_retry_attempts` failed deliveries, `queue-runtime` moves the
message to the dead-letter queue automatically; `QueueEventSource` does not
implement its own dead-letter logic.

**AWS SQS note**: AWS SQS support is planned in `pvandervelde/queue-runtime`
but not yet available. The `QueueEventConfig::provider_config` field accepts
any `serde_json::Value`; when AWS SQS support lands in `queue-runtime`, the
provider config for SQS will be documented in `queue-runtime`'s README.

---

## Section 5 — `cli` Crate

### Module Layout

```
crates/cli/src/
  main.rs     composition root: config, observability, infra construction, trigger loop
```

### TriggerMode

```rust
pub enum TriggerMode {
    /// Phase 1 CLI: synthesise a single GitHubEvent from --issue-url and
    /// call run_step once, then exit.
    SingleShot { issue_url: String },
    /// Service mode: bind a webhook HTTP server and loop on incoming events.
    Webhook(pipeline::WebhookConfig),
    /// Service mode: consume from a cloud queue and loop on incoming messages.
    Queue(pipeline::QueueEventConfig),
}
```

### CogWorksConfig

```rust
pub struct CogWorksConfig {
    pub trigger_mode:          TriggerMode,
    pub working_dir:           std::path::PathBuf,
    pub pipeline_name:         Option<pipeline::PipelineName>,
    pub approved_branch:       pipeline::BranchName,
    pub otlp_endpoint:         String,
    pub anthropic_base_url:    String,
    pub llm_model_id:          String,
}
```

**Security**: Credentials are resolved by the Vault library through the
credential-provider interface. Raw key material is not a `CogWorksConfig`
field and must never appear in prompts, context, tracing events, audit records,
logs, generated artifacts, or error messages.

### Observability Wiring

`init_observability` is called once before any other infrastructure is
constructed. It sets up a `tracing_subscriber` with two layers:

1. **JSON console layer** — writes structured JSON events to stderr.
   Controlled by `RUST_LOG` (default: `info`).
2. **OpenTelemetry OTLP layer** — exports spans and events to the OTLP
   endpoint in `CogWorksConfig.otlp_endpoint`. Format: gRPC (tonic).

All `tracing` spans and structured events emitted by `pipeline`, `nodes`,
`github`, `llm`, `extension-api`, and `listener` flow through this composite
subscriber automatically via the global `tracing` subscriber registration.

Backend (Prometheus via OTel Collector, Jaeger, Grafana Tempo, etc.) is an
OTel Collector concern — `cli` only configures the OTLP endpoint.

```rust
fn init_observability(otlp_endpoint: &str) -> anyhow::Result<opentelemetry_sdk::trace::TracerProvider>
```

> **Note**: In `opentelemetry_sdk 0.27.x`, the concrete provider struct is
> `TracerProvider` (renamed from `SdkTracerProvider` used in 0.24–0.26). The
> import is `use opentelemetry_sdk::trace::TracerProvider;`. Update if the SDK
> version in `Cargo.toml` is upgraded.

Returns the `TracerProvider` so `main` can call `.shutdown()` on clean exit.

### Trigger Loop

```
fn run_single_shot(executor, config, issue_url) -> StepResult
fn run_service(executor, config, event_source) -> !  // loops forever
```

`run_single_shot`:

1. Parse the work item ID from `issue_url`.
2. Invoke `run_step(&executor, work_item_id, &cli_config).await` directly; no synthetic `GitHubEvent` variant is required.
3. Match on `StepResult`:
   - `Completed` → loop: call `run_step` again until state machine reaches end.
   - `Gated` → print gate message and exit 0.
   - `Escalated` → print escalation reason and exit 1.
   - `Halted` → print error and exit 2.

`run_service`:

1. Loop on `event_source.next_event(Duration::from_secs(30))`.
2. `Ok(Some(event))` → dispatch to `run_step`, log result.
3. `Ok(None)` / `Err(EventSourceError::Timeout)` → continue loop.
4. `Err(other)` → log the error; for non-fatal variants (ConnectionLost,
   QueueError) apply exponential backoff and continue; for AuthError, halt.

### Composition Root Wiring Order

```
1. Parse CogWorksConfig from env / CLI flags / config file.
2. init_observability(config.otlp_endpoint).
3. Resolve infrastructure credentials through the credential-provider interface.
4. Construct GithubClient and AnthropicProvider with provider-resolved credentials.
5. Construct ExtensionApiClients from .cogworks/services.toml (one per registered service):
       clients: HashMap<DomainServiceName, Arc<dyn DomainServiceClient>>
6. Construct service registry and select primary:
       domain_svc = clients[primary_service_name].clone()
7. Construct in-process impls for knowledge/config traits (TOML readers):
       config_loader, tool_profile_store, interface_registry, summary_cache
8. Assemble PipelineExecutor:
       executor = PipelineExecutor::new(
           github.clone(),       // IssueTracker
           github.clone(),       // PullRequestManager
           github.clone(),       // CodeRepository
           domain_svc,           // DomainServiceClient
           provider,             // LlmProvider
           github.clone(),       // AuditStore
           summary_cache,
           interface_registry,
           config_loader,
           tool_profile_store,
       )
9. Select trigger mode and enter trigger loop.
```

---

## Section 6 — `nodes` Crate Addition

### HandlebarsTemplateEngine (`nodes/src/templates.rs`)

```rust
pub struct HandlebarsTemplateEngine {
    registry: handlebars::Handlebars<'static>,
    required_vars: HashMap<String, Vec<String>>,
}
```

`HandlebarsTemplateEngine` implements `pipeline::TemplateEngine`. Templates
are registered at construction time via `register_template`. The
`required_vars` map holds the declared required variable names per template
(populated from a template manifest file at startup).

**Template location**: Templates are expected in `.cogworks/templates/` in the
repository working directory. Each template is a Handlebars file (`.hbs`
extension). The manifest `.cogworks/templates/manifest.toml` lists required
variables per template name.

The `HandlebarsTemplateEngine` is an internal implementation detail of the
`nodes` crate. The `cli` composition root creates an instance and injects it
as `Arc<dyn TemplateEngine>` into the nodes that need it.

**Escaping**: `handlebars::Handlebars::new()` defaults to HTML-escaping
substituted values (`&`, `<`, `>`, `"` become entities), since the crate
assumes HTML output. `HandlebarsTemplateEngine::new()` explicitly disables
this via `register_escape_fn(handlebars::no_escape)`, so context values are
substituted literally. This is required because the engine renders GitHub
Markdown PR bodies and issue comments, where entity-encoding would corrupt
the output a human reads on GitHub — not an HTML page where escaping would
be a correctness requirement.

---

## Section 7 — CLI-Wired Adapters (`cli` crate, implements `pipeline` traits)

### SummaryCache — `GithubCommentSummaryCache`

Concrete adapter that implements `pipeline::SummaryCache`. Wired in `cli`.

**Storage**: Pyramid summaries are stored as GitHub issue comments on the
work-item issue using a structured prefix:

```
COGWORKS_SUMMARY: {"artifact_path":"<path>","level":"<L1|L2|L3|L4>","git_ref":"<SHA>","content":"..."}
```

**Struct fields** (private):

| Field | Type | Description |
|-------|------|-------------|
| `issues` | `Arc<dyn IssueTracker>` | For reading and writing summary comments |
| `work_item_id` | `WorkItemId` | The issue used as the backing store |

**Cache key**: `(ArtifactPath, SummaryLevel)`.

**Cache invalidation**: Summaries are not invalidated within a single pipeline
run. Stale summaries from previous runs are identified by checking whether the
summary's `git_ref` (stored in the comment JSON) matches the current HEAD SHA.
If mismatched, the adapter fetches and re-stores the summary.

**Construction** (in `cli`):

```rust
let summary_cache = Arc::new(GithubCommentSummaryCache::new(
    Arc::clone(&github_client),
    work_item_id,
));
```

---

### InterfaceRegistryLoader — `TomlInterfaceRegistryLoader`

Concrete adapter that implements `pipeline::InterfaceRegistryLoader`. Wired in `cli`.

**Storage**: Interface definitions are stored as TOML files under a directory
declared in `.cogworks/config.toml` (`[interfaces] registry_dir`). The default
path is `.cogworks/interfaces/`.

**File format**: Each file in the registry directory is a TOML document
representing a single `InterfaceDefinition`. The file name (minus extension) is
used as the `InterfaceId`.

**Struct fields** (private):

| Field | Type | Description |
|-------|------|-------------|
| `registry_dir` | `PathBuf` | Absolute path to the interface registry directory |

**Construction** (in `cli`):

```rust
let registry_loader = Arc::new(TomlInterfaceRegistryLoader::new(registry_dir));
```

**Error conditions**:

- `RegistryError::NotFound` — the registry directory does not exist.
- `RegistryError::ParseError` — a TOML file in the registry directory has invalid syntax.
- `RegistryError::IoError` — filesystem read failure.

---

## Reviewer Checklist (PR 10)

- [ ] Every trait method has a `todo!()` body — no partial logic
- [ ] External crate choices documented per section above
- [ ] Zero business rules in any infrastructure crate
- [ ] `GithubClient` references SDK calls in method docs
- [ ] SDK gaps return `SdkCapabilityMissing`, not `todo!()`
- [ ] HMAC validation documented as constant-time comparison
- [ ] `QueueEventSource` documents session API usage
- [ ] Credentials resolved through the credential provider and any transient credential wrapper is redacted in `Debug`; raw credentials are absent from configuration structs
- [ ] Observability wiring produces a single composite subscriber
- [ ] Composition root wiring order follows §cli above
- [ ] `HandlebarsTemplateEngine` in `nodes`, not in a separate crate

### Implementation Job Runtime Provider

The Pipeline Executor requests one ephemeral container per implementation job
through an infrastructure provider interface and requests cleanup after
success, failure, or cancellation. The provider supplies runtime-specific
operations. This interface specification intentionally does not define method
signatures, request fields, or a Docker/Kubernetes/cloud mapping.

### Credential Provider

Infrastructure clients obtain credentials through an interface backed by the
Vault library. The Vault library owns authentication and key lifecycle; the
CogWorks contract assumes the provider resolves credentials as required. This
document does not define Vault APIs, authentication flows, or rotation policy.

### Attestation Provider

The attestation provider verifies required upstream attestations against the
content and trust policy supplied by the pipeline, and signs CogWorks action
attestations. Signing capability is resolved through the credential provider.
It returns verification outcomes or signed attestation content, never private
key material. Signed content is published through `IssueTracker::post_comment`;
the provider protocol and signature format are implementation choices.
