use super::Result;
use super::state::AppState;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;

pub(crate) mod app;
pub(crate) mod delete_dialog;
pub(crate) mod edit_form;
pub(crate) mod footer;
pub(crate) mod main;

#[derive(Debug, Clone)]
pub(crate) enum AppAction {
    Noop,
    Quit,
    Refresh,
    ConnectSelected,
    DisconnectSelected,
    OpenAdd,
    OpenEditSelected,
    CancelEdit,
    SaveEdit,
    DeleteFromEdit,
    DisconnectByName(String),
    AuthenticateOneDrive,
    DeleteConfirmed(String),
    DeleteCanceled,
    SetStatus(String),
}

pub(crate) trait Component {
    fn handle_key(&mut self, _key: KeyEvent, _state: &mut AppState) -> Result<AppAction> {
        Ok(AppAction::Noop)
    }

    fn handle_mouse(
        &mut self,
        _mouse: MouseEvent,
        _area: Rect,
        _state: &mut AppState,
    ) -> Result<AppAction> {
        Ok(AppAction::Noop)
    }

    fn render(&mut self, _frame: &mut Frame, _area: Rect, _state: &AppState) {}
}
