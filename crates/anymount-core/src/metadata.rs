// Metadata utilities and helpers
// This can be expanded later for caching, etc.

pub struct MetadataCache {
    // TODO: implement caching layer for file metadata
}

impl MetadataCache {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for MetadataCache {
    fn default() -> Self {
        Self::new()
    }
}

