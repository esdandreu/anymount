use crate::auth::{OneDriveAuthorizer, TokenResponse};
use crate::cli::commands::config::ProviderType;
use crate::cli::commands::connect::{ConnectCommand, DefaultProviderConnector, StopSignalWaiter};
use crate::config::ConfigDir;
use crate::{Logger, ProviderFileConfig, StorageConfig, TracingLogger};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEvent, MouseEventKind,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use std::fs;
use std::io::{Write, stdout};
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

const COLOR_BORDER: Color = Color::Blue;
const COLOR_HIGHLIGHT: Color = Color::Yellow;
const COLOR_CONTEXT: Color = Color::Cyan;
const COLOR_STATUS: Color = Color::Green;

#[derive(Debug, Clone)]
struct ProviderEntry {
    name: String,
    config: ProviderFileConfig,
}

#[derive(Debug, Clone)]
enum UiMode {
    Browse,
    Edit(EditSession),
    ConfirmDelete,
}

#[derive(Debug, Clone)]
struct AppState {
    providers: Vec<ProviderEntry>,
    selected: usize,
    status: String,
    mode: UiMode,
}

impl AppState {
    fn load(cd: &ConfigDir) -> Result<Self, String> {
        let names = cd.list()?;
        let mut providers = Vec::with_capacity(names.len());
        for name in names {
            let config = cd.read(&name)?;
            providers.push(ProviderEntry { name, config });
        }
        Ok(Self {
            providers,
            selected: 0,
            status: "Press a/e/d/c/C/r/q".to_owned(),
            mode: UiMode::Browse,
        })
    }

    fn refresh(&mut self, cd: &ConfigDir) -> Result<(), String> {
        let selected_name = self.selected_name().map(ToOwned::to_owned);
        let refreshed = Self::load(cd)?;
        self.providers = refreshed.providers;
        self.status = refreshed.status;
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
        self.selected = (self.selected + 1) % self.providers.len();
    }

