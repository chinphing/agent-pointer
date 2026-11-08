//! 客户端 ↔ 控制面 端到端（**不需要 GUI**）。
//!
//! 覆盖桌面端真正会走的那条链路：OAuth 换 token → 拉下发目录 → 落盘 → 启用策略 →
//! 按需安装 → 撤权清理 → 配额拦截。UI 呈现（徽章、按钮、文案）不在这里，那部分只能人看。
//!
//! 前置：控制面已在跑，并设置环境变量
//!
//! ```bash
//! POINTER_ADMIN_ADDR=127.0.0.1:8788 \
//! POINTER_ADMIN_DB_URL=sqlite:/tmp/e2e/admin.db \
//! POINTER_ADMIN_DATA_DIR=/tmp/e2e/data \
//! POINTER_ADMIN_BOOTSTRAP_USER=admin \
//! POINTER_ADMIN_BOOTSTRAP_PASSWORD=admin12345 \
//! POINTER_ADMIN_OAUTH_SECRET=e2e-secret \
//!   ./target/debug/pointer-admin
//!
//! POINTER_ADMIN_E2E_BASE=http://127.0.0.1:8788 \
//! POINTER_ADMIN_E2E_PASSWORD=admin12345 \
//! POINTER_ADMIN_E2E_OAUTH_SECRET=e2e-secret \
//!   cargo test -p pointer-core --test org_sync_e2e -- --nocapture
//! ```
//!
//! 未设置 `POINTER_ADMIN_E2E_BASE` 时整条用例跳过（不影响默认 CI）。

use std::fs;
use std::path::PathBuf;

use pointer_core::platform_auth::PlatformAuthManager;
use pointer_core::skills::org_sync::{self, PackageKind};
use pointer_contract::PlatformSession;
use pointer_contract::PlatformUserSummary;
use serde_json::{json, Value};

const MEMBER: &str = "e2e_member";
const MEMBER_PASSWORD: &str = "member12345";

fn base() -> Option<String> {
    std::env::var("POINTER_ADMIN_E2E_BASE")
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
}

fn admin_password() -> String {
    std::env::var("POINTER_ADMIN_E2E_PASSWORD").unwrap_or_else(|_| "admin12345".to_string())
}

fn unique(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{tag}-{nanos}")
}

fn temp_dir(tag: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("pointer-e2e-{}-{}", tag, std::process::id()));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

// ---------- 控制面 HTTP 辅助 ----------

struct Admin {
    base: String,
    client: reqwest::Client,
    cookie: String,
}

