#ifndef ANYMOUNT_FFI_H
#define ANYMOUNT_FFI_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Initialize the XPC service with a storage provider
 * 
 * @param provider_ptr Pointer to the storage provider (currently unused, pass NULL)
 * @return true if initialization succeeded, false otherwise
 */
bool anymount_xpc_init(void* provider_ptr);

/**
 * Get metadata for a specific item
 * 
 * @param identifier The item identifier (null-terminated string)
 * @param out_json Pointer to receive the JSON string (must be freed with anymount_free_string)
 * @return true if successful, false otherwise
 */
bool anymount_xpc_get_item(const char* identifier, char** out_json);

/**
 * List items in a directory
 * 
 * @param container_id The container identifier (null-terminated string)
 * @param out_json Pointer to receive the JSON array string (must be freed with anymount_free_string)
 * @return true if successful, false otherwise
 */
bool anymount_xpc_list_items(const char* container_id, char** out_json);

/**
 * Fetch file contents
 * 
 * @param identifier The item identifier (null-terminated string)
 * @param out_path Pointer to receive the temporary file path (must be freed with anymount_free_string)
 * @return true if successful, false otherwise
 */
bool anymount_xpc_fetch_contents(const char* identifier, char** out_path);

/**
 * Free a string allocated by Rust
 * 
 * @param ptr The string to free
 */
void anymount_free_string(char* ptr);

/**
 * Shutdown the XPC service
 */
void anymount_xpc_shutdown(void);

#ifdef __cplusplus
}
#endif

#endif // ANYMOUNT_FFI_H

