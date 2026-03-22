use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct RegistrationConfig {
    pub sync_root_path: PathBuf,
    pub display_name: String,
    pub provider_id: String,
    pub provider_version: String,
    pub icon_resource: Option<String>,
    pub show_overlays: bool,
    pub auto_populate: bool,
    pub hydration_policy: HydrationPolicy,
}

#[derive(Debug, Clone, Copy)]
pub enum HydrationPolicy {
    Progressive,
    Full,
    AlwaysFull,
}

impl Default for RegistrationConfig {
    fn default() -> Self {
        Self {
            sync_root_path: PathBuf::from(r"C:\Users\Public\Anymount"),
            display_name: "Anymount".to_string(),
            provider_id: "Anymount.CloudProvider".to_string(),
            provider_version: env!("CARGO_PKG_VERSION").to_string(),
            icon_resource: None,
            show_overlays: true,
            auto_populate: true,
            hydration_policy: HydrationPolicy::Progressive,
        }
    }
}
