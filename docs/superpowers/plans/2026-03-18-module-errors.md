# Module Error Types Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `String`-based and umbrella errors with module-owned typed
errors across `anymount`, preserving third-party causes in integration-heavy
leaf modules and exposing module-specific errors from public APIs.

**Architecture:** Add top-level `Error` and `Result<T>` pairs for `auth`,
`config`, `daemon`, `providers`, `storages`, `cli`, and `tui`; add dedicated
leaf errors for OneDrive and platform-provider integrations; migrate trait
boundaries to parent-module results; keep final user-facing formatting at the
CLI and TUI edges. Remove `crate::Error` from the main public surface and keep
`src/error.rs` only as a deprecated compatibility shim if it still needs to
exist during migration.

**Tech Stack:** Rust, `thiserror`, existing `oauth2`, `ureq`, `serde_json`,
`zbus`, `windows`, `cloud-filter`, `fuser`, inline unit tests, `cargo test`,
`mise`.

---

## File Structure

Planned file responsibilities:

- Modify: `crates/anymount/src/config.rs`
  Add `config::Error`, `config::Result<T>`, and replace `String` returns in
  `ConfigDir`.
- Create: `crates/anymount/src/auth/error.rs`
  Define `auth::Error` and `auth::Result<T>`.
- Modify: `crates/anymount/src/auth/mod.rs`
  Export the auth error surface.
- Modify: `crates/anymount/src/auth/onedrive.rs`
  Add `auth::onedrive::Error` and stop stringifying OAuth failures.
- Create: `crates/anymount/src/storages/error.rs`
  Define `storages::Error` and `storages::Result<T>`.
- Modify: `crates/anymount/src/storages/mod.rs`
  Export the storage error surface.
- Modify: `crates/anymount/src/storages/storage.rs`
  Change trait signatures from `String` to `storages::Result<T>`.
- Modify: `crates/anymount/src/storages/local.rs`
  Return typed local-storage errors.
- Modify: `crates/anymount/src/storages/onedrive.rs`
  Add `storages::onedrive::Error` and wrap auth/network/JSON failures.
- Modify: `crates/anymount/src/providers/cloudfilter/callbacks.rs`
  Update `WriteAt` adapters to the new storage result type.
- Modify: `crates/anymount/src/providers/libcloudprovider/fuse.rs`
  Update storage/cache call sites to the new storage result type.
- Create: `crates/anymount/src/daemon/error.rs`
  Define `daemon::Error` and `daemon::Result<T>`.
- Modify: `crates/anymount/src/daemon/mod.rs`
  Export the daemon error surface.
- Modify: `crates/anymount/src/daemon/control.rs`
  Move transport APIs to `daemon::Result<T>`.
- Modify: `crates/anymount/src/daemon/control_unix.rs`
  Return typed transport errors.
- Modify: `crates/anymount/src/daemon/control_windows.rs`
  Return typed transport errors.
- Modify: `crates/anymount/src/daemon/messages.rs`
  Return typed decode/encode errors.
- Modify: `crates/anymount/src/daemon/paths.rs`
  Return typed endpoint derivation errors.
- Modify: `crates/anymount/src/daemon/runtime.rs`
  Return typed runtime errors.
- Create: `crates/anymount/src/providers/cloudfilter/error.rs`
  Define Windows/Cloud Filter-specific provider errors.
- Modify: `crates/anymount/src/providers/cloudfilter/mod.rs`
  Export `cloudfilter::Error`.
- Modify: `crates/anymount/src/providers/cloudfilter/cleanup_registry.rs`
  Replace strings with `cloudfilter::Result<T>`.
- Modify: `crates/anymount/src/providers/cloudfilter/placeholders.rs`
  Replace strings with `cloudfilter::Result<T>`.
- Modify: `crates/anymount/src/providers/cloudfilter/provider.rs`
  Return typed cloudfilter provider errors.
- Modify: `crates/anymount/src/providers/cloudfilter/register.rs`
  Replace `crate::Error` usage with `cloudfilter::Error`.
- Create: `crates/anymount/src/providers/libcloudprovider/error.rs`
  Define FUSE/D-Bus-specific provider errors.
- Modify: `crates/anymount/src/providers/libcloudprovider/mod.rs`
  Export `libcloudprovider::Error`.
- Modify: `crates/anymount/src/providers/libcloudprovider/provider.rs`
  Return typed mount/D-Bus errors.