    fn select_prev(&mut self) {
        if self.providers.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.providers.len() - 1;
        } else {
            self.selected -= 1;
        }
    }
}

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
            Self::Name => "provider.name",
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

    fn description(self) -> &'static str {
        match self {
            Self::Name => "Provider filename without .toml.",
            Self::Path => "Mount path where this provider is exposed locally.",
            Self::StorageType => "Storage backend kind.",
            Self::LocalRoot => "Local directory root exposed by the provider.",
            Self::OneDriveRoot => "OneDrive path used as root (for example '/').",
            Self::OneDriveEndpoint => "Microsoft Graph API base endpoint.",
            Self::OneDriveAccessToken => "Optional short-lived access token.",
            Self::OneDriveRefreshToken => "Refresh token used to obtain new access tokens.",
            Self::OneDriveClientId => "OAuth client_id. Empty uses built-in default app.",
            Self::OneDriveTokenExpiryBufferSecs => {
                "Seconds before expiry to refresh token proactively."
            }
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

    fn lines(&self, selected_field: EditField) -> Vec<Line<'static>> {
        self.visible_fields()
            .iter()
            .map(|field| {
                let value = self.field_value(*field);
                let shown = if value.is_empty() {
                    "<unset>".to_owned()
                } else {
                    value
                };
                let prefix = if *field == selected_field { ">" } else { " " };
                let text = format!("{prefix} {:40} = {shown}", field.label());
                if *field == selected_field {
                    Line::styled(text, Style::default().fg(COLOR_HIGHLIGHT))
                } else {
                    Line::from(text)
                }
            })
            .collect()
    }

    fn apply_onedrive_auth_tokens(&mut self, tokens: TokenResponse) -> Result<(), String> {
        let refresh_token = tokens
            .refresh_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "OneDrive auth did not return a refresh token".to_owned())?;
        self.onedrive_refresh_token = refresh_token;
        Ok(())
    }

    fn to_provider_config(&self) -> Result<ProviderFileConfig, String> {
        if self.name.trim().is_empty() {
            return Err("provider.name cannot be empty".to_owned());
        }
        if self.path.trim().is_empty() {
            return Err("path cannot be empty".to_owned());
        }

        let storage = match self.storage_type {
            ProviderType::Local => {
                if self.local_root.trim().is_empty() {
                    return Err("storage.local.root cannot be empty".to_owned());
                }
                StorageConfig::Local {
                    root: PathBuf::from(self.local_root.trim()),
                }
            }
            ProviderType::OneDrive => {
                if self.onedrive_root.trim().is_empty() {
                    return Err("storage.onedrive.root cannot be empty".to_owned());
                }
                if self.onedrive_endpoint.trim().is_empty() {
                    return Err("storage.onedrive.endpoint cannot be empty".to_owned());
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

        Ok(ProviderFileConfig {
            path: PathBuf::from(self.path.trim()),
            storage,
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

    fn complete_selected_path(&mut self) -> Result<Option<String>, String> {
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

    fn context_line(&self) -> String {
        format!(
            "Field: {} - {}",
            self.selected_field().label(),
            self.selected_field().description()
        )
    }

    fn mode_line(&self) -> &'static str {
        match self.mode {
            EditMode::Navigate => "Mode: navigate (Enter edits field)",
            EditMode::TextInput => "Mode: editing text (Enter confirms)",
            EditMode::StorageTypeChoice { .. } => "Mode: choosing storage type (Enter confirms)",
        }
    }

    fn choice_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let show_choices = matches!(self.mode, EditMode::StorageTypeChoice { .. })
            || matches!(self.selected_field(), EditField::StorageType);
        if show_choices {
            let selected_index = match self.mode {
                EditMode::StorageTypeChoice { index } => index,
                _ => self.storage_choice_index(),
            };
            lines.push(Line::styled(
                "Choices:",
                Style::default()
                    .fg(COLOR_CONTEXT)
                    .add_modifier(Modifier::BOLD),
            ));
            for (choice_index, value) in Self::storage_choices().iter().enumerate() {
                let marker = if choice_index == selected_index {
                    ">"
                } else {
                    " "
                };
                let label = match value {
                    ProviderType::Local => "local",
                    ProviderType::OneDrive => "onedrive",
                };
                let line = format!("{marker} {label}");
                if choice_index == selected_index {
                    lines.push(Line::styled(line, Style::default().fg(COLOR_HIGHLIGHT)));
                } else {
                    lines.push(Line::from(line));
                }
            }
        }
        lines
    }
}

enum EditAction {
    Continue,
    Cancel,
    Saved(String),
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

fn optional_u64(value: &str, key: &str) -> Result<Option<u64>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed = trimmed
        .parse::<u64>()
        .map_err(|e| format!("{key} must be a number: {e}"))?;
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

fn complete_filesystem_path(input: &str) -> Result<PathCompletion, String> {
    let expanded = expand_tilde(input);
    let (dir_text, prefix) = if expanded.ends_with('/') {
        (expanded.clone(), String::new())
    } else if let Some((dir, file)) = expanded.rsplit_once('/') {
        (format!("{dir}/"), file.to_owned())
    } else {
        (String::new(), expanded.clone())
    };
    let dir_path = if dir_text.is_empty() {
        Path::new(".")
    } else {
        Path::new(&dir_text)
    };
    let entries = fs::read_dir(dir_path)
        .map_err(|e| format!("cannot read directory {}: {e}", dir_path.display()))?;

    let mut candidates: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| format!("non-UTF-8 path in {}", dir_path.display()))?;
        if !name.starts_with(&prefix) {
            continue;
        }
        let mut candidate = format!("{dir_text}{name}");
        if entry.path().is_dir() {
            candidate.push('/');
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

fn authenticate_onedrive(draft: &mut EditDraft) -> Result<String, String> {
    if !matches!(draft.storage_type, ProviderType::OneDrive) {
        return Err("OneDrive auth is only available for storage.type=onedrive".to_owned());
    }

    let client_id = optional_trimmed(&draft.onedrive_client_id);
    let tokens = suspend_terminal(|| {
        let authorizer = OneDriveAuthorizer::new(client_id)?;
        let started = authorizer.start_authorization()?;
        eprintln!("{}", started.message());
        if open::that(started.verification_uri()).is_err() {
            eprintln!("(Could not open browser; open the URL above manually.)");
        }
        eprintln!();
        eprintln!("Waiting for you to sign in...");
        started.wait()
    })?;

    draft.apply_onedrive_auth_tokens(tokens)?;
    Ok("OneDrive authentication completed; refresh token populated".to_owned())
}

pub fn run() -> Result<(), String> {
    let cd = ConfigDir::default();
    let mut state = AppState::load(&cd)?;

    let mut terminal = enter_terminal()?;
    let loop_result = run_loop(&mut terminal, &cd, &mut state);
    let restore_result = leave_terminal();

    match (loop_result, restore_result) {
        (Err(loop_err), Ok(())) => Err(loop_err),
        (Ok(()), Err(restore_err)) => Err(restore_err),
        (Err(loop_err), Err(restore_err)) => Err(format!(
            "tui session error: {loop_err}; terminal restore error: {restore_err}"
        )),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn run_loop(
    terminal: &mut DefaultTerminal,
    cd: &ConfigDir,
    state: &mut AppState,
) -> Result<(), String> {
    loop {
        terminal
            .draw(|frame| draw_ui(frame, cd, state))
            .map_err(|e| format!("failed to render UI: {e}"))?;

        if !event::poll(Duration::from_millis(150))
            .map_err(|e| format!("event poll failed: {e}"))?
        {
            continue;
        }

        let event = event::read().map_err(|e| format!("event read failed: {e}"))?;
        match event {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                let should_quit = match state.mode {
                    UiMode::Browse => handle_browse_key(key.code, cd, state)?,
                    UiMode::ConfirmDelete => handle_delete_confirm_key(key.code, cd, state)?,
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
                            EditAction::Saved(name) => {
                                state.mode = UiMode::Browse;
                                state.refresh(cd)?;
                                state.status = format!("Saved provider '{name}'");
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
                handle_mouse_event(terminal, state, mouse)?;
            }
            _ => {}
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct UiRects {
    providers: Rect,
    editor_fields: Option<Rect>,
    editor_context: Option<Rect>,
}

fn ui_rects(area: Rect, mode: &UiMode) -> UiRects {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
        ])
        .split(area);
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(sections[1]);
    if matches!(mode, UiMode::Edit(_)) {
        let editor = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(main[1]);
        UiRects {
            providers: main[0],
            editor_fields: Some(editor[0]),
            editor_context: Some(editor[1]),
        }
    } else {
        UiRects {
            providers: main[0],
            editor_fields: None,
            editor_context: None,
        }
    }
}

fn rect_contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn content_row(rect: Rect, y: u16) -> Option<usize> {
    if rect.height <= 2 {
        return None;
    }
    let start = rect.y.saturating_add(1);
    let end = rect.y.saturating_add(rect.height.saturating_sub(1));
    if y < start || y >= end {
        return None;
    }
    Some(usize::from(y.saturating_sub(start)))
}

fn choice_provider_type_from_context_row(
    session: &EditSession,
    row: usize,
) -> Option<ProviderType> {
    if !matches!(session.selected_field(), EditField::StorageType) {
        return None;
    }
    let choices_offset = 3usize;
    if row < choices_offset {
        return None;
    }
    let choice_row = row - choices_offset;
    let choices = EditSession::storage_choices();
    if choice_row >= choices.len() {
        return None;
    }
    Some(choices[choice_row])
}

fn handle_mouse_event(
    terminal: &mut DefaultTerminal,
    state: &mut AppState,
    mouse: MouseEvent,
) -> Result<(), String> {
    let size = terminal
        .size()
        .map_err(|e| format!("terminal size read failed: {e}"))?;
    let area = Rect::new(0, 0, size.width, size.height);
    let rects = ui_rects(area, &state.mode);

    match mouse.kind {
        MouseEventKind::ScrollDown => {
            if rect_contains(rects.providers, mouse.column, mouse.row) {
                state.select_next();
            } else if let UiMode::Edit(session) = &mut state.mode {
                if let Some(fields_rect) = rects.editor_fields {
                    if rect_contains(fields_rect, mouse.column, mouse.row) {
                        session.select_next();
                    }
                }
            }
        }
        MouseEventKind::ScrollUp => {
            if rect_contains(rects.providers, mouse.column, mouse.row) {
                state.select_prev();
            } else if let UiMode::Edit(session) = &mut state.mode {
                if let Some(fields_rect) = rects.editor_fields {
                    if rect_contains(fields_rect, mouse.column, mouse.row) {
                        session.select_prev();
                    }
                }
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if rect_contains(rects.providers, mouse.column, mouse.row) {
                if let Some(row) = content_row(rects.providers, mouse.row) {
                    if row < state.providers.len() {
                        state.selected = row;
                    }
                }
                return Ok(());
            }

            if let UiMode::Edit(session) = &mut state.mode {
                if let Some(fields_rect) = rects.editor_fields {
                    if rect_contains(fields_rect, mouse.column, mouse.row) {
                        if let Some(row) = content_row(fields_rect, mouse.row) {
                            let fields = session.draft.visible_fields();
                            if row < fields.len() {
                                session.selected_field = fields[row];
                                session.mode = EditMode::Navigate;
                            }
                        }
                        return Ok(());
                    }
                }

                if let Some(context_rect) = rects.editor_context {
                    if rect_contains(context_rect, mouse.column, mouse.row) {
                        if let Some(row) = content_row(context_rect, mouse.row) {
                            if let Some(provider_type) =
                                choice_provider_type_from_context_row(session, row)
                            {
                                session.draft.storage_type = provider_type;
                                session.ensure_selected_visible();
                                session.mode = EditMode::Navigate;
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    Ok(())
}

fn handle_browse_key(code: KeyCode, cd: &ConfigDir, state: &mut AppState) -> Result<bool, String> {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => Ok(true),
        KeyCode::Down | KeyCode::Char('j') => {
            state.select_next();
            Ok(false)
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.select_prev();
            Ok(false)
        }
        KeyCode::Char('r') => {
            match state.refresh(cd) {
                Ok(()) => state.status = "Refreshed provider list".to_owned(),
                Err(e) => state.status = format!("Refresh failed: {e}"),
            }
            Ok(false)
        }
        KeyCode::Char('a') => {
            let default_name = suggest_new_provider_name(state);
            state.mode = UiMode::Edit(EditSession::new_for_add(default_name));
            state.status =
                "Editing new provider in-place (Enter edit, s save, Esc cancel)".to_owned();
            Ok(false)
        }
        KeyCode::Char('e') | KeyCode::Enter => {
            let Some(provider) = state.selected_provider() else {
                state.status = "No provider selected".to_owned();
                return Ok(false);
            };
            state.mode = UiMode::Edit(EditSession::new_for_edit(provider));
            state.status = "Editing provider in-place (Enter edit, s save, Esc cancel)".to_owned();
            Ok(false)
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            if state.selected_provider().is_some() {
                state.mode = UiMode::ConfirmDelete;
                state.status = "Confirm delete: y to remove, n/Esc to cancel".to_owned();
            } else {
                state.status = "No provider selected".to_owned();
            }
            Ok(false)
        }
        KeyCode::Char('c') => {
            match connect_selected_provider(cd, state) {
                Ok(Some(name)) => state.status = format!("Disconnected '{name}'"),
                Ok(None) => state.status = "No provider selected".to_owned(),
                Err(e) => state.status = format!("Connect failed: {e}"),
            }
            Ok(false)
        }
        KeyCode::Char('C') => {
            match connect_all_providers(cd) {
                Ok(()) => state.status = "Disconnected all providers".to_owned(),
                Err(e) => state.status = format!("Connect-all failed: {e}"),
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_edit_key(
    code: KeyCode,
    cd: &ConfigDir,
    session: &mut EditSession,
) -> Result<EditAction, String> {
    match session.mode {
        EditMode::Navigate => match code {
            KeyCode::Esc => Ok(EditAction::Cancel),
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                session.select_prev();
                Ok(EditAction::Continue)
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                session.select_next();
                Ok(EditAction::Continue)
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
            KeyCode::Char('s') => {
                let saved_name = save_edit_session(cd, session)?;
                Ok(EditAction::Saved(saved_name))
            }
            KeyCode::Char('o') => {
                let message = authenticate_onedrive(&mut session.draft)?;
                Ok(EditAction::Message(message))
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

fn handle_delete_confirm_key(
    code: KeyCode,
    cd: &ConfigDir,
    state: &mut AppState,
) -> Result<bool, String> {
    match code {
        KeyCode::Char('y') => {
            if let Some(name) = state.selected_name().map(ToOwned::to_owned) {
                cd.remove(&name)?;
                state.mode = UiMode::Browse;
                state.refresh(cd)?;
                state.status = format!("Removed provider '{name}'");
            } else {
                state.mode = UiMode::Browse;
                state.status = "No provider selected".to_owned();
            }
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

fn save_edit_session(cd: &ConfigDir, session: &EditSession) -> Result<String, String> {
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

fn draw_ui(frame: &mut Frame, cd: &ConfigDir, state: &AppState) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
        ])
        .split(frame.area());

    let title = Paragraph::new(Line::from("Anymount Interactive"))
        .block(
            Block::default()
                .title(format!("Config directory: {}", cd.dir().display()))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_BORDER)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(title, sections[0]);

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(sections[1]);

    let items: Vec<ListItem> = state
        .providers
        .iter()
        .map(|provider| ListItem::new(provider.name.clone()))
        .collect();
    let list = List::new(items)
        .block(
            Block::default()
                .title("Providers")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_BORDER)),
        )
        .highlight_style(
            Style::default()
                .fg(COLOR_HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    let mut list_state = ListState::default();
    if !state.providers.is_empty() {
        list_state.select(Some(state.selected));
    }
    frame.render_stateful_widget(list, main[0], &mut list_state);

    match &state.mode {
        UiMode::Browse | UiMode::ConfirmDelete => {
            frame.render_widget(
                Paragraph::new(provider_details(state.selected_provider()))
                    .block(
                        Block::default()
                            .title("Details")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(COLOR_BORDER)),
                    )
                    .wrap(Wrap { trim: true }),
                main[1],
            );
        }
        UiMode::Edit(session) => {
            let editor_sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(main[1]);
            frame.render_widget(
                Paragraph::new(session.draft.lines(session.selected_field()))
                    .block(
                        Block::default()
                            .title("Editor Fields")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(COLOR_BORDER)),
                    )
                    .wrap(Wrap { trim: true }),
                editor_sections[0],
            );
            frame.render_widget(
                Paragraph::new(editor_context_lines(session))
                    .block(
                        Block::default()
                            .title("Editor Context")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(COLOR_BORDER)),
                    )
                    .wrap(Wrap { trim: true }),
                editor_sections[1],
            );
        }
    }

    let help = Paragraph::new(help_lines(state))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Actions")
                .border_style(Style::default().fg(COLOR_BORDER)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(help, sections[2]);
}

fn help_lines(state: &AppState) -> Vec<Line<'static>> {
    let mut lines = match state.mode {
        UiMode::Browse => vec![
            Line::from("Keys: j/k or arrows move, a add, e edit, d remove"),
            Line::from("      c connect selected, C connect all, r refresh, q quit"),
        ],
        UiMode::Edit(_) => vec![Line::from(
            "Edit: Enter edit/select, Tab complete path, s save, Esc close editor",
        )],
        UiMode::ConfirmDelete => vec![
            Line::from("Delete confirmation: y remove selected provider"),
            Line::from("                     n/Esc cancel"),
        ],
    };
    lines.push(Line::from(vec![
        Span::styled("Status: ", Style::default().fg(COLOR_STATUS)),
        Span::raw(state.status.clone()),
    ]));
    lines
}

fn editor_context_lines(session: &EditSession) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::styled(
            session.mode_line(),
            Style::default()
                .fg(COLOR_CONTEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(session.context_line(), Style::default().fg(COLOR_CONTEXT)),
    ];
    lines.extend(session.choice_lines());
    lines
}

fn provider_details(entry: Option<&ProviderEntry>) -> Vec<Line<'static>> {
    let Some(entry) = entry else {
        return vec![Line::from("No providers configured")];
    };

    match &entry.config.storage {
        StorageConfig::Local { root } => vec![
            Line::from(format!("Name: {}", entry.name)),
            Line::from(format!("Mount path: {}", entry.config.path.display())),
            Line::from("Storage type: local"),
            Line::from(format!("Root: {}", root.display())),
        ],
        StorageConfig::OneDrive {
            root,
            endpoint,
            access_token,
            refresh_token,
            client_id,
            token_expiry_buffer_secs,
        } => vec![
            Line::from(format!("Name: {}", entry.name)),
            Line::from(format!("Mount path: {}", entry.config.path.display())),
            Line::from("Storage type: onedrive"),
            Line::from(format!("Root: {}", root.display())),
            Line::from(format!("Endpoint: {endpoint}")),
            Line::from(format!(
                "Access token: {}",
                mask_option(access_token.as_deref())
            )),
            Line::from(format!(
                "Refresh token: {}",
                mask_option(refresh_token.as_deref())
            )),
            Line::from(format!("Client ID: {}", mask_option(client_id.as_deref()))),
            Line::from(format!(
                "Token buffer secs: {}",
                token_expiry_buffer_secs
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "none".to_owned())
            )),
        ],
    }
}

fn mask_option(value: Option<&str>) -> String {
    match value {
        Some(value) if !value.is_empty() => "configured".to_owned(),
        _ => "none".to_owned(),
    }
}

fn connect_selected_provider(cd: &ConfigDir, state: &AppState) -> Result<Option<String>, String> {
    let Some(name) = state.selected_name() else {
        return Ok(None);
    };
    let name = name.to_owned();
    suspend_terminal(|| {
        run_connect(Some(name.clone()), false, cd)?;
        Ok(Some(name))
    })
}

fn connect_all_providers(cd: &ConfigDir) -> Result<(), String> {
    suspend_terminal(|| run_connect(None, true, cd))
}

fn run_connect(name: Option<String>, all: bool, cd: &ConfigDir) -> Result<(), String> {
    let cmd = ConnectCommand {
        name,
        all,
        path: None,
        config_dir: Some(cd.dir().to_path_buf()),
        storage: None,
    };
    let logger = TracingLogger::new();
    cmd._execute(
        &DefaultProviderConnector,
        &InteractiveStopSignalWaiter,
        &logger,
    )
}

fn ensure_name_available(
    cd: &ConfigDir,
    name: &str,
    current_name: Option<&str>,
) -> Result<(), String> {
    let names = cd.list()?;
    if names
        .iter()
        .any(|existing| existing == name && Some(existing.as_str()) != current_name)
    {
        return Err(format!("provider '{name}' already exists"));
    }
    Ok(())
}

fn enter_terminal() -> Result<DefaultTerminal, String> {
    enable_raw_mode().map_err(|e| format!("failed to enable raw mode: {e}"))?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, cursor::Hide, EnableMouseCapture)
        .map_err(|e| format!("failed to enter alternate screen: {e}"))?;
    Ok(ratatui::init())
}

fn leave_terminal() -> Result<(), String> {
    let mut out = stdout();
    execute!(out, DisableMouseCapture, LeaveAlternateScreen, cursor::Show)
        .map_err(|e| format!("failed to leave alternate screen: {e}"))?;
    disable_raw_mode().map_err(|e| format!("failed to disable raw mode: {e}"))?;
    ratatui::restore();
    Ok(())
}

fn suspend_terminal<T, F>(op: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    let mut out = stdout();
    execute!(out, DisableMouseCapture, LeaveAlternateScreen, cursor::Show)
        .map_err(|e| format!("failed to suspend terminal UI: {e}"))?;
    disable_raw_mode().map_err(|e| format!("failed to disable raw mode: {e}"))?;

    let result = op();

    enable_raw_mode().map_err(|e| format!("failed to enable raw mode: {e}"))?;
    execute!(out, EnterAlternateScreen, cursor::Hide, EnableMouseCapture)
        .map_err(|e| format!("failed to resume terminal UI: {e}"))?;
    out.flush()
        .map_err(|e| format!("failed to flush terminal output: {e}"))?;
    result
}

struct InteractiveStopSignalWaiter;

impl StopSignalWaiter for InteractiveStopSignalWaiter {
    fn wait<L: Logger>(&self, logger: &L) {
        logger.info("Press Enter to disconnect and return to the UI.");
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp_config_dir() -> (TempDir, ConfigDir) {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        (tmp, cd)
    }

    fn local_provider(name: &str) -> ProviderEntry {
        ProviderEntry {
            name: name.to_owned(),
            config: ProviderFileConfig {
                path: PathBuf::from(format!("/mnt/{name}")),
                storage: StorageConfig::Local {
                    root: PathBuf::from(format!("/data/{name}")),
                },
            },
        }
    }

    #[test]
    fn select_next_wraps_to_start() {
        let mut state = AppState {
            providers: vec![local_provider("a"), local_provider("b")],
            selected: 1,
            status: String::new(),
            mode: UiMode::Browse,
        };

        state.select_next();

        assert_eq!(state.selected, 0);
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
            status: String::new(),
            mode: UiMode::Browse,
        };

        state.select_prev();

        assert_eq!(state.selected, 2);
    }

    #[test]
    fn selected_name_none_when_empty() {
        let state = AppState {
            providers: Vec::new(),
            selected: 0,
            status: String::new(),
            mode: UiMode::Browse,
        };

        assert!(state.selected_name().is_none());
    }

    #[test]
    fn mask_option_hides_secrets() {
        assert_eq!(mask_option(Some("abc")), "configured");
        assert_eq!(mask_option(None), "none");
        assert_eq!(mask_option(Some("")), "none");
    }

    #[test]
    fn edit_lines_show_only_fields_for_selected_storage_type() {
        let mut draft = EditDraft::new_empty("new-provider".to_owned());

        let local_lines = draft.lines(EditField::Name);
        let local_rendered: Vec<String> = local_lines
            .into_iter()
            .map(|line| line.to_string())
            .collect();
        assert!(
            local_rendered
                .iter()
                .any(|line| line.contains("storage.local.root"))
        );
        assert!(
            !local_rendered
                .iter()
                .any(|line| line.contains("storage.onedrive.access_token"))
        );

        draft.storage_type = ProviderType::OneDrive;
        let onedrive_lines = draft.lines(EditField::Name);
        let onedrive_rendered: Vec<String> = onedrive_lines
            .into_iter()
            .map(|line| line.to_string())
            .collect();
        assert!(
            onedrive_rendered
                .iter()
                .any(|line| line.contains("storage.onedrive.access_token")
                    && line.contains("<unset>"))
        );
        assert!(
            !onedrive_rendered
                .iter()
                .any(|line| line.contains("storage.local.root"))
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
    fn edit_text_requires_enter_before_typing() {
        let (_tmp, cd) = tmp_config_dir();
        let mut session = EditSession::new_for_add("provider-1".to_owned());
        session.selected_field = EditField::Name;

        let _ = handle_edit_key(KeyCode::Char('j'), &cd, &mut session).expect("key failed");
        assert_eq!(session.draft.name, "provider-1");
        assert_eq!(session.selected_field, EditField::Path);
        assert_eq!(session.mode, EditMode::Navigate);

        let _ = handle_edit_key(KeyCode::Enter, &cd, &mut session).expect("enter failed");
        assert_eq!(session.mode, EditMode::TextInput);
        let _ = handle_edit_key(KeyCode::Char('j'), &cd, &mut session).expect("key failed");
        assert_eq!(session.draft.path, "j");

        let _ = handle_edit_key(KeyCode::Enter, &cd, &mut session).expect("enter failed");
        assert_eq!(session.mode, EditMode::Navigate);
    }

    #[test]
    fn storage_type_enter_opens_choice_list() {
        let (_tmp, cd) = tmp_config_dir();
        let mut session = EditSession::new_for_add("provider-1".to_owned());
        session.selected_field = EditField::StorageType;
        assert_eq!(session.draft.storage_type, ProviderType::Local);

        let _ = handle_edit_key(KeyCode::Enter, &cd, &mut session).expect("enter failed");
        assert!(matches!(session.mode, EditMode::StorageTypeChoice { .. }));
        let _ = handle_edit_key(KeyCode::Down, &cd, &mut session).expect("down failed");
        let _ = handle_edit_key(KeyCode::Enter, &cd, &mut session).expect("enter failed");

        assert_eq!(session.mode, EditMode::Navigate);
        assert_eq!(session.draft.storage_type, ProviderType::OneDrive);
    }

    #[test]
    fn path_completion_completes_single_match() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let dir = tmp.path().join("abc");
        std::fs::create_dir(&dir).expect("failed to create dir");
        let input = tmp.path().join("a").display().to_string();
        let output = complete_filesystem_path(&input).expect("completion failed");
        let expected = format!("{}/", dir.display());
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
    fn choice_mapping_from_context_row_works_for_storage_type() {
        let mut session = EditSession::new_for_add("provider-1".to_owned());
        session.selected_field = EditField::StorageType;
        assert_eq!(
            choice_provider_type_from_context_row(&session, 3),
            Some(ProviderType::Local)
        );
        assert_eq!(
            choice_provider_type_from_context_row(&session, 4),
            Some(ProviderType::OneDrive)
        );
        assert_eq!(choice_provider_type_from_context_row(&session, 2), None);
    }

    #[test]
    fn content_row_skips_panel_borders() {
        let rect = Rect::new(10, 5, 30, 6);
        assert_eq!(content_row(rect, 5), None);
        assert_eq!(content_row(rect, 6), Some(0));
        assert_eq!(content_row(rect, 9), Some(3));
        assert_eq!(content_row(rect, 10), None);
    }
}
