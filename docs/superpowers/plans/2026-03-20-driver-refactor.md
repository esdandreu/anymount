# Driver Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename all provider-related types and modules to driver throughout the codebase.

**Architecture:** Create a new `drivers/` directory structure mirroring the old `providers/`, rename types, then update all imports.

**Tech Stack:** Rust (anymount crate)

---

## File Structure

```
crates/anymount/src/
├── drivers/                    # NEW (replaces providers/)
│   ├── mod.rs                 # from providers/mod.rs
│   ├── error.rs               # from providers/error.rs
│   ├── driver.rs              # from providers/provider.rs (renamed)
│   ├── windows/
│   │   ├── mod.rs             # from providers/cloudfilter/mod.rs
│   │   ├── windows_driver.rs  # from providers/cloudfilter/provider.rs
│   │   ├── callbacks.rs       # from providers/cloudfilter/callbacks.rs
│   │   ├── cleanup_registry.rs # from providers/cloudfilter/cleanup_registry.rs
│   │   ├── error.rs           # from providers/cloudfilter/error.rs
│   │   ├── placeholders.rs    # from providers/cloudfilter/placeholders.rs
│   │   └── register.rs        # from providers/cloudfilter/register.rs
│   └── linux/
│       ├── mod.rs             # from providers/libcloudprovider/mod.rs
│       ├── linux_driver.rs   # from providers/libcloudprovider/provider.rs
│       ├── dbus.rs            # from providers/libcloudprovider/dbus.rs
│       ├── error.rs           # from providers/libcloudprovider/error.rs
│       ├── fuse.rs            # from providers/libcloudprovider/fuse.rs
│       └── gtk_dbus.rs        # from providers/libcloudprovider/gtk_dbus.rs
├── domain/
│   ├── driver.rs              # from domain/provider.rs (renamed)
│   └── mod.rs
├── lib.rs                     # UPDATE: re-exports from drivers/
├── cli/error.rs               # UPDATE: error variants
├── service/error.rs           # UPDATE: error variants
├── application/config.rs      # UPDATE: DuplicateProvider -> DuplicateDriver
├── tui/error.rs               # UPDATE: DuplicateProvider reference
├── telemetry/mod.rs           # UPDATE: ProviderSpec -> Driver
├── cli/commands/provide.rs    # UPDATE: Provider -> Driver, ProviderSpec -> Driver
├── cli/commands/connect.rs    # UPDATE: error variants
├── cli/commands/config.rs      # UPDATE: doc comments (ProviderType enum kept as-is)
└── tests/system/local_provider_test.rs  # UPDATE: comments

DELETE:
- crates/anymount/src/providers/ (entire directory)
- crates/anymount/src/domain/provider.rs
```

---

## Tasks

### Task 1: Create `drivers/` directory structure

**Files:**
- Create: `crates/anymount/src/drivers/`
- Create: `crates/anymount/src/drivers/windows/`
- Create: `crates/anymount/src/drivers/linux/`

- [ ] **Step 1: Create directories**

```bash
mkdir -p crates/anymount/src/drivers/windows
mkdir -p crates/anymount/src/drivers/linux
```

- [ ] **Step 2: Commit**

```bash
git add -A && git commit -m "chore: create drivers/ directory structure"
```

---

### Task 2: Create `drivers/windows/windows_driver.rs` (from `providers/cloudfilter/provider.rs`)

**Files:**
- Create: `crates/anymount/src/drivers/windows/windows_driver.rs`

**Changes:**
- `CloudFilterProvider` → `WindowsDriver`
- `Provider` → `Driver`
- Keep `"CloudFilter"` as return value (matches spec)

- [ ] **Step 1: Create `windows_driver.rs`**

```rust
use super::{Error, Driver, Result, Storage};
use crate::service::control::messages::ServiceMessage;
use crate::Logger;
use cloud_filter::root::{
    Connection, HydrationType, PopulationType, SecurityId, Session, SyncRootId, SyncRootIdBuilder,
    SyncRootInfo,
};
use std::path::{absolute, PathBuf};
use std::sync::{mpsc::Sender, Arc};

pub const ID_PREFIX: &'static str = "Anymount";

pub struct WindowsDriver<S: Storage, L: Logger> {
    path: PathBuf,
    #[allow(dead_code)]
    id: SyncRootId,
    #[allow(dead_code)]
    connection: Option<Connection<super::Callbacks<S, L>>>,
    pub logger: L,
}

impl<S: Storage, L: Logger + 'static> WindowsDriver<S, L> {
    pub fn connect(
        path: PathBuf,
        storage: S,
        logger: L,
        service_tx: Option<Sender<ServiceMessage>>,
    ) -> Result<Arc<Self>> {
        let security_id =
            SecurityId::current_user().map_err(|source| Error::CloudFilterOperation {
                operation: "resolve current user security id",
                source,
            })?;
        if !path.exists() {
            std::fs::create_dir(&path).map_err(|source| Error::Io {
                operation: "create mount path",
                path: path.clone(),
                source,
            })?;
        }
        logger.info(format!("Mount path: {}", path.display()));
        let path = absolute(&path).map_err(|source| Error::Io {
            operation: "resolve mount path",
            path: path.clone(),
            source,
        })?;
        let name = path
            .file_name()
            .and_then(|os_str| os_str.to_str())
            .ok_or_else(|| Error::InvalidPath { path: path.clone() })?;
        let driver_name = ID_PREFIX.to_owned() + "|" + name;

        let id = SyncRootIdBuilder::new(driver_name)
            .user_security_id(security_id)
            .build();

        let is_registered = id
            .is_registered()
            .map_err(|source| Error::CloudFilterOperation {
                operation: "check sync root registration",
                source,
            })?;
        if !is_registered {
            let sync_root_info = SyncRootInfo::default()
                .with_display_name(name)
                .with_icon("%SystemRoot%\\system32\\charmap.exe,0")
                .with_version(env!("CARGO_PKG_VERSION"))
                .with_hydration_type(HydrationType::Full)
                .with_population_type(PopulationType::Full)
                .with_path(&path)
                .map_err(|source| Error::CloudFilterOperation {
                    operation: "build sync root info",
                    source,
                })?;

            id.register(sync_root_info)
                .map_err(|source| Error::CloudFilterOperation {
                    operation: "register sync root",
                    source,
                })?;
            logger.info(format!("Sync root registered: {}", name));
        }

        let session = Session::new();
        let connection = session
            .connect(
                &path,
                super::Callbacks::new(path.clone(), storage, logger.clone(), service_tx),
            )
            .map_err(|source| Error::CloudFilterOperation {
                operation: "connect to sync root",
                source,
            })?;

        Ok(Arc::new(Self {
            path,
            id,
            connection: Some(connection),
            logger,
        }))
    }
}

impl<S: Storage, L: Logger + 'static> Driver for Arc<WindowsDriver<S, L>> {
    fn kind(&self) -> &'static str {
        "CloudFilter"
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/drivers/windows/windows_driver.rs && git commit -m "feat: add WindowsDriver in drivers/windows/"
```

