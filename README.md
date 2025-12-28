# `anymount`

Mount cloud storage as local filesystems using platform-native APIs.

## Architecture

Anymount uses a modular architecture with platform-specific implementations:

- **`anymount-core`** - Cross-platform abstractions and `StorageProvider` trait
- **`anymount-mock`** - Mock storage backend for testing and demonstration
- **`anymount-macos`** - macOS FileProvider extension integration
- **`anymount-windows`** - Windows Cloud Filter API integration
- **`anymount-cli`** - Command-line interface

## Quick Start

### mise-en-place

This project uses [`mise`](https://mise.jdx.dev/getting-started.html) to manage
dev tools, environments and tasks.

### macOS FileProvider

Build and run the FileProvider extension:

```bash
mise run macos:build
mise run macos:run
```

**⚠️ Important**: FileProvider extensions require a **paid Apple Developer Program** ($99/year) membership to run. The app will build successfully but fail at runtime with "The application cannot be used right now" without proper entitlements from Apple.

See `app/macos/` for the macOS FileProvider implementation.

### Windows Cloud Filter API

Build the application:

```bash
mise run windows:rust
```

Run the sync provider:

```bash
mise run windows:run
```

Or package as APPX for distribution:

```bash
mise run windows:appx
mise run windows:install
```

**⚠️ Important**: Windows Cloud Filter API requires:
- Windows 10 version 1803 (build 17134) or later
- Administrator privileges for sync root registration
- Enable Developer Mode for testing unsigned packages

#### Unregistering the Sync Root

To cleanly stop and unregister:

```bash
target\release\anymount-win.exe unregister
```

If unregistration fails with error `0x8007017C` (active connection):

1. Close all File Explorer windows showing the sync root
2. Run the registry cleanup script:
   ```powershell
   .\clean_registry.ps1 -dryRun:$false
   ```
3. If the issue persists, restart your computer

Or use the force-unregister command for detailed instructions:

```bash
target\release\anymount-win.exe force-unregister
```

## Project Status

- 🔨 **macOS FileProvider** - Complete implementation, requires Apple Developer Program
- 🚧 **Windows Cloud Filter API** - Callbacks implemented and working, triggered on folder access
- ⬜ **Linux FUSE** - Planned

## Technologies

### Windows
- [**windows-rs**](https://github.com/microsoft/windows-rs) - Official Microsoft Rust bindings
- [**cloud-filter**](https://docs.rs/cloud-filter) - Cloud Filter API wrapper

### macOS
- **FileProvider** framework - Native file system integration
- **XPC Services** - Inter-process communication

## License

GPL-3.0 - See [LICENSE](LICENSE)
