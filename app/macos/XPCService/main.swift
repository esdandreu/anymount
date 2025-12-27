import Foundation

// MARK: - XPC Protocol

@objc protocol AnymountXPCProtocol {
    func getItem(identifier: String, reply: @escaping (Data?, Error?) -> Void)
    func listItems(containerIdentifier: String, reply: @escaping (Data?, Error?) -> Void)
    func fetchContents(identifier: String, reply: @escaping (URL?, Data?, Error?) -> Void)
    func createItem(filename: String, parentIdentifier: String, isDirectory: Bool, contents: URL?, reply: @escaping (Data?, Error?) -> Void)
    func modifyItem(identifier: String, contents: URL?, reply: @escaping (Data?, Error?) -> Void)
    func deleteItem(identifier: String, reply: @escaping (Error?) -> Void)
}

/// XPC Service that runs the Rust anymount backend
class AnymountXPCServiceDelegate: NSObject, NSXPCListenerDelegate, AnymountXPCProtocol {
    
    func listener(_ listener: NSXPCListener,
                 shouldAcceptNewConnection newConnection: NSXPCConnection) -> Bool {
        
        newConnection.exportedInterface = NSXPCInterface(with: AnymountXPCProtocol.self)
        newConnection.exportedObject = self
        newConnection.resume()
        
        NSLog("[XPC Service] Accepted new connection")
        return true
    }
    
    // MARK: - AnymountXPCProtocol Implementation
    
    func getItem(identifier: String, reply: @escaping (Data?, Error?) -> Void) {
        NSLog("[XPC Service] getItem: \(identifier)")
        
        // Call Rust FFI
        anymount_get_item(identifier) { data, error in
            reply(data, error)
        }
    }
    
    func listItems(containerIdentifier: String, reply: @escaping (Data?, Error?) -> Void) {
        NSLog("[XPC Service] listItems: \(containerIdentifier)")
        
        // Call Rust FFI
        anymount_list_items(containerIdentifier) { data, error in
            reply(data, error)
        }
    }
    
    func fetchContents(identifier: String, reply: @escaping (URL?, Data?, Error?) -> Void) {
        NSLog("[XPC Service] fetchContents: \(identifier)")
        
        // Call Rust FFI
        anymount_fetch_contents(identifier) { url, data, error in
            reply(url, data, error)
        }
    }
    
    func createItem(filename: String,
                   parentIdentifier: String,
                   isDirectory: Bool,
                   contents: URL?,
                   reply: @escaping (Data?, Error?) -> Void) {
        
        NSLog("[XPC Service] createItem: \(filename) in \(parentIdentifier)")
        
        // Call Rust FFI
        anymount_create_item(filename, parentIdentifier, isDirectory, contents) { data, error in
            reply(data, error)
        }
    }
    
    func modifyItem(identifier: String,
                   contents: URL?,
                   reply: @escaping (Data?, Error?) -> Void) {
        
        NSLog("[XPC Service] modifyItem: \(identifier)")
        
        // Call Rust FFI
        anymount_modify_item(identifier, contents) { data, error in
            reply(data, error)
        }
    }
    
    func deleteItem(identifier: String, reply: @escaping (Error?) -> Void) {
        NSLog("[XPC Service] deleteItem: \(identifier)")
        
        // Call Rust FFI
        anymount_delete_item(identifier) { error in
            reply(error)
        }
    }
}

// MARK: - XPC Item Data

struct XPCItemData: Codable {
    let identifier: String
    let parentIdentifier: String
    let filename: String
    let isDirectory: Bool
    let size: Int64
    let created: UInt64?
    let modified: UInt64?
}

// MARK: - Main Entry Point

let delegate = AnymountXPCServiceDelegate()
let listener = NSXPCListener.service()
listener.delegate = delegate
listener.resume()

NSLog("[XPC Service] Started")
RunLoop.main.run()

// MARK: - Rust FFI Declarations

/// These functions will be implemented in Rust and exposed via FFI
/// For now, they're declared here to show the interface

private func anymount_get_item(_ identifier: String,
                               completion: @escaping (Data?, Error?) -> Void) {
    // TODO: Call Rust implementation
    // For now, return mock data
    let mockItem = XPCItemData(
        identifier: identifier,
        parentIdentifier: "root",
        filename: "mock-file.txt",
        isDirectory: false,
        size: 1024,
        created: UInt64(Date().timeIntervalSince1970),
        modified: UInt64(Date().timeIntervalSince1970)
    )
    
    if let data = try? JSONEncoder().encode(mockItem) {
        completion(data, nil)
    } else {
        completion(nil, NSError(domain: "AnymountXPC", code: 1,
                               userInfo: [NSLocalizedDescriptionKey: "Encoding failed"]))
    }
}

private func anymount_list_items(_ containerIdentifier: String,
                                completion: @escaping (Data?, Error?) -> Void) {
    // TODO: Call Rust implementation
    let mockItems: [XPCItemData] = []
    
    if let data = try? JSONEncoder().encode(mockItems) {
        completion(data, nil)
    } else {
        completion(nil, NSError(domain: "AnymountXPC", code: 1,
                               userInfo: [NSLocalizedDescriptionKey: "Encoding failed"]))
    }
}

private func anymount_fetch_contents(_ identifier: String,
                                    completion: @escaping (URL?, Data?, Error?) -> Void) {
    // TODO: Call Rust implementation
    completion(nil, nil, NSError(domain: "AnymountXPC", code: 501,
                                userInfo: [NSLocalizedDescriptionKey: "Not implemented"]))
}

private func anymount_create_item(_ filename: String,
                                 _ parentIdentifier: String,
                                 _ isDirectory: Bool,
                                 _ contents: URL?,
                                 completion: @escaping (Data?, Error?) -> Void) {
    // TODO: Call Rust implementation
    completion(nil, NSError(domain: "AnymountXPC", code: 501,
                           userInfo: [NSLocalizedDescriptionKey: "Not implemented"]))
}

private func anymount_modify_item(_ identifier: String,
                                 _ contents: URL?,
                                 completion: @escaping (Data?, Error?) -> Void) {
    // TODO: Call Rust implementation
    completion(nil, NSError(domain: "AnymountXPC", code: 501,
                           userInfo: [NSLocalizedDescriptionKey: "Not implemented"]))
}

private func anymount_delete_item(_ identifier: String,
                                 completion: @escaping (Error?) -> Void) {
    // TODO: Call Rust implementation
    completion(NSError(domain: "AnymountXPC", code: 501,
                      userInfo: [NSLocalizedDescriptionKey: "Not implemented"]))
}

