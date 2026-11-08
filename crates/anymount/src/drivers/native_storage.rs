use crate::domain::{
    ConnectStorageError, DirEntry, ReadDirError, ReadFileAtError,
    StorageConfig, StorageEntry, StorageError, StoragePath, WriteAt,
};
use std::{ops::Range, sync::mpsc, time::SystemTime};

pub struct NativeStorage {
    requests: tokio::sync::mpsc::Sender<Request>,
}
enum Request {
    ReadDir(
        StoragePath,
        mpsc::Sender<Result<Vec<StorageEntry>, StorageError>>,
    ),
    ReadRange(
        StoragePath,
        u64,
        usize,
        mpsc::Sender<Result<Vec<u8>, StorageError>>,
    ),
}
struct ChunkWriter {
    bytes: Vec<u8>,
    start: u64,
    limit: usize,
}
impl WriteAt for ChunkWriter {
    fn write_at(
        &mut self,
        bytes: &[u8],
        offset: u64,
    ) -> Result<(), crate::domain::WriteAtError> {
        if offset != self.start + self.bytes.len() as u64
            || bytes.len() > self.limit - self.bytes.len()
        {
            return Err(crate::domain::WriteAtError::Failed {
                offset,
                message: "provider wrote outside requested range".into(),
            });
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}
struct Entry(StorageEntry);
impl DirEntry for Entry {
    fn file_name(&self) -> String {
        self.0.name.clone()
    }
    fn is_dir(&self) -> bool {
        self.0.is_directory
    }
    fn size(&self) -> u64 {
        self.0.size.unwrap_or(0)
    }
    fn accessed(&self) -> SystemTime {
        self.0.accessed.unwrap_or(SystemTime::UNIX_EPOCH)
    }
}
impl NativeStorage {
    pub fn open(
        config: Box<dyn StorageConfig>,
    ) -> Result<Self, ConnectStorageError> {
        Self::start(move |runtime| runtime.block_on(config.connect()))
    }

    #[cfg(test)]
    pub fn from_factory<
        T: crate::domain::Storage,
        F: FnOnce() -> T + Send + 'static,
    >(
        factory: F,
    ) -> Result<Self, ConnectStorageError> {
        Self::start(move |_| Ok(Box::new(factory())))
    }

    fn start<F>(factory: F) -> Result<Self, ConnectStorageError>
    where
        F: FnOnce(
                &tokio::runtime::Runtime,
            ) -> Result<
                Box<dyn crate::domain::Storage>,
                ConnectStorageError,
            > + Send
            + 'static,
    {
        let (requests, mut receiver) = tokio::sync::mpsc::channel(16);
        let (started, ready) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("anymount-storage".to_owned())
            .spawn(move || {
                let runtime =
                    match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(runtime) => runtime,
                        Err(error) => {
                            let _ = started.send(Err(
                                ConnectStorageError::Failed {
                                    kind: "native",
                                    message: error.to_string(),
                                },
                            ));
                            return;
                        }
                    };
                let storage = match factory(&runtime) {
                    Ok(storage) => {
                        let _ = started.send(Ok(()));
                        storage
                    }
                    Err(error) => {
                        let _ = started.send(Err(error));
                        return;
                    }
                };
                runtime.block_on(async move {
                    while let Some(request) = receiver.recv().await {
                        match request {
                            Request::ReadDir(path, reply) => {
                                let _ =
                                    reply.send(storage.read_dir(path).await);
                            }
                            Request::ReadRange(path, offset, length, reply) => {
                                let mut writer = ChunkWriter { bytes: Vec::new(), start: offset, limit: length };
                                let result = storage
                                    .read_file_at(
                                        path,
                                        &mut writer,
                                        offset..offset + length as u64,
                                    )
                                    .await;
                                let _ = reply.send(
                                    result.and_then(|count| {
                                        if count != writer.bytes.len() as u64 {
                                            return Err(StorageError::Failed { message: "provider reported incorrect byte count".into() }.into());
                                        }
                                        Ok(writer.bytes)
                                    }).map_err(|error| {
                                        match error {
                                            crate::ReadFileAtError::Storage(
                                                error,
                                            ) => error,
                                            other => StorageError::Failed {
                                                message: other.to_string(),
                                            },
                                        }
                                    }),
                                );
                            }
                        }
                    }
                });
            })
            .map_err(|error| ConnectStorageError::Failed {
                kind: "native",
                message: error.to_string(),
            })?;
        ready.recv().map_err(|error| ConnectStorageError::Failed {
            kind: "native",
            message: error.to_string(),
        })??;
        Ok(Self { requests })
    }
    pub fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<Box<dyn Iterator<Item = Box<dyn DirEntry>>>, ReadDirError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(ReadDirError::Unavailable {
                message: "native callback cannot block an async runtime".into(),
            });
        }
        let (reply, response) = mpsc::channel();
        self.requests
            .blocking_send(Request::ReadDir(path, reply))
            .map_err(|error| ReadDirError::Unavailable {
                message: error.to_string(),
            })?;
        let entries = response
            .recv()
            .map_err(|error| ReadDirError::Unavailable {
                message: error.to_string(),
            })?
            .map_err(map_dir_error)?;
        if entries
            .iter()
            .any(|e| e.size.is_none() || e.accessed.is_none())
        {
            return Err(ReadDirError::NotSupported);
        }
        let entries: Vec<Box<dyn DirEntry>> = entries
            .into_iter()
            .map(|e| Box::new(Entry(e)) as Box<dyn DirEntry>)
            .collect();
        Ok(Box::new(entries.into_iter()))
    }
    pub fn read_file_at(
        &self,
        path: StoragePath,
        writer: &mut dyn WriteAt,
        range: Range<u64>,
    ) -> Result<(), ReadFileAtError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(ReadFileAtError::Unavailable {
                message: "native callback cannot block an async runtime".into(),
            });
        }
        if range.start > range.end {
            return Err(ReadFileAtError::RangeNotSatisfiable);
        }
        let mut offset = range.start;
        while offset < range.end {
            let length = (range.end - offset).min(65536) as usize;
            let (reply, response) = mpsc::channel();
            self.requests
                .blocking_send(Request::ReadRange(
                    path.clone(),
                    offset,
                    length,
                    reply,
                ))
                .map_err(|error| ReadFileAtError::Unavailable {
                    message: error.to_string(),
                })?;
            let bytes = response
                .recv()
                .map_err(|error| ReadFileAtError::Unavailable {
                    message: error.to_string(),
                })?
                .map_err(map_read_error)?;
            if bytes.len() > length {
                return Err(ReadFileAtError::Unknown {
                    message: "provider returned too much data".into(),
                });
            }
            if bytes.is_empty() {
                return Err(ReadFileAtError::RangeNotSatisfiable);
            }
            writer.write_at(&bytes, offset)?;
            offset += bytes.len() as u64;
        }
        Ok(())
    }
}
fn map_dir_error(error: StorageError) -> ReadDirError {
    match error {
        StorageError::NotSupported => ReadDirError::NotSupported,
        StorageError::NotFound => ReadDirError::NotFound,
        StorageError::NotADirectory => ReadDirError::NotADirectory,
        StorageError::PermissionDenied => ReadDirError::PermissionDenied,
        StorageError::Unavailable { message } => {
            ReadDirError::Unavailable { message }
        }
        other => ReadDirError::Unknown {
            message: other.to_string(),
        },
    }
}
fn map_read_error(error: StorageError) -> ReadFileAtError {
    match error {
        StorageError::NotSupported => ReadFileAtError::NotSupported,
        StorageError::NotFound => ReadFileAtError::NotFound,
        StorageError::NotAFile => ReadFileAtError::NotAFile,
        StorageError::PermissionDenied => ReadFileAtError::PermissionDenied,
        StorageError::Unavailable { message } => {
            ReadFileAtError::Unavailable { message }
        }
        other => ReadFileAtError::Unknown {
            message: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::mocks::MockStorage;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Duration;

    #[test]
    fn worker_drives_background_tasks_between_requests() {
        let finished = Arc::new(AtomicBool::new(false));
        let flag = finished.clone();
        let storage = NativeStorage::from_factory(move || {
            let mut storage = MockStorage::new();
            storage.expect_read_dir().returning(move |_| {
                let flag = flag.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    flag.store(true, Ordering::SeqCst);
                });
                Ok(Vec::new())
            });
            storage
        })
        .expect("create worker");
        let _ = storage
            .read_dir(StoragePath::root())
            .expect("read directory");
        std::thread::sleep(Duration::from_millis(100));
        assert!(finished.load(Ordering::SeqCst));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn callback_rejects_blocking_the_runtime() {
        let storage = tokio::task::spawn_blocking(|| {
            NativeStorage::from_factory(MockStorage::new)
                .expect("create worker")
        })
        .await
        .expect("join startup");
        assert!(matches!(
            storage.read_dir(StoragePath::root()),
            Err(ReadDirError::Unavailable { .. })
        ));
    }

    #[test]
    fn worker_owns_non_send_state() {
        let storage = NativeStorage::from_factory(|| {
            let counter = std::rc::Rc::new(std::cell::Cell::new(0));
            let mut storage = MockStorage::new();
            storage.expect_read_dir().returning_st(move |_| {
                counter.set(counter.get() + 1);
                Ok(vec![StorageEntry {
                    name: counter.get().to_string(),
                    is_directory: false,
                    size: Some(0),
                    accessed: Some(SystemTime::UNIX_EPOCH),
                }])
            });
            storage
        })
        .expect("create worker");
        let first = storage
            .read_dir(StoragePath::root())
            .expect("first request")
            .next()
            .expect("first entry");
        let second = storage
            .read_dir(StoragePath::root())
            .expect("second request")
            .next()
            .expect("second entry");
        assert_eq!(first.file_name(), "1");
        assert_eq!(second.file_name(), "2");
    }

    #[test]
    fn reports_initialization_failure() {
        let result = NativeStorage::start(|_| {
            Err(ConnectStorageError::Failed {
                kind: "test",
                message: "unavailable".into(),
            })
        });
        assert!(matches!(
            result,
            Err(ConnectStorageError::Failed { kind: "test", .. })
        ));
    }
}
