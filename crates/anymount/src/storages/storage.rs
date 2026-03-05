use std::{ops::Range, path::PathBuf, result::Result, time::SystemTime};

pub trait Storage: Send + Sync + 'static {
    type Entry: DirEntry;
    type Iter: Iterator<Item = Self::Entry>;

    fn read_dir(&self, path: PathBuf) -> Result<Self::Iter, String>;
    fn read_file_at(
        &self,
        path: PathBuf,
        writer: &mut impl WriteAt,
        range: Range<u64>,
    ) -> Result<(), String>;
}

pub trait DirEntry: Send + Sync {
    fn file_name(&self) -> String;
    fn is_dir(&self) -> bool;
    fn size(&self) -> u64;
    fn accessed(&self) -> SystemTime;
}

pub trait WriteAt {
    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<(), String>;
}
