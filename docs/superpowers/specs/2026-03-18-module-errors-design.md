# Module Error Types Design

**Date:** 2026-03-18

**Goal:** Replace `String`-based and umbrella errors with module-owned typed
errors, while preserving third-party causes in integration-heavy leaf modules
and exposing module-specific errors from public APIs.

## Summary

`anymount` currently mixes a crate-wide [`crate::error::Error`] with a much
larger set of `Result<_, String>` APIs. That loses structure, forces early
stringification, and makes error assertions in tests weaker than they need to
be.

This design moves the crate to a hybrid layered model:

- each top-level module owns a public `Error` and `Result<T>`
- integration-heavy leaf modules keep dedicated error types where they wrap
  third-party or platform APIs
- parent modules compose child errors with `#[from]`
- CLI and TUI remain the user-facing formatting boundary
- the legacy crate-wide `error` module is deprecated out of the public surface

## Scope

This design covers:

- `config`, `auth`, `storages`, `daemon`, `providers`, `cli`, and `tui`
- dedicated leaf errors for OneDrive and platform-provider integrations
- trait boundary changes needed to stop returning `String`
- deprecating the umbrella `crate::Error` public API
- test strategy for structured errors and source chains

This design does not cover:

- a broad refactor of module layout unrelated to error handling
- replacing `thiserror` with another crate
- converting every private helper into its own public error type
- changing user-facing command behavior beyond error rendering

## Design Rules

### Public API boundary

Every top-level module that exposes fallible public functions will expose its
own public `Error` and `Result<T>` alias:

- `auth::Error`
- `config::Error`
- `daemon::Error`
- `providers::Error`
- `storages::Error`
- `cli::Error`
- `tui::Error`

Those types are the primary public error surface for the module. Public APIs
must stop returning `String` and stop routing through `crate::Error`.

### Leaf error boundary

Integration-heavy leaf modules keep their own typed errors when they interact
with third-party or platform APIs and need finer-grained variants than their
parent module should expose by default.

This applies to:

- `auth::onedrive::Error`
- `storages::onedrive::Error`
- `providers::cloudfilter::Error`
- `providers::libcloudprovider::Error`

If implementation pressure shows that `providers::libcloudprovider::fuse` needs
its own error type to stay readable, that is acceptable, but it is not a
required design goal on day one.

### Conversion rule

Errors move leaf-to-parent without stringification:

1. leaf module constructs a typed error as close as possible to the raw failure
2. parent module wraps leaf error with `#[from]` or a structured variant
3. CLI/TUI decide how to render for humans

Modules may add context at their own abstraction layer, but they should not
duplicate the entire source message in the parent `Display` output.

### Trait boundary rule

Traits that currently hard-code `String` as the error type should use the
parent module error type instead of introducing associated error types in this
refactor.

That means:

- `storages::Storage`, `storages::WriteAt`, and related internal traits move to
  `storages::Result`
- daemon transport/runtime traits move to `daemon::Result`
- provider orchestration functions move to `providers::Result`

Associated error types are intentionally deferred. They preserve more precision
but add generic complexity across a codebase that is already mid-migration.

## Module Layout

### `config`

`config` stays a single-file module. Its error type lives in
`crates/anymount/src/config.rs`.

Expected variants include:

- config directory read failures
- config file read/write/remove failures with path context
- TOML parse failures with path context
- TOML serialization failures
- invalid non-UTF-8 filenames

`ConfigDir::{list, read, write, remove, load_all}` return `config::Result`.

### `auth`

`auth` gains a top-level `auth::Error` and `auth::Result<T>` alias in
`crates/anymount/src/auth/error.rs`, exported from
`crates/anymount/src/auth/mod.rs`.

`auth::onedrive::Error` owns OneDrive device-code and token-refresh failures,
including:

- URL construction problems
- device-code request failure
- token wait failure
- expired or declined device code
- missing refresh token when refresh is required

The OneDrive authorizer and token source APIs return typed auth errors directly.

### `storages`

`storages` gains `storages::Error` and `storages::Result<T>` in
`crates/anymount/src/storages/error.rs`.

`storages::onedrive::Error` keeps the OneDrive-specific integration detail:

- invalid config
- bearer-token failures
- HTTP request failures
- unexpected HTTP status responses
- JSON decode failures
- path translation or range validation issues where needed

The public storage traits in `crates/anymount/src/storages/storage.rs` switch
to `storages::Result`.

### `daemon`

