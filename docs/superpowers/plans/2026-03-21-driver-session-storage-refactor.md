# Driver-Session-Storage Refactoring Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor Driver trait to have a `connect` method returning an opaque `Session`. When `Session` drops, connection drops. Driver receives `Storage` as parameter. Add `storage::new` method returning `impl Storage` from `StorageConfig`.

**Architecture:** This is a Ports & Adapters (Hexagonal) refactoring. Driver becomes the port that receives a Storage adapter. Session encapsulates the active mount/connection lifecycle. The `storage::new` function is a factory that produces concrete Storage implementations from domain `StorageConfig`.

**Tech Stack:** Rust, trait objects (`dyn`), `Box<dyn Session>`

---

## File Structure

```
crates/anymount/src/
├── drivers/
│   ├── driver.rs         # MODIFY: Refactor Driver trait, connect function
│   ├── windows/
│   │   └── windows_driver.rs  # MODIFY: Implement Session for WindowsDriver
│   └── linux/
│       └── linux_driver.rs    # MODIFY: Implement Session for LinuxDriver
├── storages/
│   ├── mod.rs            # MODIFY: Add storage::new function
│   └── ...
└── domain/
    └── driver.rs         # MODIFY: Add Session trait to domain (optional)
```

---

## Task 1: Add Session Trait and Refactor Driver Trait

**Files:**
- Modify: `crates/anymount/src/drivers/driver.rs:10-13`

- [ ] **Step 1: Read current Driver trait**

The current Driver trait is:
```rust
pub trait Driver {
    fn kind(&self) -> &'static str;
    fn path(&self) -> &PathBuf;
}
```

- [ ] **Step 2: Add Session trait**

```rust
pub trait Session: Send + Sync + 'static {
    fn path(&self) -> &PathBuf;
    fn kind(&self) -> &'static str;
}
```

- [ ] **Step 3: Refactor Driver trait to have connect method**

```rust
pub trait Driver {
    fn connect(storage: impl Storage, path: PathBuf, logger: Logger) -> Result<Box<dyn Session>>;
}
```

- [ ] **Step 4: Remove kind() and path() from Driver (they move to Session)**

- [ ] **Step 5: Update Box<dyn Driver> usage in connect_drivers functions**

Update `connect_drivers` functions to call `driver.connect(storage, path, logger)` instead of creating drivers directly.

- [ ] **Step 6: Commit**

```bash
git add crates/anymount/src/drivers/driver.rs
git commit -m "refactor: add Session trait and refactor Driver.connect"
```

---

## Task 2: Implement Session for WindowsDriver

**Files:**
- Modify: `crates/anymount/src/drivers/windows/windows_driver.rs:1-112`

- [ ] **Step 1: Read WindowsDriver implementation**

- [ ] **Step 2: Create WindowsSession struct**

```rust
pub struct WindowsSession {
    path: PathBuf,
    #[allow(dead_code)]
    id: SyncRootId,
    #[allow(dead_code)]
    connection: Option<Connection<super::Callbacks<S, L>>>,
}
```

- [ ] **Step 3: Implement Session for WindowsSession**

```rust
impl Session for WindowsSession {
    fn path(&self) -> &PathBuf {
        &self.path
    }
    
    fn kind(&self) -> &'static str {
        "CloudFilter"
    }
}
```

- [ ] **Step 4: Refactor WindowsDriver::connect to return Box<dyn Session>**

The current `WindowsDriver::connect` returns `Result<Arc<Self>>`. Change it to:
1. Take `storage: impl Storage` instead of storing it in struct
2. Return `Result<Box<dyn Session>>` wrapping `WindowsSession`

- [ ] **Step 5: Update Driver impl**

Update the `Driver for Arc<WindowsDriver<S, L>>` impl to just be `WindowsDriver` with `connect` static method.

- [ ] **Step 6: Commit**

```bash
git add crates/anymount/src/drivers/windows/windows_driver.rs
git commit -m "refactor: implement Session for WindowsDriver"
```

---

## Task 3: Implement Session for LinuxDriver

**Files:**
- Modify: `crates/anymount/src/drivers/linux/linux_driver.rs:1-199`

- [ ] **Step 1: Read LinuxDriver implementation**

- [ ] **Step 2: Create LinuxSession struct**

```rust
pub struct LinuxSession {
    path: PathBuf,
    _session: fuser::BackgroundSession,
}
```

- [ ] **Step 3: Implement Session for LinuxSession**

```rust
impl Session for LinuxSession {
    fn path(&self) -> &PathBuf {
        &self.path
    }
    
    fn kind(&self) -> &'static str {
        "LibCloudProviders"
    }
}
```

- [ ] **Step 4: Refactor LinuxDriver**

Change `LinuxDriver` to just have a static `connect` method that:
1. Takes `storage: impl Storage`, `path: PathBuf`, `logger: impl Logger`
2. Returns `Result<Box<dyn Session>>` wrapping `LinuxSession`

- [ ] **Step 5: Update Driver trait impl**

Remove the `Driver for LinuxDriver` impl and replace with the new static `connect` pattern.

- [ ] **Step 6: Commit**

```bash
git add crates/anymount/src/drivers/linux/linux_driver.rs
git commit -m "refactor: implement Session for LinuxDriver"
```

