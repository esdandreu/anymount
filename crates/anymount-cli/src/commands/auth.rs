use std::io::Write;
use std::path::PathBuf;

use anymount::ConfigDir;
use anymount::domain::{ConfigRepository, DeviceAuthorization};
use color_eyre::eyre::eyre;

/// Authorizes an existing named storage configuration.
#[derive(clap::Args, Debug, Clone)]
pub struct AuthCommand {
    /// Configuration name, without its file extension.
    pub name: String,
}

impl AuthCommand {
    pub async fn execute(
        &self,
        config_dir: Option<PathBuf>,
    ) -> color_eyre::Result<()> {
        let repository =
            config_dir.map_or_else(ConfigDir::default, ConfigDir::new);
        let config = repository.find(&self.name)?;
        let session = config.mount.session.as_ref().ok_or_else(|| {
            eyre!("this storage does not support OAuth authorization")
        })?;
        let device = session.authorization().begin_device().await?;
        write_instructions(&mut std::io::stderr().lock(), &device)?;
        tokio::select! {
            result = session.authorization().poll_device() => result?,
            result = tokio::signal::ctrl_c() => {
                result?;
                session.authorization().cancel();
                return Err(eyre!("authorization cancelled"));
            }
        }
        loop {
            match session.save_pending().await {
                Ok(()) => break,
                Err(_) => {
                    eprintln!(
                        "Sign-in succeeded but saving failed; retrying the retained credentials. Ctrl-C to stop."
                    );
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {},
                        result = tokio::signal::ctrl_c() => {
                            result?;
                            return Err(eyre!("credentials remain unsaved"));
                        }
                    }
                }
            }
        }
        eprintln!("Credentials saved for '{}'.", self.name);
        Ok(())
    }
}

fn write_instructions(
    output: &mut impl Write,
    device: &DeviceAuthorization,
) -> std::io::Result<()> {
    writeln!(
        output,
        "Open {} and enter code {}.",
        device.verification_uri, device.user_code
    )?;
    if let Some(complete) = &device.verification_uri_complete {
        writeln!(output, "Or open {complete}")?;
    }
    writeln!(
        output,
        "Expires at Unix time {}. Waiting for sign-in...",
        device.expires_at
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_device_instructions_without_synthesizing_a_complete_url() {
        let device = DeviceAuthorization {
            verification_uri: "https://login.example/device".to_owned(),
            user_code: "ABCD-EFGH".to_owned(),
            verification_uri_complete: None,
            expires_at: 2000000000,
        };
        let mut output = Vec::new();
        write_instructions(&mut output, &device).expect("write instructions");
        let text = String::from_utf8(output).expect("utf8");
        assert!(text.contains("https://login.example/device"));
        assert!(text.contains("ABCD-EFGH"));
        assert!(text.contains("2000000000"));
        assert!(!text.contains("user_code="));
        assert!(!text.contains("Or open"));
    }

    #[test]
    fn displays_only_provider_supplied_complete_url() {
        let device = DeviceAuthorization {
            verification_uri: "https://login.example/device".to_owned(),
            user_code: "ABCD-EFGH".to_owned(),
            verification_uri_complete: Some(
                "https://login.example/device?provided=complete".to_owned(),
            ),
            expires_at: 2000000000,
        };
        let mut output = Vec::new();
        write_instructions(&mut output, &device).expect("write instructions");
        let text = String::from_utf8(output).expect("utf8");
        assert!(text.contains(
            "Or open https://login.example/device?provided=complete"
        ));
    }
}
