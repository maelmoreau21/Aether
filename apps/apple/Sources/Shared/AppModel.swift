import BackgroundTasks
import Foundation
import Observation
import UIKit

enum Phase {
    case server, signIn, ready
}

/// A title to play full screen; `startAt` (seconds) overrides the saved position.
struct PlayRequest: Identifiable, Equatable {
    let id = UUID()
    let workId: String
    let title: String
    var startAt: Double?
}

/// An account signed in on this device, as Settings and the sign-in page list it.
struct AccountEntry: Identifiable, Equatable {
    let id: String
    let name: String
    let current: Bool
}

/// Session and navigation state. Every Core call goes through the Rust core
/// (`AetherClient`); only the token's storage (Keychain), the remote-control
/// socket and the players are native.
@MainActor
@Observable
final class AppModel {
    static let refreshTaskIdentifier = "app.aether.refresh"
    /// Refresh when the token expires within 7 days (it lasts 30).
    private static let refreshMargin: Int64 = 7 * 24 * 3600
    /// Accounts kept beside the active one.
    private static let maxParked = 5

    private(set) var phase: Phase = .server
    private(set) var client: AetherClient?
    private(set) var serverAddress: String?
    private(set) var pairing: Pairing?
    /// The active account first, then the accounts set aside on this device.
    private(set) var accounts: [AccountEntry] = []
    var error: String?
    var playing: PlayRequest?
    /// The current playback; it outlives the player screen in Picture in Picture.
    var session: PlaybackSession?
    /// Bumped after sign-in and when coming back to the app: screens reload.
    private(set) var generation = 0

    let remote = RemoteControl()
    let deviceName: String
    @ObservationIgnored private var pairingTask: Task<Void, Never>?
    @ObservationIgnored private var lastForeground = Date.distantPast

    init() {
        deviceName = UIDevice.current.name
        serverAddress = UserDefaults.standard.string(forKey: "server")
        remote.onCommand = { [weak self] command in self?.handle(command) }
        #if os(iOS)
        DownloadStore.shared.client = { [weak self] in self?.client }
        #endif
        if let server = serverAddress { connect(server) }
        publishAccounts()
    }

    // MARK: Server and sign-in

    /// Opens a Core; a saved token is refreshed if due, otherwise sign-in is shown.
    func connect(_ input: String) {
        var address = input.trimmingCharacters(in: .whitespacesAndNewlines)
        if !address.contains("://") { address = "https://" + address }
        let saved = address == serverAddress ? TokenStore.load(server: address) : nil
        let created: AetherClient
        do {
            created = try AetherClient(server: address, device: deviceName, token: saved)
        } catch {
            self.error = describe(error)
            phase = .server
            return
        }
        client = created
        serverAddress = address
        UserDefaults.standard.set(address, forKey: "server")
        guard saved != nil else {
            showSignIn()
            return
        }
        // Offline or Core restarting: keep the session and show what loads
        // (offline copies play without the network).
        phase = .ready
        Task {
            do {
                _ = try await created.refreshIfDue(marginSeconds: Self.refreshMargin)
                TokenStore.save(created.token(), server: address)
                scheduleRefresh()
                connectRemote()
                rememberAccount(created)
            } catch let failure {
                if !signedOut(failure) { self.error = describe(failure) }
            }
        }
    }

    func changeServer() {
        pairingTask?.cancel()
        session?.close()
        remote.disconnect()
        client = nil
        phase = .server
    }

    func login(_ login: String, _ password: String) async {
        guard let client else { return }
        do {
            _ = try await client.login(login: login, password: password)
            signedIn()
        } catch ClientError.Unauthorized {
            error = "Identifiant ou mot de passe incorrect"
        } catch {
            self.error = describe(error)
        }
    }

    private func showSignIn() {
        phase = .signIn
        publishAccounts()
        startPairing()
    }

    /// Shows a QR code and a code, and polls until a signed-in device approves it.
    private func startPairing() {
        guard let client else { return }
        pairingTask?.cancel()
        pairingTask = Task {
            while !Task.isCancelled, phase == .signIn {
                let pairing: Pairing
                do {
                    pairing = try await client.startPairing()
                } catch {
                    try? await Task.sleep(for: .seconds(10))
                    continue
                }
                self.pairing = pairing
                let deadline = Date().addingTimeInterval(TimeInterval(pairing.expiresIn))
                do {
                    while !Task.isCancelled, Date() < deadline {
                        try await Task.sleep(for: .seconds(3))
                        if try await client.pollPairing(pairing: pairing) != nil {
                            signedIn()
                            return
                        }
                    }
                } catch {
                    // Expired or already used: a fresh code on the next turn.
                }
            }
        }
    }