---

### Task 3: Create remaining Windows driver files

**Files:**
- Copy: `providers/cloudfilter/callbacks.rs` → `drivers/windows/callbacks.rs`
- Copy: `providers/cloudfilter/cleanup_registry.rs` → `drivers/windows/cleanup_registry.rs`
- Copy: `providers/cloudfilter/placeholders.rs` → `drivers/windows/placeholders.rs`
- Copy: `providers/cloudfilter/register.rs` → `drivers/windows/register.rs`
- Copy: `providers/cloudfilter/error.rs` → `drivers/windows/error.rs`
- Copy: `providers/cloudfilter/mod.rs` → `drivers/windows/mod.rs`

**Changes in each file:**
- Update imports: `Provider` → `Driver`, `CloudFilterProvider` → `WindowsDriver`

- [ ] **Step 1: Copy files**

```bash
cp crates/anymount/src/providers/cloudfilter/callbacks.rs crates/anymount/src/drivers/windows/
cp crates/anymount/src/providers/cloudfilter/cleanup_registry.rs crates/anymount/src/drivers/windows/
cp crates/anymount/src/providers/cloudfilter/placeholders.rs crates/anymount/src/drivers/windows/
cp crates/anymount/src/providers/cloudfilter/register.rs crates/anymount/src/drivers/windows/
cp crates/anymount/src/providers/cloudfilter/error.rs crates/anymount/src/drivers/windows/
cp crates/anymount/src/providers/cloudfilter/mod.rs crates/anymount/src/drivers/windows/mod.rs
```

- [ ] **Step 2: Update imports in `drivers/windows/mod.rs`**

```rust
pub use super::driver::Driver;
pub use super::error::{Error, Result};
pub use super::{Callbacks, Storage};
pub use crate::drivers::error::Error as DriversError;
pub use crate::drivers::windows_driver::WindowsDriver;
```

- [ ] **Step 3: Update imports in `drivers/windows/callbacks.rs`**

Change: `use super::{Error, Provider, Result, Storage};` → `use super::{Error, Driver, Result, Storage};`

- [ ] **Step 4: Update imports in `drivers/windows/register.rs`**

Change: `use super::CloudFilterProvider;` → `use super::WindowsDriver;`
Change: `impl<S: Storage, L: Logger> CloudFilterProvider<S, L>` → `impl<S: Storage, L: Logger> WindowsDriver<S, L>`

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/drivers/windows/ && git commit -m "chore: copy remaining windows driver files"
```

---

### Task 4: Create `drivers/linux/linux_driver.rs` (from `providers/libcloudprovider/provider.rs`)

**Files:**
- Create: `crates/anymount/src/drivers/linux/linux_driver.rs`

**Changes:**
- `LibCloudProvider` → `LinuxDriver`
- `Provider` → `Driver`
- `"LibCloudProviders"` → `"Linux"`
- `providers::Provider` → `drivers::Driver`

- [ ] **Step 1: Create `linux_driver.rs`**

```rust
//! Linux driver: FUSE mount + D-Bus org.freedesktop.CloudProviders.

use super::dbus::{
    AccountExporter, ActionMessage, PROVIDER_PATH, ProviderExporter, new_account_interfaces,
    request_bus_name,
};
use super::gtk_dbus::{ACTION_FREE_LOCAL_CACHE, ACTION_OPEN_FOLDER};
use super::{Error, Result, StorageFilesystem};
use crate::drivers::Driver;
use crate::Logger;
use crate::storages::Storage;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

pub struct LinuxDriver {
    path: PathBuf,
    _session: fuser::BackgroundSession,
}

impl LinuxDriver {
    pub fn new(path: PathBuf, session: fuser::BackgroundSession) -> Self {
        Self {
            path,
            _session: session,
        }
    }
}

impl Driver for LinuxDriver {
    fn kind(&self) -> &'static str {
        "LibCloudProviders"
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

fn default_cache_base_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(path);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache");
    }
    std::env::temp_dir()
}

pub(crate) fn cache_root_for_mount(path: &PathBuf) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    let mount_hash = format!("{:016x}", hasher.finish());
    default_cache_base_dir()
        .join("anymount")
        .join("linux")
        .join(mount_hash)
}

pub fn mount_storage(
    path: PathBuf,
    storage: impl Storage,
    logger: impl Logger + 'static,
) -> Result<(PathBuf, fuser::BackgroundSession)> {
    if !path.exists() {
        std::fs::create_dir_all(&path).map_err(|source| Error::MountIo {
            operation: "create mount path",
            path: path.clone(),
            source,
        })?;
    }
    let path = path.canonicalize().map_err(|source| Error::MountIo {
        operation: "canonicalize mount path",
        path: path.clone(),
        source,
    })?;
    logger.info(format!("Mount path: {}", path.display()));
    let cache_root = cache_root_for_mount(&path);
    logger.info(format!("Cache path: {}", cache_root.display()));

    let fs = StorageFilesystem::new(storage, cache_root, logger.clone())?;
    let session = fuser::spawn_mount2(fs, &path, &fuser::Config::default()).map_err(|source| {
        Error::FuseMount {
            path: path.clone(),
            source,
        }
    })?;
    Ok((path, session))
}

