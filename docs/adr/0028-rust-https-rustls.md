# ADR 0028: HTTPS in the Rust port with rustls

## Status

Accepted (2026-11-02). Refines ADR 0026 for `https://` peers.

## Context

Java's `Endpoint` and the Android chat client used `HttpURLConnection`, which
accepted `https://` URLs and verified them against the platform trust store:
the JDK `cacerts` on hosts and Conscrypt's system CA directory on Android. The
first Rust port of `Endpoint` (`notemaker_core::endpoint`) spoke plain HTTP
over `std::net` and rejected `https://` targets with an I/O error. HTTPS proxy
URLs and upstreams therefore regressed in the CLI, the local proxy and the
Android app.

## Decision

- `HttpConnection` keeps its hand-written blocking HTTP/1.1 client. For
  `https://` it performs a rustls handshake on the connected socket before
  writing the request. `http://` requests write the same bytes as before.
- rustls uses the **ring** provider, without default features. ring builds with
  the NDK clang under `cargo ndk` for `arm64-v8a` and `x86_64`. aws-lc-rs would
  also need cmake and possibly nasm in `shell.nix`.
- `notemaker_core::tls::TlsConfig::platform()` builds the trust store once per
  process:
  - **Android**: the PEM files in `/apex/com.android.conscrypt/cacerts`
    (Android 14 and later), or otherwise `/system/etc/security/cacerts`.
    System CAs that the user disabled are skipped when
    `/data/misc/user/<id>/cacerts-removed` is readable. User-added CAs are not
    trusted. This matches the default network security config for
    `targetSdkVersion >= 24`, and the manifest declares no
    `networkSecurityConfig`. `rustls-platform-verifier` was rejected because it
    needs JNI initialization and Java/Kotlin support classes in the shim.
  - **Linux and other hosts**: `rustls-native-certs` (`SSL_CERT_FILE`,
    `SSL_CERT_DIR` or the distribution bundle, which is NixOS's
    `/etc/ssl/certs/ca-certificates.crt`).
  - If no platform roots load, `webpki-roots` (Mozilla) is used, in the way the
    JDK ships its own `cacerts`.
  - `NOTEMAKER_EXTRA_CA_FILE=<pem>` adds CAs on every platform. It replaces
    `-Djavax.net.ssl.trustStore` for self-signed home-server certificates.
- Hostname verification is always on, for DNS names and IP literals.
- Handshake failures use new `ErrorKind`s, `SslHandshake`
  (`javax.net.ssl.SSLHandshakeException`) and `SslPeerUnverified`
  (`javax.net.ssl.SSLPeerUnverifiedException`, Android hostname mismatch). Both
  are `IOException`s for `is_io()`. They carry the Java messages:
  - Unknown issuer: `PKIX path building failed: …unable to find valid
    certification path to requested target` on hosts, and
    `java.security.cert.CertPathValidatorException: Trust anchor for
    certification path not found.` on Android.
  - Hostname mismatch: `No subject alternative DNS name matching H found.` or
    `No subject alternative names matching IP address H found` on hosts, and
    `Hostname H not verified` on Android.
  - Expired certificates and received alerts (`Received fatal alert: …`) also
    get Java messages. Other errors use the rustls text.
  `LocalProxy`'s offline check matches only `ErrorKind::Io`, so a TLS failure
  is reported, not treated as an offline upstream. Java behaved the same way.
- A TLS peer that closes without `close_notify` ends the stream, as the JDK
  allowed. Content-Length and chunked framing still report `Premature EOF`.
- `HttpConnection::set_tls_config` and `Endpoint::with_tls_config` play the
  role of `setSSLSocketFactory` for custom roots. Tests use them.

## Consequences

- New crates are `rustls`, `rustls-pki-types`, `rustls-native-certs`
  (non-Android only) and `webpki-roots`, with `rcgen` as a dev-dependency.
  `docs/design/rust-port.md` lists them as allowed.
- There is no TLS session resumption across requests, because every request
  uses `Connection: close`, as before. There are no client certificates and no
  `javax.net.ssl.*` system properties.
- `packages/core/tests/https.rs` covers GET, POST, chunked and read-to-close
  bodies, SSE streaming with disconnect, the extra-CA variable, an untrusted
  issuer, hostname mismatches and a plain server on an `https://` URL. It uses
  a local rustls server and an rcgen CA. The Android CA directory parser has a
  unit test with an Android-style file layout. Reading the real device store
  still needs an emulator or tablet run.
