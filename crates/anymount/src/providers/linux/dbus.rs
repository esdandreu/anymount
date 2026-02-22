//! D-Bus implementation of org.freedesktop.CloudProviders (Provider and Account).

use zbus::fdo;
use zbus::interface;

const PROVIDER_NAME: &str = "Anymount";
const PROVIDER_PATH: &str = "/org/anymount/CloudProviders";
const BUS_NAME: &str = "org.anymount.CloudProviders";

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

/// Request the well-known bus name for the cloud provider.
pub async fn request_bus_name(connection: &zbus::Connection) -> zbus::Result<()> {
    connection.request_name(BUS_NAME).await?;
    Ok(())
}

pub use PROVIDER_PATH;
pub use BUS_NAME;

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