pub fn new_runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Runtime::new().map_err(|source| Error::RuntimeInit { source })
}

async fn run_actions<L: Logger>(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<ActionMessage>,
    _logger: L,
) {
    while let Some((mount_path, cache_root, action_name)) = rx.recv().await {
        match action_name.as_str() {
            ACTION_OPEN_FOLDER => {
                let _ = open::that(&mount_path);
            }
            ACTION_FREE_LOCAL_CACHE => {
                let cache_root = cache_root.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let _ = std::fs::remove_dir_all(&cache_root);
                    std::fs::create_dir_all(&cache_root).ok();
                })
                .await;
            }
            _ => {}
        }
    }
}

pub async fn export_on_dbus<L: Logger + Clone + 'static>(
    accounts: &[(PathBuf, AccountExporter)],
    logger: &L,
) -> Result<()> {
    let connection = zbus::Connection::session()
        .await
        .map_err(|source| Error::Dbus {
            operation: "open session bus",
            source,
        })?;
    request_bus_name(&connection)
        .await
        .map_err(|source| Error::Dbus {
            operation: "request bus name",
            source,
        })?;

    let (action_tx, action_rx) = tokio::sync::mpsc::unbounded_channel::<ActionMessage>();
    tokio::spawn(run_actions(action_rx, logger.clone()));

    connection
        .object_server()
        .at(PROVIDER_PATH, ProviderExporter::default())
        .await
        .map_err(|source| Error::DbusObject {
            operation: "register driver interface",
            object_path: PROVIDER_PATH.to_string(),
            source,
        })?;
    connection
        .object_server()
        .at(PROVIDER_PATH, zbus::fdo::ObjectManager)
        .await
        .map_err(|source| Error::DbusObject {
            operation: "register object manager",
            object_path: PROVIDER_PATH.to_string(),
            source,
        })?;

    for (i, (path, account)) in accounts.iter().enumerate() {
        let cache_root = cache_root_for_mount(path);
        let (cloud, actions, menus) = new_account_interfaces(
            account.clone(),
            path.display().to_string(),
            cache_root,
            action_tx.clone(),
        );
        let object_path = format!("/org/anymount/CloudProviders/Account_{}", i);
        connection
            .object_server()
            .at(object_path.as_str(), cloud)
            .await
            .map_err(|source| Error::DbusObject {
                operation: "register cloud account interface",
                object_path: object_path.clone(),
                source,
            })?;
        connection
            .object_server()
            .at(object_path.as_str(), actions)
            .await
            .map_err(|source| Error::DbusObject {
                operation: "register gtk actions interface",
                object_path: object_path.clone(),
                source,
            })?;
        connection
            .object_server()
            .at(object_path.as_str(), menus)
            .await
            .map_err(|source| Error::DbusObject {
                operation: "register gtk menus interface",
                object_path: object_path.clone(),
                source,
            })?;
    }

    tokio::spawn(async move {
        let _ = connection;
        std::future::pending::<()>().await;
    });
    Ok(())
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/drivers/linux/linux_driver.rs && git commit -m "feat: add LinuxDriver in drivers/linux/"
```

---

### Task 5: Create remaining Linux driver files

**Files:**
- Copy: `providers/libcloudprovider/dbus.rs` → `drivers/linux/dbus.rs`
- Copy: `providers/libcloudprovider/error.rs` → `drivers/linux/error.rs`
- Copy: `providers/libcloudprovider/fuse.rs` → `drivers/linux/fuse.rs`
- Copy: `providers/libcloudprovider/gtk_dbus.rs` → `drivers/linux/gtk_dbus.rs`
- Copy: `providers/libcloudprovider/mod.rs` → `drivers/linux/mod.rs`

**Changes:**
- Update doc comments mentioning "provider" → "driver"
- Update imports as needed

- [ ] **Step 1: Copy files**

```bash
cp crates/anymount/src/providers/libcloudprovider/dbus.rs crates/anymount/src/drivers/linux/
cp crates/anymount/src/providers/libcloudprovider/error.rs crates/anymount/src/drivers/linux/
cp crates/anymount/src/providers/libcloudprovider/fuse.rs crates/anymount/src/drivers/linux/
cp crates/anymount/src/providers/libcloudprovider/gtk_dbus.rs crates/anymount/src/drivers/linux/
cp crates/anymount/src/providers/libcloudprovider/mod.rs crates/anymount/src/drivers/linux/mod.rs
```

- [ ] **Step 2: Update `drivers/linux/mod.rs`**

```rust
pub use crate::drivers::Driver;
pub use linux_driver::{cache_root_for_mount, export_on_dbus, mount_storage, new_runtime, LinuxDriver};
```

- [ ] **Step 3: Update `drivers/linux/dbus.rs` doc comment**

Change: `//! D-Bus implementation of org.freedesktop.CloudProviders (Provider and Account)` → `//! D-Bus implementation of org.freedesktop.CloudProviders (Driver and Account)`

- [ ] **Step 4: Commit**

```bash
git add crates/anymount/src/drivers/linux/ && git commit -m "chore: copy remaining linux driver files"
```

---

### Task 6: Create `drivers/driver.rs` (from `providers/provider.rs`)

**Files:**
- Create: `crates/anymount/src/drivers/driver.rs`

**Changes:**
- `Provider` trait → `Driver` trait
- `connect_providers` → `connect_drivers`
- `connect_providers_with_telemetry` → `connect_drivers_with_telemetry`
- `Box<dyn Provider>` → `Box<dyn Driver>`
- `CloudFilterProvider` → `WindowsDriver`
- `LibCloudProvider` → `LinuxDriver`
- Variable names: `providers` → `drivers`, `provider` → `driver`

- [ ] **Step 1: Create `drivers/driver.rs`**

```rust
use super::Result;
use crate::domain::driver::{Driver as DomainDriver, StorageSpec};
use crate::service::control::messages::ServiceMessage;
use crate::storages::{LocalStorage, OneDriveConfig};
use crate::Logger;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

pub trait Driver {
    fn kind(&self) -> &'static str;
    fn path(&self) -> &PathBuf;
}

#[cfg(target_os = "windows")]
pub fn connect_drivers(
    specs: &[DomainDriver],
    logger: &(impl Logger + 'static),
) -> Result<Vec<Box<dyn Driver>>> {
    connect_drivers_with_telemetry(specs, logger, None)
}

#[cfg(target_os = "windows")]
pub fn connect_drivers_with_telemetry(
    specs: &[DomainDriver],
    logger: &(impl Logger + 'static),
    service_tx: Option<Sender<ServiceMessage>>,
) -> Result<Vec<Box<dyn Driver>>> {
    use super::windows::{cleanup_registry, WindowsDriver};
    let mut drivers: Vec<Box<dyn Driver>> = Vec::new();
    for spec in specs {
        match &spec.storage {
            StorageSpec::Local { root } => {
                let storage = LocalStorage::new(root.clone());
                let driver = WindowsDriver::connect(
                    spec.path.clone(),
                    storage,
                    logger.clone(),
                    service_tx.clone(),
                )?;
                drivers.push(Box::new(driver) as Box<dyn Driver>);
            }
            StorageSpec::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            } => {
                let config = OneDriveConfig {
                    root: root.clone(),
                    endpoint: endpoint.clone(),
                    access_token: access_token.clone(),
                    refresh_token: refresh_token.clone(),
                    client_id: client_id.clone(),
                    token_expiry_buffer_secs: *token_expiry_buffer_secs,
                };
                let storage = config.connect()?;
                let driver = WindowsDriver::connect(
                    spec.path.clone(),
                    storage,
                    logger.clone(),
                    service_tx.clone(),
                )?;
                drivers.push(Box::new(driver) as Box<dyn Driver>);
            }
        }
    }
    cleanup_registry(specs, logger)?;
    Ok(drivers)
}

#[cfg(target_os = "linux")]
pub fn connect_drivers(
    specs: &[DomainDriver],
    logger: &(impl Logger + 'static),
) -> Result<Vec<Box<dyn Driver>>> {
    connect_drivers_with_telemetry(specs, logger, None)
}

#[cfg(target_os = "linux")]
pub fn connect_drivers_with_telemetry(
    specs: &[DomainDriver],
    logger: &(impl Logger + 'static),
    _service_tx: Option<Sender<ServiceMessage>>,
) -> Result<Vec<Box<dyn Driver>>> {
    use super::linux::dbus::AccountExporter;
    use super::linux::{export_on_dbus, mount_storage, new_runtime, LinuxDriver};
    let rt = new_runtime()?;
    let mut accounts: Vec<(std::path::PathBuf, AccountExporter)> = Vec::new();
    let mut sessions: Vec<(std::path::PathBuf, fuser::BackgroundSession)> = Vec::new();
    for spec in specs {
        let path = spec.path.clone();
        match &spec.storage {
            StorageSpec::Local { root } => {
                let storage = LocalStorage::new(root.clone());
                let (mount_path, session) = mount_storage(path, storage, logger.clone())?;
                let name = mount_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Anymount")
                    .to_string();
                accounts.push((
                    mount_path.clone(),
                    AccountExporter {
                        name: name.clone(),
                        path: mount_path.display().to_string(),
                        icon: String::new(),
                        status: 0,
                        status_details: String::new(),
                    },
                ));
                sessions.push((mount_path, session));
            }
            StorageSpec::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            } => {
                let one_drive_config = OneDriveConfig {
                    root: root.clone(),
                    endpoint: endpoint.clone(),
                    access_token: access_token.clone(),
                    refresh_token: refresh_token.clone(),
                    client_id: client_id.clone(),
                    token_expiry_buffer_secs: *token_expiry_buffer_secs,
                };
                let storage = one_drive_config.connect()?;
                let (mount_path, session) = mount_storage(path, storage, logger.clone())?;
                let name = mount_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("OneDrive")
                    .to_string();
                accounts.push((
                    mount_path.clone(),
                    AccountExporter {
                        name,
                        path: mount_path.display().to_string(),
                        icon: String::new(),
                        status: 0,
                        status_details: String::new(),
                    },
                ));
                sessions.push((mount_path, session));
            }
        }
    }
    rt.block_on(export_on_dbus(&accounts, logger))?;
    let drivers: Vec<Box<dyn Driver>> = sessions
        .into_iter()
        .map(|(path, session)| Box::new(LinuxDriver::new(path, session)) as Box<dyn Driver>)
        .collect();
    Ok(drivers)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn connect_drivers(
    _specs: &[DomainDriver],
    _logger: &impl Logger,
) -> Result<Vec<Box<dyn Driver>>> {
    connect_drivers_with_telemetry(_specs, _logger, None)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn connect_drivers_with_telemetry(
    _specs: &[DomainDriver],
    _logger: &impl Logger,
    _service_tx: Option<Sender<ServiceMessage>>,
) -> Result<Vec<Box<dyn Driver>>> {
    Err(super::Error::NotSupported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::driver::{Driver as DomainDriver, StorageSpec, TelemetrySpec};
    use crate::NoOpLogger;

    #[test]
    fn storage_label_comes_from_domain_storage_spec() {
        let local = StorageSpec::Local {
            root: PathBuf::from("/data"),
        };
        assert_eq!(local.label(), "local");
        let onedrive = StorageSpec::OneDrive {
            root: PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
            access_token: None,
            refresh_token: None,
            client_id: None,
            token_expiry_buffer_secs: None,
        };
        assert_eq!(onedrive.label(), "onedrive");
    }

    fn local_driver_spec(name: &str) -> DomainDriver {
        DomainDriver {
            name: name.to_owned(),
            path: PathBuf::from(format!("/mnt/{name}")),
            storage: StorageSpec::Local {
                root: PathBuf::from(format!("/data/{name}")),
            },
            telemetry: TelemetrySpec::default(),
        }
    }

    #[test]
    fn connect_drivers_accepts_resolved_specs() {
        let spec = local_driver_spec("demo");
        let result = connect_drivers(&[spec], &NoOpLogger::default());
        assert!(!matches!(result, Err(crate::drivers::Error::Storage(_))));
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/drivers/driver.rs && git commit -m "feat: add Driver trait and connect_drivers functions"
```

---

### Task 7: Create `drivers/error.rs` (from `providers/error.rs`)

**Files:**
- Create: `crates/anymount/src/drivers/error.rs`

- [ ] **Step 1: Create `drivers/error.rs`**

```rust
#[cfg(target_os = "windows")]
use crate::drivers::windows::Error as WindowsError;

#[cfg(target_os = "linux")]
use crate::drivers::linux::Error as LinuxError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[cfg(target_os = "windows")]
    #[error(transparent)]
    CloudFilter(#[from] WindowsError),

    #[cfg(target_os = "linux")]
    #[error(transparent)]
    LibCloudProvider(#[from] LinuxError),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("mount io error during {operation} for {path}: {source}")]
    MountIo {
        operation: &'static str,
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("IO error during {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid path: {path:?}")]
    InvalidPath { path: std::path::PathBuf },

    #[error("FUSE mount error for {path}: {source}")]
    #[cfg(target_os = "linux")]
    FuseMount {
        path: std::path::PathBuf,
        #[source]
        #[backtrace]
        source: fuser::TempDir,
    },

    #[error("FUSE mount error for {path}: {source}")]
    #[cfg(not(target_os = "linux"))]
    FuseMount {
        path: std::path::PathBuf,
        #[source]
        #[backtrace]
        source: fuser::MountError,
    },

    #[error("D-Bus error during {operation}: {source}")]
    Dbus {
        operation: &'static str,
        #[source]
        source: zbus::Error,
    },

    #[error("D-Bus object error during {operation} for {object_path}: {source}")]
    DbusObject {
        operation: &'static str,
        object_path: String,
        #[source]
        source: zbus::Error,
    },

    #[error("runtime init error: {source}")]
    RuntimeInit {
        #[source]
        source: std::io::Error,
    },

    #[error("not supported on this platform")]
    NotSupported,
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/drivers/error.rs && git commit -m "feat: add drivers/error.rs"
```

---

### Task 8: Create `drivers/mod.rs`

**Files:**
- Create: `crates/anymount/src/drivers/mod.rs`

- [ ] **Step 1: Create `drivers/mod.rs`**

```rust
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

pub mod error;
pub mod driver;

pub use error::{Error, Result};
pub use driver::{connect_drivers, connect_drivers_with_telemetry, Driver};
```

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/drivers/mod.rs && git commit -m "feat: add drivers/mod.rs with public re-exports"
```

---

### Task 9: Create `domain/driver.rs` (from `domain/provider.rs`)

**Files:**
- Create: `crates/anymount/src/domain/driver.rs`

**Changes:**
- `ProviderSpec` → `Driver`
- `Provider domain types` → `Driver domain types`
- All doc comments: "provider" → "driver"
- `Error::MissingMountPath` message: "provider mount path" → "driver mount path"
- All other "provider" → "driver" in error messages

- [ ] **Step 1: Create `domain/driver.rs`**

```rust
//! Driver domain types.
//!
//! This module defines driver-facing concepts shared across adapters. The
//! types here describe what a driver is and the invariants it must satisfy
//! before adapter code can persist, mount, or host it.

use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

/// Driver domain validation failures.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Error {
    /// The driver mount path is empty.
    #[error("driver mount path is missing")]
    MissingMountPath,
    /// The local storage root is empty.
    #[error("local storage root is missing")]
    MissingLocalRoot,
    /// The OneDrive root is empty.
    #[error("OneDrive root is missing")]
    MissingOneDriveRoot,
    /// The OneDrive config has no access or refresh token.
    #[error("OneDrive token material is missing")]
    MissingOneDriveTokenMaterial,
}

