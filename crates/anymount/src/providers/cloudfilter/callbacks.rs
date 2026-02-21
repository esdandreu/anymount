use super::Storage;
use crate::{
    providers::cloudfilter::placeholders::{dehydrate_file, get_placeholder_info},
    storages::{DirEntry, WriteAt},
};
use cloud_filter::{
    error::CResult,
    filter::{Request, SyncFilter, info, ticket},
    metadata::Metadata,
    placeholder_file::PlaceholderFile,
    utility::{FileTime, WriteAt as CfWriteAt},
};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::SystemTime;
use tracing::{error, info, warn};
use windows::Win32::Storage::CloudFilters::CF_PIN_STATE_UNPINNED;

pub struct Callbacks<S: Storage> {
    path: PathBuf,
    storage: S,
    pending_dehydrate: Mutex<HashSet<PathBuf>>,
}

impl<S: Storage> Callbacks<S> {
    pub fn new(path: PathBuf, storage: S) -> Self {
        Self {
            path,
            storage,
            pending_dehydrate: Mutex::new(HashSet::new()),
        }
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
            Ok(p) if p.as_os_str().is_empty() => PathBuf::new(),
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
        let mut writer = FetchDataWriter::new(ticket, range.end);
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
            Ok(p) if p.as_os_str().is_empty() => PathBuf::new(),
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
        let path = request.path().to_path_buf();
        info!("closed: path={:?}", path);
        if self.pending_dehydrate.lock().remove(&path) {
            if let Err(e) = dehydrate_file(&path) {
                error!("dehydrate_file failed: {}", e);
            }
        }
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
                if let Err(e) = dehydrate_file(&path) {
                    warn!(
                        "dehydrate_file on state_changed failed, \
                         flagging for pending dehydration: {}",
                        e
                    );
                    self.pending_dehydrate.lock().insert(path);
                }
            }
        }
    }
}

/// Cloud Filter requires each TRANSFER_DATA buffer to be 4 KiB or the final
/// chunk ending at the logical file size; otherwise CfExecute returns
/// ERROR_CLOUD_FILE_INVALID_REQUEST (0x8007017C).
const CF_TRANSFER_CHUNK: usize = 4096;

struct FetchDataWriter {
    ticket: ticket::FetchData,
    range_end: u64,
    buffer: Vec<u8>,
    next_offset: u64,
}

impl FetchDataWriter {
    fn new(ticket: ticket::FetchData, range_end: u64) -> Self {
        Self {
            ticket,
            range_end,
            buffer: Vec::new(),
            next_offset: 0,
        }
    }

    fn flush_chunks(&mut self) -> Result<(), String> {
        while self.buffer.len() >= CF_TRANSFER_CHUNK
            && self.next_offset + CF_TRANSFER_CHUNK as u64 <= self.range_end
        {
            let offset = self.next_offset;
            let chunk: Vec<u8> = self.buffer.drain(..CF_TRANSFER_CHUNK).collect();
            CfWriteAt::write_at(&self.ticket, &chunk, offset).map_err(|e| e.to_string())?;
            self.next_offset += CF_TRANSFER_CHUNK as u64;
        }
        Ok(())
    }

    fn flush_final(&mut self) -> Result<(), String> {
        if !self.buffer.is_empty() {
            let offset = self.range_end - self.buffer.len() as u64;
            let chunk = std::mem::take(&mut self.buffer);
            CfWriteAt::write_at(&self.ticket, &chunk, offset).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

impl WriteAt for FetchDataWriter {
    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<(), String> {
        if buf.is_empty() {
            return Ok(());
        }
        if self.next_offset != offset {
            return Err(format!(
                "fetch_data: non-contiguous write at {} (expected {})",
                offset, self.next_offset
            ));
        }
        self.buffer.extend_from_slice(buf);
        self.next_offset += buf.len() as u64;
        self.flush_chunks()?;
        if self.next_offset == self.range_end {
            self.flush_final()?;
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
