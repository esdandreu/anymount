# Application Layer Boundaries Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce internal `domain` and `application` layers, rename
`daemon` to `service` with nested `control`, route CLI and TUI behavior
through `application`, and document the new architecture in `README.md`.

**Architecture:** Implement this refactor in three passes. First add
`domain` and `service` with compatibility seams so the crate still builds
while names move. Then add `application` use cases and switch CLI and TUI
adapters to them. Finish by removing old provider configuration traits,
cutting telemetry's CLI dependency, and updating the README architecture
section.

**Tech Stack:** Rust, `clap`, `ratatui`, `tracing`,
`opentelemetry`/`opentelemetry-otlp`, existing Cloud Filter and
FUSE/libcloudproviders adapters, unit tests plus `mise` tasks.

---

## File Structure

Planned file responsibilities:

- Create: `crates/anymount/src/domain/mod.rs`
  Re-export domain concepts owned by the new layer.
- Create: `crates/anymount/src/domain/provider.rs`
  Define `ProviderSpec`, `StorageSpec`, `TelemetrySpec`, OTLP transport,
  and domain validation helpers.
- Create: `crates/anymount/src/service/mod.rs`
  Re-export service runtime and control modules.
- Create: `crates/anymount/src/service/error.rs`
  Hold service-owned error and result types after the rename.
- Create: `crates/anymount/src/service/runtime.rs`
  Host the long-lived provider service loop.
- Create: `crates/anymount/src/service/control/mod.rs`
  Re-export control message, path, and platform transport modules.
- Create: `crates/anymount/src/service/control/messages.rs`
  Define control protocol and service runtime messages.
- Create: `crates/anymount/src/service/control/paths.rs`
  Derive provider service endpoint paths.
- Create: `crates/anymount/src/service/control/unix.rs`
  Unix socket transport for service control.
- Create: `crates/anymount/src/service/control/windows.rs`
  Windows named-pipe transport for service control.
- Create: `crates/anymount/src/application/mod.rs`
  Re-export application use-case modules.
- Create: `crates/anymount/src/application/types.rs`
  Hold shared request and response structs used by more than one use case.
- Create: `crates/anymount/src/application/connect.rs`
  Implement connect orchestration over repository, control, and launcher
  ports.
- Create: `crates/anymount/src/application/disconnect.rs`
  Implement named-service shutdown orchestration.
- Create: `crates/anymount/src/application/status.rs`
  Aggregate configured providers with service readiness.
- Create: `crates/anymount/src/application/provide.rs`
  Resolve named or inline provider specs, optional telemetry, and service
  hosting.
- Create: `crates/anymount/src/application/config.rs`
  Implement add, set, remove, list, and show operations over provider specs.
- Create: `crates/anymount/src/application/auth.rs`
  Wrap the OneDrive device flow behind application-facing requests and
  responses.
- Modify: `crates/anymount/src/config.rs`
  Convert the file-backed adapter to read and write `domain::provider`
  types instead of provider-owned config enums and traits.
- Modify: `crates/anymount/src/telemetry/mod.rs`
  Build OTLP handles from domain or application input instead of `Cli`.
- Modify: `crates/anymount/src/providers/provider.rs`
  Accept resolved provider specs and stop owning `StorageConfig`,
  `ProviderConfiguration`, and `ProvidersConfiguration`.
- Modify: `crates/anymount/src/providers/mod.rs`
  Re-export only provider adapter types and results.
- Modify: `crates/anymount/src/providers/cloudfilter/provider.rs`
  Import service control message types from the new `service` path.
- Modify: `crates/anymount/src/providers/cloudfilter/callbacks.rs`
  Import service runtime message types from the new `service` path.
- Modify: `crates/anymount/src/cli/commands/connect.rs`
  Keep argument parsing, delegate execution to `application::connect`.
- Modify: `crates/anymount/src/cli/commands/disconnect.rs`
  Keep argument parsing, delegate execution to `application::disconnect`.