/// Result type for driver domain validation.
pub type Result<T> = std::result::Result<T, Error>;

/// A configured driver definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Driver {
    /// Stable driver name derived from config.
    pub name: String,
    /// Local mount path exposed by the driver.
    pub path: PathBuf,
    /// Storage backend configuration for this driver.
    pub storage: StorageSpec,
    /// Optional telemetry configuration for this driver.
    pub telemetry: TelemetrySpec,
}

impl Driver {
    /// Validates driver invariants.
    ///
    /// # Errors
    /// Returns an error when the mount path or storage configuration is
    /// incomplete.
    pub fn validate(&self) -> Result<()> {
        if self.path.as_os_str().is_empty() {
            return Err(Error::MissingMountPath);
        }

        self.storage.validate()
    }

    pub fn onedrive_endpoint(&self) -> Option<&str> {
        match &self.storage {
            StorageSpec::OneDrive { endpoint, .. } => Some(endpoint.as_str()),
            StorageSpec::Local { .. } => None,
        }
    }
}

/// Supported storage backends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageSpec {
    /// Local directory storage.
    Local {
        /// Root directory exposed by the driver.
        root: PathBuf,
    },
    /// OneDrive storage via Microsoft Graph.
    OneDrive {
        /// OneDrive path used as the virtual root.
        root: PathBuf,
        /// Microsoft Graph endpoint.
        endpoint: String,
        /// Optional short-lived access token.
        access_token: Option<String>,
        /// Optional refresh token used to obtain new access tokens.
        refresh_token: Option<String>,
        /// Optional OAuth client id override.
        client_id: Option<String>,
        /// Refresh buffer before token expiry.
        token_expiry_buffer_secs: Option<u64>,
    },
}

