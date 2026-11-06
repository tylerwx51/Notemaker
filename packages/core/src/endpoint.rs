//! Port of `Endpoint.java`: HTTP peer, optionally authenticated with a
//! private proxy descriptor.
//!
//! `HttpURLConnection` is replaced by [`HttpConnection`], a small blocking
//! HTTP/1.1 client over `std::net`. `https://` URLs use rustls with the
//! platform trust store and hostname verification ([`crate::tls`]); `http://`
//! requests are written to the socket exactly as before.

use crate::error::{io_message, Error, Result};
use crate::json::{self, Map, Value};
use crate::net::{EndpointPath, HeaderName, HeaderValue, HttpMethod, HttpStatus, HttpUrl};
use crate::tls::{self, TlsConfig};
use crate::{json_object_newtype, json_value_newtype};
use rustls::pki_types::ServerName;
use rustls::{ClientConnection, StreamOwned};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::Duration;

/// Cloneable, thread-safe handle that closes a connection's socket
/// (`HttpURLConnection.disconnect()` from another thread).
#[derive(Clone, Default)]
pub struct Disconnector {
    inner: Arc<DisconnectInner>,
}

#[derive(Default)]
struct DisconnectInner {
    closed: AtomicBool,
    socket: Mutex<Option<TcpStream>>,
}

impl Disconnector {
    /// Shut the socket down; any blocked read or write returns promptly.
    /// Sticky: a connection that has not connected yet will not connect.
    pub fn disconnect(&self) {
        self.inner.closed.store(true, Ordering::SeqCst);
        if let Some(socket) = self.inner.socket.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
    pub fn is_disconnected(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }
    fn attach(&self, socket: &TcpStream) -> Result<()> {
        let mut slot = self.inner.socket.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_disconnected() {
            return Err(Error::io("Socket closed"));
        }
        *slot = Some(socket.try_clone()?);
        Ok(())
    }
}

struct Url {
    host: String,
    port: u16,
    /// Path and query, starting with `/`.
    target: String,
    host_header: String,
    https: bool,
}

fn parse_url(url: &str) -> Result<Url> {
    let (https, rest) = match url.strip_prefix("https://") {
        Some(rest) => (true, rest),
        None => (false, url.strip_prefix("http://").ok_or_else(|| Error::io(format!("no protocol: {}", url)))?),
    };
    let default_port = if https { 443 } else { 80 };
    let (authority, target) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let target = target.split('#').next().unwrap_or("");
    let target = if target.starts_with('/') { target.to_string() } else { format!("/{}", target) };
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let (host, port) = if let Some(stripped) = authority.strip_prefix('[') {
        let end = stripped.find(']').ok_or_else(|| Error::io(format!("Invalid host: {}", authority)))?;
        let host = &stripped[..end];
        let port = stripped[end + 1..].strip_prefix(':').filter(|p| !p.is_empty());
        (host.to_string(), port)
    } else {
        match authority.rfind(':') {
            Some(i) => (authority[..i].to_string(), Some(&authority[i + 1..]).filter(|p| !p.is_empty())),
            None => (authority.to_string(), None),
        }
    };
    if host.is_empty() {
        return Err(Error::io(format!("Invalid host: {}", url)));
    }
    let port_number = match port {
        Some(p) => p.parse::<u16>().map_err(|_| Error::io(format!("For input string: \"{}\"", p)))?,
        None => default_port,
    };
    let bracketed = if host.contains(':') { format!("[{}]", host) } else { host.clone() };
    let host_header = if port_number == default_port { bracketed } else { format!("{}:{}", bracketed, port_number) };
    Ok(Url { host, port: port_number, target, host_header, https })
}

/// Socket for one request: plain TCP or TLS over it.
enum Transport {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Transport {
    fn socket(&self) -> &TcpStream {
        match self {
            Transport::Plain(s) => s,
            Transport::Tls(s) => &s.sock,
        }
    }
}

impl Read for Transport {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Transport::Plain(s) => s.read(buf),
            // A peer that closes without close_notify ends the stream, as
            // Java's SSLSocket tolerated; Content-Length and chunked framing
            // still report truncation ("Premature EOF").
            Transport::Tls(s) => match s.read(buf) {
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(0),
                r => r,
            },
        }
    }
}

