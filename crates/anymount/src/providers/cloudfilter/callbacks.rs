use super::Storage;
use crate::{
    providers::cloudfilter::placeholders::{dehydrate_file, get_placeholder_info},
    storages::{DirEntry, WriteAt},
};
use windows::Win32::Storage::CloudFilters::CF_PIN_STATE_UNPINNED;
use cloud_filter::{
    error::CResult,
    filter::{Request, SyncFilter, info, ticket},
    metadata::Metadata,
    placeholder_file::PlaceholderFile,
    utility::{FileTime, WriteAt as CfWriteAt},
};
use std::path::PathBuf;
use std::time::SystemTime;
use tracing::{error, info};

pub struct Callbacks<S: Storage> {
    path: PathBuf,
    storage: S,
}

impl<S: Storage> Callbacks<S> {
    pub fn new(path: PathBuf, storage: S) -> Self {
        Self { path, storage }
    }
}

impl<S: Storage> SyncFilter for Callbacks<S> {
    fn fetch_data(
        &self,
        request: Request,
        ticket: ticket::FetchData,
        info: info::FetchData,
    ) -> CResult<()> {
        info!(
            "fetch_data: path={:?}, range={:?}",
            request.path(),
            info.required_file_range()
        );
        let full_path = request.path().to_path_buf();
        let relative = match full_path.strip_prefix(&self.path) {
            Ok(p) if p.as_os_str().is_empty() => PathBuf::from("."),
            Ok(p) => p.to_path_buf(),
            Err(_) => {
                info!(
                    "request path {:?} is not under sync root {:?}",
                    full_path, self.path
                );
                return Err(cloud_filter::error::CloudErrorKind::Unsuccessful);
            }
        };
        let range = info.required_file_range();
        let mut writer = FetchDataWriter { ticket };
        self.storage
            .read_file_at(relative, &mut writer, range.clone())
            .map_err(|e| {
                info!("read_file_at failed: {}", e);
                cloud_filter::error::CloudErrorKind::Unsuccessful
            })?;
        CResult::Ok(())
    }

    fn fetch_placeholders(
        &self,
        request: Request,
        ticket: ticket::FetchPlaceholders,
        info: info::FetchPlaceholders,
    ) -> CResult<()> {
        info!(
            "fetch_placeholders: path={:?}, pattern={:?}",
            request.path(),
            info.pattern()
        );
        let full_path = request.path().to_path_buf();
        let relative = match full_path.strip_prefix(&self.path) {
            Ok(p) if p.as_os_str().is_empty() => PathBuf::from("."),
            Ok(p) => p.to_path_buf(),
            Err(_) => {
                info!(
                    "request path {:?} is not under sync root {:?}",
                    full_path, self.path
                );
                return Err(cloud_filter::error::CloudErrorKind::Unsuccessful);
            }
        };
        let iter = self.storage.read_dir(relative).map_err(|e| {
            info!("read_dir failed: {}", e);
            cloud_filter::error::CloudErrorKind::Unsuccessful
        })?;
        let blob: Vec<u8> = request.path().into_os_string().into_encoded_bytes();
        let mut placeholders: Vec<PlaceholderFile> = iter
            .map(|entry| {
                PlaceholderFile::new(entry.file_name())
                    .metadata(
                        (if entry.is_dir() {
                            Metadata::directory()
                        } else {
                            Metadata::file()
                        })
                        .size(entry.size())
                        .accessed(system_time_to_file_time(entry.accessed())),
                    )
                    .mark_in_sync()
                    .overwrite()
                    .blob(blob.clone())
            })
            .collect();
        if let Err(e) = ticket.pass_with_placeholder(&mut placeholders) {
            info!("Failed to pass placeholders: {:?}", e);
        }
        CResult::Ok(())
    }

    fn cancel_fetch_data(&self, request: Request, _info: info::CancelFetchData) {
        info!("cancel_fetch_data: path={:?}", request.path());
    }

    fn cancel_fetch_placeholders(&self, request: Request, _info: info::CancelFetchPlaceholders) {
        info!("cancel_fetch_placeholders: path={:?}", request.path());
    }

    fn dehydrate(
        &self,
        request: Request,
        ticket: ticket::Dehydrate,
        info: info::Dehydrate,
    ) -> CResult<()> {
        info!(
            "dehydrate: path={:?}, reason={:?}, background={}",
            request.path(),
            info.reason(),
            info.background()
        );
        ticket.pass().map_err(|e| {
            info!("dehydrate pass failed: {:?}", e);
            cloud_filter::error::CloudErrorKind::Unsuccessful
        })
    }

    fn dehydrated(&self, request: Request, _info: info::Dehydrated) {
        info!("dehydrated: path={:?}", request.path());
    }

    fn opened(&self, request: Request, _info: info::Opened) {
        info!("opened: path={:?}", request.path());
    }

    fn closed(&self, request: Request, _info: info::Closed) {
        info!("closed: path={:?}", request.path());
    }

    fn delete(
        &self,
        request: Request,
        _ticket: ticket::Delete,
        _info: info::Delete,
    ) -> CResult<()> {
        info!("delete: path={:?}", request.path());
        CResult::Ok(())
    }

    fn deleted(&self, request: Request, _info: info::Deleted) {
        info!("deleted: path={:?}", request.path());
    }

    fn rename(&self, request: Request, _ticket: ticket::Rename, info: info::Rename) -> CResult<()> {
        info!(
            "rename: from={:?} to={:?}",
            request.path(),
            info.target_path()
        );
        CResult::Ok(())
    }

    fn renamed(&self, request: Request, info: info::Renamed) {
        info!(
            "renamed: from={:?} to={:?}",
            info.source_path(),
            request.path()
        );
    }

    fn state_changed(&self, changes: Vec<PathBuf>) {
        for path in changes {
            info!("state_changed: path={:?}", path);
            let file_info = match get_placeholder_info(&path) {
                Ok(info) => info,
                Err(e) => {
                    error!("get_placeholder_info failed: {}", e);
                    continue;
                }
            };
            info!("file_info: {:?}", file_info);
            if file_info.pin_state == CF_PIN_STATE_UNPINNED && file_info.on_disk_size > 0 {
                info!("dehydrating file: {:?}", path);
                if let Err(e) = dehydrate_file(&path) {
                    error!("dehydrate_file failed: {}", e);
                    continue;
                }
            }
        }
    }
}

struct FetchDataWriter {
    ticket: ticket::FetchData,
}

impl WriteAt for FetchDataWriter {
    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<(), String> {
        const CHUNK: usize = 4096;
        let mut pos = 0;
        while pos < buf.len() {
            let end = (pos + CHUNK).min(buf.len());
            let chunk = &buf[pos..end];
            CfWriteAt::write_at(&self.ticket, chunk, offset + pos as u64)
                .map_err(|e| e.to_string())?;
            pos = end;
        }
        Ok(())
    }
}

fn system_time_to_file_time(st: SystemTime) -> FileTime {
    st.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| FileTime::from_unix_time(d.as_secs() as i64).ok())
        .unwrap_or_else(FileTime::now)
}
