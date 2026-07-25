//! One-time rustls crypto provider selection (required for rustls 0.23+).

use std::sync::Once;

static RUSTLS_INIT: Once = Once::new();

/// Install the process-default rustls crypto provider (`ring`).
///
/// When multiple backends are linked (e.g. `reqwest` + `tokio-tungstenite` + `ossify`),
/// rustls cannot auto-select and TLS handshakes panic unless a provider is installed first.
/// Safe to call multiple times.
pub fn ensure_rustls_crypto_provider() {
    RUSTLS_INIT.call_once(
        || match rustls::crypto::ring::default_provider().install_default() {
            Ok(()) => log::debug!("tls: installed rustls ring CryptoProvider"),
            Err(e) => log::warn!("tls: rustls CryptoProvider install skipped: {e:?}"),
        },
    );
}
