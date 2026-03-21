// Copyright 2026 Dotphoton AG

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("cache I/O failed: {operation} {path}: {source}")]
    CacheIo {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cache range not cached: {path} [{start}..{end})")]
    CacheRangeNotCached { path: PathBuf, start: u64, end: u64 },
    #[error("unexpected EOF reading cache: {path}")]
    CacheUnexpectedEof { path: PathBuf },
    #[error("FUSE mount failed at {path}: {source}")]
    FuseMount {
        path: PathBuf,
        #[source]
        source: fuser::Errno,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
