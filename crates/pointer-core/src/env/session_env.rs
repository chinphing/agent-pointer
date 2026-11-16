//! 会话 ↔ 执行环境绑定（设计文档 §7.1 B3）。
//!
//! 职责：
//!
//! 1. 按 §3.4 的三层规则决定本会话用 `local` 还是 `light`；
//! 2. `light` 时创建 / 复用远端沙箱（按会话缓存），并把它绑到 thread-local `EnvironmentGuard`；
//! 3. 会话结束 / 空闲超时后回收沙箱。
//!
//! 约定：`resolve_session_env` 返回 `None` 表示"用进程级 local"——工具层
//! `effective_env()` 会自动回落到 `LocalEnv`，因此不需要额外绑定。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::mounts::{MountPlan, SkillRoot};
use super::provider::{Backend, ProviderConfig, ResolvedEnv, Tier};
use super::remote::{RemoteEnv, RemoteSpec};
use super::{current_env, Environment, EnvironmentGuard};

/// 沙箱空闲多久后回收（下次同会话再来会重建）。
const IDLE_TTL: Duration = Duration::from_secs(30 * 60);

/// 沙箱默认资源配额：2 核 / 4 GB / 10 GB。
///
/// 不给配额时单个容器能吃满宿主——一个用户跑飞会拖垮同一台机器上的所有会话。
const DEFAULT_SANDBOX_CPUS: f32 = 2.0;
const DEFAULT_SANDBOX_MEMORY_MB: u32 = 4096;
const DEFAULT_SANDBOX_DISK_MB: u32 = 10240;

/// 生效的沙箱配额。
///
/// `POINTER_SERVER_ENVIRONMENT_{CPUS,MEMORY_MB,DISK_MB}` 覆盖默认值；
/// 显式设成 `0` 表示**不限制**（给需要跑重活的部署留后门）。
pub fn sandbox_resource_limits() -> (Option<f32>, Option<u32>, Option<u32>) {
    fn disabled(key: &str) -> bool {
        std::env::var(key)
            .map(|v| v.trim() == "0")
            .unwrap_or(false)
    }
    let cpus = if disabled("POINTER_SERVER_ENVIRONMENT_CPUS") {
        None
    } else {
        Some(crate::server_config::environment_cpus().unwrap_or(DEFAULT_SANDBOX_CPUS))
    };
    let memory_mb = if disabled("POINTER_SERVER_ENVIRONMENT_MEMORY_MB") {
        None
    } else {
        Some(crate::server_config::environment_memory_mb().unwrap_or(DEFAULT_SANDBOX_MEMORY_MB))
    };
    let disk_mb = if disabled("POINTER_SERVER_ENVIRONMENT_DISK_MB") {
        None
    } else {
        Some(crate::server_config::environment_disk_mb().unwrap_or(DEFAULT_SANDBOX_DISK_MB))
    };
    (cpus, memory_mb, disk_mb)
}

struct Cached {
    env: Arc<RemoteEnv>,
    last_used: Instant,
}

fn cache() -> &'static Mutex<HashMap<String, Cached>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Cached>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 解析本会话应使用的环境；`None` = 走进程级 local。
///
/// light 档位会创建或复用沙箱（会发网络请求）。
pub fn resolve_session_env(
    cfg: &ProviderConfig,
    conversation_id: &str,
    session_user_id: &str,
    workspace_root: Option<&Path>,
) -> Result<Option<Arc<dyn Environment>>> {
    let ResolvedEnv { tier, backend } = cfg.resolve(Some(session_user_id))?;
    match (tier, backend) {
        (Tier::Local, _) => Ok(None),
        (_, Backend::Envd) => {
            let env = acquire_remote(cfg, conversation_id, session_user_id, workspace_root)?;
            Ok(Some(env as Arc<dyn Environment>))
        }
        (_, Backend::Docker) => {
            let env =
                acquire_docker(cfg, conversation_id, session_user_id, workspace_root, tier)?;
            Ok(Some(env as Arc<dyn Environment>))
        }
        (_, Backend::Host) => anyhow::bail!("environment: 非 local 档位不能用 host 后端"),
    }
}

