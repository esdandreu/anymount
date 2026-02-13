use std::{path::PathBuf, result::Result, time::SystemTime};

pub enum StorageConfiguration {}

pub trait DirEntry: Send + Sync {
    fn file_name(&self) -> String;
    fn is_dir(&self) -> bool;
    fn size(&self) -> u64;
    fn accessed(&self) -> SystemTime;
}

pub trait Storage: Send + Sync + 'static {
    type Entry: DirEntry;
    type Iter: Iterator<Item = Self::Entry>;

    fn read_dir(&self, path: PathBuf) -> Result<Self::Iter, String>;
}
