use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, Response, StatusCode},
    routing::post,
};
use bytes::Bytes;
use media_contracts::{ids::NodeId, protocol::*};
use media_node::server::send_signed_http_request;

async fn reply(
    headers: HeaderMap,
    axum::extract::Path(mode): axum::extract::Path<String>,
) -> Response<Body> {
    let now = unix_now();
    let nonce = random_nonce_hex();
    let original = b"{\"authorized\":false}";
    let body = if mode == "tampered" {
        b"{\"authorized\":true}".as_slice()
    } else {
        original.as_slice()
    };
    let mut response = Response::new(Body::from(body));
    if mode == "redirect" {
        *response.status_mut() = StatusCode::TEMPORARY_REDIRECT;
        response
            .headers_mut()
            .insert("location", "http://169.254.169.254/".parse().unwrap());
    }
    if mode != "unsigned" {
        let signature = sign_internal_response(
            "test-secret",
            headers[HDR_NONCE].to_str().unwrap(),
            response.status().as_u16(),
            "test-node",
            "test-key",
            now,
            &nonce,
            &sha256_hex(original),
        );
        for (key, value) in [
            (HDR_RES_TIMESTAMP, now.to_string()),
            (HDR_RES_NONCE, nonce),
            (HDR_RES_SIGNATURE, signature),
        ] {
            response.headers_mut().insert(key, value.parse().unwrap());
        }
    }
    response
}

#[tokio::test]
async fn mandatory_response_proof_binds_bytes_and_rejects_redirects() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/{mode}", post(reply)))
            .await
            .unwrap()
    });
    for mode in ["good", "unsigned", "tampered", "redirect"] {
        let result = send_signed_http_request(
            &url,
            "core-to-node",
            Method::POST,
            &format!("/{mode}"),
            &NodeId::new("test-node"),
            "test-key",
            "test-secret",
            None,
            Bytes::new(),
        )
        .await;
        assert_eq!(result.is_ok(), mode == "good", "{mode}");
    }
    server.abort();
}

#[tokio::test]
async fn https_never_sends_plaintext_http() {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("https://{}", listener.local_addr().unwrap());
    let capture = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 5];
        socket.read_exact(&mut bytes).await.unwrap();
        bytes
    });
    assert!(media_node::transport::connect(&url).await.is_err());
    let bytes = capture.await.unwrap();
    assert_eq!(bytes[0], 0x16, "TLS handshake record required");
    assert_eq!(bytes[1], 3);
}

#[tokio::test]
async fn missing_control_response_body_has_a_deadline() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n")
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    });
    let result = send_signed_http_request(
        &url,
        "core-to-node",
        Method::POST,
        "/missing",
        &NodeId::new("test-node"),
        "test-key",
        "test-secret",
        None,
        Bytes::new(),
    )
    .await;
    assert_eq!(result.err().unwrap().kind(), std::io::ErrorKind::TimedOut);
    server.abort();
}

/// Generate ephemeral certificates with scripts/generate_p2_tls_tests.py first.
#[tokio::test]
#[ignore = "requires ephemeral certificate directory and MEDIA_INTERNAL_CA_FILE"]
async fn real_https_certificate_chain_hostname_and_expiry() {
    use tokio_native_tls::{TlsAcceptor, native_tls};
    let directory = std::path::PathBuf::from(
        std::env::var_os("MEDIA_TLS_TEST_DIR").expect("certificate directory"),
    );
    for (certificate, hostname, expected) in [
        ("valid", "localhost", true),
        ("valid", "127.0.0.1", false),
        ("untrusted", "localhost", false),
        ("expired", "localhost", false),
    ] {
        let identity = native_tls::Identity::from_pkcs12(
            &std::fs::read(directory.join(format!("{certificate}.p12"))).unwrap(),
            "",
        )
        .unwrap();
        let acceptor = TlsAcceptor::from(native_tls::TlsAcceptor::new(identity).unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let _ = acceptor.accept(socket).await;
        });
        let result = media_node::transport::connect(&format!("https://{hostname}:{port}")).await;
        assert_eq!(result.is_ok(), expected, "{certificate} {hostname}");
        server.await.unwrap();
    }
}