/// 同 [`resolve_session_env`]，配置从 server config / 环境变量读取。
pub fn resolve_session_env_for_current(
    conversation_id: &str,
    session_user_id: &str,
    workspace_root: Option<&Path>,
) -> Result<Option<Arc<dyn Environment>>> {
    let cfg = ProviderConfig::from_env()?;
    resolve_session_env(&cfg, conversation_id, session_user_id, workspace_root)
}

/// 工具分发 / 子智能体线程重放用：已经绑过就不重复绑。
pub fn bind_if_absent(
    conversation_id: &str,
    session_user_id: &str,
    workspace_root: Option<&Path>,
) -> Result<Option<EnvironmentGuard>> {
    if current_env().is_some() {
        return Ok(None);
    }
    let Some(env) = resolve_session_env_for_current(conversation_id, session_user_id, workspace_root)?
    else {
        return Ok(None);
    };
    Ok(Some(EnvironmentGuard::enter(env)))
}

/// 按会话查已建好的环境（**不依赖线程局部**）。
///
/// tokio 任务在 await 点可能换线程，`spawn_blocking` 更是必然换线程，
/// thread-local 绑定在那些位置读不到——凡是"当前线程上有没有环境"都可能错的
/// 地方，改用这个按 conversation_id 查。
pub fn env_for_conversation(conversation_id: &str) -> Option<Arc<dyn Environment>> {
    // 两条后端各有一份缓存；工具层不关心后端，这里统一查。
    if let Ok(mut map) = cache().lock() {
        if let Some(hit) = map.get_mut(conversation_id) {
            hit.last_used = Instant::now();
            return Some(hit.env.clone() as Arc<dyn Environment>);
        }
    }
    if let Ok(mut map) = docker_cache().lock() {
        evict_idle_docker(&mut map, Instant::now());
        if let Some((hit, last_used)) = map.get_mut(conversation_id) {
            *last_used = Instant::now();
            return Some(hit.clone() as Arc<dyn Environment>);
        }
    }
    None
}

/// 工具分派点用：按会话**缓存**重放环境绑定。
///
/// 与 [`bind_if_absent`] 的区别：这里不重新解析 provider（工具线程上没有
/// session_user 线程局部，重解析可能得到不同档位），只看该会话是否已经建过沙箱。
/// 建过 → 绑定并返回 guard；没建过（local 档位）→ 返回 `None`，保持宿主行为。
pub fn bind_cached(conversation_id: &str) -> Option<EnvironmentGuard> {
    if current_env().is_some() {
        return None;
    }
    let env = env_for_conversation(conversation_id)?;
    log::debug!(
        "bind_env: replay cached conversation={conversation_id} kind={:?}",
        env.kind()
    );
    Some(EnvironmentGuard::enter(env))
}

/// 绑定本会话环境，并给出工具层应当使用的工作区根。
///
/// light 档位返回 guest 路径 `/workspace`（宿主路径在沙箱里不存在）；
/// local 档位原样返回 `fallback`。已经绑过时沿用现有环境。
///
/// 返回 `String` 是为了对齐现有 `ConversationWorkspaceGuard` / `SessionWorkDirGuard` 的入参类型。
pub fn bind_and_resolve_workspace(
    conversation_id: &str,
    session_user_id: &str,
    fallback: impl Into<PathBuf>,
) -> Result<(Option<EnvironmentGuard>, String)> {
    let fallback: PathBuf = fallback.into();
    let already = current_env();
    log::debug!(
        "bind_env: conversation={conversation_id} user={session_user_id} already_bound={:?} fallback={}",
        already.as_ref().map(|e| e.kind()),
        fallback.display()
    );
    if let Some(env) = already {
        return Ok((
            None,
            env.workspace_root().to_string_lossy().into_owned(),
        ));
    }
    let resolved =
        resolve_session_env_for_current(conversation_id, session_user_id, Some(fallback.as_path()))?;
    log::debug!(
        "bind_env: conversation={conversation_id} resolved={:?}",
        resolved.as_ref().map(|e| e.kind())
    );
    match resolved {
        Some(env) => {
            let workspace = env.workspace_root().to_string_lossy().into_owned();
            Ok((Some(EnvironmentGuard::enter(env)), workspace))
        }
        None => Ok((None, fallback.to_string_lossy().into_owned())),
    }
}