- Modify: `crates/anymount/src/providers/libcloudprovider/fuse.rs`
  Return typed FUSE/cache errors.
- Modify: `crates/anymount/src/providers/libcloudprovider/dbus.rs`
  Keep leaf-specific third-party errors structured where they cross module
  boundaries.
- Create: `crates/anymount/src/providers/error.rs`
  Define top-level `providers::Error`.
- Modify: `crates/anymount/src/providers/mod.rs`
  Export the provider error surface.
- Modify: `crates/anymount/src/providers/provider.rs`
  Return `providers::Result<T>` from orchestration entry points.
- Create: `crates/anymount/src/cli/error.rs`
  Define `cli::Error` and `cli::Result<T>`.
- Modify: `crates/anymount/src/cli/mod.rs`
  Export the CLI error surface.
- Modify: `crates/anymount/src/cli/cli.rs`
  Return `cli::Result<()>` from `Cli::run`.
- Modify: `crates/anymount/src/cli/run.rs`
  Return `cli::Result<()>`.
- Modify: `crates/anymount/src/cli/commands/auth.rs`
  Stop using `String` in auth command seams and wrap into `cli::Error`.
- Modify: `crates/anymount/src/cli/commands/config.rs`
  Wrap `config::Error` and prompt/validation failures into `cli::Error`.
- Modify: `crates/anymount/src/cli/commands/connect.rs`
  Wrap `config`, `daemon`, and process-supervision failures into `cli::Error`.
- Modify: `crates/anymount/src/cli/commands/provide.rs`
  Wrap `config`, `daemon`, and provider startup failures into `cli::Error`.
- Modify: `crates/anymount/src/main.rs`
  Return the module-specific CLI result type.
- Modify: `crates/anymount/src/lib.rs`
  Stop re-exporting the crate-wide umbrella error.
- Modify: `crates/anymount/src/error.rs`
  Keep only as a deprecated compatibility shim if still needed.
- Create: `crates/anymount/src/tui/error.rs`
  Define `tui::Error` and `tui::Result<T>`.
- Modify: `crates/anymount/src/tui/mod.rs`
  Export the TUI error surface.
- Modify: `crates/anymount/src/tui/tui.rs`
  Replace `String` returns with `tui::Result<T>` and wrap config/auth/cli/io
  errors.

## Task 1: Convert `config` to a typed public error

**Files:**
- Modify: `crates/anymount/src/config.rs`
- Test: `crates/anymount/src/config.rs`

- [ ] **Step 1: Write the failing config error tests**

```rust
#[test]
fn read_nonexistent_returns_read_error() {
    let (_tmp, cd) = tmp_config_dir();
    let err = cd.read("nope").expect_err("read should fail");

    assert!(matches!(err, Error::Read { .. }));
}

#[test]
fn read_invalid_toml_returns_parse_error() {
    let (_tmp, cd) = tmp_config_dir();
    std::fs::write(cd.dir().join("broken.toml"), "path = [").expect("seed invalid toml");

    let err = cd.read("broken").expect_err("read should fail");
    assert!(matches!(err, Error::Parse { .. }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p anymount config::tests
```

Expected: FAIL because `config::Error` does not exist and the config API still
returns `String`.

- [ ] **Step 3: Write the minimal implementation**

Add a typed config error in `config.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read config dir {path}: {source}")]
    ReadDir { path: PathBuf, #[source] source: io::Error },
    #[error("cannot read config {path}: {source}")]
    Read { path: PathBuf, #[source] source: io::Error },
    #[error("invalid config {path}: {source}")]
    Parse { path: PathBuf, #[source] source: toml::de::Error },
    #[error("cannot create config dir {path}: {source}")]
    CreateDir { path: PathBuf, #[source] source: io::Error },
    #[error("cannot write config {path}: {source}")]
    Write { path: PathBuf, #[source] source: io::Error },
    #[error("cannot remove config {path}: {source}")]
    Remove { path: PathBuf, #[source] source: io::Error },
    #[error("cannot serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("non-utf8 filename: {path}")]
    NonUtf8FileName { path: PathBuf },
}

pub type Result<T> = std::result::Result<T, Error>;
```