impl Write for Transport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Transport::Plain(s) => s.write(buf),
            Transport::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Transport::Plain(s) => s.flush(),
            Transport::Tls(s) => s.flush(),
        }
    }
}

enum Framing {
    Length(u64),
    Chunked { remaining: u64, done: bool },
    Close,
    Empty,
}

/// Response body stream (`getInputStream()` / `getErrorStream()`); decodes
/// `Content-Length`, chunked and read-to-close framing. Wrap in a
/// `BufReader` for line-oriented (SSE) reading.
pub struct BodyReader {
    reader: BufReader<Transport>,
    framing: Framing,
    disconnector: Disconnector,
}

impl BodyReader {
    fn closed_error(&self, e: std::io::Error) -> std::io::Error {
        if self.disconnector.is_disconnected() {
            std::io::Error::other("Socket closed")
        } else if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "Read timed out")
        } else {
            e
        }
    }
    fn raw_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.reader.read(buf) {
            Ok(0) if self.disconnector.is_disconnected() => Err(std::io::Error::other("Socket closed")),
            Ok(n) => Ok(n),
            Err(e) => Err(self.closed_error(e)),
        }
    }
    fn read_line(&mut self) -> std::io::Result<String> {
        let mut line = Vec::new();
        match self.reader.read_until(b'\n', &mut line) {
            Ok(_) => {}
            Err(e) => return Err(self.closed_error(e)),
        }
        if line.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Premature EOF"));
        }
        Ok(String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_string())
    }
}

impl Read for BodyReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        match self.framing {
            Framing::Empty => Ok(0),
            Framing::Close => self.raw_read(buf),
            Framing::Length(0) => Ok(0),
            Framing::Length(remaining) => {
                let want = buf.len().min(remaining.min(usize::MAX as u64) as usize);
                let n = self.raw_read(&mut buf[..want])?;
                if n == 0 {
                    return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Premature EOF"));
                }
                self.framing = Framing::Length(remaining - n as u64);
                Ok(n)
            }
            Framing::Chunked { done: true, .. } => Ok(0),
            Framing::Chunked { remaining, done: false } => {
                let mut remaining = remaining;
                if remaining == 0 {
                    let line = self.read_line()?;
                    let size = line.split(';').next().unwrap_or("").trim();
                    remaining = u64::from_str_radix(size, 16)
                        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid chunk size"))?;
                    if remaining == 0 {
                        // Trailers end with an empty line.
                        loop {
                            if self.read_line()?.is_empty() {
                                break;
                            }
                        }
                        self.framing = Framing::Chunked { remaining: 0, done: true };
                        return Ok(0);
                    }
                }
                let want = buf.len().min(remaining.min(usize::MAX as u64) as usize);
                let n = self.raw_read(&mut buf[..want])?;
                if n == 0 {
                    return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Premature EOF"));
                }
                remaining -= n as u64;
                if remaining == 0 {
                    self.read_line()?; // CRLF after the chunk data
                }
                self.framing = Framing::Chunked { remaining, done: false };
                Ok(n)
            }
        }
    }
}

/// Blocking HTTP/1.1 request with the `HttpURLConnection` subset NoteMaker uses.
pub struct HttpConnection {
    url: HttpUrl,
    method: HttpMethod,
    headers: Vec<(HeaderName, HeaderValue)>,
    body: Option<Vec<u8>>,
    connect_timeout: u32,
    read_timeout: u32,
    status: Option<HttpStatus>,
    response_headers: Vec<(String, HeaderValue)>,
    body_reader: Option<BodyReader>,
    disconnector: Disconnector,
    tls: Option<TlsConfig>,
}

