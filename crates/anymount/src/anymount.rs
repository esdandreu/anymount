use super::ConfigDir;
use crate::domain::config_repository::ConfigRepository;

pub struct Anymount<C = ConfigDir>
where
    C: ConfigRepository,
{
    config: C,
}

impl<C> Anymount<C>
where
    C: ConfigRepository,
{
    pub fn new(config: C) -> Self {
        Self { config }
    }

    pub fn connect(&self, _name: &str) {}
}

impl Default for Anymount<ConfigDir> {
    fn default() -> Self {
        Self::new(ConfigDir::default())
    }
}
