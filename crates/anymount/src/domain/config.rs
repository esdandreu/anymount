use crate::domain::storage::StorageConfig;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Config {
    // path: Path, ?
    pub storage: Box<dyn StorageConfig>,
    // driver: Option<Box<dyn DriverConfig>>,
}

#[cfg(test)]
mod test {
    use super::Config;
    use crate::domain::ConnectError::CannotConnect;

    #[test]
    fn deserializes_mount_config_from_toml() {
        let config: Config = toml::from_str(
            r#"
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
