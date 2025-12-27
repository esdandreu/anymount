import FileProvider
import Foundation

/// Enumerator for listing directory contents
class FileProviderEnumerator: NSObject, NSFileProviderEnumerator {
    
    private let containerIdentifier: NSFileProviderItemIdentifier
    private weak var xpcService: AnymountXPCService?
    
    init(containerIdentifier: NSFileProviderItemIdentifier,
         xpcService: AnymountXPCService?) {
        self.containerIdentifier = containerIdentifier
        self.xpcService = xpcService
        super.init()
    }
    
    func invalidate() {
        // Cleanup
    }
    
    func enumerateItems(for observer: NSFileProviderEnumerationObserver,
                       startingAt page: NSFileProviderPage) {
        
        NSLog("[FileProvider] Enumerating items in: \(containerIdentifier.rawValue)")
        
        xpcService?.listItems(containerIdentifier: containerIdentifier.rawValue) { items, error in
            if let error = error {
                NSLog("[FileProvider] Enumeration error: \(error)")
                observer.finishEnumeratingWithError(error)
                return
            }
            
            guard let items = items else {
                observer.finishEnumeratingWithError(NSFileProviderError(.noSuchItem))
                return
            }
            
            // Convert XPC items to FileProviderItems
            let providerItems = items.map { FileProviderItem(from: $0) }
            
            NSLog("[FileProvider] Enumerated \(providerItems.count) items")
            
            observer.didEnumerate(providerItems)
            observer.finishEnumerating(upTo: nil)
        }
    }
    
    func enumerateChanges(for observer: NSFileProviderChangeObserver,
                         from anchor: NSFileProviderSyncAnchor) {
        
        NSLog("[FileProvider] Enumerating changes from anchor")
        
        // For simplicity, we'll do a full re-enumeration
        // In production, you'd track changes and only report deltas
        xpcService?.listItems(containerIdentifier: containerIdentifier.rawValue) { items, error in
            if let error = error {
                NSLog("[FileProvider] Change enumeration error: \(error)")
                observer.finishEnumeratingWithError(error)
                return
            }
            
            guard let items = items else {
                observer.finishEnumeratingWithError(NSFileProviderError(.noSuchItem))
                return
            }
            
            let providerItems = items.map { FileProviderItem(from: $0) }
            
            observer.didUpdate(providerItems)
            
            // Create new anchor (current timestamp)
            let newAnchor = NSFileProviderSyncAnchor(String(Date().timeIntervalSince1970).data(using: .utf8)!)
            observer.finishEnumeratingChanges(upTo: newAnchor, moreComing: false)
        }
    }
    
    func currentSyncAnchor(completionHandler: @escaping (NSFileProviderSyncAnchor?) -> Void) {
        // Return current timestamp as anchor
        let anchor = NSFileProviderSyncAnchor(String(Date().timeIntervalSince1970).data(using: .utf8)!)
        completionHandler(anchor)
    }
}

