//! `https://` through `Endpoint`/`HttpConnection` against a local rustls
//! server with a throwaway CA (rcgen). Covers GET/POST/chunked/read-to-close,
//! SSE streaming with disconnect, `NOTEMAKER_EXTRA_CA_FILE`, and the
//! Java-style errors for untrusted certificates and hostname mismatches.

use notemaker_core::endpoint::{Endpoint, HttpConnection};
use notemaker_core::error::ErrorKind;
use notemaker_core::json;
use notemaker_core::net::{ApiPath, ChatId, ChatPath, HttpStatus, HttpUrl};
use notemaker_core::tls::TlsConfig;
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Once, OnceLock};
use std::time::{Duration, Instant};

struct Ca {
    pem: String,
    cert: rcgen::Certificate,
    key: KeyPair,
}

fn ca(name: &str) -> Ca {
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.distinguished_name.push(DnType::CommonName, name);
    let key = KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    Ca { pem: cert.pem(), cert, key }
}

/// Server certificate for `names` (DNS names or IP literals) signed by `ca`.
fn leaf(ca: &Ca, names: &[&str]) -> Arc<ServerConfig> {
    let params = CertificateParams::new(names.iter().map(|n| n.to_string()).collect::<Vec<_>>()).unwrap();
    let key = KeyPair::generate().unwrap();
    let cert = params.signed_by(&key, &ca.cert, &ca.key).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert.der().to_vec()), CertificateDer::from(ca.cert.der().to_vec())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
        .unwrap();
    Arc::new(config)
}

/// Serve TLS connections; `respond(head, body)` returns the raw response and
/// whether to send close_notify before closing.
fn server(config: Arc<ServerConfig>, respond: impl Fn(&str, &[u8]) -> (Vec<u8>, bool) + Send + Sync + 'static) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let respond = Arc::new(respond);
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let (config, respond) = (config.clone(), respond.clone());
            std::thread::spawn(move || {
                let socket = socket.unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                let mut tls = StreamOwned::new(ServerConnection::new(config).unwrap(), socket);
                let mut head = String::new();
                let mut body = Vec::new();
                {
                    let mut reader = BufReader::new(&mut tls);
                    loop {
                        let mut line = String::new();
                        match reader.read_line(&mut line) {
                            Ok(0) | Err(_) => return, // client rejected the handshake
                            Ok(_) if line == "\r\n" => break,
                            Ok(_) => head.push_str(&line),
                        }
                    }
                    let length = head
                        .lines()
                        .find_map(|l| l.strip_prefix("Content-Length: "))
                        .map(|v| v.trim().parse::<usize>().unwrap())
                        .unwrap_or(0);
                    body.resize(length, 0);
                    reader.read_exact(&mut body).unwrap();
                }
                let (response, close_notify) = respond(&head, &body);
                let _ = tls.write_all(&response);
                let _ = tls.flush();
                if close_notify {
                    tls.conn.send_close_notify();
                    let _ = tls.conn.complete_io(&mut tls.sock);
                } else if head.starts_with("GET /api/stream") {
                    std::thread::sleep(Duration::from_secs(10));
                }
            });
        }
    });
    port
}

fn trusted(ca: &Ca) -> TlsConfig {
    TlsConfig::with_roots_pem(ca.pem.as_bytes()).unwrap()
}

/// A GET target the test servers route on.
fn chat(id: &str) -> ChatPath {
    ChatPath::Get { chat: ChatId::from(id) }
}

fn json_reply(text: &str) -> Vec<u8> {
    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", text.len(), text).into_bytes()
}

