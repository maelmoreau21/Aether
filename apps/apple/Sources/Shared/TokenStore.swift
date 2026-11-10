import Foundation
import Security

/// The device token in the Keychain, one item per server address. Readable
/// after the first unlock so the background refresh can rotate it; never
/// synced to iCloud nor restored onto another device.
enum TokenStore {
    private static let service = "app.aether.token"

    private struct Stored: Codable {
        let access: String
        let expiresAt: Int64
    }

    static func load(server: String) -> Token? {
        var query = base(server)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data,
              let stored = try? JSONDecoder().decode(Stored.self, from: data)
        else { return nil }
        return Token(access: stored.access, expiresAt: stored.expiresAt)
    }

    /// `nil` forgets the token (sign-out, refused token).
    static func save(_ token: Token?, server: String) {
        SecItemDelete(base(server) as CFDictionary)
        guard let token,
              let data = try? JSONEncoder().encode(Stored(access: token.access, expiresAt: token.expiresAt))
        else { return }
        var item = base(server)
        item[kSecValueData as String] = data
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(item as CFDictionary, nil)
    }

    private static func base(_ server: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: server,
        ]
    }
}