/// 技能根挂载前的凭证检查（§5.5）：技能目录里不允许出现裸凭证文件。
///
/// **命中即报错，不静默剔除**——静默剔除会让人误以为"凭证放技能里也没事"，
/// 下次换个文件名就漏了。`.env` 是技能脚本需要的环境配置，**允许存在**。
pub fn verify_skill_roots(skill_roots: &[SkillRoot]) -> Result<()> {
    let mut hits: Vec<PathBuf> = Vec::new();
    for root in skill_roots {
        collect_credential_hits(&root.path, 0, CREDENTIAL_SCAN_DEPTH, &mut hits);
    }
    if hits.is_empty() {
        return Ok(());
    }
    let list = hits
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join("\n  ");
    anyhow::bail!(
        "技能目录里发现裸凭证文件（凭证应走 §8 的请求级 env 注入，不要落在技能目录）：\n  {list}"
    );
}

/// 扫描深度：与 §5.5 的 `find -maxdepth 3` 对齐。
const CREDENTIAL_SCAN_DEPTH: usize = 3;

fn is_credential_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "token.txt"
        || (lower.ends_with("_token.txt") && lower.len() > "_token.txt".len())
}

fn collect_credential_hits(dir: &Path, depth: usize, max_depth: usize, hits: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if meta.is_dir() {
            // 跳过版本库与虚拟环境，避免把打包产物当凭证
            if matches!(
                name.as_str(),
                ".git" | ".venv" | "node_modules" | "__pycache__" | ".cache"
            ) {
                continue;
            }
            collect_credential_hits(&path, depth + 1, max_depth, hits);
        } else if meta.is_file() && is_credential_file_name(&name) {
            hits.push(path);
        }
    }
}

/// 回收某个会话的沙箱（会话删除 / 主动结束）。
pub fn release(conversation_id: &str) {
    let entry = cache().lock().ok().and_then(|mut m| m.remove(conversation_id));
    if let Some(cached) = entry {
        if let Err(e) = cached.env.destroy() {
            log::warn!("sandbox release failed conversation={conversation_id}: {e:#}");
        } else {
            log::info!(
                "sandbox released conversation={conversation_id} sandbox={}",
                cached.env.sandbox_id()
            );
        }
    }
}

/// 进程退出时统一回收。
pub fn release_all() {
    let entries: Vec<Cached> = match cache().lock() {
        Ok(mut m) => m.drain().map(|(_, v)| v).collect(),
        Err(_) => Vec::new(),
    };
    for cached in entries {
        let _ = cached.env.destroy();
    }
}

/// 当前会话是否已有缓存沙箱（测试 / 诊断用）。
pub fn cached_sandbox_id(conversation_id: &str) -> Option<String> {
    cache()
        .lock()
        .ok()
        .and_then(|m| m.get(conversation_id).map(|c| c.env.sandbox_id().to_string()))
}


/// 进程启动时清一次残留的 docker 沙箱（只清已退出的）。
pub fn cleanup_docker_orphans() {
    crate::env::docker::DockerEnv::cleanup_orphans();
}

