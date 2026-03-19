# Provider status command design

**Date:** 2026-03-19

**Goal:** Add `anymount status` so users can see which **configured** providers
have a live daemon (control endpoint answers `Ping` with `Ready`).

## Summary

The command lists provider names from the config directory (`ConfigDir::list()`,
same source as `connect --all`). For each name it loads the provider TOML for
display metadata and probes liveness via the existing per-provider control
transport (Unix socket / Windows named pipe), using the same semantics as
`connect`’s `is_provider_running` check.

**Discovery scope (approved):** config files only. Do **not** scan the runtime
state directory for orphan `.sock` / `.pipe` files or list endpoints that have no
matching config.

## Relation to prior work

[`2026-03-18-provider-daemon-design.md`](2026-03-18-provider-daemon-design.md)
listed “daemon status or health query commands” as out of scope; this spec adds
that surface in a minimal form.

## Scope

**In scope**

- New CLI subcommand: `anymount status`
- Optional `--config-dir` (consistent with `connect` and `provide`)
- One row per configured provider: at least **name** and **running / not
  running** (wording TBD in implementation; map to control `Ready` vs
  otherwise)
- Display columns from config where cheap: e.g. mount **path** and a short
  **storage kind** (local vs OneDrive, etc.)
- Extract shared liveness probing from `connect` so `status` and `connect`
  cannot drift

**Out of scope**

- Listing inline `provide` sessions (`provide` without `--name` does not install
  a named control endpoint; no status without new machinery)
- Scanning `daemon` state paths for unknown endpoints
- `--strict` exit codes, `--json`, or machine-oriented output (follow-ups if
  needed)

## User-facing behavior

### `anymount status`

1. Resolve config directory (default or `--config-dir`).
2. If there are no `*.toml` providers, print a clear message (e.g. no
   configured providers) and exit successfully.
3. For each provider name from `list()`:
   - Read config for display fields; surface read errors like other CLI
     commands (fail the command or skip that row with error—prefer failing fast
     for corrupt config).
   - Send `ControlMessage::Ping` to that name’s endpoint.
   - If the reply is `ControlMessage::Ready`, mark **running**; else **not
     running** (unreachable socket, wrong reply, etc. treated as down without
     spamming stderr unless `--verbose`).

Platform: reuse the same `cfg` split as `connect` for Unix vs Windows; other
platforms should behave like `connect` (no control transport → not running or
documented limitation).

## Architecture

- **`cli/commands/status.rs`:** argument struct, execution, formatting.
- **`cli/cli.rs` / `cli/commands/mod.rs`:** register subcommand.
- **Shared probe:** move or wrap `is_provider_running` from
  `cli/commands/connect.rs` into a small shared module (e.g. under `cli` or
  `daemon`) used by both `connect` and `status`.

## Testing

- Unit tests for status aggregation/formatting using an injected transport or
  supervisor-style test double (pattern: `RecordingSupervisor` in
  `connect.rs`).
- Tests for empty config dir and for “all stopped” vs “all running” without
  requiring a real socket where possible.

## Open choices (implementation)

- Exact column labels and alignment (stable plain text; no dependency required
  for v1).
- Whether a single failed `read()` aborts the whole command vs partial output
  (default: abort with `cli::Error`).
