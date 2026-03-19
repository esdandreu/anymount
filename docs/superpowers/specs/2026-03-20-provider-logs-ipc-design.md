# Provider logs over IPC (IPC-only)

**Date:** 2026-03-20

**Status:** Draft

**Goal:** Let users **live-inspect** logs from a running named provider (`provide
--name`) without relying on stdout inheritance or log files. **IPC-only:** no
per-provider log files in this iteration; history is **bounded in-memory** only.

## Summary

Add a **second transport** per provider (Unix domain socket / Windows named
pipe) dedicated to **log streaming**. The existing control endpoint stays
**one-shot request/response** (`Ping`, `Shutdown`, …) so `connect`, `status`,
and `disconnect` never block behind a long-lived `logs` client.

Inside the provider process, tracing (or an equivalent log sink) writes into a
**bounded in-memory ring buffer** and **fans out** to all connected log clients.
CLI command **`anymount logs --name <NAME>`** connects, receives a snapshot of
recent lines (optional), then streams new lines until the user disconnects or
the provider exits.

## Relation to prior work

[`2026-03-18-provider-daemon-design.md`](2026-03-18-provider-daemon-design.md)
defines the control loop and telemetry channel; this spec adds a **parallel**
listener and a **subscriber-facing** log protocol.

[`2026-03-20-provider-disconnect-design.md`](2026-03-20-provider-disconnect-design.md)
requires control IPC to remain responsive; **log traffic must not share the
control accept loop.**

## Non-goals (this iteration)

- Persisted logs, rotation, or `tail` of files.
- Attaching to another process’s OS-level stdout/stderr.
- Reading logs for **inline** `provide` (no `--name` / no endpoints).
- Structured query APIs (e.g. filter by span) beyond what the text format allows.

## Architecture

### Endpoints

- **Control** (unchanged): `provider_endpoint(name)` → existing `.sock` /
  `.pipe` metadata and Windows `\\.\pipe\anymount-{name}`.
- **Logs (new):** derive a sibling path/name from the same validated provider
  name, e.g. Unix bind path alongside control: `{name}.logs.sock` (exact
  filename pattern is an implementation detail; must not collide with control).
  Windows: e.g. `\\.\pipe\anymount-logs-{name}` plus a metadata file if needed
  for discovery (match existing `.pipe` file pattern).

Both listeners are started when a **named** provider starts; both are torn down
when the provider process exits.

### Log bus (in-process)

- **Ring buffer:** fixed bound by **line count** and/or **total bytes** (pick
  constants; document defaults). On overflow, **drop oldest** entries (never
  block the provider on slow clients).
- **Fan-out:** each accepted log client gets either:
  - snapshot of the ring (optional MVP: last *N* lines), then
  - live events until disconnect.
- **Threading:** acceptable approaches include:
  - one **log accept** thread that spawns a **per-client** writer thread, or
  - a single thread using `select`/non-blocking accept (nice-to-have).

Tracing integration: add a `tracing_subscriber::Layer` (or custom `MakeWriter`)
that formats lines consistently and **pushes** into the ring + notifies
subscribers. **Do not** remove file/stdout layers in this spec unless product
requires it; IPC is **additional** visibility for named providers.

### Wire protocol (log stream)

Keep it simple and debuggable with `nc` / manual pipes where possible.

**Recommendation:** **newline-delimited UTF-8**, one log record per line. Each
line is plain text (same as console/file formatting) or a minimal prefix
`LEVEL target: message` — exact format matches whatever the `Layer` emits.

**Optional v1 handshake (if needed):** client sends one line `v1\n` then
shutdown write; server may ignore or use for future versioning. If omitted,
server may begin pushing immediately after accept (snapshot + live).

**Framing:** no length prefix required for v1 if newlines in messages are escaped
or stripped by the formatter (tracing’s default line formatter is typically
single-line safe for simple messages; document the assumption).

**Backpressure:** if a client is slow, **drop** for that client or **disconnect**
it after an outbound buffer limit — do not stall the provider. Document
behavior.

### CLI

- `anymount logs --name <NAME>` — connect to log endpoint, print to stdout until
  SIGINT/EOF.
- Optional `--config-dir` for consistency with other commands (only if needed
  for resolving name; IPC uses **name** only).
- Errors: cannot connect (provider not running), invalid name, unsupported OS.

### Concurrency / correctness

- **Control** and **logs** accept loops are **independent**.
- Multiple **`logs`** clients may connect simultaneously; each receives the same
  stream from the fan-out.
- **Shutdown:** `disconnect` / `Shutdown` must still work while log clients are
  connected; provider exit closes log connections.

## Testing

- **Unit:** ring buffer eviction, fan-out notify, protocol encode/decode if any.
- **Integration (Unix):** spawn a test listener or minimal fake provider that
  binds log socket and emits lines; CLI or client reads expected sequence.
- **Windows:** mirror with named pipe if CI runs Windows; otherwise document
  manual verification.

## Open questions (resolve during implementation)

1. Default ring size (lines vs bytes).
2. Whether to include **initial snapshot** in v1 or only **live** lines after
   connect.
3. Exact **log line format** parity with `logs/anymount.log` / stdout.

## Implementation sketch (non-binding)

1. `daemon::paths::provider_log_endpoint(name)` (+ Windows pipe naming).
2. `LogBus` type: `Arc` inner with mutex/lock-free ring + subscriber list.
3. `spawn_log_server` in `provide` next to `spawn_control_server`.
4. Tracing layer registration in named-provider startup path only.
5. `cli::commands::logs` + `ProviderLogClient` (Unix stream / Windows pipe read
   loop).
6. Docs: README snippet for `anymount logs`.
