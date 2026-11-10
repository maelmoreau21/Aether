import AVFoundation
import AVKit
import Combine
import SwiftUI

enum PlayerError: LocalizedError {
    case unavailable
    case unreadable

    var errorDescription: String? {
        switch self {
        case .unavailable: return "Ce titre n’est pas disponible pour le moment."
        case .unreadable: return "Ce titre n’a pas pu être lu."
        }
    }
}

/// One playback: the Core's HLS (or an offline copy) in AVPlayer, the
/// position saved every 15 s, the state reported on the remote-control
/// channel. It outlives the player screen while Picture in Picture is on.
@MainActor
@Observable
final class PlaybackSession: NSObject {
    let request: PlayRequest
    private(set) var ready = false
    let player = AVPlayer()
    let controller = AVPlayerViewController()
    @ObservationIgnored private(set) var pictureInPicture = false
    @ObservationIgnored private weak var model: AppModel?
    @ObservationIgnored private var title: String
    @ObservationIgnored private var revision: Int64 = 0
    @ObservationIgnored private var timeObserver: Any?
    @ObservationIgnored private var statusObservation: NSKeyValueObservation?
    @ObservationIgnored private var endObserver: NSObjectProtocol?
    @ObservationIgnored private var lastSave = Date()
    @ObservationIgnored private var lastReport = Date.distantPast
    @ObservationIgnored private var stopped = false

    init(request: PlayRequest, model: AppModel) {
        self.request = request
        self.model = model
        title = request.title
        super.init()
        controller.player = player
        controller.delegate = self
        controller.allowsPictureInPicturePlayback = true
        #if os(iOS)
        controller.canStartPictureInPictureAutomaticallyFromInline = true
        #endif
        // AirPlay: the HLS URL needs no credential, the Apple TV fetches it itself.
        player.allowsExternalPlayback = true
    }

    func start() async throws {
        guard let model, let client = model.client else { throw PlayerError.unavailable }
        var source: URL?
        #if os(iOS)
        source = DownloadStore.shared.localURL(workId: request.workId)
        #endif
        if source == nil || title.isEmpty {
            do {
                let media = try await client.media(id: request.workId)
                if title.isEmpty { title = media.title }
                if source == nil {
                    guard media.available else { throw PlayerError.unavailable }
                    source = URL(string: try await client.playbackUrl(assetId: media.id).url)
                }
            } catch {
                // An offline copy plays without the network.
                if source == nil { throw error }
            }
        }
        guard let source, !stopped else { throw PlayerError.unreadable }

        var start = request.startAt
        if let saved = try? await client.progress(workId: request.workId) {
            revision = saved.revision
            if start == nil, !saved.watched, saved.position > 30, saved.position < saved.duration - 30 {
                start = saved.position
            }
        }

        let item = AVPlayerItem(asset: AVURLAsset(url: source))
        item.externalMetadata = [metadata(.commonIdentifierTitle, title)]
        player.replaceCurrentItem(with: item)
        for await status in item.publisher(for: \.status).values {
            if status == .readyToPlay { break }
            if status == .failed { throw item.error ?? PlayerError.unreadable }
        }
        if let start {
            _ = await player.seek(to: CMTime(seconds: start, preferredTimescale: 600))
        }
        guard !stopped else { return }
        observe(item)
        ready = true
        player.play()
    }

    private func observe(_ item: AVPlayerItem) {
        timeObserver = player.addPeriodicTimeObserver(
            forInterval: CMTime(seconds: 1, preferredTimescale: 1), queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        statusObservation = player.observe(\.timeControlStatus) { [weak self] player, _ in
            let paused = player.timeControlStatus == .paused
            Task { @MainActor in
                self?.report()
                if paused { self?.save() }
            }
        }
        endObserver = NotificationCenter.default.addObserver(
            forName: AVPlayerItem.didPlayToEndTimeNotification, object: item, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.save(watched: true)
                self?.close()
            }
        }
        model?.remote.onPlayerCommand = { [weak self] command in self?.handle(command) }
    }

    private func tick() {
        let now = Date()
        if now.timeIntervalSince(lastReport) >= 5 { report() }
        if player.timeControlStatus == .playing, now.timeIntervalSince(lastSave) >= 15 { save() }
    }

    private var position: Double {
        let seconds = player.currentTime().seconds
        return seconds.isFinite ? seconds : 0
    }

    private var duration: Double {
        let seconds = player.currentItem?.duration.seconds ?? 0
        return seconds.isFinite ? seconds : 0
    }

    private func report() {
        guard ready, !stopped else { return }
        lastReport = Date()
        model?.remote.report(NowPlaying(
            workId: request.workId,
            title: title.isEmpty ? nil : title,
            position: position,
            duration: duration > 0 ? duration : nil,
            paused: player.timeControlStatus == .paused
        ))
    }

