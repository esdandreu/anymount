# Provider Daemon Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `connect` ensure one long-lived `provide --name <provider>`
process exists per configured provider, with daemon-owned provider sessions and
callback telemetry flowing into the daemon loop.

**Architecture:** Add a new `provide` CLI command that loads one provider by
config name, connects it, exposes a local control endpoint, and runs a daemon
message loop. Refactor `connect` into a supervisor client that probes or
launches provider daemons instead of owning provider sessions itself. Isolate
cross-platform IPC and daemon runtime concerns behind small modules so provider
implementations only need to emit telemetry.

**Tech Stack:** Rust, clap, std::sync::mpsc, Unix domain sockets on
Linux/macOS, Windows named pipes, existing provider modules, `mise` tasks for
build and test.

---

## File Structure

Planned file responsibilities:

- Modify: `crates/anymount/src/cli/cli.rs`
  Add the `Provide` subcommand and dispatch logic.
- Modify: `crates/anymount/src/cli/commands/mod.rs`
  Export the new command module.
- Modify: `crates/anymount/src/cli/commands/connect.rs`
  Replace direct provider ownership with daemon supervision logic and focused
  tests.
- Create: `crates/anymount/src/cli/commands/provide.rs`
  Define `ProvideCommand` and its command-level tests.
- Create: `crates/anymount/src/daemon/mod.rs`
  Re-export daemon runtime, IPC, and message types.
- Create: `crates/anymount/src/daemon/messages.rs`
  Define control protocol and internal telemetry/control loop message enums.
- Create: `crates/anymount/src/daemon/runtime.rs`
  Implement the provider daemon startup path and loop orchestration.
- Create: `crates/anymount/src/daemon/paths.rs`
  Map provider names to state directory and endpoint locations.
- Create: `crates/anymount/src/daemon/control.rs`
  Define a platform-agnostic client/server control transport trait and shared
  helpers.
- Create: `crates/anymount/src/daemon/control_unix.rs`
  Unix socket transport for Linux/macOS.
- Create: `crates/anymount/src/daemon/control_windows.rs`
  Windows named-pipe transport.
- Modify: `crates/anymount/src/lib.rs`
  Export the daemon module if needed by CLI and tests.
- Modify: `crates/anymount/src/providers/provider.rs`
  Add a daemon-oriented connection entry point that loads one provider and
  keeps it alive in the provider process.
- Modify: `crates/anymount/src/providers/cloudfilter/provider.rs`
  Thread daemon telemetry sender into Windows callback setup.
- Modify: `crates/anymount/src/providers/cloudfilter/callbacks.rs`
  Emit structured telemetry into the daemon loop from callback threads.
- Modify: `README.md`
  Document the new `provide` command and the changed semantics of `connect`.
- Test: `crates/anymount/src/cli/commands/connect.rs`
  Expand unit tests around reuse, launch, and partial failure behavior.
- Test: `crates/anymount/src/cli/commands/provide.rs`
  Add command parsing and startup-failure tests.
- Test: `crates/anymount/src/daemon/runtime.rs`
  Add loop and orchestration tests.
- Test: `crates/anymount/src/daemon/paths.rs`
  Add endpoint path derivation tests.
- Test: `crates/anymount/src/daemon/control.rs`
  Add protocol serialization and handshake tests.

## Task 1: Add the `provide` CLI command surface

**Files:**
- Create: `crates/anymount/src/cli/commands/provide.rs`
- Modify: `crates/anymount/src/cli/commands/mod.rs`
- Modify: `crates/anymount/src/cli/cli.rs`
- Test: `crates/anymount/src/cli/commands/provide.rs`

- [ ] **Step 1: Write the failing command parsing tests**

```rust
#[test]
fn parse_provide_name_command() {
    let cli = Cli::try_parse_from(["anymount", "provide", "--name", "demo"])
        .expect("parse should succeed");

    match cli.command.expect("command should exist") {
        Commands::Provide(cmd) => assert_eq!(cmd.name, "demo"),
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn provide_requires_name() {
    let err = Cli::try_parse_from(["anymount", "provide"]).unwrap_err();
    let rendered = err.to_string();
    assert!(rendered.contains("--name"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
mise run test -- cargo test parse_provide_name_command provide_requires_name
```

