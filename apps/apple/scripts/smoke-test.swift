// Calls the Rust core through its Swift bindings, without a server:
// construction, URLs, the token, and how errors reach Swift.

let client = try AetherClient(
    server: "http://127.0.0.1:9/app/?x=1",
    device: "Swift smoke test",
    token: Token(access: "token", expiresAt: 0)
)
let poster = try client.url(path: "/api/v1/assets/a/image?v=1")
precondition(poster == "http://127.0.0.1:9/api/v1/assets/a/image?v=1", poster)
precondition(client.token() == Token(access: "token", expiresAt: 0))

do {
    _ = try AetherClient(server: "ftp://media.example", device: "d", token: nil)
    fatalError("an ftp address was accepted")
} catch ClientError.InvalidUrl {}

// Nothing listens on port 9: a network error, not a crash.
do {
    _ = try await client.libraries()
    fatalError("port 9 answered")
} catch ClientError.Network {}

// No token: sign in again.
let signedOut = try AetherClient(server: "http://127.0.0.1:9", device: "d", token: nil)
do {
    _ = try await signedOut.continueWatching()
    fatalError("no token, yet no error")
} catch ClientError.Unauthorized {}

print("Swift bindings OK")
