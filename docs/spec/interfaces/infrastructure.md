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
  lib.rs          ModelGatewayConfig, ModelGatewayProvider, LlmProvider impl
```

### ModelGatewayProvider

```rust
pub struct ModelGatewayProvider {
    config: ModelGatewayConfig,
    client: reqwest::Client,
}

pub struct ModelGatewayConfig {
    /// The deployment's model gateway endpoint.
    pub endpoint: String,
    /// Logical model name, resolved by the gateway to a concrete provider model.
    pub model: String,
    pub max_retries: u32,
    pub request_timeout_secs: u64,
}
```

The `reqwest::Client` is initialised once at construction and reused across calls
(connection pooling), built with `rustls-tls`; no native TLS dependency.

**No API key field, and no provider name.** The gateway holds the provider
credentials and decides which concrete model a logical name resolves to. This
crate cannot reach a model provider directly even if misconfigured, because it
has nothing to authenticate with. See `docs/spec/constraints.md` §Security.

**No rate-limit state.** The gateway holds the credentials and sees all factory
traffic, so it is the only component positioned to enforce provider rate limits. A
gateway response indicating throttling is surfaced as a retriable error; this
crate tracks no window and no backoff state of its own.

**Scope**: CogWorks' own deterministic helper calls. An agent turn runs inside a
stage attempt and reaches the gateway through the stage runner, not through this
crate.

### LlmProvider::complete — Request Flow

```
1.  Build RequestBody:
      POST {config.endpoint}
      Headers:
        Content-Type: application/json
        X-CogWorks-Run: {run_id}        (for gateway-side attribution)
      Body:
        { "model": config.model,
          "system": <assembled system prompt>,
          "messages": [...],
          "max_tokens": ... }

2.  Send with `config.request_timeout_secs` timeout.

3.  Map the response:
      2xx              → parse content + token usage
      429              → RateLimited { retry_after }  (retriable)
      5xx              → GatewayUnavailable            (retriable)
      4xx (other)      → InvalidRequest                (non-retriable)

4.  Retry retriable classes up to `config.max_retries` with exponential
    backoff and jitter.

5.  Return the structured response plus its token usage for budget accounting.
```

**Redaction**: no request or response body is logged at any level. Prompts carry
work-item content, and responses carry generated artifacts; neither belongs in a
log. Spans record token counts, latency and outcome class only.

### Multi-Provider Extension Point

Provider selection is the gateway's concern, not CogWorks'. A deployment that
wants a different model provider configures its gateway; nothing in CogWorks
changes, and no provider name appears anywhere in this codebase.

`LlmProvider` remains a trait so the provider can be substituted in tests with a
double, not so CogWorks can carry several provider implementations.

---

## Section 3 — `extension-api` Crate

### Module Layout

```
crates/extension-api/src/
  lib.rs            ExtensionApiClient + DomainServiceClient impl
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
| `simulate` | `"simulate"` | `{ "filter": {...} }` |
| `validate_deps` | `"validate_deps"` | `{ "artifact_paths": [...] }` |
| `extract_interfaces` | `"extract_interfaces"` | `{ "artifact_paths": [...] }` |
| `dependency_graph` | `"dependency_graph"` | `{ "artifact_paths": [...] }` |
| `health_check` | `"health_check"` | `{}` |

All domain-service operations use this one envelope format. A domain service
exposes only the operations its handshake declares, and CogWorks routes to it on
that basis rather than on static configuration.

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

**Authentication**: the Unix domain socket transport is the default, and file
system permissions on the socket provide its access control. The HTTP transport
MUST only be used where the network path between CogWorks and the domain service
lies inside the deployment's trust boundary.

Its design MUST NOT preclude adding an authentication mechanism — bearer token or
mutual TLS — without a breaking change. The mechanism is selected when a domain
service genuinely needs to run across a network boundary, and recorded as its own
decision at that point.

The factory's deny-by-default egress model pushes toward the socket case anyway: a
sandbox reaches only the model gateway and a curated dependency proxy, so a domain
service on a remote network boundary is the exception rather than the norm.

---

## Section 4 — `listener` Crate

### Module Layout

```
crates/listener/src/
  lib.rs        module declarations, re-exports
  webhook.rs    GitHubWebhookEventSource
  queue.rs      QueueEventSource
```

### Event ingress is not in this crate

CogWorks does not receive GitHub webhooks. Webhook ingress, HMAC verification and
normalisation belong to the factory's webhook ingress service, which routes
normalised events onto durable queues. There is one place in the factory where an
inbound signature is checked, and it is not here.

