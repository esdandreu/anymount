use crate::service::{Error, Result};
use std::path::PathBuf;

const APP_STATE_DIR: &str = "anymount/providers";

pub fn provider_endpoint(provider_name: &str) -> Result<PathBuf> {
    validate_provider_name(provider_name)?;
    let state_dir = service_state_root();
    let file_name = format!("{provider_name}{}", endpoint_suffix());
    Ok(state_dir.join(APP_STATE_DIR).join(file_name))
}

fn service_state_root() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(std::env::temp_dir)
}

fn validate_provider_name(provider_name: &str) -> Result<()> {
    if provider_name.is_empty()
        || provider_name
            .chars()
            .any(|ch| !matches!(ch, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_'))
    {
        return Err(Error::InvalidProviderName {
            name: provider_name.to_owned(),
        });
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn endpoint_suffix() -> &'static str {
    ".pipe"
}

#[cfg(not(target_os = "windows"))]
fn endpoint_suffix() -> &'static str {
    ".sock"
}

#[cfg(test)]
mod tests {
    use super::provider_endpoint;
    use crate::service::Error;

    #[test]
    fn endpoint_path_is_stable_for_provider_name() {
        let a = provider_endpoint("demo").expect("path should build");
        let b = provider_endpoint("demo").expect("path should build");
        assert_eq!(a, b);
    }

    #[test]
    fn provider_endpoint_rejects_separator_in_provider_name() {
        let err = provider_endpoint("demo/provider").expect_err("path should fail");
        assert!(matches!(err, Error::InvalidProviderName { .. }));
    }
}
