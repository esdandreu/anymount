use super::app::App;
use crate::domain::ConfigRepository;

pub fn run(config_repository: impl ConfigRepository) -> color_eyre::Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    let result = App::new(config_repository).run(terminal);
    ratatui::restore();
    result
}
