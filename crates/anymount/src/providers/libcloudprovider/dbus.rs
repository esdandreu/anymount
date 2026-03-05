//! D-Bus implementation of org.freedesktop.CloudProviders (Provider and Account)
//! and org.gtk.Actions / org.gtk.Menus for account context menu.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use zbus::fdo;
use zbus::interface;
use zbus::zvariant::OwnedValue;

use super::gtk_dbus;

const PROVIDER_NAME: &str = "Anymount";
pub const PROVIDER_PATH: &str = "/org/anymount/CloudProviders";
pub const BUS_NAME: &str = "org.anymount.CloudProviders";

/// Exposes org.freedesktop.CloudProviders.Provider at the root path.
#[derive(Clone, Default)]
pub struct ProviderExporter;

#[interface(name = "org.freedesktop.CloudProviders.Provider")]
impl ProviderExporter {
    #[zbus(property, name = "Name")]
    fn name(&self) -> fdo::Result<String> {
        Ok(PROVIDER_NAME.to_string())
    }
}

/// Exposes org.freedesktop.CloudProviders.Account for one account.
#[derive(Clone)]
pub struct AccountExporter {
    pub name: String,
    pub path: String,
    pub icon: String,
    pub status: i32,
    pub status_details: String,
}

#[interface(name = "org.freedesktop.CloudProviders.Account")]
impl AccountExporter {
    #[zbus(property, name = "Name")]
    fn name(&self) -> fdo::Result<String> {
        Ok(self.name.clone())
    }

    #[zbus(property, name = "Path")]
    fn path(&self) -> fdo::Result<String> {
        Ok(self.path.clone())
    }

    #[zbus(property, name = "Icon")]
    fn icon(&self) -> fdo::Result<String> {
        Ok(self.icon.clone())
    }

    #[zbus(property, name = "Status")]
    fn status(&self) -> fdo::Result<i32> {
        Ok(self.status)
    }

    #[zbus(property, name = "StatusDetails")]
    fn status_details(&self) -> fdo::Result<String> {
        Ok(self.status_details.clone())
    }
}

/// Message sent to the action runner: (mount_path, cache_root, action_name).
pub type ActionMessage = (String, PathBuf, String);

fn dir_size_bytes(path: &std::path::Path) -> Option<u64> {
    let mut total = 0u64;
    for e in std::fs::read_dir(path).ok()? {
        let e = e.ok()?;
        let m = e.metadata().ok()?;
        if m.is_dir() {
            total += dir_size_bytes(&e.path()).unwrap_or(0);
        } else {
            total += m.len();
        }
    }
    Some(total)
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else if bytes > 0 {
        format!("{} B", bytes)
    } else {
        String::new()
    }
}

#[cfg(test)]
#[test]
fn format_size_zero_is_empty() {
    assert_eq!(format_size(0), "");
}

#[cfg(test)]
#[test]
fn format_size_units() {
    assert_eq!(format_size(500), "500 B");
    assert!(format_size(1024).starts_with("1.0 KB"));
    assert!(format_size(1024 * 1024).starts_with("1.0 MB"));
    assert!(format_size(1024 * 1024 * 1024).starts_with("1.0 GB"));
}

/// Shared state for one account; three interface types wrap this.
pub struct AccountShared {
    account: AccountExporter,
    mount_path: String,
    cache_root: PathBuf,
    action_tx: UnboundedSender<ActionMessage>,
}

/// Exposes org.freedesktop.CloudProviders.Account at an account path.
pub struct AccountCloudProvider(pub Arc<AccountShared>);

#[interface(name = "org.freedesktop.CloudProviders.Account")]
impl AccountCloudProvider {
    #[zbus(property, name = "Name")]
    fn name(&self) -> fdo::Result<String> {
        self.0.account.name()
    }

    #[zbus(property, name = "Path")]
    fn path(&self) -> fdo::Result<String> {
        self.0.account.path()
    }

    #[zbus(property, name = "Icon")]
    fn icon(&self) -> fdo::Result<String> {
        self.0.account.icon()
    }

