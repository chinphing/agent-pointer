//! Client IP resolution for login rate limiting.
//!
//! `X-Forwarded-For` is attacker-controlled, so it is only read when the direct
//! peer is one of the configured `trusted_proxies`. Everything else rate-limits
//! on the socket address. When a proxy chain is trusted, the address used is the
//! rightmost hop we do **not** control — appending a fake leftmost entry must not
//! buy a fresh bucket.

use axum::http::HeaderMap;
use std::net::IpAddr;

/// The address a request should be rate-limited on.
pub fn resolve_client_ip(
    peer: Option<IpAddr>,
    headers: &HeaderMap,
    trusted_proxies: &[String],
) -> Option<IpAddr> {
    let peer = canonical(peer?);
    if trusted_proxies.is_empty() || !trusted_proxies.iter().any(|spec| ip_matches(peer, spec)) {
        return Some(peer);
    }
    let chain = forwarded_chain(headers);
    if chain.is_empty() {
        return Some(peer);
    }
    for hop in chain.iter().rev() {
        if !trusted_proxies.iter().any(|spec| ip_matches(*hop, spec)) {
            return Some(*hop);
        }
    }
    // Every hop is one of ours: fall back to the origin the chain started at.
    Some(chain[0])
}

/// Stable key for limiter maps (`::ffff:1.2.3.4` and `1.2.3.4` share a bucket).
pub fn client_key(ip: Option<IpAddr>) -> String {
    match ip {
        Some(ip) => canonical(ip).to_string(),
        // No peer address (unix socket / tests): one shared bucket is still
        // better than no limit at all.
        None => "unknown".to_string(),
    }
}

fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        v4 => v4,
    }
}

fn forwarded_chain(headers: &HeaderMap) -> Vec<IpAddr> {
    let mut out = Vec::new();
    for value in headers.get_all("x-forwarded-for").iter() {
        let Ok(text) = value.to_str() else {
            continue;
        };
        for part in text.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            // Hop entries may be `ip:port` or `[v6]:port`.
            let candidate = part
                .strip_prefix('[')
                .and_then(|rest| rest.split_once(']'))
                .map(|(host, _)| host)
                .unwrap_or(part);
            let candidate = candidate
                .split_once(':')
                .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
                .map(|(host, _)| host)
                .unwrap_or(candidate);
            if let Ok(ip) = candidate.parse::<IpAddr>() {
                out.push(canonical(ip));
            }
        }
    }
    out
}