- Modify: `crates/anymount/src/cli/commands/status.rs`
  Keep formatting, delegate status loading to `application::status`.
- Modify: `crates/anymount/src/cli/commands/provide.rs`
  Keep argument parsing, delegate orchestration to `application::provide`.
- Modify: `crates/anymount/src/cli/commands/config.rs`
  Keep prompts and output, delegate config mutations to `application::config`.
- Modify: `crates/anymount/src/cli/commands/auth.rs`
  Keep terminal UX, delegate auth flow to `application::auth`.
- Modify: `crates/anymount/src/cli/provider_control/mod.rs`
  Import service control types from `service::control`.
- Modify: `crates/anymount/src/cli/provider_control/provider_control_unix.rs`
  Use `service::control::unix`.
- Modify: `crates/anymount/src/cli/provider_control/provider_control_windows.rs`
  Use `service::control::windows`.
- Modify: `crates/anymount/src/cli/run.rs`
  Keep base logging setup only; stop doing provider-specific OTLP config
  lookup.
- Modify: `crates/anymount/src/tui/tui.rs`
  Replace direct CLI and auth adapter calls with application use cases.
- Modify: `crates/anymount/src/tui/error.rs`
  Wrap application-facing errors where needed.
- Modify: `crates/anymount/src/lib.rs`
  Add internal `application` and `service` modules, expose `domain`, and
  stop re-exporting provider-owned config concepts.
- Modify: `crates/anymount/src/main.rs`
  Keep the CLI entry point unchanged after module renames.
- Modify: `README.md`
  Add the short architecture section from the approved spec.
- Delete after cutover: `crates/anymount/src/daemon/`
  Remove the old module once all imports point to `service`.

## Task 1: Add the `domain::provider` model

**Files:**
- Create: `crates/anymount/src/domain/mod.rs`
- Create: `crates/anymount/src/domain/provider.rs`
- Modify: `crates/anymount/src/lib.rs`
- Test: `crates/anymount/src/domain/provider.rs`

- [ ] **Step 1: Write the failing domain tests**

```rust
#[test]
fn onedrive_spec_requires_token_material() {
    let spec = ProviderSpec {
        name: "demo".to_owned(),
        path: PathBuf::from("/mnt/demo"),
        storage: StorageSpec::OneDrive {
            root: PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
            access_token: None,
            refresh_token: None,
            client_id: None,
            token_expiry_buffer_secs: Some(60),
        },
        telemetry: TelemetrySpec::default(),
    };

    let err = spec.validate().expect_err("spec should be invalid");
    assert!(matches!(err, Error::MissingOneDriveTokenMaterial));
}

#[test]
fn local_spec_validation_accepts_path_and_root() {
    let spec = local_provider_spec("demo");
    spec.validate().expect("local spec should be valid");
}
```

- [ ] **Step 2: Run the focused test command and confirm failure**

Run:

```bash
cargo test -p anymount domain::provider::tests
```

Expected: FAIL because `domain::provider` does not exist yet.

- [ ] **Step 3: Implement the minimal domain model**

Create `domain/provider.rs` with a pure data model:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSpec {
    pub name: String,
    pub path: PathBuf,
    pub storage: StorageSpec,
    pub telemetry: TelemetrySpec,
}

