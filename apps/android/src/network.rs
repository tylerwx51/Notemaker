//! Network settings: the notes server address and the editing proxy
//! (ADR 0030). Each is an enum so "not configured" is its own case instead
//! of an empty string. The parsing keeps the Java originals' `java.net.URI`
//! behaviour exactly, quirks included; the constructors do not validate
//! beyond what those originals did.

use crate::error::{AppError, AppResult};
use crate::java_port::integer::parse_int;
use crate::java_port::uri;
use crate::ui::{PrefText, UiText};
use notemaker_core::jfmt;
use notemaker_core::net::{BearerToken, HttpUrl, Port};
use std::fmt;

/// Java `"/+$"` removal (`replaceAll("/+$", "")`) on an already trimmed string.
fn strip_trailing_slashes(s: &str) -> &str {
    s.trim_end_matches('/')
}

/// The notes server the app syncs with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ServerAddr {
    /// Local-only notes.
    #[default]
    Unset,
    /// The server URL, normally `scheme://host:port`.
    Set(HttpUrl),
}

impl ServerAddr {
    /// `MainActivity.normalizeServerAddress`: a saved address, given
    /// `http://` when it names no scheme and the default port when it names
    /// no port.
    pub fn normalize(saved: &PrefText) -> AppResult<ServerAddr> {
        let mut address = strip_trailing_slashes(jfmt::java_trim(saved)).to_string();
        if address.is_empty() {
            return Ok(ServerAddr::Unset);
        }
        if !(address.starts_with("http://") || address.starts_with("https://")) {
            address = format!("http://{address}");
        }
        let uri = uri::parse(&address).map_err(|_| AppError::java("java.net.URISyntaxException", format!("Invalid URI: {address}")))?;
        if uri.host.is_none() || uri.port > 65535 || uri.port == 0 {
            return Err(AppError::illegal_argument("invalid server address"));
        }
        if uri.port < 0 {
            let authority = uri.authority.clone().unwrap_or_else(|| "null".into());
            address = format!(
                "{}://{}:{}{}{}{}",
                uri.scheme.clone().unwrap_or_else(|| "null".into()),
                authority,
                Port::DEFAULT_SERVER,
                uri.path.clone().unwrap_or_default(),
                uri.query.as_ref().map(|q| format!("?{q}")).unwrap_or_default(),
                uri.fragment.as_ref().map(|f| format!("#{f}")).unwrap_or_default()
            );
        }
        Ok(ServerAddr::Set(HttpUrl::from(strip_trailing_slashes(&address))))
    }

    /// `MainActivity.composeServerAddress`: the address and port settings
    /// fields. An address without a scheme keeps `current`'s scheme.
    pub fn compose(address: &UiText, port: &UiText, current: &ServerAddr) -> AppResult<ServerAddr> {
        let mut address = jfmt::java_trim(address);
        if address.is_empty() {
            if !jfmt::java_trim(port).is_empty() {
                return Err(AppError::illegal_argument("Port requires a server address"));
            }
            return Ok(ServerAddr::Unset);
        }
        let current_https = matches!(current, ServerAddr::Set(url) if url.starts_with("https://"));
        let mut scheme = if current_https { "https" } else { "http" };
        if let Some(rest) = address.strip_prefix("https://") {
            scheme = "https";
            address = rest;
        } else if let Some(rest) = address.strip_prefix("http://") {
            scheme = "http";
            address = rest;
        }
        let text = format!("http://{}", strip_trailing_slashes(address));
        let parsed = uri::parse(&text).map_err(|_| AppError::java("java.net.URISyntaxException", format!("Invalid URI: {text}")))?;
        if parsed.host.is_none() || parsed.user_info.is_some() || parsed.path.as_ref().is_some_and(|p| !p.is_empty()) || parsed.query.is_some() || parsed.fragment.is_some() {
            return Err(AppError::illegal_argument("Enter only a host or IP address"));
        }
        let mut number = parsed.port;
        let supplied_port = jfmt::java_trim(port);
        if !supplied_port.is_empty() {
            number = parse_int(supplied_port).ok_or_else(|| AppError::illegal_argument("Invalid port"))?;
        }
        if number < 0 {
            number = u16::from(Port::DEFAULT_SERVER).into();
        }
        let port = u16::try_from(number).ok().filter(|p| *p != 0).map(Port::from).ok_or_else(|| AppError::illegal_argument("Port must be between 1 and 65535"))?;
        Ok(ServerAddr::Set(HttpUrl::from(format!("{}://{}:{}", scheme, parsed.host.unwrap_or_default(), port))))
    }

