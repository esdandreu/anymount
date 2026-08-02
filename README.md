# `anymount`

[![codecov](https://codecov.io/gh/esdandreu/anymount/graph/badge.svg)](https://codecov.io/gh/esdandreu/anymount)

Mount cloud storage as local filesystems using platform-native APIs. Today,
storage providers offer both the storage service and the client software used to
access it. Storage providers have no incentive to build client software that
compresses on device in order to reduce transfer and storage costs. They are not
motivated either to include first-class end-to-end encryption features. The
software layer responsible for interacting with cloud storage should not be
owned by the storage provider, it should be owned by us.


> Working with agents is a lot about making sure the right context and tools are
  available to do the task. If both are covered, agents can get stuff done
  without you interfering. 
  [Ben's Bites - Jul 2,
  2026](https://www.bensbites.com/p/fable-is-back)

Harnesses are optimized for local storage handling. Re-using that interface to
access any storage allows agents to access your data more effectively and more
efficiently.

## Quick Start

### mise-en-place

This project uses [`mise`](https://mise.jdx.dev/getting-started.html) to manage
dev tools, environments and tasks.

### Commands

`connect` ensures configured named drivers are running in the background.
It is non-blocking.

`connect-sync` runs one provider process and blocks for its lifetime. It
supports either a configured named driver or a temporary `temp` driver.

Build and run the application:

```bash
mise run anymount -- connect --all
```

Run one configured driver in the foreground:

```bash
mise run anymount -- connect-sync demo
```

Ephemeral driver (mount path is the first argument after `temp`):

```bash
mise run anymount -- connect-sync temp /mnt/demo local /path/to/data
```

Build the release binary (`target/release/anymount-cli`):

```bash
mise run build
```

## Architecture

`anymount` is organized around three layers.

- `domain` models driver concepts and invariants. It owns driver,
  storage, and telemetry specifications without filesystem, UI, or platform
  code.
- `application` implements use cases such as `connect`, `connect-sync`, `auth`,
  `status`, and config updates. It orchestrates work over domain types and
  internal ports.
- Adapters live at the edges. CLI and TUI handle input and output, `config`
  persists named drivers, `telemetry` builds observability, `auth`
  handles external authorization flows, `service` hosts long-running
  driver processes and control transport, and `drivers` / `storages`
  integrate with platform APIs.

Dependency direction flows inward: frontends call `application`,
`application` works in terms of `domain`, and adapters implement the
external details around those layers.

## Notable dependencies

### Windows
- [**windows-rs**](https://github.com/microsoft/windows-rs) - Official
  Microsoft Rust bindings
- [**cloud-filter**](https://github.com/ho-229/cloud-filter-rs) - Cloud Filter
  API wrapper

## Testing

Run the full suite:

```bash
mise run test
```

## Roadmap

- [ ] Terminal User Interface for mount management. List mounts, connect/disconnect, edit existing mounts and add new ones.
- [ ] Local storage.
- [ ] FUSE driver.
- [ ] Windows' CloudFilter driver.
- [ ] OneDrive storage.
- [ ] MacOS FileProvider driver.
- [ ] S3 storage.
- [ ] Optional mouse support for TUI.

## License

GPL-3.0 - See [LICENSE](LICENSE)
