# Provider status command design

**Date:** 2026-03-19

**Goal:** Add `anymount status` so users can see which **configured** providers
have a live daemon (control endpoint answers `Ping` with `Ready`).

## Summary

The command lists provider names from the config directory (`ConfigDir::list()`,
same source as `connect --all`). For each name it loads the provider TOML for
display metadata (or prints an **error** bullet with read-failure detail) and,
when read succeeds, probes liveness via the existing per-provider control
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
- Simple **markdown-style bullet list** on stdout, **one entry per configured
  name** from `list()`. Normal case: **`- name (storage type, path): status`**
  where **status** is **running** or **not running** (from control `Ping` →
  `Ready` vs otherwise). Read failure case: **`- name: error — <detail>`** where
  **detail** is a short explanation of why the config read failed (use the
  error’s display text; include enough context to fix the file).
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
3. For each provider name from `list()` (sorted order, as `ConfigDir::list()`
   already guarantees):
   - **Read config** for display fields. If `read()` fails, print one **stdout**
     bullet in the **error** form (**`- name: error — <detail>`**), **continue**
     with the remaining providers (**partial output**). Do **not** abort the
     whole command for one bad TOML. **Do not** send a control `Ping` for that
     name (type/path are unknown; liveness is meaningless for display).
   - On **successful** read:
     - Send `ControlMessage::Ping` to that name’s endpoint.
     - If the reply is `ControlMessage::Ready`, report **running**; else **not
       running** (unreachable socket, wrong reply, etc. treated as down without
       spamming stderr unless `--verbose`).

**Output shape:** each provider is one **stdout** line.

**When config read succeeds** — required format:

```text
- name (storage type, path): status
```

Examples:

```text
- demo (local, /mnt/demo): running
- other (onedrive, /mnt/other): not running
```

**When config read fails** — required form (status is the word **error**, detail
is mandatory):

```text
- name: error — <detail>
```

Example:

```text
- broken: error — failed to read /path/to/broken.toml: invalid TOML at line 2
```

(Exact `<detail>` wording follows the `config::Error` / CLI error display for
that failure; it must be the human-readable reason, not an opaque code.)

**Storage type** labels should match user-facing names used elsewhere (e.g.
`local`, `onedrive`). **Path** is the configured mount path. No column
alignment beyond these single-line patterns.

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
- Test that a failing `read()` for one name still prints bullets for other
  providers and prints **`- name: error — …`** on stdout for the bad one with a
  non-empty detail substring.
