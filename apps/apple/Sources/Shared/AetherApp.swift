import AVFoundation
import SwiftUI
import UIKit

@main
struct AetherApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var model = AppModel()
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { model.foreground() }
        }
        // Daily token refresh while the app is closed: the Core expires a
        // device token unused for 7 days.
        .backgroundTask(.appRefresh(AppModel.refreshTaskIdentifier)) {
            await model.refreshInBackground()
        }
    }
}

final class AppDelegate: NSObject, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        // Playback continues in Picture in Picture, over AirPlay and with the
        // silent switch on.
        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .moviePlayback)
        #if os(iOS)
        DownloadStore.shared.restore()
        #endif
        return true
    }

    #if os(iOS)
    /// Offline copies progress while the app is suspended; iOS wakes it to
    /// deliver their events.
    func application(
        _ application: UIApplication,
        handleEventsForBackgroundURLSession identifier: String,
        completionHandler: @escaping () -> Void
    ) {
        DownloadStore.shared.backgroundEventsDone = completionHandler
    }
    #endif
}

/// Server address, sign-in, then the signed-in tabs.
struct RootView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        Group {
            switch model.phase {
            case .server: ServerView()
            case .signIn: SignInView()
            case .ready: MainView()
            }
        }
        .fullScreenCover(item: $model.playing) { request in
            PlayerScreen(request: request)
                .environment(model)
        }
        .alert(
            "Aether",
            isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } }),
            actions: { Button("OK") { model.error = nil } },
            message: { Text(model.error ?? "") }
        )
    }
}

struct MainView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        TabView {
            NavigationStack { HomeView() }
                .tabItem { Label("Accueil", systemImage: "house") }
            NavigationStack { LibrariesView() }
                .tabItem { Label("Bibliothèques", systemImage: "square.grid.2x2") }
            NavigationStack { SearchView() }
                .tabItem { Label("Rechercher", systemImage: "magnifyingglass") }
            #if os(iOS)
            NavigationStack { DownloadsView() }
                .tabItem { Label("Téléchargements", systemImage: "arrow.down.circle") }
            #endif
            NavigationStack { DevicesView() }
                .tabItem { Label("Appareils", systemImage: "tv.and.mediabox") }
            NavigationStack { SettingsView() }
                .tabItem { Label("Réglages", systemImage: "gearshape") }
        }
    }
}
