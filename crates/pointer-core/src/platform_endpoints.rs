//! pointer-server API base URLs.
//!
//! The open-source tree hardcodes no Pointer domain. A managed build injects
//! the domains at build time (see `build.rs`); without them this build is unbound.
//!
//! - **Managed build** (`POINTER_EDITION=managed`, or the legacy `official`): the
//!   injected domains.
//! - **Any other build**: unbound unless `POINTER_*` is set, i.e. standalone.
//! - **Standalone / unbound**: empty, so related platform features stay disabled.
//! - Any build can override with `POINTER_*`.

/// Baked in by `build.rs` from `POINTER_API_BASE`; empty when the build supplied none.
pub const DEFAULT_API_BASE: &str = env!("POINTER_BUILTIN_API_BASE");

pub const DEFAULT_OAUTH_CLIENT_ID: &str = "pointer-desktop";

/// Resolve one platform base URL: an explicit env var wins, otherwise only a
/// managed build falls back to the domain injected at build time. Anything else
/// is unbound and behaves as standalone.
///
/// 控制面地址是**部署配置**，只由构建期注入或启动时的 `POINTER_API_BASE` 决定，
/// 不提供运行期改写入口：终端用户没有这个信息，填错还会把自己锁在外面。
fn resolve_platform_base(key: &str, platform_default: &str) -> String {
    if let Ok(raw) = std::env::var(key) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if crate::edition::is_managed() {
        return platform_default.to_string();
    }
    String::new()
}

/// Whether this process has a control plane to talk to.
///
/// 只看 `POINTER_API_BASE` —— 控制面只有一个地址，登录页/授权页/接口同源。
pub fn control_plane_bound() -> bool {
    is_bound(&api_base())
}

fn is_bound(api_base: &str) -> bool {
    !api_base.trim().is_empty()
}

pub fn api_base() -> String {
    resolve_platform_base("POINTER_API_BASE", DEFAULT_API_BASE)
}

pub fn oauth_client_id() -> String {
    std::env::var("POINTER_OAUTH_CLIENT_ID").unwrap_or_else(|_| DEFAULT_OAUTH_CLIENT_ID.to_string())
}

#[cfg(test)]
mod tests {
    /// Guard: the open-source tree must not carry a vendor production domain in
    /// shipped code. Docs, licences, metadata, and example configs may name it.
    #[test]
    fn shipped_code_hardcodes_no_vendor_domain() {
        let domain = concat!("readflow", "ai.com");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut offenders: Vec<String> = Vec::new();
        collect_offenders(&root, &root, domain, &mut offenders);
        assert!(
            offenders.is_empty(),
            "shipped code must not hardcode a vendor domain — inject it at build time instead: {offenders:#?}"
        );
    }

    #[test]
    fn binding_depends_only_on_the_api_base() {
        assert!(!super::is_bound(""));
        assert!(!super::is_bound("   "));
        assert!(super::is_bound("https://admin.example.com"));
    }

    fn collect_offenders(
        root: &std::path::Path,
        dir: &std::path::Path,
        domain: &str,
        out: &mut Vec<String>,
    ) {
        const SKIP_DIRS: [&str; 7] = [
            "target",
            "node_modules",
            "dist",
            ".git",
            ".scratch",
            "gen",
            "vendor",
        ];
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Local-only files (env overrides, scratch) are not shipped; dotfiles
            // are tool state. Everything else under the repo is fair game.
            if name.starts_with('.') || name.ends_with(".env") || name.contains(".local.") {
                continue;
            }
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if path.is_dir() {
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                collect_offenders(root, &path, domain, out);
                continue;
            }
            if is_allowed_path(&rel) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (n, line) in text.lines().enumerate() {
                if line.contains(domain) {
                    out.push(format!("{rel}:{}", n + 1));
                }
            }
        }
    }

    fn is_allowed_path(rel: &str) -> bool {
        const ALLOWED_DIRS: [&str; 2] = ["docs/", "skills/"];
        const ALLOWED_FILES: [&str; 17] = [
            "NOTICE",
            "LICENSE",
            "README.md",
            "README.zh-CN.md",
            "CHANGELOG.md",
            "DEVELOPMENT.md",
            "CONTRIBUTING.md",
            "package.json",
            "Cargo.toml",
            "Cargo.lock",
            "src-tauri/tauri.conf.json",
            "scripts/build-server-deb.mjs",
            // Release tooling that has to recognise the official hosts to label them.
            "scripts/rename-release-assets.mjs",
            "scripts/rename-release-assets.test.mjs",
            "pointer.local.env.example",
            "server/pointer-server.toml.example",
            "src-tauri/tauri.personal.conf.json",
        ];
        ALLOWED_DIRS.iter().any(|d| rel.starts_with(d)) || ALLOWED_FILES.contains(&rel)
    }
}
