use super::{Error, Result};
use crate::application::auth::{Application as AuthApplication, AuthUseCase};
use crate::application::config::{
    Application as ConfigApplication, ConfigRepository, ConfigUseCase,
};
use crate::application::connect::{
    Application as ConnectApplication, ConnectRepository, ConnectUseCase, ServiceControl,
    ServiceLauncher,
};
use crate::auth::{OneDriveAuthFlow, TokenResponse};
use crate::cli::commands::config::ProviderType;
use crate::config::ConfigDir;
use crate::domain::driver::{DriverConfig, StorageConfig};
use crate::{DriverFileConfig, Logger, TracingLogger};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEvent, MouseEventKind,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute};
use ratatui::layout::Rect;
use ratatui::prelude::Stylize;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{DefaultTerminal, Frame};
use std::fs;
use std::io::{Write, stdout};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const COLOR_CONNECTED: Color = Color::Green;
const COLOR_DISCONNECTED: Color = Color::DarkGray;
const COLOR_ROW_BG_NORMAL: Color = Color::Reset;
const COLOR_ROW_BG_HOVERED: Color = Color::Rgb(30, 40, 60);
const COLOR_ROW_BG_SELECTED: Color = Color::Rgb(45, 66, 99);
const COLOR_ROW_3D_SHADOW: Color = Color::DarkGray;
const COLOR_BUTTON: Color = Color::Cyan;
const COLOR_BUTTON_TEXT: Color = Color::Black;
const MIN_TERMINAL_WIDTH: u16 = 80;
const MIN_TERMINAL_HEIGHT: u16 = 24;
const MIN_NAME_WIDTH: u16 = 8;
const STORAGE_TYPE_WIDTH: u16 = 10;
const MIN_PATH_WIDTH: u16 = 8;
const BUTTONS_WIDTH: u16 = 13;
const STATUS_WIDTH: u16 = 2;
const COLUMN_GAP_WIDTH: u16 = 2;

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy)]
struct MountRowLayout {
    show_name: bool,
    show_path: bool,
    show_storage_type: bool,
    show_buttons: bool,
    preferred_path_width: u16,
    path_width: u16,
}

fn is_supported_size(area: Rect) -> bool {
    area.width >= MIN_TERMINAL_WIDTH && area.height >= MIN_TERMINAL_HEIGHT
}

fn compute_mount_row_layout(
    available_width: u16,
    show_path: bool,
    show_storage_type: bool,
    show_buttons: bool,
) -> MountRowLayout {
    let preferred_path_width = 25;
    let mut remaining = available_width;

    remaining = remaining.saturating_sub(STATUS_WIDTH);
    remaining = remaining.saturating_sub(MIN_NAME_WIDTH);

    let show_buttons = show_buttons && remaining >= BUTTONS_WIDTH + COLUMN_GAP_WIDTH;
    if show_buttons {
        remaining = remaining.saturating_sub(BUTTONS_WIDTH + COLUMN_GAP_WIDTH);
    }

    let mut show_storage_type = show_storage_type;
    if show_storage_type {
        let storage_budget = STORAGE_TYPE_WIDTH + COLUMN_GAP_WIDTH;
        if remaining >= storage_budget + MIN_PATH_WIDTH + COLUMN_GAP_WIDTH {
            remaining = remaining.saturating_sub(storage_budget);
        } else {
            show_storage_type = false;
        }
    }

    let mut path_width = 0;
    let mut show_path = show_path;
    if show_path {
        let path_budget = remaining.saturating_sub(COLUMN_GAP_WIDTH);
        if path_budget >= MIN_PATH_WIDTH {
            path_width = path_budget.min(preferred_path_width);
        } else {
            show_path = false;
        }
    }

    MountRowLayout {
        show_name: true,
        show_path,
        show_storage_type,
        show_buttons,
        preferred_path_width,
        path_width,
    }
}

#[derive(Debug, Clone)]
struct ProviderEntry {
    name: String,
    config: DriverFileConfig,
}

impl ProviderEntry {
    fn is_connected(&self) -> bool {
        crate::cli::provider_control::provider_daemon_ready(&self.name)
    }
}

#[derive(Debug, Clone)]
enum UiMode {
    Browse,
    Edit(EditSession),
    DeleteConfirm { name: String },
}

#[derive(Debug, Clone)]
struct AppState {
    providers: Vec<ProviderEntry>,
    selected: usize,
    hovered: usize,
    is_keyboard_mode: bool,
    status: String,
    mode: UiMode,
}

impl AppState {
    fn load<U>(use_case: &U) -> Result<Self>
    where
        U: ConfigUseCase,
    {
        let names = use_case.list()?;
        let mut providers = Vec::with_capacity(names.len());
        for name in names {
            let spec = use_case.read(&name)?;
            providers.push(provider_entry_from_spec(spec));
        }
        Ok(Self {
            providers,
            selected: 0,
            hovered: 0,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::Browse,
        })
    }

    fn refresh<U>(&mut self, use_case: &U) -> Result<()>
    where
        U: ConfigUseCase,
    {
        let selected_name = self.selected_name().map(ToOwned::to_owned);
        let refreshed = Self::load(use_case)?;
        self.providers = refreshed.providers;
        self.status = refreshed.status;
        self.hovered = 0;
        if let Some(name) = selected_name {
            if let Some(pos) = self
                .providers
                .iter()
                .position(|provider| provider.name == name)
            {
                self.selected = pos;
                return Ok(());
            }
        }
        self.selected = self.selected.min(self.providers.len().saturating_sub(1));
        Ok(())
    }

    fn selected_name(&self) -> Option<&str> {
        self.providers
            .get(self.selected)
            .map(|provider| provider.name.as_str())
    }

    fn selected_provider(&self) -> Option<&ProviderEntry> {
        self.providers.get(self.selected)
    }

    fn select_next(&mut self) {
        if self.providers.is_empty() {
            return;
        }
        self.hovered = (self.hovered + 1) % (self.providers.len() + 1);
        self.selected = self.hovered;
    }

    fn select_prev(&mut self) {
        if self.providers.is_empty() {
            return;
        }
        if self.hovered == 0 {
            self.hovered = self.providers.len();
        } else {
            self.hovered -= 1;
        }
        self.selected = self.hovered;
    }

    fn is_add_row(&self) -> bool {
        self.hovered >= self.providers.len()
    }
}

fn provider_entry_from_spec(spec: DriverConfig) -> ProviderEntry {
    let name = spec.name.clone();
    ProviderEntry {
        name,
        config: DriverFileConfig {
            path: spec.path,
            storage: spec.storage.into(),
            telemetry: spec.telemetry.into(),
        },
    }
}

#[derive(Debug, Clone)]
struct TuiConfigRepository {
    config_dir: ConfigDir,
}

impl TuiConfigRepository {
    fn new(config_dir: ConfigDir) -> Self {
        Self { config_dir }
    }
}

impl ConfigRepository for TuiConfigRepository {
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

#[derive(Debug, Clone)]
struct TuiConnectRepository {
    config_dir: ConfigDir,
}

impl TuiConnectRepository {
    fn new(config_dir: ConfigDir) -> Self {
        Self { config_dir }
    }
}

impl ConnectRepository for TuiConnectRepository {
    fn list_names(&self) -> crate::application::connect::Result<Vec<String>> {
        self.config_dir.list().map_err(Into::into)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct TuiServiceControl;

impl ServiceControl for TuiServiceControl {
    fn ready(&self, provider_name: &str) -> bool {
        crate::cli::provider_control::provider_daemon_ready(provider_name)
    }
}

#[derive(Debug, Clone)]
struct ProcessServiceLauncher<L: Logger> {
    logger: L,
}

impl<L: Logger> ProcessServiceLauncher<L> {
    fn new(logger: L) -> Self {
        Self { logger }
    }
}

impl<L: Logger> ServiceLauncher for ProcessServiceLauncher<L> {
    fn launch(&self, provider_name: &str, config_dir: &Path) -> std::result::Result<(), String> {
        let mut child =
            spawn_provider_process(provider_name, config_dir).map_err(|error| error.to_string())?;
        wait_until_ready(provider_name, &mut child, &self.logger).map_err(|error| error.to_string())
    }
}

const READY_TIMEOUT: Duration = Duration::from_secs(5);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditField {
    Name,
    Path,
    StorageType,
    LocalRoot,
    OneDriveRoot,
    OneDriveEndpoint,
    OneDriveAccessToken,
    OneDriveRefreshToken,
    OneDriveClientId,
    OneDriveTokenExpiryBufferSecs,
}

const EDIT_FIELDS: [EditField; 10] = [
    EditField::Name,
    EditField::Path,
    EditField::StorageType,
    EditField::LocalRoot,
    EditField::OneDriveRoot,
    EditField::OneDriveEndpoint,
    EditField::OneDriveAccessToken,
    EditField::OneDriveRefreshToken,
    EditField::OneDriveClientId,
    EditField::OneDriveTokenExpiryBufferSecs,
];

impl EditField {
    fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Path => "path",
            Self::StorageType => "storage.type",
            Self::LocalRoot => "storage.local.root",
            Self::OneDriveRoot => "storage.onedrive.root",
            Self::OneDriveEndpoint => "storage.onedrive.endpoint",
            Self::OneDriveAccessToken => "storage.onedrive.access_token",
            Self::OneDriveRefreshToken => "storage.onedrive.refresh_token",
            Self::OneDriveClientId => "storage.onedrive.client_id",
            Self::OneDriveTokenExpiryBufferSecs => "storage.onedrive.token_expiry_buffer_secs",
        }
    }
}

#[derive(Debug, Clone)]
struct EditDraft {
    name: String,
    path: String,
    storage_type: ProviderType,
    local_root: String,
    onedrive_root: String,
    onedrive_endpoint: String,
    onedrive_access_token: String,
    onedrive_refresh_token: String,
    onedrive_client_id: String,
    onedrive_token_expiry_buffer_secs: String,
}

impl EditDraft {
    fn new_empty(default_name: String) -> Self {
        Self {
            name: default_name,
            path: String::new(),
            storage_type: ProviderType::Local,
            local_root: String::new(),
            onedrive_root: "/".to_owned(),
            onedrive_endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
            onedrive_access_token: String::new(),
            onedrive_refresh_token: String::new(),
            onedrive_client_id: String::new(),
            onedrive_token_expiry_buffer_secs: String::new(),
        }
    }