impl ProviderSpec {
    pub fn validate(&self) -> Result<()> { ... }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageSpec { ... }

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TelemetrySpec { ... }
```

Expose `pub mod domain;` from `lib.rs`. Keep `application` internal-only
with `pub(crate)` visibility.

- [ ] **Step 4: Re-run the focused test command and confirm success**

Run:

```bash
cargo test -p anymount domain::provider::tests
```

Expected: PASS.

- [ ] **Step 5: Commit the domain model**

```bash
git add crates/anymount/src/domain/mod.rs \
  crates/anymount/src/domain/provider.rs \
  crates/anymount/src/lib.rs
git commit -m "refactor: add provider domain model"
```

## Task 2: Teach `config` to read and write `ProviderSpec`

**Files:**
- Modify: `crates/anymount/src/config.rs`
- Test: `crates/anymount/src/config.rs`

- [ ] **Step 1: Write the failing repository tests**

```rust
#[test]
fn write_spec_round_trips_provider_spec() {
    let (_tmp, cd) = tmp_config_dir();
    let spec = local_provider_spec("alpha");

    cd.write_spec(&spec).expect("write should work");

    let loaded = cd.read_spec("alpha").expect("read should work");
    assert_eq!(loaded, spec);
}

#[test]
fn load_all_specs_preserves_provider_names() {
    let (_tmp, cd) = tmp_config_dir();
    cd.write_spec(&local_provider_spec("alpha")).expect("write alpha");
    cd.write_spec(&local_provider_spec("beta")).expect("write beta");

    let specs = cd.load_all_specs().expect("load should work");
    let names = specs.into_iter().map(|spec| spec.name).collect::<Vec<_>>();
    assert_eq!(names, vec!["alpha".to_owned(), "beta".to_owned()]);
}
```

- [ ] **Step 2: Run the focused test command and confirm failure**

Run:

```bash
cargo test -p anymount config::tests::write_spec_round_trips_provider_spec
```

Expected: FAIL because `ConfigDir` has no `read_spec`, `write_spec`, or
`load_all_specs` methods.

- [ ] **Step 3: Implement private file DTO mapping in `config.rs`**

Keep the module name as `config`, but make the domain types canonical:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderFileConfig {
    path: PathBuf,
    storage: StorageFileConfig,
    #[serde(default)]
    telemetry: TelemetryFileConfig,
}

impl ConfigDir {
    pub fn read_spec(&self, name: &str) -> Result<ProviderSpec> { ... }
    pub fn write_spec(&self, spec: &ProviderSpec) -> Result<()> { ... }
    pub fn load_all_specs(&self) -> Result<Vec<ProviderSpec>> { ... }
}
```

Use the filename stem as `ProviderSpec.name`. Keep `StorageFileConfig` and
`TelemetryFileConfig` private to `config.rs` so `domain` does not own the
TOML representation.

- [ ] **Step 4: Re-run the focused config tests and adjacent config suite**

Run:

```bash
cargo test -p anymount config::tests
```

Expected: PASS.

- [ ] **Step 5: Commit the config adapter change**

```bash
git add crates/anymount/src/config.rs
git commit -m "refactor: map config files to provider specs"
```

## Task 3: Rename `daemon` to `service` and nest `control`

**Files:**
- Move: `crates/anymount/src/daemon/error.rs`
  -> `crates/anymount/src/service/error.rs`
- Move: `crates/anymount/src/daemon/mod.rs`
  -> `crates/anymount/src/service/mod.rs`
- Move: `crates/anymount/src/daemon/runtime.rs`
  -> `crates/anymount/src/service/runtime.rs`
- Move: `crates/anymount/src/daemon/messages.rs`
  -> `crates/anymount/src/service/control/messages.rs`
- Move: `crates/anymount/src/daemon/paths.rs`
  -> `crates/anymount/src/service/control/paths.rs`
- Move: `crates/anymount/src/daemon/control.rs`
  -> `crates/anymount/src/service/control/mod.rs`
- Move: `crates/anymount/src/daemon/control_unix.rs`
  -> `crates/anymount/src/service/control/unix.rs`
- Move: `crates/anymount/src/daemon/control_windows.rs`
  -> `crates/anymount/src/service/control/windows.rs`
- Modify: `crates/anymount/src/lib.rs`
- Modify: `crates/anymount/src/cli/error.rs`
- Modify: `crates/anymount/src/cli/provider_control/mod.rs`
- Modify: `crates/anymount/src/cli/provider_control/provider_control_unix.rs`
- Modify: `crates/anymount/src/cli/provider_control/provider_control_windows.rs`
- Modify: `crates/anymount/src/cli/commands/provide.rs`
- Modify: `crates/anymount/src/providers/provider.rs`
- Modify: `crates/anymount/src/providers/cloudfilter/provider.rs`
- Modify: `crates/anymount/src/providers/cloudfilter/callbacks.rs`
- Test: `crates/anymount/src/service/control/messages.rs`
- Test: `crates/anymount/src/service/runtime.rs`

- [ ] **Step 1: Write the failing tests at the new `service` paths**

```rust
#[test]
fn control_message_round_trips() {
    let encoded = ControlMessage::Ping.encode();
    let decoded = ControlMessage::decode(&encoded).expect("decode should work");
    assert_eq!(decoded, ControlMessage::Ping);
}

#[test]
fn service_runtime_logs_telemetry_until_shutdown() {
    let (tx, rx) = mpsc::channel();
    let logger = RecordingLogger::default();
    let mut runtime = ServiceRuntime::new(logger.clone(), rx);

    tx.send(ServiceMessage::Telemetry("opened: file.txt".into()))
        .expect("send should work");
    tx.send(ServiceMessage::Shutdown).expect("send should work");

    runtime.run().expect("runtime should succeed");
    assert_eq!(logger.entries(), vec!["opened: file.txt"]);
}
```

- [ ] **Step 2: Run the focused test command and confirm failure**

Run:

```bash
cargo test -p anymount service::control::messages::tests \
  service::runtime::tests
```

Expected: FAIL because `service` does not exist yet.

- [ ] **Step 3: Move the runtime and control code to `service`**

Use `git mv` for the mechanical rename, then update imports to the new
paths:

```rust
mod error;
pub mod control;
pub mod runtime;

pub use runtime::ServiceRuntime;
pub use error::{Error, Result};
```

Inside `service/control/mod.rs`, re-export the nested modules:

```rust
pub mod messages;
pub mod paths;
#[cfg(unix)]
pub mod unix;
#[cfg(target_os = "windows")]
pub mod windows;
```

Rename `DaemonRuntime` to `ServiceRuntime` and `DaemonMessage` to
`ServiceMessage` while updating all imports.

- [ ] **Step 4: Re-run the focused service tests and the provider control
  tests**

Run:

```bash
cargo test -p anymount service::control::messages::tests \
  service::control::paths::tests service::runtime::tests \
  cli::provider_control::tests
```

Expected: PASS.

- [ ] **Step 5: Commit the service rename**

```bash
git add crates/anymount/src/service \
  crates/anymount/src/cli/error.rs \
  crates/anymount/src/cli/provider_control \
  crates/anymount/src/cli/commands/provide.rs \
  crates/anymount/src/providers/provider.rs \
  crates/anymount/src/providers/cloudfilter/provider.rs \
  crates/anymount/src/providers/cloudfilter/callbacks.rs \
  crates/anymount/src/lib.rs
git commit -m "refactor: rename daemon module to service"
```

## Task 4: Add `application` for connect, disconnect, and status

**Files:**
- Create: `crates/anymount/src/application/mod.rs`
- Create: `crates/anymount/src/application/types.rs`
- Create: `crates/anymount/src/application/connect.rs`
- Create: `crates/anymount/src/application/disconnect.rs`
- Create: `crates/anymount/src/application/status.rs`
- Modify: `crates/anymount/src/cli/commands/connect.rs`
- Modify: `crates/anymount/src/cli/commands/disconnect.rs`
- Modify: `crates/anymount/src/cli/commands/status.rs`
- Modify: `crates/anymount/src/lib.rs`
- Test: `crates/anymount/src/application/connect.rs`
- Test: `crates/anymount/src/application/disconnect.rs`
- Test: `crates/anymount/src/application/status.rs`

- [ ] **Step 1: Write the failing application tests**

```rust
#[test]
fn connect_all_collects_failures_without_stopping_successes() {
    let app = test_connect_app()
        .with_names(["alpha", "beta"])
        .with_ready("alpha")
        .with_launch_failure("beta", "spawn failed");

    let err = app.connect_all().expect_err("connect should fail");
    assert!(err.to_string().contains("beta"));
}

#[test]
fn status_includes_not_running_entries() {
    let app = test_status_app().with_spec(local_provider_spec("demo"));

    let rows = app.list().expect("status should work");
    assert_eq!(rows[0].name, "demo");
    assert!(!rows[0].ready);
}

#[test]
fn disconnect_name_is_idempotent_when_service_is_missing() {
    let app = test_disconnect_app();
    app.disconnect_name("demo").expect("missing service should be fine");
}
```

- [ ] **Step 2: Run the focused test command and confirm failure**

Run:

```bash
cargo test -p anymount application::connect::tests \
  application::disconnect::tests application::status::tests
```

Expected: FAIL because `application` does not exist yet.

- [ ] **Step 3: Implement the first application use cases**

Create small, module-local ports instead of a large shared hierarchy:

```rust
pub trait ConnectRepository {
    fn list_names(&self) -> Result<Vec<String>, ConnectError>;
}

pub trait ServiceControl {
    fn ready(&self, provider_name: &str) -> bool;
    fn disconnect(&self, provider_name: &str) -> Result<(), String>;
}

pub trait ServiceLauncher {
    fn launch(
        &self,
        provider_name: &str,
        config_dir: &Path,
    ) -> Result<(), String>;
}
```

Add `ProviderStatusRow` to `application/types.rs`, then update CLI commands
to construct concrete adapters and delegate logic to application functions.
Keep CLI formatting and `clap` parsing where they are today.

- [ ] **Step 4: Re-run the focused application tests and the CLI command tests**

Run:

```bash
cargo test -p anymount application::connect::tests \
  application::disconnect::tests application::status::tests \
  cli::commands::connect::tests cli::commands::disconnect::tests \
  cli::commands::status::tests
```

Expected: PASS.

- [ ] **Step 5: Commit the connect/status/disconnect cutover**

```bash
git add crates/anymount/src/application \
  crates/anymount/src/cli/commands/connect.rs \
  crates/anymount/src/cli/commands/disconnect.rs \
  crates/anymount/src/cli/commands/status.rs \
  crates/anymount/src/lib.rs
git commit -m "refactor: route service control use cases through application"
```

## Task 5: Move `provide` orchestration into `application` and decouple OTLP
from `Cli`

**Files:**
- Create: `crates/anymount/src/application/provide.rs`
- Modify: `crates/anymount/src/application/mod.rs`
- Modify: `crates/anymount/src/application/types.rs`
- Modify: `crates/anymount/src/cli/commands/provide.rs`
- Modify: `crates/anymount/src/cli/run.rs`
- Modify: `crates/anymount/src/telemetry/mod.rs`
- Modify: `crates/anymount/src/service/runtime.rs`
- Test: `crates/anymount/src/application/provide.rs`
- Test: `crates/anymount/src/telemetry/mod.rs`

- [ ] **Step 1: Write the failing provide and telemetry tests**

```rust
#[test]
fn named_provide_loads_spec_and_starts_host() {
    let app = test_provide_app().with_spec(local_provider_spec("demo"));
    app.run_named("demo").expect("provide should work");
    assert_eq!(app.hosted_specs(), vec!["demo".to_owned()]);
}

#[test]
fn inline_provide_skips_repository_lookup() {
    let app = test_provide_app();
    app.run_inline(local_provider_spec("inline"))
        .expect("inline provide should work");
    assert_eq!(app.repository_reads(), 0);
}

#[test]
fn telemetry_handles_build_from_provider_spec() {
    let spec = telemetry_enabled_provider_spec("demo");
    let handles = OtelHandles::from_provider_spec(&spec)
        .expect("telemetry build should work");
    assert!(handles.is_some());
}
```

- [ ] **Step 2: Run the focused test command and confirm failure**

Run:

```bash
cargo test -p anymount application::provide::tests telemetry::tests
```

Expected: FAIL because the new application use case and
`OtelHandles::from_provider_spec` do not exist.

- [ ] **Step 3: Implement the provide use case and telemetry input change**

Add small ports in `application/provide.rs`:

```rust
pub trait ProvideRepository {
    fn read_spec(&self, name: &str) -> Result<ProviderSpec, ProvideError>;
}

pub trait TelemetryFactory {
    fn build(
        &self,
        spec: &ProviderSpec,
    ) -> Result<Option<OtelHandles>, ProvideError>;
}

pub trait ProviderRuntimeHost {
    fn run(&self, spec: ProviderSpec, telemetry: Option<OtelHandles>)
        -> Result<(), ProvideError>;
}
```

Change `telemetry::OtelHandles::try_from_cli` to
`telemetry::OtelHandles::from_provider_spec(&ProviderSpec)`. Keep `cli/run.rs`
responsible for base tracing setup only; let `application::provide` decide
whether provider-specific OTLP handles are needed.

- [ ] **Step 4: Re-run the focused provide tests and the existing provide
  command tests**

Run:

```bash
cargo test -p anymount application::provide::tests telemetry::tests \
  cli::commands::provide::tests
```

Expected: PASS.

- [ ] **Step 5: Commit the provide and telemetry cutover**

```bash
git add crates/anymount/src/application/provide.rs \
  crates/anymount/src/application/mod.rs \
  crates/anymount/src/application/types.rs \
  crates/anymount/src/cli/commands/provide.rs \
  crates/anymount/src/cli/run.rs \
  crates/anymount/src/telemetry/mod.rs \
  crates/anymount/src/service/runtime.rs
git commit -m "refactor: move provide orchestration into application"
```

## Task 6: Move config and auth use cases into `application`

**Files:**
- Create: `crates/anymount/src/application/config.rs`
- Create: `crates/anymount/src/application/auth.rs`
- Modify: `crates/anymount/src/application/mod.rs`
- Modify: `crates/anymount/src/application/types.rs`
- Modify: `crates/anymount/src/cli/commands/config.rs`
- Modify: `crates/anymount/src/cli/commands/auth.rs`
- Test: `crates/anymount/src/application/config.rs`
- Test: `crates/anymount/src/application/auth.rs`

- [ ] **Step 1: Write the failing config and auth application tests**

```rust
#[test]
fn add_rejects_duplicate_provider_names() {
    let app = test_config_app().with_existing(local_provider_spec("alpha"));
    let err = app
        .add(local_provider_spec("alpha"))
        .expect_err("add should fail");
    assert!(err.to_string().contains("already exists"));
}

#[test]
fn set_updates_storage_endpoint() {
    let mut app = test_config_app()
        .with_existing(onedrive_provider_spec("alpha"));
    app.set("alpha", "storage.endpoint", "https://example.test/v1")
        .expect("set should work");

    let spec = app.read("alpha").expect("read should work");
    assert_eq!(
        spec.onedrive_endpoint().as_deref(),
        Some("https://example.test/v1")
    );
}

#[test]
fn auth_returns_instructions_and_tokens() {
    let app = test_auth_app().with_tokens("refresh", "access");
    let started = app.start_onedrive_auth(None).expect("auth should start");

    assert!(started.message.contains("open"));
    assert_eq!(started.refresh_token.as_deref(), Some("refresh"));
}
```

- [ ] **Step 2: Run the focused test command and confirm failure**

Run:

```bash
cargo test -p anymount application::config::tests \
  application::auth::tests
```

Expected: FAIL because these use cases do not exist yet.

- [ ] **Step 3: Implement the config and auth application modules**

Keep prompts and terminal output in the CLI adapters, but move business
logic to `application`:

```rust
pub fn add<R: ConfigRepository>(
    repo: &R,
    spec: ProviderSpec,
) -> Result<(), ConfigError> { ... }
pub fn set<R: ConfigRepository>(repo: &R, name: &str, key: &str, value: &str)
    -> Result<(), ConfigError> { ... }

pub trait AuthFlow {
    fn start(
        &self,
        client_id: Option<String>,
    ) -> Result<StartedAuth, AuthError>;
}
```

`application::auth` should return data the CLI can print instead of opening
the browser or printing directly.

- [ ] **Step 4: Re-run the focused application tests and the CLI auth/config
  tests**

Run:

```bash
cargo test -p anymount application::config::tests \
  application::auth::tests cli::commands::config::tests \
  cli::commands::auth::tests
```

Expected: PASS.

- [ ] **Step 5: Commit the config and auth cutover**

```bash
git add crates/anymount/src/application/config.rs \
  crates/anymount/src/application/auth.rs \
  crates/anymount/src/application/mod.rs \
  crates/anymount/src/application/types.rs \
  crates/anymount/src/cli/commands/config.rs \
  crates/anymount/src/cli/commands/auth.rs
git commit -m "refactor: route config and auth through application"
```

## Task 7: Cut the TUI over to `application`

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`
- Modify: `crates/anymount/src/tui/error.rs`
- Test: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Write the failing TUI delegation tests**

```rust
#[test]
fn connect_selected_provider_uses_application_connect() {
    let mut harness = TuiHarness::with_selected(local_provider_spec("alpha"));
    harness.connect_selected_provider().expect("connect should work");
    assert_eq!(harness.connected_names(), vec!["alpha".to_owned()]);
}

#[test]
fn authenticate_onedrive_updates_draft_from_application_response() {
    let mut draft = EditDraft::new_empty("demo".to_owned());
    draft.storage_type = ProviderType::OneDrive;

    let status = authenticate_onedrive(
        &mut draft,
        &FakeAuthApp::success("refresh"),
    )
        .expect("auth should work");

    assert!(status.contains("refresh token populated"));
    assert_eq!(draft.onedrive_refresh_token, "refresh".to_owned());
}
```

- [ ] **Step 2: Run the focused TUI test command and confirm failure**

Run:

```bash
cargo test -p anymount tui::tui::tests
```

Expected: FAIL because the TUI still imports `ConnectCommand` and
`OneDriveAuthorizer` directly.

- [ ] **Step 3: Replace direct CLI and auth adapter calls with application
  calls**

Update `tui/tui.rs` to call `application::connect`, `application::config`,
and `application::auth` through small helper adapters. Remove imports of
`ConnectCommand`, `DefaultProviderProcessSupervisor`, and
`OneDriveAuthorizer`.

Prefer helper functions shaped like:

```rust
fn run_connect(app: &impl ConnectUseCase, name: Option<String>, all: bool)
    -> Result<()> { ... }

fn authenticate_onedrive(app: &impl AuthUseCase, draft: &mut EditDraft)
    -> Result<String> { ... }
```

- [ ] **Step 4: Re-run the focused TUI tests and adjacent config/auth tests**

Run:

```bash
cargo test -p anymount tui::tui::tests cli::commands::config::tests \
  cli::commands::auth::tests
```

Expected: PASS.

- [ ] **Step 5: Commit the TUI cutover**

```bash
git add crates/anymount/src/tui/tui.rs \
  crates/anymount/src/tui/error.rs
git commit -m "refactor: route tui actions through application"
```

## Task 8: Narrow `providers` to resolved `ProviderSpec` inputs

**Files:**
- Modify: `crates/anymount/src/providers/provider.rs`
- Modify: `crates/anymount/src/providers/mod.rs`
- Modify: `crates/anymount/src/application/provide.rs`
- Modify: `crates/anymount/src/config.rs`
- Modify: `crates/anymount/src/lib.rs`
- Test: `crates/anymount/src/providers/provider.rs`

- [ ] **Step 1: Write the failing provider orchestration tests**

```rust
#[test]
fn storage_label_comes_from_domain_storage_spec() {
    let spec = local_provider_spec("demo");
    assert_eq!(spec.storage.label(), "local");
}

#[test]
fn connect_providers_accepts_resolved_specs() {
    let spec = local_provider_spec("demo");
    let result = connect_providers(&[spec], &NoOpLogger::default());
    assert!(result.is_ok() || matches!(result, Err(Error::NotSupported)));
}
```

- [ ] **Step 2: Run the focused provider test command and confirm failure**

Run:

```bash
cargo test -p anymount providers::provider::tests
```

Expected: FAIL because `connect_providers` still expects
`ProviderConfiguration` and `ProvidersConfiguration`.

- [ ] **Step 3: Change provider orchestration to resolved domain input**

Replace the old traits with direct domain inputs:

```rust
pub fn connect_providers(
    specs: &[ProviderSpec],
    logger: &(impl Logger + 'static),
) -> Result<Vec<Box<dyn Provider>>> { ... }
```

Use `spec.path` and `spec.storage` directly. Remove
`ProviderConfiguration`, `ProvidersConfiguration`, and provider-owned
`StorageConfig` from `providers/provider.rs`. Update `providers/mod.rs` and
`lib.rs` exports accordingly.

- [ ] **Step 4: Re-run the focused provider tests and the provide/connect
  application tests**

Run:

```bash
cargo test -p anymount providers::provider::tests \
  application::provide::tests application::connect::tests
```

Expected: PASS.

- [ ] **Step 5: Commit the provider boundary cleanup**

```bash
git add crates/anymount/src/providers/provider.rs \
  crates/anymount/src/providers/mod.rs \
  crates/anymount/src/application/provide.rs \
  crates/anymount/src/config.rs \
  crates/anymount/src/lib.rs
git commit -m "refactor: connect providers from resolved specs"
```

## Task 9: Remove the old `daemon` module, update README, and verify

**Files:**
- Delete: `crates/anymount/src/daemon/`
- Modify: `README.md`
- Modify: `crates/anymount/src/lib.rs`

- [ ] **Step 1: Write the failing documentation and import cleanup checks**

Add a short checklist to the task branch notes and verify these fail before
cleanup:

- `rg -n "crate::daemon|src/daemon|pub mod daemon"
  crates/anymount/src README.md`
- `rg -n "Architecture" README.md`

Expected: the first command still finds old references and the second does
not yet show the new architecture section.

- [ ] **Step 2: Remove the old module and add the README architecture section**

Delete `crates/anymount/src/daemon/` after all imports point to `service`.
Add a brief `Architecture` section to `README.md` that explains:

- `domain` for provider concepts and invariants
- `application` for use-case orchestration
- adapters for CLI, TUI, config, telemetry, auth, service, providers, and
  storages
- dependency direction from frontends inward

- [ ] **Step 3: Run the cleanup checks and confirm success**

Run:

```bash
rg -n "crate::daemon|src/daemon|pub mod daemon" crates/anymount/src README.md
```

Expected: no output.

Run:

```bash
rg -n "## Architecture|domain|application" README.md
```

Expected: README shows the new architecture section.

- [ ] **Step 4: Run full verification with project tasks**

Run:

```bash
mise run test
```

Expected: PASS.

Run:

```bash
mise run build
```

Expected: PASS and `target/release/anymount-cli` updated.

- [ ] **Step 5: Commit the cleanup and docs**

```bash
git add README.md crates/anymount/src/lib.rs crates/anymount/src/service
git add -u crates/anymount/src/daemon
git commit -m "refactor: finalize application layer boundaries"
```