    #[zbus(property, name = "Status")]
    fn status(&self) -> fdo::Result<i32> {
        self.0.account.status()
    }

    #[zbus(property, name = "StatusDetails")]
    fn status_details(&self) -> fdo::Result<String> {
        let base = self.0.account.status_details()?;
        let cache_size = dir_size_bytes(&self.0.cache_root).unwrap_or(0);
        let cache_str = format_size(cache_size);
        if cache_str.is_empty() {
            Ok(base)
        } else {
            Ok(format!("{} · Cache: {}", base, cache_str))
        }
    }
}

/// Exposes org.gtk.Actions at an account path.
pub struct AccountGtkActions(pub Arc<AccountShared>);

#[interface(name = "org.gtk.Actions")]
impl AccountGtkActions {
    fn list(&self) -> fdo::Result<Vec<String>> {
        Ok(gtk_dbus::action_names()
            .iter()
            .map(|s| (*s).to_string())
            .collect())
    }

    fn describe(&self, name: &str) -> fdo::Result<(bool, String, Vec<OwnedValue>)> {
        let enabled = gtk_dbus::action_names().contains(&name);
        Ok(gtk_dbus::describe_action(enabled))
    }

    fn describe_all(&self) -> fdo::Result<HashMap<String, (bool, String, Vec<OwnedValue>)>> {
        let mut out = HashMap::new();
        for name in gtk_dbus::action_names() {
            out.insert((*name).to_string(), gtk_dbus::describe_action(true));
        }
        Ok(out)
    }

    fn activate(
        &self,
        name: &str,
        _params: Vec<OwnedValue>,
        _platform_data: HashMap<String, OwnedValue>,
    ) -> fdo::Result<()> {
        if gtk_dbus::action_names().contains(&name) {
            let _ = self.0.action_tx.send((
                self.0.mount_path.clone(),
                self.0.cache_root.clone(),
                name.to_string(),
            ));
        }
        Ok(())
    }
}

/// Exposes org.gtk.Menus at an account path.
pub struct AccountGtkMenus(pub Arc<AccountShared>);

#[interface(name = "org.gtk.Menus")]
impl AccountGtkMenus {
    fn start(
        &self,
        _groups: Vec<u32>,
    ) -> fdo::Result<Vec<(u32, u32, Vec<HashMap<String, OwnedValue>>)>> {
        Ok(gtk_dbus::build_start_reply())
    }

    fn end(&self, _groups: Vec<u32>) -> fdo::Result<()> {
        Ok(())
    }
}

/// Build the three interface objects for one account; register all at the same path.
pub fn new_account_interfaces(
    account: AccountExporter,
    mount_path: String,
    cache_root: PathBuf,
    action_tx: UnboundedSender<ActionMessage>,
) -> (AccountCloudProvider, AccountGtkActions, AccountGtkMenus) {
    let shared = Arc::new(AccountShared {
        account,
        mount_path,
        cache_root,
        action_tx,
    });
    (
        AccountCloudProvider(Arc::clone(&shared)),
        AccountGtkActions(shared.clone()),
        AccountGtkMenus(shared),
    )
}

/// Request the well-known bus name for the cloud provider.
pub async fn request_bus_name(connection: &zbus::Connection) -> zbus::Result<()> {
    connection.request_name(BUS_NAME).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_exporter_name() {
        let p = ProviderExporter::default();
        assert_eq!(p.name().unwrap(), "Anymount");
    }

    #[test]
    fn account_exporter_properties() {
        let a = AccountExporter {
            name: "TestAccount".to_string(),
            path: "/run/user/1000/anymount".to_string(),
            icon: "folder".to_string(),
            status: 0,
            status_details: "Idle".to_string(),
        };
        assert_eq!(a.name().unwrap(), "TestAccount");
        assert_eq!(a.path().unwrap(), "/run/user/1000/anymount");
        assert_eq!(a.icon().unwrap(), "folder");
        assert_eq!(a.status().unwrap(), 0);
        assert_eq!(a.status_details().unwrap(), "Idle");
    }
}
