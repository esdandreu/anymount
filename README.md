# `anymount`

Mount cloud storage as local filesystems using platform-native APIs.

## Quick Start

### macOS FileProvider

Build and run the FileProvider extension:

```bash
mise run build
```
```bash
mise run run
```

**⚠️ Important**: FileProvider extensions require a **paid Apple Developer Program** ($99/year) membership to run. The app will build successfully but fail at runtime with "The application cannot be used right now" without proper entitlements from Apple.

See `app/macos/` for the macOS FileProvider implementation.

## Project Status

- 🔨 **macOS FileProvider** - Complete implementation, requires Apple Developer Program
- ⬜ **Windows CloudFilterAPI** - Planned
- ⬜ **Linux FUSE** - Planned

## License

GPL-3.0 - See [LICENSE](LICENSE)
