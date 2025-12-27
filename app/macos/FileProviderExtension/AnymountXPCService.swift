import Foundation
import UniformTypeIdentifiers

/// XPC Service for communicating with the Rust anymount backend
class AnymountXPCService {
    
    private var connection: NSXPCConnection?
    private let serviceName = "com.anymount.xpc"
    
    init() {
        setupConnection()
    }
    
    private func setupConnection() {
        connection = NSXPCConnection(serviceName: serviceName)
        connection?.remoteObjectInterface = NSXPCInterface(with: AnymountXPCProtocol.self)
        
        connection?.invalidationHandler = {
            NSLog("[XPC] Connection invalidated")
        }
        
        connection?.interruptionHandler = {
            NSLog("[XPC] Connection interrupted")
        }
        
        connection?.resume()
        NSLog("[XPC] Connection established to: \(serviceName)")
    }
    
    func disconnect() {
        connection?.invalidate()
        connection = nil
    }
    
    private var service: AnymountXPCProtocol? {
        return connection?.remoteObjectProxyWithErrorHandler { error in
            NSLog("[XPC] Error getting remote object: \(error)")
        } as? AnymountXPCProtocol
    }
    
    // MARK: - Item Operations
    
    func getItem(identifier: String,
                 completion: @escaping (XPCItemData?, Error?) -> Void) {
        
        service?.getItem(identifier: identifier) { data, error in
            if let error = error {
                completion(nil, error)
                return
            }
            
            guard let data = data else {
                completion(nil, NSError(domain: "AnymountXPC", code: 1,
                                       userInfo: [NSLocalizedDescriptionKey: "No data returned"]))
                return
            }
            
            do {
                let item = try JSONDecoder().decode(XPCItemData.self, from: data)
                completion(item, nil)
            } catch {
                completion(nil, error)
            }
        }
    }
    
    func listItems(containerIdentifier: String,
                   completion: @escaping ([XPCItemData]?, Error?) -> Void) {
        
        service?.listItems(containerIdentifier: containerIdentifier) { data, error in
            if let error = error {
                completion(nil, error)
                return
            }
            
            guard let data = data else {
                completion(nil, NSError(domain: "AnymountXPC", code: 1,
                                       userInfo: [NSLocalizedDescriptionKey: "No data returned"]))
                return
            }
            
            do {
                let items = try JSONDecoder().decode([XPCItemData].self, from: data)
                completion(items, nil)
            } catch {
                completion(nil, error)
            }
        }
    }
    
    func fetchContents(identifier: String,
                      completion: @escaping (URL?, XPCItemData?, Error?) -> Void) {
        
        service?.fetchContents(identifier: identifier) { fileURL, itemData, error in
            if let error = error {
                completion(nil, nil, error)
                return
            }
            
            guard let itemData = itemData else {
                completion(nil, nil, NSError(domain: "AnymountXPC", code: 1,
                                             userInfo: [NSLocalizedDescriptionKey: "No item data"]))
                return
            }
            
            do {
                let item = try JSONDecoder().decode(XPCItemData.self, from: itemData)
                completion(fileURL, item, nil)
            } catch {
                completion(nil, nil, error)
            }
        }
    }
    
    func createItem(filename: String,
                   parentIdentifier: String,
                   contentType: UTType,
                   contents: URL?,
                   completion: @escaping (XPCItemData?, Error?) -> Void) {
        
        service?.createItem(
            filename: filename,
            parentIdentifier: parentIdentifier,
            isDirectory: contentType == .folder,
            contents: contents
        ) { data, error in
            if let error = error {
                completion(nil, error)
                return
            }
            
            guard let data = data else {
                completion(nil, NSError(domain: "AnymountXPC", code: 1,
                                       userInfo: [NSLocalizedDescriptionKey: "No data returned"]))
                return
            }
            
            do {
                let item = try JSONDecoder().decode(XPCItemData.self, from: data)
                completion(item, nil)
            } catch {
                completion(nil, error)
            }
        }
    }
    
    func modifyItem(identifier: String,
                   contents: URL?,
                   completion: @escaping (XPCItemData?, Error?) -> Void) {
        
        service?.modifyItem(identifier: identifier, contents: contents) { data, error in
            if let error = error {
                completion(nil, error)
                return
            }
            
            guard let data = data else {
                completion(nil, NSError(domain: "AnymountXPC", code: 1,
                                       userInfo: [NSLocalizedDescriptionKey: "No data returned"]))
                return
            }
            
            do {
                let item = try JSONDecoder().decode(XPCItemData.self, from: data)
                completion(item, nil)
            } catch {
                completion(nil, error)
            }
        }
    }
    
    func deleteItem(identifier: String,
                   completion: @escaping (Error?) -> Void) {
        
        service?.deleteItem(identifier: identifier) { error in
            completion(error)
        }
    }
}

// MARK: - XPC Protocol

@objc protocol AnymountXPCProtocol {
    
    func getItem(identifier: String,
                 reply: @escaping (Data?, Error?) -> Void)
    
    func listItems(containerIdentifier: String,
                  reply: @escaping (Data?, Error?) -> Void)
    
    func fetchContents(identifier: String,
                      reply: @escaping (URL?, Data?, Error?) -> Void)
    
    func createItem(filename: String,
                   parentIdentifier: String,
                   isDirectory: Bool,
                   contents: URL?,
                   reply: @escaping (Data?, Error?) -> Void)
    
    func modifyItem(identifier: String,
                   contents: URL?,
                   reply: @escaping (Data?, Error?) -> Void)
    
    func deleteItem(identifier: String,
                   reply: @escaping (Error?) -> Void)
}

