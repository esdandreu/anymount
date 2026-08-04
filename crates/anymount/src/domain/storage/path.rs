//! Defines validated storage-relative paths.

use std::path::{Component, Path, PathBuf};

/// Identifies a path within a storage root.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StoragePath(PathBuf);

impl StoragePath {
    /// Returns the storage root path.
    pub fn root() -> Self {
        Self(PathBuf::new())
    }

    /// Returns this path as a [`Path`].
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Consumes this value and returns its path buffer.
    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl TryFrom<PathBuf> for StoragePath {
    type Error = InvalidStoragePath;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let is_valid = path.as_os_str().is_empty()
            || path
                .components()
                .all(|component| matches!(component, Component::Normal(_)));
        if is_valid {
            Ok(Self(path))
        } else {
            Err(InvalidStoragePath { path })
        }
    }
}

impl AsRef<Path> for StoragePath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

/// Reports a path that cannot be resolved within a storage root.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error(
    "storage paths must be relative and contain only normal components: {path:?}"
)]
pub struct InvalidStoragePath {
    path: PathBuf,
}

impl InvalidStoragePath {
    /// Returns the rejected path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_path_accepts_root_and_normal_components() {
        let root = StoragePath::try_from(PathBuf::new())
            .expect("empty storage path should identify the root");
        let nested = StoragePath::try_from(PathBuf::from("a/b"))
            .expect("normal components should be accepted");

        assert_eq!(root, StoragePath::root());
        assert_eq!(nested.as_path(), Path::new("a/b"));
    }

    #[test]
    fn storage_path_rejects_absolute_and_parent_components() {
        let absolute = StoragePath::try_from(PathBuf::from("/a"));
        let parent = StoragePath::try_from(PathBuf::from("../a"));
        let nested = StoragePath::try_from(PathBuf::from("a/../../b"));

        assert!(absolute.is_err());
        assert!(parent.is_err());
        assert!(nested.is_err());
    }
}