`daemon` gains `daemon::Error` and `daemon::Result<T>` in
`crates/anymount/src/daemon/error.rs`.

This module does not need multiple public leaf errors yet. A single daemon
error enum is sufficient for:

- invalid provider names for endpoint derivation
- control message encode/decode failures
- control transport IO failures
- runtime channel failures
- unsupported platform transport cases

`ControlMessage::decode`, endpoint helpers, transport helpers, and runtime APIs
all return `daemon::Result`.

### `providers`

`providers` gains `providers::Error` and `providers::Result<T>` in
`crates/anymount/src/providers/error.rs`.

The public provider orchestration layer wraps:

- `storages::Error`
- `providers::cloudfilter::Error`
- `providers::libcloudprovider::Error`
- unsupported-platform failures
- module-level validation errors such as missing named providers

The platform-provider submodules each own their own error types:

- `providers::cloudfilter::Error` wraps Windows and Cloud Filter failures,
  registry cleanup failures, placeholder failures, and sync-root registration
  failures
- `providers::libcloudprovider::Error` wraps FUSE, zbus/dbus, runtime, and
  cache-related failures

### `cli`

`cli` gains `cli::Error` and `cli::Result<T>` in
`crates/anymount/src/cli/error.rs`.

`Cli::run`, `cli::run`, and command `execute` methods return `cli::Result`.

Commands may keep smaller private error enums inside command files where that
improves clarity, but the public module boundary is `cli::Error`.

`cli::Error` wraps:

- `config::Error`
- `auth::Error`
- `daemon::Error`
- `providers::Error`
- command-argument and command-policy errors
- browser open or process-spawn errors where those belong at the CLI layer

### `tui`

`tui` gains `tui::Error` and `tui::Result<T>` in
`crates/anymount/src/tui/error.rs`.

The TUI remains the broadest application surface, so its error enum wraps:

- `config::Error`
- `cli::Error`
- `auth::Error`
- `std::io::Error`
- input-validation and form-conversion errors currently flattened into strings

`tui::run` and TUI helper functions stop returning `String`.

### Legacy `crate::error`

The existing `crates/anymount/src/error.rs` becomes a deprecated compatibility
shim only.

Required changes:

- stop re-exporting `Error` and `Result` from `crates/anymount/src/lib.rs`
- mark the public `error` module deprecated if the attribute applies cleanly
- remove internal uses of `crate::Error`

If deprecating the module directly proves awkward, retain it with explicit
module docs marking it legacy-only and keep it out of the main public exports.

## Error Shape Rules

- Use `thiserror::Error` for all module-owned error types.
- Prefer structured variants over generic `String` payloads.
- Use `#[from]` to preserve source chains when additional context is not
  needed.
- Use `#[error(transparent)]` only when the parent layer adds no useful
  information.
- Keep `Display` messages short, lowercase, and user-readable.
- Attach path, provider name, endpoint, or operation context as fields when the
  source error alone is ambiguous.

## Data Flow

### Leaf to parent

Examples:

- `oauth2` or `ureq` failure becomes `auth::onedrive::Error`
- `auth::onedrive::Error` becomes `auth::Error` or remains public at the leaf
  boundary
- `storages::onedrive::Error` becomes `storages::Error`
- `storages::Error` becomes `providers::Error`
- `providers::Error` becomes `cli::Error` or `tui::Error`

### Rendering boundary

Formatting for humans stays at the outer edge:

- `cli` decides what to print to stderr
- `tui` decides what to show in status lines, dialogs, or prompts

Deep modules should preserve structure, not write final user-facing text.

## Testing Strategy

### Variant assertions

Tests should assert concrete variants wherever the variant is stable API:

- config read/parse/write failures
- auth token-refresh and device-code classification
- OneDrive invalid-config and HTTP-status failures
- daemon decode and endpoint-validation failures
- provider wrapping behavior

### Source preservation

Modules that wrap third-party errors should test that conversions preserve the
source chain rather than flattening everything into a new string.

### UI rendering

CLI and TUI tests should separate:

- internal typed error assertions
- rendered user-facing error text assertions

Only the outer layer should care about exact output strings.

## Migration Strategy

Apply the migration from the bottom up:

1. `config`
2. `auth`
3. `storages`
4. `daemon`
5. `providers`
6. `cli`
7. `tui`
8. deprecate umbrella exports and run full verification

This sequence minimizes temporary adapter code and lets later modules wrap
earlier typed errors instead of reintroducing `String`.
