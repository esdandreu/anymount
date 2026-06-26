use std::path::PathBuf;

use crate::domain::{driver::DriverConfig, storage::StorageConfig};

// TODO(GIA) Should it be a trait? This struct would be specific to the
// ConfigDir repository.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub path: PathBuf,
    pub storage: Box<dyn StorageConfig>,
    pub driver: Option<Box<dyn DriverConfig>>,
}

#[cfg(test)]
mod test {
    use super::Config;
    use crate::domain::ConnectError::CannotConnect;

    #[test]
    fn deserializes_mount_config_from_toml() {
        let config: Config = toml::from_str(
            r#"
path = "/mnt/test"
[storage]
type = "cannot-connect"
error_message = "Hello world!"
"#,
        )
        .expect("deserialize mount config");

        match config.storage.connect() {
            Err(CannotConnect { message }) => {
                assert_eq!(message, "Hello world!");
            }
            Ok(_) => panic!("should fail!"),
        }
    }
}
