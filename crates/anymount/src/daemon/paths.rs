use std::path::PathBuf;

const APP_STATE_DIR: &str = "anymount/providers";

pub fn provider_endpoint(provider_name: &str) -> Result<PathBuf, String> {
    let state_dir = dirs::state_dir()
        .ok_or_else(|| "could not resolve state directory for daemon endpoints".to_owned())?;
    let file_name = format!("{}{}", sanitize(provider_name), endpoint_suffix());
    Ok(state_dir.join(APP_STATE_DIR).join(file_name))
}

fn sanitize(provider_name: &str) -> String {
    provider_name
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '_',
        })
        .collect()
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

    #[test]
    fn endpoint_path_is_stable_for_provider_name() {
        let a = provider_endpoint("demo").expect("path should build");
        let b = provider_endpoint("demo").expect("path should build");
        assert_eq!(a, b);
    }

    #[test]
    fn endpoint_path_sanitizes_provider_name() {
        let path = provider_endpoint("demo/provider").expect("path should build");
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("file name should be utf-8");
        assert!(file_name.starts_with("demo_provider"));
    }
}
