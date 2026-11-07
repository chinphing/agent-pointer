//! 启动配置，全部来自环境变量（不读配置文件，避免多一处漂移）。

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AdminConfig {
    pub addr: String,
    pub tenant_id: String,
    /// 管理端网页会话有效期。
    pub session_ttl_secs: i64,
    /// 下发给客户端/server 的 access token 有效期。
    pub access_token_ttl_secs: i64,
    /// refresh token 有效期。
    pub refresh_token_ttl_secs: i64,
    pub cookie_name: String,
    pub cookie_secure: bool,
    pub static_dir: Option<PathBuf>,
    /// 技能包等二进制的落盘根目录（默认 `admin/data`）。
    pub data_dir: PathBuf,
    pub bootstrap_user: Option<String>,
    pub bootstrap_password: Option<String>,
    /// 未配置时 OAuth 端点返回 503，而不是拿一个随机密钥糊过去。
    pub oauth_secret: Option<String>,
    pub server_redirect_uri: String,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1:8788".to_string(),
            tenant_id: "default".to_string(),
            session_ttl_secs: 12 * 3600,
            access_token_ttl_secs: 3600,
            refresh_token_ttl_secs: 30 * 24 * 3600,
            cookie_name: "pointer_admin_session".to_string(),
            cookie_secure: false,
            static_dir: None,
            data_dir: PathBuf::from("admin/data"),
            bootstrap_user: None,
            bootstrap_password: None,
            oauth_secret: None,
            server_redirect_uri: "http://127.0.0.1:8787/auth/callback".to_string(),
        }
    }
}

fn env_str(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_i64(key: &str) -> Option<i64> {
    env_str(key).and_then(|v| v.parse::<i64>().ok())
}

fn env_bool(key: &str) -> Option<bool> {
    env_str(key).map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes"))
}

impl AdminConfig {
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Some(v) = env_str("POINTER_ADMIN_ADDR") {
            cfg.addr = v;
        }
        if let Some(v) = env_str("POINTER_ADMIN_TENANT") {
            cfg.tenant_id = v;
        }
        if let Some(v) = env_i64("POINTER_ADMIN_SESSION_TTL") {
            cfg.session_ttl_secs = v;
        }
        if let Some(v) = env_i64("POINTER_ADMIN_ACCESS_TTL") {
            cfg.access_token_ttl_secs = v;
        }
        if let Some(v) = env_i64("POINTER_ADMIN_REFRESH_TTL") {
            cfg.refresh_token_ttl_secs = v;
        }
        if let Some(v) = env_str("POINTER_ADMIN_COOKIE_NAME") {
            cfg.cookie_name = v;
        }
        if let Some(v) = env_bool("POINTER_ADMIN_COOKIE_SECURE") {
            cfg.cookie_secure = v;
        }
        if let Some(v) = env_str("POINTER_ADMIN_STATIC_DIR") {
            cfg.static_dir = Some(PathBuf::from(v));
        } else {
            let candidate = PathBuf::from("admin/web/dist");
            if candidate.is_dir() {
                cfg.static_dir = Some(candidate);
            }
        }
        if let Some(v) = env_str("POINTER_ADMIN_DATA_DIR") {
            cfg.data_dir = PathBuf::from(v);
        }
        cfg.bootstrap_user = env_str("POINTER_ADMIN_BOOTSTRAP_USER");
        cfg.bootstrap_password = env_str("POINTER_ADMIN_BOOTSTRAP_PASSWORD");
        cfg.oauth_secret = env_str("POINTER_ADMIN_OAUTH_SECRET");
        if let Some(v) = env_str("POINTER_ADMIN_SERVER_REDIRECT") {
            cfg.server_redirect_uri = v;
        }
        cfg
    }

    pub fn db_url(&self) -> String {
        env_str("POINTER_ADMIN_DB_URL").unwrap_or_else(|| "sqlite:pointer-admin.db".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_loopback_and_short_lived() {
        let cfg = AdminConfig::default();
        assert!(cfg.addr.starts_with("127.0.0.1:"));
        assert_eq!(cfg.cookie_name, "pointer_admin_session");
        assert!(!cfg.cookie_secure, "本地 http 默认不加 Secure");
        assert!(cfg.oauth_secret.is_none(), "未配置时必须显式为 None");
    }
}
