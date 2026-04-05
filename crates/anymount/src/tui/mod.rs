mod adapters;
mod components;
mod edit;
mod error;
mod input;
mod model;
mod services;
mod state;
mod terminal;
mod theme_layout;

pub use error::{Error, Result};

use crate::config::ConfigDir;

pub fn run() -> Result<()> {
    let cd = ConfigDir::default();
    let mut state = services::load_state(&cd)?;

    let mut term = terminal::enter_terminal()?;
    let loop_result = input::run_loop(&mut term, &cd, &mut state);
    let restore_result = terminal::leave_terminal();

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