    /// `MainActivity.serverAddressForSettings`: the host for the address
    /// field, or the whole stored value when it does not parse.
    pub fn settings_host(&self) -> UiText {
        match self {
            ServerAddr::Unset => UiText::default(),
            ServerAddr::Set(url) => match uri::parse(url) {
                Ok(uri) if uri.host.is_some() => UiText::from(uri.host.unwrap_or_default()),
                _ => UiText::from(&**url),
            },
        }
    }

    /// `MainActivity.serverPortForSettings`: the port for the port field,
    /// blank for the default port.
    pub fn settings_port(&self) -> UiText {
        match self {
            ServerAddr::Set(url) => match uri::parse(url) {
                Ok(uri) if uri.port > 0 && uri.port != i32::from(u16::from(Port::DEFAULT_SERVER)) => UiText::from(uri.port.to_string()),
                _ => UiText::default(),
            },
            ServerAddr::Unset => UiText::default(),
        }
    }
}

/// A stored or test-supplied address, taken as is. Blank means [`ServerAddr::Unset`].
impl From<&str> for ServerAddr {
    fn from(value: &str) -> ServerAddr {
        if jfmt::java_trim(value).is_empty() {
            ServerAddr::Unset
        } else {
            ServerAddr::Set(HttpUrl::from(value))
        }
    }
}

/// The stored form: empty when unset.
impl fmt::Display for ServerAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerAddr::Unset => Ok(()),
            ServerAddr::Set(url) => f.write_str(url),
        }
    }
}

/// The editing proxy the app sends commands through.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProxyAddr {
    /// This device's embedded proxy.
    #[default]
    Unset,
    /// A remote proxy's URL, without trailing slashes.
    Set(HttpUrl),
}

impl ProxyAddr {
    /// The proxy URL settings field: trimmed, trailing slashes removed.
    pub fn from_input(text: &UiText) -> ProxyAddr {
        ProxyAddr::from(strip_trailing_slashes(jfmt::java_trim(text)))
    }

    /// An HTTP(S) proxy with a token, or no proxy.
    pub fn accepts(&self, token: &BearerToken) -> bool {
        match self {
            ProxyAddr::Unset => true,
            ProxyAddr::Set(url) => (url.starts_with("http://") || url.starts_with("https://")) && !token.is_empty(),
        }
    }
}

/// A stored or test-supplied URL, taken as is. Empty means [`ProxyAddr::Unset`].
impl From<&str> for ProxyAddr {
    fn from(value: &str) -> ProxyAddr {
        if value.is_empty() {
            ProxyAddr::Unset
        } else {
            ProxyAddr::Set(HttpUrl::from(value))
        }
    }
}

/// The stored form: empty when unset.
impl fmt::Display for ProxyAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProxyAddr::Unset => Ok(()),
            ProxyAddr::Set(url) => f.write_str(url),
        }
    }
}

/// The network settings fields as typed, before validation.
pub struct NetworkSettingsForm {
    pub server: UiText,
    pub port: UiText,
    pub proxy: UiText,
    pub token: UiText,
}

