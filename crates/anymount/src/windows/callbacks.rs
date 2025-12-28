use crate::StorageProvider;
use cloud_filter::{
    error::{CResult, CloudErrorKind},
    filter::{info, ticket, Request, SyncFilter},
    placeholder_file::PlaceholderFile,
    root::Session,
    utility::WriteAt,
};
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::{debug, error, info, trace};

pub struct CloudFilterCallbacks {
    provider: Arc<dyn StorageProvider>,
    #[allow(dead_code)]
    session: Session,
    runtime: Arc<Runtime>,
}

impl CloudFilterCallbacks {
    pub fn new(provider: Arc<dyn StorageProvider>, session: Session, runtime: Arc<Runtime>) -> Self {
        Self { provider, session, runtime }
    }
}

impl SyncFilter for CloudFilterCallbacks {
    fn fetch_data(
        &self,
        request: Request,
        ticket: ticket::FetchData,
        info: info::FetchData,
    ) -> CResult<()> {
        debug!(
            "fetch_data: path={:?}, offset={}, length={}",
            request.path(),
            info.required_file_range().start,
            info.required_file_range().end - info.required_file_range().start
        );

        let path = request.path().to_string_lossy().to_string();
        let offset = info.required_file_range().start;
        let length = info.required_file_range().end - offset;

        let provider = self.provider.clone();
        let runtime = self.runtime.clone();

        match runtime.block_on(async {
            if length == 0 {
                provider.read_file(&path).await
            } else {
                provider.read_file_range(&path, offset, length).await
            }
        }) {
            Ok(data) => {
                trace!("Hydrating file with {} bytes", data.len());
                match ticket.write_at(&data, offset) {
                    Ok(_) => {
                        info!("✓ File hydrated: {}", path);
                        Ok(())
                    }
                    Err(e) => {
                        error!("Failed to write data for {}: {:?}", path, e);
                        Err(CloudErrorKind::InvalidRequest)
                    }
                }
            }
            Err(e) => {
                error!("Failed to read file {}: {:?}", path, e);
                Err(CloudErrorKind::InvalidRequest)
            }
        }
    }

    fn fetch_placeholders(
        &self,
        request: Request,
        ticket: ticket::FetchPlaceholders,
        _info: info::FetchPlaceholders,
    ) -> CResult<()> {
        debug!("fetch_placeholders: path={:?}", request.path());

        let path = request.path().to_string_lossy().to_string();
        let provider = self.provider.clone();
        let runtime = self.runtime.clone();

        match runtime.block_on(async {
            let normalized_path = if path == "\\" || path.is_empty() {
                "/"
            } else {
                path.trim_start_matches('\\').trim_end_matches('\\')
            };
            provider.list_dir("/").await
        }) {
            Ok(entries) => {
                debug!("Fetched {} entries from {}", entries.len(), path);

                let mut placeholder_files = Vec::new();
                for entry in entries {
                    let mut placeholder = PlaceholderFile::new(&entry.path);
                    
                    if entry.file_type == crate::FileType::File {
                        placeholder = placeholder.metadata(
                            cloud_filter::metadata::Metadata::default()
                                .size(entry.size)
                        );
                    }
                    
                    placeholder_files.push(placeholder);
                }

                match ticket.pass_with_placeholder(&mut placeholder_files) {
                    Ok(_) => {
                        info!("✓ Created {} placeholders in {}", placeholder_files.len(), path);
                        Ok(())
                    }
                    Err(e) => {
                        error!("Failed to create placeholders: {:?}", e);
                        Err(CloudErrorKind::InvalidRequest)
                    }
                }
            }
            Err(e) => {
                error!("Failed to list directory {}: {:?}", path, e);
                Err(CloudErrorKind::InvalidRequest)
            }
        }
    }

    fn cancel_fetch_data(&self, request: Request, _info: info::CancelFetchData) {
        debug!("cancel_fetch_data: path={:?}", request.path());
    }

    fn cancel_fetch_placeholders(&self, request: Request, _info: info::CancelFetchPlaceholders) {
        debug!("cancel_fetch_placeholders: path={:?}", request.path());
    }

    fn opened(&self, request: Request, _info: info::Opened) {
        trace!("opened: path={:?}", request.path());
    }

    fn closed(&self, request: Request, _info: info::Closed) {
        trace!("closed: path={:?}", request.path());
    }

    fn delete(
        &self,
        request: Request,
        ticket: ticket::Delete,
        _info: info::Delete,
    ) -> CResult<()> {
        debug!("delete: path={:?}", request.path());
        
        match ticket.pass() {
            Ok(_) => {
                info!("✓ File delete approved: {:?}", request.path());
                Ok(())
            }
            Err(_e) => {
                error!("Failed to approve delete");
                Err(CloudErrorKind::InvalidRequest)
            }
        }
    }

    fn deleted(&self, request: Request, _info: info::Deleted) {
        debug!("deleted: path={:?}", request.path());
    }

    fn rename(
        &self,
        request: Request,
        ticket: ticket::Rename,
        info: info::Rename,
    ) -> CResult<()> {
        debug!(
            "rename: from={:?} to={:?}",
            request.path(),
            info.target_path()
        );
        
        match ticket.pass() {
            Ok(_) => {
                info!("✓ File rename approved: {:?} -> {:?}", request.path(), info.target_path());
                Ok(())
            }
            Err(_e) => {
                error!("Failed to approve rename");
                Err(CloudErrorKind::InvalidRequest)
            }
        }
    }

    fn renamed(&self, request: Request, info: info::Renamed) {
        debug!(
            "renamed: from={:?} to={:?}",
            info.source_path(),
            request.path()
        );
    }
}

