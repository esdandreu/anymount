# `anymount`

[![codecov](https://codecov.io/gh/esdandreu/anymount/graph/badge.svg)](https://codecov.io/gh/esdandreu/anymount)

Mount cloud storage as local filesystems using platform-native APIs.

## Quick Start

### mise-en-place

This project uses [`mise`](https://mise.jdx.dev/getting-started.html) to manage
dev tools, environments and tasks.

### Commands

`connect` ensures configured named providers are running in the background.
It is non-blocking.

`provide` runs one provider process and blocks for its lifetime. It supports
either a named provider from config or an inline unnamed provider.

Build and run the application:

```bash
mise run anymount -- connect --all
```

Run one provider process directly:

```bash
mise run anymount -- provide --name demo
```

Build the release binary (`target/release/anymount-cli`):

```bash
mise run build
```

## Architecture

`anymount` is organized around three layers.

- `domain` models provider concepts and invariants. It owns provider,
  storage, and telemetry specifications without filesystem, UI, or platform
  code.
- `application` implements use cases such as `connect`, `provide`, `auth`,
  `status`, and config updates. It orchestrates work over domain types and
  internal ports.
- Adapters live at the edges. CLI and TUI handle input and output, `config`
  persists named providers, `telemetry` builds observability, `auth`
  handles external authorization flows, `service` hosts long-running
  provider processes and control transport, and `providers` / `storages`
  integrate with platform APIs.

Dependency direction flows inward: frontends call `application`,
`application` works in terms of `domain`, and adapters implement the
external details around those layers.

## Notable dependencies

### Windows
- [**windows-rs**](https://github.com/microsoft/windows-rs) - Official Microsoft Rust bindings
- [**cloud-filter**](https://github.com/ho-229/cloud-filter-rs) - Cloud Filter API wrapper

## Testing

Run the full suite:

```bash
mise run test
```

## License

GPL-3.0 - See [LICENSE](LICENSE)
