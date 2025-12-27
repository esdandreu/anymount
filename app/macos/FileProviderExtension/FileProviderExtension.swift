import FileProvider
import Foundation
import UniformTypeIdentifiers

/// Main FileProvider extension class
///
/// This extension implements NSFileProviderReplicatedExtension to provide
/// file system integration with macOS Finder.
class FileProviderExtension: NSObject, NSFileProviderReplicatedExtension {
    
    /// Reference to the XPC service for communicating with Rust
    private var xpcService: AnymountXPCService?
    
    /// The domain this extension is serving
    private let domain: NSFileProviderDomain
    
    required init(domain: NSFileProviderDomain) {
        self.domain = domain
        super.init()
        
        // Connect to XPC service
        self.xpcService = AnymountXPCService()
        
        NSLog("[FileProvider] Extension initialized for domain: \(domain.displayName)")
    }
    
    func invalidate() {
        // Cleanup when extension is invalidated
        xpcService?.disconnect()
        NSLog("[FileProvider] Extension invalidated")
    }
    
    // MARK: - Item Fetching
    
    /// Fetch metadata for a specific item
    func item(for identifier: NSFileProviderItemIdentifier,
              request: NSFileProviderRequest,
              completionHandler: @escaping (NSFileProviderItem?, Error?) -> Void) -> Progress {
        
        NSLog("[FileProvider] Fetching item: \(identifier.rawValue)")
        
        let progress = Progress(totalUnitCount: 1)
        
        // Special handling for root
        if identifier == .rootContainer {
            let rootItem = FileProviderItem(
                identifier: .rootContainer,
                parentIdentifier: .rootContainer,
                filename: domain.displayName,
                contentType: .folder,
                capabilities: [.allowsContentEnumerating, .allowsAddingSubItems]
            )
            completionHandler(rootItem, nil)
            progress.completedUnitCount = 1
            return progress
        }
        
        // Fetch from XPC service
        xpcService?.getItem(identifier: identifier.rawValue) { item, error in
            if let error = error {
                NSLog("[FileProvider] Error fetching item: \(error)")
                completionHandler(nil, error)
            } else if let item = item {
                let providerItem = FileProviderItem(from: item)
                completionHandler(providerItem, nil)
            } else {
                completionHandler(nil, NSFileProviderError(.noSuchItem))
            }
            progress.completedUnitCount = 1
        }
        
        return progress
    }
    
    // MARK: - Content Fetching
    
    /// Fetch the contents of a file
    func fetchContents(for itemIdentifier: NSFileProviderItemIdentifier,
                      version requestedVersion: NSFileProviderItemVersion?,
                      request: NSFileProviderRequest,
                      completionHandler: @escaping (URL?, NSFileProviderItem?, Error?) -> Void) -> Progress {
        
        NSLog("[FileProvider] Fetching contents for: \(itemIdentifier.rawValue)")
        
        let progress = Progress(totalUnitCount: 100)
        
        xpcService?.fetchContents(identifier: itemIdentifier.rawValue) { localURL, item, error in
            if let error = error {
                NSLog("[FileProvider] Error fetching contents: \(error)")
                completionHandler(nil, nil, error)
            } else if let localURL = localURL, let item = item {
                let providerItem = FileProviderItem(from: item)
                completionHandler(localURL, providerItem, nil)
            } else {
                completionHandler(nil, nil, NSFileProviderError(.noSuchItem))
            }
            progress.completedUnitCount = 100
        }
        
        return progress
    }
    
    // MARK: - Creating Items
    
    func createItem(basedOn itemTemplate: NSFileProviderItem,
                   fields: NSFileProviderItemFields,
                   contents url: URL?,
                   options: NSFileProviderCreateItemOptions,
                   request: NSFileProviderRequest,
                   completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, Error?) -> Void) -> Progress {
        
        NSLog("[FileProvider] Creating item: \(itemTemplate.filename)")
        
        let progress = Progress(totalUnitCount: 1)
        
        xpcService?.createItem(
            filename: itemTemplate.filename,
            parentIdentifier: itemTemplate.parentItemIdentifier.rawValue,
            contentType: itemTemplate.contentType!,
            contents: url
        ) { item, error in
            if let error = error {
                NSLog("[FileProvider] Error creating item: \(error)")
                completionHandler(nil, [], false, error)
            } else if let item = item {
                let providerItem = FileProviderItem(from: item)
                completionHandler(providerItem, [], false, nil)
            } else {
                completionHandler(nil, [], false, NSFileProviderError(.noSuchItem))
            }
            progress.completedUnitCount = 1
        }
        
        return progress
    }
    
    // MARK: - Modifying Items
    
    func modifyItem(_ item: NSFileProviderItem,
                   baseVersion version: NSFileProviderItemVersion,
                   changedFields: NSFileProviderItemFields,
                   contents newContents: URL?,
                   options: NSFileProviderModifyItemOptions,
                   request: NSFileProviderRequest,
                   completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, Error?) -> Void) -> Progress {
        
        NSLog("[FileProvider] Modifying item: \(item.itemIdentifier.rawValue)")
        
        let progress = Progress(totalUnitCount: 1)
        
        xpcService?.modifyItem(
            identifier: item.itemIdentifier.rawValue,
            contents: newContents
        ) { updatedItem, error in
            if let error = error {
                NSLog("[FileProvider] Error modifying item: \(error)")
                completionHandler(nil, [], false, error)
            } else if let updatedItem = updatedItem {
                let providerItem = FileProviderItem(from: updatedItem)
                completionHandler(providerItem, [], false, nil)
            } else {
                completionHandler(nil, [], false, NSFileProviderError(.noSuchItem))
            }
            progress.completedUnitCount = 1
        }
        
        return progress
    }
    
    // MARK: - Deleting Items
    
    func deleteItem(identifier: NSFileProviderItemIdentifier,
                   baseVersion version: NSFileProviderItemVersion,
                   options: NSFileProviderDeleteItemOptions,
                   request: NSFileProviderRequest,
                   completionHandler: @escaping (Error?) -> Void) -> Progress {
        
        NSLog("[FileProvider] Deleting item: \(identifier.rawValue)")
        
        let progress = Progress(totalUnitCount: 1)
        
        xpcService?.deleteItem(identifier: identifier.rawValue) { error in
            if let error = error {
                NSLog("[FileProvider] Error deleting item: \(error)")
            }
            completionHandler(error)
            progress.completedUnitCount = 1
        }
        
        return progress
    }
}

// MARK: - Enumeration Support

extension FileProviderExtension: NSFileProviderEnumerating {
    
    func enumerator(for containerItemIdentifier: NSFileProviderItemIdentifier,
                   request: NSFileProviderRequest) throws -> NSFileProviderEnumerator {
        
        NSLog("[FileProvider] Creating enumerator for: \(containerItemIdentifier.rawValue)")
        
        return FileProviderEnumerator(
            containerIdentifier: containerItemIdentifier,
            xpcService: xpcService
        )
    }
}

