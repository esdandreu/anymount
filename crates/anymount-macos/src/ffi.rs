/*!
FFI bridge for XPC communication between Swift and Rust.

This module provides C-compatible functions that can be called from Swift
to access the Rust storage provider implementation.
*/

use anymount_core::provider::{FileMetadata, FileType, StorageProvider};
use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Arc;
use tokio::runtime::Runtime;

use crate::{identifier_to_path, path_to_identifier};

/// Global state for the XPC service
static mut XPC_STATE: Option<XPCState> = None;

struct XPCState {
    runtime: Runtime,
    provider: Arc<dyn StorageProvider>,
}

/// Initialize the XPC service with a storage provider
///
/// # Safety
/// Must be called once before any other FFI functions
#[no_mangle]
pub unsafe extern "C" fn anymount_xpc_init(_provider_ptr: *mut std::ffi::c_void) -> bool {
    // This would receive a provider instance from the Rust side
    // For now, we'll use a mock provider
    
    let runtime = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Failed to create runtime: {}", e);
            return false;
        }
    };
    
    // In production, cast provider_ptr to the actual provider
    // For now, use mock
    let provider: Arc<dyn StorageProvider> = Arc::new(anymount_providers::MockProvider::new());
    
    XPC_STATE = Some(XPCState { runtime, provider });
    
    true
}

/// Get item metadata
///
/// # Safety
/// - identifier must be a valid null-terminated UTF-8 string
/// - out_json will be allocated and must be freed with anymount_free_string
#[no_mangle]
pub unsafe extern "C" fn anymount_xpc_get_item(
    identifier: *const c_char,
    out_json: *mut *mut c_char,
) -> bool {
    let state = match XPC_STATE.as_ref() {
        Some(s) => s,
        None => return false,
    };
    
    let identifier = match CStr::from_ptr(identifier).to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    
    // Convert FileProvider identifier to path
    let path = crate::identifier_to_path(identifier);
    let path = match path {
        Some(p) => p,
        None => return false,
    };
    
    // Fetch metadata from provider
    let result = state.runtime.block_on(async {
        state.provider.get_metadata(&path).await
    });
    
    match result {
        Ok(metadata) => {
            // Convert to XPC item format
            let xpc_item = XPCItem::from_metadata(&metadata, identifier);
            
            // Serialize to JSON
            match serde_json::to_string(&xpc_item) {
                Ok(json) => {
                    let c_json = CString::new(json).unwrap();
                    *out_json = c_json.into_raw();
                    true
                }
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

/// List items in a directory
///
/// # Safety
/// - container_id must be a valid null-terminated UTF-8 string  
/// - out_json will be allocated and must be freed with anymount_free_string
#[no_mangle]
pub unsafe extern "C" fn anymount_xpc_list_items(
    container_id: *const c_char,
    out_json: *mut *mut c_char,
) -> bool {
    let state = match XPC_STATE.as_ref() {
        Some(s) => s,
        None => return false,
    };
    
    let container_id = match CStr::from_ptr(container_id).to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    
    // Convert to path
    let path = crate::identifier_to_path(container_id).unwrap_or_else(|| "/".to_string());
    
    // List directory
    let result = state.runtime.block_on(async {
        state.provider.list_dir(&path).await
    });
    
    match result {
        Ok(items) => {
            let xpc_items: Vec<XPCItem> = items
                .iter()
                .map(|metadata| {
                    let id = crate::path_to_identifier(&metadata.path);
                    let parent_id = crate::path_to_identifier(&path);
                    XPCItem::from_metadata_with_parent(metadata, &id, &parent_id)
                })
                .collect();
            
            match serde_json::to_string(&xpc_items) {
                Ok(json) => {
                    let c_json = CString::new(json).unwrap();
                    *out_json = c_json.into_raw();
                    true
                }
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

/// Fetch file contents
///
/// # Safety
/// - identifier must be a valid null-terminated UTF-8 string
/// - out_path will be allocated and must be freed with anymount_free_string
#[no_mangle]
pub unsafe extern "C" fn anymount_xpc_fetch_contents(
    identifier: *const c_char,
    out_path: *mut *mut c_char,
) -> bool {
    let state = match XPC_STATE.as_ref() {
        Some(s) => s,
        None => return false,
    };
    
    let identifier = match CStr::from_ptr(identifier).to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    
    let path = match crate::identifier_to_path(identifier) {
        Some(p) => p,
        None => return false,
    };
    
    // Fetch file contents
    let result = state.runtime.block_on(async {
        state.provider.read_file(&path).await
    });
    
    match result {
        Ok(contents) => {
            // Write to temporary file
            let temp_path = format!("/tmp/anymount-{}.tmp", uuid::Uuid::new_v4());
            
            if let Err(_) = std::fs::write(&temp_path, &contents) {
                return false;
            }
            
            let c_path = CString::new(temp_path).unwrap();
            *out_path = c_path.into_raw();
            true
        }
        Err(_) => false,
    }
}

/// Free a string allocated by Rust
///
/// # Safety
/// - ptr must have been allocated by one of our FFI functions
/// - ptr must not be used after calling this function
#[no_mangle]
pub unsafe extern "C" fn anymount_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        let _ = CString::from_raw(ptr);
    }
}

/// Shutdown the XPC service
#[no_mangle]
pub unsafe extern "C" fn anymount_xpc_shutdown() {
    XPC_STATE = None;
}

// MARK: - XPC Item Structure

#[derive(Debug, Serialize, Deserialize)]
struct XPCItem {
    identifier: String,
    #[serde(rename = "parentIdentifier")]
    parent_identifier: String,
    filename: String,
    #[serde(rename = "isDirectory")]
    is_directory: bool,
    size: i64,
    created: Option<u64>,
    modified: Option<u64>,
}

impl XPCItem {
    fn from_metadata(metadata: &FileMetadata, identifier: &str) -> Self {
        let filename = std::path::Path::new(&metadata.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&metadata.path)
            .to_string();
        
        Self {
            identifier: identifier.to_string(),
            parent_identifier: "root".to_string(),
            filename,
            is_directory: matches!(metadata.file_type, FileType::Directory),
            size: metadata.size as i64,
            created: metadata.created,
            modified: metadata.modified,
        }
    }
    
    fn from_metadata_with_parent(
        metadata: &FileMetadata,
        identifier: &str,
        parent_id: &str,
    ) -> Self {
        let mut item = Self::from_metadata(metadata, identifier);
        item.parent_identifier = parent_id.to_string();
        item
    }
}