Expected: FAIL because `Commands::Provide` and `ProvideCommand` do not exist.

- [ ] **Step 3: Write the minimal implementation**

Add a new command module:

```rust
#[derive(Args, Debug, Clone)]
pub struct ProvideCommand {
    #[arg(long)]
    pub name: String,

    #[arg(long)]
    pub config_dir: Option<PathBuf>,
}
```

Wire it into the CLI:

```rust
#[derive(Subcommand)]
pub enum Commands {
    Auth(AuthCommand),
    Config(ConfigCommand),
    Connect(ConnectCommand),
    Provide(ProvideCommand),
}
```

Add a temporary `execute` stub that returns a descriptive `Err("not yet
implemented".to_owned())` so later tasks can replace it incrementally.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
mise run test -- cargo test parse_provide_name_command provide_requires_name
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/cli/cli.rs \
  crates/anymount/src/cli/commands/mod.rs \
  crates/anymount/src/cli/commands/provide.rs
git commit -m "feat: add provide command surface"
```

## Task 2: Add daemon message types and endpoint path helpers

**Files:**
- Create: `crates/anymount/src/daemon/mod.rs`
- Create: `crates/anymount/src/daemon/messages.rs`
- Create: `crates/anymount/src/daemon/paths.rs`
- Modify: `crates/anymount/src/lib.rs`
- Test: `crates/anymount/src/daemon/paths.rs`
- Test: `crates/anymount/src/daemon/messages.rs`

- [ ] **Step 1: Write the failing path and message tests**

```rust
#[test]
fn endpoint_path_is_stable_for_provider_name() {
    let a = provider_endpoint("demo").expect("path should build");
    let b = provider_endpoint("demo").expect("path should build");
    assert_eq!(a, b);
}

