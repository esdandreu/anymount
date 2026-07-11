use std::path::Path;

pub trait Mount: Send + Sync {
    fn name(&self) -> &str;

    // Should it be just Path?
    fn root(&self) -> &Path;

    fn is_connected(&self) -> bool;

    fn connect(&self) -> ();

    fn disconnect(&self) -> ();

    fn unregister(&self) -> ();
}