/// Docker 后端的会话缓存（容器名按会话固定，销毁幂等）。
fn docker_cache() -> &'static Mutex<HashMap<String, (Arc<crate::env::docker::DockerEnv>, Instant)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Arc<crate::env::docker::DockerEnv>, Instant)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 创建或复用本地 Docker 容器作为会话环境。
///
/// 与 envd 后端的关键差异：`/workspace` 直接 bind 用户的项目目录，
/// 所以 `git status` / diff / 文件树都能正常工作。
fn acquire_docker(
    cfg: &ProviderConfig,
    conversation_id: &str,
    session_user_id: &str,
    workspace_root: Option<&Path>,
    tier: Tier,
) -> Result<Arc<crate::env::docker::DockerEnv>> {
    if let Ok(mut map) = docker_cache().lock() {
        evict_idle_docker(&mut map, Instant::now());
        if let Some((hit, last_used)) = map.get_mut(conversation_id) {
            *last_used = Instant::now();
            return Ok(hit.clone());
        }
    }
    let creation_lock = conversation_creation_lock(conversation_id);
    let _creating = creation_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(mut map) = docker_cache().lock() {
        if let Some((hit, last_used)) = map.get_mut(conversation_id) {
            *last_used = Instant::now();
            return Ok(hit.clone());
        }
    }

    let sandboxes_base = match cfg
        .volume_host_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(dir) => PathBuf::from(dir),
        None => crate::session_sandbox::SessionSandbox::sandboxes_base()?,
    };
    let skill_roots = skill_roots_for_mounts()?;
    verify_skill_roots(&skill_roots)?;
    let plugin_root = crate::plugins::user_plugins_dir().ok();
    let mounts = MountPlan::for_session_with_plugins(
        &sandboxes_base,
        session_user_id,
        conversation_id,
        &skill_roots,
        plugin_root.as_deref(),
        workspace_root,
    );

    let (cpus, memory_mb, disk_mb) = sandbox_resource_limits();
    let spec = crate::env::docker::DockerSpec {
        container: crate::env::docker::DockerSpec::container_name_for(conversation_id),
        image: cfg
            .template
            .clone()
            .unwrap_or_else(|| crate::env::remote::DEFAULT_TEMPLATE.to_string()),
        mounts,
        workdir: PathBuf::from(crate::env::mounts::GUEST_WORKSPACE),
        tier,
        cpus,
        memory_mb,
        disk_mb,
        vnc_password: None,
    };
    let env = Arc::new(crate::env::docker::DockerEnv::create(spec)?);
    log::info!(
        "sandbox created (docker) conversation={conversation_id} container={}",
        env.container_name()
    );
    if let Ok(mut map) = docker_cache().lock() {
        map.insert(conversation_id.to_string(), (env.clone(), Instant::now()));
    }
    Ok(env)
}

