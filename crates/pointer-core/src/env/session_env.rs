//! 会话 ↔ 执行环境绑定（设计文档 §7.1 B3）。
//!
//! 职责：
//!
//! 1. 按 §3.4 的三层规则决定本会话用 `local` 还是 `headless`；
//! 2. `headless` 时创建 / 复用远端沙箱（按会话缓存），并把它绑到 thread-local `EnvironmentGuard`；
//! 3. 会话结束 / 空闲超时后回收沙箱。
//!
//! 约定：`resolve_session_env` 返回 `None` 表示"本会话走 local 档位"（宿主直跑）。
//!
//! **local 档位也要绑定**：绑定的是 `LocalEnv`（工作区 = 会话工作区，路径映射恒等）。
//! 沙箱档位的部署里，"admin 走宿主 + 普通用户走沙箱"要能并存；不绑的话工具线程
//! 查不到环境，`effective_env()` 会按**部署档位**返回 `DeniedEnv`，admin 的宿主
//! 工具全线报"工作区不存在"。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::mounts::{MountPlan, SkillRoot};
use super::provider::{Backend, ProviderConfig, ResolvedEnv, Tier};
use super::remote::{RemoteEnv, RemoteSpec};
use super::{current_env, Environment, EnvironmentGuard, LocalEnv};

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

/// local 档位的会话环境缓存。
///
/// 为什么需要：沙箱档位的部署里，工具线程（`spawn_blocking` / tokio 换线程）靠
/// [`env_for_conversation`] 按会话重放绑定。local 档位以前什么都不绑，重放时查不到
/// → `effective_env()` 按**部署档位**返回 `DeniedEnv`，admin 的宿主工具全线报
/// 「工作区目录无效或不存在」。这里按会话记一份 `LocalEnv`，让"admin 走宿主 +
/// 普通用户走沙箱"在同一个进程里并存。
///
/// 不需要空闲淘汰：`LocalEnv` 只有几个路径，没有可回收的资源；会话删除时由
/// [`release`] 摘掉。
fn local_cache() -> &'static Mutex<HashMap<String, Arc<LocalEnv>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<LocalEnv>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// local 档位的环境：工作区**原样**使用会话工作区。
///
/// 空工作区不换成设置值 / home：那会让"没有工作区"的工具悄悄写进 `$HOME`。
/// 保持空串，下游照旧报「未设置工作区」。
fn local_env_for(workspace: &Path) -> Arc<LocalEnv> {
    Arc::new(LocalEnv::new(workspace.to_path_buf()))
}

fn remember_local(conversation_id: &str, env: Arc<LocalEnv>) {
    if let Ok(mut map) = local_cache().lock() {
        map.insert(conversation_id.to_string(), env);
    }
}

/// 测试用：等价于跑过一次 local 档位的 [`bind_and_resolve_workspace`]。
#[cfg(test)]
pub(crate) fn remember_local_for_test(conversation_id: &str, workspace: &Path) {
    remember_local(conversation_id, local_env_for(workspace));
}

fn cached_local(conversation_id: &str) -> Option<Arc<LocalEnv>> {
    local_cache()
        .lock()
        .ok()
        .and_then(|map| map.get(conversation_id).cloned())
}

/// 解析本会话应使用的环境；`None` = **local 档位**（宿主直跑）。
///
/// 返回 `None` 不代表"什么都不用绑"：调用方（[`bind_and_resolve_workspace`] /
/// [`bind_if_absent`]）要绑一个工作区正确的 `LocalEnv`，工具线程才重放得到。
///
/// headless 档位会创建或复用沙箱（会发网络请求）。
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
            let env =
                acquire_remote(cfg, conversation_id, session_user_id, workspace_root, tier)?;
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
///
/// local 档位也算"要绑"：绑一个 `LocalEnv`（工作区取 `workspace_root`）。
pub fn bind_if_absent(
    conversation_id: &str,
    session_user_id: &str,
    workspace_root: Option<&Path>,
) -> Result<Option<EnvironmentGuard>> {
    if current_env().is_some() {
        return Ok(None);
    }
    let resolved =
        resolve_session_env_for_current(conversation_id, session_user_id, workspace_root)?;
    let fallback = workspace_root
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let (guard, _) = bind_resolved_env(conversation_id, resolved, fallback);
    Ok(guard)
}