    fn from_provider(provider: &ProviderEntry) -> Self {
        match &provider.config.storage {
            StorageConfig::Local { root } => Self {
                name: provider.name.clone(),
                path: provider.config.path.display().to_string(),
                storage_type: ProviderType::Local,
                local_root: root.display().to_string(),
                onedrive_root: root.display().to_string(),
                onedrive_endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
                onedrive_access_token: String::new(),
                onedrive_refresh_token: String::new(),
                onedrive_client_id: String::new(),
                onedrive_token_expiry_buffer_secs: String::new(),
            },
            StorageConfig::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            } => Self {
                name: provider.name.clone(),
                path: provider.config.path.display().to_string(),
                storage_type: ProviderType::OneDrive,
                local_root: root.display().to_string(),
                onedrive_root: root.display().to_string(),
                onedrive_endpoint: endpoint.clone(),
                onedrive_access_token: access_token.clone().unwrap_or_default(),
                onedrive_refresh_token: refresh_token.clone().unwrap_or_default(),
                onedrive_client_id: client_id.clone().unwrap_or_default(),
                onedrive_token_expiry_buffer_secs: token_expiry_buffer_secs
                    .map(|secs| secs.to_string())
                    .unwrap_or_default(),
            },
        }
    }

    fn field_value(&self, field: EditField) -> String {
        match field {
            EditField::Name => self.name.clone(),
            EditField::Path => self.path.clone(),
            EditField::StorageType => match self.storage_type {
                ProviderType::Local => "local".to_owned(),
                ProviderType::OneDrive => "onedrive".to_owned(),
            },
            EditField::LocalRoot => self.local_root.clone(),
            EditField::OneDriveRoot => self.onedrive_root.clone(),
            EditField::OneDriveEndpoint => self.onedrive_endpoint.clone(),
            EditField::OneDriveAccessToken => self.onedrive_access_token.clone(),
            EditField::OneDriveRefreshToken => self.onedrive_refresh_token.clone(),
            EditField::OneDriveClientId => self.onedrive_client_id.clone(),
            EditField::OneDriveTokenExpiryBufferSecs => {
                self.onedrive_token_expiry_buffer_secs.clone()
            }
        }
    }

    fn set_field_value(&mut self, field: EditField, value: String) {
        match field {
            EditField::Name => self.name = value,
            EditField::Path => self.path = value,
            EditField::StorageType => {
                self.storage_type = match value.to_lowercase().as_str() {
                    "local" => ProviderType::Local,
                    "onedrive" => ProviderType::OneDrive,
                    _ => self.storage_type,
                }
            }
            EditField::LocalRoot => self.local_root = value,
            EditField::OneDriveRoot => self.onedrive_root = value,
            EditField::OneDriveEndpoint => self.onedrive_endpoint = value,
            EditField::OneDriveAccessToken => self.onedrive_access_token = value,
            EditField::OneDriveRefreshToken => self.onedrive_refresh_token = value,
            EditField::OneDriveClientId => self.onedrive_client_id = value,
            EditField::OneDriveTokenExpiryBufferSecs => {
                self.onedrive_token_expiry_buffer_secs = value
            }
        }
    }

    fn cycle_storage_type(&mut self) {
        self.storage_type = match self.storage_type {
            ProviderType::Local => ProviderType::OneDrive,
            ProviderType::OneDrive => ProviderType::Local,
        };
    }

    fn field_active(&self, field: EditField) -> bool {
        match self.storage_type {
            ProviderType::Local => !matches!(
                field,
                EditField::OneDriveRoot
                    | EditField::OneDriveEndpoint
                    | EditField::OneDriveAccessToken
                    | EditField::OneDriveRefreshToken
                    | EditField::OneDriveClientId
                    | EditField::OneDriveTokenExpiryBufferSecs
            ),
            ProviderType::OneDrive => !matches!(field, EditField::LocalRoot),
        }
    }

    fn visible_fields(&self) -> Vec<EditField> {
        EDIT_FIELDS
            .iter()
            .copied()
            .filter(|field| self.field_active(*field))
            .collect()
    }

    fn apply_onedrive_auth_tokens(&mut self, tokens: TokenResponse) -> Result<()> {
        let refresh_token = tokens
            .refresh_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                Error::Validation("OneDrive auth did not return a refresh token".to_owned())
            })?;
        self.onedrive_refresh_token = refresh_token;
        Ok(())
    }

    fn to_provider_config(&self) -> Result<DriverFileConfig> {
        if self.name.trim().is_empty() {
            return Err(Error::Validation("driver.name cannot be empty".to_owned()));
        }
        if self.path.trim().is_empty() {
            return Err(Error::Validation("path cannot be empty".to_owned()));
        }

        let storage = match self.storage_type {
            ProviderType::Local => {
                if self.local_root.trim().is_empty() {
                    return Err(Error::Validation(
                        "storage.local.root cannot be empty".to_owned(),
                    ));
                }
                StorageConfig::Local {
                    root: PathBuf::from(self.local_root.trim()),
                }
            }
            ProviderType::OneDrive => {
                if self.onedrive_root.trim().is_empty() {
                    return Err(Error::Validation(
                        "storage.onedrive.root cannot be empty".to_owned(),
                    ));
                }
                if self.onedrive_endpoint.trim().is_empty() {
                    return Err(Error::Validation(
                        "storage.onedrive.endpoint cannot be empty".to_owned(),
                    ));
                }
                let token_expiry_buffer_secs = optional_u64(
                    &self.onedrive_token_expiry_buffer_secs,
                    "storage.onedrive.token_expiry_buffer_secs",
                )?;
                StorageConfig::OneDrive {
                    root: PathBuf::from(self.onedrive_root.trim()),
                    endpoint: self.onedrive_endpoint.trim().to_owned(),
                    access_token: optional_trimmed(&self.onedrive_access_token),
                    refresh_token: optional_trimmed(&self.onedrive_refresh_token),
                    client_id: optional_trimmed(&self.onedrive_client_id),
                    token_expiry_buffer_secs,
                }
            }
        };

        Ok(DriverFileConfig {
            path: PathBuf::from(self.path.trim()),
            storage,
            telemetry: Default::default(),
        })
    }
}

#[derive(Debug, Clone)]
struct EditSession {
    original_name: Option<String>,
    draft: EditDraft,
    selected_field: EditField,
    mode: EditMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditMode {
    Navigate,
    TextInput,
    StorageTypeChoice { index: usize },
}

impl EditSession {
    fn new_for_add(default_name: String) -> Self {
        Self {
            original_name: None,
            draft: EditDraft::new_empty(default_name),
            selected_field: EditField::Name,
            mode: EditMode::Navigate,
        }
    }

    fn new_for_edit(provider: &ProviderEntry) -> Self {
        Self {
            original_name: Some(provider.name.clone()),
            draft: EditDraft::from_provider(provider),
            selected_field: EditField::Name,
            mode: EditMode::Navigate,
        }
    }

    fn selected_field(&self) -> EditField {
        self.selected_field
    }

    fn ensure_selected_visible(&mut self) {
        if !self.draft.field_active(self.selected_field) {
            self.selected_field = EditField::StorageType;
        }
    }

    fn storage_choices() -> [ProviderType; 2] {
        [ProviderType::Local, ProviderType::OneDrive]
    }

    fn storage_choice_index(&self) -> usize {
        let selected = self.draft.storage_type;
        Self::storage_choices()
            .iter()
            .position(|kind| *kind == selected)
            .unwrap_or(0)
    }

    fn select_next(&mut self) {
        let visible = self.draft.visible_fields();
        let current = visible
            .iter()
            .position(|field| *field == self.selected_field)
            .unwrap_or(0);
        self.selected_field = visible[(current + 1) % visible.len()];
    }

