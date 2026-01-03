use super::{CloudFilterProvider, Storage};
use cloud_filter::{
    error::CResult,
    filter::{Request, SyncFilter, info, ticket},
};
use std::sync::Arc;
use tracing::info;

// Newtype wrapper to implement SyncFilter for Arc<CloudFilterProvider<S>>
pub struct Callbacks<S: Storage>(pub Arc<CloudFilterProvider<S>>);

impl<S: Storage> SyncFilter for Callbacks<S> {
    fn fetch_data(
        &self,
        request: Request,
        ticket: ticket::FetchData,
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
        _info: info::FetchPlaceholders,
    ) -> CResult<()> {
        info!("fetch_placeholders: path={:?}", request.path());
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

    fn delete(&self, request: Request, ticket: ticket::Delete, _info: info::Delete) -> CResult<()> {
        info!("delete: path={:?}", request.path());
        CResult::Ok(())
    }

    fn deleted(&self, request: Request, _info: info::Deleted) {
        info!("deleted: path={:?}", request.path());
    }

    fn rename(&self, request: Request, ticket: ticket::Rename, info: info::Rename) -> CResult<()> {
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