impl HttpConnection {
    /// `new URL(url).openConnection()` (no I/O happens until [`Self::response_code`]).
    pub fn new(url: &HttpUrl) -> Result<HttpConnection> {
        parse_url(url)?;
        Ok(HttpConnection {
            url: url.clone(),
            method: HttpMethod::Get,
            headers: Vec::new(),
            body: None,
            connect_timeout: 0,
            read_timeout: 0,
            status: None,
            response_headers: Vec::new(),
            body_reader: None,
            disconnector: Disconnector::default(),
            tls: None,
        })
    }
    /// TLS settings for an `https://` URL (`setSSLSocketFactory`); the
    /// platform trust store ([`TlsConfig::platform`]) when not set.
    pub fn set_tls_config(&mut self, config: TlsConfig) {
        self.tls = Some(config);
    }
    /// `setConnectTimeout(ms)`; 0 means no timeout.
    pub fn set_connect_timeout(&mut self, millis: u32) {
        self.connect_timeout = millis;
    }
    /// `setReadTimeout(ms)`; 0 means no timeout. Applies to an open socket too.
    pub fn set_read_timeout(&mut self, millis: u32) {
        self.read_timeout = millis;
        if let Some(reader) = &self.body_reader {
            let _ = reader.reader.get_ref().socket().set_read_timeout(timeout(millis));
        }
    }
    pub fn read_timeout(&self) -> u32 {
        self.read_timeout
    }
    /// `setRequestMethod(method)`.
    pub fn set_request_method(&mut self, method: HttpMethod) {
        self.method = method;
    }
    /// `setRequestProperty(name, value)` (replaces an existing value).
    pub fn set_request_property(&mut self, name: HeaderName, value: HeaderValue) {
        if let Some(entry) = self.headers.iter_mut().find(|(k, _)| *k == name) {
            entry.1 = value;
        } else {
            self.headers.push((name, value));
        }
    }
    /// Fixed-length request body (`setDoOutput` + `setFixedLengthStreamingMode`
    /// + writing `getOutputStream()`); sent with the request.
    pub fn write_body(&mut self, bytes: Vec<u8>) {
        self.body = Some(bytes);
    }
    /// Handle for closing this connection from another thread.
    pub fn disconnector(&self) -> Disconnector {
        self.disconnector.clone()
    }
    /// `disconnect()`.
    pub fn disconnect(&self) {
        self.disconnector.disconnect();
    }
    /// Response header value (case-insensitive name), after `response_code`.
    pub fn header_field(&self, name: HeaderName) -> Option<&HeaderValue> {
        let name = name.to_string();
        self.response_headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(&name)).map(|(_, v)| v)
    }

    fn io_error(&self, e: std::io::Error, connecting: bool) -> Error {
        if self.disconnector.is_disconnected() {
            return Error::io("Socket closed");
        }
        match e.kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                Error::io(if connecting { "Connect timed out" } else { "Read timed out" })
            }
            _ => Error::io(io_message(&e)),
        }
    }

    fn connect(&mut self) -> Result<()> {
        let url = parse_url(&self.url)?;
        if self.disconnector.is_disconnected() {
            return Err(Error::io("Socket closed"));
        }
        let addresses: Vec<_> = (url.host.as_str(), url.port)
            .to_socket_addrs()
            .map_err(|_| Error::io(url.host.clone()))?
            .collect();
        let mut last: Option<std::io::Error> = None;
        let mut stream = None;
        for address in addresses {
            let attempt = match timeout(self.connect_timeout) {
                Some(t) => TcpStream::connect_timeout(&address, t),
                None => TcpStream::connect(address),
            };
            match attempt {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e) => last = Some(e),
            }
        }
        let stream = match stream {
            Some(s) => s,
            None => {
                let e = last.unwrap_or_else(|| std::io::Error::other(url.host.clone()));
                return Err(self.io_error(e, true));
            }
        };
        self.disconnector.attach(&stream)?;
        stream.set_read_timeout(timeout(self.read_timeout))?;
        let _ = stream.set_nodelay(true);
        let mut stream = if url.https { self.handshake(&url, stream)? } else { Transport::Plain(stream) };
        let mut request = format!("{} {} HTTP/1.1\r\nHost: {}\r\n", self.method, url.target, url.host_header);
        for (k, v) in &self.headers {
            request.push_str(&format!("{}: {}\r\n", k, v));
        }
        if let Some(body) = &self.body {
            request.push_str(&format!("{}: {}\r\n", HeaderName::ContentLength, body.len()));
        }
        request.push_str("Connection: close\r\n\r\n");
        let write = stream.write_all(request.as_bytes()).and_then(|_| match &self.body {
            Some(body) => stream.write_all(body),
            None => Ok(()),
        });
        write.and_then(|_| stream.flush()).map_err(|e| self.io_error(e, false))?;
        let mut reader = BodyReader { reader: BufReader::new(stream), framing: Framing::Close, disconnector: self.disconnector.clone() };
        let status_line = reader.read_line().map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                Error::io("Unexpected end of file from server")
            } else {
                self.io_error(e, false)
            }
        })?;
        let mut parts = status_line.splitn(3, ' ');
        let version = parts.next().unwrap_or("");
        let status = parts.next().and_then(|s| s.parse::<u16>().ok()).map(HttpStatus::from);
        let status = match status {
            Some(s) if version.starts_with("HTTP/") => s,
            _ => return Err(Error::io(format!("Invalid Http response: {}", status_line))),
        };
        let mut headers = Vec::new();
        loop {
            let line = reader.read_line().map_err(|e| self.io_error(e, false))?;
            if line.is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                headers.push((k.trim().to_string(), HeaderValue::from(v.trim())));
            }
        }
        let header = |name: HeaderName| {
            let name = name.to_string();
            headers.iter().find(|(k, _): &&(String, HeaderValue)| k.eq_ignore_ascii_case(&name)).map(|(_, v)| v.to_string())
        };
        reader.framing = if status.has_no_body() {
            Framing::Empty
        } else if header(HeaderName::TransferEncoding).is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
            Framing::Chunked { remaining: 0, done: false }
        } else if let Some(length) = header(HeaderName::ContentLength).and_then(|v| v.parse::<u64>().ok()) {
            Framing::Length(length)
        } else {
            Framing::Close
        };
        self.status = Some(status);
        self.response_headers = headers;
        self.body_reader = Some(reader);
        Ok(())
    }

    /// TLS handshake on a connected socket, verifying the certificate chain
    /// and the URL host. The read timeout bounds the handshake.
    fn handshake(&self, url: &Url, socket: TcpStream) -> Result<Transport> {
        let config = match &self.tls {
            Some(c) => c.clone(),
            None => TlsConfig::platform()?,
        };
        let name = ServerName::try_from(url.host.clone()).map_err(|_| Error::io(format!("Invalid host: {}", url.host)))?;
        let connection = ClientConnection::new(config.0, name).map_err(|e| tls::handshake_error(&e, &url.host))?;
        let mut stream = StreamOwned::new(connection, socket);
        while stream.conn.is_handshaking() {
            if let Err(e) = stream.conn.complete_io(&mut stream.sock) {
                if let Some(tls_error) = e.get_ref().and_then(|inner| inner.downcast_ref::<rustls::Error>()) {
                    return Err(tls::handshake_error(tls_error, &url.host));
                }
                if e.kind() == std::io::ErrorKind::UnexpectedEof && !self.disconnector.is_disconnected() {
                    return Err(Error::new(
                        crate::error::ErrorKind::SslHandshake,
                        "Remote host terminated the handshake",
                    ));
                }
                return Err(self.io_error(e, false));
            }
        }
        Ok(Transport::Tls(Box::new(stream)))
    }

    /// `getResponseCode()`: sends the request on first use.
    pub fn response_code(&mut self) -> Result<HttpStatus> {
        if self.status.is_none() {
            self.connect()?;
        }
        self.status.ok_or_else(|| Error::io("Unexpected end of file from server"))
    }

    /// `getInputStream()`: the response body; I/O error for status >= 400.
    pub fn input_stream(&mut self) -> Result<BodyReader> {
        let status = self.response_code()?;
        if status.is_error() {
            return Err(Error::io(format!("Server returned HTTP response code: {} for URL: {}", status, self.url)));
        }
        self.body_reader.take().ok_or_else(|| Error::io("Stream closed"))
    }

    /// `getErrorStream()`: the body of a >= 400 response, otherwise `None`.
    pub fn error_stream(&mut self) -> Result<Option<BodyReader>> {
        match self.status {
            Some(status) if status.is_error() => Ok(self.body_reader.take()),
            _ => Ok(None),
        }
    }
}