    private func signedIn() {
        pairingTask?.cancel()
        pairing = nil
        guard let client, let serverAddress else { return }
        TokenStore.save(client.token(), server: serverAddress)
        scheduleRefresh()
        phase = .ready
        generation += 1
        connectRemote()
        rememberAccount(client)
    }

    func signOut() async {
        guard let client, let serverAddress else { return }
        session?.close()
        remote.disconnect()
        try? await client.logout()
        forgetCurrent()
        // Another account of this device takes over; the sign-in page otherwise.
        if let next = TokenStore.parked(server: serverAddress).first {
            switchAccount(next.id)
        } else {
            BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: Self.refreshTaskIdentifier)
            showSignIn()
        }
    }

    // MARK: Several accounts on this device

    private var accountId: String? {
        get { UserDefaults.standard.string(forKey: "accountId") }
        set { UserDefaults.standard.set(newValue, forKey: "accountId") }
    }

    private var accountName: String? {
        get { UserDefaults.standard.string(forKey: "accountName") }
        set { UserDefaults.standard.set(newValue, forKey: "accountName") }
    }

    private func publishAccounts() {
        var listed: [AccountEntry] = []
        if let id = accountId, client?.token() != nil {
            listed.append(AccountEntry(id: id, name: accountName ?? "", current: true))
        }
        if let serverAddress {
            listed += TokenStore.parked(server: serverAddress).map { AccountEntry(id: $0.id, name: $0.name, current: false) }
        }
        accounts = listed
    }

    /// Names the active account, and drops a copy of it set aside by an earlier sign-in.
    private func rememberAccount(_ signedIn: AetherClient) {
        Task {
            guard let user = try? await signedIn.me(), client === signedIn, let serverAddress else { return }
            accountId = user.id
            accountName = user.displayName ?? user.login
            TokenStore.setParked(TokenStore.parked(server: serverAddress).filter { $0.id != user.id }, server: serverAddress)
            publishAccounts()
        }
    }

    /// Sets the active account aside: it stays signed in, ready to come back.
    private func parkCurrent() {
        guard let serverAddress, let token = client?.token(), let id = accountId else { return }
        let aside = SavedAccount(id: id, name: accountName ?? "", access: token.access, expiresAt: token.expiresAt)
        let others = TokenStore.parked(server: serverAddress).filter { $0.id != id }
        TokenStore.setParked(Array(([aside] + others).prefix(Self.maxParked)), server: serverAddress)
    }

    private func forgetCurrent() {
        if let serverAddress { TokenStore.save(nil, server: serverAddress) }
        accountId = nil
        accountName = nil
    }

    /// Signs in to one more account; the active one is set aside, not signed out.
    func addAccount() {
        guard let serverAddress,
              let fresh = try? AetherClient(server: serverAddress, device: deviceName, token: nil)
        else { return }
        session?.close()
        remote.disconnect()
        parkCurrent()
        forgetCurrent()
        client = fresh
        showSignIn()
    }

    /// Makes an account set aside the active one, without signing in again.
    func switchAccount(_ id: String) {
        guard let serverAddress,
              let target = TokenStore.parked(server: serverAddress).first(where: { $0.id == id }),
              let next = try? AetherClient(server: serverAddress, device: deviceName, token: target.token)
        else { return }
        pairingTask?.cancel()
        pairing = nil
        session?.close()
        remote.disconnect()
        parkCurrent()
        TokenStore.setParked(TokenStore.parked(server: serverAddress).filter { $0.id != id }, server: serverAddress)
        client = next
        TokenStore.save(target.token, server: serverAddress)
        accountId = target.id
        accountName = target.name
        publishAccounts()
        scheduleRefresh()
        phase = .ready
        generation += 1
        connectRemote()
    }

    /// Signs an account set aside out of this device.
    func forgetAccount(_ id: String) {
        guard let serverAddress,
              let target = TokenStore.parked(server: serverAddress).first(where: { $0.id == id })
        else { return }
        TokenStore.setParked(TokenStore.parked(server: serverAddress).filter { $0.id != id }, server: serverAddress)
        publishAccounts()
        let gone = try? AetherClient(server: serverAddress, device: deviceName, token: target.token)
        Task { _ = try? await gone?.logout() }
    }

    /// From a signed-in phone: approves the code a TV (or another device) shows.
    func approve(code: String) async -> Bool {
        guard let client else { return false }
        do {
            try await client.approvePairing(code: code.trimmingCharacters(in: .whitespaces).uppercased())
            return true
        } catch {
            self.error = describe(error)
            return false
        }
    }

    /// `true` when the token was refused: sign-in is shown again.
    @discardableResult
    func signedOut(_ error: Error) -> Bool {
        guard case ClientError.Unauthorized = error, let serverAddress else { return false }
        TokenStore.save(nil, server: serverAddress)
        remote.disconnect()
        showSignIn()
        return true
    }

    /// Reports a failed call, signing out if the token is no longer valid.
    func fail(_ error: Error) {
        if !signedOut(error) { self.error = describe(error) }
    }

    func describe(_ error: Error) -> String {
        switch error {
        case ClientError.Unauthorized: return "Session expirée : reconnectez-vous."
        case ClientError.PairingExpired: return "Ce code a expiré."
        case ClientError.InvalidUrl: return "Adresse de serveur invalide."
        case let ClientError.Server(status, code):
            return code.isEmpty ? "Le serveur a répondu \(status)." : "Le serveur a répondu \(status) (\(code))."
        case let ClientError.Network(detail): return "Serveur injoignable : \(detail)"
        default: return error.localizedDescription
        }
    }

    // MARK: Token refresh

    /// Back in the app: refresh the token if due, reconnect, reload the screens.
    func foreground() {
        guard phase == .ready, let client, let serverAddress else { return }
        guard Date().timeIntervalSince(lastForeground) > 60 else { return }
        lastForeground = Date()
        Task {
            do {
                if try await client.refreshIfDue(marginSeconds: Self.refreshMargin) {
                    TokenStore.save(client.token(), server: serverAddress)
                }
                connectRemote()
                generation += 1
            } catch {
                signedOut(error)
            }
        }
    }

    func refreshInBackground() async {
        scheduleRefresh()
        guard let serverAddress = UserDefaults.standard.string(forKey: "server") else { return }
        // The accounts set aside stay signed in too; one that was revoked leaves the list.
        var kept: [SavedAccount] = []
        for var account in TokenStore.parked(server: serverAddress) {
            guard let other = try? AetherClient(server: serverAddress, device: deviceName, token: account.token) else { continue }
            do {
                _ = try await other.me()
                _ = try await other.refreshIfDue(marginSeconds: Self.refreshMargin)
                if let token = other.token() {
                    account.access = token.access
                    account.expiresAt = token.expiresAt
                }
            } catch ClientError.Unauthorized {
                continue
            } catch {
                // Offline: keep it for the next run.
            }
            kept.append(account)
        }
        TokenStore.setParked(kept, server: serverAddress)
        guard let token = TokenStore.load(server: serverAddress),
              let client = try? AetherClient(server: serverAddress, device: deviceName, token: token)
        else { return }
        if (try? await client.refreshIfDue(marginSeconds: Self.refreshMargin)) == true {
            TokenStore.save(client.token(), server: serverAddress)
        }
    }

    private func scheduleRefresh() {
        let request = BGAppRefreshTaskRequest(identifier: Self.refreshTaskIdentifier)
        request.earliestBeginDate = Date().addingTimeInterval(24 * 3600)
        try? BGTaskScheduler.shared.submit(request)
    }

    // MARK: Images and playback

    /// Absolute URL of a server-relative path (posters), on this server only.
    func absoluteURL(_ path: String?) -> URL? {
        guard let path, let client, let string = try? client.url(path: path) else { return nil }
        return URL(string: string)
    }

    /// The bearer token, sent to this server's own URLs only (images, socket).
    func authorization(for url: URL) -> String? {
        guard let serverAddress, let server = URL(string: serverAddress),
              url.host == server.host, url.port == server.port,
              let token = client?.token()
        else { return nil }
        return "Bearer \(token.access)"
    }

    func play(workId: String, title: String, startAt: Double? = nil) {
        playing = PlayRequest(workId: workId, title: title, startAt: startAt)
    }

    // MARK: Remote control

    private func connectRemote() {
        guard let serverAddress else { return }
        remote.connect(server: serverAddress, device: deviceName) { [weak self] in
            self?.client?.token()?.access
        }
    }

    /// Commands for the player go to the player on screen; `open` starts one.
    private func handle(_ command: RemoteCommand) {
        guard command.action == "open", let workId = command.workId else { return }
        play(workId: workId, title: "", startAt: command.position)
    }

    /// "Resume here": continue locally where the other device is, and stop it there.
    func resumeHere(_ device: RemoteDevice) {
        guard let now = device.playing else { return }
        remote.command(to: device, action: "stop")
        play(workId: now.workId, title: now.title ?? "", startAt: now.position)
    }
}
