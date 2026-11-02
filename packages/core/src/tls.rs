//! TLS for `https://` peers (replaces the JDK/Conscrypt `HttpsURLConnection`
//! default `SSLSocketFactory`, see ADR 0028).
//!
//! [`TlsConfig::platform`] trusts the platform CA store, like Java's default
//! `TrustManager`:
//!
//! - Android: the system CA directory Conscrypt reads
//!   (`/apex/com.android.conscrypt/cacerts` on Android 14+, otherwise
//!   `/system/etc/security/cacerts`), minus the system CAs the user disabled in
//!   Settings (`/data/misc/user/<id>/cacerts-removed`, when readable).
//!   User-added CAs are not trusted, matching the default network security
//!   config for `targetSdkVersion >= 24` (the manifest declares no
//!   `networkSecurityConfig`).
//! - Linux and other hosts: the OS store through `rustls-native-certs`
//!   (`SSL_CERT_FILE`/`SSL_CERT_DIR` or the distribution bundle). When it
//!   yields no certificates, the bundled Mozilla roots (`webpki-roots`) are
//!   used, as the JDK falls back to its bundled `cacerts`.
//!
//! On every platform, `NOTEMAKER_EXTRA_CA_FILE` may name a PEM file whose
//! certificates are trusted in addition (the role `javax.net.ssl.trustStore`
//! played for the JVM). Hostname verification is always on.
//!
//! Handshake failures map to `javax.net.ssl` exception kinds with the messages
//! the JDK (hosts) or Conscrypt/OkHttp (Android) reported.

use crate::error::{io_message, Error, ErrorKind, Result};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use rustls::{ClientConfig, RootCertStore};
use std::sync::{Arc, OnceLock};

/// Environment variable naming an extra PEM CA bundle to trust.
pub const EXTRA_CA_ENV: &str = "NOTEMAKER_EXTRA_CA_FILE";

/// Client TLS settings for an `https://` connection
/// (`HttpsURLConnection.setSSLSocketFactory` equivalent).
#[derive(Clone)]
pub struct TlsConfig(pub(crate) Arc<ClientConfig>);

impl std::fmt::Debug for TlsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TlsConfig")
    }
}

impl TlsConfig {
    /// Platform trust store plus `NOTEMAKER_EXTRA_CA_FILE` (loaded once).
    pub fn platform() -> Result<TlsConfig> {
        static PLATFORM: OnceLock<Result<TlsConfig>> = OnceLock::new();
        PLATFORM
            .get_or_init(|| {
                let extra = std::env::var_os(EXTRA_CA_ENV).filter(|v| !v.is_empty());
                let mut roots = platform_roots();
                if let Some(path) = extra {
                    let bytes = std::fs::read(&path).map_err(|e| {
                        Error::io(format!("{}: {}: {}", EXTRA_CA_ENV, path.to_string_lossy(), io_message(&e)))
                    })?;
                    add_pem(&mut roots, &bytes)?;
                }
                build(roots)
            })
            .clone()
    }

    /// Trust only the certificates in `pem` (tests and private deployments).
    pub fn with_roots_pem(pem: &[u8]) -> Result<TlsConfig> {
        let mut roots = RootCertStore::empty();
        add_pem(&mut roots, pem)?;
        build(roots)
    }
}

fn build(roots: RootCertStore) -> Result<TlsConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::new(ErrorKind::SslHandshake, e.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(TlsConfig(Arc::new(config)))
}

/// Add every certificate in a PEM bundle; error when it holds none.
fn add_pem(roots: &mut RootCertStore, pem: &[u8]) -> Result<()> {
    let mut added = 0;
    for cert in CertificateDer::pem_slice_iter(pem) {
        let cert = cert.map_err(|e| Error::io(format!("Invalid PEM certificate: {e}")))?;
        roots.add(cert).map_err(|e| Error::io(format!("Invalid CA certificate: {e}")))?;
        added += 1;
    }
    if added == 0 {
        return Err(Error::io("No certificates found in PEM data"));
    }
    Ok(())
}

/// Add certificates leniently: an unparsable or rejected entry is skipped,
/// as Java's store loaders skip entries they cannot decode.
fn add_lenient(roots: &mut RootCertStore, certs: impl IntoIterator<Item = CertificateDer<'static>>) {
    for cert in certs {
        let _ = roots.add(cert);
    }
}

#[cfg(not(target_os = "android"))]
fn platform_roots() -> RootCertStore {
    let mut roots = RootCertStore::empty();
    add_lenient(&mut roots, rustls_native_certs::load_native_certs().certs);
    if roots.is_empty() {
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }
    roots
}

