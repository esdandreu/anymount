#![allow(unused_imports)]
use crate::application::config::{
    Application as ConfigApplication, ConfigRepository, ConfigUseCase,
    Error as ConfigApplicationError,
};
use crate::cli::commands::connect_sync::{
    ConnectSyncStorageSubcommand, LocalStorageArgs, OneDriveStorageArgs,
};
use crate::config::{ConfigDir, DriverFileConfig};
use crate::domain::driver::{DriverConfig, StorageConfig};
use clap::{Args, Subcommand};
use inquire::{Select, Text};
use std::path::{Path, PathBuf};

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
    pub storage: Option<ConnectSyncStorageSubcommand>,
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
    pub fn execute(&self) -> crate::cli::Result<()> {
        let config_dir = self.config_dir();
        let repository = ConfigRepositoryAdapter::new(config_dir.clone());
        let app = ConfigApplication::new(&repository);
        self._execute(&app, config_dir.dir())
    }

    pub(crate) fn _execute<U>(&self, use_case: &U, config_dir: &Path) -> crate::cli::Result<()>
    where
        U: ConfigUseCase,
    {
        match &self.action {
            ConfigAction::List => execute_list(use_case, config_dir),
            ConfigAction::Show(args) => execute_show(use_case, args),
            ConfigAction::Add(args) => execute_add(use_case, args),
            ConfigAction::Remove(args) => execute_remove(use_case, args),
            ConfigAction::Set(args) => execute_set(use_case, args),
        }
    }

    fn config_dir(&self) -> ConfigDir {
        match &self.config_dir {
            Some(path) => ConfigDir::new(path.clone()),
            None => ConfigDir::default(),
        }
    }
}

#[derive(Debug, Clone)]
struct ConfigRepositoryAdapter {
    config_dir: ConfigDir,
}

impl ConfigRepositoryAdapter {
    fn new(config_dir: ConfigDir) -> Self {
        Self { config_dir }
    }
}

impl ConfigRepository for ConfigRepositoryAdapter {
    fn list_names(&self) -> crate::application::config::Result<Vec<String>> {
        self.config_dir.list().map_err(Into::into)
    }

    fn read_spec(&self, name: &str) -> crate::application::config::Result<DriverConfig> {
        self.config_dir.read_spec(name).map_err(Into::into)
    }

    fn write_spec(&self, spec: &DriverConfig) -> crate::application::config::Result<()> {
        self.config_dir.write_spec(spec).map_err(Into::into)
    }

    fn remove(&self, name: &str) -> crate::application::config::Result<()> {
        self.config_dir.remove(name).map_err(Into::into)
    }
}

fn execute_list<U>(use_case: &U, config_dir: &Path) -> crate::cli::Result<()>
where
    U: ConfigUseCase,
{
    let names = use_case.list().map_err(map_config_error)?;
    if names.is_empty() {
        println!("No providers configured in {}", config_dir.display());
    } else {
        for name in &names {
            println!("{name}");
        }
    }
    Ok(())
}

fn execute_show<U>(use_case: &U, args: &ShowArgs) -> crate::cli::Result<()>
where
    U: ConfigUseCase,
{
    let spec = use_case.read(&args.name).map_err(map_config_error)?;
    let cfg = config_from_spec(&spec);
    let text = toml::to_string_pretty(&cfg)
        .map_err(|source| crate::cli::Error::SerializeConfig { source })?;
    print!("{text}");
    Ok(())
}

fn execute_add<U>(use_case: &U, args: &AddArgs) -> crate::cli::Result<()>
where
    U: ConfigUseCase,
{
    let resolved = resolve_add_args(args)?;
    let spec = DriverConfig {
        name: resolved.name.clone(),
        path: resolved.path,
        storage: resolved.storage.to_storage_config(),
        telemetry: Default::default(),
    };
    use_case.add(spec).map_err(map_config_error)?;
    println!("Added driver '{}'", resolved.name);
    Ok(())
}

struct ResolvedAddArgs {
    name: String,
    path: PathBuf,
    storage: ConnectSyncStorageSubcommand,
}

