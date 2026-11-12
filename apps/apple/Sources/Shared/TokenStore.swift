import Foundation
import Security

/// An account signed in on this device and set aside while another one is used.
struct SavedAccount: Codable, Identifiable, Equatable {
    let id: String
    var name: String
    var access: String
    var expiresAt: Int64

    var token: Token { Token(access: access, expiresAt: expiresAt) }
}

/// The device tokens in the Keychain, per server address: the active
/// account's, and those of the accounts set aside. Readable after the first
/// unlock so the background refresh can rotate them; never synced to iCloud
/// nor restored onto another device.
enum TokenStore {
    private static let service = "app.aether.token"

    private struct Stored: Codable {
        let access: String
        let expiresAt: Int64
    }

    static func load(server: String) -> Token? {
        guard let data = read(server),
              let stored = try? JSONDecoder().decode(Stored.self, from: data)
        else { return nil }
        return Token(access: stored.access, expiresAt: stored.expiresAt)
    }

    /// `nil` forgets the token (sign-out, refused token).
    static func save(_ token: Token?, server: String) {
        write(token.flatMap { try? JSONEncoder().encode(Stored(access: $0.access, expiresAt: $0.expiresAt)) }, server)
    }

    /// The accounts set aside on this server, most recently used first.
    static func parked(server: String) -> [SavedAccount] {
        guard let data = read(server + "#accounts") else { return [] }
        return (try? JSONDecoder().decode([SavedAccount].self, from: data)) ?? []
    }

    static func setParked(_ accounts: [SavedAccount], server: String) {
        let data = accounts.isEmpty ? nil : (try? JSONEncoder().encode(accounts))
        write(data, server + "#accounts")
    }

    private static func read(_ account: String) -> Data? {
        var query = base(account)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess else { return nil }
        return result as? Data
    }

    private static func write(_ data: Data?, _ account: String) {
        SecItemDelete(base(account) as CFDictionary)
        guard let data else { return }
        var item = base(account)
        item[kSecValueData as String] = data
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(item as CFDictionary, nil)
    }

    private static func base(_ account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
    }
}
