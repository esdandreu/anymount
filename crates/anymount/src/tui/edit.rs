use super::adapters::TuiConfigRepository;
use super::model::ProviderEntry;
use super::terminal::suspend_terminal;
use super::{Error, Result};
use crate::DriverFileConfig;
use crate::application::auth::{Application as AuthApplication, AuthUseCase};
use crate::application::config::{Application as ConfigApplication, ConfigUseCase};
use crate::auth::{OneDriveAuthFlow, TokenResponse};
use crate::cli::commands::config::ProviderType;
use crate::config::ConfigDir;
use crate::domain::driver::StorageConfig;
use crossterm::event::KeyCode;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_ONEDRIVE_ENDPOINT: &str = "https://graph.microsoft.com/v1.0";

#[derive(Debug, Clone)]
pub(crate) enum UiMode {
    Browse,
    Edit(EditSession),
    DeleteConfirm { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditField {
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
    pub(crate) fn label(self) -> &'static str {
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
pub(crate) struct EditDraft {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) storage_type: ProviderType,
    pub(crate) local_root: String,
    pub(crate) onedrive_root: String,
    pub(crate) onedrive_endpoint: String,
    pub(crate) onedrive_access_token: String,
    pub(crate) onedrive_refresh_token: String,
    pub(crate) onedrive_client_id: String,
    pub(crate) onedrive_token_expiry_buffer_secs: String,
}

impl EditDraft {
    pub(crate) fn new_empty(default_name: String) -> Self {
        Self {
            name: default_name,
            path: String::new(),
            storage_type: ProviderType::Local,
            local_root: String::new(),
            onedrive_root: "/".to_owned(),
            onedrive_endpoint: DEFAULT_ONEDRIVE_ENDPOINT.to_owned(),
            onedrive_access_token: String::new(),
            onedrive_refresh_token: String::new(),
            onedrive_client_id: String::new(),
            onedrive_token_expiry_buffer_secs: String::new(),
        }
    }

    pub(crate) fn from_provider(provider: &ProviderEntry) -> Self {
        match &provider.config.storage {
            StorageConfig::Local { root } => Self {
                name: provider.name.clone(),
                path: provider.config.path.display().to_string(),
                storage_type: ProviderType::Local,
                local_root: root.display().to_string(),
                onedrive_root: root.display().to_string(),
                onedrive_endpoint: DEFAULT_ONEDRIVE_ENDPOINT.to_owned(),
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

    pub(crate) fn field_value(&self, field: EditField) -> String {
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

    pub(crate) fn set_field_value(&mut self, field: EditField, value: String) {
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

    pub(crate) fn visible_fields(&self) -> Vec<EditField> {
        EDIT_FIELDS
            .iter()
            .copied()
            .filter(|field| self.field_active(*field))
            .collect()
    }

    pub(crate) fn apply_onedrive_auth_tokens(&mut self, tokens: TokenResponse) -> Result<()> {
        let refresh_token = tokens
            .refresh_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                Error::Validation("OneDrive auth did not return a refresh token".to_owned())
            })?;
        self.onedrive_refresh_token = refresh_token;
        Ok(())
    }

    pub(crate) fn to_provider_config(&self) -> Result<DriverFileConfig> {
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
pub(crate) struct EditSession {
    pub(crate) original_name: Option<String>,
    pub(crate) draft: EditDraft,
    pub(crate) selected_field: EditField,
    pub(crate) mode: EditMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditMode {
    Navigate,
    TextInput,
    StorageTypeChoice { index: usize },
}

impl EditSession {
    pub(crate) fn new_for_add(default_name: String) -> Self {
        Self {
            original_name: None,
            draft: EditDraft::new_empty(default_name),
            selected_field: EditField::Name,
            mode: EditMode::Navigate,
        }
    }

    pub(crate) fn new_for_edit(provider: &ProviderEntry) -> Self {
        Self {
            original_name: Some(provider.name.clone()),
            draft: EditDraft::from_provider(provider),
            selected_field: EditField::Name,
            mode: EditMode::Navigate,
        }
    }

    pub(crate) fn selected_field(&self) -> EditField {
        self.selected_field
    }

    pub(crate) fn ensure_selected_visible(&mut self) {
        if !self.draft.field_active(self.selected_field) {
            self.selected_field = EditField::StorageType;
        }
    }

    pub(crate) fn storage_choices() -> [ProviderType; 2] {
        [ProviderType::Local, ProviderType::OneDrive]
    }

    pub(crate) fn storage_choice_index(&self) -> usize {
        let selected = self.draft.storage_type;
        Self::storage_choices()
            .iter()
            .position(|kind| *kind == selected)
            .unwrap_or(0)
    }

    pub(crate) fn select_next(&mut self) {
        let visible = self.draft.visible_fields();
        let current = visible
            .iter()
            .position(|field| *field == self.selected_field)
            .unwrap_or(0);
        self.selected_field = visible[(current + 1) % visible.len()];
    }

    pub(crate) fn select_prev(&mut self) {
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

    pub(crate) fn append_char(&mut self, c: char) {
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

    pub(crate) fn backspace(&mut self) {
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

    pub(crate) fn clear_selected(&mut self) {
        let field = self.selected_field();
        if matches!(field, EditField::StorageType) {
            self.draft.cycle_storage_type();
            self.ensure_selected_visible();
        } else {
            self.draft.set_field_value(field, String::new());
        }
    }

    pub(crate) fn complete_selected_path(&mut self) -> Result<Option<String>> {
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

pub(crate) enum PathCompletion {
    NoMatch,
    Updated {
        value: String,
        matches: usize,
        exact: bool,
    },
}

pub(crate) fn optional_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

pub(crate) fn optional_u64(value: &str, key: &str) -> Result<Option<u64>> {
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

pub(crate) fn complete_filesystem_path(input: &str) -> Result<PathCompletion> {
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

pub(crate) fn authenticate_onedrive_with_browser<U, F>(
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

pub(crate) fn authenticate_onedrive_in_terminal(draft: &mut EditDraft) -> Result<String> {
    suspend_terminal(|| {
        let flow = OneDriveAuthFlow;
        let app = AuthApplication::new(&flow);
        authenticate_onedrive(draft, &app)
    })
}

pub(crate) fn save_edit_session(cd: &ConfigDir, session: &EditSession) -> Result<String> {
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

pub(crate) fn ensure_name_available(
    cd: &ConfigDir,
    name: &str,
    current_name: Option<&str>,
) -> Result<()> {
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

/// Keys that trigger OneDrive browser auth in navigate mode when not selecting storage type.
/// `l`/`o` on `EditField::StorageType` are handled separately in `input::handle_edit_key`.
pub(crate) fn is_onedrive_auth_key(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Char('l') | KeyCode::Char('L') | KeyCode::Char('o') | KeyCode::Char('O')
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::auth::{
        AuthUseCase, Result as AuthApplicationResult, StartedAuthSession,
    };
    use std::cell::RefCell;

    #[test]
    fn optional_u64_invalid_returns_invalid_number_error() {
        let err = optional_u64("abc", "storage.onedrive.token_expiry_buffer_secs")
            .expect_err("parse should fail");

        assert!(matches!(err, crate::tui::Error::InvalidNumber { .. }));
    }

    #[test]
    fn edit_field_labels_match_spec_name_field() {
        assert_eq!(EditField::Name.label(), "name");
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
