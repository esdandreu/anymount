use crate::service::{Error, Result};
use std::path::PathBuf;

const APP_STATE_DIR: &str = "anymount/drivers";

pub fn driver_endpoint(driver_name: &str) -> Result<PathBuf> {
    validate_driver_name(driver_name)?;
    let state_dir = service_state_root();
    let file_name = format!("{driver_name}{}", endpoint_suffix());
    Ok(state_dir.join(APP_STATE_DIR).join(file_name))
}

fn service_state_root() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(std::env::temp_dir)
}

fn validate_driver_name(driver_name: &str) -> Result<()> {
    if driver_name.is_empty()
        || driver_name
            .chars()
            .any(|ch| !matches!(ch, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_'))
    {
        return Err(Error::InvalidDriverName {
            name: driver_name.to_owned(),
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
    use super::driver_endpoint;
    use crate::service::Error;

    #[test]
    fn endpoint_path_is_stable_for_driver_name() {
        let a = driver_endpoint("demo").expect("path should build");
        let b = driver_endpoint("demo").expect("path should build");
        assert_eq!(a, b);
    }

    #[test]
    fn driver_endpoint_rejects_separator_in_driver_name() {
        let err = driver_endpoint("demo/driver").expect_err("path should fail");
        assert!(matches!(err, Error::InvalidDriverName { .. }));
    }
}
