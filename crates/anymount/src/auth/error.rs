#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    OneDrive(#[from] crate::auth::onedrive::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
