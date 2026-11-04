//! Network values: URLs, ports, bearer tokens and the HTTP vocabulary that
//! [`crate::endpoint`] speaks (ADR 0030).
//!
//! [`EndpointPath`] is the complete set of requests a NoteMaker client sends
//! to a server or proxy. Each variant renders the exact request target the
//! Java code built by hand, including query encoding, and knows its method.

use crate::ids::DocumentName;
use crate::java_port::url::url_encode;
use std::fmt;

crate::string_newtype! {
    /// An absolute `http://` or `https://` URL, as typed or stored. It is not
    /// validated on construction; [`crate::endpoint::HttpConnection::new`]
    /// rejects URLs it cannot connect to.
    pub struct HttpUrl;
}

impl HttpUrl {
    /// This base URL followed by `path`'s request target.
    pub fn join(&self, path: impl Into<EndpointPath>) -> HttpUrl {
        HttpUrl(format!("{}{}", self.0, path.into()))
    }
}

/// A TCP port. `0` asks the OS for an ephemeral port when binding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Port(u16);

impl Port {
    /// Bind any free port.
    pub const EPHEMERAL: Port = Port(0);
    /// The notes server's port when an address names none.
    pub const DEFAULT_SERVER: Port = Port(8765);
    /// The local replica proxy's port (`serve` without a port, the app's
    /// embedded proxy).
    pub const DEFAULT_PROXY: Port = Port(8766);
}

impl From<u16> for Port {
    fn from(value: u16) -> Port {
        Port(value)
    }
}
impl From<Port> for u16 {
    fn from(value: Port) -> u16 {
        value.0
    }
}
impl From<Port> for crate::json::Value {
    fn from(value: Port) -> crate::json::Value {
        crate::json::Value::from(value.0)
    }
}
impl fmt::Display for Port {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A proxy's bearer token. `Debug` hides the value so tokens do not reach
/// logs; there is deliberately no `Display`. Read the value with `&*token`
/// where it must be stored or sent.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct BearerToken(String);

impl BearerToken {
    /// The `Authorization` header value for this token.
    pub fn authorization(&self) -> HeaderValue {
        HeaderValue::new(format!("Bearer {}", self.0))
    }
}
impl fmt::Debug for BearerToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_empty() { "BearerToken(\"\")" } else { "BearerToken(<redacted>)" })
    }
}
impl std::ops::Deref for BearerToken {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
impl From<String> for BearerToken {
    fn from(value: String) -> BearerToken {
        BearerToken(value)
    }
}
impl From<&str> for BearerToken {
    fn from(value: &str) -> BearerToken {
        BearerToken(value.to_string())
    }
}
impl From<&BearerToken> for crate::json::Value {
    fn from(value: &BearerToken) -> crate::json::Value {
        crate::json::Value::String(value.0.clone())
    }
}

/// The request methods NoteMaker uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    Get,
    Post,
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
        })
    }
}
impl std::str::FromStr for HttpMethod {
    type Err = crate::error::Error;
    /// The exact upper-case method token from a request line.
    fn from_str(method: &str) -> crate::error::Result<HttpMethod> {
        match method {
            "GET" => Ok(HttpMethod::Get),
            "POST" => Ok(HttpMethod::Post),
            _ => Err(crate::error::Error::illegal_argument(format!("Unsupported method: {method}"))),
        }
    }
}

/// The headers NoteMaker sets or reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeaderName {
    Authorization,
    ContentLength,
    ContentType,
    TransferEncoding,
}

impl fmt::Display for HeaderName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HeaderName::Authorization => "Authorization",
            HeaderName::ContentLength => "Content-Length",
            HeaderName::ContentType => "Content-Type",
            HeaderName::TransferEncoding => "Transfer-Encoding",
        })
    }
}

crate::string_newtype! {
    /// A header value, sent or received verbatim.
    pub struct HeaderValue;
}

impl HeaderValue {
    /// `application/json`, the type of every JSON request body.
    pub fn json() -> HeaderValue {
        HeaderValue::from("application/json")
    }
}

/// An HTTP response status code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HttpStatus(u16);

impl HttpStatus {
    pub const OK: HttpStatus = HttpStatus(200);
    pub const NO_CONTENT: HttpStatus = HttpStatus(204);
    pub const NOT_MODIFIED: HttpStatus = HttpStatus(304);
    pub const BAD_REQUEST: HttpStatus = HttpStatus(400);
    pub const NOT_FOUND: HttpStatus = HttpStatus(404);

    /// 400 and above: `HttpURLConnection` serves the body as the error stream.
    pub fn is_error(self) -> bool {
        self.0 >= 400
    }
    /// 1xx, 204 and 304 responses carry no body.
    pub fn has_no_body(self) -> bool {
        (100..200).contains(&self.0) || self == HttpStatus::NO_CONTENT || self == HttpStatus::NOT_MODIFIED
    }
}
impl From<u16> for HttpStatus {
    fn from(value: u16) -> HttpStatus {
        HttpStatus(value)
    }
}
impl From<HttpStatus> for u16 {
    fn from(value: HttpStatus) -> u16 {
        value.0
    }
}
impl fmt::Display for HttpStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

