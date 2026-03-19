use crate::domain::provider::ProviderSpec;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatusRow {
    pub name: String,
    pub storage: Option<String>,
    pub path: Option<PathBuf>,
    pub ready: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvideRequest {
    pub spec: ProviderSpec,
    pub control_name: Option<String>,
}
