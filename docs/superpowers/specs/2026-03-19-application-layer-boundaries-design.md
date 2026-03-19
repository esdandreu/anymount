# Application Layer Boundaries Design

**Date:** 2026-03-19

**Goal:** Introduce an internal `application` layer and a narrow `domain`
layer inside the existing crate, rename `daemon` to `service`, keep
`control` inside `service`, and remove the current adapter-to-adapter
coupling. The resulting structure should make CLI, TUI, config,
telemetry, auth, and platform provider code point inward through one
shared application boundary. The README should also gain a short
architecture section that explains these responsibilities.

## Summary

`anymount` should stay as a single crate for now. The refactor should
create two new internal layers:

- `domain` for provider concepts and invariants
- `application` for use cases and orchestration

Everything else becomes an adapter around those layers:

- `cli` and `tui` are input and output adapters
- `config` is the file-backed repository adapter
- `telemetry` is the OTLP adapter
- `auth` is the OneDrive OAuth adapter
- `service` owns long-lived provider process runtime and control
- `providers` and `storages` stay as platform and external-service adapters

The refactor should not split the workspace yet and should not commit to a
public library API for `application`. `application` remains internal-only
so the module can be reshaped while the boundary settles.

## Scope

This design covers:

- introducing `src/application/`
- introducing `src/domain/`
- renaming `src/daemon/` to `src/service/`
- nesting control transport under `src/service/control/`
- moving provider and telemetry configuration concepts into `domain`
- routing CLI and TUI behavior through `application`
- keeping the top-level adapter module name as `config`
- documenting the new architecture briefly in `README.md`

This design does not yet cover:

- splitting the crate into a workspace
- exposing `application` as a stable public API
- changing user-facing command semantics
- redesigning the provider platform adapters themselves

## User-Facing Behavior

This refactor is structural. Existing user-facing behavior should stay the
same unless a small behavior fix is required to preserve the new
boundaries.

The expected outcomes are architectural rather than visible:

- TUI no longer reaches into CLI commands directly
- telemetry setup no longer depends on parsed CLI types
- config no longer depends on provider-owned traits and config enums
- platform provider code no longer owns top-level orchestration

## Target Layout

The target structure inside `crates/anymount/src` is:

```text
application/
  mod.rs
  auth.rs
  config.rs
  connect.rs
  disconnect.rs
  provide.rs
  status.rs
  types.rs

domain/
  mod.rs
  provider.rs

service/
  mod.rs
  runtime.rs
  control/
    mod.rs
    messages.rs
    paths.rs
    unix.rs
    windows.rs

auth/
cli/
config.rs
providers/
storages/
telemetry/
tui/
```

The exact file count may change during implementation, but the dependency
direction should match this structure even if a few files remain split
differently during migration.

## Dependency Direction

The target dependency direction is:

```text
cli -> application -> domain
tui -> application -> domain

config -> domain
telemetry -> domain
auth -> domain or application request types
service -> domain
providers -> domain
storages -> domain
```

`application` may depend on adapter-facing traits or small adapter wrapper
types, but adapters must not call each other laterally.

The following current edges should be removed:

- `tui -> cli`
- `telemetry -> cli`
- `config -> providers`
- config-backed provider traits crossing from persistence into runtime
- provider runtime setup owning top-level multi-provider orchestration

## Responsibilities

### `domain`

`domain` owns provider concepts and invariants. It should contain pure data
structures and validation rules only.

At minimum, `domain` should own:

- `ProviderSpec`
- `StorageSpec`
- `TelemetrySpec`
- related enums such as OTLP transport or storage kind

`domain` should not know about:

- filesystem layout
- `clap`, `ratatui`, or terminal I/O
- OS control transports
- Cloud Filter, FUSE, D-Bus, or HTTP clients
- tracing or OpenTelemetry SDK setup

`domain` may validate invariants such as:

- provider mount path is required
- OneDrive storage needs token material
- telemetry settings are internally coherent

### `application`

`application` owns use cases. It coordinates repository access, service
control, auth flows, telemetry setup, and provider runtime startup without
owning the concrete adapter implementations.

The initial `application` modules should be:

- `auth`
- `config`
- `connect`
- `disconnect`
- `provide`
- `status`

The layer may use `types.rs` for shared request and response structs, but
the module should not turn into a generic dumping ground. Use-case-local
types should stay in their owning modules unless shared by more than one
use case.

### `cli`

`cli` owns argument parsing, prompt interaction, and line-oriented output.

It should:

- parse `clap` command shapes
- prompt with `inquire` where needed
- format output and map application errors to CLI errors
- call `application` use cases

It should not:

- host provider runtime logic directly
- own control protocol handling
- be called by the TUI

### `tui`

`tui` owns interactive terminal state, rendering, and input handling.

It should:

- manage TUI session state
- render provider lists and edit forms
- call `application` use cases

It should not:

- call `ConnectCommand`
- construct `OneDriveAuthorizer` directly
- load config files directly except through application-facing requests

### `config`

The top-level module name remains `config`.

Its responsibility changes from mixed persistence plus domain ownership to a
file-backed repository adapter over `domain` types.

It should:

- map config files to and from `domain::provider` types
- own config directory paths and file I/O
- expose repository-style operations such as list, read, write, and remove

It should not:

- implement provider runtime traits from the `providers` module
- own the canonical definition of storage or telemetry configuration