---

## Task 4: Implement Session for FuseDriver

**Files:**
- Modify: `crates/anymount/src/drivers/driver.rs:256-280`

- [ ] **Step 1: Create FuseSession struct**

```rust
#[cfg(feature = "fuse")]
pub struct FuseSession {
    path: PathBuf,
    _session: fuser::BackgroundSession,
}
```

- [ ] **Step 2: Implement Session for FuseSession**

```rust
#[cfg(feature = "fuse")]
impl Session for FuseSession {
    fn path(&self) -> &PathBuf {
        &self.path
    }
    
    fn kind(&self) -> &'static str {
        "macos"
    }
}
```

- [ ] **Step 3: Refactor FuseDriver to FuseDriver connect method**

Rename `FuseDriver` struct to `FuseDriver` struct with static `connect` method.

- [ ] **Step 4: Commit**

```bash
git add crates/anymount/src/drivers/driver.rs
git commit -m "refactor: implement Session for FuseDriver"
```

---

## Task 5: Add storage::new Function

**Files:**
- Modify: `crates/anymount/src/storages/mod.rs`
- Modify: `crates/anymount/src/storages/local.rs`
- Modify: `crates/anymount/src/storages/onedrive.rs`
- Modify: `crates/anymount/src/domain/driver.rs`

- [ ] **Step 1: Read current StorageConfig in domain/driver.rs**

- [ ] **Step 2: Add storage::new function to mod.rs**

```rust
use crate::domain::driver::StorageConfig;

pub fn new(config: StorageConfig) -> storages::Result<impl Storage> {
    match config {
        StorageConfig::Local { root } => Ok(LocalStorage::new(root)),
        StorageConfig::OneDrive { root, endpoint, access_token, refresh_token, client_id, token_expiry_buffer_secs } => {
            let config = OneDriveConfig {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            };
            config.connect()
        }
    }
}
```

Note: This requires adding `StorageConfig` import path and the function returns `impl Storage` with the concrete error type.

- [ ] **Step 3: Update storages/mod.rs exports**

```rust
pub use storage::new;
```

- [ ] **Step 4: Update connect_drivers to use storage::new**

Replace the match on StorageConfig + manual creation with:
```rust
let storage = storages::new(spec.storage.clone())?;
```

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/storages/mod.rs
git commit -m "feat: add storage::new factory function"
```

---

## Task 6: Update connect_drivers Functions

**Files:**
- Modify: `crates/anymount/src/drivers/driver.rs`

- [ ] **Step 1: Update Windows connect_drivers**

Replace the match block with `storage::new(spec.storage.clone())?` and call `WindowsDriver::connect(storage, spec.path.clone(), ...)`.

- [ ] **Step 2: Update Linux connect_drivers**

Replace the match block with `storage::new(spec.storage.clone())?` and call `LinuxDriver::connect(storage, spec.path.clone(), ...)`.

- [ ] **Step 3: Update Fuse connect_drivers**

Replace the match block with `storage::new(spec.storage.clone())?` and call `FuseDriver::connect(storage, spec.path.clone(), ...)`.

- [ ] **Step 4: Update return type**

Change `Result<Vec<Box<dyn Driver>>>` to `Result<Vec<Box<dyn Session>>>` since we now return Sessions.

- [ ] **Step 5: Update tests**

Update any tests that use `Box<dyn Driver>` to use `Box<dyn Session>`.

- [ ] **Step 6: Commit**

```bash
git add crates/anymount/src/drivers/driver.rs
git commit -m "refactor: update connect_drivers to use new Session pattern"
```

---

## Task 7: Update Consumers of connect_drivers

**Files:**
- Search for all usages of `connect_drivers` in the codebase

- [ ] **Step 1: Find all consumers**

```bash
grep -r "connect_drivers" --include="*.rs" crates/
```

- [ ] **Step 2: Update return type handling**

Update any code that handles `Vec<Box<dyn Driver>>` to handle `Vec<Box<dyn Session>>`.

- [ ] **Step 3: Update method calls from driver.kind() to session.kind()**

- [ ] **Step 4: Update method calls from driver.path() to session.path()**

- [ ] **Step 5: Commit**

```bash
git add <updated-files>
git commit -m "refactor: update consumers to use Session"
```

---

## Task 8: Verify and Run Tests

- [ ] **Step 1: Run cargo check**

```bash
cd crates/anymount && cargo check
```

- [ ] **Step 2: Run tests**

```bash
cd crates/anymount && cargo test
```

- [ ] **Step 3: Fix any compilation or test errors**

- [ ] **Step 4: Final commit**

```bash
git add -A && git commit -m "chore: verify refactoring complete"
```

---

## Verification Checklist

- [ ] `Driver` trait has `connect(storage, path, logger) -> Result<Box<dyn Session>>`
- [ ] `Session` trait has `path()` and `kind()` methods
- [ ] `WindowsSession`, `LinuxSession`, `FuseSession` implement `Session`
- [ ] When `Session` is dropped, connection/mount is terminated
- [ ] `storage::new(StorageConfig) -> Result<impl Storage>` exists
- [ ] `connect_drivers` returns `Vec<Box<dyn Session>>`
- [ ] All tests pass
- [ ] All consumers updated