crate::string_newtype! {
    /// An agent chat's identifier, as the server assigned it. It is placed in
    /// request paths verbatim.
    pub struct ChatId;
}

/// Every request target a NoteMaker client sends. `Display` renders the
/// path and query; [`EndpointPath::method`] gives the method.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EndpointPath {
    /// `POST /command`: run CLI arguments at a proxy.
    Command,
    /// `/api/...`: replica sync, locks, UI sessions and chats.
    Api(ApiPath),
}

/// The `/api/...` requests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiPath {
    /// `GET /api/info?trail=…[&validate=1]`: protocol, root and lineage.
    /// `trail` is the comma-separated replica lineage asking.
    Info { trail: String, validate: bool },
    /// `GET /api/replica?document=…&after=…&pendingAfter=…`: committed and
    /// provisional events after the two cursors.
    Replica { document: DocumentName, after: i32, pending_after: i32 },
    /// `POST /api/events`: upload pending operations.
    Events,
    /// `GET /api/locks?document=…`: the document's leases and previews.
    Locks { document: DocumentName },
    /// `POST /api/locks`: claim, release or preview leases.
    LockUpdate,
    /// `GET /api/stream`: server-sent change notifications.
    Stream,
    /// `/api/ui/...`: live app sessions.
    Ui(UiPath),
    /// `/api/chats...`: agent chats on the primary server.
    Chats(ChatPath),
}

/// The `/api/ui/...` requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiPath {
    /// `GET /api/ui/sessions`.
    Sessions,
    /// `POST /api/ui/heartbeat`.
    Heartbeat,
    /// `POST /api/ui/command`.
    Command,
}

/// The `/api/chats...` requests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatPath {
    /// `GET /api/chats[?limit=…&offset=…]`; the server defaults are 5 and 0.
    List { limit: Option<i64>, offset: Option<i64> },
    /// `POST /api/chats`.
    Create,
    /// `GET /api/chats/options`: available harnesses and models.
    Options,
    /// `GET /api/chats/{chat}`.
    Get { chat: ChatId },
    /// `GET /api/chats/{chat}/events[?after=…]`.
    Events { chat: ChatId, after: Option<i64> },
    /// `GET /api/chats/{chat}/stream[?after=…]`: newline-delimited events.
    Stream { chat: ChatId, after: Option<i64> },
    /// `POST /api/chats/{chat}/messages`.
    Messages { chat: ChatId },
    /// `POST /api/chats/{chat}/stop`.
    Stop { chat: ChatId },
}

impl EndpointPath {
    /// The method this request is sent with.
    pub fn method(&self) -> HttpMethod {
        use HttpMethod::{Get, Post};
        match self {
            EndpointPath::Command => Post,
            EndpointPath::Api(api) => match api {
                ApiPath::Info { .. } | ApiPath::Replica { .. } | ApiPath::Locks { .. } | ApiPath::Stream => Get,
                ApiPath::Events | ApiPath::LockUpdate => Post,
                ApiPath::Ui(UiPath::Sessions) => Get,
                ApiPath::Ui(UiPath::Heartbeat | UiPath::Command) => Post,
                ApiPath::Chats(chat) => match chat {
                    ChatPath::List { .. } | ChatPath::Options | ChatPath::Get { .. } | ChatPath::Events { .. } | ChatPath::Stream { .. } => Get,
                    ChatPath::Create | ChatPath::Messages { .. } | ChatPath::Stop { .. } => Post,
                },
            },
        }
    }
}

impl From<ApiPath> for EndpointPath {
    fn from(path: ApiPath) -> EndpointPath {
        EndpointPath::Api(path)
    }
}
impl From<UiPath> for EndpointPath {
    fn from(path: UiPath) -> EndpointPath {
        EndpointPath::Api(ApiPath::Ui(path))
    }
}
impl From<ChatPath> for EndpointPath {
    fn from(path: ChatPath) -> EndpointPath {
        EndpointPath::Api(ApiPath::Chats(path))
    }
}
impl From<&EndpointPath> for EndpointPath {
    fn from(path: &EndpointPath) -> EndpointPath {
        path.clone()
    }
}

impl fmt::Display for EndpointPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EndpointPath::Command => f.write_str("/command"),
            EndpointPath::Api(api) => write!(f, "{api}"),
        }
    }
}

impl fmt::Display for ApiPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiPath::Info { trail, validate } => {
                write!(f, "/api/info?trail={}", url_encode(trail))?;
                if *validate {
                    f.write_str("&validate=1")?;
                }
                Ok(())
            }
            ApiPath::Replica { document, after, pending_after } => {
                write!(f, "/api/replica?document={}&after={}&pendingAfter={}", url_encode(document), after, pending_after)
            }
            ApiPath::Events => f.write_str("/api/events"),
            ApiPath::Locks { document } => write!(f, "/api/locks?document={}", url_encode(document)),
            ApiPath::LockUpdate => f.write_str("/api/locks"),
            ApiPath::Stream => f.write_str("/api/stream"),
            ApiPath::Ui(ui) => f.write_str(match ui {
                UiPath::Sessions => "/api/ui/sessions",
                UiPath::Heartbeat => "/api/ui/heartbeat",
                UiPath::Command => "/api/ui/command",
            }),
            ApiPath::Chats(chat) => write!(f, "{chat}"),
        }
    }
}