/// 按会话查已建好的环境（**不依赖线程局部**）。
///
/// tokio 任务在 await 点可能换线程，`spawn_blocking` 更是必然换线程，
/// thread-local 绑定在那些位置读不到——凡是"当前线程上有没有环境"都可能错的
/// 地方，改用这个按 conversation_id 查。
pub fn env_for_conversation(conversation_id: &str) -> Option<Arc<dyn Environment>> {
    // local 档位（admin / standalone）也要能按会话重放：工具线程上没有 thread-local，
    // 查不到就会退到 `DeniedEnv`，宿主工具全线失效。
    if let Some(local) = cached_local(conversation_id) {
        let env: Arc<dyn Environment> = local;
        return Some(env);
    }
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

/// 只查不碰：命中不刷新 `last_used`。
///
/// 给面板轮询用——面板每隔几秒问一次"有没有桌面"，如果按普通查询算活跃，
/// 30 分钟空闲回收就永远不会触发（只要面板开着容器就活着）。
pub fn env_for_conversation_peek(conversation_id: &str) -> Option<Arc<dyn Environment>> {
    if let Ok(map) = cache().lock() {
        if let Some(hit) = map.get(conversation_id) {
            return Some(hit.env.clone() as Arc<dyn Environment>);
        }
    }
    if let Ok(mut map) = docker_cache().lock() {
        evict_idle_docker(&mut map, Instant::now());
        if let Some((hit, _)) = map.get(conversation_id) {
            return Some(hit.clone() as Arc<dyn Environment>);
        }
    }
    None
}

/// 接管期间「别回收这个容器」的标记。
///
/// 与 `env_for_conversation_peek` 配套：面板轮询不再刷新活跃时间，改由接管页
/// 显式 pin，关掉接管页再 unpin。
fn pinned_set() -> &'static Mutex<std::collections::HashSet<String>> {
    static PINNED: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    PINNED.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// 接管页打开/关闭时调用。pin 住的会话不参与空闲淘汰。
pub fn set_pinned(conversation_id: &str, pinned: bool) {
    if let Ok(mut set) = pinned_set().lock() {
        if pinned {
            set.insert(conversation_id.to_string());
        } else {
            set.remove(conversation_id);
        }
    }
}

fn is_pinned(conversation_id: &str) -> bool {
    pinned_set()
        .lock()
        .map(|set| set.contains(conversation_id))
        .unwrap_or(false)
}

/// 工具分派点用：按会话**缓存**重放环境绑定。
///
/// 与 [`bind_if_absent`] 的区别：这里不重新解析 provider（工具线程上没有
/// session_user 线程局部，重解析可能得到不同档位），只看该会话的环境缓存：
/// 沙箱（envd / docker）或 local 档位的 `LocalEnv` 都算。
/// 都没有（会话还没跑过对话）→ 返回 `None`。
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
/// 沙箱档位返回 guest 路径 `/workspace`（宿主路径在沙箱里不存在）；
/// local 档位绑 `LocalEnv` 并返回 `fallback`（宿主路径，路径映射恒等）。
/// 已经绑过时沿用现有环境。
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
    Ok(bind_resolved_env(conversation_id, resolved, fallback))
}

