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

/// Session and navigation state. Every Core call goes through the Rust core
/// (`AetherClient`); only the token's storage (Keychain), the remote-control
/// socket and the players are native.
@MainActor
@Observable
final class AppModel {
    static let refreshTaskIdentifier = "app.aether.refresh"
    /// Refresh when the token expires within 7 days (it lasts 30).
    private static let refreshMargin: Int64 = 7 * 24 * 3600

    private(set) var phase: Phase = .server
    private(set) var client: AetherClient?
    private(set) var serverAddress: String?
    private(set) var pairing: Pairing?
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
    }

    func signOut() async {
        guard let client, let serverAddress else { return }
        session?.close()
        remote.disconnect()
        try? await client.logout()
        TokenStore.save(nil, server: serverAddress)
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: Self.refreshTaskIdentifier)
        showSignIn()
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
        guard let serverAddress = UserDefaults.standard.string(forKey: "server"),
              let token = TokenStore.load(server: serverAddress),
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