    /// Without the network (offline copy) the position is simply not saved.
    private func save(watched: Bool = false) {
        guard ready, let client = model?.client, duration > 0 else { return }
        lastSave = Date()
        let at = min(position, duration)
        let done = watched || at >= duration * 0.95
        let workId = request.workId
        let expected = revision
        Task {
            if let saved = try? await client.saveProgress(
                workId: workId, position: at, duration: duration,
                watched: done, expectedRevision: expected
            ) {
                revision = saved.revision
            }
        }
    }

    private func handle(_ command: RemoteCommand) {
        switch command.action {
        case "play": player.play()
        case "pause": player.pause()
        case "seek":
            if let target = command.position {
                player.seek(to: CMTime(seconds: max(0, target), preferredTimescale: 600))
            }
        case "stop": close()
        default: break
        }
    }

    /// Leaves the player: saves the position and frees the session.
    func close() {
        stop()
        if model?.playing?.id == request.id { model?.playing = nil }
    }

    func stop() {
        guard !stopped else { return }
        save()
        stopped = true
        ready = false
        model?.remote.report(nil)
        model?.remote.onPlayerCommand = nil
        if let timeObserver { player.removeTimeObserver(timeObserver) }
        if let endObserver { NotificationCenter.default.removeObserver(endObserver) }
        statusObservation = nil
        player.pause()
        player.replaceCurrentItem(with: nil)
        if model?.session === self { model?.session = nil }
    }

    private func metadata(_ identifier: AVMetadataIdentifier, _ value: String) -> AVMetadataItem {
        let item = AVMutableMetadataItem()
        item.identifier = identifier
        item.value = value as NSString
        item.extendedLanguageTag = "und"
        return item
    }
}

extension PlaybackSession: AVPlayerViewControllerDelegate {
    // Picture in Picture: the player screen closes, the session keeps playing,
    // and comes back full screen when the person restores it.
    nonisolated func playerViewControllerDidStartPictureInPicture(_ playerViewController: AVPlayerViewController) {
        MainActor.assumeIsolated {
            pictureInPicture = true
            if model?.playing?.id == request.id { model?.playing = nil }
        }
    }

    nonisolated func playerViewController(
        _ playerViewController: AVPlayerViewController,
        restoreUserInterfaceForPictureInPictureStopWithCompletionHandler completionHandler: @escaping (Bool) -> Void
    ) {
        MainActor.assumeIsolated {
            model?.playing = request
            completionHandler(true)
        }
    }

    nonisolated func playerViewControllerDidStopPictureInPicture(_ playerViewController: AVPlayerViewController) {
        MainActor.assumeIsolated {
            pictureInPicture = false
            // Closed from the floating window, not restored: playback ends.
            if model?.playing?.id != request.id { stop() }
        }
    }
}

struct PlayerScreen: View {
    @Environment(AppModel.self) private var model
    let request: PlayRequest
    @State private var failure: String?

    var body: some View {
        ZStack(alignment: .topLeading) {
            Color.black.ignoresSafeArea()
            if let session = model.session, session.request.id == request.id, session.ready {
                PlayerControllerView(controller: session.controller).ignoresSafeArea()
            } else if let failure {
                VStack(spacing: 24) {
                    Text(failure).multilineTextAlignment(.center)
                    Button("Fermer", action: close)
                }
                .foregroundStyle(.white)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ProgressView().tint(.white).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            #if os(iOS)
            Button(action: close) {
                Image(systemName: "xmark")
                    .font(.body.bold())
                    .padding(10)
                    .background(.ultraThinMaterial, in: Circle())
            }
            .accessibilityLabel("Fermer le lecteur")
            .padding()
            #endif
        }
        .task { await open() }
        #if os(tvOS)
        .onExitCommand(perform: close)
        #endif
    }

    private func open() async {
        // Restored from Picture in Picture: the session is already playing.
        if model.session?.request.id == request.id { return }
        model.session?.stop()
        let session = PlaybackSession(request: request, model: model)
        model.session = session
        do {
            try await session.start()
        } catch {
            session.stop()
            if !model.signedOut(error) { failure = model.describe(error) }
        }
    }

    private func close() {
        if let session = model.session, session.request.id == request.id {
            session.close()
        } else {
            model.playing = nil
        }
    }
}

struct PlayerControllerView: UIViewControllerRepresentable {
    let controller: AVPlayerViewController

    func makeUIViewController(context: Context) -> AVPlayerViewController { controller }
    func updateUIViewController(_ controller: AVPlayerViewController, context: Context) {}
}
