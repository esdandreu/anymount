use crate::domain::Storage;

#[derive(Default)]
pub struct NoOpStorage;

#[async_trait::async_trait(?Send)]
impl Storage for NoOpStorage {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