impl fmt::Display for ChatPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let after = |f: &mut fmt::Formatter<'_>, after: &Option<i64>| match after {
            Some(after) => write!(f, "?after={after}"),
            None => Ok(()),
        };
        match self {
            ChatPath::List { limit, offset } => {
                f.write_str("/api/chats")?;
                let mut separator = '?';
                for (key, value) in [("limit", limit), ("offset", offset)] {
                    if let Some(value) = value {
                        write!(f, "{separator}{key}={value}")?;
                        separator = '&';
                    }
                }
                Ok(())
            }
            ChatPath::Create => f.write_str("/api/chats"),
            ChatPath::Options => f.write_str("/api/chats/options"),
            ChatPath::Get { chat } => write!(f, "/api/chats/{chat}"),
            ChatPath::Events { chat, after: a } => {
                write!(f, "/api/chats/{chat}/events")?;
                after(f, a)
            }
            ChatPath::Stream { chat, after: a } => {
                write!(f, "/api/chats/{chat}/stream")?;
                after(f, a)
            }
            ChatPath::Messages { chat } => write!(f, "/api/chats/{chat}/messages"),
            ChatPath::Stop { chat } => write!(f, "/api/chats/{chat}/stop"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(path: impl Into<EndpointPath>) -> (String, HttpMethod) {
        let path = path.into();
        (path.to_string(), path.method())
    }

    #[test]
    fn targets_match_the_hand_built_java_strings() {
        use HttpMethod::{Get, Post};
        let chat = || ChatId::from("c1");
        let notes = || DocumentName::from("my notes/ä");
        let cases: Vec<(EndpointPath, &str, HttpMethod)> = vec![
            (EndpointPath::Command, "/command", Post),
            (ApiPath::Info { trail: "r1,r2".into(), validate: true }.into(), "/api/info?trail=r1%2Cr2&validate=1", Get),
            (ApiPath::Info { trail: "r-1".into(), validate: false }.into(), "/api/info?trail=r-1", Get),
            (ApiPath::Replica { document: notes(), after: 3, pending_after: -1 }.into(), "/api/replica?document=my+notes%2F%C3%A4&after=3&pendingAfter=-1", Get),
            (ApiPath::Events.into(), "/api/events", Post),
            (ApiPath::Locks { document: notes() }.into(), "/api/locks?document=my+notes%2F%C3%A4", Get),
            (ApiPath::LockUpdate.into(), "/api/locks", Post),
            (ApiPath::Stream.into(), "/api/stream", Get),
            (UiPath::Sessions.into(), "/api/ui/sessions", Get),
            (UiPath::Heartbeat.into(), "/api/ui/heartbeat", Post),
            (UiPath::Command.into(), "/api/ui/command", Post),
            (ChatPath::List { limit: Some(5), offset: Some(0) }.into(), "/api/chats?limit=5&offset=0", Get),
            (ChatPath::List { limit: None, offset: Some(20) }.into(), "/api/chats?offset=20", Get),
            (ChatPath::List { limit: None, offset: None }.into(), "/api/chats", Get),
            (ChatPath::Create.into(), "/api/chats", Post),
            (ChatPath::Options.into(), "/api/chats/options", Get),
            (ChatPath::Get { chat: chat() }.into(), "/api/chats/c1", Get),
            (ChatPath::Events { chat: chat(), after: Some(7) }.into(), "/api/chats/c1/events?after=7", Get),
            (ChatPath::Stream { chat: chat(), after: Some(0) }.into(), "/api/chats/c1/stream?after=0", Get),
            (ChatPath::Stream { chat: chat(), after: None }.into(), "/api/chats/c1/stream", Get),
            (ChatPath::Messages { chat: chat() }.into(), "/api/chats/c1/messages", Post),
            (ChatPath::Stop { chat: chat() }.into(), "/api/chats/c1/stop", Post),
        ];
        for (path, expected, method) in cases {
            assert_eq!(target(path), (expected.to_string(), method));
        }
    }

    #[test]
    fn url_join_token_redaction_and_status() {
        let base = HttpUrl::from("http://h:8765");
        assert_eq!(&*base.join(ApiPath::Stream), "http://h:8765/api/stream");
        let token = BearerToken::from("secret");
        assert_eq!(format!("{token:?}"), "BearerToken(<redacted>)");
        assert_eq!(&*token.authorization(), "Bearer secret");
        assert!(HttpStatus::from(400).is_error() && !HttpStatus::OK.is_error());
        assert!(HttpStatus::NO_CONTENT.has_no_body() && HttpStatus::from(101).has_no_body() && !HttpStatus::OK.has_no_body());
        assert_eq!("POST".parse::<HttpMethod>().unwrap(), HttpMethod::Post);
        assert!("post".parse::<HttpMethod>().is_err());
    }
}
