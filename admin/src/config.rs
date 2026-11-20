//! 启动配置，全部来自环境变量。
//!
//! 支持从 `.env` 补齐（见 [`load_env_file`]）：**OS 环境优先，文件只填空缺**，
//! 与 `pointer-server` 的语义一致。仍然不读结构化配置文件，避免多一处漂移。

use std::path::{Path, PathBuf};

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
    /// 服务端回跳白名单（可多个：多套 pointer-server 部署 / 内外网地址各一份）。
    /// 环境变量 `POINTER_ADMIN_SERVER_REDIRECT` 用逗号或换行分隔。
    pub server_redirect_uris: Vec<String>,
    /// 对外基址（自动更新清单里拼下载链接用）。留空则按请求的 Host 推。
    pub public_base: Option<String>,
    /// 登录是否必须过验证码。默认开；只有自动化测试/内网联调才关。
    /// 关掉时启动日志会打一条 WARN，避免悄悄跑在生产上。
    pub captcha_required: bool,
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
            server_redirect_uris: vec![DEFAULT_SERVER_REDIRECT.to_string()],
            public_base: None,
            captcha_required: true,
        }
    }
}

/// 服务端回跳的默认值（本机 pointer-server）。
pub const DEFAULT_SERVER_REDIRECT: &str = "http://127.0.0.1:8787/auth/callback";

/// 逗号或换行分隔 → 去空白、去空项。URI 里不会出现这两种分隔符，按它们切是安全的。
pub fn split_redirect_uris(raw: &str) -> Vec<String> {
    raw.split([',', '\n', '\r'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
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
            let uris = split_redirect_uris(&v);
            if !uris.is_empty() {
                cfg.server_redirect_uris = uris;
            }
        }
        // 只认明确的关闭值；写错/留空一律按「开启」处理，宁可多一道验证码。
        if let Some(v) = env_str("POINTER_ADMIN_PUBLIC_BASE") {
            cfg.public_base = Some(v.trim().to_string()).filter(|v| !v.is_empty());
        }
        if let Some(v) = env_str("POINTER_ADMIN_CAPTCHA") {
            cfg.captcha_required = !matches!(v.trim().to_ascii_lowercase().as_str(), "off" | "0" | "false" | "disabled");
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

// ---------------------------------------------------------------------------
// `.env` 支持
// ---------------------------------------------------------------------------

/// 解析 dotenv 内容。规则刻意保守，只认最常见的写法：
///
/// - `KEY=VALUE`，允许 `export ` 前缀
/// - 空行与 `#` 开头的行跳过
/// - 值两侧的成对单/双引号会被剥掉
/// - 值里的 `#` 不当作注释（避免把密码里的 `#` 截断）
///
/// 不做变量插值——配置项里没有这种需求，多一层求值只会多一处猜。
pub fn parse_env_content(content: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || key.contains(char::is_whitespace) {
            continue;
        }
        let mut value = value.trim();
        for quote in ['"', '\''] {
            if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
                value = &value[1..value.len() - 1];
                break;
            }
        }
        out.push((key.to_string(), value.to_string()));
    }
    out
}

/// 把文件里的键值补进进程环境。已存在且非空的变量**不覆盖**（OS 环境优先）。
///
/// 返回实际写入的键名，供启动日志打印（值不打印，避免泄露密钥）。
pub fn apply_env_file(path: &Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut applied = Vec::new();
    for (key, value) in parse_env_content(&content) {
        let occupied = std::env::var(&key).map(|v| !v.trim().is_empty()).unwrap_or(false);
        if occupied {
            continue;
        }
        std::env::set_var(&key, value);
        applied.push(key);
    }
    applied
}

/// 启动时加载 `.env`：`POINTER_ADMIN_ENV_FILE` 指定路径优先，否则用工作目录下的 `.env`。
///
/// 文件不存在时静默跳过——裸部署完全可以只用 systemd / shell 注入环境变量。
pub fn load_env_file() {
    let explicit = std::env::var("POINTER_ADMIN_ENV_FILE")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let path = match explicit {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(".env"),
    };
    if !path.is_file() {
        return;
    }
    let applied = apply_env_file(&path);
    if !applied.is_empty() {
        log::info!(
            "已从 {} 补充环境变量：{}",
            path.display(),
            applied.join(", ")
        );
    }
}

#[cfg(test)]
mod redirect_uris_tests {
    use super::*;

    #[test]
    fn splits_on_comma_and_newline() {
        assert_eq!(
            split_redirect_uris("https://a/cb, https://b/cb\nhttps://c/cb"),
            vec![
                "https://a/cb".to_string(),
                "https://b/cb".to_string(),
                "https://c/cb".to_string(),
            ]
        );
    }

    #[test]
    fn single_value_still_works_and_blank_is_dropped() {
        assert_eq!(split_redirect_uris("https://a/cb"), vec!["https://a/cb".to_string()]);
        assert!(split_redirect_uris("  ,  \n ").is_empty());
    }
}

#[cfg(test)]
mod env_file_tests {
    use super::*;

    #[test]
    fn parses_basic_pairs_and_skips_noise() {
        let got = parse_env_content(
            "# comment\n\nPOINTER_ADMIN_ADDR=127.0.0.1:8788\nexport POINTER_ADMIN_TENANT=acme\n",
        );
        assert_eq!(
            got,
            vec![
                ("POINTER_ADMIN_ADDR".to_string(), "127.0.0.1:8788".to_string()),
                ("POINTER_ADMIN_TENANT".to_string(), "acme".to_string()),
            ]
        );
    }

    #[test]
    fn strips_matching_quotes_but_keeps_inner_hash() {
        let got = parse_env_content(
            "A=\"has space\"\nB='x#y'\nC=p#q\nD=\"mismatched'\n",
        );
        let map: std::collections::HashMap<_, _> = got.into_iter().collect();
        assert_eq!(map["A"], "has space");
        assert_eq!(map["B"], "x#y");
        // 值里的 # 不当注释
        assert_eq!(map["C"], "p#q");
        // 引号不配对时不剥
        assert_eq!(map["D"], "\"mismatched'");
    }

    #[test]
    fn ignores_lines_without_equals_or_with_bad_key() {
        let got = parse_env_content("NOVALUE\n=noname\nBAD KEY=v\nOK=v\n");
        assert_eq!(got, vec![("OK".to_string(), "v".to_string())]);
    }

    #[test]
    fn os_env_wins_over_file() {
        // 用独立键名，避免和其它测试抢进程环境。
        std::env::set_var("POINTER_ADMIN_TEST_OS_WINS", "from-os");
        let dir = std::env::temp_dir().join(format!("pointer-admin-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(".env");
        std::fs::write(
            &file,
            "POINTER_ADMIN_TEST_OS_WINS=from-file\nPOINTER_ADMIN_TEST_FROM_FILE=1\n",
        )
        .unwrap();
        let applied = apply_env_file(&file);
        assert_eq!(std::env::var("POINTER_ADMIN_TEST_OS_WINS").unwrap(), "from-os");
        assert_eq!(std::env::var("POINTER_ADMIN_TEST_FROM_FILE").unwrap(), "1");
        assert_eq!(applied, vec!["POINTER_ADMIN_TEST_FROM_FILE".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