#[cfg(target_os = "android")]
fn platform_roots() -> RootCertStore {
    let mut roots = RootCertStore::empty();
    // SAFETY: getuid has no preconditions.
    let user = unsafe { libc::getuid() } / 100_000;
    let removed = format!("/data/misc/user/{user}/cacerts-removed");
    add_lenient(&mut roots, android_dir_certs(&android_system_dir(), &removed));
    if roots.is_empty() {
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }
    roots
}

/// Conscrypt prefers the updatable APEX store when it exists (Android 14+).
#[cfg(target_os = "android")]
fn android_system_dir() -> String {
    let apex = "/apex/com.android.conscrypt/cacerts";
    let populated = std::fs::read_dir(apex).map(|mut d| d.next().is_some()).unwrap_or(false);
    if populated { apex.into() } else { "/system/etc/security/cacerts".into() }
}

/// Certificates in an Android `cacerts` directory (`<hash>.<n>` files, PEM
/// followed by a text dump), skipping names present in `removed`.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn android_dir_certs(dir: &str, removed: &str) -> Vec<CertificateDer<'static>> {
    let removed: std::collections::HashSet<_> = std::fs::read_dir(removed)
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<_> = entries.flatten().map(|e| e.file_name()).filter(|n| !removed.contains(n)).collect();
    names.sort();
    let mut certs = Vec::new();
    for name in names {
        let Ok(bytes) = std::fs::read(std::path::Path::new(dir).join(&name)) else { continue };
        certs.extend(CertificateDer::pem_slice_iter(&bytes).flatten());
    }
    certs
}

/// Java-style exception for a TLS failure during the handshake with `host`.
pub(crate) fn handshake_error(e: &rustls::Error, host: &str) -> Error {
    use rustls::CertificateError as C;
    let android = cfg!(target_os = "android");
    let handshake = |m: String| Error::new(ErrorKind::SslHandshake, m);
    match e {
        rustls::Error::InvalidCertificate(cert) => match cert {
            C::NotValidForName | C::NotValidForNameContext { .. } => {
                if android {
                    Error::new(ErrorKind::SslPeerUnverified, format!("Hostname {host} not verified"))
                } else if host.parse::<std::net::IpAddr>().is_ok() {
                    handshake(format!("No subject alternative names matching IP address {host} found"))
                } else {
                    handshake(format!("No subject alternative DNS name matching {host} found."))
                }
            }
            C::Expired | C::ExpiredContext { .. } | C::NotValidYet | C::NotValidYetContext { .. } => handshake(if android {
                "java.security.cert.CertPathValidatorException: timestamp check failed".into()
            } else {
                "PKIX path validation failed: java.security.cert.CertPathValidatorException: validity check failed".into()
            }),
            C::UnknownIssuer | C::BadSignature | C::UnhandledCriticalExtension | C::InvalidPurpose => {
                handshake(if android {
                    "java.security.cert.CertPathValidatorException: Trust anchor for certification path not found.".into()
                } else {
                    "PKIX path building failed: sun.security.provider.certpath.SunCertPathBuilderException: unable to find valid certification path to requested target".into()
                })
            }
            other => handshake(format!("Certificate rejected: {other:?}")),
        },
        rustls::Error::AlertReceived(alert) => handshake(format!("Received fatal alert: {}", snake(&format!("{alert:?}")))),
        other => handshake(other.to_string()),
    }
}

/// `HandshakeFailure` → `handshake_failure` (TLS alert names as the JDK prints them).
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_directory_layout_and_removed_list() {
        let ca = rcgen::generate_simple_self_signed(vec!["a.test".into()]).unwrap();
        let other = rcgen::generate_simple_self_signed(vec!["b.test".into()]).unwrap();
        let root = std::env::temp_dir().join(format!("notemaker-cacerts-{}", uuid::Uuid::new_v4()));
        let dir = root.join("cacerts");
        let removed = root.join("removed");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&removed).unwrap();
        // Android files: PEM block followed by `openssl x509 -text` output.
        std::fs::write(dir.join("aaaa0000.0"), format!("{}Certificate:\n    Data: ...\n", ca.cert.pem())).unwrap();
        std::fs::write(dir.join("bbbb0000.0"), other.cert.pem()).unwrap();
        std::fs::write(removed.join("bbbb0000.0"), other.cert.pem()).unwrap();
        let certs = android_dir_certs(dir.to_str().unwrap(), removed.to_str().unwrap());
        assert_eq!(certs.len(), 1);
        assert_eq!(certs[0].as_ref(), ca.cert.der().as_ref());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn pem_errors_and_platform_store() {
        assert!(TlsConfig::with_roots_pem(b"not pem").unwrap_err().message().contains("No certificates"));
        assert!(!platform_roots().is_empty());
        assert_eq!(snake("HandshakeFailure"), "handshake_failure");
    }
}