#[test]
fn https_get_post_and_framing() {
    let ca = ca("NoteMaker test CA");
    let port = server(leaf(&ca, &["localhost", "127.0.0.1"]), |head, body| {
        let target = head.split(' ').nth(1).unwrap().to_string();
        match target.as_str() {
            "/api/chats/plain" => (json_reply("{\"a\":1}"), true),
            "/api/chats/chunked" => (b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"b\"\r\n3;x=y\r\n:2}\r\n0\r\n\r\n".to_vec(), true),
            // Read-to-close body ended without close_notify.
            "/api/chats/close" => (b"HTTP/1.1 200 OK\r\n\r\n{\"c\":3}".to_vec(), false),
            "/api/chats/truncated" => (b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n{\"c\":3}".to_vec(), false),
            "/api/chats/error" => (b"HTTP/1.1 409 Conflict\r\nContent-Length: 17\r\n\r\n{\"error\":\"nope\"}\n".to_vec(), true),
            "/api/chats/host" => {
                let host = head.lines().find_map(|l| l.strip_prefix("Host: ")).unwrap();
                (json_reply(&format!("{{\"host\":\"{host}\"}}")), true)
            }
            "/api/events" => {
                let auth = head.lines().any(|l| l == "Authorization: Bearer secret");
                let json = head.lines().any(|l| l == "Content-Type: application/json");
                (json_reply(&format!("{{\"echo\":{},\"auth\":{},\"json\":{}}}", String::from_utf8_lossy(body), auth, json)), true)
            }
            _ => (b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(), true),
        }
    });
    for host in ["localhost", "127.0.0.1"] {
        let url = format!("https://{host}:{port}");
        let peer = Endpoint::resolve(&format!("{url}/")).unwrap().with_tls_config(trusted(&ca));
        assert_eq!(json::write_map(&peer.request(chat("plain"), None).unwrap()), "{\"a\":1}");
        assert_eq!(json::write_map(&peer.request(chat("chunked"), None).unwrap()), "{\"b\":2}");
        assert_eq!(json::write_map(&peer.request(chat("close"), None).unwrap()), "{\"c\":3}");
        assert_eq!(peer.request(chat("truncated"), None).unwrap_err().message(), "Premature EOF");
        assert_eq!(json::write_map(&peer.request(chat("host"), None).unwrap()), format!("{{\"host\":\"{host}:{port}\"}}"));
        let e = peer.request(chat("error"), None).unwrap_err();
        assert_eq!((e.kind, e.status, e.message()), (ErrorKind::Http, Some(HttpStatus::from(409)), "nope"));
        let secret = Endpoint::new(notemaker_core::object! { "url" => url.as_str(), "token" => "secret" }).with_tls_config(trusted(&ca));
        let body = json::parse("{\"operations\":[1,\"é\"]}").unwrap();
        assert_eq!(
            json::write_map(&secret.request(ApiPath::Events, Some(&body)).unwrap()),
            "{\"echo\":{\"operations\":[1,\"é\"]},\"auth\":true,\"json\":true}"
        );
    }
}

#[test]
fn https_stream_lines_and_disconnect() {
    let ca = ca("NoteMaker stream CA");
    let port = server(leaf(&ca, &["localhost"]), |_, _| {
        (b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nc\r\ndata: one\n\n\r\n".to_vec(), false)
    });
    let peer = Endpoint::new(notemaker_core::object! { "url" => format!("https://localhost:{port}") }).with_tls_config(trusted(&ca));
    let mut c = peer.open(ApiPath::Stream).unwrap();
    assert_eq!(c.response_code().unwrap(), HttpStatus::OK);
    let closer = c.disconnector();
    let mut lines = BufReader::new(c.input_stream().unwrap()).lines();
    assert_eq!(lines.next().unwrap().unwrap(), "data: one");
    assert_eq!(lines.next().unwrap().unwrap(), "");
    let started = Instant::now();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        closer.disconnect();
    });
    // Same outcome as the plain-HTTP stream test: the blocked read fails.
    assert!(lines.next().unwrap().is_err());
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// The CA that `NOTEMAKER_EXTRA_CA_FILE` names for this test binary; set
/// before any test reads the platform store.
fn env_ca() -> &'static Ca {
    static CA: OnceLock<Ca> = OnceLock::new();
    static SET: Once = Once::new();
    let ca = CA.get_or_init(|| ca("NoteMaker env CA"));
    SET.call_once(|| {
        let path = std::env::temp_dir().join(format!("notemaker-extra-ca-{}.pem", std::process::id()));
        std::fs::write(&path, &ca.pem).unwrap();
        std::env::set_var(notemaker_core::tls::EXTRA_CA_ENV, &path);
    });
    ca
}

#[test]
fn extra_ca_file_is_trusted_with_platform_store() {
    let ca = env_ca();
    let port = server(leaf(ca, &["localhost"]), |_, _| (json_reply("{\"ok\":true}"), true));
    let peer = Endpoint::resolve(&format!("https://localhost:{port}")).unwrap();
    assert_eq!(json::write_map(&peer.request(chat("x"), None).unwrap()), "{\"ok\":true}");
}

#[test]
fn untrusted_certificate_fails_like_java() {
    env_ca();
    let stranger = ca("Untrusted CA");
    let port = server(leaf(&stranger, &["localhost"]), |_, _| (json_reply("{}"), true));
    let pkix = "PKIX path building failed: sun.security.provider.certpath.SunCertPathBuilderException: unable to find valid certification path to requested target";
    // Platform store (+ the env CA, which did not sign this server).
    let e = Endpoint::resolve(&format!("https://localhost:{port}")).unwrap().request(chat("x"), None).unwrap_err();
    assert_eq!((e.kind, e.message()), (ErrorKind::SslHandshake, pkix));
    assert!(e.kind.is_io());
    assert_eq!(e.to_string(), format!("javax.net.ssl.SSLHandshakeException: {pkix}"));
    // Explicit roots that do not include the issuer.
    let mut c = HttpConnection::new(&HttpUrl::from(format!("https://localhost:{port}/x"))).unwrap();
    c.set_tls_config(trusted(&ca("Other CA")));
    assert_eq!(c.response_code().unwrap_err().message(), pkix);
}

#[test]
fn hostname_mismatch_fails_like_java() {
    let ca = ca("NoteMaker name CA");
    let port = server(leaf(&ca, &["other.test"]), |_, _| (json_reply("{}"), true));
    for (host, message) in [
        ("localhost", "No subject alternative DNS name matching localhost found.".to_string()),
        ("127.0.0.1", "No subject alternative names matching IP address 127.0.0.1 found".to_string()),
    ] {
        let peer = Endpoint::resolve(&format!("https://{host}:{port}")).unwrap().with_tls_config(trusted(&ca));
        let e = peer.request(chat("x"), None).unwrap_err();
        assert_eq!((e.kind, e.message()), (ErrorKind::SslHandshake, message.as_str()), "{host}");
    }
}

#[test]
fn plain_http_server_on_https_url_fails_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in listener.incoming() {
            let mut s = s.unwrap();
            let mut buf = [0; 1024];
            let _ = s.read(&mut buf);
            let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
        }
    });
    let ca = ca("unused");
    let e = Endpoint::resolve(&format!("https://localhost:{port}")).unwrap().with_tls_config(trusted(&ca)).request(chat("x"), None).unwrap_err();
    assert_eq!(e.kind, ErrorKind::SslHandshake, "{e}");
}
