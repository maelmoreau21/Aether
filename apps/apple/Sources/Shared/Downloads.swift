#if os(iOS)
import AVFoundation
import Observation
import SwiftUI

/// One offline copy (a film or an episode).
struct OfflineCopy: Codable, Identifiable, Equatable {
    enum State: String, Codable {
        case downloading, done, failed
    }

    let workId: String
    var title: String
    var state: State
    var fraction: Double
    /// The `.movpkg`, relative to the app's home directory (which moves
    /// between launches and updates).
    var relativePath: String?
    /// One automatic retry with a fresh URL (an HLS grant lasts 12 h).
    var renewed = false
    var id: String { workId }
}

/// Offline HLS copies with `AVAssetDownloadURLSession`: a background session
/// that keeps downloading while the app is suspended, every audio track and
/// subtitle included. The copy plays without the network through
/// `localURL(workId:)`.
@MainActor
@Observable
final class DownloadStore: NSObject {
    static let shared = DownloadStore()
    private static let sessionIdentifier = "app.aether.downloads"
    private static let storageKey = "offline_copies"

    private(set) var copies: [OfflineCopy] = []
    /// Set by the app delegate when iOS wakes the app for this session.
    @ObservationIgnored var backgroundEventsDone: (() -> Void)?
    /// The signed-in client, to get fresh HLS URLs.
    @ObservationIgnored var client: () -> AetherClient? = { nil }
    @ObservationIgnored private var session: AVAssetDownloadURLSession?
    @ObservationIgnored private var tasks: [String: AVAssetDownloadTask] = [:]
    @ObservationIgnored private var observations: [String: NSKeyValueObservation] = [:]

    /// Reattaches the background session and the downloads in flight.
    func restore() {
        guard session == nil else { return }
        if let data = UserDefaults.standard.data(forKey: Self.storageKey),
           let saved = try? JSONDecoder().decode([OfflineCopy].self, from: data) {
            copies = saved
        }
        let configuration = URLSessionConfiguration.background(withIdentifier: Self.sessionIdentifier)
        configuration.sessionSendsLaunchEvents = true
        configuration.isDiscretionary = false
        let session = AVAssetDownloadURLSession(
            configuration: configuration, assetDownloadDelegate: self, delegateQueue: .main)
        self.session = session
        session.getAllTasks { tasks in
            let live = tasks.compactMap { $0 as? AVAssetDownloadTask }
            Task { @MainActor in
                for task in live {
                    guard let id = task.taskDescription else { continue }
                    self.track(task, workId: id)
                }
                // Interrupted (app force-quit): offer a retry.
                for copy in self.copies where copy.state == .downloading && self.tasks[copy.workId] == nil {
                    self.update(copy.workId) { $0.state = .failed }
                }
            }
        }
    }

    func copy(_ workId: String) -> OfflineCopy? {
        copies.first { $0.workId == workId }
    }

    /// The finished copy, playable without the network.
    func localURL(workId: String) -> URL? {
        guard let copy = copy(workId), copy.state == .done, let path = copy.relativePath else { return nil }
        let url = URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent(path)
        return FileManager.default.fileExists(atPath: url.path) ? url : nil
    }

    /// Copies a film or an episode: original quality, every audio track and subtitle.
    func download(workId: String, title: String) async throws {
        guard let client = client() else { return }
        let media = try await client.media(id: workId)
        guard media.available else { throw PlayerError.unavailable }
        let grant = try await client.playbackUrl(assetId: media.id)
        guard let url = URL(string: grant.url) else { throw PlayerError.unreadable }
        if copy(workId) == nil {
            copies.append(OfflineCopy(workId: workId, title: title, state: .downloading, fraction: 0))
        }
        update(workId) {
            $0.state = .downloading
            $0.fraction = 0
        }
        try await begin(workId: workId, title: title, url: url)
    }

    private func begin(workId: String, title: String, url: URL) async throws {
        guard let session else { throw PlayerError.unreadable }
        let asset = AVURLAsset(url: url)
        let configuration = AVAssetDownloadConfiguration(asset: asset, title: title)
        configuration.primaryContentConfiguration.mediaSelections = try await asset.load(.allMediaSelections)
        let task = session.makeAssetDownloadTask(downloadConfiguration: configuration)
        task.taskDescription = workId
        track(task, workId: workId)
        task.resume()
    }

    private func track(_ task: AVAssetDownloadTask, workId: String) {
        tasks[workId] = task
        observations[workId] = task.progress.observe(\.fractionCompleted) { [weak self] progress, _ in
            let fraction = progress.fractionCompleted
            Task { @MainActor in
                guard let self, self.copy(workId)?.state == .downloading else { return }
                // Saved sparingly: the list redraws, the defaults are written at the end.
                self.copies = self.copies.map { copy in
                    var copy = copy
                    if copy.workId == workId { copy.fraction = fraction }
                    return copy
                }
            }
        }
    }

