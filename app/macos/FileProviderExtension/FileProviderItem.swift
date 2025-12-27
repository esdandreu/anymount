import FileProvider
import Foundation
import UniformTypeIdentifiers

/// Represents an item in the file provider
class FileProviderItem: NSObject, NSFileProviderItem {
    
    let itemIdentifier: NSFileProviderItemIdentifier
    let parentItemIdentifier: NSFileProviderItemIdentifier
    let filename: String
    let contentType: UTType
    let capabilities: NSFileProviderItemCapabilities
    
    var documentSize: NSNumber?
    var childItemCount: NSNumber?
    var creationDate: Date?
    var contentModificationDate: Date?
    
    // MARK: - Initialization
    
    init(identifier: NSFileProviderItemIdentifier,
         parentIdentifier: NSFileProviderItemIdentifier,
         filename: String,
         contentType: UTType,
         capabilities: NSFileProviderItemCapabilities,
         size: Int64? = nil,
         created: Date? = nil,
         modified: Date? = nil) {
        
        self.itemIdentifier = identifier
        self.parentItemIdentifier = parentIdentifier
        self.filename = filename
        self.contentType = contentType
        self.capabilities = capabilities
        
        if let size = size {
            self.documentSize = NSNumber(value: size)
        }
        
        self.creationDate = created
        self.contentModificationDate = modified
        
        super.init()
    }
    
    /// Initialize from XPC item data
    convenience init(from xpcItem: XPCItemData) {
        let identifier = NSFileProviderItemIdentifier(xpcItem.identifier)
        let parentIdentifier = NSFileProviderItemIdentifier(xpcItem.parentIdentifier)
        
        // Determine content type
        let utType: UTType
        if xpcItem.isDirectory {
            utType = .folder
        } else {
            utType = Self.contentType(for: xpcItem.filename)
        }
        
        // Determine capabilities
        var caps: NSFileProviderItemCapabilities = [
            .allowsReading,
            .allowsDeleting,
            .allowsRenaming
        ]
        
        if xpcItem.isDirectory {
            caps.insert(.allowsContentEnumerating)
            caps.insert(.allowsAddingSubItems)
        } else {
            caps.insert(.allowsWriting)
        }
        
        // Convert timestamps
        let created = xpcItem.created.map { Date(timeIntervalSince1970: TimeInterval($0)) }
        let modified = xpcItem.modified.map { Date(timeIntervalSince1970: TimeInterval($0)) }
        
        self.init(
            identifier: identifier,
            parentIdentifier: parentIdentifier,
            filename: xpcItem.filename,
            contentType: utType,
            capabilities: caps,
            size: xpcItem.size,
            created: created,
            modified: modified
        )
    }
    
    // MARK: - Content Type Detection
    
    private static func contentType(for filename: String) -> UTType {
        let ext = (filename as NSString).pathExtension.lowercased()
        
        switch ext {
        case "txt":
            return .plainText
        case "pdf":
            return .pdf
        case "jpg", "jpeg":
            return .jpeg
        case "png":
            return .png
        case "gif":
            return .gif
        case "mp4":
            return .mpeg4Movie
        case "mov":
            return .quickTimeMovie
        case "mp3":
            return .mp3
        case "zip":
            return .zip
        case "json":
            return .json
        case "xml":
            return .xml
        case "html", "htm":
            return .html
        case "css":
            return .text
        case "js":
            return .javaScript
        case "md":
            return .text
        default:
            return .data
        }
    }
}

// MARK: - XPC Data Structure

/// Data structure for items passed over XPC
struct XPCItemData: Codable {
    let identifier: String
    let parentIdentifier: String
    let filename: String
    let isDirectory: Bool
    let size: Int64
    let created: UInt64?
    let modified: UInt64?
}