Then update `ConfigDir::{list, read, write, remove, load_all}` to return
`config::Result<T>` and construct the new variants instead of formatting
strings.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p anymount config::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/config.rs
git commit -m "refactor: add typed config errors"
```

## Task 2: Add typed auth errors and adapt auth command seams

**Files:**
- Create: `crates/anymount/src/auth/error.rs`
- Modify: `crates/anymount/src/auth/mod.rs`
- Modify: `crates/anymount/src/auth/onedrive.rs`
- Modify: `crates/anymount/src/cli/commands/auth.rs`
- Test: `crates/anymount/src/auth/onedrive.rs`

- [ ] **Step 1: Write the failing auth error tests**

```rust
#[test]
fn classify_wait_error_returns_device_code_expired() {
    let err = classify_wait_error("expired_token");
    assert!(matches!(err, Error::DeviceCodeExpired));
}

#[test]
fn access_token_without_refresh_token_returns_missing_refresh_token_error() {
    let source = OneDriveTokenSource::new(None, None, None, None)
        .expect("constructor should succeed");

    let err = source.access_token().expect_err("access token should fail");
    assert!(matches!(err, Error::MissingRefreshToken));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p anymount auth::onedrive::tests
```

Expected: FAIL because `auth::onedrive::Error` and the classification helper do
not exist.

- [ ] **Step 3: Write the minimal implementation**

Create the top-level auth error surface:

```rust
// crates/anymount/src/auth/error.rs
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    OneDrive(#[from] crate::auth::onedrive::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
```

Add a leaf error in `auth/onedrive.rs` with concrete variants:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid device authorization url: {0}")]
    InvalidDeviceAuthorizationUrl(#[source] url::ParseError),
    #[error("invalid auth url: {0}")]
    InvalidAuthUrl(#[source] url::ParseError),
    #[error("invalid token url: {0}")]
    InvalidTokenUrl(#[source] url::ParseError),
    #[error("device code request failed")]
    DeviceCodeRequest(#[source] DeviceCodeRequestError),
    #[error("device code expired")]
    DeviceCodeExpired,
    #[error("sign-in was declined")]
    AuthorizationDeclined,
    #[error("token request failed")]
    TokenRequest(#[source] DeviceTokenError),
    #[error("missing refresh token")]
    MissingRefreshToken,
    #[error("refresh token request failed")]
    RefreshTokenRequest(#[source] RefreshTokenError),
}
```

Extract a small `classify_wait_error` helper for the existing `"expired"` and
`"declined"` mapping, and update `cli/commands/auth.rs` to use the typed auth
errors internally while temporarily converting to strings at the CLI edge until
Task 7.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p anymount auth::onedrive::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/auth/error.rs \
  crates/anymount/src/auth/mod.rs \
  crates/anymount/src/auth/onedrive.rs \
  crates/anymount/src/cli/commands/auth.rs
git commit -m "refactor: add typed auth errors"
```

## Task 3: Add storage errors and migrate the storage trait boundary

**Files:**
- Create: `crates/anymount/src/storages/error.rs`
- Modify: `crates/anymount/src/storages/mod.rs`
- Modify: `crates/anymount/src/storages/storage.rs`
- Modify: `crates/anymount/src/storages/local.rs`
- Modify: `crates/anymount/src/storages/onedrive.rs`
- Modify: `crates/anymount/src/providers/cloudfilter/callbacks.rs`
- Modify: `crates/anymount/src/providers/libcloudprovider/fuse.rs`
- Test: `crates/anymount/src/storages/onedrive.rs`

- [ ] **Step 1: Write the failing storage error tests**

```rust
#[test]
fn config_fails_with_no_token_returns_invalid_config_error() {
    let config = OneDriveConfig {
        root: PathBuf::from("/"),
        endpoint: "https://graph.microsoft.com/v1.0".into(),
        access_token: None,
        refresh_token: None,
        client_id: None,
        token_expiry_buffer_secs: None,
    };

    let err = config.connect().expect_err("config should fail");
    assert!(matches!(err, Error::OneDrive(onedrive::Error::InvalidConfig { .. })));
}

#[test]
fn read_dir_http_failure_returns_list_error() {
    let storage = fake_onedrive_storage_with_http_status(500, b"boom");
    let err = storage.read_dir(PathBuf::new()).expect_err("list should fail");

    assert!(matches!(err, Error::OneDrive(onedrive::Error::ListFailed { .. })));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p anymount storages::onedrive::tests
```

Expected: FAIL because `storages::Error`, `storages::onedrive::Error`, and the
storage trait aliases do not exist.

- [ ] **Step 3: Write the minimal implementation**

Define the parent storage error:

```rust
// crates/anymount/src/storages/error.rs
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("storage io error at {path}: {source}")]
    Io { path: PathBuf, #[source] source: std::io::Error },
    #[error("unexpected eof while reading {path}")]
    UnexpectedEof { path: PathBuf },
    #[error(transparent)]
    OneDrive(#[from] crate::storages::onedrive::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
```

Update the storage traits:

```rust
pub trait Storage: Send + Sync + 'static {
    type Entry: DirEntry;
    type Iter: Iterator<Item = Self::Entry>;

    fn read_dir(&self, path: PathBuf) -> crate::storages::Result<Self::Iter>;
    fn read_file_at(
        &self,
        path: PathBuf,
        writer: &mut impl WriteAt,
        range: Range<u64>,
    ) -> crate::storages::Result<()>;
}

pub trait WriteAt {
    fn write_at(&mut self, buf: &[u8], offset: u64) -> crate::storages::Result<()>;
}
```

Then add `storages::onedrive::Error` for config, token, request, status, JSON,
and write-at failures, and update the callback/FUSE write adapters to return
`storages::Error` instead of `String`.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p anymount storages::onedrive::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/storages/error.rs \
  crates/anymount/src/storages/mod.rs \
  crates/anymount/src/storages/storage.rs \
  crates/anymount/src/storages/local.rs \
  crates/anymount/src/storages/onedrive.rs \
  crates/anymount/src/providers/cloudfilter/callbacks.rs \
  crates/anymount/src/providers/libcloudprovider/fuse.rs
git commit -m "refactor: add typed storage errors"
```

## Task 4: Add typed daemon errors and remove `String` transport results

**Files:**
- Create: `crates/anymount/src/daemon/error.rs`
- Modify: `crates/anymount/src/daemon/mod.rs`
- Modify: `crates/anymount/src/daemon/control.rs`
- Modify: `crates/anymount/src/daemon/control_unix.rs`
- Modify: `crates/anymount/src/daemon/control_windows.rs`
- Modify: `crates/anymount/src/daemon/messages.rs`
- Modify: `crates/anymount/src/daemon/paths.rs`
- Modify: `crates/anymount/src/daemon/runtime.rs`
- Modify: `crates/anymount/src/cli/commands/connect.rs`
- Modify: `crates/anymount/src/cli/commands/provide.rs`
- Test: `crates/anymount/src/daemon/messages.rs`
- Test: `crates/anymount/src/daemon/paths.rs`

- [ ] **Step 1: Write the failing daemon error tests**

```rust
#[test]
fn decode_invalid_utf8_returns_decode_error() {
    let err = ControlMessage::decode(&[0xff]).expect_err("decode should fail");
    assert!(matches!(err, Error::DecodeUtf8(_)));
}

#[test]
fn provider_endpoint_rejects_separator_in_provider_name() {
    let err = provider_endpoint("demo/name").expect_err("endpoint should fail");
    assert!(matches!(err, Error::InvalidProviderName { .. }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p anymount daemon::messages::tests
cargo test -p anymount daemon::paths::tests
```

Expected: FAIL because `daemon::Error` does not exist and the daemon APIs still
return `String`.

- [ ] **Step 3: Write the minimal implementation**

Define the daemon error surface:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid provider name: {name}")]
    InvalidProviderName { name: String },
    #[error("control message was not valid utf-8: {0}")]
    DecodeUtf8(#[from] std::str::Utf8Error),
    #[error("unknown control message: {value}")]
    UnknownControlMessage { value: String },
    #[error("daemon io error during {operation} for {provider_name}: {source}")]
    Io {
        operation: &'static str,
        provider_name: String,
        #[source]
        source: std::io::Error,
    },
    #[error("daemon receive failed: {0}")]
    Receive(#[from] std::sync::mpsc::RecvError),
    #[error("control transport not supported on this platform")]
    NotSupported,
}

pub type Result<T> = std::result::Result<T, Error>;
```

Switch `ControlTransport`, `ControlMessage::decode`, endpoint helpers, and
runtime APIs to `daemon::Result<T>`. Keep the CLI callers compiling by
temporarily converting daemon errors to strings at the command boundary until
Task 7.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p anymount daemon::messages::tests
cargo test -p anymount daemon::paths::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/daemon/error.rs \
  crates/anymount/src/daemon/mod.rs \
  crates/anymount/src/daemon/control.rs \
  crates/anymount/src/daemon/control_unix.rs \
  crates/anymount/src/daemon/control_windows.rs \
  crates/anymount/src/daemon/messages.rs \
  crates/anymount/src/daemon/paths.rs \
  crates/anymount/src/daemon/runtime.rs \
  crates/anymount/src/cli/commands/connect.rs \
  crates/anymount/src/cli/commands/provide.rs
git commit -m "refactor: add typed daemon errors"
```

## Task 5: Add Windows Cloud Filter leaf errors

**Files:**
- Create: `crates/anymount/src/providers/cloudfilter/error.rs`
- Modify: `crates/anymount/src/providers/cloudfilter/mod.rs`
- Modify: `crates/anymount/src/providers/cloudfilter/cleanup_registry.rs`
- Modify: `crates/anymount/src/providers/cloudfilter/placeholders.rs`
- Modify: `crates/anymount/src/providers/cloudfilter/provider.rs`
- Modify: `crates/anymount/src/providers/cloudfilter/register.rs`
- Test: `crates/anymount/src/providers/cloudfilter/cleanup_registry.rs`
- Test: `crates/anymount/src/providers/cloudfilter/register.rs`

- [ ] **Step 1: Write the failing Cloud Filter error tests**

```rust
#[cfg(target_os = "windows")]
#[test]
fn cleanup_registry_wraps_unregister_error() {
    let err = cleanup_registry_with::<FailingRegistryAccess>(&config, &NoOpLogger)
        .expect_err("cleanup should fail");

    assert!(matches!(err, Error::CleanupRegistry { .. }));
}

#[cfg(target_os = "windows")]
#[test]
fn register_sync_root_wraps_platform_error() {
    let err = make_test_registrar()
        .register_sync_root(&registration_config())
        .expect_err("registration should fail");

    assert!(matches!(err, Error::RegisterSyncRoot { .. }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p anymount providers::cloudfilter
```

Expected: FAIL because `cloudfilter::Error` does not exist and the Cloud Filter
helpers still return `String` or `crate::Error`.

- [ ] **Step 3: Write the minimal implementation**

Define a Windows-specific provider error:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cleanup registry failed for {path}: {source}")]
    CleanupRegistry { path: PathBuf, #[source] source: windows::core::Error },
    #[error("placeholder operation failed for {path}: {source}")]
    Placeholder { path: PathBuf, #[source] source: std::io::Error },
    #[error("register sync root failed for {path}: {source}")]
    RegisterSyncRoot { path: PathBuf, #[source] source: windows::core::Error },
    #[error("unregister sync root failed for {path}: {source}")]
    UnregisterSyncRoot { path: PathBuf, #[source] source: windows::core::Error },
    #[error("connect sync root failed for {path}: {source}")]
    ConnectSyncRoot { path: PathBuf, #[source] source: windows::core::Error },
}

pub type Result<T> = std::result::Result<T, Error>;
```

Replace the remaining `crate::Error` and `String` returns in the Cloud Filter
submodule with `cloudfilter::Result<T>`.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p anymount providers::cloudfilter
```

Expected: PASS on Windows. On non-Windows platforms, the module remains
target-gated and is skipped.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/providers/cloudfilter/error.rs \
  crates/anymount/src/providers/cloudfilter/mod.rs \
  crates/anymount/src/providers/cloudfilter/cleanup_registry.rs \
  crates/anymount/src/providers/cloudfilter/placeholders.rs \
  crates/anymount/src/providers/cloudfilter/provider.rs \
  crates/anymount/src/providers/cloudfilter/register.rs
git commit -m "refactor: add typed cloudfilter errors"
```

## Task 6: Add Linux libcloudprovider errors and top-level provider errors

**Files:**
- Create: `crates/anymount/src/providers/libcloudprovider/error.rs`
- Modify: `crates/anymount/src/providers/libcloudprovider/mod.rs`
- Modify: `crates/anymount/src/providers/libcloudprovider/provider.rs`
- Modify: `crates/anymount/src/providers/libcloudprovider/fuse.rs`
- Modify: `crates/anymount/src/providers/libcloudprovider/dbus.rs`
- Create: `crates/anymount/src/providers/error.rs`
- Modify: `crates/anymount/src/providers/mod.rs`
- Modify: `crates/anymount/src/providers/provider.rs`
- Modify: `crates/anymount/src/lib.rs`
- Test: `crates/anymount/src/providers/provider.rs`
- Test: `crates/anymount/src/providers/libcloudprovider/fuse.rs`

- [ ] **Step 1: Write the failing provider error tests**

```rust
#[test]
fn connect_providers_invalid_onedrive_config_returns_storage_error() {
    let config = test_config_with_invalid_onedrive();
    let err = connect_providers(&config, &NoOpLogger)
        .expect_err("connect should fail");

    assert!(matches!(err, Error::Storage(_)));
}

#[cfg(target_os = "linux")]
#[test]
fn sparse_cache_new_wraps_cache_io_error() {
    let err = SparseFsCache::new(PathBuf::from("/proc/anymount-denied"))
        .expect_err("cache init should fail");

    assert!(matches!(err, crate::providers::libcloudprovider::Error::CacheIo { .. }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p anymount providers::provider::tests
```

Expected: FAIL because `providers::Error` and
`providers::libcloudprovider::Error` do not exist.

- [ ] **Step 3: Write the minimal implementation**

Define the top-level provider error surface:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Storage(#[from] crate::storages::Error),
    #[cfg(target_os = "windows")]
    #[error(transparent)]
    CloudFilter(#[from] crate::providers::cloudfilter::Error),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    LibCloudProvider(#[from] crate::providers::libcloudprovider::Error),
    #[error("provider runtime not supported on this platform")]
    NotSupported,
}

pub type Result<T> = std::result::Result<T, Error>;
```

Define the Linux leaf error surface in
`providers/libcloudprovider/error.rs` for mount, cache, D-Bus, and action
runtime failures, then convert `connect_providers*`, `mount_storage`, and
`export_on_dbus` to return typed provider results.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p anymount providers::provider::tests
```

Expected: PASS. On Linux, also run:

```bash
cargo test -p anymount providers::libcloudprovider
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/providers/libcloudprovider/error.rs \
  crates/anymount/src/providers/libcloudprovider/mod.rs \
  crates/anymount/src/providers/libcloudprovider/provider.rs \
  crates/anymount/src/providers/libcloudprovider/fuse.rs \
  crates/anymount/src/providers/libcloudprovider/dbus.rs \
  crates/anymount/src/providers/error.rs \
  crates/anymount/src/providers/mod.rs \
  crates/anymount/src/providers/provider.rs \
  crates/anymount/src/lib.rs
git commit -m "refactor: add typed provider errors"
```

## Task 7: Convert the CLI surface and deprecate the umbrella error export

**Files:**
- Create: `crates/anymount/src/cli/error.rs`
- Modify: `crates/anymount/src/cli/mod.rs`
- Modify: `crates/anymount/src/cli/cli.rs`
- Modify: `crates/anymount/src/cli/run.rs`
- Modify: `crates/anymount/src/cli/commands/auth.rs`
- Modify: `crates/anymount/src/cli/commands/config.rs`
- Modify: `crates/anymount/src/cli/commands/connect.rs`
- Modify: `crates/anymount/src/cli/commands/provide.rs`
- Modify: `crates/anymount/src/main.rs`
- Modify: `crates/anymount/src/lib.rs`
- Modify: `crates/anymount/src/error.rs`
- Test: `crates/anymount/src/cli/commands/auth.rs`
- Test: `crates/anymount/src/cli/commands/connect.rs`
- Test: `crates/anymount/src/cli/commands/provide.rs`

- [ ] **Step 1: Write the failing CLI error tests**

```rust
#[test]
fn connect_without_args_returns_missing_target_error() {
    let cmd = ConnectCommand {
        name: None,
        all: false,
        config_dir: None,
    };

    let err = cmd
        ._execute(&RecordingSupervisor::default(), &NoOpLogger)
        .expect_err("connect should fail");

    assert!(matches!(err, crate::cli::Error::MissingConnectTarget));
}

#[test]
fn auth_execute_wraps_auth_error() {
    let cmd = AuthCommand {
        subcommand: AuthSubcommand::OneDrive(AuthOneDrive { client_id: None }),
    };

    let err = cmd._execute(FailingAuthorizer, &NoOpUrlOpener)
        .expect_err("auth should fail");

    assert!(matches!(err, crate::cli::Error::Auth(_)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p anymount cli::commands
```

Expected: FAIL because `cli::Error` does not exist and command execute methods
still return `String`.

- [ ] **Step 3: Write the minimal implementation**

Create the CLI error surface:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Auth(#[from] crate::auth::Error),
    #[error(transparent)]
    Config(#[from] crate::config::Error),
    #[error(transparent)]
    Daemon(#[from] crate::daemon::Error),
    #[error(transparent)]
    Providers(#[from] crate::providers::Error),
    #[error("specify --name <NAME> or --all")]
    MissingConnectTarget,
    #[error("specify --name <NAME> or --path <PATH> with a storage subcommand")]
    MissingProvideTarget,
    #[error("could not open browser automatically")]
    OpenBrowser,
    #[error("failed to spawn provider process for {provider_name}: {source}")]
    SpawnProvider {
        provider_name: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Prompt(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

Then:

- change `Cli::run`, `cli::run`, and command `execute` methods to `cli::Result`
- convert the temporary `to_string()` adapters from earlier tasks into `?`
  conversions
- stop re-exporting `error::{Error, Result}` from `lib.rs`
- keep `src/error.rs` only as a deprecated compatibility shim if it still has
  callers outside this crate

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p anymount cli::commands
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/cli/error.rs \
  crates/anymount/src/cli/mod.rs \
  crates/anymount/src/cli/cli.rs \
  crates/anymount/src/cli/run.rs \
  crates/anymount/src/cli/commands/auth.rs \
  crates/anymount/src/cli/commands/config.rs \
  crates/anymount/src/cli/commands/connect.rs \
  crates/anymount/src/cli/commands/provide.rs \
  crates/anymount/src/main.rs \
  crates/anymount/src/lib.rs \
  crates/anymount/src/error.rs
git commit -m "refactor: add typed cli errors"
```

## Task 8: Convert the TUI and finish full verification

**Files:**
- Create: `crates/anymount/src/tui/error.rs`
- Modify: `crates/anymount/src/tui/mod.rs`
- Modify: `crates/anymount/src/tui/tui.rs`
- Test: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Write the failing TUI error tests**

```rust
#[test]
fn optional_u64_invalid_returns_invalid_number_error() {
    let err = optional_u64("abc", "storage.token_expiry_buffer_secs")
        .expect_err("parse should fail");

    assert!(matches!(err, Error::InvalidNumber { .. }));
}

#[test]
fn app_state_load_wraps_config_error() {
    let cd = ConfigDir::new(PathBuf::from("does-not-exist"));
    let err = AppState::load(&cd).expect_err("load should fail");

    assert!(matches!(err, Error::Config(_)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p anymount tui::tui::tests
```

Expected: FAIL because `tui::Error` does not exist and the TUI still returns
`String`.

- [ ] **Step 3: Write the minimal implementation**

Add the TUI error surface:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Auth(#[from] crate::auth::Error),
    #[error(transparent)]
    Cli(#[from] crate::cli::Error),
    #[error(transparent)]
    Config(#[from] crate::config::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid number for {key}: {value}")]
    InvalidNumber {
        key: &'static str,
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },
    #[error("{0}")]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

Convert `tui::run`, `AppState`, the edit/save helpers, terminal enter/leave
helpers, and the OneDrive auth integration points to `tui::Result<T>`.

- [ ] **Step 4: Run verification**

Run:

```bash
cargo test -p anymount
mise run test
mise run build
```

Expected: PASS.

If you are on Windows, also rerun:

```bash
cargo test -p anymount providers::cloudfilter
```

If you are on Linux, also rerun:

```bash
cargo test -p anymount providers::libcloudprovider
```

Expected: PASS on the active platform.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/tui/error.rs \
  crates/anymount/src/tui/mod.rs \
  crates/anymount/src/tui/tui.rs
git commit -m "refactor: add typed tui errors"
```

## Notes for the Implementer

- `thiserror = "1.0"` is already present in `crates/anymount/Cargo.toml`; use
  it as-is unless there is a separate reason to upgrade.
- Prefer inline error enums for single-file modules like `config.rs`, and
  dedicated `error.rs` siblings for directory modules like `auth`, `daemon`,
  `providers`, `storages`, `cli`, and `tui`.
- Do not reintroduce `String` adapters once a parent module has its typed error
  surface. Temporary `.to_string()` bridges are allowed only until the parent
  module is migrated in the next task.
- Where a leaf module has no meaningful structured failure beyond `io::Error`,
  keep the parent-module error and do not create a gratuitous new public type.
- After Task 7, run a workspace search for `Result<.*, String>`,
  `crate::Error`, and `crate::Result` and eliminate any remaining public API
  uses before claiming completion.