impl Drop for HttpConnection {
    fn drop(&mut self) {
        if let Some(reader) = &self.body_reader {
            let _ = reader.reader.get_ref().socket().shutdown(Shutdown::Both);
        }
    }
}

fn timeout(millis: u32) -> Option<Duration> {
    if millis == 0 {
        None
    } else {
        Some(Duration::from_millis(millis as u64))
    }
}

/// `Endpoint.open`'s per-path read timeout in milliseconds.
fn read_timeout(path: &EndpointPath) -> u32 {
    use crate::net::{ApiPath, UiPath};
    match path {
        EndpointPath::Command | EndpointPath::Api(ApiPath::Ui(UiPath::Command)) => 40000,
        EndpointPath::Api(ApiPath::Locks { .. } | ApiPath::LockUpdate | ApiPath::Replica { .. } | ApiPath::Info { .. }) => 2000,
        EndpointPath::Api(ApiPath::Stream) => 45000,
        EndpointPath::Api(ApiPath::Events | ApiPath::Ui(UiPath::Sessions | UiPath::Heartbeat) | ApiPath::Chats(_)) => 5000,
    }
}

json_object_newtype! {
    /// A peer descriptor: `{"url": …, "token"?: …}` (a private proxy
    /// descriptor file adds the bearer token).
    pub struct EndpointDescriptor;
}

