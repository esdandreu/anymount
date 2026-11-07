use std::rc::Rc;

use async_trait::async_trait;

use crate::domain::{Driver, Mount, MountConfig, MountError};
use crate::test_utils::mocks::NoOpMount;

#[derive(Default)]
pub struct NoOpDriver(Rc<()>);

#[async_trait(?Send)]
impl Driver for NoOpDriver {
    async fn mount(
        &self,
        _config: &MountConfig,
    ) -> Result<Box<dyn Mount>, MountError> {
        tokio::task::yield_now().await;
        let _ = &self.0;
        Ok(Box::new(NoOpMount::default()))
    }

    async fn unmount(&self, _name: &str) -> Result<(), MountError> {
        tokio::task::yield_now().await;
        let _ = &self.0;
        Ok(())
    }
}
