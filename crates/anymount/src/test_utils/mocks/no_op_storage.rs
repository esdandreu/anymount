use crate::domain::Storage;

#[derive(Default)]
pub struct NoOpStorage;

impl Storage for NoOpStorage {}
