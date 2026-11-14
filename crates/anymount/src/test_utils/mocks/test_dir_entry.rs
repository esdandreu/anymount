use std::time::SystemTime;

use crate::domain::DirEntry;

pub struct TestDirEntry {
    pub name: String,
    pub is_directory: bool,
    pub size: Option<u64>,
    pub accessed: Option<SystemTime>,
}

impl DirEntry for TestDirEntry {
    fn file_name(&self) -> String {
        self.name.clone()
    }

    fn is_dir(&self) -> bool {
        self.is_directory
    }

    fn size(&self) -> Option<u64> {
        self.size
    }

    fn accessed(&self) -> Option<SystemTime> {
        self.accessed
    }
}