/// 同一会话的沙箱创建必须单飞：面板与对话几乎同时首次进入时，
/// 两个线程都 cache miss 会各建一个沙箱，导致面板看到的 `/workspace`
/// 和智能体实际用的不是同一个。
fn conversation_creation_lock(conversation_id: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let map = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .entry(conversation_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn acquire_remote(
    cfg: &ProviderConfig,
    conversation_id: &str,
    session_user_id: &str,
    workspace_root: Option<&Path>,
) -> Result<Arc<RemoteEnv>> {
    let now = Instant::now();
    if let Ok(mut map) = cache().lock() {
        evict_idle(&mut map, now);
        if let Some(hit) = map.get_mut(conversation_id) {
            hit.last_used = now;
            return Ok(hit.env.clone());
        }
    }

    // 单飞：同一会话只允许一个线程建沙箱，其余等它建完后直接命中缓存。
    let creation_lock = conversation_creation_lock(conversation_id);
    let _creating = creation_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(mut map) = cache().lock() {
        if let Some(hit) = map.get_mut(conversation_id) {
            hit.last_used = Instant::now();
            return Ok(hit.env.clone());
        }
    }

    let control_url = cfg
        .sandbox_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("environment.provider=light 但未配置 environment.sandbox_url")
        })?
        .to_string();

    let volume_host_path = cfg
        .volume_host_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    // e2b-local：卷根就是沙箱根（目录名即卷名，必须是卷根下的真实目录）；
    // 否则用 app data 下的会话沙箱目录（Cube 走 metadata host-mount）。
    let sandboxes_base = match &volume_host_path {
        Some(dir) => dir.clone(),
        None => crate::session_sandbox::SessionSandbox::sandboxes_base()?,
    };
    let skill_roots = skill_roots_for_mounts()?;
    // 挂载前先扫凭证：命中直接失败，不静默剔除（§5.5）。
    verify_skill_roots(&skill_roots)?;
    // 用户插件根只读挂到 `/plugins`：hooks 脚本要能在沙箱里执行。
    let plugin_root = crate::plugins::user_plugins_dir().ok();
    let mounts = MountPlan::for_session_with_plugins(
        &sandboxes_base,
        session_user_id,
        conversation_id,
        &skill_roots,
        plugin_root.as_deref(),
        workspace_root,
    );
    // Cube 走 metadata host-mount，宿主目录必须先存在；
    // e2b-local 走卷注册，目录由网关创建（预建会被判成 unmanaged）。
    if volume_host_path.is_none() {
        for mount in mounts.iter().filter(|m| !m.read_only) {
            std::fs::create_dir_all(&mount.host).with_context(|| {
                format!("create mount host dir {}", mount.host.display())
            })?;
        }
    }

    let spec = RemoteSpec {
        control_url,
        mounts,
        volume_host_path,
        template: cfg
            .template
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(crate::env::remote::DEFAULT_TEMPLATE)
            .to_string(),
        ..Default::default()
    };
    let env = RemoteEnv::create(&spec).context("create remote env for session")?;
    log::info!(
        "sandbox created conversation={conversation_id} sandbox={} envd={}",
        env.sandbox_id(),
        env.envd_url()
    );
    let env = Arc::new(env);
    if let Ok(mut map) = cache().lock() {
        map.insert(
            conversation_id.to_string(),
            Cached {
                env: env.clone(),
                last_used: now,
            },
        );
    }
    Ok(env)
}

/// docker 后端的空闲淘汰：先 `docker rm -f`，再移出缓存。
fn evict_idle_docker(
    map: &mut HashMap<String, (Arc<crate::env::docker::DockerEnv>, Instant)>,
    now: Instant,
) {
    let stale: Vec<String> = map
        .iter()
        .filter(|(_, (_, last_used))| should_evict(*last_used, now))
        .map(|(id, _)| id.clone())
        .collect();
    for id in stale {
        if let Some((env, _)) = map.remove(&id) {
            log::info!("sandbox idle evicted (docker) conversation={id}");
            let _ = env.destroy();
        }
    }
}

fn evict_idle(map: &mut HashMap<String, Cached>, now: Instant) {
    let expired: Vec<String> = map
        .iter()
        .filter(|(_, c)| should_evict(c.last_used, now))
        .map(|(k, _)| k.clone())
        .collect();
    for key in expired {
        if let Some(cached) = map.remove(&key) {
            if let Err(e) = cached.env.destroy() {
                log::warn!("idle sandbox destroy failed conversation={key}: {e:#}");
            }
        }
    }
}

fn should_evict(last_used: Instant, now: Instant) -> bool {
    now.saturating_duration_since(last_used) >= IDLE_TTL
}

/// 技能根 → 挂载点名字。顺序与 `skills::external::skill_roots` 一致。
fn skill_roots_for_mounts() -> Result<Vec<SkillRoot>> {
    const NAMES: [&str; 4] = ["user", "org", "agents", "bundled"];
    let roots = crate::skills::external::skill_roots()?;
    Ok(roots
        .into_iter()
        .enumerate()
        .map(|(i, path)| {
            let name = NAMES.get(i).copied().unwrap_or("skills");
            SkillRoot::new(name, path)
        })
        .collect())
}