This crate therefore contains no HTTP server, no HMAC verification and no
development proxy. It contains a queue consumer.

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
    /// L1: synthesise a single event from --issue and call run_step once,
    /// then exit.
    SingleShot { issue: u64 },
    /// L2: consume the factory's event queue and loop on incoming messages.
    Queue(pipeline::QueueEventConfig),
}
```

### CogWorksConfig

```rust
pub struct CogWorksConfig {
    pub trigger_mode:          TriggerMode,
    pub repository:            pipeline::RepositoryId,
    /// Operate against a harness version other than the one the repository pins.
    pub pin_override:          Option<pipeline::HarnessVersion>,
    pub otlp_endpoint:         String,
    /// The deployment's model gateway endpoint. Holds no credential.
    pub model_gateway_url:     String,
    /// Logical model name the gateway resolves to a concrete provider model.
    pub llm_model_id:          String,
    pub stage_runner_url:      String,
    pub mount_plan_url:        String,
    /// Identity of the gate service whose check runs are the only ones trusted.
    pub gate_app_id:           u64,
    pub harness_source:        String,
    pub domain_services:       Vec<DomainServiceRegistration>,
    pub labels:                LabelMappings,
    pub max_concurrent_runs:   u32,
}
```

This struct describes **the host CogWorks runs on**, not the repository it is
working on. There is no working directory (attempts run in sandboxes CogWorks
does not own), no pipeline name (graphs resolve per tier and discipline from the
harness), and no approved branch for constitutional rules (rules resolve from the
pin, so a repository cannot supply the rules governing its own runs).

**Security**: credentials are resolved through the credential-provider interface.
Raw key material is not a `CogWorksConfig` field and must never appear in a
context bundle, a prompt, a tracing event, an audit record, a log, a generated
artifact, or an error message.

There is **no model API key field and no push-capable token field**, in this
struct or anywhere else in the process. The model gateway holds model
credentials; the stage runner's identity roles hold push credentials.

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
     Assert the resolved GitHub credential carries no contents:write and no
     checks:write. A credential wider than the process needs is a configuration
     error, and failing here is cheaper than discovering it from an audit log.
4. Construct GithubClient and ModelGatewayProvider.
     The gateway provider takes an endpoint and no credential.
5. Construct ExtensionApiClients from config.domain_services (one per service):
       clients: HashMap<DomainServiceName, Arc<dyn DomainServiceClient>>
6. Construct service registry and select primary:
       domain_svc = clients[primary_service_name].clone()
7. Construct the factory-facing adapters:
       harness      = PinnedHarnessLoader::new(config.harness_source)
       stage_runner = StageRunnerClient::new(config.stage_runner_url)
       check_runs   = GithubCheckRunReader::new(github.clone(), config.gate_app_id)
       mount_plans  = MountPlanClient::new(config.mount_plan_url)
       risk_map     = GithubRiskMapLoader::new(github.clone())
       attestations = AttestationProvider::new(credentials.signing_handle())
8. Construct in-process impls for the remaining traits:
       interface_registry
9. Assemble PipelineExecutor:
       executor = PipelineExecutor::new(
           github.clone(),       // IssueTracker
           github.clone(),       // PullRequestManager
           github.clone(),       // CodeRepository      [read-only]
           domain_svc,           // DomainServiceClient
           provider,             // LlmProvider         [helper calls only]
           github.clone(),       // AuditStore
           interface_registry,
           harness,              // HarnessLoader
           stage_runner,         // StageRunner
           check_runs,           // CheckRunReader
           mount_plans,          // MountPlanProvider
           risk_map,             // RiskMapLoader
           attestations,         // AttestationProvider
       )
10. Select trigger mode and enter trigger loop.
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

**Template location**: templates are resolved from the pinned harness through
`HarnessLoader`, not from the target repository. Each is a Handlebars file, and
the harness's template manifest declares the required variables per template
name.

**Scope**: these templates render CogWorks' own deterministic helper prompts —
alignment verification, classification, failure interpretation. An agent turn's
prompt is assembled inside the sandbox from the resolved profile, so this engine
never renders it. Registering a template for an agent stage would be a defect.

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

### HarnessLoader — `PinnedHarnessLoader`

Concrete adapter that implements `pipeline::HarnessLoader`. Wired in `cli`.

**Responsibility**: read `.factory/pin.yaml` from the target repository, resolve
the harness at that version, verify the resolution, and parse its contents — the
stage-type registry, stage graphs, agent profiles, stage contracts, Context
Packs, prompt templates, output schemas and constitutional rules.

**Resolution is content-addressed** where the configured source supports it, so a
resolved version cannot change underneath a run.

**Read-only.** The adapter exposes no write path. CogWorks never writes to the
harness, and a harness source configured with write credentials is a
misconfiguration.

**Caching**: a resolved harness is cached for the duration of a run, keyed by the
resolved version. This is what makes the version stable across the steps of one
run rather than merely likely to be.

**Failure modes**, all of which halt rather than fall back:

| Condition | Mapped to |
|-----------|-----------|
| Pin file absent or malformed | `HarnessError::PinUnreadable` |
| Pin names a version that does not resolve | `HarnessError::VersionNotFound` |
| Resolved content fails verification | `HarnessError::VerificationFailed` |
| Harness source unreachable | `HarnessError::SourceUnavailable` (retriable) |

There is deliberately no default harness and no built-in fallback registry. A run
that cannot establish what governs it does not run.

### CheckRunReader — `GithubCheckRunReader`

Concrete adapter that implements `pipeline::CheckRunReader`. Wired in `cli`.

**Responsibility**: list check runs for a commit SHA through the GitHub Checks
API, exposing each run's authoring App identity, conclusion and structured
output.

**Author filtering happens here, not in business logic.** The adapter is
constructed with the configured gate service App identity and discards every
check run whose `app_id` does not match before returning. Business logic
therefore cannot act on a check run from an unexpected author even by mistake.

**No write path.** The adapter exposes no method to create or update a check run,
and the credential it is constructed with carries `checks:read` only.

**In-progress runs** are returned with `conclusion: None`. That is a distinct
state from any conclusion and callers must not collapse it into a failure.

### RiskMapLoader — `GithubRiskMapLoader`

Concrete adapter that implements `pipeline::RiskMapLoader`. Wired in `cli`.

**Responsibility**: read `risk-map.yaml` from the target repository through the
Contents API and parse its path-glob-to-tier entries.

**Read-only.** The risk map is human-owned; CogWorks reads it and never writes it.
The `architect` stage may propose a patch to it as part of its output, which a
human reviews and merges.

An absent or malformed map is reported as an error, never as an empty map. An
empty map would derive the lowest tier for everything, silently applying the least
rigour to work nobody has assessed.

### InterfaceRegistryLoader — `TomlInterfaceRegistryLoader`

Concrete adapter that implements `pipeline::InterfaceRegistryLoader`. Wired in `cli`.

**Storage**: Interface definitions are stored as TOML files under a directory in
the target repository's specification paths, declared in CogWorks' own process
configuration. The registry is human-authored: CogWorks reads it, and may propose
additions for human review, but never writes it autonomously.

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

### Stage Runner

The Pipeline Executor requests one stage attempt per attempt through this
provider, passing the image, context bundle, mount plan, network profile,
resource limits, identity role and branch. It requests cancellation on terminal
paths.

**One fresh sandbox per attempt.** A retry is a new attempt and therefore a new
request. No sandbox is reused across attempts.

The provider owns sandbox creation, the virtualisation privilege it requires, the
hosts it runs on, and pushing the attempt's commits under the given identity
role. CogWorks holds none of those. This specification intentionally does not
define the provider's method signatures, request wire format, or any mapping onto
a specific isolation technology.

### Mount-Plan Provider

Translates a stage's identity role and the writable paths its contract declares
into a mount plan the sandbox enforces. CogWorks passes the plan into the attempt
request and is never the only enforcement point for a write boundary.

A plan that cannot be produced for a role, or that would need to be wider than the
role may ever write, halts the stage.

### Credential Provider

Infrastructure clients obtain credentials through an interface backed by the
deployment's secret store, abstracted over any Vault or KMS so the deployment
stays cloud-neutral. The provider owns authentication and key lifecycle; this
document does not define its APIs, authentication flows, or rotation policy.

Raw key material is never returned to business logic, and never appears in a
context bundle, a prompt, an audit record, a log, or a generated artifact.

**Three credentials this provider must never be configured to resolve for this
process**: one that can push commits to a target repository, a model provider API
key, and one that can create or update a check run. Each is held by the component
that owns the corresponding responsibility — the stage runner's identity roles,
the model gateway, and the gate service.

### Attestation Provider

Verifies required upstream attestations against the content and trust policy
supplied by the pipeline, and signs CogWorks' own stage attestations. Signing
capability is resolved through the credential provider.

It returns verification outcomes or signed attestation content, never private key
material. Signed content is published through `IssueTracker::post_comment`; the
provider protocol and signature format are implementation choices, aligned with
the factory's shared signing library so every service produces the same shape.

Verification **fails closed**: a required attestation that is missing, unsigned,
signed by an unexpected identity, or mismatched against the content it describes
stops work rather than being treated as absent.