json_value_newtype! {
    /// A JSON request body sent to a peer.
    pub struct RequestBody;
}

json_object_newtype! {
    /// A peer's JSON object reply.
    pub struct ResponseBody;
}

/// The thread that made an `Endpoint` request, so a peer can cancel that
/// request alone (`cancel(Thread)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestThread(ThreadId);

impl RequestThread {
    /// The calling thread.
    pub fn current() -> RequestThread {
        RequestThread(std::thread::current().id())
    }
}

impl From<ThreadId> for RequestThread {
    fn from(thread: ThreadId) -> Self {
        RequestThread(thread)
    }
}

struct RequestState {
    cancelled: AtomicBool,
    connection: Mutex<Option<Disconnector>>,
}

/// `Endpoint`: HTTP peer, optionally authenticated with a private proxy descriptor.
pub struct Endpoint {
    pub descriptor: EndpointDescriptor,
    active_requests: Mutex<HashMap<RequestThread, Arc<RequestState>>>,
    tls: Option<TlsConfig>,
}

impl Endpoint {
    pub fn new(descriptor: EndpointDescriptor) -> Endpoint {
        Endpoint { descriptor, active_requests: Mutex::new(HashMap::new()), tls: None }
    }

    /// Use `config` instead of the platform trust store for `https://` URLs.
    pub fn with_tls_config(mut self, config: TlsConfig) -> Endpoint {
        self.tls = Some(config);
        self
    }