#[test]
fn control_message_round_trips() {
    let encoded = ControlMessage::Ping.encode();
    let decoded = ControlMessage::decode(&encoded).expect("decode should work");
    assert_eq!(decoded, ControlMessage::Ping);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
mise run test -- cargo test endpoint_path_is_stable_for_provider_name control_message_round_trips
```

Expected: FAIL because the daemon modules do not exist.

- [ ] **Step 3: Write the minimal implementation**

Add:

- `daemon::messages` with small serializable enums:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlMessage {
    Ping,
    Ready,
    Shutdown,
    Ack,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonMessage {
    Shutdown,
    Telemetry(String),
}
```

- `daemon::paths` with deterministic provider-name-to-endpoint mapping using a
  dedicated app state directory.
- `daemon::mod` re-exporting the new modules.
- `lib.rs` wiring so CLI/tests can import the daemon code.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
mise run test -- cargo test endpoint_path_is_stable_for_provider_name control_message_round_trips
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/daemon/mod.rs \
  crates/anymount/src/daemon/messages.rs \
  crates/anymount/src/daemon/paths.rs \
  crates/anymount/src/lib.rs
git commit -m "feat: add daemon message and path primitives"
```

## Task 3: Build the cross-platform control transport

**Files:**
- Create: `crates/anymount/src/daemon/control.rs`
- Create: `crates/anymount/src/daemon/control_unix.rs`
- Create: `crates/anymount/src/daemon/control_windows.rs`
- Modify: `crates/anymount/src/daemon/mod.rs`
- Test: `crates/anymount/src/daemon/control.rs`

- [ ] **Step 1: Write the failing handshake tests**

```rust
#[test]
fn client_ping_receives_ready() {
    let transport = InMemoryControlTransport::default();
    let server = transport.bind("demo").expect("bind should succeed");
    transport
        .serve_once(server, |message| match message {
            ControlMessage::Ping => ControlMessage::Ready,
            other => ControlMessage::Error(format!("unexpected: {other:?}")),
        })
        .expect("serve should succeed");

    let reply = transport
        .send("demo", ControlMessage::Ping)
        .expect("send should succeed");
    assert_eq!(reply, ControlMessage::Ready);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
mise run test -- cargo test client_ping_receives_ready
```

Expected: FAIL because the control transport abstraction does not exist.

- [ ] **Step 3: Write the minimal implementation**

Create a platform-agnostic interface:

```rust
pub trait ControlTransport {
    type Server;

    fn bind(&self, provider_name: &str) -> Result<Self::Server, String>;
    fn send(
        &self,
        provider_name: &str,
        message: ControlMessage,
    ) -> Result<ControlMessage, String>;
}
```

Implement:

- an in-memory test transport in `control.rs`
- a Unix socket transport in `control_unix.rs` for Linux/macOS
- a named-pipe transport in `control_windows.rs` for Windows

Keep serialization logic centralized in `control.rs` so the protocol stays
shared across platforms.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
mise run test -- cargo test client_ping_receives_ready
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/daemon/control.rs \
  crates/anymount/src/daemon/control_unix.rs \
  crates/anymount/src/daemon/control_windows.rs \
  crates/anymount/src/daemon/mod.rs
git commit -m "feat: add daemon control transport"
```

## Task 4: Implement the daemon runtime loop

**Files:**
- Create: `crates/anymount/src/daemon/runtime.rs`
- Modify: `crates/anymount/src/daemon/mod.rs`
- Modify: `crates/anymount/src/cli/commands/provide.rs`
- Test: `crates/anymount/src/daemon/runtime.rs`
- Test: `crates/anymount/src/cli/commands/provide.rs`

- [ ] **Step 1: Write the failing daemon loop tests**

```rust
#[test]
fn daemon_logs_telemetry_until_shutdown() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut runtime = DaemonRuntime::new(FakeLogger::default(), rx);

    tx.send(DaemonMessage::Telemetry("opened: file.txt".into()))
        .expect("send should work");
    tx.send(DaemonMessage::Shutdown).expect("send should work");

    runtime.run().expect("runtime should succeed");
    assert_eq!(runtime.logged(), vec!["opened: file.txt"]);
}

#[test]
fn provide_returns_error_when_provider_startup_fails() {
    let command = ProvideCommand {
        name: "demo".to_owned(),
        config_dir: None,
    };

    let err = command
        .run_with(&FailingProviderDaemonFactory::default())
        .unwrap_err();
    assert!(err.contains("startup"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
mise run test -- cargo test daemon_logs_telemetry_until_shutdown provide_returns_error_when_provider_startup_fails
```

Expected: FAIL because the runtime and injectable `ProvideCommand` execution
path do not exist.

- [ ] **Step 3: Write the minimal implementation**

Add:

- `DaemonRuntime` that blocks on a receiver of `DaemonMessage`
- a small runtime facade used by `ProvideCommand`
- `ProvideCommand::execute` delegating to the runtime facade
- a hook for provider startup that later tasks will connect to real provider
  code

Keep the loop minimal:

```rust
loop {
    match self.rx.recv().map_err(|e| e.to_string())? {
        DaemonMessage::Telemetry(message) => self.logger.info(message),
        DaemonMessage::Shutdown => break,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
mise run test -- cargo test daemon_logs_telemetry_until_shutdown provide_returns_error_when_provider_startup_fails
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/daemon/runtime.rs \
  crates/anymount/src/daemon/mod.rs \
  crates/anymount/src/cli/commands/provide.rs
git commit -m "feat: add provider daemon runtime"
```

## Task 5: Refactor provider connection for daemon ownership

**Files:**
- Modify: `crates/anymount/src/providers/provider.rs`
- Modify: `crates/anymount/src/lib.rs`
- Test: `crates/anymount/src/providers/provider.rs`

- [ ] **Step 1: Write the failing provider runtime tests**

```rust
#[test]
fn connect_named_provider_loads_single_provider() {
    let config = TestProvidersConfiguration::single_local("demo");
    let result = connect_named_provider("demo", &config, &NoOpLogger);
    assert!(result.is_ok());
}

#[test]
fn connect_named_provider_errors_for_missing_name() {
    let config = TestProvidersConfiguration::single_local("demo");
    let err = connect_named_provider("other", &config, &NoOpLogger).unwrap_err();
    assert!(err.contains("other"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
mise run test -- cargo test connect_named_provider_loads_single_provider connect_named_provider_errors_for_missing_name
```

Expected: FAIL because there is no daemon-oriented single-provider entry point.

- [ ] **Step 3: Write the minimal implementation**

Add a focused entry point for one provider, separate from the multi-provider
`connect_providers` path:

```rust
pub fn connect_named_provider(
    provider_name: &str,
    config: &impl ProvidersConfiguration,
    logger: &(impl Logger + 'static),
) -> Result<Box<dyn ProviderSession>, String> {
    // Load exactly one provider config and connect it.
}
```

If the current `ProvidersConfiguration` trait makes that awkward, add a small
helper layer in the CLI or config module to resolve one `ProviderFileConfig`
first and pass that into a new `connect_provider` function. Keep the daemon
path single-provider-first.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
mise run test -- cargo test connect_named_provider_loads_single_provider connect_named_provider_errors_for_missing_name
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/providers/provider.rs crates/anymount/src/lib.rs
git commit -m "refactor: add single-provider runtime entry point"
```

## Task 6: Thread callback telemetry into the daemon loop

**Files:**
- Modify: `crates/anymount/src/providers/cloudfilter/provider.rs`
- Modify: `crates/anymount/src/providers/cloudfilter/callbacks.rs`
- Modify: `crates/anymount/src/daemon/messages.rs`
- Test: `crates/anymount/src/providers/cloudfilter/callbacks.rs`

- [ ] **Step 1: Write the failing callback telemetry tests**

```rust
#[test]
fn callback_emits_telemetry_for_opened_event() {
    let (tx, rx) = std::sync::mpsc::channel();
    let callbacks = Callbacks::new(
        PathBuf::from("/mnt/demo"),
        FakeStorage::default(),
        NoOpLogger,
        tx,
    );

    callbacks.opened(fake_request("/mnt/demo/file.txt"), fake_opened_info());

    let message = rx.recv().expect("telemetry should be sent");
    assert!(matches!(message, DaemonMessage::Telemetry(text) if text.contains("opened")));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
mise run test -- cargo test callback_emits_telemetry_for_opened_event
```

Expected: FAIL because callbacks do not accept or emit daemon messages.

- [ ] **Step 3: Write the minimal implementation**

Extend callback construction to accept a cloned sender:

```rust
pub fn new(
    path: PathBuf,
    storage: S,
    logger: L,
    daemon_tx: Sender<DaemonMessage>,
) -> Self
```

Emit structured telemetry on key events such as `opened`, `closed`,
`fetch_data`, and failure cases. Logging inside callbacks may remain as
best-effort local fallback, but daemon telemetry becomes the primary path.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
mise run test -- cargo test callback_emits_telemetry_for_opened_event
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/providers/cloudfilter/provider.rs \
  crates/anymount/src/providers/cloudfilter/callbacks.rs \
  crates/anymount/src/daemon/messages.rs
git commit -m "feat: route callback telemetry through daemon messages"
```

## Task 7: Convert `connect` into a daemon supervisor

**Files:**
- Modify: `crates/anymount/src/cli/commands/connect.rs`
- Modify: `crates/anymount/src/cli/commands/provide.rs`
- Modify: `crates/anymount/src/daemon/control.rs`
- Modify: `crates/anymount/src/daemon/runtime.rs`
- Test: `crates/anymount/src/cli/commands/connect.rs`

- [ ] **Step 1: Write the failing supervisor tests**

```rust
#[test]
fn execute_reuses_running_provider_daemon() {
    let cmd = connect_named("demo");
    let launcher = RecordingLauncher::with_running("demo");

    cmd.run_with(&launcher, &NoOpLogger).expect("connect should succeed");

    assert_eq!(launcher.spawned(), Vec::<String>::new());
}

#[test]
fn execute_returns_error_when_one_provider_fails_during_all() {
    let cmd = connect_all();
    let launcher = RecordingLauncher::with_failure("broken", "startup failed");

    let err = cmd.run_with(&launcher, &NoOpLogger).unwrap_err();

    assert!(err.contains("broken"));
    assert!(launcher.started().contains(&"healthy".to_owned()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
mise run test -- cargo test execute_reuses_running_provider_daemon execute_returns_error_when_one_provider_fails_during_all
```

Expected: FAIL because `connect` still owns direct provider connections and has
no injectable daemon supervisor.

- [ ] **Step 3: Write the minimal implementation**

Split `connect` into:

- provider target resolution
- daemon liveness probe
- daemon launch and ready wait
- failure aggregation for `--all`

Add a small injectable launcher/supervisor port:

```rust
pub trait ProviderDaemonSupervisor {
    fn ensure_running(
        &self,
        provider_name: &str,
        config_dir: Option<&Path>,
        logger: &impl Logger,
    ) -> Result<(), String>;
}
```

The default implementation should:

- `Ping` existing endpoints
- spawn `anymount-cli provide --name <provider>`
- wait for `Ready` or `Error`

Keep inline `--path` mode either unsupported with a clear error or mapped to a
deliberately separate path. Decide explicitly and encode it in tests.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
mise run test -- cargo test execute_reuses_running_provider_daemon execute_returns_error_when_one_provider_fails_during_all
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/cli/commands/connect.rs \
  crates/anymount/src/cli/commands/provide.rs \
  crates/anymount/src/daemon/control.rs \
  crates/anymount/src/daemon/runtime.rs
git commit -m "feat: make connect supervise provider daemons"
```

## Task 8: Add docs, macOS transport coverage, and full verification

**Files:**
- Modify: `README.md`
- Modify: `crates/anymount/Cargo.toml`
- Test: platform-gated daemon control tests

- [ ] **Step 1: Write the failing docs or smoke tests**

```rust
#[test]
fn readme_mentions_provide_command() {
    let readme = std::fs::read_to_string("README.md").expect("readme should load");
    assert!(readme.contains("provide --name"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
mise run test -- cargo test readme_mentions_provide_command
```

Expected: FAIL because the README has not been updated.

- [ ] **Step 3: Write the minimal implementation**

Update:

- README quick-start and command descriptions to explain:
  - `connect` ensures provider daemons are running
  - `provide --name <provider>` is the long-lived provider process
  - control transport is Unix sockets on Linux/macOS and named pipes on
    Windows
- `crates/anymount/Cargo.toml` if new macOS or Windows IPC dependencies are
  required

Then run the full verification suite.

- [ ] **Step 4: Run verification**

Run:

```bash
mise run test
mise run build
```

Expected: all tests pass and the CLI builds successfully.

If targeted platform-specific transport tests exist, also run the relevant
subset on each platform before claiming completion.

- [ ] **Step 5: Commit**

```bash
git add README.md crates/anymount/Cargo.toml Cargo.lock
git commit -m "docs: document provider daemon workflow"
```

## Notes for the Implementer

- The current codebase has no macOS provider backend, only Linux and Windows
  provider connection paths. The daemon control transport should still be
  implemented for macOS, but provider runtime support may need to stay
  explicitly unsupported until a macOS provider backend exists.
- Favor small injectable traits over large mocks. The CLI tests already follow
  that pattern in `connect.rs`.
- Keep callback telemetry structured enough that it can evolve from plain log
  strings into richer status events later without breaking the loop design.
- If inline `connect --path ...` no longer fits the daemon model cleanly, call
  that out early and either preserve it as a non-daemon compatibility mode or
  reject it explicitly with tests and documentation.
