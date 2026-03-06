use crate::cli::commands::config::ProviderType;
use crate::cli::commands::connect::{
    ConnectCommand, ConnectStorageSubcommand, DefaultProviderConnector, OneDriveStorageArgs,
    StopSignalWaiter,
};
use crate::config::ConfigDir;
use crate::{Logger, ProviderFileConfig, StorageConfig, TracingLogger};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute};
use inquire::{Confirm, Select, Text};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use std::io::{Write, stdout};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
struct ProviderEntry {
    name: String,
    config: ProviderFileConfig,
}

#[derive(Debug, Clone)]
struct AppState {
    providers: Vec<ProviderEntry>,
    selected: usize,
    status: String,
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
        })
    }

    fn refresh(&mut self, cd: &ConfigDir) -> Result<(), String> {
        let previously_selected = self.selected_name().map(ToOwned::to_owned);
        let refreshed = Self::load(cd)?;
        self.providers = refreshed.providers;
        self.status = refreshed.status;
        if let Some(name) = previously_selected {
            if let Some(pos) = self.providers.iter().position(|p| p.name == name) {
                self.selected = pos;
                return Ok(());
            }
        }
        self.selected = self.selected.min(self.providers.len().saturating_sub(1));
        Ok(())
    }

    fn selected_name(&self) -> Option<&str> {
        self.providers.get(self.selected).map(|p| p.name.as_str())
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
        if let Event::Key(key) = event {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Down | KeyCode::Char('j') => state.select_next(),
                KeyCode::Up | KeyCode::Char('k') => state.select_prev(),
                KeyCode::Char('r') => match state.refresh(cd) {
                    Ok(()) => state.status = "Refreshed provider list".to_owned(),
                    Err(e) => state.status = format!("Refresh failed: {e}"),
                },
                KeyCode::Char('a') => match add_provider(cd) {
                    Ok(name) => {
                        let _ = state.refresh(cd);
                        state.status = format!("Added provider '{name}'");
                    }
                    Err(e) => state.status = format!("Add failed: {e}"),
                },
                KeyCode::Char('e') | KeyCode::Enter => match edit_selected_provider(cd, state) {
                    Ok(Some(name)) => {
                        let _ = state.refresh(cd);
                        state.status = format!("Saved provider '{name}'");
                    }
                    Ok(None) => state.status = "No provider selected".to_owned(),
                    Err(e) => state.status = format!("Edit failed: {e}"),
                },
                KeyCode::Char('d') | KeyCode::Delete => match remove_selected_provider(cd, state) {
                    Ok(Some(name)) => {
                        let _ = state.refresh(cd);
                        state.status = format!("Removed provider '{name}'");
                    }
                    Ok(None) => state.status = "No provider selected".to_owned(),
                    Err(e) => state.status = format!("Remove failed: {e}"),
                },
                KeyCode::Char('c') => match connect_selected_provider(cd, state) {
                    Ok(Some(name)) => state.status = format!("Disconnected '{name}'"),
                    Ok(None) => state.status = "No provider selected".to_owned(),
                    Err(e) => state.status = format!("Connect failed: {e}"),
                },
                KeyCode::Char('C') => match connect_all_providers(cd) {
                    Ok(()) => state.status = "Disconnected all providers".to_owned(),
                    Err(e) => state.status = format!("Connect-all failed: {e}"),
                },
                _ => {}
            }
        }
    }
    Ok(())
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
                .borders(Borders::ALL),
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
        .map(|p| ListItem::new(p.name.clone()))
        .collect();
    let list = List::new(items)
        .block(Block::default().title("Providers").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");
    let mut list_state = ListState::default();
    if !state.providers.is_empty() {
        list_state.select(Some(state.selected));
    }
    frame.render_stateful_widget(list, main[0], &mut list_state);

    let details = provider_details(state.selected_provider());
    frame.render_widget(
        Paragraph::new(details)
            .block(Block::default().title("Details").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        main[1],
    );

    let help = Paragraph::new(vec![
        Line::from("Keys: j/k or arrows move, a add, e edit, d remove"),
        Line::from("      c connect selected, C connect all, r refresh, q quit"),
        Line::from(format!("Status: {}", state.status)),
    ])
    .block(Block::default().borders(Borders::ALL).title("Actions"))
    .wrap(Wrap { trim: true });
    frame.render_widget(help, sections[2]);
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
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "none".to_owned())
            )),
        ],
    }
}

