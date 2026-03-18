# Provider Daemon Design

**Date:** 2026-03-18

**Goal:** Make `connect` ensure one long-lived background provider process
exists per configured provider, reusing running processes and leaving provider
session ownership to `anymount provide --name <provider>`.

## Summary

`connect` becomes an idempotent supervisor command. For each configured
provider name, it derives that provider's control endpoint, pings it, and
either reuses the running process or launches `anymount provide --name
<provider>`.

`provide --name <provider>` becomes the long-lived per-provider process. It
loads one provider config, connects the existing platform-specific provider
runtime, then enters a consumer loop that listens for control messages from
local IPC and telemetry messages sent by callbacks from OS-owned threads.

## Scope

This design covers:

- persistent per-provider background processes
- cross-platform liveness and shutdown control
- daemon-owned provider session lifecycle
- callback-to-daemon telemetry delivery
- `connect --name` and `connect --all` behavior

This design does not yet cover:

- a full `disconnect` command UX
- callback work execution inside the daemon beyond telemetry logging
- daemon status or health query commands

## User-Facing Behavior

### `connect --name <provider>`

`connect` checks whether the provider's control endpoint is reachable.

- If the endpoint answers, `connect` returns success without relaunching the
  provider.
- If the endpoint is missing or stale, `connect` launches `anymount provide
  --name <provider>` and waits for a ready handshake.
- If startup fails, `connect` returns an error for that provider.

### `connect --all`

`connect` repeats the same flow for every configured provider.

- Already-running providers are reused.
- Missing providers are started.
- If one or more providers fail, the command returns an overall error.
- Providers that were already running or started successfully remain running.

### Provider lifetime

Each provider process is persistent and stops only when explicitly
disconnected. Exiting the `connect` command does not stop provider processes.

## Architecture

### CLI supervisor

`connect` is a client and supervisor only. It does not own provider sessions
after this change.

Responsibilities:

- resolve target provider names from config
- derive per-provider IPC endpoint locations
- probe existing provider processes
- spawn `provide --name <provider>` when needed
- wait for ready or error handshakes
- aggregate failures across `--all`

### Provider runtime

`provide --name <provider>` owns exactly one configured provider instance.

Responsibilities:

- load provider config by config name
- initialize logging and message channels
- connect the provider using existing platform-specific code
- expose a control endpoint for liveness and shutdown
- run the daemon loop until shutdown

### Control transport

The source of truth for liveness is a successful IPC handshake, not process
inspection and not file existence.

- Linux/macOS: Unix domain socket
- Windows: named pipe

The endpoint name is derived from the provider config name, which is the
human-readable unique identifier for the provider.

An optional metadata file may exist for debugging or endpoint discovery, but
it is not the liveness source of truth.

### Internal daemon messaging

The daemon loop receives two categories of messages:

- control messages from IPC clients
- telemetry messages from provider callbacks

For v1, a thread-safe fan-in channel such as `std::sync::mpsc` is sufficient
for callback telemetry. Callback code may run on OS-owned threads, so the
sender must be safe to clone and send from arbitrary threads.

## Message Model

### IPC protocol

The external control protocol should stay minimal:

- `Ping`
- `Ready`
- `Shutdown`
- `Ack`
- `Error`

`connect` uses `Ping` and `Ready`/`Error`.

Future disconnect commands will use `Shutdown` and `Ack`.

### Internal daemon messages

The initial internal daemon loop can be modeled with:

- `Shutdown`
- `Telemetry { ... }`

The daemon serializes telemetry to stdout logging and handles shutdown in one
place.

## Data Flow

### Start or reuse

1. `connect` resolves the provider config name.
2. `connect` derives the provider's control endpoint.
3. `connect` attempts a handshake.
4. If the endpoint responds, the provider is considered running.
5. If the endpoint does not respond, `connect` launches `anymount provide
   --name <provider>`.
6. The launched process loads config, connects the provider, binds the control
   endpoint, and reports `Ready`.
7. `connect` continues to the next provider or returns success.

### Daemon runtime

1. `provide` loads one provider config by name.
2. `provide` creates the internal message channel.
3. `provide` passes callback-safe sender handles into provider runtime code.
4. `provide` connects the platform-specific provider implementation.
5. `provide` binds the control endpoint and publishes readiness.
6. `provide` enters a loop waiting for control or telemetry messages.
7. Callback-originated telemetry is printed to stdout by the daemon loop.
8. On shutdown, the daemon drops the provider session, removes its endpoint,
   and exits.

## Callback Telemetry

Callback code should not own the final logging policy. Instead, callbacks send
structured telemetry into the daemon loop.

This provides:

- a single serialization point for logs
- a safe bridge from OS-owned callback threads into daemon-owned logging
- a clean place to add future metrics or status handling

If telemetry enqueueing fails because the receiver is already gone, the
callback should fail closed. Best-effort fallback logging is acceptable, but
telemetry failure must not crash the provider runtime.

## Error Handling

### Startup failures

There are three main startup failure points:

- `connect` cannot launch `provide`
- `provide` cannot load provider config
- `provide` cannot connect the provider session

If startup reaches the IPC layer, `provide` should report `Error` before
exiting whenever practical.

### Stale endpoints

If an endpoint exists but does not answer within a short timeout, `connect`
should treat it as stale. A successful handshake remains the only proof that a
provider is alive and usable.

### Partial success

`connect --all` returns an overall error if one or more providers fail, but it
does not stop providers that were already running or that started
successfully.

## Testing Strategy

### CLI supervisor tests

Add tests for:

- reusing an already-running provider without relaunch
- launching a missing provider and waiting for readiness
- `connect --all` returning an error when one provider fails while leaving
  successful providers alone

### Daemon loop tests

Test the daemon loop independently of transport and provider implementations:

- multiple telemetry senders can feed the loop
- telemetry is serialized to logging output
- shutdown exits the loop cleanly
- provider startup failure never reports ready

### Transport tests

Keep most transport tests behind a platform-agnostic interface. Add thin
per-platform integration tests only for bind/connect/handshake semantics.

## Implementation Notes

- Use provider config name as the stable identifier everywhere.
- Keep provider-specific Cloud Filter or FUSE callback behavior in existing
  platform code.
- Introduce the daemon runtime around the provider connection rather than
  pushing provider logic into the CLI.
- Keep the initial protocol small and defer status queries or richer command
  routing until there is a concrete need.
