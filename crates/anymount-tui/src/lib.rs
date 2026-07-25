//! Provides the anymount terminal user interface.

mod app;
mod event;
mod service;
#[cfg(test)]
mod test_utils;
mod tui;
mod widgets;

pub use tui::Tui;