fn mask_option(value: Option<&str>) -> String {
    match value {
        Some(v) if !v.is_empty() => "configured".to_owned(),
        _ => "none".to_owned(),
    }
}

fn add_provider(cd: &ConfigDir) -> Result<String, String> {
    suspend_terminal(|| {
        let name = prompt_name(None)?;
        ensure_name_available(cd, &name, None)?;
        let config = prompt_provider_config(None)?;
        cd.write(&name, &config)?;
        Ok(name)
    })
}

fn edit_selected_provider(cd: &ConfigDir, state: &AppState) -> Result<Option<String>, String> {
    let Some(selected) = state.selected_provider() else {
        return Ok(None);
    };
    let old_name = selected.name.clone();
    let old_config = selected.config.clone();
    suspend_terminal(|| {
        let new_name = prompt_name(Some(&old_name))?;
        ensure_name_available(cd, &new_name, Some(&old_name))?;
        let new_config = prompt_provider_config(Some(&old_config))?;
        cd.write(&new_name, &new_config)?;
        if new_name != old_name {
            cd.remove(&old_name)?;
        }
        Ok(Some(new_name))
    })
}

fn remove_selected_provider(cd: &ConfigDir, state: &AppState) -> Result<Option<String>, String> {
    let Some(name) = state.selected_name() else {
        return Ok(None);
    };
    let name = name.to_owned();
    suspend_terminal(|| {
        let yes = Confirm::new(&format!("Remove provider '{name}'?"))
            .with_default(false)
            .prompt()
            .map_err(|e| format!("failed to read confirmation: {e}"))?;
        if yes {
            cd.remove(&name)?;
            Ok(Some(name))
        } else {
            Ok(None)
        }
    })
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

fn prompt_name(default: Option<&str>) -> Result<String, String> {
    let mut prompt = Text::new("Provider name:");
    if let Some(default) = default {
        prompt = prompt.with_initial_value(default);
    } else {
        prompt = prompt.with_help_message("This becomes <name>.toml in your config directory");
    }
    prompt
        .prompt()
        .map_err(|e| format!("failed to read provider name: {e}"))
}

fn prompt_provider_config(
    current: Option<&ProviderFileConfig>,
) -> Result<ProviderFileConfig, String> {
    let path_default = current
        .map(|cfg| cfg.path.display().to_string())
        .unwrap_or_default();
    let mut path_prompt = Text::new("Mount-point path:");
    if current.is_some() {
        path_prompt = path_prompt.with_initial_value(&path_default);
    } else {
        path_prompt =
            path_prompt.with_help_message("The local path where the provider will be mounted");
    }
    let path_value = path_prompt
        .prompt()
        .map_err(|e| format!("failed to read mount path: {e}"))?;

    let storage = prompt_storage(current.map(|cfg| &cfg.storage))?;
    Ok(ProviderFileConfig {
        path: PathBuf::from(path_value),
        storage,
    })
}

fn prompt_storage(current: Option<&StorageConfig>) -> Result<StorageConfig, String> {
    let options = vec![ProviderType::Local, ProviderType::OneDrive];
    let default_index = match current {
        Some(StorageConfig::OneDrive { .. }) => 1,
        _ => 0,
    };
    let selected = Select::new("Select provider type:", options)
        .with_starting_cursor(default_index)
        .prompt()
        .map_err(|e| format!("failed to select provider type: {e}"))?;
    match selected {
        ProviderType::Local => prompt_local_storage(current),
        ProviderType::OneDrive => prompt_onedrive_storage(current),
    }
}

fn prompt_local_storage(current: Option<&StorageConfig>) -> Result<StorageConfig, String> {
    let default_root = match current {
        Some(StorageConfig::Local { root }) | Some(StorageConfig::OneDrive { root, .. }) => {
            root.display().to_string()
        }
        None => String::new(),
    };
    let mut prompt = Text::new("Root directory to expose:");
    if !default_root.is_empty() {
        prompt = prompt.with_initial_value(&default_root);
    }
    let root = prompt
        .prompt()
        .map_err(|e| format!("failed to read root directory: {e}"))?;
    Ok(StorageConfig::Local {
        root: PathBuf::from(root),
    })
}

fn prompt_onedrive_storage(current: Option<&StorageConfig>) -> Result<StorageConfig, String> {
    let defaults = match current {
        Some(StorageConfig::OneDrive {
            root,
            endpoint,
            access_token,
            refresh_token,
            client_id,
            token_expiry_buffer_secs,
        }) => (
            root.display().to_string(),
            endpoint.clone(),
            access_token.clone().unwrap_or_default(),
            refresh_token.clone().unwrap_or_default(),
            client_id.clone().unwrap_or_default(),
            token_expiry_buffer_secs.unwrap_or(60).to_string(),
        ),
        Some(StorageConfig::Local { root }) => (
            root.display().to_string(),
            "https://graph.microsoft.com/v1.0".to_owned(),
            String::new(),
            String::new(),
            String::new(),
            "60".to_owned(),
        ),
        None => (
            "/".to_owned(),
            "https://graph.microsoft.com/v1.0".to_owned(),
            String::new(),
            String::new(),
            String::new(),
            "60".to_owned(),
        ),
    };

    let root = Text::new("OneDrive path to use as root:")
        .with_initial_value(&defaults.0)
        .prompt()
        .map_err(|e| format!("failed to read OneDrive root: {e}"))?;
    let endpoint = Text::new("Graph API endpoint:")
        .with_initial_value(&defaults.1)
        .prompt()
        .map_err(|e| format!("failed to read endpoint: {e}"))?;
    let access_token = optional_with_initial("Access token (optional):", &defaults.2)?;
    let refresh_token = optional_with_initial("Refresh token (optional):", &defaults.3)?;
    let client_id = optional_with_initial("OAuth client_id (optional):", &defaults.4)?;
    let token_expiry_buffer_secs = Text::new("Token expiry buffer (seconds):")
        .with_initial_value(&defaults.5)
        .prompt()
        .map_err(|e| format!("failed to read token expiry buffer: {e}"))?
        .parse::<u64>()
        .map_err(|e| format!("invalid number: {e}"))?;

    let sub = ConnectStorageSubcommand::OneDrive(OneDriveStorageArgs {
        root: PathBuf::from(root),
        endpoint,
        access_token,
        refresh_token,
        client_id,
        token_expiry_buffer_secs,
    });
    Ok(sub.to_storage_config())
}

fn optional_with_initial(prompt_text: &str, initial: &str) -> Result<Option<String>, String> {
    let input = Text::new(prompt_text)
        .with_initial_value(initial)
        .prompt()
        .map_err(|e| format!("failed to read input: {e}"))?;
    if input.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(input))
    }
}

fn enter_terminal() -> Result<DefaultTerminal, String> {
    enable_raw_mode().map_err(|e| format!("failed to enable raw mode: {e}"))?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, cursor::Hide)
        .map_err(|e| format!("failed to enter alternate screen: {e}"))?;
    Ok(ratatui::init())
}

fn leave_terminal() -> Result<(), String> {
    let mut out = stdout();
    execute!(out, LeaveAlternateScreen, cursor::Show)
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
    execute!(out, LeaveAlternateScreen, cursor::Show)
        .map_err(|e| format!("failed to suspend terminal UI: {e}"))?;
    disable_raw_mode().map_err(|e| format!("failed to disable raw mode: {e}"))?;

    let result = op();

    enable_raw_mode().map_err(|e| format!("failed to enable raw mode: {e}"))?;
    execute!(out, EnterAlternateScreen, cursor::Hide)
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
        };
        assert!(state.selected_name().is_none());
    }

    #[test]
    fn mask_option_hides_secrets() {
        assert_eq!(mask_option(Some("abc")), "configured");
        assert_eq!(mask_option(None), "none");
        assert_eq!(mask_option(Some("")), "none");
    }
}
