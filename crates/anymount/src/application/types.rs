use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatusRow {
    pub name: String,
    pub storage: Option<String>,
    pub path: Option<PathBuf>,
    pub ready: bool,
    pub error: Option<String>,
}
