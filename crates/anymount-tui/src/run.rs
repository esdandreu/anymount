use super::app::{App, MountConfig};

pub fn run(
    mounts: impl IntoIterator<Item = MountConfig>,
) -> color_eyre::Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    let result = App::new(mounts).run(terminal);
    ratatui::restore();
    result
}