fn resolve_add_args(args: &AddArgs) -> crate::cli::Result<ResolvedAddArgs> {
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

fn prompt_name() -> crate::cli::Result<String> {
    Text::new("Provider name:")
        .with_help_message("This becomes <name>.toml in your config directory")
        .prompt()
        .map_err(|error| {
            crate::cli::Error::Prompt(format!("failed to read provider name: {error}"))
        })
}

fn prompt_path() -> crate::cli::Result<PathBuf> {
    let input = Text::new("Mount-point path:")
        .with_help_message("The local path where the provider will be mounted")
        .prompt()
        .map_err(|error| {
            crate::cli::Error::Prompt(format!("failed to read mount path: {error}"))
        })?;
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

fn prompt_storage() -> crate::cli::Result<ConnectSyncStorageSubcommand> {
    let options = vec![ProviderType::Local, ProviderType::OneDrive];
    let selected = Select::new("Select provider type:", options)
        .prompt()
        .map_err(|error| {
            crate::cli::Error::Prompt(format!("failed to select provider type: {error}"))
        })?;

    match selected {
        ProviderType::Local => prompt_local_storage(),
        ProviderType::OneDrive => prompt_onedrive_storage(),
    }
}

fn prompt_local_storage() -> crate::cli::Result<ConnectSyncStorageSubcommand> {
    let root = Text::new("Root directory to expose:")
        .prompt()
        .map_err(|error| {
            crate::cli::Error::Prompt(format!("failed to read root directory: {error}"))
        })?;
    Ok(ConnectSyncStorageSubcommand::Local(LocalStorageArgs {
        root: PathBuf::from(root),
    }))
}

fn prompt_onedrive_storage() -> crate::cli::Result<ConnectSyncStorageSubcommand> {
    let root = Text::new("OneDrive path to use as root:")
        .with_default("/")
        .prompt()
        .map_err(|error| {
            crate::cli::Error::Prompt(format!("failed to read OneDrive root: {error}"))
        })?;

    let endpoint = Text::new("Graph API endpoint:")
        .with_default("https://graph.microsoft.com/v1.0")
        .prompt()
        .map_err(|error| crate::cli::Error::Prompt(format!("failed to read endpoint: {error}")))?;

    let access_token = prompt_optional("Access token (optional):")?;
    let refresh_token = prompt_optional("Refresh token (optional):")?;
    let client_id = prompt_optional("OAuth client_id (optional):")?;

    let token_expiry_buffer_secs = Text::new("Token expiry buffer (seconds):")
        .with_default("60")
        .prompt()
        .map_err(|error| {
            crate::cli::Error::Prompt(format!("failed to read token expiry buffer: {error}"))
        })?;
    let token_expiry_buffer_secs = parse_u64(token_expiry_buffer_secs)?;

    Ok(ConnectSyncStorageSubcommand::OneDrive(
        OneDriveStorageArgs {
            root: PathBuf::from(root),
            endpoint,
            access_token,
            refresh_token,
            client_id,
            token_expiry_buffer_secs,
        },
    ))
}

fn prompt_optional(message: &str) -> crate::cli::Result<Option<String>> {
    let input = Text::new(message)
        .prompt()
        .map_err(|error| crate::cli::Error::Prompt(format!("failed to read input: {error}")))?;
    if input.is_empty() {
        Ok(None)
    } else {
        Ok(Some(input))
    }
}

fn execute_remove<U>(use_case: &U, args: &RemoveArgs) -> crate::cli::Result<()>
where
    U: ConfigUseCase,
{
    use_case.remove(&args.name).map_err(map_config_error)?;
    println!("Removed provider '{}'", args.name);
    Ok(())
}

fn execute_set<U>(use_case: &U, args: &SetArgs) -> crate::cli::Result<()>
where
    U: ConfigUseCase,
{
    use_case
        .set(&args.name, &args.key, &args.value)
        .map_err(map_config_error)?;
    println!("Updated '{}' in provider '{}'", args.key, args.name);
    Ok(())
}

fn parse_u64(value: String) -> crate::cli::Result<u64> {
    value
        .parse::<u64>()
        .map_err(|source| crate::cli::Error::ParseInteger { value, source })
}

fn config_from_spec(spec: &DriverConfig) -> DriverFileConfig {
    DriverFileConfig {
        path: spec.path.clone(),
        storage: spec.storage.clone().into(),
        telemetry: spec.telemetry.clone().into(),
    }
}

#[cfg(test)]
fn apply_set(cfg: &mut DriverFileConfig, key: &str, value: &str) -> crate::cli::Result<()> {
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
                return Err(crate::cli::Error::Validation(
                    "'storage.endpoint' only applies to onedrive storage".to_owned(),
                ));
            }
        },
        "storage.access_token" => match &mut cfg.storage {
            StorageConfig::OneDrive { access_token, .. } => {
                *access_token = Some(value.to_owned());
            }
            _ => {
                return Err(crate::cli::Error::Validation(
                    "'storage.access_token' only applies to onedrive storage".to_owned(),
                ));
            }
        },
        "storage.refresh_token" => match &mut cfg.storage {
            StorageConfig::OneDrive { refresh_token, .. } => {
                *refresh_token = Some(value.to_owned());
            }
            _ => {
                return Err(crate::cli::Error::Validation(
                    "'storage.refresh_token' only applies to onedrive storage".to_owned(),
                ));
            }
        },
        "storage.client_id" => match &mut cfg.storage {
            StorageConfig::OneDrive { client_id, .. } => {
                *client_id = Some(value.to_owned());
            }
            _ => {
                return Err(crate::cli::Error::Validation(
                    "'storage.client_id' only applies to onedrive storage".to_owned(),
                ));
            }
        },
        "storage.token_expiry_buffer_secs" => match &mut cfg.storage {
            StorageConfig::OneDrive {
                token_expiry_buffer_secs,
                ..
            } => {
                let secs = parse_u64(value.to_owned())?;
                *token_expiry_buffer_secs = Some(secs);
            }
            _ => {
                return Err(crate::cli::Error::Validation(
                    "'storage.token_expiry_buffer_secs' only applies to onedrive storage"
                        .to_owned(),
                ));
            }
        },
        _ => {
            return Err(crate::cli::Error::Validation(format!(
                "unknown key '{key}'. Valid keys: path, \
                 storage.root, storage.endpoint, \
                 storage.access_token, \
                 storage.refresh_token, storage.client_id, \
                 storage.token_expiry_buffer_secs"
            )));
        }
    }
    Ok(())
}

