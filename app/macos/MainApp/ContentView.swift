import SwiftUI
import FileProvider

struct ContentView: View {
    @State private var domains: [NSFileProviderDomain] = []
    @State private var statusMessage = "Ready"
    
    var body: some View {
        VStack(spacing: 20) {
            Text("Anymount FileProvider")
                .font(.largeTitle)
                .padding()
            
            Text(statusMessage)
                .foregroundColor(.secondary)
            
            if domains.isEmpty {
                Text("No mounts active")
                    .foregroundColor(.secondary)
            } else {
                List(domains, id: \.identifier) { domain in
                    HStack {
                        Image(systemName: "externaldrive")
                        Text(domain.displayName)
                    }
                }
                .frame(height: 200)
            }
            
            HStack(spacing: 15) {
                Button("Add Test Mount") {
                    addTestDomain()
                }
                
                Button("Refresh") {
                    loadDomains()
                }
                
                Button("Remove All") {
                    removeAllDomains()
                }
                .foregroundColor(.red)
            }
            .padding()
        }
        .frame(minWidth: 400, minHeight: 300)
        .padding()
        .onAppear {
            loadDomains()
        }
    }
    
    func loadDomains() {
        statusMessage = "Loading domains..."
        NSFileProviderManager.getDomainsWithCompletionHandler { domains, error in
            DispatchQueue.main.async {
                if let error = error {
                    statusMessage = "Error: \(error.localizedDescription)"
                    self.domains = []
                } else {
                    self.domains = Array(domains)
                    statusMessage = "Found \(domains.count) mount(s)"
                }
            }
        }
    }
    
    func addTestDomain() {
        let identifier = "com.anymount.test.\(UUID().uuidString.prefix(8))"
        let domain = NSFileProviderDomain(
            identifier: NSFileProviderDomainIdentifier(identifier),
            displayName: "Test Storage \(domains.count + 1)"
        )
        
        statusMessage = "Adding domain..."
        NSFileProviderManager.add(domain) { error in
            DispatchQueue.main.async {
                if let error = error {
                    statusMessage = "Error adding: \(error.localizedDescription)"
                } else {
                    statusMessage = "Domain added successfully!"
                    loadDomains()
                }
            }
        }
    }
    
    func removeAllDomains() {
        statusMessage = "Removing all domains..."
        for domain in domains {
            NSFileProviderManager.remove(domain) { error in
                if let error = error {
                    print("Error removing \(domain.displayName): \(error)")
                }
            }
        }
        
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            loadDomains()
        }
    }
}

#Preview {
    ContentView()
}

