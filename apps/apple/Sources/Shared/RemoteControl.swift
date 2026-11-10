import Foundation
import Observation

struct NowPlaying: Equatable {
    let workId: String
    let title: String?
    let position: Double
    let duration: Double?
    let paused: Bool
}

struct RemoteDevice: Identifiable, Equatable {
    let id: String
    let name: String
    /// phone, tv, web or desktop.
    let kind: String
    let playing: NowPlaying?
}

struct RemoteCommand {
    let fromName: String
    /// play, pause, seek, stop or open.
    let action: String
    let position: Double?
    let workId: String?
}

/// This device on the account's remote-control channel (`/api/v1/remote/ws`,
/// see crates/core/src/remote.rs): it lists the other devices and what they
/// play, reports its own playback and receives commands. Reconnects on its own.
@MainActor
@Observable
final class RemoteControl {
    private(set) var devices: [RemoteDevice] = []
    /// `open` (start playing a title); the app shows the player.
    @ObservationIgnored var onCommand: ((RemoteCommand) -> Void)?
    /// play, pause, seek and stop, for the player on screen.
    @ObservationIgnored var onPlayerCommand: ((RemoteCommand) -> Void)?

    @ObservationIgnored private var socket: URLSessionWebSocketTask?
    @ObservationIgnored private var target: (server: String, name: String, token: () -> String?)?
    @ObservationIgnored private var state: [String: Any] = ["type": "state", "playing": NSNull()]
    @ObservationIgnored private var backoff: Double = 2
    @ObservationIgnored private var retry: Task<Void, Never>?
    @ObservationIgnored private var pinger: Task<Void, Never>?

    #if os(tvOS)
    private let kind = "tv"
    #else
    private let kind = "phone"
    #endif

    private let deviceId: String = {
        if let id = UserDefaults.standard.string(forKey: "device_id") { return id }
        let id = UUID().uuidString.lowercased()
        UserDefaults.standard.set(id, forKey: "device_id")
        return id
    }()

    /// `token` is read at every (re)connection, so a refreshed token is picked up.
    func connect(server: String, device: String, token: @escaping () -> String?) {
        target = (server, device, token)
        backoff = 2
        open()
    }

    func disconnect() {
        target = nil
        retry?.cancel()
        pinger?.cancel()
        socket?.cancel(with: .normalClosure, reason: nil)
        socket = nil
        devices = []
    }

    private func open() {
        guard let target, let access = target.token(),
              var components = URLComponents(string: target.server)
        else { return }
        components.scheme = components.scheme == "https" ? "wss" : "ws"
        components.path = "/api/v1/remote/ws"
        components.query = nil
        guard let url = components.url else { return }
        var request = URLRequest(url: url)
        request.setValue("Bearer \(access)", forHTTPHeaderField: "Authorization")
        socket?.cancel(with: .goingAway, reason: nil)
        let task = URLSession.shared.webSocketTask(with: request)
        socket = task
        task.resume()
        send(["type": "hello", "deviceId": deviceId, "name": target.name, "kind": kind])
        send(state)
        receive(on: task)
        pinger?.cancel()
        pinger = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(30))
                self?.socket?.sendPing { _ in }
            }
        }
    }

    private func receive(on task: URLSessionWebSocketTask) {
        task.receive { [weak self] result in
            Task { @MainActor in
                guard let self, task === self.socket else { return }
                switch result {
                case .success(let message):
                    self.backoff = 2
                    if case .string(let text) = message { self.handle(text) }
                    self.receive(on: task)
                case .failure:
                    self.reconnect()
                }
            }
        }
    }

    private func reconnect() {
        guard target != nil else { return }
        retry?.cancel()
        let delay = backoff
        backoff = min(backoff * 2, 30)
        retry = Task { [weak self] in
            try? await Task.sleep(for: .seconds(delay))
            guard !Task.isCancelled else { return }
            self?.open()
        }
    }

    private func handle(_ text: String) {
        guard let data = text.data(using: .utf8),
              let message = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return }
        switch message["type"] as? String {
        case "devices":
            let list = message["devices"] as? [[String: Any]] ?? []
            devices = list.map { device in
                let playing = (device["playing"] as? [String: Any]).flatMap { p -> NowPlaying? in
                    guard let workId = p["workId"] as? String else { return nil }
                    return NowPlaying(
                        workId: workId,
                        title: p["title"] as? String,
                        position: (p["position"] as? NSNumber)?.doubleValue ?? 0,
                        duration: (p["duration"] as? NSNumber)?.doubleValue,
                        paused: p["paused"] as? Bool ?? false
                    )
                }
                return RemoteDevice(
                    id: device["deviceId"] as? String ?? "",
                    name: device["name"] as? String ?? "",
                    kind: device["kind"] as? String ?? "",
                    playing: playing
                )
            }
        case "command":
            let command = RemoteCommand(
                fromName: message["fromName"] as? String ?? "",
                action: message["action"] as? String ?? "",
                position: (message["position"] as? NSNumber)?.doubleValue,
                workId: message["workId"] as? String
            )
            if command.action == "open" { onCommand?(command) } else { onPlayerCommand?(command) }
        default:
            break
        }
    }

    /// What this device plays now (`nil` when nothing).
    func report(_ playing: NowPlaying?) {
        var now: Any = NSNull()
        if let playing {
            now = [
                "workId": playing.workId,
                "title": playing.title as Any? ?? NSNull(),
                "position": playing.position,
                "duration": playing.duration as Any? ?? NSNull(),
                "paused": playing.paused,
            ] as [String: Any]
        }
        state = ["type": "state", "playing": now]
        send(state)
    }

    func command(to device: RemoteDevice, action: String, position: Double? = nil, workId: String? = nil) {
        var message: [String: Any] = ["type": "command", "to": device.id, "action": action]
        if let position { message["position"] = max(0, position) }
        if let workId { message["workId"] = workId }
        send(message)
    }

    private func send(_ message: [String: Any]) {
        guard let socket,
              let data = try? JSONSerialization.data(withJSONObject: message),
              let text = String(data: data, encoding: .utf8)
        else { return }
        socket.send(.string(text)) { _ in }
    }
}
