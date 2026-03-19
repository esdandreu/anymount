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