    fn select_prev(&mut self) {
        let visible = self.draft.visible_fields();
        let current = visible
            .iter()
            .position(|field| *field == self.selected_field)
            .unwrap_or(0);
        let previous = if current == 0 {
            visible.len() - 1
        } else {
            current - 1
        };
        self.selected_field = visible[previous];
    }

    fn append_char(&mut self, c: char) {
        let field = self.selected_field();
        if matches!(field, EditField::StorageType) {
            if matches!(c, 'l' | 'L') {
                self.draft.storage_type = ProviderType::Local;
            } else if matches!(c, 'o' | 'O') {
                self.draft.storage_type = ProviderType::OneDrive;
            }
            self.ensure_selected_visible();
            return;
        }

        let mut value = self.draft.field_value(field);
        value.push(c);
        self.draft.set_field_value(field, value);
    }

    fn backspace(&mut self) {
        let field = self.selected_field();
        if matches!(field, EditField::StorageType) {
            self.draft.cycle_storage_type();
            self.ensure_selected_visible();
            return;
        }

        let mut value = self.draft.field_value(field);
        value.pop();
        self.draft.set_field_value(field, value);
    }

    fn clear_selected(&mut self) {
        let field = self.selected_field();
        if matches!(field, EditField::StorageType) {
            self.draft.cycle_storage_type();
            self.ensure_selected_visible();
        } else {
            self.draft.set_field_value(field, String::new());
        }
    }

    fn complete_selected_path(&mut self) -> Result<Option<String>> {
        let field = self.selected_field();
        if !matches!(field, EditField::Path | EditField::LocalRoot) {
            return Ok(None);
        }
        let current = self.draft.field_value(field);
        match complete_filesystem_path(&current)? {
            PathCompletion::NoMatch => Ok(Some("No matching path entries".to_owned())),
            PathCompletion::Updated {
                value,
                matches,
                exact,
            } => {
                self.draft.set_field_value(field, value.clone());
                let message = if exact {
                    format!("Completed path: {value}")
                } else {
                    format!("Expanded path prefix ({matches} matches): {value}")
                };
                Ok(Some(message))
            }
        }
    }
}

enum EditAction {
    Continue,
    Cancel,
    Quit,
    Saved(String),
    Deleted,
    Disconnected(String),
    Message(String),
}

enum PathCompletion {
    NoMatch,
    Updated {
        value: String,
        matches: usize,
        exact: bool,
    },
}

fn optional_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn optional_u64(value: &str, key: &str) -> Result<Option<u64>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed = trimmed
        .parse::<u64>()
        .map_err(|source| Error::InvalidNumber {
            key: key.to_owned(),
            value: trimmed.to_owned(),
            source,
        })?;
    Ok(Some(parsed))
}

fn expand_tilde(path: &str) -> String {
    if path == "~" {
        return dirs::home_dir()
            .map(|home| home.display().to_string())
            .unwrap_or_else(|| path.to_owned());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}/{}", home.display(), rest);
        }
    }
    path.to_owned()
}

fn longest_common_prefix(values: &[String]) -> String {
    if values.is_empty() {
        return String::new();
    }
    let mut prefix = values[0].clone();
    for value in values.iter().skip(1) {
        while !value.starts_with(&prefix) {
            if prefix.is_empty() {
                return String::new();
            }
            prefix.pop();
        }
    }
    prefix
}

fn complete_filesystem_path(input: &str) -> Result<PathCompletion> {
    let expanded = expand_tilde(input);
    let (dir_text, prefix) = if expanded.chars().last().is_some_and(std::path::is_separator) {
        (expanded.clone(), String::new())
    } else if let Some(index) = expanded.rfind(std::path::is_separator) {
        let (dir, file) = expanded.split_at(index + 1);
        (dir.to_owned(), file.to_owned())
    } else {
        (String::new(), expanded.clone())
    };
    let dir_path = if dir_text.is_empty() {
        Path::new(".")
    } else {
        Path::new(&dir_text)
    };
    let entries = fs::read_dir(dir_path).map_err(|error| {
        Error::Validation(format!(
            "cannot read directory {}: {error}",
            dir_path.display()
        ))
    })?;

    let mut candidates: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| Error::Validation(error.to_string()))?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| {
            Error::Validation(format!("non-UTF-8 path in {}", dir_path.display()))
        })?;
        if !name.starts_with(&prefix) {
            continue;
        }
        let mut candidate = format!("{dir_text}{name}");
        if entry.path().is_dir() {
            candidate.push(std::path::MAIN_SEPARATOR);
        }
        candidates.push(candidate);
    }

    if candidates.is_empty() {
        return Ok(PathCompletion::NoMatch);
    }
    candidates.sort();
    if candidates.len() == 1 {
        return Ok(PathCompletion::Updated {
            value: candidates[0].clone(),
            matches: 1,
            exact: true,
        });
    }

    let prefix = longest_common_prefix(&candidates);
    if prefix.len() > expanded.len() {
        return Ok(PathCompletion::Updated {
            value: prefix,
            matches: candidates.len(),
            exact: false,
        });
    }
    Ok(PathCompletion::NoMatch)
}

fn authenticate_onedrive_with_browser<U, F>(
    draft: &mut EditDraft,
    use_case: &U,
    mut open_browser: F,
) -> Result<String>
where
    U: AuthUseCase,
    F: FnMut(&str) -> std::result::Result<(), String>,
{
    if !matches!(draft.storage_type, ProviderType::OneDrive) {
        return Err(Error::Validation(
            "OneDrive auth is only available for storage.type=onedrive".to_owned(),
        ));
    }

    let started = use_case
        .start_onedrive_auth(optional_trimmed(&draft.onedrive_client_id))
        .map_err(Error::from)?;
    eprintln!("{}", started.message());
    if open_browser(&started.verification_uri()).is_err() {
        eprintln!("(Could not open browser; open the URL above manually.)");
    }
    eprintln!();
    eprintln!("Waiting for you to sign in...");
    let tokens = started.finish().map_err(Error::from)?;

    draft.apply_onedrive_auth_tokens(tokens)?;
    Ok("OneDrive authentication completed; refresh token populated. Press s to save.".to_owned())
}

fn authenticate_onedrive<U>(draft: &mut EditDraft, use_case: &U) -> Result<String>
where
    U: AuthUseCase,
{
    authenticate_onedrive_with_browser(draft, use_case, |uri| {
        open::that(uri).map_err(|error| error.to_string())
    })
}

fn authenticate_onedrive_in_terminal(draft: &mut EditDraft) -> Result<String> {
    suspend_terminal(|| {
        let flow = OneDriveAuthFlow;
        let app = AuthApplication::new(&flow);
        authenticate_onedrive(draft, &app)
    })
}