impl Admin {
    async fn login(base: &str, username: &str, password: &str) -> reqwest::Result<Self> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{base}/api/admin/auth/login"))
            .json(&json!({ "username": username, "password": password }))
            .send()
            .await?;
        let cookie = resp
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|raw| raw.split(';').next())
            .collect::<Vec<_>>()
            .join("; ");
        Ok(Self {
            base: base.to_string(),
            client,
            cookie,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    async fn create_resource(&self, body: Value) -> String {
        let resp = self
            .client
            .post(self.url("/api/admin/resources"))
            .header(reqwest::header::COOKIE, &self.cookie)
            .json(&body)
            .send()
            .await
            .expect("create resource");
        let status = resp.status();
        let value: Value = resp.json().await.expect("json");
        assert!(
            status.is_success(),
            "创建资源失败 {status}: {value}"
        );
        value["id"].as_str().expect("id").to_string()
    }

    async fn upload_version(&self, id: &str, version: &str, bytes: Vec<u8>) -> String {
        let resp = self
            .client
            .post(self.url(&format!(
                "/api/admin/resources/{id}/versions?version={version}"
            )))
            .header(reqwest::header::COOKIE, &self.cookie)
            .header(reqwest::header::CONTENT_TYPE, "application/zip")
            .body(bytes)
            .send()
            .await
            .expect("upload");
        let status = resp.status();
        let value: Value = resp.json().await.expect("json");
        assert!(status.is_success(), "上传版本失败 {status}: {value}");
        value["id"].as_str().expect("version id").to_string()
    }

    async fn publish(&self, id: &str, version_id: &str) {
        let resp = self
            .client
            .post(self.url(&format!(
                "/api/admin/resources/{id}/versions/{version_id}/publish"
            )))
            .header(reqwest::header::COOKIE, &self.cookie)
            .send()
            .await
            .expect("publish");
        assert!(resp.status().is_success(), "发布失败: {:?}", resp.status());
    }

    async fn grant_all(&self, id: &str) {
        let resp = self
            .client
            .put(self.url(&format!("/api/admin/resources/{id}/grants")))
            .header(reqwest::header::COOKIE, &self.cookie)
            .json(&json!({ "items": [{ "type": "all" }] }))
            .send()
            .await
            .expect("grant");
        assert!(resp.status().is_success(), "授权失败");
    }

    async fn revoke_all(&self, id: &str) {
        let resp = self
            .client
            .put(self.url(&format!("/api/admin/resources/{id}/grants")))
            .header(reqwest::header::COOKIE, &self.cookie)
            .json(&json!({ "items": [] }))
            .send()
            .await
            .expect("revoke");
        assert!(resp.status().is_success(), "撤销授权失败");
    }

    async fn update_activation(&self, id: &str, activation: &str) {
        let resp = self
            .client
            .put(self.url(&format!("/api/admin/resources/{id}")))
            .header(reqwest::header::COOKIE, &self.cookie)
            .json(&json!({ "activation": activation }))
            .send()
            .await
            .expect("update");
        assert!(resp.status().is_success(), "改启用策略失败");
    }

    async fn ensure_member(&self) -> String {
        let resp = self
            .client
            .get(self.url("/api/admin/users"))
            .header(reqwest::header::COOKIE, &self.cookie)
            .send()
            .await
            .expect("list users");
        let value: Value = resp.json().await.expect("json");
        if let Some(id) = value["items"]
            .as_array()
            .and_then(|items| items.iter().find(|u| u["username"] == MEMBER))
            .and_then(|u| u["id"].as_str())
        {
            return id.to_string();
        }

        let resp = self
            .client
            .post(self.url("/api/admin/users"))
            .header(reqwest::header::COOKIE, &self.cookie)
            .json(&json!({
                "username": MEMBER,
                "display_name": "端到端员工",
                "password": MEMBER_PASSWORD,
                "roles": ["member"]
            }))
            .send()
            .await
            .expect("create user");
        let status = resp.status();
        let value: Value = resp.json().await.expect("json");
        assert!(status.is_success(), "建员工失败 {status}: {value}");
        value["id"].as_str().expect("user id").to_string()
    }

    async fn set_quota_block(&self, user_id: &str, limit: i64, action: &str) {
        let resp = self
            .client
            .put(self.url(&format!("/api/admin/usage/quota/{user_id}")))
            .header(reqwest::header::COOKIE, &self.cookie)
            .json(&json!({ "monthly_token_limit": limit, "over_action": action }))
            .send()
            .await
            .expect("quota");
        assert!(resp.status().is_success(), "设配额失败");
    }
}

/// 走一遍 OAuth，拿到员工 access_token（模拟桌面端登录）。
async fn member_session(base: &str) -> PlatformSession {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client");
    let verifier = "e2e-verifier-0123456789";
    let challenge = {
        use sha2::{Digest, Sha256};
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
    };
    let redirect = "http://127.0.0.1:8799/auth/callback";

    let resp = client
        .post(format!("{base}/oauth/authorize"))
        .form(&[
            ("username", MEMBER),
            ("password", MEMBER_PASSWORD),
            ("client_id", "pointer-desktop"),
            ("redirect_uri", redirect),
            ("state", "e2e"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ])
        .send()
        .await
        .expect("authorize");
    if !resp.status().is_redirection() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        panic!("授权应返回 302，实际 {status}: {}", body.chars().take(300).collect::<String>());
    }
    let location = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("location")
        .to_string();
    let code = location
        .split("code=")
        .nth(1)
        .map(|rest| rest.split('&').next().unwrap_or(rest))
        .expect("code")
        .to_string();

    let resp = client
        .post(format!("{base}/auth/oauth/exchange-code"))
        .json(&json!({
            "client_id": "pointer-desktop",
            "code": code,
            "code_verifier": verifier
        }))
        .send()
        .await
        .expect("exchange");
    let status = resp.status();
    let value: Value = resp.json().await.expect("json");
    assert!(status.is_success(), "换 token 失败 {status}: {value}");

    let token = value["access_token"].as_str().expect("access_token");
    PlatformSession {
        access_token: token.to_string(),
        refresh_token: value["refresh_token"].as_str().unwrap_or("").to_string(),
        expires_at: chrono_like_now() + 3600,
        agent_id: String::new(),
        user: PlatformUserSummary {
            id: String::new(),
            nickname: Some("端到端员工".into()),
            is_platform_admin: false,
            included_tokens: 0,
            consumed_tokens: 0,
            token_quota_exhausted: false,
        },
    }
}

fn chrono_like_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------- 包构造 ----------

fn skill_zip(name: &str, body: &str) -> Vec<u8> {
    use std::io::Write;
    let mut buffer = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("SKILL.md", options).expect("start");
        write!(
            writer,
            "---\nname: {name}\ndescription: E2E 测试技能\n---\n\n{body}\n"
        )
        .expect("write");
        writer.finish().expect("finish");
    }
    buffer
}

