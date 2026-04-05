use super::adapters::{
    ProcessServiceLauncher, TuiConfigRepository, TuiConnectRepository, TuiServiceControl,
};
use super::state::AppState;
use super::terminal::suspend_terminal;
use super::{Error, Result};
use crate::TracingLogger;
use crate::application::config::{Application as ConfigApplication, ConfigUseCase};
use crate::application::connect::{Application as ConnectApplication, ConnectUseCase};
use crate::config::ConfigDir;

pub(crate) fn load_state(cd: &ConfigDir) -> Result<AppState> {
    let repository = TuiConfigRepository::new(cd.clone());
    let app = ConfigApplication::new(&repository);
    AppState::load(&app)
}

pub(crate) fn refresh_state(cd: &ConfigDir, state: &mut AppState) -> Result<()> {
    let repository = TuiConfigRepository::new(cd.clone());
    let app = ConfigApplication::new(&repository);
    state.refresh(&app)
}

pub(crate) fn remove_provider(cd: &ConfigDir, name: &str) -> Result<()> {
    let repository = TuiConfigRepository::new(cd.clone());
    let app = ConfigApplication::new(&repository);
    app.remove(name).map_err(Error::from)
}

pub(crate) fn connect_selected_provider<U>(use_case: &U, state: &AppState) -> Result<Option<String>>
where
    U: ConnectUseCase,
{
    let Some(name) = state.selected_name() else {
        return Ok(None);
    };
    let name = name.to_owned();
    run_connect(use_case, Some(name.clone()), false)?;
    Ok(Some(name))
}

pub(crate) fn run_connect<U>(use_case: &U, name: Option<String>, all: bool) -> Result<()>
where
    U: ConnectUseCase,
{
    if all {
        use_case.connect_all().map_err(Error::from)
    } else if let Some(name) = name {
        use_case.connect_name(&name).map_err(Error::from)
    } else {
        Ok(())
    }
}

pub(crate) fn connect_selected_provider_for_config(
    cd: &ConfigDir,
    state: &AppState,
) -> Result<Option<String>> {
    suspend_terminal(|| {
        let logger = TracingLogger::new();
        let repository = TuiConnectRepository::new(cd.clone());
        let control = TuiServiceControl;
        let launcher = ProcessServiceLauncher::new(logger);
        let app = ConnectApplication::new(cd.dir(), &repository, &control, &launcher);
        connect_selected_provider(&app, state)
    })
}

pub(crate) fn disconnect_selected_provider(
    _cd: &ConfigDir,
    state: &AppState,
) -> Result<Option<String>> {
    let Some(name) = state.selected_name() else {
        return Ok(None);
    };
    let name = name.to_owned();
    crate::cli::provider_control::try_disconnect_provider(&name).map_err(Error::Validation)?;
    Ok(Some(name))
}