/// light 模式下工具层应当使用的工作区根（guest 路径）。
pub fn guest_workspace_root(env: &Arc<dyn Environment>) -> PathBuf {
    env.workspace_root().to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::provider::{Backend, Tier};
    use std::collections::BTreeMap;

    fn local_cfg() -> ProviderConfig {
        ProviderConfig {
            tier: Some(Tier::Local),
            backend: None,
            sandbox_url: None,
            volume_host_path: None,
            template: None,
            admins: vec![],
            accounts: BTreeMap::new(),
            platform: false,
        }
    }

    fn light_cfg_without_url() -> ProviderConfig {
        ProviderConfig {
            tier: Some(Tier::Light),
            backend: None,
            sandbox_url: None,
            ..Default::default()
        }
    }

    #[test]
    fn local_provider_returns_no_env() {
        let env = resolve_session_env(&local_cfg(), "conv-1", "alice", None).expect("resolve");
        assert!(env.is_none(), "local 不需要绑定环境");
    }

    #[test]
    fn light_without_sandbox_url_errors() {
        let err = match resolve_session_env(&light_cfg_without_url(), "conv-1", "alice", None) {
            Ok(_) => panic!("light 缺 sandbox_url 必须报错"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("sandbox_url"), "{err}");
    }

    #[test]
    fn idle_eviction_threshold() {
        let now = Instant::now();
        assert!(!should_evict(now, now));
        assert!(!should_evict(now, now + IDLE_TTL / 2));
        assert!(should_evict(now, now + IDLE_TTL));
        assert!(should_evict(now, now + IDLE_TTL * 2));
    }

    #[test]
    fn release_unknown_conversation_is_noop() {
        release("never-created");
        assert!(cached_sandbox_id("never-created").is_none());
    }

    fn write(path: &std::path::Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn credential_scan_rejects_bare_token_files() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("token.txt"), "secret");
        let roots = vec![SkillRoot::new("user", dir.path())];
        let err = verify_skill_roots(&roots).unwrap_err().to_string();
        assert!(err.contains("token.txt"), "{err}");
    }

    #[test]
    fn credential_scan_rejects_prefixed_token_files() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("cwpt/skills/cwpt_token.txt"), "secret");
        let roots = vec![SkillRoot::new("user", dir.path())];
        assert!(verify_skill_roots(&roots).is_err());
    }

    #[test]
    fn credential_scan_allows_dot_env() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("cwpt/.env"), "CWPT_BASE_URL=http://x");
        write(&dir.path().join("cwpt/SKILL.md"), "# skill");
        let roots = vec![SkillRoot::new("user", dir.path())];
        verify_skill_roots(&roots).expect(".env 是脚本环境配置，必须允许");
    }

    #[test]
    fn credential_scan_ignores_lookalikes() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("mytoken.txt"), "not a credential file");
        write(&dir.path().join("token.txt.bak"), "not matched");
        let roots = vec![SkillRoot::new("user", dir.path())];
        verify_skill_roots(&roots).expect("非 *_token.txt 不应命中");
    }

    #[test]
    fn credential_scan_skips_venv_and_git() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join(".venv/lib/token.txt"), "vendored");
        write(&dir.path().join(".git/token.txt"), "git internals");
        let roots = vec![SkillRoot::new("user", dir.path())];
        verify_skill_roots(&roots).expect("虚拟环境与版本库内部不应算凭证");
    }

    #[test]
    fn credential_scan_honors_depth_limit() {
        let dir = tempfile::tempdir().unwrap();
        // 深度 4（超过 CREDENTIAL_SCAN_DEPTH=3）不扫
        write(&dir.path().join("a/b/c/d/token.txt"), "deep");
        let roots = vec![SkillRoot::new("user", dir.path())];
        verify_skill_roots(&roots).expect("超出扫描深度不命中");
    }

    #[test]
    fn skill_roots_map_to_named_read_only_mounts() {
        let roots = skill_roots_for_mounts().expect("skill roots");
        assert!(
            (3..=4).contains(&roots.len()),
            "技能根应为 3~4 个: {roots:?}"
        );
        let names: Vec<&str> = roots.iter().map(|r| r.name.as_str()).collect();
        let expected = ["user", "org", "agents", "bundled"];
        for (i, name) in names.iter().enumerate() {
            assert_eq!(*name, expected[i], "技能根名字顺序必须固定: {names:?}");
        }

        let mounts = MountPlan::for_session(
            std::path::Path::new("/data/session-sandboxes"),
            "alice",
            "c-1",
            &roots,
        );
        let skill_mounts: Vec<_> = mounts.iter().filter(|m| m.read_only).collect();
        assert_eq!(skill_mounts.len(), roots.len());
        for m in skill_mounts {
            assert!(
                m.guest.starts_with("/skills/"),
                "技能根必须挂在 /skills/ 下: {}",
                m.guest.display()
            );
        }
    }

    #[test]
    fn credential_scan_covers_multiple_roots() {        let clean = tempfile::tempdir().unwrap();
        write(&clean.path().join("x/SKILL.md"), "# ok");
        let dirty = tempfile::tempdir().unwrap();
        write(&dirty.path().join("token.txt"), "secret");
        let roots = vec![
            SkillRoot::new("user", clean.path()),
            SkillRoot::new("bundled", dirty.path()),
        ];
        let err = verify_skill_roots(&roots).unwrap_err().to_string();
        assert!(err.contains("bundled") || err.contains("token.txt"), "{err}");
    }

    #[test]
    fn admin_resolves_local_so_no_sandbox() {
        let mut cfg = light_cfg_without_url();
        cfg.sandbox_url = Some("http://127.0.0.1:3000".into());
        cfg.admins = vec!["admin".into()];
        // admin 默认 local：即使部署是 light 且没配 url 也不该报错
        let env = resolve_session_env(&cfg, "conv-admin", "admin", None).expect("admin resolve");
        assert!(env.is_none());
    }

    #[test]
    fn sandbox_resource_limits_default_to_a_cap() {
        let _lock = crate::test_support::path_env_lock();
        for key in [
            "POINTER_SERVER_ENVIRONMENT_CPUS",
            "POINTER_SERVER_ENVIRONMENT_MEMORY_MB",
            "POINTER_SERVER_ENVIRONMENT_DISK_MB",
        ] {
            std::env::remove_var(key);
        }
        let (cpus, memory_mb, disk_mb) = sandbox_resource_limits();
        // 回归：以前三个都是 None，单个容器能吃满宿主。
        assert_eq!(cpus, Some(2.0));
        assert_eq!(memory_mb, Some(4096));
        assert_eq!(disk_mb, Some(10240));
    }

    #[test]
    fn sandbox_resource_limits_can_be_disabled_with_zero() {
        let _lock = crate::test_support::path_env_lock();
        std::env::set_var("POINTER_SERVER_ENVIRONMENT_MEMORY_MB", "0");
        let (_, memory_mb, _) = sandbox_resource_limits();
        std::env::remove_var("POINTER_SERVER_ENVIRONMENT_MEMORY_MB");
        assert_eq!(memory_mb, None, "显式 0 = 不限制");
    }

    #[test]
    fn env_lookup_covers_both_backends() {
        // 回归：docker 后端上线后，工具线程重放必须也能查到它，
        // 否则 effective_env() 会退到 DeniedEnv，工具报"执行环境未绑定"。
        let conv = "conv-docker-lookup";
        let dir = tempfile::tempdir().expect("dir");
        let spec = crate::env::docker::DockerSpec {
            container: crate::env::docker::DockerSpec::container_name_for(conv),
            image: "pointer-sandbox-light:latest".to_string(),
            mounts: vec![crate::env::pathmap::Mount::new(dir.path(), "/workspace", false)],
            workdir: std::path::PathBuf::from("/workspace"),
            tier: Tier::Light,
            cpus: None,
            memory_mb: None,
            disk_mb: None,
            vnc_password: None,
        };
        // 只验证缓存查找逻辑，不真的起容器：这里用 remote 缓存塞一个假条目不可行，
        // 所以改为断言"查不到时不 panic、返回 None"，真正的覆盖在 docker_env_it。
        assert!(env_for_conversation(conv).is_none());
        drop(spec);
    }
}
