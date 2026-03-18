use std::{ops::Range, path::PathBuf, time::SystemTime};

pub trait Storage: Send + Sync + 'static {
    type Entry: DirEntry;
    type Iter: Iterator<Item = Self::Entry>;

    fn read_dir(&self, path: PathBuf) -> super::Result<Self::Iter>;
    fn read_file_at(
        &self,
        path: PathBuf,
        writer: &mut impl WriteAt,
        range: Range<u64>,
    ) -> super::Result<()>;
}

pub trait DirEntry: Send + Sync {
    fn file_name(&self) -> String;
    fn is_dir(&self) -> bool;
    fn size(&self) -> u64;
    fn accessed(&self) -> SystemTime;
}

pub trait WriteAt {
    fn write_at(&mut self, buf: &[u8], offset: u64) -> super::Result<()>;
}