impl NetworkSettingsForm {
    /// The proxy token field, trimmed.
    pub fn token(&self) -> BearerToken {
        BearerToken::from(jfmt::java_trim(&self.token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (input, normalizeServerAddress, serverAddressForSettings, serverPortForSettings),
    /// produced by the Java originals on OpenJDK 17.
    const NORMALIZE: &[(&str, Result<&str, &str>, &str, &str)] = &[
        ("", Ok(""), "", ""),
        ("  ", Ok(""), "", ""),
        ("host", Ok("http://host:8765"), "host", ""),
        ("host/", Ok("http://host:8765"), "host/", ""),
        ("host///", Ok("http://host:8765"), "host///", ""),
        ("http://host", Ok("http://host:8765"), "host", ""),
        ("https://host", Ok("https://host:8765"), "host", ""),
        ("http://host:80", Ok("http://host:80"), "host", "80"),
        ("http://host:0", Err("invalid server address"), "host", ""),
        ("http://host:65535", Ok("http://host:65535"), "host", "65535"),
        ("http://host:65536", Err("invalid server address"), "host", "65536"),
        ("http://host:99999999999", Err("invalid server address"), "http://host:99999999999", ""),
        ("http://host:", Ok("http://host::8765"), "host", ""),
        ("10.0.0.2", Ok("http://10.0.0.2:8765"), "10.0.0.2", ""),
        ("10.0.0.2:9000", Ok("http://10.0.0.2:9000"), "10.0.0.2:9000", ""),
        ("192.168.50.129:8765/", Ok("http://192.168.50.129:8765"), "192.168.50.129:8765/", ""),
        ("http://user@host", Ok("http://user@host:8765"), "host", ""),
        ("http://user@host:1", Ok("http://user@host:1"), "host", "1"),
        ("http://host/path", Ok("http://host:8765/path"), "host", ""),
        ("http://host/path/?q=1#f", Ok("http://host:8765/path/?q=1#f"), "host", ""),
        ("http://host?q", Ok("http://host:8765?q"), "host", ""),
        ("http://host#frag", Ok("http://host:8765#frag"), "host", ""),
        ("my_host", Err("invalid server address"), "my_host", ""),
        ("my-host.example.com", Ok("http://my-host.example.com:8765"), "my-host.example.com", ""),
        ("bad-.com", Err("invalid server address"), "bad-.com", ""),
        ("1.2.3.400", Err("invalid server address"), "1.2.3.400", ""),
        ("1.2.3.4.5", Err("invalid server address"), "1.2.3.4.5", ""),
        ("[::1]", Ok("http://[::1]:8765"), "[::1]", ""),
        ("[::1]:9000", Ok("http://[::1]:9000"), "[::1]:9000", ""),
        ("http://[fe80::1%25eth0]:80", Ok("http://[fe80::1%25eth0]:80"), "[fe80::1%25eth0]", "80"),
        ("[::1", Err("Expected closing bracket for IPv6 address at index 11: http://[::1"), "[::1", ""),
        ("ho st", Err("Illegal character in authority at index 7: http://ho st"), "ho st", ""),
        ("http://", Ok("http://http::8765"), "http://", ""),
        ("https://", Ok("http://https::8765"), "https://", ""),
        ("ftp://x", Ok("http://ftp::8765//x"), "x", ""),
        ("HTTP://host", Ok("http://HTTP::8765//host"), "host", ""),
        ("host:abc", Err("invalid server address"), "host:abc", ""),
        ("-host", Err("invalid server address"), "-host", ""),
        ("a.1b", Err("invalid server address"), "a.1b", ""),
        ("a.b1", Ok("http://a.b1:8765"), "a.b1", ""),
        ("%41", Err("invalid server address"), "%41", ""),
        ("h%41st", Err("invalid server address"), "h%41st", ""),
        ("x:y@z", Ok("http://x:y@z:8765"), "x:y@z", ""),
        ("été.fr", Err("invalid server address"), "été.fr", ""),
        ("http://host:8765", Ok("http://host:8765"), "host", ""),
        ("http://h:08765", Ok("http://h:08765"), "h", ""),
        ("http://[1:2:3:4:5:6:7:8]", Ok("http://[1:2:3:4:5:6:7:8]:8765"), "[1:2:3:4:5:6:7:8]", ""),
        ("http://[1:2:3:4:5:6:7]", Err("IPv6 address too short at index 8: http://[1:2:3:4:5:6:7]"), "http://[1:2:3:4:5:6:7]", ""),
        ("http://[::ffff:1.2.3.4]", Ok("http://[::ffff:1.2.3.4]:8765"), "[::ffff:1.2.3.4]", ""),
        ("http://host:+80", Err("invalid server address"), "http://host:+80", ""),
        ("http://host:-1", Err("invalid server address"), "http://host:-1", ""),
    ];
    /// (address, port, currentHost, composeServerAddress) from the Java original.
    const COMPOSE: &[(&str, &str, Option<&str>, Result<&str, &str>)] = &[
        ("host", "", None, Ok("http://host:8765")),
        ("host", "", Some("https://x"), Ok("https://host:8765")),
        ("https://host", "", None, Ok("https://host:8765")),
        ("http://host:9000", "", None, Ok("http://host:9000")),
        ("host", "9000", None, Ok("http://host:9000")),
        ("host", "0", None, Err("Port must be between 1 and 65535")),
        ("host", "-5", None, Ok("http://host:8765")),
        ("host", "+80", None, Ok("http://host:80")),
        ("host", "abc", None, Err("Invalid port")),
        ("host", "70000", None, Err("Port must be between 1 and 65535")),
        ("", "", None, Ok("")),
        ("", "80", None, Err("Port requires a server address")),
        ("host/", "", None, Ok("http://host:8765")),
        ("host/x", "", None, Err("Enter only a host or IP address")),
        ("user@host", "", None, Err("Enter only a host or IP address")),
        ("host?q", "", None, Err("Enter only a host or IP address")),
        ("host#f", "", None, Err("Enter only a host or IP address")),
        ("[::1]", "", None, Ok("http://[::1]:8765")),
        ("my_host", "", None, Err("Enter only a host or IP address")),
        ("host:99999", "", None, Err("Port must be between 1 and 65535")),
        (" host ", "  ", None, Ok("http://host:8765")),
        ("1.2.3.4", "8080", Some("https://old:1"), Ok("https://1.2.3.4:8080")),
        ("ho st", "", None, Err("Illegal character in authority at index 7: http://ho st")),
    ];

    #[test]
    fn server_addresses_match_java() {
        for (input, normalized, host, port) in NORMALIZE {
            let actual = ServerAddr::normalize(&PrefText::from(*input)).map(|a| a.to_string());
            match normalized {
                Ok(expected) => assert_eq!(actual.as_deref().ok(), Some(*expected), "normalize {input:?}"),
                Err(_) => assert!(actual.is_err(), "normalize {input:?} gave {actual:?}"),
            }
            let stored = ServerAddr::from(*input);
            assert_eq!(&*stored.settings_host(), *host, "host {input:?}");
            assert_eq!(&*stored.settings_port(), *port, "port {input:?}");
        }
        assert_eq!(&*ServerAddr::Unset.settings_host(), "");
        assert_eq!(&*ServerAddr::Unset.settings_port(), "");
    }

    #[test]
    fn compose_matches_java() {
        for (address, port, current, expected) in COMPOSE {
            let current = current.map(ServerAddr::from).unwrap_or_default();
            let actual = ServerAddr::compose(&UiText::from(*address), &UiText::from(*port), &current).map(|a| a.to_string());
            match expected {
                Ok(expected) => assert_eq!(actual.as_deref().ok(), Some(*expected), "compose {address:?} {port:?}"),
                Err(message) if message.contains(" at index ") => assert!(actual.is_err(), "compose {address:?}"),
                Err(message) => assert_eq!(actual.err().and_then(|e| e.message()).as_deref(), Some(*message), "compose {address:?} {port:?}"),
            }
        }
    }

    #[test]
    fn proxy_needs_an_http_url_and_token() {
        let token = BearerToken::from("t");
        assert_eq!(ProxyAddr::from_input(&UiText::from("  https://p:1// ")), ProxyAddr::Set(HttpUrl::from("https://p:1")));
        assert_eq!(ProxyAddr::from_input(&UiText::from("   ")), ProxyAddr::Unset);
        assert!(ProxyAddr::Unset.accepts(&BearerToken::default()));
        assert!(ProxyAddr::from("http://p").accepts(&token));
        assert!(!ProxyAddr::from("http://p").accepts(&BearerToken::default()));
        assert!(!ProxyAddr::from("p:1").accepts(&token));
        assert_eq!(strip_trailing_slashes("a//"), "a");
    }
}
