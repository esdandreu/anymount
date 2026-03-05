use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::CloudFilters::{
    CF_DEHYDRATE_FLAG_NONE, CF_IN_SYNC_STATE, CF_PIN_STATE, CF_PLACEHOLDER_INFO_STANDARD,
    CF_PLACEHOLDER_STANDARD_INFO, CfDehydratePlaceholder, CfGetPlaceholderInfo,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, OPEN_EXISTING,
};
use windows::core::PCWSTR;

const FILE_ID_MAX_LENGTH: u32 = 400;

/// Opens a handle to a file or directory for placeholder operations.
/// When `open_as_placeholder` is true, uses `FILE_FLAG_OPEN_REPARSE_POINT`;
/// when the path is a directory, adds `FILE_FLAG_BACKUP_SEMANTICS`.
pub fn open_file_handle(
    path: &Path,
    desired_access: u32,
    open_as_placeholder: bool,
) -> Result<OwnedHandle, String> {
    let is_directory = path.is_dir();
    let mut flags: u32 = 0;
    if open_as_placeholder {
        flags |= FILE_FLAG_OPEN_REPARSE_POINT.0;
    }
    if is_directory {
        flags |= FILE_FLAG_BACKUP_SEMANTICS.0;
    }
    let share_mode = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            PCWSTR::from_raw(wide.as_ptr()),
            desired_access,
            share_mode,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(flags),
            None,
        )
    }
    .map_err(|e| format!("Failed to open file handle: {}", e))?;
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0 as _) })
}

/// Placeholder metadata returned by `get_placeholder_info`.
#[derive(Debug, Clone)]
pub struct PlaceholderState {
    pub placeholder_id: String,
    pub uuid: String,
    pub pin_state: CF_PIN_STATE,
    pub in_sync_state: CF_IN_SYNC_STATE,
    pub on_disk_size: i64,
}

/// Reads placeholder metadata for the given path (file or directory).
pub fn get_placeholder_info(path: &Path) -> Result<PlaceholderState, String> {
    let handle = open_file_handle(path, FILE_READ_ATTRIBUTES.0, true)?;
    let info_size = (std::mem::size_of::<CF_PLACEHOLDER_STANDARD_INFO>()
        + (FILE_ID_MAX_LENGTH as usize)
        - 1) as u32;
    let mut buffer = vec![0u8; info_size as usize];
    unsafe {
        CfGetPlaceholderInfo(
            HANDLE(handle.as_raw_handle() as _),
            CF_PLACEHOLDER_INFO_STANDARD,
            buffer.as_mut_ptr() as _,
            info_size,
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    let info = unsafe { &*(buffer.as_ptr() as *const CF_PLACEHOLDER_STANDARD_INFO) };
    let identity_len = info.FileIdentityLength as usize;
    let identity_start = std::mem::size_of::<CF_PLACEHOLDER_STANDARD_INFO>() - 1;
    let identity_end = (identity_start + identity_len).min(buffer.len());
    let identity_bytes = &buffer[identity_start..identity_end];
    let placeholder_id = String::from_utf8_lossy(identity_bytes)
        .replace('\0', "")
        .trim()
        .to_string();
    let uuid = placeholder_id.split(':').nth(1).unwrap_or("").to_string();
    Ok(PlaceholderState {
        placeholder_id,
        uuid,
        pin_state: info.PinState,
        in_sync_state: info.InSyncState,
        on_disk_size: info.OnDiskDataSize,
    })
}

/// Dehydrates a placeholder file so its data is no longer present on disk.
pub fn dehydrate_file(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        return Err("Cannot dehydrate folder".to_string());
    }
    let handle = open_file_handle(path, FILE_WRITE_ATTRIBUTES.0, true)?;
    unsafe {
        CfDehydratePlaceholder(
            HANDLE(handle.as_raw_handle() as _),
            0i64,
            -1i64,
            CF_DEHYDRATE_FLAG_NONE,
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(())
}
