//! 从 HTTP 请求里取出用户访问的域名。
//!
//! 医院内网与外网共用一套 Pointer 服务、两个域名，技能脚本要据此选择财务系统
//! 链接。服务部署在 nginx 后面，所以原始域名可能只在 `X-Forwarded-Host` 里。
//!
//! 取值顺序：`X-Forwarded-Host` → `Host`。都没有则返回 `None` —— 宁可让脚本
//! 拿到空值自己回退，也不要编一个域名出来。

use axum::http::{header, HeaderMap};

/// 代理链会用逗号拼接多级 Host，取最左边（最接近客户端）的那一个。
fn first_hop(value: &str) -> &str {
    value.split(',').next().unwrap_or(value).trim()
}

/// 去掉 `:8080` 这类端口。只在冒号后面全是数字时才截断，
/// 避免把 IPv6 字面量（`[::1]`）误伤成 `[`。
fn strip_port(host: &str) -> &str {
    match host.rsplit_once(':') {
        Some((head, port))
            if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) =>
        {
            head
        }
        _ => host,
    }
}

/// 归一化一个 Host 头取值。空值、只有端口、全是空白都返回 `None`。
pub fn normalize_host(raw: &str) -> Option<String> {
    let host = strip_port(first_hop(raw)).trim();
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// 从请求头推导用户访问的域名。
pub fn request_access_host(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|value| value.to_str().ok())
        .and_then(normalize_host)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }
        map
    }

    #[test]
    fn prefers_forwarded_host_over_host() {
        let map = headers(&[
            ("x-forwarded-host", "pointer.hospital.example"),
            ("host", "127.0.0.1:3000"),
        ]);
        assert_eq!(
            request_access_host(&map).as_deref(),
            Some("pointer.hospital.example")
        );
    }

    #[test]
    fn falls_back_to_host_when_proxy_header_absent() {
        let map = headers(&[("host", "pointer.example.com")]);
        assert_eq!(
            request_access_host(&map).as_deref(),
            Some("pointer.example.com")
        );
    }

    #[test]
    fn takes_first_hop_of_a_proxy_chain() {
        let map = headers(&[("x-forwarded-host", "outer.example.com, inner.local")]);
        assert_eq!(request_access_host(&map).as_deref(), Some("outer.example.com"));
    }

    #[test]
    fn strips_port_and_lowercases() {
        assert_eq!(
            normalize_host("Pointer.Example.COM:8443").as_deref(),
            Some("pointer.example.com")
        );
    }

    #[test]
    fn keeps_ipv6_literals_intact() {
        assert_eq!(normalize_host("[::1]").as_deref(), Some("[::1]"));
        assert_eq!(normalize_host("[::1]:8080").as_deref(), Some("[::1]"));
    }

    #[test]
    fn empty_or_blank_values_yield_none() {
        assert_eq!(normalize_host(""), None);
        assert_eq!(normalize_host("   "), None);
        assert_eq!(normalize_host(":8080"), None);
    }

    #[test]
    fn missing_headers_yield_none() {
        assert_eq!(request_access_host(&HeaderMap::new()), None);
    }
}
