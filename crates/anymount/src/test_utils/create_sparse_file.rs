use std::{fs::File, io, path::Path};

/// Creates a sparse file for large-offset storage tests.
///
/// # Errors
///
/// Returns an error if creating, marking, or extending the file fails.
pub fn create_sparse_file(path: &Path, len: u64) -> io::Result<File> {
    let file = File::create(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_SPARSE},
        };

        let mut returned = 0;
        // SAFETY: The file owns a live synchronous handle, and the output
        // pointer remains valid until DeviceIoControl returns. No buffers
        // are required to enable sparse allocation.
        unsafe {
            DeviceIoControl(
                HANDLE(file.as_raw_handle()),
                FSCTL_SET_SPARSE,
                None,
                0,
                None,
                0,
                Some(&mut returned),
                None,
            )
        }
        .map_err(io::Error::other)?;
    }
    file.set_len(len)?;
    Ok(file)
}
