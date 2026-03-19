# Provider disconnect command design

**Date:** 2026-03-20

**Goal:** Add `anymount disconnect` so users can stop background provider daemons
cleanly via the existing control IPC (`Shutdown` → `Ack`), with **idempotent**
semantics and **`ConfigDir::each_provider()`** for `--all`.

## Summary

The command targets providers by **config name** (same identity as `connect`,
`status`, and the per-provider control socket/pipe). For each target name it
attempts an orderly shutdown: when the daemon is **reachable** and **ready**,
send `ControlMessage::Shutdown` and require `ControlMessage::Ack`. When the
daemon is **already stopped** (no endpoint, ping not `Ready`, etc.), the
operation **succeeds with no error** — disconnect is **idempotent**.

For **`disconnect --all`**, walk configuration using **`ConfigDir::each_provider()`**
(not `list()` + separate reads): the iterator yields `(name, load_result)` in
sorted name order. **Disconnect always uses `name` for IPC.** A failed config
read does **not** skip shutdown for that name; loading the TOML is irrelevant to
control transport, but the iterator is still the single canonical way to visit
every configured provider entry (including broken files) without duplicating
directory logic.

## Relation to prior work

[`2026-03-18-provider-daemon-design.md`](2026-03-18-provider-daemon-design.md)
defines `Shutdown` / `Ack` for future disconnect; the Unix `provide` control
server already implements this path.

[`2026-03-19-provider-status-design.md`](2026-03-19-provider-status-design.md)
shares config scope and naming; `disconnect` complements `status` (stop vs
observe).

## Scope

**In scope**

- CLI: `anymount disconnect --name <NAME>`, `anymount disconnect --all`,
  optional `--config-dir` (same pattern as `connect` / `status`).
- **`--all`:** iterate with **`each_provider()`**; for each `(name, _load_result)`
  run the same per-name disconnect logic using **`name` only**.
- **Idempotent per name:**
  - Control endpoint **unreachable** (I/O error connecting, etc.) → **success**
    (treat as already stopped).
  - **`Ping` reply is not `Ready`** (including non-ready control replies) →
    **success** (not running as a daemon we manage).
  - **`Ping` → `Ready`:** send **`Shutdown`**; reply **`Ack`** → **success**.
  - **`Ping` → `Ready`**, then shutdown path returns **`Error`**, wrong reply,
    or I/O failure → **failure** for that provider (daemon did not acknowledge
    clean shutdown as specified).
- **`--all`:** continue after per-name successes; aggregate **failures** into a
  single CLI error if any name failed (same spirit as `connect --all`).
- Shared control client helper (extend or complement `cli::provider_control`) for
  `Ping` / `Shutdown` round-trips on Unix.

**Out of scope**

- Non-config “orphan” processes (no `each_provider()` row).
- Windows until `WindowsControl` supports the same protocol (command may return
  **not supported** with a clear message).
- `disconnect` for inline `provide` without `--name` (no named endpoint).

## User-facing behavior

### `disconnect --name <NAME>`

1. Resolve config directory (default or `--config-dir`).
2. Run **idempotent disconnect** for `NAME` (IPC only; **do not** require a
   successful config `read` for that name).

### `disconnect --all`

1. Resolve config directory.
2. Call **`each_provider()`**. If the outer `Result` fails (cannot list
   directory), return that error.
3. For **each** `(name, _)` from the iterator (including pairs where the inner
   load failed), run **idempotent disconnect** for `name`.
4. If one or more names failed the non-idempotent shutdown path, return an
   aggregated error; otherwise success.

### No target

If neither `--name` nor `--all` is given, return an error (mirror
`MissingConnectTarget` style).

### Empty configuration

`each_provider()` yields nothing → success, no output lines required (optional
single info line under `--verbose` only).

## Architecture

- **`cli/commands/disconnect.rs`:** args, `execute`, `_execute` with injectable
  control client for tests (same discipline as `connect` / `status`).
- **`cli::provider_control` (or adjacent module):** functions such as
  idempotent disconnect by name, built on `UnixControl::send` and
  `ControlMessage` variants.
- **Tests:** unit tests for idempotent branches (unreachable endpoint, not
  `Ready`, `Ack` after shutdown); integration-style test with local Unix listener
  if practical.

## Idempotency (normative)

| Situation | Outcome |
|-----------|---------|
| Cannot connect to control endpoint | Success |
| `Ping` not `Ready` | Success |
| `Ping` `Ready`, `Shutdown` → `Ack` | Success |
| `Ping` `Ready`, then shutdown fails or non-`Ack` | Failure for that name |

Repeated `disconnect` on a stopped provider must **always** succeed.
