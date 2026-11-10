import SwiftUI

/// The account's other devices: what they play, a few remote buttons, and
/// "resume here" to continue on this device at the same position.
struct DevicesView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        List {
            if model.remote.devices.isEmpty {
                Text("Aucun autre appareil connecté. Ouvrez Aether sur un autre appareil du même compte.")
                    .foregroundStyle(.secondary)
            }
            ForEach(model.remote.devices) { device in
                DeviceRow(device: device)
            }
        }
        .navigationTitle("Appareils")
    }
}

struct DeviceRow: View {
    @Environment(AppModel.self) private var model
    let device: RemoteDevice

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label(device.name, systemImage: icon).font(.headline)
            if let playing = device.playing {
                Text("\(playing.title ?? "Lecture en cours") · \(clock(playing.position))\(playing.paused ? " (pause)" : "")")
                    .foregroundStyle(.secondary)
                HStack(spacing: 12) {
                    control("gobackward.10", "Reculer de 10 s") {
                        model.remote.command(to: device, action: "seek", position: playing.position - 10)
                    }
                    control(playing.paused ? "play.fill" : "pause.fill", playing.paused ? "Lecture" : "Pause") {
                        model.remote.command(to: device, action: playing.paused ? "play" : "pause")
                    }
                    control("goforward.30", "Avancer de 30 s") {
                        model.remote.command(to: device, action: "seek", position: playing.position + 30)
                    }
                    control("stop.fill", "Arrêter") {
                        model.remote.command(to: device, action: "stop")
                    }
                    Button("Reprendre ici") { model.resumeHere(device) }
                        .buttonStyle(.borderedProminent)
                }
            } else {
                Text("Rien en lecture").foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 6)
    }

    private var icon: String {
        switch device.kind {
        case "tv": return "tv"
        case "web": return "globe"
        case "desktop": return "desktopcomputer"
        default: return "iphone"
        }
    }

    private func control(_ symbol: String, _ label: String, action: @escaping () -> Void) -> some View {
        Button(action: action) { Image(systemName: symbol) }
            .buttonStyle(.bordered)
            .accessibilityLabel(label)
    }
}

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var code = ""
    @State private var approved = false

    var body: some View {
        Form {
            Section("Serveur") {
                Text(model.serverAddress ?? "")
                Button("Changer de serveur") { model.changeServer() }
                Button("Se déconnecter", role: .destructive) {
                    Task { await model.signOut() }
                }
            }
            #if os(iOS)
            Section {
                TextField("Code affiché sur la TV", text: $code)
                    .textInputAutocapitalization(.characters)
                    .autocorrectionDisabled()
                Button("Connecter cet appareil") {
                    Task {
                        approved = await model.approve(code: code)
                        if approved { code = "" }
                    }
                }
                .disabled(code.trimmingCharacters(in: .whitespaces).isEmpty)
                if approved { Label("Appareil connecté", systemImage: "checkmark.circle") }
            } header: {
                Text("Connecter une TV")
            } footer: {
                Text("Saisissez le code affiché par Aether sur une TV ou un autre appareil pour le connecter à votre compte.")
            }
            #endif
            Section("Cet appareil") {
                Text(model.deviceName)
            }
        }
        .navigationTitle("Réglages")
    }
}