    func delete(_ workId: String) {
        tasks[workId]?.cancel()
        tasks[workId] = nil
        observations[workId] = nil
        if let path = copy(workId)?.relativePath {
            try? FileManager.default.removeItem(
                at: URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent(path))
        }
        copies.removeAll { $0.workId == workId }
        persist()
    }

    private func update(_ workId: String, _ change: (inout OfflineCopy) -> Void) {
        guard let index = copies.firstIndex(where: { $0.workId == workId }) else { return }
        change(&copies[index])
        persist()
    }

    private func persist() {
        if let data = try? JSONEncoder().encode(copies) {
            UserDefaults.standard.set(data, forKey: Self.storageKey)
        }
    }

    private func finished(_ workId: String, error: Error?) {
        tasks[workId] = nil
        observations[workId] = nil
        guard let copy = copy(workId) else { return }
        if error == nil {
            update(workId) {
                $0.state = .done
                $0.fraction = 1
            }
            return
        }
        if (error as? URLError)?.code == .cancelled { return }
        // Most failures of a long download are an expired 12 h grant: one
        // retry with a fresh URL.
        guard !copy.renewed else {
            update(workId) { $0.state = .failed }
            return
        }
        update(workId) { $0.renewed = true }
        Task {
            do {
                try await download(workId: workId, title: copy.title)
            } catch {
                update(workId) { $0.state = .failed }
            }
        }
    }

    private func relative(_ location: URL) -> String {
        let full = location.absoluteURL.resolvingSymlinksInPath().path
        let home = URL(fileURLWithPath: NSHomeDirectory()).resolvingSymlinksInPath().path
        return full.hasPrefix(home + "/") ? String(full.dropFirst(home.count + 1)) : location.relativePath
    }
}

extension DownloadStore: AVAssetDownloadDelegate {
    nonisolated func urlSession(
        _ session: URLSession, assetDownloadTask: AVAssetDownloadTask, willDownloadTo location: URL
    ) {
        let workId = assetDownloadTask.taskDescription
        MainActor.assumeIsolated {
            guard let workId else { return }
            let path = relative(location)
            update(workId) { $0.relativePath = path }
        }
    }

    nonisolated func urlSession(
        _ session: URLSession, assetDownloadTask: AVAssetDownloadTask, didFinishDownloadingTo location: URL
    ) {
        let workId = assetDownloadTask.taskDescription
        MainActor.assumeIsolated {
            guard let workId else { return }
            let path = relative(location)
            update(workId) { $0.relativePath = path }
        }
    }

    nonisolated func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        let workId = task.taskDescription
        MainActor.assumeIsolated {
            guard let workId else { return }
            finished(workId, error: error)
        }
    }

    nonisolated func urlSessionDidFinishEvents(forBackgroundURLSession session: URLSession) {
        MainActor.assumeIsolated {
            backgroundEventsDone?()
            backgroundEventsDone = nil
        }
    }
}

/// Download, progress, done or retry, for one film or episode.
struct DownloadButton: View {
    @Environment(AppModel.self) private var model
    let workId: String
    let title: String
    private var store: DownloadStore { .shared }

    var body: some View {
        Group {
            switch store.copy(workId)?.state {
            case .downloading:
                Text("\(Int((store.copy(workId)?.fraction ?? 0) * 100)) %")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            case .done:
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(.green)
                    .accessibilityLabel("Téléchargé")
            case .failed:
                Button(action: start) { Image(systemName: "exclamationmark.arrow.circlepath") }
                    .accessibilityLabel("Réessayer le téléchargement")
            case nil:
                Button(action: start) { Image(systemName: "arrow.down.circle") }
                    .accessibilityLabel("Télécharger")
            }
        }
        .font(.title2)
        .buttonStyle(.borderless)
    }

    private func start() {
        Task {
            do {
                try await store.download(workId: workId, title: title)
            } catch {
                model.fail(error)
            }
        }
    }
}

struct DownloadsView: View {
    @Environment(AppModel.self) private var model
    private var store: DownloadStore { .shared }

    var body: some View {
        List {
            if store.copies.isEmpty {
                Text("Les films et épisodes téléchargés se lisent ici, même sans réseau.")
                    .foregroundStyle(.secondary)
            }
            ForEach(store.copies) { copy in
                Button {
                    if copy.state == .done { model.play(workId: copy.workId, title: copy.title) }
                } label: {
                    HStack {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(copy.title).foregroundStyle(.primary)
                            switch copy.state {
                            case .downloading: ProgressView(value: copy.fraction)
                            case .done: Text("Disponible hors ligne").font(.caption).foregroundStyle(.secondary)
                            case .failed: Text("Échec du téléchargement").font(.caption).foregroundStyle(.red)
                            }
                        }
                        Spacer()
                        DownloadButton(workId: copy.workId, title: copy.title)
                    }
                }
                .swipeActions {
                    Button("Supprimer", role: .destructive) { store.delete(copy.workId) }
                }
            }
        }
        .navigationTitle("Téléchargements")
    }
}
#endif
