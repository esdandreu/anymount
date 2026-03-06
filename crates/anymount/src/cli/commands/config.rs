use crate::StorageConfig;
use crate::cli::commands::connect::{
    ConnectStorageSubcommand, LocalStorageArgs, OneDriveStorageArgs,
};
use crate::config::{ConfigDir, ProviderFileConfig};
use clap::{Args, Subcommand};
use inquire::{Select, Text};
use std::path::PathBuf;

#[derive(Args, Debug, Clone)]
pub struct ConfigCommand {
    /// Path to config directory (defaults to platform config dir).
    #[arg(long, global = true)]
    pub config_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub action: ConfigAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigAction {
    /// List all configured providers.
    List,
    /// Show a provider's configuration.
    Show(ShowArgs),
    /// Add a new provider configuration.
    Add(AddArgs),
    /// Remove a provider configuration.
    Remove(RemoveArgs),
    /// Set a field in an existing provider configuration.
    Set(SetArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ShowArgs {
    /// Provider name (filename without .toml).
    pub name: String,
}

#[derive(Args, Debug, Clone)]
pub struct AddArgs {
    /// Provider name (becomes <name>.toml).
    pub name: Option<String>,
    /// Mount-point path.
    #[arg(long)]
    pub path: Option<PathBuf>,
    #[command(subcommand)]
    pub storage: Option<ConnectStorageSubcommand>,
}

#[derive(Args, Debug, Clone)]
pub struct RemoveArgs {
    /// Provider name to remove.
    pub name: String,
}

#[derive(Args, Debug, Clone)]
pub struct SetArgs {
    /// Provider name to modify.
    pub name: String,
    /// Dotted key (e.g. `path`, `storage.root`,
    /// `storage.endpoint`).
    pub key: String,
    /// New value.
    pub value: String,
}

impl ConfigCommand {
    pub fn execute(&self) -> Result<(), String> {
        let cd = match &self.config_dir {
            Some(p) => ConfigDir::new(p.clone()),
            None => ConfigDir::default(),
        };

        match &self.action {
            ConfigAction::List => execute_list(&cd),
            ConfigAction::Show(args) => execute_show(&cd, args),
            ConfigAction::Add(args) => execute_add(&cd, args),
            ConfigAction::Remove(args) => execute_remove(&cd, args),
            ConfigAction::Set(args) => execute_set(&cd, args),
        }
    }
}

fn execute_list(cd: &ConfigDir) -> Result<(), String> {
    let names = cd.list()?;
    if names.is_empty() {
        println!("No providers configured in {}", cd.dir().display());
    } else {
        for name in &names {
            println!("{name}");
        }
    }
    Ok(())
}

fn execute_show(cd: &ConfigDir, args: &ShowArgs) -> Result<(), String> {
    let cfg = cd.read(&args.name)?;
    let text = toml::to_string_pretty(&cfg).map_err(|e| format!("cannot serialize config: {e}"))?;
    print!("{text}");
    Ok(())
}

fn execute_add(cd: &ConfigDir, args: &AddArgs) -> Result<(), String> {
    let resolved = resolve_add_args(args)?;
    let existing = cd.list()?;
    if existing.contains(&resolved.name) {
        return Err(format!(
            "provider '{}' already exists, use 'set' to modify \
             or 'remove' first",
            resolved.name
        ));
    }

    let storage = storage_config_from_subcommand(&resolved.storage);
    let cfg = ProviderFileConfig {
        path: resolved.path.clone(),
        storage,
    };
    cd.write(&resolved.name, &cfg)?;
    println!("Added provider '{}'", resolved.name);
    Ok(())
}

struct ResolvedAddArgs {
    name: String,
    path: PathBuf,
    storage: ConnectStorageSubcommand,
}

fn resolve_add_args(args: &AddArgs) -> Result<ResolvedAddArgs, String> {
    let name = match &args.name {
        Some(n) => n.clone(),
        None => prompt_name()?,
    };
    let path = match &args.path {
        Some(p) => p.clone(),
        None => prompt_path()?,
    };
    let storage = match &args.storage {
        Some(s) => s.clone(),
        None => prompt_storage()?,
    };
    Ok(ResolvedAddArgs {
        name,
        path,
        storage,
    })
}

fn prompt_name() -> Result<String, String> {
    Text::new("Provider name:")
        .with_help_message("This becomes <name>.toml in your config directory")
        .prompt()
        .map_err(|e| format!("failed to read provider name: {e}"))
}

fn prompt_path() -> Result<PathBuf, String> {
    let input = Text::new("Mount-point path:")
        .with_help_message("The local path where the provider will be mounted")
        .prompt()
        .map_err(|e| format!("failed to read mount path: {e}"))?;
    Ok(PathBuf::from(input))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderType {
    Local,
    OneDrive,
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local => write!(f, "Local directory"),
            Self::OneDrive => write!(f, "OneDrive (Microsoft Graph)"),
        }
    }
}

fn prompt_storage() -> Result<ConnectStorageSubcommand, String> {
    let options = vec![ProviderType::Local, ProviderType::OneDrive];
    let selected = Select::new("Select provider type:", options)
        .prompt()
        .map_err(|e| format!("failed to select provider type: {e}"))?;

    match selected {
        ProviderType::Local => prompt_local_storage(),
        ProviderType::OneDrive => prompt_onedrive_storage(),
    }
}

fn prompt_local_storage() -> Result<ConnectStorageSubcommand, String> {
    let root = Text::new("Root directory to expose:")
        .prompt()
        .map_err(|e| format!("failed to read root directory: {e}"))?;
    Ok(ConnectStorageSubcommand::Local(LocalStorageArgs {
        root: PathBuf::from(root),
    }))
}

fn prompt_onedrive_storage() -> Result<ConnectStorageSubcommand, String> {
    let root = Text::new("OneDrive path to use as root:")
        .with_default("/")
        .prompt()
        .map_err(|e| format!("failed to read OneDrive root: {e}"))?;

    let endpoint = Text::new("Graph API endpoint:")
        .with_default("https://graph.microsoft.com/v1.0")
        .prompt()
        .map_err(|e| format!("failed to read endpoint: {e}"))?;

    let access_token = prompt_optional("Access token (optional):")?;
    let refresh_token = prompt_optional("Refresh token (optional):")?;
    let client_id = prompt_optional("OAuth client_id (optional):")?;

    let token_expiry_buffer_secs = Text::new("Token expiry buffer (seconds):")
        .with_default("60")
        .prompt()
        .map_err(|e| format!("failed to read token expiry buffer: {e}"))?
        .parse::<u64>()
        .map_err(|e| format!("invalid number: {e}"))?;

    Ok(ConnectStorageSubcommand::OneDrive(OneDriveStorageArgs {
        root: PathBuf::from(root),
        endpoint,
        access_token,
        refresh_token,
        client_id,
        token_expiry_buffer_secs,
    }))
}

fn prompt_optional(message: &str) -> Result<Option<String>, String> {
    let input = Text::new(message)
        .prompt()
        .map_err(|e| format!("failed to read input: {e}"))?;
    if input.is_empty() {
        Ok(None)
    } else {
        Ok(Some(input))
    }
}

fn execute_remove(cd: &ConfigDir, args: &RemoveArgs) -> Result<(), String> {
    cd.remove(&args.name)?;
    println!("Removed provider '{}'", args.name);
    Ok(())
}

fn execute_set(cd: &ConfigDir, args: &SetArgs) -> Result<(), String> {
    let mut cfg = cd.read(&args.name)?;
    apply_set(&mut cfg, &args.key, &args.value)?;
    cd.write(&args.name, &cfg)?;
    println!("Updated '{}' in provider '{}'", args.key, args.name);
    Ok(())
}

fn storage_config_from_subcommand(sub: &ConnectStorageSubcommand) -> StorageConfig {
    match sub {
        ConnectStorageSubcommand::Local(a) => StorageConfig::Local {
            root: a.root.clone(),
        },
        ConnectStorageSubcommand::OneDrive(a) => StorageConfig::OneDrive {
            root: a.root.clone(),
            endpoint: a.endpoint.clone(),
            access_token: a.access_token.clone(),
            refresh_token: a.refresh_token.clone(),
            client_id: a.client_id.clone(),
            token_expiry_buffer_secs: Some(a.token_expiry_buffer_secs),
        },
    }
}

fn apply_set(cfg: &mut ProviderFileConfig, key: &str, value: &str) -> Result<(), String> {
    match key {
        "path" => {
            cfg.path = PathBuf::from(value);
        }
        "storage.root" => match &mut cfg.storage {
            StorageConfig::Local { root } | StorageConfig::OneDrive { root, .. } => {
                *root = PathBuf::from(value);
            }
        },
        "storage.endpoint" => match &mut cfg.storage {
            StorageConfig::OneDrive { endpoint, .. } => {
                *endpoint = value.to_owned();
            }
            _ => {
                return Err("'storage.endpoint' only applies to \
                     onedrive storage"
                    .to_owned());
            }
        },
        "storage.access_token" => match &mut cfg.storage {
            StorageConfig::OneDrive { access_token, .. } => {
                *access_token = Some(value.to_owned());
            }
            _ => {
                return Err("'storage.access_token' only applies to \
                     onedrive storage"
                    .to_owned());
            }
        },
        "storage.refresh_token" => match &mut cfg.storage {
            StorageConfig::OneDrive { refresh_token, .. } => {
                *refresh_token = Some(value.to_owned());
            }
            _ => {
                return Err("'storage.refresh_token' only applies to \
                     onedrive storage"
                    .to_owned());
            }
        },
        "storage.client_id" => match &mut cfg.storage {
            StorageConfig::OneDrive { client_id, .. } => {
                *client_id = Some(value.to_owned());
            }
            _ => {
                return Err("'storage.client_id' only applies to \
                     onedrive storage"
                    .to_owned());
            }
        },
        "storage.token_expiry_buffer_secs" => match &mut cfg.storage {
            StorageConfig::OneDrive {
                token_expiry_buffer_secs,
                ..
            } => {
                let secs: u64 = value
                    .parse()
                    .map_err(|_| format!("invalid u64 value: '{value}'"))?;
                *token_expiry_buffer_secs = Some(secs);
            }
            _ => {
                return Err("'storage.token_expiry_buffer_secs' \
                         only applies to onedrive storage"
                    .to_owned());
            }
        },
        _ => {
            return Err(format!(
                "unknown key '{key}'. Valid keys: path, \
                 storage.root, storage.endpoint, \
                 storage.access_token, \
                 storage.refresh_token, storage.client_id, \
                 storage.token_expiry_buffer_secs"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands::connect::LocalStorageArgs;

    fn local_config() -> ProviderFileConfig {
        ProviderFileConfig {
            path: PathBuf::from("/mnt/local"),
            storage: StorageConfig::Local {
                root: PathBuf::from("/data"),
            },
        }
    }

    fn onedrive_config() -> ProviderFileConfig {
        ProviderFileConfig {
            path: PathBuf::from("/mnt/onedrive"),
            storage: StorageConfig::OneDrive {
                root: PathBuf::from("/"),
                endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
                access_token: None,
                refresh_token: Some("rt".to_owned()),
                client_id: Some("cid".to_owned()),
                token_expiry_buffer_secs: Some(60),
            },
        }
    }

    #[test]
    fn apply_set_path() {
        let mut cfg = local_config();
        apply_set(&mut cfg, "path", "/new/path").expect("set failed");
        assert_eq!(cfg.path, PathBuf::from("/new/path"));
    }

    #[test]
    fn apply_set_storage_root_local() {
        let mut cfg = local_config();
        apply_set(&mut cfg, "storage.root", "/new/root").expect("set failed");
        if let StorageConfig::Local { root } = &cfg.storage {
            assert_eq!(root, &PathBuf::from("/new/root"));
        } else {
            panic!("expected Local");
        }
    }

    #[test]
    fn apply_set_storage_root_onedrive() {
        let mut cfg = onedrive_config();
        apply_set(&mut cfg, "storage.root", "/Documents").expect("set failed");
        if let StorageConfig::OneDrive { root, .. } = &cfg.storage {
            assert_eq!(root, &PathBuf::from("/Documents"));
        } else {
            panic!("expected OneDrive");
        }
    }

    #[test]
    fn apply_set_endpoint() {
        let mut cfg = onedrive_config();
        apply_set(&mut cfg, "storage.endpoint", "https://other.api").expect("set failed");
        if let StorageConfig::OneDrive { endpoint, .. } = &cfg.storage {
            assert_eq!(endpoint, "https://other.api");
        } else {
            panic!("expected OneDrive");
        }
    }

    #[test]
    fn apply_set_endpoint_on_local_fails() {
        let mut cfg = local_config();
        assert!(apply_set(&mut cfg, "storage.endpoint", "x").is_err());
    }

    #[test]
    fn apply_set_token_expiry_buffer_secs() {
        let mut cfg = onedrive_config();
        apply_set(&mut cfg, "storage.token_expiry_buffer_secs", "120").expect("set failed");
        if let StorageConfig::OneDrive {
            token_expiry_buffer_secs,
            ..
        } = &cfg.storage
        {
            assert_eq!(*token_expiry_buffer_secs, Some(120));
        } else {
            panic!("expected OneDrive");
        }
    }

    #[test]
    fn apply_set_invalid_u64_fails() {
        let mut cfg = onedrive_config();
        assert!(apply_set(&mut cfg, "storage.token_expiry_buffer_secs", "not_a_number").is_err());
    }

    #[test]
    fn apply_set_unknown_key_fails() {
        let mut cfg = local_config();
        assert!(apply_set(&mut cfg, "nonexistent", "x").is_err());
    }

    #[test]
    fn execute_add_rejects_duplicate() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        cd.write("dup", &local_config()).expect("write failed");

        let args = AddArgs {
            name: Some("dup".to_owned()),
            path: Some(PathBuf::from("/mnt/x")),
            storage: Some(ConnectStorageSubcommand::Local(LocalStorageArgs {
                root: PathBuf::from("/x"),
            })),
        };
        assert!(execute_add(&cd, &args).is_err());
    }

    #[test]
    fn execute_add_and_show_roundtrip() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());

        let args = AddArgs {
            name: Some("test".to_owned()),
            path: Some(PathBuf::from("/mnt/test")),
            storage: Some(ConnectStorageSubcommand::Local(LocalStorageArgs {
                root: PathBuf::from("/test/root"),
            })),
        };
        execute_add(&cd, &args).expect("add failed");

        let cfg = cd.read("test").expect("read failed");
        assert_eq!(cfg.path, PathBuf::from("/mnt/test"));
    }

    #[test]
    fn execute_set_modifies_existing() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        cd.write("edit", &local_config()).expect("write failed");

        let args = SetArgs {
            name: "edit".to_owned(),
            key: "path".to_owned(),
            value: "/updated".to_owned(),
        };
        execute_set(&cd, &args).expect("set failed");

        let cfg = cd.read("edit").expect("read failed");
        assert_eq!(cfg.path, PathBuf::from("/updated"));
    }

    #[test]
    fn provider_type_display_local() {
        assert_eq!(format!("{}", ProviderType::Local), "Local directory");
    }

    #[test]
    fn provider_type_display_onedrive() {
        assert_eq!(
            format!("{}", ProviderType::OneDrive),
            "OneDrive (Microsoft Graph)"
        );
    }

    #[test]
    fn resolve_add_args_with_all_fields() {
        let args = AddArgs {
            name: Some("my-provider".to_owned()),
            path: Some(PathBuf::from("/mnt/test")),
            storage: Some(ConnectStorageSubcommand::Local(LocalStorageArgs {
                root: PathBuf::from("/data"),
            })),
        };
        let resolved = resolve_add_args(&args).expect("resolve failed");
        assert_eq!(resolved.name, "my-provider");
        assert_eq!(resolved.path, PathBuf::from("/mnt/test"));
    }
}
