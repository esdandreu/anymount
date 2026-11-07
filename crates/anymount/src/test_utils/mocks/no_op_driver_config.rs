use crate::domain::{Driver, DriverConfig};
use crate::test_utils::mocks::NoOpDriver;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct NoOpDriverConfig;

#[typetag::serde(name = "no-op")]
impl DriverConfig for NoOpDriverConfig {
    fn init(&self) -> Box<dyn Driver> {
        Box::new(NoOpDriver::default())
    }
}
