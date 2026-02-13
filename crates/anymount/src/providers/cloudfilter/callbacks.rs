use super::Storage;
use cloud_filter::{
    error::CResult,
    filter::{Request, SyncFilter, info, ticket},
    metadata::Metadata,
    placeholder_file::PlaceholderFile,
    utility::FileTime,
};
use std::path::PathBuf;
use tracing::info;

pub struct Callbacks<S: Storage> {
    #[allow(dead_code)]
    path: PathBuf,
    #[allow(dead_code)]
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
        // Create a single directory placeholder
        let now = FileTime::now();

        let dir = PlaceholderFile::new("folder")
            .metadata(Metadata::directory().size(0).accessed(now))
            .mark_in_sync()
            .overwrite()
            .blob(request.path().into_os_string().into_encoded_bytes());

        let file = PlaceholderFile::new("file.txt")
            .metadata(Metadata::file().size(0).accessed(now))
            .mark_in_sync()
            .overwrite()
            .blob(request.path().into_os_string().into_encoded_bytes());

        info!("Passing 1 directory and 1 file placeholder");
        match ticket.pass_with_placeholder(&mut [dir, file]) {
            Ok(_) => info!("Successfully created Documents directory placeholder"),
            Err(e) => info!("Failed to create placeholder: {:?}", e),
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
