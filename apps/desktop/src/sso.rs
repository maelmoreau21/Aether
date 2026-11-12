//! Single sign-on in the computer's own browser, where the person's provider
//! session and password manager live, never inside the window.
//!
//! The application listens on a loopback port, opens the Core's sign-in
//! address in the default browser and receives a one-time code on that port.
//! The code is worthless without the verifier kept here, which the page of
//! the Core then redeems for its session.
use aether_client::Client;
use std::{
    io::{ErrorKind, Read, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};

/// How long the browser may take to come back.
const WAIT: Duration = Duration::from_secs(300);

/// What the browser brought back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Code {
        code: String,
        verifier: String,
    },
    /// An error code of the Core, worded by its sign-in page.
    Refused(String),
}

/// A sign-in waiting for the browser.
pub struct Attempt {
    listener: TcpListener,
    /// Opened in the default browser.
    pub url: String,
    verifier: String,
}

pub fn prepare(server: &str, device: &str) -> Option<Attempt> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    let request = Client::new(server, device)
        .ok()?
        .start_sso(&format!("http://127.0.0.1:{port}/sso"))
        .ok()?;
    Some(Attempt {
        listener,
        url: request.url,
        verifier: request.verifier,
    })
}

impl Attempt {
    /// Waits for the browser. `None` when nobody came back in time, or when
    /// `current` says a newer attempt replaced this one.
    pub fn wait(self, current: impl Fn() -> bool) -> Option<Outcome> {
        self.listener.set_nonblocking(true).ok()?;
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline && current() {
            match self.listener.accept() {
                Ok((stream, _)) => match answer(stream) {
                    Some(Ok(code)) => {
                        return Some(Outcome::Code {
                            code,
                            verifier: self.verifier,
                        });
                    }
                    Some(Err(reason)) => return Some(Outcome::Refused(reason)),
                    // Anything else the browser asks (an icon): keep waiting.
                    None => {}
                },
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(150));
                }
                Err(_) => return None,
            }
        }
        None
    }
}

/// Reads one request and tells the person to return to the application.
fn answer(mut stream: TcpStream) -> Option<Result<String, String>> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut buffer = [0u8; 4096];
    let mut length = 0;
    while length < buffer.len() {
        match stream.read(&mut buffer[length..]) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                length += read;
                if buffer[..length].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
        }
    }
    let head = String::from_utf8_lossy(&buffer[..length]);
    let result = parse(head.lines().next().unwrap_or(""));
    let (status, body) = match &result {
        Some(outcome) => ("200 OK", page(outcome.is_ok())),
        None => ("404 Not Found", String::new()),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    result
}

/// `GET /sso?code=<64 hex> HTTP/1.1`, or `error=<code of the Core>`.
fn parse(request_line: &str) -> Option<Result<String, String>> {
    let target = request_line.strip_prefix("GET ")?.split(' ').next()?;
    let query = target.strip_prefix("/sso?")?;
    query
        .split('&')
        .find_map(|pair| match pair.split_once('=') {
            Some(("code", value)) if is_hex64(value) => Some(Ok(value.to_owned())),
            Some(("error", value)) if is_error_code(value) => Some(Err(value.to_owned())),
            _ => None,
        })
}

pub fn is_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn is_error_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

fn page(signed_in: bool) -> String {
    let (title, text) = if signed_in {
        (
            "Connexion réussie",
            "Vous pouvez fermer cet onglet et revenir à Aether.",
        )
    } else {
        (
            "Connexion refusée",
            "Revenez à Aether pour réessayer. Vous pouvez fermer cet onglet.",
        )
    };
    format!(
        "<!doctype html><html lang=\"fr\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Aether</title>\
         <style>html,body{{height:100%;margin:0}}body{{display:grid;place-items:center;background:#0b0e18;\
         color:#eef1f8;font:16px/1.5 'Segoe UI',system-ui,sans-serif;text-align:center}}\
         h1{{font-size:24px;margin:0 0 8px}}p{{margin:0;color:#a2a9ba}}</style></head>\
         <body><main><h1>{title}</h1><p>{text}</p></main></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_code_or_a_reason_is_read_from_the_browser() {
        let code = "ab".repeat(32);
        assert_eq!(
            parse(&format!("GET /sso?code={code} HTTP/1.1")),
            Some(Ok(code.clone()))
        );
        assert_eq!(
            parse("GET /sso?error=oidc_group_denied HTTP/1.1"),
            Some(Err("oidc_group_denied".into()))
        );
        assert_eq!(parse("GET /favicon.ico HTTP/1.1"), None);
        assert_eq!(parse("GET /sso?code=short HTTP/1.1"), None);
        assert_eq!(parse(&format!("POST /sso?code={code} HTTP/1.1")), None);
        assert_eq!(
            parse("GET /sso?error=%3Cscript%3E HTTP/1.1"),
            None,
            "nothing but a code of the Core reaches the page"
        );
        assert_eq!(parse(&format!("GET /other?code={code} HTTP/1.1")), None);
    }

    #[test]
    fn the_browser_is_answered_and_the_code_comes_back_with_the_verifier() {
        let attempt = prepare("https://media.example", "test").unwrap();
        let port = attempt.listener.local_addr().unwrap().port();
        assert!(
            attempt
                .url
                .starts_with("https://media.example/api/v1/auth/oidc/app/start?challenge=")
        );
        assert!(
            attempt
                .url
                .contains(&format!("redirect=http%3A%2F%2F127.0.0.1%3A{port}%2Fsso"))
        );
        let verifier = attempt.verifier.clone();
        assert!(is_hex64(&verifier));
        let code = "0f".repeat(32);
        let request = format!("GET /sso?code={code} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        let browser = std::thread::spawn(move || {
            let mut icon = TcpStream::connect(("127.0.0.1", port)).unwrap();
            icon.write_all(b"GET /favicon.ico HTTP/1.1\r\n\r\n")
                .unwrap();
            let mut reply = String::new();
            icon.read_to_string(&mut reply).unwrap();
            assert!(reply.starts_with("HTTP/1.1 404"));
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.write_all(request.as_bytes()).unwrap();
            let mut reply = String::new();
            stream.read_to_string(&mut reply).unwrap();
            reply
        });
        assert_eq!(
            attempt.wait(|| true),
            Some(Outcome::Code { code, verifier })
        );
        let reply = browser.join().unwrap();
        assert!(reply.starts_with("HTTP/1.1 200 OK"));
        assert!(reply.contains("Connexion réussie"));

        // An attempt replaced by a newer one stops waiting.
        let replaced = prepare("https://media.example", "test").unwrap();
        assert_eq!(replaced.wait(|| false), None);
    }
}
