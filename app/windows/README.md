# Windows APPX Packaging

Configuration for packaging Anymount as a Windows APPX/MSIX package.

## Structure

- `AppxManifest.xml` - Package manifest with Cloud Files extension
- `Assets/` - App icons

## Building

Build the Rust binary:
```bash
mise run windows:rust
```

Create APPX package:
```bash
mise run windows:appx
```

Install for testing:
```bash
mise run windows:install
```

Run the sync provider:
```bash
mise run windows:run
```

## Requirements

- Windows 10 version 1803 or later
- Developer Mode enabled for testing
- Code signing certificate for production

## Resources

- [Cloud Filter API](https://learn.microsoft.com/en-us/windows/win32/api/_cloudapi/)
- [APPX Manifest Schema](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/appx-package-manifest)