impl StorageSpec {
    /// Short label for CLI and status output (`local`, `onedrive`, ...).
    pub fn label(&self) -> &'static str {
        match self {
            Self::Local { .. } => "local",
            Self::OneDrive { .. } => "onedrive",
        }
    }

    /// Validates storage-specific invariants.
    ///
    /// # Errors
    /// Returns an error when the storage config is missing a required path or
    /// token.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Local { root } => {
                if root.as_os_str().is_empty() {
                    return Err(Error::MissingLocalRoot);
                }
            }
            Self::OneDrive {
                root,
                access_token,
                refresh_token,
                ..
            } => {
                if root.as_os_str().is_empty() {
                    return Err(Error::MissingOneDriveRoot);
                }

                if access_token.is_none() && refresh_token.is_none() {
                    return Err(Error::MissingOneDriveTokenMaterial);
                }
            }
        }

        Ok(())
    }
}

/// Driver telemetry settings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TelemetrySpec {
    /// Optional OTLP exporter configuration.
    pub otlp: Option<OtlpSpec>,
}

/// OTLP exporter settings for one driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtlpSpec {
    /// Whether OTLP export is enabled.
    pub enabled: bool,
    /// Optional OTLP endpoint override.
    pub endpoint: Option<String>,
    /// Optional transport override.
    pub protocol: Option<OtlpTransport>,
    /// Optional transport headers.
    pub headers: Option<HashMap<String, String>>,
    /// Optional extra resource attributes.
    pub resource_attributes: Option<HashMap<String, String>>,
}

