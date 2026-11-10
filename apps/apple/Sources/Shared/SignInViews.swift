import SwiftUI

struct ServerView: View {
    @Environment(AppModel.self) private var model
    @State private var address = ""

    var body: some View {
        VStack(spacing: 24) {
            Text("Votre serveur Aether").font(.title.bold())
            TextField("Adresse, par exemple https://media.exemple.fr", text: $address)
                .textContentType(.URL)
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .onSubmit(connect)
                #if os(iOS)
                .textFieldStyle(.roundedBorder)
                #endif
            Button("Continuer", action: connect)
                .buttonStyle(.borderedProminent)
                .disabled(address.trimmingCharacters(in: .whitespaces).isEmpty)
        }
        .frame(maxWidth: 640)
        .padding()
        .onAppear { address = model.serverAddress ?? "" }
    }

    private func connect() {
        model.connect(address)
    }
}

/// Password sign-in, or a QR code / code approved from a signed-in device
/// (the comfortable way on a TV).
struct SignInView: View {
    @Environment(AppModel.self) private var model
    @State private var login = ""
    @State private var password = ""
    @State private var busy = false

    var body: some View {
        ScrollView {
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .top, spacing: 80) {
                    form
                    pairing
                }
                VStack(spacing: 40) {
                    form
                    pairing
                }
            }
            .padding()
            .frame(maxWidth: .infinity)
        }
    }

    private var form: some View {
        VStack(spacing: 20) {
            Text("Connexion").font(.title.bold())
            TextField("Identifiant", text: $login)
                .textContentType(.username)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
            SecureField("Mot de passe", text: $password)
                .textContentType(.password)
                .onSubmit(signIn)
            Button(action: signIn) {
                if busy { ProgressView() } else { Text("Se connecter") }
            }
            .buttonStyle(.borderedProminent)
            .disabled(login.isEmpty || password.isEmpty || busy)
            Button("Changer de serveur") { model.changeServer() }
        }
        #if os(iOS)
        .textFieldStyle(.roundedBorder)
        #endif
        .frame(maxWidth: 480)
    }

    @ViewBuilder
    private var pairing: some View {
        VStack(spacing: 16) {
            Text("Ou scannez ce code avec un appareil déjà connecté")
                .font(.headline)
                .multilineTextAlignment(.center)
            if let pairing = model.pairing {
                QRCodeView(width: Int(pairing.qrWidth), modules: pairing.qrModules)
                    .frame(width: qrSize, height: qrSize)
                Text("Code : \(pairing.code)")
                    .font(.title2.monospaced())
            } else {
                ProgressView().frame(width: qrSize, height: qrSize)
            }
        }
        .frame(maxWidth: 480)
    }

    private var qrSize: CGFloat {
        #if os(tvOS)
        360
        #else
        220
        #endif
    }

    private func signIn() {
        guard !login.isEmpty, !password.isEmpty else { return }
        busy = true
        Task {
            await model.login(login, password)
            busy = false
        }
    }
}

/// Draws the Core's QR modules (row-major, `true` = dark) with a quiet zone.
struct QRCodeView: View {
    let width: Int
    let modules: [Bool]

    var body: some View {
        Canvas { context, size in
            let quiet = 4
            let cell = min(size.width, size.height) / CGFloat(width + 2 * quiet)
            context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(.white))
            var dark = Path()
            for (index, on) in modules.enumerated() where on {
                let x = CGFloat(index % width + quiet) * cell
                let y = CGFloat(index / width + quiet) * cell
                dark.addRect(CGRect(x: x, y: y, width: cell, height: cell))
            }
            context.fill(dark, with: .color(.black))
        }
        .accessibilityLabel("Code QR de connexion")
    }
}
