/// Applies native thread-safety bounds outside WebAssembly.
#[cfg(not(target_arch = "wasm32"))]
pub trait PlatformSendSync: Send + Sync {}

#[cfg(not(target_arch = "wasm32"))]
impl<T> PlatformSendSync for T where T: Send + Sync + ?Sized {}

/// Omits native thread-safety bounds for browser targets.
#[cfg(target_arch = "wasm32")]
pub trait PlatformSendSync {}

#[cfg(target_arch = "wasm32")]
impl<T> PlatformSendSync for T where T: ?Sized {}