fn map_config_error(error: ConfigApplicationError) -> crate::cli::Error {
    match error {
        ConfigApplicationError::Config(source) => crate::cli::Error::Config(source),
        ConfigApplicationError::DuplicateDriver { name } => crate::cli::Error::Validation(format!(
            "driver '{name}' already exists, use 'set' to modify or 'remove' first"
        )),
        ConfigApplicationError::InvalidStorageKey { key } => {
            crate::cli::Error::Validation(format!("'{key}' only applies to onedrive storage"))
        }
        ConfigApplicationError::UnknownKey { key } => crate::cli::Error::Validation(format!(
            "unknown key '{key}'. Valid keys: path, \
                 storage.root, storage.endpoint, \
                 storage.access_token, \
                 storage.refresh_token, storage.client_id, \
                 storage.token_expiry_buffer_secs"
        )),
        ConfigApplicationError::ParseInteger { value, source } => {
            crate::cli::Error::ParseInteger { value, source }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::config::{ConfigUseCase, Result as ConfigApplicationResult};
    use crate::cli::commands::connect_sync::{ConnectSyncStorageSubcommand, LocalStorageArgs};
    use crate::domain::driver::{DriverConfig, StorageConfig, TelemetrySpec};
    use std::cell::RefCell;

    fn local_config() -> DriverFileConfig {
        DriverFileConfig {
            path: PathBuf::from("/mnt/local"),
            storage: StorageConfig::Local {
                root: PathBuf::from("/data"),
            },
            telemetry: Default::default(),
        }
    }

    fn local_spec(name: &str) -> DriverConfig {
        DriverConfig {
            name: name.to_owned(),
            path: PathBuf::from("/mnt/local"),
            storage: StorageConfig::Local {
                root: PathBuf::from("/data"),
            },
            telemetry: TelemetrySpec::default(),
        }
    }

    #[derive(Default)]
    struct RecordingUseCase {
        added: RefCell<Vec<DriverConfig>>,
        set_calls: RefCell<Vec<(String, String, String)>>,
    }

    impl ConfigUseCase for RecordingUseCase {
        fn list(&self) -> ConfigApplicationResult<Vec<String>> {
            Ok(Vec::new())
        }

        fn read(&self, name: &str) -> ConfigApplicationResult<DriverConfig> {
            Ok(local_spec(name))
        }

        fn add(&self, spec: DriverConfig) -> ConfigApplicationResult<()> {
            self.added.borrow_mut().push(spec);
            Ok(())
        }

        fn remove(&self, _name: &str) -> ConfigApplicationResult<()> {
            Ok(())
        }

        fn set(&self, name: &str, key: &str, value: &str) -> ConfigApplicationResult<()> {
            self.set_calls
                .borrow_mut()
                .push((name.to_owned(), key.to_owned(), value.to_owned()));
            Ok(())
        }
    }

    fn onedrive_config() -> DriverFileConfig {
        DriverFileConfig {
            path: PathBuf::from("/mnt/onedrive"),
            storage: StorageConfig::OneDrive {
                root: PathBuf::from("/"),
                endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
                access_token: None,
                refresh_token: Some("rt".to_owned()),
                client_id: Some("cid".to_owned()),
                token_expiry_buffer_secs: Some(60),
            },
            telemetry: Default::default(),
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
        let repository = ConfigRepositoryAdapter::new(cd.clone());
        let app = ConfigApplication::new(&repository);

        let args = AddArgs {
            name: Some("dup".to_owned()),
            path: Some(PathBuf::from("/mnt/x")),
            storage: Some(ConnectSyncStorageSubcommand::Local(LocalStorageArgs {
                root: PathBuf::from("/x"),
            })),
        };
        assert!(execute_add(&app, &args).is_err());
    }

    #[test]
    fn execute_dispatches_add_to_use_case() {
        let cmd = ConfigCommand {
            config_dir: None,
            action: ConfigAction::Add(AddArgs {
                name: Some("test".to_owned()),
                path: Some(PathBuf::from("/mnt/test")),
                storage: Some(ConnectSyncStorageSubcommand::Local(LocalStorageArgs {
                    root: PathBuf::from("/test/root"),
                })),
            }),
        };
        let use_case = RecordingUseCase::default();

        cmd._execute(&use_case, PathBuf::from("/tmp/config").as_path())
            .expect("add should work");

        let added = use_case.added.borrow();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].name, "test");
    }

    #[test]
    fn execute_add_and_show_roundtrip() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        let repository = ConfigRepositoryAdapter::new(cd.clone());
        let app = ConfigApplication::new(&repository);

        let args = AddArgs {
            name: Some("test".to_owned()),
            path: Some(PathBuf::from("/mnt/test")),
            storage: Some(ConnectSyncStorageSubcommand::Local(LocalStorageArgs {
                root: PathBuf::from("/test/root"),
            })),
        };
        execute_add(&app, &args).expect("add failed");

        let cfg = cd.read("test").expect("read failed");
        assert_eq!(cfg.path, PathBuf::from("/mnt/test"));
    }

    #[test]
    fn execute_set_modifies_existing() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        cd.write("edit", &local_config()).expect("write failed");
        let repository = ConfigRepositoryAdapter::new(cd.clone());
        let app = ConfigApplication::new(&repository);

        let args = SetArgs {
            name: "edit".to_owned(),
            key: "path".to_owned(),
            value: "/updated".to_owned(),
        };
        execute_set(&app, &args).expect("set failed");

        let cfg = cd.read("edit").expect("read failed");
        assert_eq!(cfg.path, PathBuf::from("/updated"));
    }

    #[test]
    fn execute_dispatches_set_to_use_case() {
        let cmd = ConfigCommand {
            config_dir: None,
            action: ConfigAction::Set(SetArgs {
                name: "edit".to_owned(),
                key: "path".to_owned(),
                value: "/updated".to_owned(),
            }),
        };
        let use_case = RecordingUseCase::default();

        cmd._execute(&use_case, PathBuf::from("/tmp/config").as_path())
            .expect("set should work");

        let calls = use_case.set_calls.borrow();
        assert_eq!(
            calls.as_slice(),
            [("edit".to_owned(), "path".to_owned(), "/updated".to_owned(),)]
        );
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
            storage: Some(ConnectSyncStorageSubcommand::Local(LocalStorageArgs {
                root: PathBuf::from("/data"),
            })),
        };
        let resolved = resolve_add_args(&args).expect("resolve failed");
        assert_eq!(resolved.name, "my-provider");
        assert_eq!(resolved.path, PathBuf::from("/mnt/test"));
    }
}