Adapter-specific file DTOs may exist temporarily during migration, but they
should become private to `config` once the domain model is established.

### `telemetry`

`telemetry` remains the OTLP adapter.

It should:

- build telemetry handles from domain or application input
- own OpenTelemetry SDK and exporter setup

It should not:

- inspect `Cli`
- match on parsed commands
- load config files directly

### `service`

`service` replaces `daemon` as the name for the long-lived provider host
runtime. The name is less Unix-specific and better matches how the process
is used on all platforms.

`service` should own:

- long-lived provider runtime lifecycle
- shutdown loop and internal runtime messages
- control protocol transport and message handling

`control` stays inside `service` because it is part of the service runtime
boundary, not a top-level application concept.

### `providers`

`providers` remains the platform provider adapter layer.

It should:

- connect one resolved provider runtime for a platform
- return runtime handles needed by the service host

It should not:

- own domain configuration types
- own config collection traits
- iterate configured providers from persistence
- build top-level CLI or TUI behavior

The current `ProviderConfiguration` and `ProvidersConfiguration` traits
should be removed or reduced away. Platform provider code should receive a
resolved domain type rather than a persistence-shaped trait object.

### `storages`

`storages` remains the storage backend adapter layer.

It should:

- translate domain storage configuration into backend-specific clients
- expose storage operations to provider adapters

It should not:

- own the canonical public storage configuration model

## Internal Ports

The first pass should introduce only the ports needed to stop lateral
adapter coupling. Avoid a large generic port hierarchy.

The minimum useful ports are:

- `ProviderSpecRepository`
- `ProviderServiceControl`
- `ProviderServiceLauncher`
- `ProviderRuntimeHost`
- `AuthFlow`
- `TelemetryFactory`

These names are internal and may evolve. The important part is their role:

- repository access for named provider specs
- probe and shutdown control for running services
- background launch of named provider services
- in-process hosting of one resolved provider service
- auth flow orchestration for OneDrive
- telemetry handle construction from resolved settings

## Use-Case Boundaries

### `application::connect`

`connect` should:

1. resolve target names
2. probe service readiness
3. launch missing services
4. aggregate failures

It should not know how command-line parsing worked, and it should not build
`std::process::Command` directly without going through a launcher seam.

### `application::provide`

`provide` should:

1. resolve a named or inline provider spec
2. resolve telemetry configuration
3. start one provider service host
4. run until shutdown

It becomes the main orchestration point for named or inline provider
hosting, but the transport details stay under `service` and the platform
mount details stay under `providers`.

### `application::config`

`config` should own add, set, remove, show, and list use cases over
`ProviderSpec` values.

The TUI and CLI should share these operations instead of reimplementing
config editing behavior in separate adapters.

### `application::auth`

`auth` should wrap the OneDrive device flow behind application-facing
requests and responses. CLI and TUI may present the flow differently, but
they should not construct concrete auth adapters themselves.

### `application::status`

`status` should aggregate configured providers with service readiness
information. Formatting remains adapter-specific.

### `application::disconnect`

`disconnect` should own shutdown orchestration over named services and
aggregate failures, while CLI and TUI remain responsible for user
interaction and output.

## README Documentation

`README.md` should gain a short `Architecture` section after the design is
implemented.

That section should explain:

- `domain` as provider concepts and invariants
- `application` as use-case orchestration
- adapters as CLI, TUI, config, telemetry, auth, service, providers, and
  storages
- the dependency direction from frontends inward

The README should stay brief. The detailed ownership map belongs in this
design document, not in the top-level project introduction.

## Migration Plan

Implementation should proceed in this order:

1. add `domain` and move provider and telemetry config concepts there
2. rename `daemon` to `service`
3. move control transport under `service/control`
4. add `application` modules as thin use-case facades
5. switch CLI commands to call `application`
6. switch TUI to call `application`
7. decouple `telemetry` from `Cli`
8. narrow `providers` to one-provider runtime connection responsibilities
9. update `README.md` with the short architecture section

This order allows the refactor to land incrementally while keeping the
crate single and minimizing behavior drift.

## Error Handling

The refactor should preserve module-specific errors. New boundaries should
prefer translation at adapter edges instead of pulling outer-layer errors
inward.

Key rules:

- `domain` validation errors stay domain-local
- `application` aggregates use-case failures in application terms
- adapters translate infrastructure failures into their local error types
- frontends format errors for users without leaking transport details when
  not useful

## Testing Strategy

### Domain tests

Add pure tests for provider and telemetry invariants in `domain`.

### Application tests

Use fake ports to test:

- connect orchestration
- provide resolution and launch flow
- status aggregation
- config use cases
- disconnect failure aggregation

### Adapter tests

Keep thin adapter-focused tests for:

- file-backed config repository behavior
- service control transport behavior
- telemetry builder behavior
- platform provider integration seams

The application layer should absorb most orchestration tests now found in
CLI and TUI modules.

## Acceptance Criteria

This design is complete when the implementation achieves the following:

- `application` exists and is used by both CLI and TUI
- `domain` owns provider and telemetry configuration concepts
- `service` replaces `daemon`
- `service/control` owns control messages, paths, and platform transport
- `telemetry` no longer depends on parsed CLI command types
- `config` no longer depends on provider-owned configuration traits
- TUI no longer imports CLI command implementations directly
- `README.md` documents the architecture briefly and accurately