pub fn run() -> Result<()> {
    let cd = ConfigDir::default();
    let mut state = load_state(&cd)?;

    let mut terminal = enter_terminal()?;
    let loop_result = run_loop(&mut terminal, &cd, &mut state);
    let restore_result = leave_terminal();

    match (loop_result, restore_result) {
        (Err(loop_err), Ok(())) => Err(loop_err),
        (Ok(()), Err(restore_err)) => Err(restore_err),
        (Err(loop_err), Err(restore_err)) => Err(Error::SessionRestore {
            session: loop_err.to_string(),
            restore: restore_err.to_string(),
        }),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn run_loop(terminal: &mut DefaultTerminal, cd: &ConfigDir, state: &mut AppState) -> Result<()> {
    loop {
        terminal
            .draw(|frame| draw_ui(frame, cd, state))
            .map_err(|source| Error::Terminal {
                operation: "render UI",
                source,
            })?;

        if !event::poll(Duration::from_millis(150)).map_err(|source| Error::Terminal {
            operation: "poll terminal events",
            source,
        })? {
            continue;
        }

        let event = event::read().map_err(|source| Error::Terminal {
            operation: "read terminal event",
            source,
        })?;
        match event {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                let should_quit = match state.mode.clone() {
                    UiMode::Browse => handle_browse_key(key.code, cd, state)?,
                    UiMode::DeleteConfirm { .. } => handle_delete_confirm_key(key.code, cd, state)?,
                    UiMode::Edit(_) => {
                        let action = {
                            let UiMode::Edit(session) = &mut state.mode else {
                                unreachable!()
                            };
                            handle_edit_key(key.code, cd, session)?
                        };
                        match action {
                            EditAction::Continue => {}
                            EditAction::Cancel => {
                                state.mode = UiMode::Browse;
                                state.status = "Edit canceled".to_owned();
                            }
                            EditAction::Quit => {
                                break;
                            }
                            EditAction::Saved(name) => {
                                state.mode = UiMode::Browse;
                                refresh_state(cd, state)?;
                                state.status = format!("Saved mount '{name}'");
                            }
                            EditAction::Deleted => {
                                let name = if let UiMode::Edit(session) = &state.mode {
                                    session.draft.name.clone()
                                } else {
                                    String::new()
                                };
                                state.mode = UiMode::DeleteConfirm { name };
                            }
                            EditAction::Disconnected(name) => {
                                match crate::cli::provider_control::try_disconnect_provider(&name) {
                                    Ok(()) => {
                                        state.mode = UiMode::Browse;
                                        state.status = format!("Disconnected '{name}'");
                                    }
                                    Err(e) => {
                                        state.status = format!("Disconnect failed: {}", e);
                                    }
                                }
                            }
                            EditAction::Message(message) => {
                                state.status = message;
                            }
                        }
                        false
                    }
                };

                if should_quit {
                    break;
                }
            }
            Event::Mouse(mouse) => {
                handle_mouse_event(terminal, cd, state, mouse)?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn handle_mouse_event(
    terminal: &mut DefaultTerminal,
    cd: &ConfigDir,
    state: &mut AppState,
    mouse: MouseEvent,
) -> Result<()> {
    if matches!(state.mode, UiMode::Edit(_)) {
        return Ok(());
    }

    let size = terminal.size().map_err(|source| Error::Terminal {
        operation: "read terminal size",
        source,
    })?;
    let list_area = Rect::new(0, 0, size.width, size.height.saturating_sub(2));

    match mouse.kind {
        MouseEventKind::Moved => {
            state.is_keyboard_mode = false;
            let row = (mouse.row as usize).saturating_sub(list_area.y as usize);
            if row <= state.providers.len() {
                state.hovered = row;
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let row = (mouse.row as usize).saturating_sub(list_area.y as usize);
            if row < state.providers.len() {
                state.hovered = row;
                state.selected = row;
                let provider = state.selected_provider().cloned();
                if let Some(p) = provider {
                    let is_connected = p.is_connected();
                    let style = if is_connected {
                        RowStyle::HoveredConnected
                    } else {
                        RowStyle::HoveredDisconnected
                    };
                    let rect = Rect::new(list_area.x, mouse.row, list_area.width, 1);
                    let model = mount_row_render_model(rect.width, style, true);
                    let row_rect = Rect::new(
                        rect.x + model.left_offset,
                        rect.y,
                        model.row_rect.width,
                        rect.height,
                    );
                    match hit_test_row_action(row_rect, mouse.column, is_connected) {
                        Some(RowAction::Connect) => {
                            match connect_selected_provider_for_config(cd, state) {
                                Ok(Some(name)) => state.status = format!("Connected '{name}'"),
                                Ok(None) => state.status = "No mount selected".to_owned(),
                                Err(e) => state.status = format!("Connect failed: {e}"),
                            }
                        }
                        Some(RowAction::Disconnect) => {
                            match disconnect_selected_provider(cd, state) {
                                Ok(Some(name)) => state.status = format!("Disconnected '{name}'"),
                                Ok(None) => state.status = "No mount selected".to_owned(),
                                Err(e) => state.status = format!("Disconnect failed: {e}"),
                            }
                        }
                        Some(RowAction::Edit) | None => {
                            state.mode = UiMode::Edit(EditSession::new_for_edit(&p));
                        }
                    }
                }
            } else if row == state.providers.len() {
                let default_name = suggest_new_provider_name(state);
                state.mode = UiMode::Edit(EditSession::new_for_add(default_name));
            }
        }
        _ => {}
    }

    Ok(())
}

fn handle_browse_key(code: KeyCode, cd: &ConfigDir, state: &mut AppState) -> Result<bool> {
    match code {
        KeyCode::Char('q') => Ok(true),
        KeyCode::Esc => Ok(false),
        KeyCode::Down | KeyCode::Char('j') => {
            state.is_keyboard_mode = true;
            state.select_next();
            Ok(false)
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.is_keyboard_mode = true;
            state.select_prev();
            Ok(false)
        }
        KeyCode::Char('r') => {
            match refresh_state(cd, state) {
                Ok(()) => state.status = "Refreshed mount list".to_owned(),
                Err(e) => state.status = format!("Refresh failed: {e}"),
            }
            Ok(false)
        }
        KeyCode::Char('e') | KeyCode::Enter => {
            if state.is_add_row() {
                let default_name = suggest_new_provider_name(state);
                state.mode = UiMode::Edit(EditSession::new_for_add(default_name));
                state.status = "Adding new mount".to_owned();
            } else if let Some(provider) = state.selected_provider() {
                state.mode = UiMode::Edit(EditSession::new_for_edit(provider));
                state.status = "Editing mount".to_owned();
            } else {
                state.status = "No mount selected".to_owned();
            }
            Ok(false)
        }
        KeyCode::Char('d') => {
            match disconnect_selected_provider(cd, state) {
                Ok(Some(name)) => state.status = format!("Disconnected '{name}'"),
                Ok(None) => state.status = "No mount selected".to_owned(),
                Err(e) => state.status = format!("Disconnect failed: {e}"),
            }
            Ok(false)
        }
        KeyCode::Char('c') => {
            match connect_selected_provider_for_config(cd, state) {
                Ok(Some(name)) => state.status = format!("Connected '{name}'"),
                Ok(None) => state.status = "No mount selected".to_owned(),
                Err(e) => state.status = format!("Connect failed: {e}"),
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_edit_key(code: KeyCode, cd: &ConfigDir, session: &mut EditSession) -> Result<EditAction> {
    match session.mode {
        EditMode::Navigate => match code {
            KeyCode::Esc => Ok(EditAction::Cancel),
            KeyCode::Char('q') => Ok(EditAction::Quit),
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                session.select_prev();
                Ok(EditAction::Continue)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                session.select_next();
                Ok(EditAction::Continue)
            }
            KeyCode::Tab => {
                if let Some(message) = session.complete_selected_path()? {
                    Ok(EditAction::Message(message))
                } else {
                    session.select_next();
                    Ok(EditAction::Continue)
                }
            }
            KeyCode::Enter => {
                if matches!(session.selected_field(), EditField::StorageType) {
                    session.mode = EditMode::StorageTypeChoice {
                        index: session.storage_choice_index(),
                    };
                } else {
                    session.mode = EditMode::TextInput;
                }
                Ok(EditAction::Continue)
            }
            KeyCode::Char('c') => {
                let saved_name = save_edit_session(cd, session)?;
                Ok(EditAction::Saved(saved_name))
            }
            KeyCode::Char('x') => Ok(EditAction::Deleted),
            KeyCode::Char('d') => {
                let name = session.draft.name.clone();
                Ok(EditAction::Disconnected(name))
            }
            KeyCode::Char('l') | KeyCode::Char('L')
                if matches!(session.selected_field(), EditField::StorageType) =>
            {
                session.draft.storage_type = ProviderType::Local;
                session.ensure_selected_visible();
                Ok(EditAction::Continue)
            }
            KeyCode::Char('o') | KeyCode::Char('O')
                if matches!(session.selected_field(), EditField::StorageType) =>
            {
                session.draft.storage_type = ProviderType::OneDrive;
                session.ensure_selected_visible();
                Ok(EditAction::Continue)
            }
            _ if is_onedrive_auth_key(code) => {
                let message = authenticate_onedrive_in_terminal(&mut session.draft)?;
                Ok(EditAction::Message(message))
            }
            KeyCode::Backspace => {
                session.backspace();
                Ok(EditAction::Continue)
            }
            KeyCode::Char(c) => {
                session.append_char(c);
                Ok(EditAction::Continue)
            }
            _ => Ok(EditAction::Continue),
        },
        EditMode::TextInput => match code {
            KeyCode::Esc => {
                session.mode = EditMode::Navigate;
                Ok(EditAction::Continue)
            }
            KeyCode::Enter => {
                session.mode = EditMode::Navigate;
                Ok(EditAction::Continue)
            }
            KeyCode::Backspace => {
                session.backspace();
                Ok(EditAction::Continue)
            }
            KeyCode::Delete => {
                session.clear_selected();
                Ok(EditAction::Continue)
            }
            KeyCode::Tab => {
                if let Some(message) = session.complete_selected_path()? {
                    Ok(EditAction::Message(message))
                } else {
                    Ok(EditAction::Continue)
                }
            }
            KeyCode::Char(c) => {
                session.append_char(c);
                Ok(EditAction::Continue)
            }
            _ => Ok(EditAction::Continue),
        },
        EditMode::StorageTypeChoice { mut index } => match code {
            KeyCode::Esc => {
                session.mode = EditMode::Navigate;
                Ok(EditAction::Continue)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if index == 0 {
                    index = EditSession::storage_choices().len() - 1;
                } else {
                    index -= 1;
                }
                session.mode = EditMode::StorageTypeChoice { index };
                Ok(EditAction::Continue)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                index = (index + 1) % EditSession::storage_choices().len();
                session.mode = EditMode::StorageTypeChoice { index };
                Ok(EditAction::Continue)
            }
            KeyCode::Enter => {
                session.draft.storage_type = EditSession::storage_choices()[index];
                session.ensure_selected_visible();
                session.mode = EditMode::Navigate;
                Ok(EditAction::Continue)
            }
            _ => Ok(EditAction::Continue),
        },
    }
}

fn handle_delete_confirm_key(code: KeyCode, cd: &ConfigDir, state: &mut AppState) -> Result<bool> {
    let name = if let UiMode::DeleteConfirm { ref name } = state.mode {
        name.clone()
    } else {
        return Ok(false);
    };

    match code {
        KeyCode::Char('y') => {
            remove_provider(cd, &name)?;
            state.mode = UiMode::Browse;
            refresh_state(cd, state)?;
            state.status = format!("Deleted '{}'", name);
            Ok(false)
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            state.mode = UiMode::Browse;
            state.status = "Delete canceled".to_owned();
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn suggest_new_provider_name(state: &AppState) -> String {
    let mut index = 1;
    loop {
        let candidate = format!("provider-{index}");
        if !state
            .providers
            .iter()
            .any(|provider| provider.name == candidate)
        {
            return candidate;
        }
        index += 1;
    }
}

fn save_edit_session(cd: &ConfigDir, session: &EditSession) -> Result<String> {
    let new_name = session.draft.name.trim().to_owned();
    ensure_name_available(cd, &new_name, session.original_name.as_deref())?;
    let new_config = session.draft.to_provider_config()?;
    cd.write(&new_name, &new_config)?;
    if let Some(old_name) = &session.original_name {
        if old_name != &new_name {
            cd.remove(old_name)?;
        }
    }
    Ok(new_name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowStyle {
    Normal,
    Disconnected,
    HoveredConnected,
    HoveredDisconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowAction {
    Connect,
    Disconnect,
    Edit,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy)]
struct MountRowRenderModel {
    left_offset: u16,
    shadow_width: u16,
    row_rect: Rect,
    right_overflow: u16,
}

fn mount_row_render_model(
    available_width: u16,
    style: RowStyle,
    _show_buttons: bool,
) -> MountRowRenderModel {
    let left_offset = match style {
        RowStyle::Normal => 0,
        RowStyle::Disconnected => 1,
        RowStyle::HoveredConnected | RowStyle::HoveredDisconnected => 2,
    };
    let row_rect = bounded_row_rect(Rect::new(0, 0, available_width, 1), left_offset);
    let right_edge = row_rect.x.saturating_add(row_rect.width);
    let right_overflow = right_edge.saturating_sub(available_width);

    MountRowRenderModel {
        left_offset,
        shadow_width: left_offset,
        row_rect,
        right_overflow,
    }
}

fn bounded_row_rect(rect: Rect, left_offset: u16) -> Rect {
    Rect::new(
        rect.x,
        rect.y,
        rect.width.saturating_sub(left_offset),
        rect.height,
    )
}

fn row_button_labels(is_connected: bool) -> (&'static str, &'static str) {
    if is_connected {
        ("[ ⇐ ]", "[ ↵ ]")
    } else {
        ("[ ⇒ ]", "[ ↵ ]")
    }
}

fn row_action_areas(row_rect: Rect, is_connected: bool) -> Option<(Rect, Rect)> {
    let inner_width = row_rect.width.saturating_sub(2);
    let inner_x = row_rect.x + 1;
    let (primary_label, edit_label) = row_button_labels(is_connected);
    let total_width = primary_label.chars().count() as u16 + 1 + edit_label.chars().count() as u16;
    if inner_width < total_width {
        return None;
    }

    let start_x = inner_x + inner_width - total_width;
    let primary_rect = Rect::new(
        start_x,
        row_rect.y,
        primary_label.chars().count() as u16,
        row_rect.height,
    );
    let edit_rect = Rect::new(
        start_x + primary_rect.width + 1,
        row_rect.y,
        edit_label.chars().count() as u16,
        row_rect.height,
    );
    Some((primary_rect, edit_rect))
}

fn hit_test_row_action(row_rect: Rect, column: u16, is_connected: bool) -> Option<RowAction> {
    let (primary_rect, edit_rect) = row_action_areas(row_rect, is_connected)?;
    if column >= primary_rect.x && column < primary_rect.x + primary_rect.width {
        return Some(if is_connected {
            RowAction::Disconnect
        } else {
            RowAction::Connect
        });
    }
    if column >= edit_rect.x && column < edit_rect.x + edit_rect.width {
        return Some(RowAction::Edit);
    }
    None
}

fn truncate_cell(value: &str, width: u16) -> String {
    let width = width as usize;
    if width == 0 {
        return String::new();
    }

    let len = value.chars().count();
    if len <= width {
        return format!("{value:<width$}");
    }
    if width == 1 {
        return "…".to_owned();
    }

    let mut result: String = value.chars().take(width - 1).collect();
    result.push('…');
    result
}

fn format_mount_row_text(
    entry: &ProviderEntry,
    available_width: u16,
    show_buttons: bool,
) -> String {
    let layout = compute_mount_row_layout(available_width, true, true, show_buttons);
    let mut segments = vec![truncate_cell(&entry.name, MIN_NAME_WIDTH)];

    if layout.show_path {
        segments.push(truncate_cell(
            &entry.config.path.display().to_string(),
            layout.path_width,
        ));
    }

    if layout.show_storage_type {
        segments.push(truncate_cell(
            get_storage_type_label(&entry.config.storage),
            STORAGE_TYPE_WIDTH,
        ));
    }

    let mut line = segments.join("  ");
    if layout.show_buttons {
        if !line.is_empty() {
            line.push_str("  ");
        }
        line.push_str("[ ⇒ ] [ ↵ ]");
    }
    line
}

fn render_mount_row(
    frame: &mut Frame,
    entry: &ProviderEntry,
    rect: Rect,
    style: RowStyle,
    show_buttons: bool,
    is_keyboard_mode: bool,
) {
    let is_hovered = matches!(
        style,
        RowStyle::HoveredConnected | RowStyle::HoveredDisconnected
    );
    let is_connected = entry.is_connected();
    let model = mount_row_render_model(rect.width, style, show_buttons);

    let bg_color = if is_hovered {
        COLOR_ROW_BG_HOVERED
    } else {
        COLOR_ROW_BG_NORMAL
    };

    let status_icon = if is_connected { "●" } else { "○" };
    let status_color = if is_connected {
        COLOR_CONNECTED
    } else {
        COLOR_DISCONNECTED
    };

    if model.shadow_width > 0 {
        let shadow_rect = Rect::new(rect.x, rect.y, model.shadow_width, rect.height);
        frame.render_widget(
            Paragraph::new(" ".repeat(model.shadow_width as usize))
                .style(Style::default().bg(COLOR_ROW_3D_SHADOW)),
            shadow_rect,
        );
    }

    let row_rect = Rect::new(
        rect.x + model.left_offset,
        rect.y,
        model.row_rect.width,
        rect.height,
    );
    let row_block = Block::default()
        .bg(bg_color)
        .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT);
    frame.render_widget(row_block, row_rect);

    let keyboard_indicator = if is_keyboard_mode && is_hovered {
        "⇅"
    } else {
        " "
    };
    let content_width = row_rect.width.saturating_sub(2);
    let button_width = if show_buttons {
        row_action_areas(row_rect, is_connected)
            .map(|(primary_rect, edit_rect)| primary_rect.width + edit_rect.width + 1)
            .unwrap_or(0)
    } else {
        0
    };
    let content_padding = if show_buttons && button_width > 0 {
        2
    } else {
        0
    };
    let content = format_mount_row_text(
        entry,
        content_width.saturating_sub(button_width + content_padding),
        false,
    );
    let content = if content_width > 0 {
        let prefix = format!("{keyboard_indicator}{status_icon}  ");
        let line = format!("{prefix}{content}");
        truncate_cell(&line, content_width)
    } else {
        String::new()
    };
    let text_style = Style::default().fg(status_color);

    frame.render_widget(
        Paragraph::new(content).style(text_style),
        Rect::new(
            row_rect.x + 1,
            row_rect.y,
            row_rect.width.saturating_sub(2),
            row_rect.height,
        ),
    );

    if show_buttons {
        if let Some((primary_rect, edit_rect)) = row_action_areas(row_rect, is_connected) {
            let (primary_label, edit_label) = row_button_labels(is_connected);
            frame.render_widget(
                Paragraph::new(primary_label).style(Style::default().fg(COLOR_BUTTON)),
                primary_rect,
            );
            frame.render_widget(
                Paragraph::new(edit_label).style(Style::default().fg(COLOR_BUTTON)),
                edit_rect,
            );
        }
    }
}

fn get_storage_type_label(storage: &StorageConfig) -> &'static str {
    match storage {
        StorageConfig::Local { .. } => "local",
        StorageConfig::OneDrive { .. } => "onedrive",
    }
}

fn render_add_row(frame: &mut Frame, rect: Rect, is_hovered: bool) {
    let bg_color = if is_hovered {
        COLOR_ROW_BG_HOVERED
    } else {
        COLOR_ROW_BG_NORMAL
    };

    let displacement = if is_hovered { 2 } else { 0 };
    let shadow_width = if is_hovered { 2 } else { 0 };

    if shadow_width > 0 {
        let shadow_rect = Rect::new(
            rect.x.saturating_sub(shadow_width),
            rect.y,
            shadow_width,
            rect.height,
        );
        frame.render_widget(
            Paragraph::new(" ".repeat(shadow_width as usize))
                .style(Style::default().bg(COLOR_ROW_3D_SHADOW)),
            shadow_rect,
        );
    }

    let row_rect = Rect::new(rect.x + displacement, rect.y, rect.width, rect.height);
    let row_block = Block::default()
        .bg(bg_color)
        .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT);
    frame.render_widget(row_block, row_rect);

    let content = if is_hovered {
        "+                                                  [ ↵ Add ]"
    } else {
        "+"
    };

    frame.render_widget(
        Paragraph::new(content).style(Style::default().fg(COLOR_BUTTON)),
        Rect::new(
            row_rect.x + 1,
            row_rect.y,
            row_rect.width.saturating_sub(2),
            row_rect.height,
        ),
    );
}

fn browse_footer_text() -> &'static str {
    "j/k/↑/↓ select  c connect  d disconnect  ↵ edit  r refresh  q quit"
}

fn edit_footer_text(is_new: bool) -> String {
    let save_label = if is_new { "Create" } else { "Save" };
    format!("[ d Disc. ] [ x ] [ c {save_label} ] [ q Quit ]")
}

fn unsupported_size_message(area: Rect) -> String {
    format!(
        "Terminal size not supported. Current: {}x{}, required: {}x{}.",
        area.width, area.height, MIN_TERMINAL_WIDTH, MIN_TERMINAL_HEIGHT
    )
}

fn draw_ui(frame: &mut Frame, _cd: &ConfigDir, state: &AppState) {
    let area = frame.area();
    if !is_supported_size(area) {
        draw_unsupported_size(frame, area);
        return;
    }

    let (list_area, footer_area) = if matches!(state.mode, UiMode::Edit(_)) {
        (
            Rect::new(0, 0, area.width, area.height.saturating_sub(4)),
            Rect::new(0, area.height.saturating_sub(4), area.width, 4),
        )
    } else {
        (
            Rect::new(0, 0, area.width, area.height.saturating_sub(2)),
            Rect::new(0, area.height.saturating_sub(2), area.width, 2),
        )
    };

    match &state.mode {
        UiMode::Browse => {
            draw_main_menu(frame, list_area, state);
        }
        UiMode::Edit(_) | UiMode::DeleteConfirm { .. } => {
            draw_main_menu(frame, list_area, state);
            if let UiMode::Edit(session) = &state.mode {
                draw_edit_menu(frame, session);
            } else if let UiMode::DeleteConfirm { name } = &state.mode {
                draw_delete_dialog(frame, name);
            }
        }
    }

    draw_footer(frame, footer_area, state);
}

fn draw_unsupported_size(frame: &mut Frame, area: Rect) {
    let block = Block::default().borders(Borders::ALL);
    let inner = Rect::new(
        area.x + 1,
        area.y + area.height.saturating_div(2),
        area.width.saturating_sub(2),
        1,
    );
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(unsupported_size_message(area)), inner);
}

fn draw_main_menu(frame: &mut Frame, area: Rect, state: &AppState) {
    let row_height = 1;
    let mut y = area.y;

    for (i, entry) in state.providers.iter().enumerate() {
        let is_hovered = i == state.hovered;
        let is_connected = entry.is_connected();

        let style = if is_hovered {
            if is_connected {
                RowStyle::HoveredConnected
            } else {
                RowStyle::HoveredDisconnected
            }
        } else {
            if is_connected {
                RowStyle::Normal
            } else {
                RowStyle::Disconnected
            }
        };

        let rect = Rect::new(area.x, y, area.width, row_height);
        render_mount_row(
            frame,
            entry,
            rect,
            style,
            is_hovered,
            state.is_keyboard_mode,
        );
        y += row_height;
    }

    let add_rect = Rect::new(area.x, y, area.width, row_height);
    render_add_row(frame, add_rect, state.is_add_row());
}

fn draw_footer(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .bg(COLOR_ROW_BG_NORMAL)
        .borders(Borders::TOP);

    frame.render_widget(block, area);
    let legend = match state.mode {
        UiMode::Browse | UiMode::DeleteConfirm { .. } => browse_footer_text(),
        UiMode::Edit(_) => "",
    };
    let legend_area = Rect::new(area.x, area.y, area.width, 1);
    frame.render_widget(Paragraph::new(legend), legend_area);

    if area.height > 1 && !state.status.is_empty() {
        let status_area = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
        frame.render_widget(Paragraph::new(state.status.clone()), status_area);
    }
}

fn draw_edit_menu(frame: &mut Frame, session: &EditSession) {
    let size = frame.area();
    let edit_area = Rect::new(0, 0, size.width, size.height.saturating_sub(2));
    let button_area = Rect::new(0, size.height.saturating_sub(2), size.width, 2);

    let visible_fields = session.draft.visible_fields();
    let mut y = edit_area.y + 1;

    for field in &visible_fields {
        let is_active = *field == session.selected_field();
        let value = session.draft.field_value(*field);
        let shown = if value.is_empty() {
            "<unset>".to_owned()
        } else {
            value
        };

        let bg = if is_active {
            COLOR_ROW_BG_SELECTED
        } else {
            COLOR_ROW_BG_NORMAL
        };
        let cursor = if is_active { "█" } else { "" };
        let content = format!("  {:25}  {}{}", field.label(), shown, cursor);

        let rect = Rect::new(edit_area.x, y, edit_area.width, 1);
        let block = Block::default().bg(bg);
        frame.render_widget(block, rect);
        frame.render_widget(Paragraph::new(content), rect);
        y += 1;
    }

    let is_new = session.original_name.is_none();
    let nav_text = "[ ⇑ ] [ ⇓ ]";
    let action_text = edit_footer_text(is_new);
    let button_text = format!("{nav_text}  {action_text}");

    let block = Block::default()
        .bg(COLOR_ROW_BG_NORMAL)
        .borders(Borders::TOP);
    frame.render_widget(block, button_area);
    frame.render_widget(
        Paragraph::new(button_text)
            .style(Style::default().fg(COLOR_BUTTON))
            .alignment(ratatui::layout::Alignment::Right),
        button_area,
    );
}

fn draw_delete_dialog(frame: &mut Frame, name: &str) {
    let size = frame.area();
    let dialog_width = 50;
    let dialog_height = 5;
    let x = (size.width.saturating_sub(dialog_width)) / 2;
    let y = (size.height.saturating_sub(dialog_height)) / 2;

    let dialog_rect = Rect::new(x, y, dialog_width, dialog_height);

    let content = format!("  Delete '{}'?  [ y ]  [ n ]", name);

    let block = Block::default()
        .bg(COLOR_ROW_BG_SELECTED)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_BUTTON));

    frame.render_widget(
        Paragraph::new(content)
            .style(Style::default().fg(COLOR_BUTTON_TEXT))
            .block(block),
        dialog_rect,
    );
}

fn is_onedrive_auth_key(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Char('l') | KeyCode::Char('L') | KeyCode::Char('o') | KeyCode::Char('O')
    )
}

fn load_state(cd: &ConfigDir) -> Result<AppState> {
    let repository = TuiConfigRepository::new(cd.clone());
    let app = ConfigApplication::new(&repository);
    AppState::load(&app)
}

fn refresh_state(cd: &ConfigDir, state: &mut AppState) -> Result<()> {
    let repository = TuiConfigRepository::new(cd.clone());
    let app = ConfigApplication::new(&repository);
    state.refresh(&app)
}

fn remove_provider(cd: &ConfigDir, name: &str) -> Result<()> {
    let repository = TuiConfigRepository::new(cd.clone());
    let app = ConfigApplication::new(&repository);
    app.remove(name).map_err(Error::from)
}

fn connect_selected_provider<U>(use_case: &U, state: &AppState) -> Result<Option<String>>
where
    U: ConnectUseCase,
{
    let Some(name) = state.selected_name() else {
        return Ok(None);
    };
    let name = name.to_owned();
    run_connect(use_case, Some(name.clone()), false)?;
    Ok(Some(name))
}

fn run_connect<U>(use_case: &U, name: Option<String>, all: bool) -> Result<()>
where
    U: ConnectUseCase,
{
    if all {
        use_case.connect_all().map_err(Error::from)
    } else if let Some(name) = name {
        use_case.connect_name(&name).map_err(Error::from)
    } else {
        Ok(())
    }
}

fn connect_selected_provider_for_config(
    cd: &ConfigDir,
    state: &AppState,
) -> Result<Option<String>> {
    suspend_terminal(|| {
        let logger = TracingLogger::new();
        let repository = TuiConnectRepository::new(cd.clone());
        let control = TuiServiceControl;
        let launcher = ProcessServiceLauncher::new(logger);
        let app = ConnectApplication::new(cd.dir(), &repository, &control, &launcher);
        connect_selected_provider(&app, state)
    })
}

fn disconnect_selected_provider(_cd: &ConfigDir, state: &AppState) -> Result<Option<String>> {
    let Some(name) = state.selected_name() else {
        return Ok(None);
    };
    let name = name.to_owned();
    crate::cli::provider_control::try_disconnect_provider(&name).map_err(Error::Validation)?;
    Ok(Some(name))
}

fn ensure_name_available(cd: &ConfigDir, name: &str, current_name: Option<&str>) -> Result<()> {
    let repository = TuiConfigRepository::new(cd.clone());
    let app = ConfigApplication::new(&repository);
    let names = app.list().map_err(Error::from)?;
    if names
        .iter()
        .any(|existing| existing == name && Some(existing.as_str()) != current_name)
    {
        return Err(Error::Validation(format!(
            "provider '{name}' already exists"
        )));
    }
    Ok(())
}

fn spawn_provider_process(provider_name: &str, config_dir: &Path) -> Result<std::process::Child> {
    let current_exe = std::env::current_exe()
        .map_err(|source| crate::cli::Error::ResolveCurrentExecutable { source })?;
    Command::new(current_exe)
        .arg("connect-sync")
        .arg("--name")
        .arg(provider_name)
        .arg("--config-dir")
        .arg(config_dir)
        .spawn()
        .map_err(|source| crate::cli::Error::SpawnDriver {
            driver_name: provider_name.to_owned(),
            source,
        })
        .map_err(Error::from)
}

fn wait_until_ready<L: Logger>(
    provider_name: &str,
    child: &mut std::process::Child,
    logger: &L,
) -> Result<()> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        let child_status = child
            .try_wait()
            .map(|status| status.map(|value| value.to_string()))
            .map_err(|source| crate::cli::Error::WaitForDriver {
                driver_name: provider_name.to_owned(),
                source,
            })
            .map_err(Error::from)?;

        match next_ready_action(
            provider_name,
            crate::cli::provider_control::provider_daemon_ready(provider_name),
            child_status,
            Instant::now() >= deadline,
        )? {
            ReadyAction::Ready => {
                logger.info(format!("Provider {provider_name} is ready"));
                return Ok(());
            }
            ReadyAction::Wait => {}
        }

        thread::sleep(READY_POLL_INTERVAL);
    }
}

#[derive(Debug)]
enum ReadyAction {
    Ready,
    Wait,
}

fn next_ready_action(
    provider_name: &str,
    is_running: bool,
    child_status: Option<String>,
    deadline_expired: bool,
) -> Result<ReadyAction> {
    if is_running {
        return Ok(ReadyAction::Ready);
    }

    if let Some(status) = child_status {
        return Err(crate::cli::Error::DriverExitedBeforeReady {
            driver_name: provider_name.to_owned(),
            status,
        }
        .into());
    }

    if deadline_expired {
        return Err(crate::cli::Error::DriverDidNotBecomeReady {
            driver_name: provider_name.to_owned(),
        }
        .into());
    }

    Ok(ReadyAction::Wait)
}

fn enter_terminal() -> Result<DefaultTerminal> {
    enable_raw_mode().map_err(|source| Error::Terminal {
        operation: "enable raw mode",
        source,
    })?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, cursor::Hide, EnableMouseCapture).map_err(|source| {
        Error::Terminal {
            operation: "enter alternate screen",
            source,
        }
    })?;
    Ok(ratatui::init())
}

fn leave_terminal() -> Result<()> {
    let mut out = stdout();
    execute!(out, DisableMouseCapture, LeaveAlternateScreen, cursor::Show).map_err(|source| {
        Error::Terminal {
            operation: "leave alternate screen",
            source,
        }
    })?;
    disable_raw_mode().map_err(|source| Error::Terminal {
        operation: "disable raw mode",
        source,
    })?;
    ratatui::restore();
    Ok(())
}

fn suspend_terminal<T, F>(op: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let mut out = stdout();
    execute!(out, DisableMouseCapture, LeaveAlternateScreen, cursor::Show).map_err(|source| {
        Error::Terminal {
            operation: "suspend terminal UI",
            source,
        }
    })?;
    disable_raw_mode().map_err(|source| Error::Terminal {
        operation: "disable raw mode",
        source,
    })?;

    let result = op();

    enable_raw_mode().map_err(|source| Error::Terminal {
        operation: "re-enable raw mode",
        source,
    })?;
    execute!(out, EnterAlternateScreen, cursor::Hide, EnableMouseCapture).map_err(|source| {
        Error::Terminal {
            operation: "resume terminal UI",
            source,
        }
    })?;
    out.flush().map_err(|source| Error::Terminal {
        operation: "flush terminal output",
        source,
    })?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::auth::{
        AuthUseCase, Result as AuthApplicationResult, StartedAuthSession,
    };
    use crate::application::connect::{ConnectUseCase, Result as ConnectApplicationResult};
    use std::cell::RefCell;
    use tempfile::TempDir;

    fn tmp_config_dir() -> (TempDir, ConfigDir) {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        (tmp, cd)
    }

    #[test]
    fn optional_u64_invalid_returns_invalid_number_error() {
        let err = optional_u64("abc", "storage.onedrive.token_expiry_buffer_secs")
            .expect_err("parse should fail");

        assert!(matches!(err, crate::tui::Error::InvalidNumber { .. }));
    }

    #[test]
    fn app_state_load_wraps_config_error() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let path = tmp.path().join("not-a-directory");
        std::fs::write(&path, "oops").expect("seed file should succeed");
        let cd = ConfigDir::new(path);
        let repository = TuiConfigRepository::new(cd);
        let app = ConfigApplication::new(&repository);
        let err = AppState::load(&app).expect_err("load should fail");

        assert!(matches!(err, crate::tui::Error::Config(_)));
    }

    fn local_provider(name: &str) -> ProviderEntry {
        ProviderEntry {
            name: name.to_owned(),
            config: DriverFileConfig {
                path: PathBuf::from(format!("/mnt/{name}")),
                storage: StorageConfig::Local {
                    root: PathBuf::from(format!("/data/{name}")),
                },
                telemetry: Default::default(),
            },
        }
    }

    #[test]
    fn terminal_size_support_rejects_smaller_than_80x24() {
        assert!(!is_supported_size(Rect::new(0, 0, 79, 24)));
        assert!(!is_supported_size(Rect::new(0, 0, 80, 23)));
        assert!(is_supported_size(Rect::new(0, 0, 80, 24)));
    }

    #[test]
    fn row_layout_drops_path_before_storage_type() {
        let layout = compute_mount_row_layout(80, true, true, true);

        assert!(layout.show_name);
        assert!(layout.show_buttons);
        assert!(layout.path_width <= layout.preferred_path_width);
    }

    #[test]
    fn row_layout_can_remove_path_and_storage_type_but_keeps_name_and_buttons() {
        let layout = compute_mount_row_layout(32, true, true, true);

        assert!(layout.show_name);
        assert!(layout.show_buttons);
        assert!(!layout.show_path || !layout.show_storage_type);
    }

    #[test]
    fn render_model_uses_left_displacement_for_hovered_rows() {
        let model = mount_row_render_model(80, RowStyle::HoveredConnected, true);

        assert!(model.left_offset > 0);
        assert_eq!(model.right_overflow, 0);
    }

    #[test]
    fn row_text_truncates_path_before_name() {
        let line = format_mount_row_text(&local_provider("backup"), 40, true);

        assert!(line.contains("backup"));
    }

    #[test]
    fn hovered_row_rect_stays_within_frame_width() {
        let rect = bounded_row_rect(Rect::new(0, 0, 80, 1), 2);

        assert!(rect.x + rect.width <= 80);
    }

    #[test]
    fn browse_footer_contains_full_shortcut_legend() {
        let footer = browse_footer_text();

        assert!(footer.contains("j"));
        assert!(footer.contains("r"));
        assert!(footer.contains("q"));
    }

    #[test]
    fn unsupported_size_message_mentions_minimum() {
        let message = unsupported_size_message(Rect::new(0, 0, 72, 20));

        assert!(message.contains("80x24"));
        assert!(message.contains("72x20"));
    }

    #[test]
    fn hit_test_returns_edit_when_click_is_inside_edit_button() {
        let row_rect = Rect::new(0, 0, 30, 1);
        let (_, edit_rect) = row_action_areas(row_rect, true).expect("buttons should fit");

        let target = hit_test_row_action(row_rect, edit_rect.x, true);

        assert_eq!(target, Some(RowAction::Edit));
    }

    #[test]
    fn hit_test_returns_connect_or_disconnect_for_primary_action_button() {
        let row_rect = Rect::new(0, 0, 30, 1);
        let (primary_rect, _) = row_action_areas(row_rect, true).expect("buttons should fit");

        let target = hit_test_row_action(row_rect, primary_rect.x, true);

        assert_eq!(target, Some(RowAction::Disconnect));
    }

    #[test]
    fn edit_q_quits_tui() {
        let (_tmp, cd) = tmp_config_dir();
        let mut session = EditSession::new_for_add("provider-1".to_owned());

        let action = handle_edit_key(KeyCode::Char('q'), &cd, &mut session).expect("key failed");

        assert!(matches!(action, EditAction::Quit));
    }

    #[test]
    fn edit_footer_contains_quit_and_save_shortcuts() {
        let footer = edit_footer_text(false);

        assert!(footer.contains("q"));
        assert!(footer.contains("c Save"));
    }

    #[test]
    fn edit_field_labels_match_spec_name_field() {
        assert_eq!(EditField::Name.label(), "name");
    }

    #[derive(Default)]
    struct RecordingConnectApp {
        connected_names: RefCell<Vec<String>>,
    }

    impl ConnectUseCase for RecordingConnectApp {
        fn connect_name(&self, provider_name: &str) -> ConnectApplicationResult<()> {
            self.connected_names
                .borrow_mut()
                .push(provider_name.to_owned());
            Ok(())
        }

        fn connect_all(&self) -> ConnectApplicationResult<()> {
            Ok(())
        }
    }

    struct FakeAuthSession {
        refresh_token: String,
    }

    impl StartedAuthSession for FakeAuthSession {
        fn message(&self) -> String {
            "Open example.com".to_owned()
        }

        fn verification_uri(&self) -> String {
            "https://example.com/device".to_owned()
        }

        fn finish(self: Box<Self>) -> AuthApplicationResult<TokenResponse> {
            Ok(TokenResponse {
                access_token: "access".to_owned(),
                refresh_token: Some(self.refresh_token),
                expires_in: 3600,
            })
        }
    }

    struct FakeAuthApp {
        refresh_token: String,
    }

    impl FakeAuthApp {
        fn success(refresh_token: &str) -> Self {
            Self {
                refresh_token: refresh_token.to_owned(),
            }
        }
    }

    impl AuthUseCase for FakeAuthApp {
        fn start_onedrive_auth(
            &self,
            _client_id: Option<String>,
        ) -> AuthApplicationResult<Box<dyn StartedAuthSession>> {
            Ok(Box::new(FakeAuthSession {
                refresh_token: self.refresh_token.clone(),
            }))
        }
    }

    #[test]
    fn select_next_wraps_to_start() {
        let mut state = AppState {
            providers: vec![local_provider("a"), local_provider("b")],
            selected: 1,
            hovered: 1,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::Browse,
        };

        state.select_next();

        assert_eq!(state.hovered, 2);
        assert_eq!(state.selected, 2);
    }

    #[test]
    fn select_prev_wraps_to_end() {
        let mut state = AppState {
            providers: vec![
                local_provider("a"),
                local_provider("b"),
                local_provider("c"),
            ],
            selected: 0,
            hovered: 0,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::Browse,
        };

        state.select_prev();

        assert_eq!(state.hovered, 3);
        assert_eq!(state.selected, 3);
    }

    #[test]
    fn selected_name_none_when_empty() {
        let state = AppState {
            providers: Vec::new(),
            selected: 0,
            hovered: 0,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::Browse,
        };

        assert!(state.selected_name().is_none());
    }

    #[test]
    fn connect_selected_provider_uses_application_connect() {
        let state = AppState {
            providers: vec![local_provider("alpha")],
            selected: 0,
            hovered: 0,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::Browse,
        };
        let app = RecordingConnectApp::default();

        let connected = connect_selected_provider(&app, &state).expect("connect should work");

        assert_eq!(connected, Some("alpha".to_owned()));
        assert_eq!(
            app.connected_names.borrow().as_slice(),
            ["alpha".to_owned()]
        );
    }

    #[test]
    fn onedrive_empty_optional_values_roundtrip_to_none() {
        let draft = EditDraft {
            name: "od".to_owned(),
            path: "/mnt/od".to_owned(),
            storage_type: ProviderType::OneDrive,
            local_root: String::new(),
            onedrive_root: "/".to_owned(),
            onedrive_endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
            onedrive_access_token: String::new(),
            onedrive_refresh_token: String::new(),
            onedrive_client_id: String::new(),
            onedrive_token_expiry_buffer_secs: String::new(),
        };

        let config = draft.to_provider_config().expect("conversion failed");
        let StorageConfig::OneDrive {
            access_token,
            refresh_token,
            client_id,
            token_expiry_buffer_secs,
            ..
        } = config.storage
        else {
            panic!("expected onedrive config");
        };

        assert!(access_token.is_none());
        assert!(refresh_token.is_none());
        assert!(client_id.is_none());
        assert!(token_expiry_buffer_secs.is_none());
    }

    #[test]
    fn apply_onedrive_auth_tokens_sets_refresh_token() {
        let mut draft = EditDraft::new_empty("new-provider".to_owned());
        draft.storage_type = ProviderType::OneDrive;

        let tokens = TokenResponse {
            access_token: "at".to_owned(),
            refresh_token: Some("rt".to_owned()),
            expires_in: 3600,
        };

        draft
            .apply_onedrive_auth_tokens(tokens)
            .expect("should apply token response");

        assert_eq!(draft.onedrive_refresh_token, "rt");
    }

    #[test]
    fn authenticate_onedrive_updates_draft_from_application_response() {
        let mut draft = EditDraft::new_empty("demo".to_owned());
        draft.storage_type = ProviderType::OneDrive;
        let opened = RefCell::new(Vec::new());

        let status = authenticate_onedrive_with_browser(
            &mut draft,
            &FakeAuthApp::success("refresh"),
            |uri: &str| {
                opened.borrow_mut().push(uri.to_owned());
                Ok(())
            },
        )
        .expect("auth should work");

        assert!(status.contains("refresh token populated"));
        assert_eq!(draft.onedrive_refresh_token, "refresh".to_owned());
        assert_eq!(opened.borrow().as_slice(), ["https://example.com/device"]);
    }

    #[test]
    fn onedrive_auth_shortcuts_include_login_key() {
        assert!(is_onedrive_auth_key(KeyCode::Char('l')));
        assert!(is_onedrive_auth_key(KeyCode::Char('o')));
        assert!(!is_onedrive_auth_key(KeyCode::Char('s')));
    }

    #[test]
    fn browse_q_quits() {
        let (_tmp, cd) = tmp_config_dir();
        let mut state = AppState {
            providers: vec![local_provider("alpha")],
            selected: 0,
            hovered: 0,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::Browse,
        };

        assert!(handle_browse_key(KeyCode::Char('q'), &cd, &mut state).expect("key failed"));
    }

    #[test]
    fn browse_escape_does_not_quit() {
        let (_tmp, cd) = tmp_config_dir();
        let mut state = AppState {
            providers: vec![local_provider("alpha")],
            selected: 0,
            hovered: 0,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::Browse,
        };

        assert!(!handle_browse_key(KeyCode::Esc, &cd, &mut state).expect("key failed"));
    }

    #[test]
    fn typing_on_active_edit_field_updates_value_without_enter() {
        let (_tmp, cd) = tmp_config_dir();
        let mut session = EditSession::new_for_add("provider-1".to_owned());
        session.selected_field = EditField::Name;

        let _ = handle_edit_key(KeyCode::Char('v'), &cd, &mut session).expect("key failed");

        assert_eq!(session.draft.name, "provider-1v");
    }

    #[test]
    fn typing_o_on_storage_type_selects_onedrive() {
        let (_tmp, cd) = tmp_config_dir();
        let mut session = EditSession::new_for_add("provider-1".to_owned());
        session.selected_field = EditField::StorageType;
        assert_eq!(session.draft.storage_type, ProviderType::Local);

        let _ = handle_edit_key(KeyCode::Char('o'), &cd, &mut session).expect("key failed");

        assert_eq!(session.draft.storage_type, ProviderType::OneDrive);
    }

    #[test]
    fn path_completion_completes_single_match() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let dir = tmp.path().join("abc");
        std::fs::create_dir(&dir).expect("failed to create dir");
        let input = tmp.path().join("a").display().to_string();
        let output = complete_filesystem_path(&input).expect("completion failed");
        let expected = format!("{}{}", dir.display(), std::path::MAIN_SEPARATOR);
        match output {
            PathCompletion::Updated { value, exact, .. } => {
                assert_eq!(value, expected);
                assert!(exact);
            }
            PathCompletion::NoMatch => panic!("expected completion"),
        }
    }

    #[test]
    fn path_completion_expands_shared_prefix_for_multiple_matches() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        std::fs::create_dir(tmp.path().join("alpha")).expect("failed to create alpha");
        std::fs::create_dir(tmp.path().join("alpine")).expect("failed to create alpine");
        let input = tmp.path().join("a").display().to_string();
        let output = complete_filesystem_path(&input).expect("completion failed");
        match output {
            PathCompletion::Updated {
                value,
                exact,
                matches,
            } => {
                assert!(value.ends_with("alp"));
                assert!(!exact);
                assert_eq!(matches, 2);
            }
            PathCompletion::NoMatch => panic!("expected prefix expansion"),
        }
    }

    #[test]
    fn path_completion_returns_no_match_when_directory_has_no_matches() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        std::fs::create_dir(tmp.path().join("alpha")).expect("failed to create alpha");
        let input = tmp.path().join("z").display().to_string();
        let output = complete_filesystem_path(&input).expect("completion failed");
        assert!(matches!(output, PathCompletion::NoMatch));
    }
}