fn plugin_zip(plugin_id: &str) -> Vec<u8> {
    use std::io::Write;
    let mut buffer = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file(format!("{plugin_id}/pointer-plugin.toml"), options)
            .expect("start");
        write!(
            writer,
            "[plugin]\nid = \"{plugin_id}\"\nname = \"E2E 插件\"\nversion = \"1.0.0\"\napi_version = \"v1\"\n"
        )
        .expect("write");
        writer.finish().expect("finish");
    }
    buffer
}

// ---------- 用例 ----------

#[tokio::test]
async fn client_syncs_skills_and_plugins_from_control_plane() {
    let Some(base) = base() else {
        eprintln!("skipped: POINTER_ADMIN_E2E_BASE 未设置");
        return;
    };

    // 客户端侧目录隔离：技能走 POINTER_APP_DATA_DIR。
    // 插件目录固定是 `~/.pointer/plugins`（没有环境变量可覆盖），
    // 所以下面用带时间戳的唯一 plugin id，并在结尾清理，避免污染真实目录。
    let data_dir = temp_dir("data");
    std::env::set_var("POINTER_API_BASE", &base);
    std::env::set_var("POINTER_DEPLOYMENT_MODE", "platform");
    std::env::set_var("POINTER_APP_DATA_DIR", &data_dir);

    let admin = Admin::login(&base, "admin", &admin_password())
        .await
        .expect("admin login");
    assert!(!admin.cookie.is_empty(), "登录没拿到 cookie");

    // ---- 造资源 ----
    let provider = admin
        .create_resource(json!({
            "kind": "provider", "slug": unique("e2e-qwen"), "name": "E2E 百炼", "enabled": true,
            "payload": {"baseUrl": "https://example.invalid/v1"}, "secret": "sk-e2e-dummy"
        }))
        .await;
    admin.grant_all(&provider).await;

    let member_id = admin.ensure_member().await;

    // 技能：先 optional，稍后改 default_on
    let skill_slug = unique("e2e-skill");
    let skill = admin
        .create_resource(json!({
            "kind": "skill", "slug": skill_slug, "name": "E2E 技能",
            "enabled": true, "activation": "optional"
        }))
        .await;
    let skill_vid = admin
        .upload_version(&skill, "1.0.0", skill_zip(&skill_slug, "# E2E"))
        .await;
    admin.publish(&skill, &skill_vid).await;
    admin.grant_all(&skill).await;

    // 插件 A：default_on（应自动下载）
    let auto_plugin_id = unique("com.example.auto");
    let auto_plugin = admin
        .create_resource(json!({
            "kind": "plugin", "slug": unique("e2e-auto-plugin"), "name": "E2E 自动插件",
            "enabled": true, "activation": "default_on",
            "payload": {"pluginId": auto_plugin_id}
        }))
        .await;
    let auto_vid = admin
        .upload_version(&auto_plugin, "1.0.0", plugin_zip(&auto_plugin_id))
        .await;
    admin.publish(&auto_plugin, &auto_vid).await;
    admin.grant_all(&auto_plugin).await;

    // 插件 B：optional（不该自动下载）
    let lazy_plugin_id = unique("com.example.lazy");
    let lazy_plugin_slug = unique("e2e-lazy-plugin");
    let lazy_plugin = admin
        .create_resource(json!({
            "kind": "plugin", "slug": lazy_plugin_slug, "name": "E2E 按需插件",
            "enabled": true, "activation": "optional",
            "payload": {"pluginId": lazy_plugin_id}
        }))
        .await;
    let lazy_vid = admin
        .upload_version(&lazy_plugin, "1.0.0", plugin_zip(&lazy_plugin_id))
        .await;
    admin.publish(&lazy_plugin, &lazy_vid).await;
    admin.grant_all(&lazy_plugin).await;

    // ---- 客户端登录态 ----
    let session = member_session(&base).await;
    let auth = PlatformAuthManager::new();
    auth.set_partner_session(session);

    // ---- 1. 技能下发 ----
    let report = org_sync::sync(&auth, PackageKind::Skill)
        .await
        .expect("skill sync");
    let skills_root = pointer_core::skills::external::org_skills_dir().expect("org skills dir");
    assert!(
        skills_root.join(&skill_slug).join("SKILL.md").is_file(),
        "技能应落盘: {report:?}"
    );

    // ---- 2. 插件：default_on 自动下，optional 不下 ----
    let report = org_sync::sync(&auth, PackageKind::Plugin)
        .await
        .expect("plugin sync");
    let plugins_root = org_sync::plugins_root().expect("plugins dir");
    assert!(
        plugins_root.join(&auto_plugin_id).join("pointer-plugin.toml").is_file(),
        "default_on 插件应自动落盘: {report:?}"
    );
    assert!(
        !plugins_root.join(&lazy_plugin_id).exists(),
        "optional 插件不该自动下载: {report:?}"
    );
    assert!(
        report.deferred.iter().any(|slug| slug == &lazy_plugin_slug),
        "optional 插件应进 deferred: {report:?}"
    );

    // ---- 3. 按需安装 ----
    org_sync::install_package(&auth, PackageKind::Plugin, &lazy_plugin_slug)
        .await
        .expect("install on demand");
    assert!(
        plugins_root.join(&lazy_plugin_id).join("pointer-plugin.toml").is_file(),
        "按需安装后应落盘"
    );

    // ---- 4. 启用策略：改 default_on 后写进 policy ----
    admin.update_activation(&skill, "default_on").await;
    org_sync::sync(&auth, PackageKind::Skill)
        .await
        .expect("resync");
    assert!(
        org_sync::default_on_skill_ids().contains(&skill_slug),
        "default_on 应写进 .org_policy.json"
    );
    assert!(org_sync::is_default_on(&skill_slug));

    // ---- 5. 撤销授权 → 未改动的目录被清理 ----
    admin.revoke_all(&skill).await;
    org_sync::sync(&auth, PackageKind::Skill)
        .await
        .expect("sync after revoke");
    assert!(
        !skills_root.join(&skill_slug).exists(),
        "撤销授权后应删除未改动的技能目录"
    );

    // ---- 6. 配额 block 能拦住 ----
    admin.set_quota_block(&member_id, 0, "block").await;
    let gate = auth.ensure_llm_allowed().await;
    assert!(
        gate.is_err(),
        "配额 block 时 ensure_llm_allowed 应失败，实际 {gate:?}"
    );
    let message = gate.unwrap_err().to_string();
    assert!(
        message.contains("token_quota_exhausted"),
        "错误信息应可识别，实际: {message}"
    );

    // ---- 清理：只删本次装的两个插件目录，绝不碰整个插件根目录 ----
    let _ = fs::remove_dir_all(plugins_root.join(&auto_plugin_id));
    let _ = fs::remove_dir_all(plugins_root.join(&lazy_plugin_id));
    let _ = org_sync::forget_package(PackageKind::Plugin, &auto_plugin_id);
    let _ = org_sync::forget_package(PackageKind::Plugin, &lazy_plugin_id);
    let _ = fs::remove_dir_all(&data_dir);
}

/// 目录缓存要能让 UI 列出「企业提供了什么」。
#[tokio::test]
async fn catalog_cache_lists_available_packages() {
    let Some(base) = base() else {
        eprintln!("skipped: POINTER_ADMIN_E2E_BASE 未设置");
        return;
    };
    std::env::set_var("POINTER_API_BASE", &base);

    // 员工账号要存在，否则授权页会以「用户名或密码错误」拒绝。
    let admin = Admin::login(&base, "admin", &admin_password())
        .await
        .expect("admin login");
    admin.ensure_member().await;

    let session = member_session(&base).await;
    let auth = PlatformAuthManager::new();
    auth.set_partner_session(session);
    org_sync::sync(&auth, PackageKind::Plugin)
        .await
        .expect("sync");

    let catalog = org_sync::read_catalog_cache(PackageKind::Plugin);
    assert!(!catalog.is_empty(), "同步后应有目录缓存");
    assert!(
        catalog.iter().any(|item| item.activation == "optional"),
        "缓存里应保留 optional 项（供 UI 显示可安装）"
    );
}