/// 把解析结果绑到当前线程，并给出工具层该用的工作区根。
///
/// `None`（local 档位）**也要绑**：绑一个工作区为 `fallback` 的 `LocalEnv`，
/// 并记进 [`local_cache`]，这样工具线程（`bind_cached`）能重放。
fn bind_resolved_env(
    conversation_id: &str,
    resolved: Option<Arc<dyn Environment>>,
    fallback: PathBuf,
) -> (Option<EnvironmentGuard>, String) {
    let env: Arc<dyn Environment> = match resolved {
        Some(env) => env,
        None => {
            let local = local_env_for(fallback.as_path());
            remember_local(conversation_id, local.clone());
            local
        }
    };
    let workspace = env.workspace_root().to_string_lossy().into_owned();
    (Some(EnvironmentGuard::enter(env)), workspace)
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

    // 注册表也要清：否则下次同 id 会话会去 attach 一个已经删掉的沙箱。
    crate::env::sandbox_registry::forget(conversation_id);

    // Docker 后端是另一份缓存：漏掉这里，删会话后容器会一直跑到空闲淘汰。
    set_pinned(conversation_id, false);
    let docker_entry = docker_cache().lock().ok().and_then(|mut m| m.remove(conversation_id));
    if let Some((env, _)) = docker_entry {
        if let Err(e) = env.destroy() {
            log::warn!("sandbox release failed (docker) conversation={conversation_id}: {e:#}");
        } else {
            log::info!(
                "sandbox released (docker) conversation={conversation_id} container={}",
                env.container_name()
            );
        }
    }

    // local 档位没有沙箱要销毁，但缓存条目要摘掉：删会话后不该还留着它的工作区。
    if let Ok(mut map) = local_cache().lock() {
        map.remove(conversation_id);
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

    // 同上：docker 那份也要收，否则退出应用后容器还在跑。
    let docker_entries: Vec<_> = match docker_cache().lock() {
        Ok(mut m) => m.drain().map(|(_, v)| v).collect(),
        Err(_) => Vec::new(),
    };
    for (env, _) in docker_entries {
        if let Err(e) = env.destroy() {
            log::warn!("sandbox release_all failed (docker) container={}: {e:#}", env.container_name());
        }
    }
    if let Ok(mut set) = pinned_set().lock() {
        set.clear();
    }
    if let Ok(mut map) = local_cache().lock() {
        map.clear();
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

/// 会话的容器规格（挂载计划、镜像、配额）。创建与收养共用，保证两边指纹一致。
fn docker_spec_for(
    cfg: &ProviderConfig,
    conversation_id: &str,
    session_user_id: &str,
    workspace_root: Option<&Path>,
    tier: Tier,
) -> Result<crate::env::docker::DockerSpec> {
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
    Ok(crate::env::docker::DockerSpec {
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
    })
}

/// 收养一个已在运行的容器（应用重启后缓存为空时用），**不创建、不删除**。
///
/// 面板接管入口靠它恢复：否则重启应用后必须等会话再跑一次工具，
/// 缓存才会重新有值，接管按钮才出现。
pub fn adopt_docker_env(
    cfg: &ProviderConfig,
    conversation_id: &str,
    session_user_id: &str,
    workspace_root: Option<&Path>,
) -> Option<Arc<dyn Environment>> {
    let spec = docker_spec_for(cfg, conversation_id, session_user_id, workspace_root, Tier::Desktop)
        .ok()?;
    let env = Arc::new(match crate::env::docker::DockerEnv::attach(spec.clone()) {
        Ok(Some(env)) => env,
        // 容器在、但规格变了（换镜像 / 改配额）或已经停了。轮询走到这里说明
        // 用户正想看桌面，按当前规格重建——否则升级镜像后入口会凭空消失，
        // 要等下一次跑工具才回来。
        _ => {
            if !crate::env::docker::DockerEnv::exists(&spec.container) {
                return None;
            }
            log::info!(
                "sandbox stale (docker) conversation={conversation_id} container={} → 按当前规格重建",
                spec.container
            );
            crate::env::docker::DockerEnv::create(spec).ok()?
        }
    });
    log::info!(
        "sandbox adopted (docker) conversation={conversation_id} container={}",
        env.container_name()
    );
    if let Ok(mut map) = docker_cache().lock() {
        map.insert(conversation_id.to_string(), (env.clone(), Instant::now()));
    }
    Some(env as Arc<dyn Environment>)
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
        if let Some(hit) = cached_docker_alive(&mut map, conversation_id) {
            return Ok(hit);
        }
    }
    let creation_lock = conversation_creation_lock(conversation_id);
    let _creating = creation_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(mut map) = docker_cache().lock() {
        if let Some(hit) = cached_docker_alive(&mut map, conversation_id) {
            return Ok(hit);
        }
    }

    let spec = docker_spec_for(cfg, conversation_id, session_user_id, workspace_root, tier)?;
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

/// envd 沙箱要不要带图形桌面。
///
/// **档位是唯一事实来源**：`desktop` 有桌面、`headless` 没有——与 docker 后端的
/// `DockerEnv::capabilities()` 是同一套推导，否则同一个档位两条后端会各说一套
/// （`tier=desktop` 时 envd 报无桌面、docker 报有桌面）。
///
/// `[environment] desktop` 只作**显式覆盖**：写了就听它的，与档位矛盾时告警
/// 但不拦——模板里到底有没有 X 栈只有部署的人知道。
fn remote_desktop(tier: Tier, explicit: Option<bool>) -> bool {
    let by_tier = tier == Tier::Desktop;
    match explicit {
        None => by_tier,
        Some(want) => {
            if want != by_tier {
                log::warn!(
                    "environment.desktop={want} 与档位 {tier:?} 矛盾：该档位默认{}桌面，按显式配置{}",
                    if by_tier { "带" } else { "不带" },
                    if want { "开启" } else { "关闭" }
                );
            }
            want
        }
    }
}

fn acquire_remote(
    cfg: &ProviderConfig,
    conversation_id: &str,
    session_user_id: &str,
    workspace_root: Option<&Path>,
    tier: Tier,
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
            anyhow::anyhow!("environment.provider=headless 但未配置 environment.sandbox_url")
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
        // 桌面能力：档位是唯一事实来源，`[environment] desktop` 只作显式覆盖。
        desktop: remote_desktop(tier, crate::server_config::environment_desktop_override()),
        template: cfg
            .template
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(crate::env::remote::DEFAULT_TEMPLATE)
            .to_string(),
        ..Default::default()
    };
    // 先查注册表：应用重启后缓存是空的，但沙箱可能还活着。不认回就会新建一个，
    // 旧的挂到超时才消失（实测攒了 26 个 e2b-envd-* 容器）。
    let reattached = crate::env::sandbox_registry::reattach(conversation_id, &spec.template, |entry| {
        let env = RemoteEnv::attach(&spec, entry.sandbox_id.clone(), entry.envd_url.clone()).ok()?;
        env.probe_alive().then(|| Arc::new(env))
    });

    let env = match reattached {
        Some(env) => env,
        None => {
            let env = RemoteEnv::create(&spec).context("create remote env for session")?;
            log::info!(
                "sandbox created conversation={conversation_id} sandbox={} envd={}",
                env.sandbox_id(),
                env.envd_url()
            );
            crate::env::sandbox_registry::record(
                conversation_id,
                crate::env::sandbox_registry::Entry {
                    sandbox_id: env.sandbox_id().to_string(),
                    envd_url: env.envd_url().to_string(),
                    template: spec.template.clone(),
                },
            );
            Arc::new(env)
        }
    };
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
/// 取缓存里这个会话的容器，**并确认它还在**。
///
/// 容器可能在应用不知情的情况下消失：清理脚本、`docker system prune`、用户手动
/// `docker rm`、宿主重启都会造成这种情况。只查缓存不校验存在性的话，这个会话会
/// 一直打向已删除的容器——每条命令、每次截图都报 `No such container`，而且自己
/// 永远回不来，只能等缓存空闲过期或重启应用。表现就是"环境突然不可用"。
///
/// 校验成本是一次 `docker inspect`（约 30ms），比随后必然发生的 `docker exec`
/// 便宜一个量级，所以放在每次取环境时做，而不是等命令失败再补救。
fn cached_docker_alive(
    map: &mut HashMap<String, (Arc<crate::env::docker::DockerEnv>, Instant)>,
    conversation_id: &str,
) -> Option<Arc<crate::env::docker::DockerEnv>> {
    let env = map.get(conversation_id)?.0.clone();
    if crate::env::docker::DockerEnv::exists(env.container_name()) {
        if let Some((_, last_used)) = map.get_mut(conversation_id) {
            *last_used = Instant::now();
        }
        return Some(env);
    }
    log::warn!(
        "sandbox lost (docker) conversation={conversation_id} container={} → 剔除缓存，重建",
        env.container_name()
    );
    map.remove(conversation_id);
    None
}

fn evict_idle_docker(
    map: &mut HashMap<String, (Arc<crate::env::docker::DockerEnv>, Instant)>,
    now: Instant,
) {
    let stale: Vec<String> = map
        .iter()
        .filter(|(id, (_, last_used))| should_evict(*last_used, now) && !is_pinned(id))
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

/// headless 模式下工具层应当使用的工作区根（guest 路径）。
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
            admin_tier: Default::default(),
            sandbox_url: None,
            volume_host_path: None,
            template: None,
            admins: vec![],
            accounts: BTreeMap::new(),
            tier_backends: Default::default(),
            platform: false,
        }
    }

    fn light_cfg_without_url() -> ProviderConfig {
        ProviderConfig {
            tier: Some(Tier::Headless),
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
            Ok(_) => panic!("headless 缺 sandbox_url 必须报错"),
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
        // admin 默认 local：即使部署是 headless 且没配 url 也不该报错
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
    fn adopt_returns_none_when_container_absent() {
        // 收养只在容器**已经存在**时才有意义；不存在就该老老实实返回 None，
        // 不能顺手把容器建起来（面板轮询会反复调用）。
        let cfg = ProviderConfig {
            tier: Some(Tier::Desktop),
            backend: Some(Backend::Docker),
            admin_tier: Default::default(),
            sandbox_url: None,
            volume_host_path: None,
            template: Some("pointer-sandbox-desktop:latest".to_string()),
            admins: Vec::new(),
            accounts: Default::default(),
            tier_backends: Default::default(),
            platform: false,
        };
        assert!(adopt_docker_env(&cfg, "conv-adopt-absent", "tester", None).is_none());
    }

    #[test]
    fn pinned_conversation_survives_idle_eviction() {
        // 接管页 pin 住的会话不能被空闲淘汰干掉，unpin 后恢复正常淘汰。
        let id = "conv-pinned-evict";
        let stale = Instant::now() - IDLE_TTL - std::time::Duration::from_secs(60);
        let mut map: HashMap<String, (Arc<crate::env::docker::DockerEnv>, Instant)> = HashMap::new();
        map.insert(
            id.to_string(),
            (
                Arc::new(crate::env::docker::DockerEnv::fake_for_tests("pointer-sbx-pinned")),
                stale,
            ),
        );

        set_pinned(id, true);
        evict_idle_docker(&mut map, Instant::now());
        assert!(map.contains_key(id), "pin 住的会话不该被淘汰");

        set_pinned(id, false);
        evict_idle_docker(&mut map, Instant::now());
        assert!(!map.contains_key(id), "unpin 后应恢复正常淘汰");
    }

    #[test]
    fn release_drops_docker_cache_entry() {
        // 回归：release 只清 envd 缓存时，删会话后容器会一直跑到空闲淘汰。
        let id = "conv-release-docker";
        if let Ok(mut map) = docker_cache().lock() {
            map.insert(
                id.to_string(),
                (
                    Arc::new(crate::env::docker::DockerEnv::fake_for_tests("pointer-sbx-release")),
                    Instant::now(),
                ),
            );
        }
        assert!(env_for_conversation_peek(id).is_some());

        release(id);

        assert!(
            env_for_conversation_peek(id).is_none(),
            "release 必须同时清掉 docker 缓存"
        );
    }

    #[test]
    fn env_lookup_covers_both_backends() {
        // 回归：docker 后端上线后，工具线程重放必须也能查到它，
        // 否则 effective_env() 会退到 DeniedEnv，工具报"执行环境未绑定"。
        let conv = "conv-docker-lookup";
        // 不真的起容器：塞一个 fake 实例进 docker 缓存，只验证查找链路。
        if let Ok(mut map) = docker_cache().lock() {
            map.insert(
                conv.to_string(),
                (
                    Arc::new(crate::env::docker::DockerEnv::fake_for_tests(
                        "pointer-sbx-docker-lookup",
                    )),
                    Instant::now(),
                ),
            );
        }

        let found = env_for_conversation(conv).expect("docker 缓存里的会话必须查得到");
        assert!(
            found.capabilities().desktop,
            "fake 是 container 档，应当带桌面"
        );
        assert!(
            env_for_conversation("conv-not-registered").is_none(),
            "没注册过的会话不该查到环境"
        );

        // 清理：直接摘缓存条目，fake 实例不连真实容器。
        if let Ok(mut map) = docker_cache().lock() {
            map.remove(conv);
        }
    }

    // --- local 档位也要显式绑定：admin 走宿主 + 普通用户走沙箱并存 ---

    /// 沙箱档位的部署配置（desktop + docker），`[auth] admins` 里有 `admin`。
    fn sandbox_deployment_cfg() -> ProviderConfig {
        ProviderConfig {
            tier: Some(Tier::Desktop),
            backend: Some(Backend::Docker),
            admin_tier: Default::default(),
            sandbox_url: None,
            volume_host_path: None,
            template: None,
            admins: vec!["admin".into()],
            accounts: Default::default(),
            tier_backends: Default::default(),
            platform: true,
        }
    }

    #[test]
    fn admin_on_sandbox_deployment_binds_local_env() {
        // 部署是 desktop（非 local 档位），admin 命中名单 → 解析成 local 档位。
        let cfg = sandbox_deployment_cfg();
        let dir = tempfile::tempdir().unwrap();
        let resolved = resolve_session_env(&cfg, "conv-admin-bind", "admin", Some(dir.path()))
            .expect("admin 解析不能报错");
        assert!(resolved.is_none(), "admin 是 local 档位，不该建沙箱");

        let (guard, workspace) =
            bind_resolved_env("conv-admin-bind", resolved, dir.path().to_path_buf());
        let guard = guard.expect("local 档位也必须绑定 LocalEnv");
        assert_eq!(workspace, dir.path().display().to_string());

        let env = current_env().expect("绑定后必须能读到环境");
        assert_eq!(env.kind(), crate::env::EnvKind::Local);
        assert_eq!(env.workspace_root(), dir.path());
        assert!(!env.is_denied(), "local 档位不是「未绑定」");

        // 回归：以前工具线程拿到的是 DeniedEnv，这里报「工作区目录无效或不存在」。
        let cwd = crate::tools::terminal::effective_terminal_cwd(None, &workspace)
            .expect("admin 的宿主工作区必须可用");
        assert_eq!(cwd, dir.path().canonicalize().unwrap());

        drop(guard);
        release("conv-admin-bind");
    }

    #[test]
    fn non_admin_on_sandbox_deployment_stays_in_sandbox() {
        // 同一份配置下普通用户仍是沙箱档位——local 绑定不能把沙箱用户带回宿主。
        // 真正建容器由 `tests/desktop_tier_it.rs` 覆盖，这里只锁档位判定。
        let cfg = sandbox_deployment_cfg();
        let resolved = cfg.resolve(Some("alice")).expect("resolve");
        assert_eq!(resolved.tier, Tier::Desktop);
        assert_eq!(resolved.backend, Backend::Docker);
        assert!(
            cached_local("conv-alice").is_none(),
            "沙箱用户不该有 local 缓存"
        );
    }

    #[test]
    fn local_env_is_replayable_on_tool_threads() {
        // 工具线程（`spawn_blocking`）上没有 thread-local，只能按会话缓存重放。
        // 查不到就会退到 DeniedEnv——这正是 admin 宿主工具全挂的原因。
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().display().to_string();
        let (guard, workspace) =
            bind_resolved_env("conv-local-replay", None, dir.path().to_path_buf());
        assert_eq!(workspace, ws);
        drop(guard);

        let expected = dir.path().canonicalize().unwrap();
        let cwd = std::thread::spawn(move || {
            assert!(current_env().is_none(), "新线程不该有 thread-local 绑定");
            let _g = bind_cached("conv-local-replay").expect("local 档位必须能按会话重放");
            assert_eq!(
                current_env().unwrap().kind(),
                crate::env::EnvKind::Local,
                "重放出来的必须是 LocalEnv"
            );
            crate::tools::terminal::effective_terminal_cwd(None, &ws)
        })
        .join()
        .unwrap()
        .expect("重放后 terminal 必须能定位工作区");
        assert_eq!(cwd, expected);
        release("conv-local-replay");
    }

    #[test]
    fn release_drops_local_cache_entry() {
        let dir = tempfile::tempdir().unwrap();
        let (_guard, _ws) =
            bind_resolved_env("conv-local-release", None, dir.path().to_path_buf());
        assert!(cached_local("conv-local-release").is_some());
        release("conv-local-release");
        assert!(
            cached_local("conv-local-release").is_none(),
            "release 必须清掉 local 缓存"
        );
    }

    // --- 桌面能力：档位是唯一事实来源，`[environment] desktop` 只作显式覆盖 ---

    #[test]
    fn remote_desktop_follows_tier_when_unset() {
        // 与 docker 后端 `DockerEnv::capabilities()` 同一套推导：不能一个说有一个说没有。
        assert!(
            remote_desktop(Tier::Desktop, None),
            "desktop 档位默认带桌面"
        );
        assert!(
            !remote_desktop(Tier::Headless, None),
            "headless 档位默认不带桌面"
        );
        assert!(!remote_desktop(Tier::Local, None));
    }

    #[test]
    fn explicit_desktop_overrides_tier_both_ways() {
        // 显式 true + headless → 覆盖为 true（并告警）
        assert!(remote_desktop(Tier::Headless, Some(true)));
        // 显式 false + container → 覆盖为 false（并告警）
        assert!(!remote_desktop(Tier::Desktop, Some(false)));
        // 与档位一致时结果不变
        assert!(remote_desktop(Tier::Desktop, Some(true)));
        assert!(!remote_desktop(Tier::Headless, Some(false)));
    }

    #[test]
    fn envd_capabilities_follow_tier_derived_desktop() {
        // envd 侧的 `capabilities().desktop` 只取决于 `RemoteSpec.desktop`；
        // 后者现在由档位推导，因此 desktop 档的 envd 沙箱同样报"有桌面"。
        let spec_for = |tier| crate::env::remote::RemoteSpec {
            desktop: remote_desktop(tier, None),
            ..Default::default()
        };
        assert!(spec_for(Tier::Desktop).desktop);
        assert!(!spec_for(Tier::Headless).desktop);
    }
}