impl Default for OtlpSpec {
    fn default() -> Self {
        Self {
            enabled: true,
            endpoint: None,
            protocol: None,
            headers: None,
            resource_attributes: None,
        }
    }
}

/// OTLP wire transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OtlpTransport {
    /// HTTP/protobuf transport.
    #[default]
    HttpProtobuf,
    /// gRPC transport.
    Grpc,
}

#[cfg(test)]
mod tests {
    use super::{Driver, Error, StorageSpec, TelemetrySpec};
    use std::path::PathBuf;

    fn local_driver_spec(name: &str) -> Driver {
        Driver {
            name: name.to_owned(),
            path: PathBuf::from(format!("/mnt/{name}")),
            storage: StorageSpec::Local {
                root: PathBuf::from(format!("/data/{name}")),
            },
            telemetry: TelemetrySpec::default(),
        }
    }

    #[test]
    fn onedrive_spec_requires_token_material() {
        let spec = Driver {
            name: "demo".to_owned(),
            path: PathBuf::from("/mnt/demo"),
            storage: StorageSpec::OneDrive {
                root: PathBuf::from("/"),
                endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
                access_token: None,
                refresh_token: None,
                client_id: None,
                token_expiry_buffer_secs: Some(60),
            },
            telemetry: TelemetrySpec::default(),
        };

        let err = spec.validate().expect_err("spec should be invalid");
        assert!(matches!(err, Error::MissingOneDriveTokenMaterial));
    }

