use std::path::PathBuf;

use crate::domain::{driver::DriverConfig, storage::StorageConfig};

pub struct Config {
    pub name: String,
    pub path: PathBuf,
    pub storage: Box<dyn StorageConfig>,
    pub driver: Option<Box<dyn DriverConfig>>,
}

pub trait ConfigRepository {
    type Iter<'a>: Iterator<Item = Config>
    where
        Self: 'a;

    fn list(&self) -> Self::Iter<'_>;
}