/// Match an address against an exact IP or a `addr/prefix` CIDR spec.
pub fn ip_matches(ip: IpAddr, spec: &str) -> bool {
    let spec = spec.trim();
    if spec.is_empty() {
        return false;
    }
    let (addr, prefix) = match spec.split_once('/') {
        Some((addr, prefix)) => match prefix.trim().parse::<u8>() {
            Ok(prefix) => (addr, Some(prefix)),
            Err(_) => return false,
        },
        None => (spec, None),
    };
    let Ok(base) = addr.trim().parse::<IpAddr>() else {
        return false;
    };
    match (canonical(base), canonical(ip)) {
        (IpAddr::V4(base), IpAddr::V4(ip)) => {
            let prefix = prefix.unwrap_or(32);
            if prefix > 32 {
                return false;
            }
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            (u32::from(base) & mask) == (u32::from(ip) & mask)
        }
        (IpAddr::V6(base), IpAddr::V6(ip)) => {
            let prefix = prefix.unwrap_or(128);
            if prefix > 128 {
                return false;
            }
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            (u128::from(base) & mask) == (u128::from(ip) & mask)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("valid ip")
    }

    fn headers_with_xff(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn untrusted_peer_ignores_the_forwarded_header() {
        let headers = headers_with_xff("1.2.3.4");
        let resolved = resolve_client_ip(Some(ip("198.51.100.9")), &headers, &[]);
        assert_eq!(resolved, Some(ip("198.51.100.9")));

        // A configured list that does not contain the peer changes nothing.
        let resolved = resolve_client_ip(
            Some(ip("198.51.100.9")),
            &headers,
            &["10.0.0.1".to_string()],
        );
        assert_eq!(resolved, Some(ip("198.51.100.9")));
    }

    #[test]
    fn trusted_proxy_uses_the_rightmost_untrusted_hop() {
        // peer 10.0.0.1 (ours) ← 10.0.0.2 (ours) ← 203.0.113.7 (client)
        let trusted = ["10.0.0.0/8".to_string()];
        let headers = headers_with_xff("203.0.113.7, 10.0.0.2");
        assert_eq!(
            resolve_client_ip(Some(ip("10.0.0.1")), &headers, &trusted),
            Some(ip("203.0.113.7"))
        );

        // A spoofed leftmost entry must not move the rate-limit bucket.
        let headers = headers_with_xff("1.2.3.4, 203.0.113.7, 10.0.0.2");
        assert_eq!(
            resolve_client_ip(Some(ip("10.0.0.1")), &headers, &trusted),
            Some(ip("203.0.113.7"))
        );
    }

    #[test]
    fn trusted_proxy_skips_its_own_hops() {
        let headers = headers_with_xff("203.0.113.7, 10.0.0.2, 10.0.0.3");
        let resolved = resolve_client_ip(
            Some(ip("10.0.0.1")),
            &headers,
            &["10.0.0.0/8".to_string()],
        );
        assert_eq!(resolved, Some(ip("203.0.113.7")));
    }

    #[test]
    fn all_hops_trusted_falls_back_to_the_origin() {
        let headers = headers_with_xff("10.0.0.5, 10.0.0.6");
        let resolved = resolve_client_ip(
            Some(ip("10.0.0.1")),
            &headers,
            &["10.0.0.0/8".to_string()],
        );
        assert_eq!(resolved, Some(ip("10.0.0.5")));
    }

    #[test]
    fn trusted_proxy_without_the_header_uses_the_peer() {
        let resolved = resolve_client_ip(
            Some(ip("10.0.0.1")),
            &HeaderMap::new(),
            &["10.0.0.1".to_string()],
        );
        assert_eq!(resolved, Some(ip("10.0.0.1")));
    }

    #[test]
    fn ports_and_ipv6_entries_are_parsed() {
        let headers = headers_with_xff("203.0.113.7:5555, [2001:db8::1]:443");
        let resolved = resolve_client_ip(
            Some(ip("10.0.0.1")),
            &headers,
            &["10.0.0.1".to_string(), "10.0.0.1/32".to_string()],
        );
        assert_eq!(resolved, Some(ip("2001:db8::1")));
    }

    #[test]
    fn ipv4_mapped_addresses_share_a_bucket() {
        assert_eq!(client_key(Some(ip("::ffff:1.2.3.4"))), "1.2.3.4");
        assert_eq!(client_key(None), "unknown");
        assert!(ip_matches(ip("::ffff:10.1.2.3"), "10.0.0.0/8"));
    }

    #[test]
    fn cidr_matching_is_correct() {
        assert!(ip_matches(ip("10.1.2.3"), "10.0.0.0/8"));
        assert!(!ip_matches(ip("11.1.2.3"), "10.0.0.0/8"));
        assert!(ip_matches(ip("192.168.4.4"), "192.168.4.0/24"));
        assert!(!ip_matches(ip("192.168.5.4"), "192.168.4.0/24"));
        assert!(ip_matches(ip("8.8.8.8"), "8.8.8.8"));
        assert!(!ip_matches(ip("8.8.8.9"), "8.8.8.8"));
        assert!(ip_matches(ip("2001:db8::5"), "2001:db8::/32"));
        assert!(!ip_matches(ip("2001:db9::5"), "2001:db8::/32"));
        // Garbage specs never match, and never panic.
        assert!(!ip_matches(ip("1.2.3.4"), ""));
        assert!(!ip_matches(ip("1.2.3.4"), "1.2.3.4/99"));
        assert!(!ip_matches(ip("1.2.3.4"), "not-an-ip"));
    }
}
