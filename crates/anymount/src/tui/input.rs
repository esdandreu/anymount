use super::components::app::AppComponent;
use super::{Error, Result};
use crate::config::ConfigDir;
use crossterm::event;
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;
use std::time::Duration;

pub(crate) fn run_loop(
    terminal: &mut DefaultTerminal,
    cd: &ConfigDir,
    state: &mut super::state::AppState,
) -> Result<()> {
    let mut app = AppComponent::new();

    loop {
        terminal
            .draw(|frame| app.render(frame, state))
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
        let size = terminal.size().map_err(|source| Error::Terminal {
            operation: "read terminal size",
            source,
        })?;
        let area = Rect::new(0, 0, size.width, size.height);
        let action = app.handle_event(event, area, state)?;
        let should_quit = app.apply_action(action, cd, state)?;
        if should_quit {
            break;
        }
    }

    Ok(())
}
