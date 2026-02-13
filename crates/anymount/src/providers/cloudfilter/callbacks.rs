use super::Storage;
use crate::storages::DirEntry;
use cloud_filter::{
    error::CResult,
    filter::{Request, SyncFilter, info, ticket},
    metadata::Metadata,
    placeholder_file::PlaceholderFile,
    utility::FileTime,
};
use std::path::PathBuf;
use std::time::SystemTime;
use tracing::info;

fn system_time_to_file_time(st: SystemTime) -> FileTime {
    st.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| FileTime::from_unix_time(d.as_secs() as i64).ok())
        .unwrap_or_else(FileTime::now)
}

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
        _ticket: ticket::FetchData,
        info: info::FetchData,
    ) -> CResult<()> {
        info!(
            "fetch_data: path={:?}, offset={}, length={}",
            request.path(),
            info.required_file_range().start,
            info.required_file_range().end - info.required_file_range().start
        );
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
}