    /// Interrupt this peer's request from a particular worker without
    /// disturbing other callers (`cancel(Thread)`).
    pub fn cancel(&self, thread: RequestThread) -> bool {
        let state = self.active_requests.lock().unwrap_or_else(|e| e.into_inner()).get(&thread).cloned();
        let Some(state) = state else { return false };
        state.cancelled.store(true, Ordering::SeqCst);
        if let Some(connection) = state.connection.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            connection.disconnect();
        }
        true
    }

    /// `Endpoint.resolve(target)`: an `http(s)://` URL or a descriptor JSON file.
    pub fn resolve(target: &str) -> Result<Endpoint> {
        if target.starts_with("http://") || target.starts_with("https://") {
            let url = target.trim_end_matches('/');
            return Ok(Endpoint::new(crate::object! { "url" => url }.into()));
        }
        let bytes = std::fs::read(target).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::io(target.to_string())
            } else {
                Error::io(format!("{}: {}", target, io_message(&e)))
            }
        })?;
        let value = json::parse(&String::from_utf8_lossy(&bytes))?;
        Ok(Endpoint::new(json::map(&value)?.clone().into()))
    }

    /// `open(path)`: connection with the per-path timeouts and bearer token.
    pub fn open(&self, path: impl Into<EndpointPath>) -> Result<HttpConnection> {
        let path = path.into();
        let mut c = HttpConnection::new(&HttpUrl::from(json::string(&self.descriptor, "url")?).join(&path))?;
        if let Some(config) = &self.tls {
            c.set_tls_config(config.clone());
        }
        c.set_connect_timeout(3000);
        c.set_read_timeout(read_timeout(&path));
        if self.descriptor.contains_key("token") {
            let token = crate::net::BearerToken::from(json::string(&self.descriptor, "token")?);
            c.set_request_property(HeaderName::Authorization, token.authorization());
        }
        Ok(c)
    }

    /// `request(path, body)`: the path's method, with `body` as JSON when given.
    pub fn request(&self, path: impl Into<EndpointPath>, body: Option<&RequestBody>) -> Result<ResponseBody> {
        let path = path.into();
        let thread = RequestThread::current();
        let state = Arc::new(RequestState { cancelled: AtomicBool::new(false), connection: Mutex::new(None) });
        self.active_requests.lock().unwrap_or_else(|e| e.into_inner()).insert(thread, state.clone());
        let mut connection: Option<HttpConnection> = None;
        let result = self.request_inner(&path, body.map(|b| &b.0), &state, &mut connection).map(ResponseBody);
        {
            let mut active = self.active_requests.lock().unwrap_or_else(|e| e.into_inner());
            if active.get(&thread).is_some_and(|s| Arc::ptr_eq(s, &state)) {
                active.remove(&thread);
            }
        }
        if let Some(c) = connection {
            c.disconnect();
        }
        result
    }

    fn request_inner(&self, path: &EndpointPath, body: Option<&Value>, state: &RequestState, slot: &mut Option<HttpConnection>) -> Result<Map> {
        let c = slot.insert(self.open(path)?);
        *state.connection.lock().unwrap_or_else(|e| e.into_inner()) = Some(c.disconnector());
        if state.cancelled.load(Ordering::SeqCst) {
            return Err(Error::io("Peer request cancelled"));
        }
        if body.is_some() && *path == EndpointPath::Api(crate::net::ApiPath::Events) {
            c.set_read_timeout(40000);
        }
        c.set_request_method(path.method());
        if let Some(body) = body {
            let bytes = json::write(body).into_bytes();
            c.set_request_property(HeaderName::ContentType, HeaderValue::json());
            c.write_body(bytes);
        }
        let status = c.response_code()?;
        let source = if status == HttpStatus::OK { Some(c.input_stream()?) } else { c.error_stream()? };
        let Some(mut source) = source else {
            return Err(Error::http(status, format!("Peer HTTP {}", status)));
        };
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes).map_err(|e| Error::io(io_message(&e)))?;
        let value = json::parse(&String::from_utf8_lossy(&bytes))?;
        let result = json::map(&value)?.clone();
        if status != HttpStatus::OK {
            return Err(Error::http(status, json::java_string(result.get("error"))));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{ApiPath, ChatId, ChatPath};
    use std::net::TcpListener;

    /// A GET target the test servers route on.
    fn chat(id: &str) -> ChatPath {
        ChatPath::Get { chat: ChatId::from(id) }
    }

    /// Serve each connection with `respond(request_head, request_body)`.
    fn server(respond: impl Fn(&str, &[u8]) -> Vec<u8> + Send + 'static) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                }
                let length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("Content-Length: "))
                    .map(|v| v.trim().parse::<usize>().unwrap())
                    .unwrap_or(0);
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let response = respond(&head, &body);
                let _ = stream.write_all(&response);
            }
        });
        address
    }

    #[test]
    fn plain_chunked_error_and_post() {
        let url = server(|head, body| {
            let target = head.split(' ').nth(1).unwrap().to_string();
            match target.as_str() {
                "/api/chats/plain" => b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\n{\"a\":1}".to_vec(),
                "/api/chats/chunked" => b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"b\"\r\n3;x=y\r\n:2}\r\n0\r\n\r\n".to_vec(),
                "/api/chats/close" => b"HTTP/1.1 200 OK\r\n\r\n{\"c\":3}".to_vec(),
                "/api/chats/error" => b"HTTP/1.1 409 Conflict\r\nContent-Length: 17\r\n\r\n{\"error\":\"nope\"}\n".to_vec(),
                "/api/chats/empty" => b"HTTP/1.1 204 No Content\r\n\r\n".to_vec(),
                "/api/events" => {
                    let auth = head.lines().any(|l| l == "Authorization: Bearer secret");
                    let json = head.lines().any(|l| l == "Content-Type: application/json");
                    let reply = format!("{{\"echo\":{},\"auth\":{},\"json\":{}}}", String::from_utf8_lossy(body), auth, json);
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", reply.len(), reply).into_bytes()
                }
                _ => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
            }
        });
        let peer = Endpoint::resolve(&format!("{}//", url)).unwrap();
        assert_eq!(json::write_map(&peer.request(chat("plain"), None).unwrap()), "{\"a\":1}");
        assert_eq!(json::write_map(&peer.request(chat("chunked"), None).unwrap()), "{\"b\":2}");
        assert_eq!(json::write_map(&peer.request(chat("close"), None).unwrap()), "{\"c\":3}");
        let e = peer.request(chat("error"), None).unwrap_err();
        assert_eq!((e.kind, e.status, e.message()), (crate::error::ErrorKind::Http, Some(HttpStatus::from(409)), "nope"));
        let e = peer.request(chat("empty"), None).unwrap_err();
        assert_eq!((e.status, e.message()), (Some(HttpStatus::NO_CONTENT), "Peer HTTP 204"));
        let secret = Endpoint::new(crate::object! { "url" => url.as_str(), "token" => "secret" }.into());
        let body = json::parse("{\"operations\":[1,\"é\"]}").unwrap();
        assert_eq!(
            json::write_map(&secret.request(ApiPath::Events, Some((&body).into())).unwrap()),
            "{\"echo\":{\"operations\":[1,\"é\"]},\"auth\":true,\"json\":true}"
        );
    }

    #[test]
    fn https_urls_parse_with_default_port() {
        let u = parse_url("https://host/api?x=1#f").unwrap();
        assert_eq!((u.https, u.port, u.host_header.as_str(), u.target.as_str()), (true, 443, "host", "/api?x=1"));
        let u = parse_url("https://[::1]:8443").unwrap();
        assert_eq!((u.host.as_str(), u.port, u.host_header.as_str()), ("::1", 8443, "[::1]:8443"));
        let u = parse_url("http://host:443/").unwrap();
        assert_eq!((u.https, u.host_header.as_str()), (false, "host:443"));
    }

    #[test]
    fn stream_lines_and_disconnect() {
        let url = server(|_, _| {
            let mut out = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
            out.extend_from_slice(b"c\r\ndata: one\n\n\r\n");
            out
        });
        let peer = Endpoint::new(crate::object! { "url" => url.as_str() }.into());
        let mut c = peer.open(ApiPath::Stream).unwrap();
        assert_eq!(c.read_timeout(), 45000);
        assert_eq!(c.response_code().unwrap(), HttpStatus::OK);
        let closer = c.disconnector();
        let mut lines = BufReader::new(c.input_stream().unwrap()).lines();
        assert_eq!(lines.next().unwrap().unwrap(), "data: one");
        assert_eq!(lines.next().unwrap().unwrap(), "");
        let started = std::time::Instant::now();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            closer.disconnect();
        });
        assert!(lines.next().unwrap().is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn cancel_in_flight_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let held: Vec<_> = listener.incoming().take(1).collect();
            std::thread::sleep(Duration::from_secs(10));
            drop(held);
        });
        let peer = Arc::new(Endpoint::new(crate::object! { "url" => url.as_str() }.into()));
        let worker = {
            let peer = peer.clone();
            std::thread::spawn(move || peer.request(EndpointPath::Command, None))
        };
        let id = RequestThread::from(worker.thread().id());
        let started = std::time::Instant::now();
        while !peer.cancel(id) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let e = worker.join().unwrap().unwrap_err();
        assert!(e.kind.is_io(), "{e}");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!peer.cancel(id));
    }
}
