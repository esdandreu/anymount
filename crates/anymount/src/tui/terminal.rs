use super::Error;
use super::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute};
use ratatui::DefaultTerminal;
use std::io::{Write, stdout};

pub(crate) fn enter_terminal() -> Result<DefaultTerminal> {
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

pub(crate) fn leave_terminal() -> Result<()> {
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

pub(crate) fn suspend_terminal<T, F>(op: F) -> Result<T>
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
