// Copyright 2026 Dotphoton AG

use crate::drivers::Session;
use std::path::PathBuf;

#[cfg(feature = "fuse")]
pub struct FuseDriver {
    path: PathBuf,
    _session: fuser::BackgroundSession,
}

#[cfg(feature = "fuse")]
impl FuseDriver {
    pub fn new(path: PathBuf, session: fuser::BackgroundSession) -> Self {
        Self {
            path,
            _session: session,
        }
    }
}

#[cfg(feature = "fuse")]
impl Session for FuseDriver {
    fn path(&self) -> &PathBuf {
        &self.path
    }
    fn kind(&self) -> &'static str {
        "macos"
    }
}

#[cfg(feature = "fuse")]
impl std::fmt::Debug for FuseDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FuseDriver")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}
