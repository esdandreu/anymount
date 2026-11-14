use crate::domain::{
    ConnectStorageError, DirEntry, ReadDirError, ReadFileAtError,
    StorageConfig, StoragePath, WriteAt,
};
use futures::StreamExt;
use std::{ops::Range, sync::mpsc};

pub struct NativeStorage {
    requests: tokio::sync::mpsc::Sender<Request>,
}
enum Request {
    ReadDir(
        StoragePath,
        mpsc::Sender<
            Result<
                tokio::sync::mpsc::UnboundedSender<
                    mpsc::Sender<
                        Option<Result<Box<dyn DirEntry>, ReadDirError>>,
                    >,
                >,
                ReadDirError,
            >,
        >,
    ),
    ReadRange(
        StoragePath,
        u64,
        usize,
        mpsc::Sender<Result<Vec<u8>, ReadFileAtError>>,
    ),
}
struct NativeDir {
    requests: tokio::sync::mpsc::UnboundedSender<
        mpsc::Sender<Option<Result<Box<dyn DirEntry>, ReadDirError>>>,
    >,
    done: bool,
}

impl Iterator for NativeDir {
    type Item = Result<Box<dyn DirEntry>, ReadDirError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if tokio::runtime::Handle::try_current().is_ok() {
            self.done = true;
            return Some(Err(ReadDirError::Unavailable {
                message: "native callback cannot block an async runtime".into(),
            }));
        }
        let (reply, response) = mpsc::channel();
        if self.requests.send(reply).is_err() {
            self.done = true;
            return Some(Err(ReadDirError::Unavailable {
                message: "directory worker stopped".into(),
            }));
        }
        match response.recv() {
            Ok(Some(Ok(entry))) => Some(Ok(entry)),
            Ok(item) => {
                self.done = true;
                item
            }
            Err(error) => {
                self.done = true;
                Some(Err(ReadDirError::Unavailable {
                    message: error.to_string(),
                }))
            }
        }
    }
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
                runtime.block_on(tokio::task::LocalSet::new().run_until(async move {
                    while let Some(request) = receiver.recv().await {
                        match request {
                            Request::ReadDir(path, reply) => {
                                match storage.read_dir(path).await {
                                    Ok(mut entries) => {
                                        let (next, mut requests) = tokio::sync::mpsc::unbounded_channel::<mpsc::Sender<Option<Result<Box<dyn DirEntry>, ReadDirError>>>>();
                                        if reply.send(Ok(next)).is_ok() {
                                            tokio::task::spawn_local(async move {
                                                while let Some(reply) = requests.recv().await {
                                                    let item = entries.next().await.map(|result| result.and_then(|entry| {
                                                        if entry.size().is_none() || entry.accessed().is_none() { Err(ReadDirError::NotSupported) } else { Ok(entry) }
                                                    }));
                                                    let done = !matches!(item, Some(Ok(_)));
                                                    if reply.send(item).is_err() || done { break; }
                                                }
                                            });
                                        }
                                    }
                                    Err(error) => { let _ = reply.send(Err(error)); }
                                }
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
                                let _ = reply.send(result.and_then(|count| {
                                    if count != writer.bytes.len() as u64 {
                                        return Err(ReadFileAtError::Unknown {
                                            message: "provider reported incorrect byte count".into(),
                                        });
                                    }
                                    Ok(writer.bytes)
                                }));
                            }
                        }
                    }
                }));
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
    ) -> Result<
        Box<dyn Iterator<Item = Result<Box<dyn DirEntry>, ReadDirError>>>,
        ReadDirError,
    > {
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
        let requests =
            response
                .recv()
                .map_err(|error| ReadDirError::Unavailable {
                    message: error.to_string(),
                })??;
        Ok(Box::new(NativeDir {
            requests,
            done: false,
        }))
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
            let bytes = response.recv().map_err(|error| {
                ReadFileAtError::Unavailable {
                    message: error.to_string(),
                }
            })??;
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::StorageEntry;
    use crate::test_utils::mocks::MockStorage;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Duration;
    use std::time::SystemTime;

    #[test]
    fn preserves_disconnected_errors() {
        let storage = NativeStorage::from_factory(|| {
            let mut storage = MockStorage::new();
            storage
                .expect_read_dir()
                .returning(|_| Err(ReadDirError::Disconnected));
            storage
        })
        .expect("create worker");
        assert!(matches!(
            storage.read_dir(StoragePath::root()),
            Err(ReadDirError::Disconnected)
        ));

        let (requests, mut receiver) = tokio::sync::mpsc::channel(1);
        let worker = std::thread::spawn(move || {
            if let Some(Request::ReadRange(_, _, _, reply)) =
                receiver.blocking_recv()
            {
                let _ = reply.send(Err(ReadFileAtError::Disconnected));
            }
        });
        let storage = NativeStorage { requests };
        let mut writer = ChunkWriter {
            bytes: Vec::new(),
            start: 0,
            limit: 1,
        };
        assert!(matches!(
            storage.read_file_at(StoragePath::root(), &mut writer, 0..1),
            Err(ReadFileAtError::Disconnected)
        ));
        worker.join().expect("join worker");
    }

    #[test]
    fn preserves_range_and_writer_errors() {
        for error in [
            ReadFileAtError::RangeNotSatisfiable,
            ReadFileAtError::WriteAt(crate::domain::WriteAtError::Failed {
                offset: 0,
                message: "write failed".into(),
            }),
        ] {
            let expected = std::mem::discriminant(&error);
            let (requests, mut receiver) = tokio::sync::mpsc::channel(1);
            let worker = std::thread::spawn(move || {
                if let Some(Request::ReadRange(_, _, _, reply)) =
                    receiver.blocking_recv()
                {
                    let _ = reply.send(Err(error));
                }
            });
            let storage = NativeStorage { requests };
            let mut writer = ChunkWriter {
                bytes: Vec::new(),
                start: 0,
                limit: 1,
            };
            let result =
                storage.read_file_at(StoragePath::root(), &mut writer, 0..1);
            assert_eq!(
                std::mem::discriminant(&result.expect_err("read error")),
                expected
            );
            worker.join().expect("join worker");
        }
    }

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
                Ok(vec![Box::new(StorageEntry {
                    name: counter.get().to_string(),
                    is_directory: false,
                    size: Some(0),
                    accessed: Some(SystemTime::UNIX_EPOCH),
                })])
            });
            storage
        })
        .expect("create worker");
        let first = storage
            .read_dir(StoragePath::root())
            .expect("first request")
            .next()
            .expect("first entry")
            .expect("entry succeeds");
        let second = storage
            .read_dir(StoragePath::root())
            .expect("second request")
            .next()
            .expect("second entry")
            .expect("entry succeeds");
        assert_eq!(first.file_name(), "1");
        assert_eq!(second.file_name(), "2");
    }

    #[test]
    fn rejects_entries_with_missing_metadata() {
        for (size, accessed) in
            [(None, Some(SystemTime::UNIX_EPOCH)), (Some(1), None)]
        {
            let storage = NativeStorage::from_factory(move || {
                let mut storage = MockStorage::new();
                storage.expect_read_dir().returning(move |_| {
                    Ok(vec![Box::new(StorageEntry {
                        name: "note".into(),
                        is_directory: false,
                        size,
                        accessed,
                    })])
                });
                storage
            })
            .expect("create worker");
            assert!(matches!(
                storage
                    .read_dir(StoragePath::root())
                    .expect("open directory")
                    .next(),
                Some(Err(ReadDirError::NotSupported))
            ));
        }
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
