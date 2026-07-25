//! SSRF guards for `web_fetch` (http/https only; block private/metadata targets).

use anyhow::{anyhow, bail, Result};
use reqwest::Url;
use std::cell::Cell;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

thread_local! {
    static ALLOW_LOOPBACK_FOR_TESTS: Cell<bool> = const { Cell::new(false) };
}

/// Test-only: allow loopback targets (wiremock). Not for production use.
#[cfg(test)]
pub struct AllowLoopbackGuard;

#[cfg(test)]
impl AllowLoopbackGuard {
    pub fn enter() -> Self {
        ALLOW_LOOPBACK_FOR_TESTS.with(|c| c.set(true));
        Self
    }
}

#[cfg(test)]
impl Drop for AllowLoopbackGuard {
    fn drop(&mut self) {
        ALLOW_LOOPBACK_FOR_TESTS.with(|c| c.set(false));
    }
}

pub(super) fn loopback_allowed() -> bool {
    ALLOW_LOOPBACK_FOR_TESTS.with(|c| c.get())
}

/// Apply test loopback allowance on a worker thread (thread-local does not inherit).
#[cfg(test)]
pub(super) fn enter_loopback_allowance_if(allowed: bool) -> Option<AllowLoopbackGuard> {
    if allowed {
        Some(AllowLoopbackGuard::enter())
    } else {
        None
    }
}

#[cfg(not(test))]
pub(super) fn enter_loopback_allowance_if(_allowed: bool) -> Option<()> {
    None
}

const BLOCKED_HOST_SUFFIXES: &[&str] = &[".local", ".internal", ".intranet", ".corp", ".lan"];
const BLOCKED_HOSTS: &[&str] = &[
    "localhost",
    "metadata.google.internal",
    "metadata.goog",
    "kubernetes.default",
    "kubernetes.default.svc",
];

/// Validate that `raw` is a safe http(s) URL before connecting (and after redirects).
pub fn assert_url_safe(raw: &str) -> Result<Url> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("url is empty");
    }
    let url = Url::parse(trimmed).map_err(|e| anyhow!("invalid url: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("only http and https URLs are allowed");
    }
    if url.username() != "" || url.password().is_some() {
        bail!("URLs with embedded credentials are not allowed");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("url missing host"))?
        .trim()
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    if host.is_empty() {
        bail!("url missing host");
    }
    if BLOCKED_HOSTS.iter().any(|h| host == *h) {
        if !(loopback_allowed() && host == "localhost") {
            bail!("host '{host}' is blocked");
        }
    }
    if BLOCKED_HOST_SUFFIXES
        .iter()
        .any(|suffix| host.ends_with(suffix))
    {
        bail!("host '{host}' is blocked");
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_blocked_ip(ip) {
            bail!("IP address {ip} is not allowed");
        }
        return Ok(url);
    }
    // Resolve DNS and reject if any answer is private/metadata.
    let port = url.port_or_known_default().unwrap_or(80);
    let addrs = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| anyhow!("DNS lookup failed for {host}: {e}"))?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        bail!("DNS lookup returned no addresses for {host}");
    }
    for addr in &addrs {
        if is_blocked_ip(addr.ip()) {
            bail!("host '{host}' resolves to blocked address {}", addr.ip());
        }
    }
    Ok(url)
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => is_blocked_v6(v6),
    }
}

fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    if loopback_allowed() && ip.is_loopback() {
        return false;
    }
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.octets()[0] == 0
        // Carrier-grade NAT / shared
        || matches!(ip.octets(), [100, 64..=127, ..])
        // Benchmark / SIIT (often used by fake-IP proxies; keep blocked by default)
        || matches!(ip.octets(), [198, 18..=19, ..])
        // Cloud metadata
        || ip == Ipv4Addr::new(169, 254, 169, 254)
}

fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_blocked_v4(v4);
    }
    if loopback_allowed() && ip.is_loopback() {
        return false;
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || (ip.segments()[0] & 0xffc0) == 0xfe80 // link-local
        || (ip.segments()[0] & 0xff00) == 0xff00 // multicast
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_schemes() {
        assert!(assert_url_safe("file:///etc/passwd").is_err());
        assert!(assert_url_safe("ftp://example.com/a").is_err());
    }

    #[test]
    fn rejects_localhost_and_private_literals() {
        assert!(assert_url_safe("http://localhost/a").is_err());
        assert!(assert_url_safe("http://127.0.0.1/a").is_err());
        assert!(assert_url_safe("http://10.0.0.1/a").is_err());
        assert!(assert_url_safe("http://192.168.1.1/a").is_err());
        assert!(assert_url_safe("http://169.254.169.254/latest").is_err());
    }

    #[test]
    fn test_guard_allows_loopback_only_while_active() {
        assert!(assert_url_safe("http://127.0.0.1/a").is_err());
        let _g = AllowLoopbackGuard::enter();
        assert!(assert_url_safe("http://127.0.0.1/a").is_ok());
        drop(_g);
        assert!(assert_url_safe("http://127.0.0.1/a").is_err());
    }

    #[test]
    fn rejects_credentials_in_url() {
        assert!(assert_url_safe("https://user:pass@example.com/a").is_err());
    }

    #[test]
    fn accepts_public_https() {
        // May fail offline if DNS is unavailable — skip soft.
        match assert_url_safe("https://example.com/path") {
            Ok(u) => assert_eq!(u.host_str(), Some("example.com")),
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains("DNS lookup failed"), "unexpected error: {msg}");
            }
        }
    }
}
