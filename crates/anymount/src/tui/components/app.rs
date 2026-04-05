use super::super::edit::{
    EditSession, UiMode, authenticate_onedrive_in_terminal, save_edit_session,
};
use super::super::services::{
    connect_selected_provider_for_config, disconnect_selected_provider, refresh_state,
    remove_provider,
};
use super::super::state::{AppState, suggest_new_provider_name};
use super::super::theme_layout::{is_supported_size, unsupported_size_message};
use super::delete_dialog::DeleteDialogComponent;
use super::edit_form::EditFormComponent;
use super::footer::FooterComponent;
use super::provider_list::ProviderListComponent;
use super::{AppAction, Component};
use crate::config::ConfigDir;
use crate::tui::Result;
use crossterm::event::{Event, KeyEventKind, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};

pub(crate) struct AppComponent {
    provider_list: ProviderListComponent,
    edit_form: EditFormComponent,
    delete_dialog: DeleteDialogComponent,
    footer: FooterComponent,
}

impl AppComponent {
    pub(crate) fn new() -> Self {
        Self {
            provider_list: ProviderListComponent::new(),
            edit_form: EditFormComponent::new(),
            delete_dialog: DeleteDialogComponent::new(),
            footer: FooterComponent::new(),
        }
    }

    pub(crate) fn render(&mut self, frame: &mut Frame, state: &AppState) {
        let area = frame.area();
        if !is_supported_size(area) {
            self.draw_unsupported_size(frame, area);
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

        self.provider_list.render(frame, list_area, state);
        if matches!(state.mode, UiMode::Edit(_)) {
            self.edit_form.render(frame, list_area, state);
        } else if matches!(state.mode, UiMode::DeleteConfirm { .. }) {
            self.delete_dialog.render(frame, area, state);
        }
        self.footer.render(frame, footer_area, state);
    }

    pub(crate) fn handle_event(
        &mut self,
        event: Event,
        terminal_area: Rect,
        state: &mut AppState,
    ) -> Result<AppAction> {
        match event {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return Ok(AppAction::Noop);
                }

                match &state.mode {
                    UiMode::Browse => Ok(self.provider_list.handle_browse_key(key, state)),
                    UiMode::DeleteConfirm { .. } => self.delete_dialog.handle_key(key, state),
                    UiMode::Edit(_) => self.edit_form.handle_key(key, state),
                }
            }
            Event::Mouse(mouse) => self.handle_mouse(mouse, terminal_area, state),
            _ => Ok(AppAction::Noop),
        }
    }

    pub(crate) fn apply_action(
        &mut self,
        action: AppAction,
        cd: &ConfigDir,
        state: &mut AppState,
    ) -> Result<bool> {
        match action {
            AppAction::Noop => Ok(false),
            AppAction::Quit => Ok(true),
            AppAction::Refresh => {
                match refresh_state(cd, state) {
                    Ok(()) => state.status = "Refreshed mount list".to_owned(),
                    Err(e) => state.status = format!("Refresh failed: {e}"),
                }
                Ok(false)
            }
            AppAction::ConnectSelected => {
                match connect_selected_provider_for_config(cd, state) {
                    Ok(Some(name)) => state.status = format!("Connected '{name}'"),
                    Ok(None) => state.status = "No mount selected".to_owned(),
                    Err(e) => state.status = format!("Connect failed: {e}"),
                }
                Ok(false)
            }
            AppAction::DisconnectSelected => {
                match disconnect_selected_provider(cd, state) {
                    Ok(Some(name)) => state.status = format!("Disconnected '{name}'"),
                    Ok(None) => state.status = "No mount selected".to_owned(),
                    Err(e) => state.status = format!("Disconnect failed: {e}"),
                }
                Ok(false)
            }
            AppAction::OpenAdd => {
                let default_name = suggest_new_provider_name(state);
                state.mode = UiMode::Edit(EditSession::new_for_add(default_name));
                state.status = "Adding new mount".to_owned();
                Ok(false)
            }
            AppAction::OpenEditSelected => {
                if let Some(provider) = state.selected_provider() {
                    state.mode = UiMode::Edit(EditSession::new_for_edit(provider));
                    state.status = "Editing mount".to_owned();
                } else {
                    state.status = "No mount selected".to_owned();
                }
                Ok(false)
            }
            AppAction::CancelEdit => {
                state.mode = UiMode::Browse;
                state.status = "Edit canceled".to_owned();
                Ok(false)
            }
            AppAction::SaveEdit => {
                let saved_name = match &state.mode {
                    UiMode::Edit(session) => save_edit_session(cd, session)?,
                    _ => return Ok(false),
                };
                state.mode = UiMode::Browse;
                refresh_state(cd, state)?;
                state.status = format!("Saved mount '{saved_name}'");
                Ok(false)
            }
            AppAction::DeleteFromEdit => {
                let name = match &state.mode {
                    UiMode::Edit(session) => session.draft.name.clone(),
                    _ => String::new(),
                };
                state.mode = UiMode::DeleteConfirm { name };
                Ok(false)
            }
            AppAction::DisconnectByName(name) => {
                match crate::cli::provider_control::try_disconnect_provider(&name) {
                    Ok(()) => {
                        state.mode = UiMode::Browse;
                        state.status = format!("Disconnected '{name}'");
                    }
                    Err(e) => {
                        state.status = format!("Disconnect failed: {}", e);
                    }
                }
                Ok(false)
            }
            AppAction::AuthenticateOneDrive => {
                if let UiMode::Edit(session) = &mut state.mode {
                    let message = authenticate_onedrive_in_terminal(&mut session.draft)?;
                    state.status = message;
                }
                Ok(false)
            }
            AppAction::DeleteConfirmed(name) => {
                remove_provider(cd, &name)?;
                state.mode = UiMode::Browse;
                refresh_state(cd, state)?;
                state.status = format!("Deleted '{}'", name);
                Ok(false)
            }
            AppAction::DeleteCanceled => {
                state.mode = UiMode::Browse;
                state.status = "Delete canceled".to_owned();
                Ok(false)
            }
            AppAction::SetStatus(message) => {
                state.status = message;
                Ok(false)
            }
        }
    }

    fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        area: Rect,
        state: &mut AppState,
    ) -> Result<AppAction> {
        self.provider_list.handle_mouse(mouse, area, state)
    }

    fn draw_unsupported_size(&self, frame: &mut Frame, area: Rect) {
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
}