    #[test]
    fn local_spec_validation_accepts_path_and_root() {
        let spec = local_driver_spec("demo");
        spec.validate().expect("local spec should be valid");
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/domain/driver.rs && git commit -m "feat: add domain/driver.rs with Driver type"
```

---

### Task 10: Update `domain/mod.rs`

**Files:**
- Modify: `crates/anymount/src/domain/mod.rs`

- [ ] **Step 1: Update `domain/mod.rs`**

Change: `pub mod provider;` → `pub mod driver;`

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/domain/mod.rs && git commit -m "chore: update domain/mod.rs to use driver module"
```

---

### Task 11: Update `lib.rs`

**Files:**
- Modify: `crates/anymount/src/lib.rs`

**Changes:**
- `pub mod providers;` → `pub mod drivers;`
- `pub use providers::{connect_providers, connect_providers_with_telemetry, Provider};` → `pub use drivers::{connect_drivers, connect_drivers_with_telemetry, Driver};`

- [ ] **Step 1: Update `lib.rs`**

```rust
pub mod application;
pub mod auth;
pub mod cli;
pub mod config;
pub mod domain;
#[deprecated(note = "use module-specific errors instead")]
pub mod error;
pub mod logger;
pub mod drivers;
pub mod service;
pub mod storages;
pub mod telemetry;
pub mod tui;

pub use config::{Config, ConfigDir, DriverFileConfig, StorageConfig, TelemetryFileConfig};
pub use logger::{Logger, NoOpLogger, TracingLogger};
pub use drivers::{connect_drivers, connect_drivers_with_telemetry, Driver};
pub use storages::Storage;
```

Note: `DriverFileConfig` will be updated in Task 13.

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/lib.rs && git commit -m "chore: update lib.rs to use drivers module"
```

---

### Task 12: Update error modules

**Files:**
- Modify: `crates/anymount/src/error.rs`
- Modify: `crates/anymount/src/cli/error.rs`
- Modify: `crates/anymount/src/service/error.rs`
- Modify: `crates/anymount/src/tui/error.rs`

**Changes in `error.rs`:**
- `#[error("Provider error: {0}")]` → `#[error("Driver error: {0}")]`
- `Provider(String)` → `Driver(String)`

**Changes in `cli/error.rs`:**
- `Providers(#[from] crate::providers::Error)` → `Drivers(#[from] crate::drivers::Error)`
- `SpawnProvider` → `SpawnDriver`
- `WaitForProvider` → `WaitForDriver`
- `ProviderExitedBeforeReady` → `DriverExitedBeforeReady`
- `ProviderDidNotBecomeReady` → `DriverDidNotBecomeReady`
- `ConnectFailures { failures: "failed to connect providers" }` → `ConnectFailures { failures: "failed to connect drivers" }`
- `DisconnectFailures { failures: "failed to disconnect providers" }` → `DisconnectFailures { failures: "failed to disconnect drivers" }`
- Update error messages: "provider" → "driver"
- Update variable names: `provider_name` → `driver_name`

**Changes in `service/error.rs`:**
- `InvalidProviderName` → `InvalidDriverName`
- `provider_name` → `driver_name` in error messages and fields

**Changes in `tui/error.rs`:**
- Update pattern match: `DuplicateProvider` → `DuplicateDriver`
- Update error message: "provider" → "driver"

- [ ] **Step 1: Update `crates/anymount/src/error.rs`**

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Driver error: {0}")]
    Driver(String),

    #[error("Mount error: {0}")]
    Mount(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Not supported on this platform: {0}")]
    NotSupported(String),

    #[error("Platform-specific error: {0}")]
    Platform(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 2: Update `crates/anymount/src/cli/error.rs`**

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Auth(#[from] crate::auth::Error),

    #[error(transparent)]
    Config(#[from] crate::config::Error),

    #[error(transparent)]
    Otlp(#[from] crate::telemetry::OtlpInitError),

    #[error(transparent)]
    Service(#[from] crate::service::Error),

    #[error(transparent)]
    Drivers(#[from] crate::drivers::Error),

    #[error("failed to serialize config: {source}")]
    SerializeConfig {
        #[source]
        source: toml::ser::Error,
    },

    #[error("invalid integer value {value}: {source}")]
    ParseInteger {
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },

    #[error("specify --name <NAME> or --all")]
    MissingConnectTarget,

    #[error("specify --name <NAME> or --all")]
    MissingDisconnectTarget,

    #[error("specify --name <NAME> or --path <PATH> with a storage subcommand")]
    MissingProvideTarget,

    #[error("failed to install Ctrl-C handler: {source}")]
    InstallCtrlC {
        #[source]
        source: ctrlc::Error,
    },

    #[error("failed to resolve current executable: {source}")]
    ResolveCurrentExecutable {
        #[source]
        source: std::io::Error,
    },

    #[error("failed to spawn driver process for {driver_name}: {source}")]
    SpawnDriver {
        driver_name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to wait for driver process {driver_name}: {source}")]
    WaitForDriver {
        driver_name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("driver process {driver_name} exited before ready with status {status}")]
    DriverExitedBeforeReady {
        driver_name: String,
        status: String,
    },

    #[error("driver process {driver_name} did not become ready")]
    DriverDidNotBecomeReady { driver_name: String },

    #[error("failed to connect drivers: {failures}")]
    ConnectFailures { failures: String },

    #[error("failed to disconnect drivers: {failures}")]
    DisconnectFailures { failures: String },

    #[error("{0}")]
    Prompt(String),

    #[error("{0}")]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 3: Update `crates/anymount/src/service/error.rs`**

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid driver name: {name}")]
    InvalidDriverName { name: String },

    #[error("control message was not valid utf-8: {0}")]
    DecodeUtf8(#[from] std::str::Utf8Error),

    #[error("unknown control message: {value}")]
    UnknownControlMessage { value: String },

    #[error("service io error during {operation} for {driver_name}: {source}")]
    Io {
        operation: &'static str,
        driver_name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("service receive failed: {0}")]
    Receive(#[from] std::sync::mpsc::RecvError),

    #[error("in-memory control transport was poisoned")]
    Poisoned,

    #[error("no in-memory server bound for driver {driver_name}")]
    NotBound { driver_name: String },

    #[error("no queued response available for driver {driver_name}")]
    NoQueuedResponse { driver_name: String },

    #[error("control transport not supported on this platform")]
    NotSupported,
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 4: Update `crates/anymount/src/tui/error.rs`**

Change the pattern match from `DuplicateProvider` to `DuplicateDriver`.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/error.rs crates/anymount/src/cli/error.rs crates/anymount/src/service/error.rs crates/anymount/src/tui/error.rs && git commit -m "chore: update error variants to use driver terminology"
```

---

### Task 13: Update `application/config.rs`

**Files:**
- Modify: `crates/anymount/src/application/config.rs`

**Changes:**
- `ProviderSpec` → `Driver`
- `DuplicateProvider` → `DuplicateDriver`
- Update doc comments
- Update variable names in tests

- [ ] **Step 1: Update imports and types**

```rust
use crate::domain::driver::{Driver, StorageSpec};
```

- [ ] **Step 2: Update error variant**

```rust
#[error("driver '{name}' already exists")]
DuplicateDriver { name: String },
```

- [ ] **Step 3: Update trait definitions**

```rust
pub trait ConfigRepository {
    fn list_names(&self) -> Result<Vec<String>>;
    fn read_spec(&self, name: &str) -> Result<Driver>;
    fn write_spec(&self, spec: &Driver) -> Result<()>;
    fn remove(&self, name: &str) -> Result<()>;
}

pub trait ConfigUseCase {
    fn list(&self) -> Result<Vec<String>>;
    fn read(&self, name: &str) -> Result<Driver>;
    fn add(&self, spec: Driver) -> Result<()>;
    fn remove(&self, name: &str) -> Result<()>;
    fn set(&self, name: &str, key: &str, value: &str) -> Result<()>;
}
```

- [ ] **Step 4: Update impl and tests**

Replace all `ProviderSpec` → `Driver` and `DuplicateProvider` → `DuplicateDriver`.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/application/config.rs && git commit -m "chore: update application/config.rs to use Driver"
```

---

### Task 14: Update `config.rs` (DriverFileConfig)

**Files:**
- Modify: `crates/anymount/src/config.rs`

**Changes:**
- `ProviderFileConfig` → `DriverFileConfig`
- Update doc comments

- [ ] **Step 1: Update `config.rs`**

```rust
pub use crate::application::config::{ConfigUseCase, DriverFileConfig, StorageConfig, TelemetryFileConfig};
```

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/config.rs && git commit -m "chore: update config.rs to use DriverFileConfig"
```

---

### Task 15: Update `telemetry/mod.rs`

**Files:**
- Modify: `crates/anymount/src/telemetry/mod.rs`

**Changes:**
- `ProviderSpec` → `Driver`
- `from_provider_spec` → `from_driver_spec`
- `anymount.provider.name` → `anymount.driver.name`
- Update doc comments
- Update tests

- [ ] **Step 1: Update `telemetry/mod.rs`**

Change all `ProviderSpec` to `Driver` and `from_provider_spec` to `from_driver_spec`.

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/telemetry/mod.rs && git commit -m "chore: update telemetry/mod.rs to use Driver"
```

---

### Task 16: Update `cli/commands/provide.rs`

**Files:**
- Modify: `crates/anymount/src/cli/commands/provide.rs`

**Changes:**
- `Provider` → `Driver`
- `ProviderSpec` → `Driver`
- `ProviderRuntimeHost` → `DriverRuntimeHost`
- `connect_providers_with_telemetry` → `connect_drivers_with_telemetry`
- `providers: Vec<Box<dyn Provider>>` → `drivers: Vec<Box<dyn Driver>>`
- Update variable names: `providers` → `drivers`, `provider` → `driver`
- Update doc comments
- Update tests

- [ ] **Step 1: Update imports**

```rust
use crate::application::provide::{
    Application as ProvideApplication, Error as ProvideError, ProvideRepository, ProvideUseCase,
    DriverRuntimeHost, TelemetryFactory,
};
use crate::domain::driver::{Driver, StorageSpec, TelemetrySpec};
use crate::{Driver, Logger, TracingLogger};
```

- [ ] **Step 2: Update function signatures and bodies**

- [ ] **Step 3: Commit**

```bash
git add crates/anymount/src/cli/commands/provide.rs && git commit -m "chore: update cli/commands/provide.rs to use Driver"
```

---

### Task 17: Update `cli/commands/connect.rs`

**Files:**
- Modify: `crates/anymount/src/cli/commands/connect.rs`

**Changes:**
- Update error imports: `SpawnProvider` → `SpawnDriver`, etc.
- Update variable names in error handling
- Update tests

- [ ] **Step 1: Update error handling**

Replace all `SpawnProvider` → `SpawnDriver`, `WaitForProvider` → `WaitForDriver`, etc.

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/cli/commands/connect.rs && git commit -m "chore: update cli/commands/connect.rs error handling"
```

---

### Task 18: Update `service/control/paths.rs`

**Files:**
- Modify: `crates/anymount/src/service/control/paths.rs`

**Changes:**
- `InvalidProviderName` → `InvalidDriverName`

- [ ] **Step 1: Update error handling**

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/service/control/paths.rs && git commit -m "chore: update service/control/paths.rs to use InvalidDriverName"
```

---

### Task 19: Update `tui/tui.rs`

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`

**Changes:**
- `ProviderSpec` → `Driver`
- `ProviderFileConfig` → `DriverFileConfig`
- Update doc comments and string literals

- [ ] **Step 1: Update `tui/tui.rs`**

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/tui/tui.rs && git commit -m "chore: update tui/tui.rs to use Driver"
```

---

### Task 20: Update `application/provide.rs`

**Files:**
- Modify: `crates/anymount/src/application/provide.rs`

**Changes:**
- `ProviderSpec` → `Driver`
- `ProviderRuntimeHost` → `DriverRuntimeHost`
- `TelemetryFactory::build` parameter type
- Update doc comments

- [ ] **Step 1: Update `application/provide.rs`**

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/application/provide.rs && git commit -m "chore: update application/provide.rs to use Driver"
```

---

### Task 21: Update `application/connect.rs`

**Files:**
- Modify: `crates/anymount/src/application/connect.rs`

**Changes:**
- Update doc comments and error handling

- [ ] **Step 1: Update `application/connect.rs`**

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/application/connect.rs && git commit -m "chore: update application/connect.rs"
```

---

### Task 22: Update `cli/provider_control/` module

**Files:**
- Modify: `crates/anymount/src/cli/provider_control/mod.rs`
- Modify: `crates/anymount/src/cli/provider_control/provider_control_unix.rs`
- Modify: `crates/anymount/src/cli/provider_control/provider_control_windows.rs`

**Changes:**
- Module directory can stay as `provider_control/` since it's about controlling provider processes
- Update imports if needed

- [ ] **Step 1: Review and update if needed**

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/cli/provider_control/ && git commit -m "chore: review cli/provider_control/ module"
```

---

### Task 23: Update system tests

**Files:**
- Modify: `crates/anymount/tests/system/local_provider_test.rs`

**Changes:**
- Update comments: "provider" → "driver" where referring to the driver concept

- [ ] **Step 1: Update comments**

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/tests/system/local_provider_test.rs && git commit -m "chore: update system test comments"
```

---

### Task 24: Delete old `providers/` directory

**Files:**
- Delete: `crates/anymount/src/providers/` (entire directory)
- Delete: `crates/anymount/src/domain/provider.rs`

- [ ] **Step 1: Delete old directories**

```bash
rm -rf crates/anymount/src/providers
rm -f crates/anymount/src/domain/provider.rs
```

- [ ] **Step 2: Commit**

```bash
git add -A && git commit -m "chore: remove old providers/ directory"
```

---

### Task 25: Build and fix any remaining issues

**Files:**
- All files in `crates/anymount/src/`

- [ ] **Step 1: Run cargo build**

```bash
cargo build --package anymount 2>&1
```

- [ ] **Step 2: Fix any compilation errors**

- [ ] **Step 3: Run cargo test**

```bash
cargo test --package anymount 2>&1
```

- [ ] **Step 4: Commit any fixes**

```bash
git add -A && git commit -m "fix: resolve remaining compilation issues"
```

---

### Task 26: Update remaining references

**Files:**
- `crates/anymount/src/cli/commands/config.rs` - Update `ProviderType` enum (keep as-is, it's a CLI type)
- `crates/anymount/src/cli/commands/config.rs` - Update comments mentioning "provider"
- `crates/anymount/src/tui/tui.rs` - Update comments mentioning "provider"
- Any other remaining references that should change

**Changes:**
- Keep `ProviderType` enum as-is (it's a user-facing CLI type)
- Update user-facing strings that should show "driver" instead of "provider"

- [ ] **Step 1: Search for remaining Provider references**

```bash
rg "Provider" crates/anymount/src/ --type rust | grep -v "StorageProvider\|ProviderType\|provider_control"
```

- [ ] **Step 2: Update doc comments and user-facing strings**

Update any "provider" references that should be "driver" in:
- `cli/commands/config.rs` - doc comments
- `tui/tui.rs` - doc comments and string literals

Keep as-is:
- `ProviderType` - CLI enum for storage type
- `provider_control/` - module for controlling driver processes
- `StorageProvider*` - Windows API types

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "chore: update remaining Provider references"
```

---

## Verification

After all tasks are complete, run:

```bash
cargo build --package anymount
cargo test --package anymount
cargo clippy --package anymount
```

All tests should pass and clippy should report no warnings (beyond existing ones).
