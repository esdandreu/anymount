# Driver Refactor Design

## Overview

Rename the mount provider abstraction to "driver" throughout the codebase. This reflects that these are platform-specific mount drivers (Windows CloudFilter API, Linux FUSE/D-Bus) rather than generic "providers".

## Changes

### Directory Structure

| Current | New |
|---------|-----|
| `providers/` | `drivers/` |
| `providers/cloudfilter/` | `drivers/windows/` |
| `providers/libcloudprovider/` | `drivers/linux/` |

### Files

| Current | New |
|---------|-----|
| `providers/mod.rs` | `drivers/mod.rs` |
| `providers/provider.rs` | `drivers/driver.rs` |
| `providers/error.rs` | `drivers/error.rs` |
| `providers/cloudfilter/mod.rs` | `drivers/windows/mod.rs` |
| `providers/cloudfilter/provider.rs` | `drivers/windows/windows_driver.rs` |
| `providers/libcloudprovider/mod.rs` | `drivers/linux/mod.rs` |
| `providers/libcloudprovider/provider.rs` | `drivers/linux/linux_driver.rs` |
| `domain/provider.rs` | `domain/driver.rs` |

### Types and Traits

| Current | New |
|---------|-----|
| `Provider` trait | `Driver` trait |
| `CloudFilterProvider` struct | `WindowsDriver` struct |
| `LibCloudProvider` struct | `LinuxDriver` struct |
| `ProviderSpec` struct | `Driver` struct |

### Functions

| Current | New |
|---------|-----|
| `connect_providers` | `connect_drivers` |
| `connect_providers_with_telemetry` | `connect_drivers_with_telemetry` |

### Error Variants

| Current | New |
|---------|-----|
| `DuplicateProvider` | `DuplicateDriver` |
| `InvalidProviderName` | `InvalidDriverName` |
| `SpawnProvider` | `SpawnDriver` |
| `WaitForProvider` | `WaitForDriver` |
| `ProviderExitedBeforeReady` | `DriverExitedBeforeReady` |
| `ProviderDidNotBecomeReady` | `DriverDidNotBecomeReady` |

### Unchanged Types

- `StorageSpec` - storage backend configuration (local, OneDrive)
- `TelemetrySpec` - telemetry configuration
- `OtlpSpec` - OTLP exporter settings
- `OtlpTransport` - OTLP wire transport

These are domain types for configuration, not mount driver abstractions.

## Implementation

1. Create new `drivers/` directory structure
2. Create `drivers/windows/windows_driver.rs` from `providers/cloudfilter/provider.rs`
3. Create `drivers/linux/linux_driver.rs` from `providers/libcloudprovider/provider.rs`
4. Create `drivers/driver.rs` with renamed trait
5. Create `drivers/mod.rs` and `drivers/error.rs`
6. Create `domain/driver.rs` with renamed types
7. Update all imports across the crate
8. Update all doc comments
9. Update variable names (`providers` → `drivers`, etc.)
10. Update public re-exports in `lib.rs`
11. Update CLI error variants
12. Delete old `providers/` directory
13. Update tests
14. Update documentation references

## Notes

- Backward compatibility: config files (`.toml`) use `ProviderSpec` fields which map to `Driver` - no schema changes
- User-facing output (logs, TUI) should use "driver" terminology
- The `kind()` method on `Driver` trait returns `"CloudFilter"` for Windows and `"LibCloudProviders"` for Linux - these could be renamed to `"Windows"` and `"Linux"` but kept as-is for this refactor
