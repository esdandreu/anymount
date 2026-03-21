# macOS FUSE Driver Design

## Context

`anymount` provides cloud storage as local filesystems using platform-native APIs. Currently, only Linux (FUSE via `libcloudproviders`) and Windows (Cloud Filter API) drivers exist. macOS has no working driver.

This spec adds a cross-platform FUSE driver using the `fuser` crate, enabling macOS support with on-demand reads (no caching).

## Goals

- Add macOS support via FUSE (requires macFUSE)
- Reuse FUSE implementation between Linux and macOS
- Support the generic `Storage` interface (read-only)
- Minimal initial implementation: on-demand reads without caching

## Architecture

### Module Structure

```
drivers/
├── mod.rs           # Re-exports, cfg-gated connect_drivers
├── error.rs         # Driver errors
├── driver.rs        # Driver trait, connect_drivers entry points
├── fuse/            # Shared FUSE implementation
│   ├── mod.rs       # CachePort trait, StorageFilesystem, NoCacheFsCache
│   └── ...
├── linux/           # Linux-specific (DBus, SparseFsCache)
│   ├── mod.rs
│   ├── linux_driver.rs
│   ├── fuse.rs      # Linux StorageFilesystem with SparseFsCache
│   └── ...
└── windows/         # Windows-specific (unchanged)
```

### Key Components

#### `drivers/fuse/mod.rs`

- `CachePort` trait: interface for cache operations
  - `sync_metadata_placeholders(dir, entries)` - sync directory structure
  - `read_range(path, start, end) -> Vec<u8>` - read cached data
  - `write_range(path, start, data, size)` - write data to cache

- `NoCacheFsCache` struct: implements `CachePort` without caching
  - `read_range` returns `Err(CacheRangeNotCached)` always
  - `write_range` returns `Ok(())` without doing anything

- `StorageFilesystem<S, L>`: generic FUSE filesystem
  - Takes any `Storage` implementation
  - Takes any `CachePort` implementation
  - Handles all FUSE operations (lookup, getattr, read, readdir)
  - On-demand reads: if cache miss, reads from storage and attempts cache write

#### `drivers/linux/mod.rs`

- Re-exports from `fuse` module
- Linux-specific `SparseFsCache` implementation (moved from existing fuse.rs)
- Linux-specific mount logic (D-Bus integration, mount path handling)

#### `drivers/linux/linux_driver.rs`

- Imports `StorageFilesystem` from `drivers::fuse`
- Uses `SparseFsCache` from `drivers::linux`

#### `drivers/fuse/macos.rs` (new)

- `MacosDriver` struct implementing `Driver` trait
- `kind()` returns "fuse"
- `path()` returns mount point (from driver spec configuration, same as Linux)
- Uses `StorageFilesystem` with `NoCacheFsCache`

#### `drivers/mod.rs`

- Add `cfg(target_os = "macos")` implementation of `connect_drivers`
- Uses `drivers::fuse::macos::MacosDriver`
- Depends on `fuser` crate (same as Linux)

### Data Flow

1. User runs `anymount provide --name demo`
2. `driver.rs:connect_drivers()` is called (macOS cfg)
3. Creates `Storage` from config (e.g., `LocalStorage`)
4. Creates `StorageFilesystem<LocalStorage, NoCacheFsCache>`
5. Spawns FUSE mount with `fuser::spawn_mount2`
6. Returns `MacosDriver` wrapping the session

### Error Handling

- Mount failures: `Error::FuseMount { path, source }`
- Storage errors: propagated from `Storage::read_file_at`
- Cache errors: logged but non-fatal (no caching)

### Dependencies

Add to `Cargo.toml`:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
fuser = "0.17"
libc = "0.2"
```

## Out of Scope

- Caching (handled by `NoCacheFsCache`)
- D-Bus integration (Linux-only)
- Cloud providers accounting/status (Linux-only)
- Read-write support (Storage interface is read-only)
- Write operations (mkdir, create, delete)
- Hardlinks, symlinks

## Testing

- Unit tests for `NoCacheFsCache`
- Unit tests for `StorageFilesystem` with `NoCacheFsCache`
- Integration test: mount local directory and verify read operations
