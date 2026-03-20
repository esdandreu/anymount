use crate::domain::driver::Driver;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverStatusRow {
    pub name: String,
    pub storage: Option<String>,
    pub path: Option<PathBuf>,
    pub ready: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvideRequest {
    pub spec: Driver,
    pub control_name: Option<String>,
}
