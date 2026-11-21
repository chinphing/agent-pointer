//! 执行环境抽象层（P1：只实现 `local`）。
//!
//! 设计来源：`docs/zh-CN/design/environment-sandbox.md` §3（总体架构）/ §4（阶段一）。
//!
//! 分层约定：
//!
//! - `Environment` 是**唯一**的 IO / exec 出口。工具层不允许直接 `std::fs` 或
//!   `Command::new`（验收见设计文档 §4.4）。
//! - 路径策略留在 `tools::file::path`，`env` 只提供根与映射，不反向依赖工具层。
//! - 会话在创建时绑定一个 `Environment`；**会话中途不允许切换 provider**。
//!
//! 为什么用 thread-local 句柄而不是给每个工具加参数：`ToolHandler` 的类型是
//! `Arc<dyn Fn(Value) -> Result<String>>`（见 `tools/mod.rs`），注册只在启动时发生一次，
//! 无法按会话注入参数。这与既有的 `scoped_env_slot!`（`SESSION_USER_ID` / `WORKING_DIR` /
//! `POINTER_ACCESS_HOST`）同一套模式：绑定在分发点重放，见
//! `chat_service/agent_tool_pass/dispatch/registry.rs`。

pub mod docker;
mod local;
pub mod mounts;
pub mod pathmap;
pub mod process;
pub mod provider;
pub mod remote;
pub mod sandbox_registry;
pub mod session_env;

pub use local::LocalEnv;
pub use pathmap::{Mount, MountSpec, PathMap};

use anyhow::{bail, Result};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// 隔离档位。P1 只落地 `Local`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvKind {
    /// 宿主真实桌面，无隔离。
    Local,
    /// namespace + uid + 挂载，无 GUI。
    Headless,
    /// 完整运行时 + GUI（Xvfb + WM）。
    Desktop,
}

impl EnvKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Headless => "headless",
            Self::Desktop => "desktop",
        }
    }
}

/// 读权限范围。非 local 后端下 `resolve_accessible_path` 的绝对路径直通必须由它收口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadScope {
    /// 整台宿主可读（local 现状）。
    Host,
    /// 仅当前用户目录。
    User,
    /// 仅工作区。
    Workspace,
    /// 不可读。
    None,
}

/// 写权限范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteScope {
    /// 宿主可写（local 现状：workspace / home / temp / 用户数据目录）。
    Host,
    /// 仅当前用户目录。
    User,
    /// 仅工作区。
    Workspace,
    /// 不可写。
    None,
}

/// 出网策略。沙箱**默认允许出网**（联网搜索、装包、API 调用都依赖它）；
/// 用户间隔离靠 net namespace，不是断网。需要禁网的离线场景由配置显式关闭。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPolicy {
    Allow,
    Deny,
}

/// 交互式终端的启动参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtySpec {
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
}

impl PtySpec {
    pub fn new(argv: Vec<String>) -> Self {
        Self {
            argv,
            cwd: None,
            env: Vec::new(),
            cols: 80,
            rows: 24,
        }
    }
}

/// 交互式终端句柄：pty 由**环境**分配（宿主用 portable_pty，沙箱用 envd 的
/// `StartRequest.pty`），上层只做读写与 resize，不关心后端。
pub trait PtyHandle: Send + Sync {
    fn id(&self) -> u32;
    fn write(&self, data: &[u8]) -> Result<()>;
    fn resize(&self, cols: u16, rows: u16) -> Result<()>;
    /// 取一次输出流（只能取一次：底层是单消费者通道/文件句柄）。
    fn take_reader(&self) -> Result<Box<dyn std::io::Read + Send>>;
    fn try_wait(&self) -> Result<Option<i32>>;
    fn kill(&self) -> Result<()>;
}

/// 能力集：决定工具列表裁剪（如 `desktop == false` 时不下发 screenshot / click）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub desktop: bool,
    pub read_scope: ReadScope,
    pub write_scope: WriteScope,
    pub snapshot: bool,
    pub network: NetworkPolicy,
}

impl Capabilities {
    /// 宿主本机：可读可写整机，桌面能力取决于是否编入 `computer` feature。
    pub fn local() -> Self {
        Self {
            desktop: cfg!(feature = "computer"),
            read_scope: ReadScope::Host,
            write_scope: WriteScope::Host,
            snapshot: false,
            network: NetworkPolicy::Allow,
        }
    }

    /// headless：无桌面、无快照；出网默认允许（隔离靠 net namespace）。
    pub fn headless() -> Self {
        Self {
            desktop: false,
            read_scope: ReadScope::User,
            write_scope: WriteScope::User,
            snapshot: false,
            network: NetworkPolicy::Allow,
        }
    }

    /// desktop：headless + 桌面。
    ///
    /// `snapshot` 目前是 `false`——快照还没实现（设计文档 §6 列在后续）。
    /// 这里不能写 `true`：能力位是给上层做决策用的，声明了却做不到会让人
    /// 以为"跑完能回滚"。
    pub fn desktop() -> Self {
        Self {
            desktop: true,
            read_scope: ReadScope::User,
            write_scope: WriteScope::User,
            snapshot: false,
            network: NetworkPolicy::Allow,
        }
    }
}

/// 一次 exec 的可选参数。`Default` = 继承宿主 cwd / env，无 stdin，无超时。
#[derive(Debug, Clone, Default)]
pub struct ExecOptions {
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    /// 需要从子进程环境中**删除**的键（例如 git 的 `GIT_DIR` 加固）。
    pub env_remove: Vec<String>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Option<Duration>,
    /// 每条流的字节上限；超出部分丢弃并置 `*_truncated`。
    pub max_output_bytes: Option<usize>,
}

impl ExecOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn without_env(mut self, key: impl Into<String>) -> Self {
        self.env_remove.push(key.into());
        self
    }

    pub fn with_stdin(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(data.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_max_output_bytes(mut self, max: usize) -> Self {
        self.max_output_bytes = Some(max);
        self
    }
}

/// exec 结果。`status` 在无退出码（被信号杀死 / 超时终止）时为 `-1`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// 超时被终止（此时 `status == -1`，已捕获的输出保留）。
    pub timed_out: bool,
}

impl ExecOutput {
    pub fn success(&self) -> bool {
        self.status == 0
    }

    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// 目录项。`path` 是**环境内**路径（非宿主路径），调用方输出给模型前要走反向翻译。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
}

/// 文件元数据（只暴露工具层真正用到的字段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    pub is_file: bool,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub len: u64,
}

/// 目录遍历选项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkOptions {
    /// 深度上限，1 = 只看直接子项。
    pub max_depth: usize,
    /// 是否包含 `.` 开头的隐藏项。
    pub include_hidden: bool,
    /// 是否遵循 `.gitignore` / `.ignore`（grep 需要，glob 不需要）。
    pub respect_gitignore: bool,
    /// 是否跳过 `.git` 元数据目录（grep / glob 需要，`file_list` 不需要）。
    pub exclude_git_metadata: bool,
}

impl WalkOptions {
    pub fn new(max_depth: usize) -> Self {
        Self {
            max_depth: max_depth.max(1),
            include_hidden: false,
            respect_gitignore: false,
            exclude_git_metadata: true,
        }
    }

    pub fn with_hidden(mut self, include_hidden: bool) -> Self {
        self.include_hidden = include_hidden;
        self
    }

    pub fn with_gitignore(mut self, respect_gitignore: bool) -> Self {
        self.respect_gitignore = respect_gitignore;
        self
    }

    pub fn with_git_metadata(mut self, exclude_git_metadata: bool) -> Self {
        self.exclude_git_metadata = exclude_git_metadata;
        self
    }
}

/// 遍历结果项。`is_dir == false` 时一定是常规文件（符号链接等已过滤）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkEntry {
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
}

/// agent 可见的一切 IO / 执行都经过它。
///
/// 约定：
/// - 入参 `path` 是**宿主路径**（工具层已解析完成），provider 内部做 `to_host` / `to_agent`。
/// - 返回错误用 `anyhow`，映射失败统一带 `PATH_NOT_MAPPED`。
/// 常驻子进程的启动参数。
#[derive(Debug, Clone, Default)]
pub struct SpawnOptions {
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

/// 常驻子进程句柄（MCP server / hook 脚本这类要 stdin/stdout 双向流的场景）。
///
/// 设计文档 §3.3：agent 可触发的进程一律在环境内跑。`exec` 是一次性的，
/// 常驻进程必须经 [`Environment::spawn`]。
pub trait ChildHandle: Send {
    fn id(&self) -> u32;
    /// 写 stdin（换行由调用方负责）。
    fn write_all(&mut self, data: &[u8]) -> Result<()>;
    /// 关闭 stdin（子进程读到 EOF 才会退出——一次性脚本必须调）。
    fn close_stdin(&mut self) -> Result<()>;
    /// 取 stdout 读端（每个句柄只能取一次）。
    fn reader(&mut self) -> Result<Box<dyn std::io::BufRead + Send>>;
    /// 已退出则返回退出码。
    fn try_wait(&mut self) -> Result<Option<i32>>;
    /// 阻塞等待退出并返回退出码。
    fn wait(&mut self) -> Result<i32>;
    /// 强杀并回收。
    fn kill(&mut self) -> Result<()>;
}

/// 桌面接管入口。容器档位才有。
#[derive(Debug, Clone)]
pub struct DesktopEndpoint {
    /// `vnc://127.0.0.1:<port>`，交给系统屏幕共享（`open_sandbox_desktop`）。
    pub vnc_url: String,
    /// `ws://127.0.0.1:<port>/websockify`，面板内 noVNC 直连。
    ///
    /// 老镜像（无 websockify）或端口没映射出来时是 `None`——面板据此退回「用系统
    /// 屏幕共享打开」，而不是给用户一个连不上的画面。
    pub ws_url: Option<String>,
    pub password: Option<String>,
}

pub trait Environment: Send + Sync {
    fn kind(&self) -> EnvKind;

    /// 是否是「环境没绑定」的占位实现（[`DeniedEnv`]）。
    ///
    /// `DeniedEnv::kind()` 报 `Headless` 是**刻意**的：调用方靠 `kind() != Local`
    /// 决定"不要碰宿主"。但只凭 kind 分不出"沙箱环境"和"环境压根没绑定"，
    /// 于是漏绑定会被说成"沙箱里没有这个路径"——错误信息指向完全错误的方向。
    /// 需要区分时问这里，别去比对 `kind()`。
    fn is_denied(&self) -> bool {
        false
    }

    /// Active project/worktree path in this Environment namespace.
    ///
    /// For sandboxes this is a compatibility default (`/data/workspaces`); the
    /// authoritative per-conversation root lives in `ResolvedWorkspace`.
    fn workspace_root(&self) -> &Path;
    fn skills_root(&self) -> &Path;
    fn home_root(&self) -> &Path;
    /// Persistent user data root (`/data` in sandbox). Local returns an empty path.
    fn data_root(&self) -> &Path {
        Path::new("")
    }

    fn capabilities(&self) -> Capabilities;

    /// 宿主 ↔ 环境路径翻译表。local 为恒等。
    fn path_map(&self) -> &PathMap;

    /// 桌面接管入口（容器档位才有）。
    ///
    /// 宿主和 headless 档位没有可观察的桌面，返回 `None`。
    fn desktop_endpoint(&self) -> Option<DesktopEndpoint> {
        None
    }

    fn exec(&self, argv: &[String], opts: ExecOptions) -> Result<ExecOutput>;

    /// 起一个常驻子进程（stdin/stdout 双向流）。
    fn spawn(&self, argv: &[String], opts: SpawnOptions) -> Result<Box<dyn ChildHandle>>;

    /// 开一个交互式终端。默认不支持——只有实现了 pty 的环境才能用。
    fn pty_open(&self, _spec: PtySpec) -> Result<Box<dyn PtyHandle>> {
        Err(anyhow::anyhow!(
            "当前执行环境（{:?}）不支持交互式终端",
            self.kind()
        ))
    }
    fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// 读文件但限制体积：超过 `max_bytes` 直接报错，而不是把大文件拉进内存。
    ///
    /// 媒体管线必须用它（设计文档 §7.2：5 GB 视频整段读入会撑爆宿主）。
    fn read_file_capped(&self, path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
        let meta = self.metadata(path)?;
        if meta.len > max_bytes {
            anyhow::bail!(
                "{} 有 {} 字节，超过单次读取上限 {} 字节；大文件请走路径模式（沙箱内处理）",
                path.display(),
                meta.len,
                max_bytes
            );
        }
        self.read_file(path)
    }
    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()>;
    fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;

    /// 元数据。不存在 / 无权限时返回 `Err`。
    fn metadata(&self, path: &Path) -> Result<FileMetadata>;

    /// 该路径是否存在（可访问）。
    fn exists(&self, path: &Path) -> bool {
        self.metadata(path).is_ok()
    }

    /// 解析符号链接后的绝对路径。
    fn canonicalize(&self, path: &Path) -> Result<PathBuf>;

    /// 递归创建目录（已存在视为成功）。
    fn create_dir_all(&self, path: &Path) -> Result<()>;

    /// 读全文（要求合法 UTF-8，与 `fs::read_to_string` 语义一致）。
    fn read_to_string(&self, path: &Path) -> Result<String>;

    /// 流式读句柄：大文件按行扫描时必须用它，避免整文件进内存。
    fn open_reader(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>>;

    /// 批量遍历。**一次调用返回整批**，调用方不得逐目录往返（设计文档 §2.2）。
    fn walk(&self, root: &Path, opts: WalkOptions) -> Result<Vec<WalkEntry>>;

    /// 删除单个文件（不存在视为成功）。
    fn remove_file(&self, path: &Path) -> Result<()>;

    /// 递归删除目录（不存在视为成功）。
    fn remove_dir_all(&self, path: &Path) -> Result<()>;

    /// 标记为可执行（unix 0o700；其他平台 no-op）。
    fn set_executable(&self, path: &Path) -> Result<()>;
}

/// 当前生效的环境：会话绑定了环境就用它，否则用进程级 local 环境。
///
/// 工具层只通过这里拿 IO 能力，不直接碰 `std::fs` / `Command`。
/// 在异步上下文里安全执行**阻塞**工作。
///
/// `reqwest::blocking` 内部会创建/丢弃 tokio Runtime，直接在 async 线程上跑会 panic：
/// `Cannot drop a runtime in a context where blocking is not allowed`。
/// 异步上下文里一律丢到独立线程执行；同步上下文原地执行（不付线程开销）。
pub(crate) fn run_blocking<T, F>(f: F) -> T
where
    F: FnOnce() -> T + Send,
    T: Send,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::scope(|scope| match scope.spawn(f).join() {
            Ok(value) => value,
            Err(panic) => std::panic::resume_unwind(panic),
        })
    } else {
        f()
    }
}

/// 未绑定环境时的**拒绝执行**环境。
///
/// headless/desktop 档位下，一旦某个线程没有会话绑定（tokio 换线程、
/// `spawn_blocking`、新起的后台任务…），旧实现会静默回落到 `LocalEnv`——
/// 命令跑到宿主、工作区指向宿主路径，属于**数据泄露级**的错误。
/// 这里改为显式失败：宁可报错，也不在宿主执行。
struct DeniedEnv {
    workspace: PathBuf,
    home: PathBuf,
    data: PathBuf,
    skills: PathBuf,
    map: PathMap,
}

impl DeniedEnv {
    fn new() -> Self {
        Self {
            workspace: PathBuf::from("/data/workspaces"),
            home: PathBuf::from("/home/pointer"),
            data: PathBuf::from("/data"),
            skills: PathBuf::from("/skills"),
            map: PathMap::identity(),
        }
    }

    fn deny(op: &str) -> anyhow::Error {
        anyhow::anyhow!(
            "执行环境未绑定：当前会话应运行在沙箱里，但该线程上没有绑定（{op} 已拒绝，未在宿主执行）"
        )
    }
}

impl Environment for DeniedEnv {
    fn kind(&self) -> EnvKind {
        EnvKind::Headless
    }
    /// 让调用方能区分「沙箱环境」和「环境没绑定」（见 [`Environment::is_denied`]）。
    fn is_denied(&self) -> bool {
        true
    }
    /// 别让默认实现报「不支持交互式终端」——真正原因是环境没绑定。
    fn pty_open(&self, _spec: PtySpec) -> Result<Box<dyn PtyHandle>> {
        Err(anyhow::anyhow!(
            "执行环境未绑定：当前会话运行在沙箱里，但该线程上没有绑定执行环境（终端已拒绝，未在宿主执行）"
        ))
    }
    fn workspace_root(&self) -> &Path {
        &self.workspace
    }
    fn skills_root(&self) -> &Path {
        &self.skills
    }
    fn home_root(&self) -> &Path {
        &self.home
    }
    fn data_root(&self) -> &Path {
        &self.data
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::headless()
    }
    fn path_map(&self) -> &PathMap {
        &self.map
    }
    fn exec(&self, _argv: &[String], _opts: ExecOptions) -> Result<ExecOutput> {
        Err(Self::deny("exec"))
    }
    fn spawn(&self, _argv: &[String], _opts: SpawnOptions) -> Result<Box<dyn ChildHandle>> {
        Err(Self::deny("spawn"))
    }
    fn read_file(&self, _path: &Path) -> Result<Vec<u8>> {
        Err(Self::deny("read_file"))
    }
    fn write_file(&self, _path: &Path, _data: &[u8]) -> Result<()> {
        Err(Self::deny("write_file"))
    }
    fn list_dir(&self, _path: &Path) -> Result<Vec<DirEntry>> {
        Err(Self::deny("list_dir"))
    }
    fn metadata(&self, _path: &Path) -> Result<FileMetadata> {
        Err(Self::deny("metadata"))
    }
    fn canonicalize(&self, _path: &Path) -> Result<PathBuf> {
        Err(Self::deny("canonicalize"))
    }
    fn create_dir_all(&self, _path: &Path) -> Result<()> {
        Err(Self::deny("create_dir_all"))
    }
    fn read_to_string(&self, _path: &Path) -> Result<String> {
        Err(Self::deny("read_to_string"))
    }
    fn open_reader(&self, _path: &Path) -> Result<Box<dyn std::io::Read + Send>> {
        Err(Self::deny("open_reader"))
    }
    fn walk(&self, _root: &Path, _opts: WalkOptions) -> Result<Vec<WalkEntry>> {
        Err(Self::deny("walk"))
    }
    fn remove_file(&self, _path: &Path) -> Result<()> {
        Err(Self::deny("remove_file"))
    }
    fn remove_dir_all(&self, _path: &Path) -> Result<()> {
        Err(Self::deny("remove_dir_all"))
    }
    fn set_executable(&self, _path: &Path) -> Result<()> {
        Err(Self::deny("set_executable"))
    }
}

/// 测试用：一个 kind=Headless 但不触碰宿主的环境（所有 IO 都拒绝）。
#[cfg(test)]
pub(crate) fn test_light_env() -> Arc<dyn Environment> {
    Arc::new(DeniedEnv::new())
}

/// 部署档位是否为 headless/desktop（只读一次环境变量，进程内不变）。
fn deployment_is_remote() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        crate::env::provider::ProviderConfig::from_env()
            .ok()
            .and_then(|cfg| cfg.resolve(None).ok())
            .map(|r| r.tier != crate::env::provider::Tier::Local)
            .unwrap_or(false)
    })
}

/// 宿主侧执行准入的纯判定（便于单测，不读全局配置）。
///
/// `bound` 是该会话已记录的执行环境类型，`None` = 找不到该会话的环境记录。
/// - 绑定为 `local`：允许，宿主本身就是它的执行环境。
/// - 绑定为 `headless` / `desktop`：拒绝。沙箱会话的动作不得落到宿主。
/// - 找不到记录：远程部署下拒绝（不知道会话属于哪个档位，宁可失败）；
///   standalone 部署只有宿主一个环境，允许。
pub(crate) fn decide_host_execution(
    bound: Option<EnvKind>,
    remote_deployment: bool,
    what: &str,
) -> Result<()> {
    match bound {
        Some(EnvKind::Local) => Ok(()),
        Some(kind) => {
            log::warn!("host execution refused: {what} bound_kind={}", kind.as_str());
            bail!(
                "{what}只能在宿主（local 档位）执行，当前会话运行在 {} 环境，已拒绝在宿主执行",
                kind.as_str()
            )
        }
        None if remote_deployment => {
            log::warn!("host execution refused: {what} no bound environment (remote deployment)");
            bail!("{what}需要会话的执行环境，但该会话没有环境记录，远程部署下已拒绝在宿主执行")
        }
        None => Ok(()),
    }
}

/// 会话即将在**宿主**上执行某个动作时必须先过这一关（computer 桌面 / 剪贴板 / 截屏、
/// MCP 子进程、插件 hook 脚本等）。
///
/// 没有这一关时，沙箱会话的请求会静默回落到宿主（见设计文档 §3.3「不存在宿主侧例外」）。
pub fn ensure_host_execution_allowed(conversation_id: &str, what: &str) -> Result<()> {
    let bound = session_env::env_for_conversation(conversation_id).map(|env| env.kind());
    decide_host_execution(bound, deployment_is_remote(), what)
}

/// 指定工作区的宿主环境。只用于本地档位：路径就是本机路径。
///
/// 沙箱档位的项目和数据在容器里的 `/data` 卷上，不是本机目录的 bind mount。
/// 容器路径必须走会话环境，不要用这个函数去打开 `/data/...`。
pub fn host_env_at(root: &std::path::Path) -> Arc<dyn Environment> {
    Arc::new(local::LocalEnv::new(root.to_path_buf()))
}

/// 宿主本机环境（**固定**，不随部署档位变化）。
///
/// 给本地档位和进程自身的数据整理用。沙箱会话里的 git、文件工具和 stdio MCP
/// 走会话环境，不要用这个去执行容器里的路径。
pub fn host_env() -> Arc<dyn Environment> {
    static HOST: OnceLock<Arc<dyn Environment>> = OnceLock::new();
    HOST.get_or_init(|| Arc::new(LocalEnv::from_settings()) as Arc<dyn Environment>)
        .clone()
}

/// 当前生效的环境：本线程绑过就用绑定的，否则**按部署档位兜底**。
///
/// 兜底出来的实例可能与会话无关（local 部署 → 与当前会话无关的 `LocalEnv`；
/// 沙箱部署 → [`DeniedEnv`]）。所以工具执行路径一律用 [`require_env`]：
/// 分发点已经绑过环境，兜底只会把「漏绑定」伪装成另一个错误。
///
/// 仍然合法的用途：UI 面板 / 宿主专属能力 / 还没解析会话档位的早期阶段。
/// **工具层不允许新增调用**（守卫见 `tests::tool_and_chat_layers_do_not_fall_back_to_effective_env`）。
pub fn effective_env() -> Arc<dyn Environment> {
    if let Some(env) = current_env() {
        return env;
    }
    // headless/desktop 档位绝不能静默回落宿主：返回拒绝执行的环境，把漏绑定变成显式错误。
    if deployment_is_remote() {
        static DENIED: OnceLock<Arc<dyn Environment>> = OnceLock::new();
        return DENIED
            .get_or_init(|| Arc::new(DeniedEnv::new()) as Arc<dyn Environment>)
            .clone();
    }
    static DEFAULT_LOCAL: OnceLock<Arc<dyn Environment>> = OnceLock::new();
    DEFAULT_LOCAL
        .get_or_init(|| Arc::new(LocalEnv::from_settings()) as Arc<dyn Environment>)
        .clone()
}

/// 取**当前线程已绑定**的执行环境；没绑定就报错，不按部署档位兜底。
///
/// 与 [`effective_env`] 的唯一区别就是「没绑定时怎么办」：这里直接失败。
/// 工具执行路径（terminal / file / media …）在分发点已经绑过环境，兜底只会把
/// 漏绑定藏成别处的错误——沙箱部署下是 `DeniedEnv` 报「工作区不存在」，local
/// 部署下是一个与会话无关的 `LocalEnv`，把命令跑到宿主上。
///
/// 错误文案刻意**不复用**工作区那套（[`WorkspaceDirState::Unbound`]）：那是
/// 「工作区不可用」的措辞，这里是「调用方要求显式绑定」的契约问题，指向不同。
pub fn require_env() -> Result<Arc<dyn Environment>> {
    current_env().ok_or_else(|| {
        anyhow::anyhow!(
            "执行环境未绑定，且调用方要求显式绑定：该入口不会按部署档位回落到默认环境\
             （请先在该线程上绑定会话环境，见 env::session_env::bind_cached / bind_and_resolve_workspace）"
        )
    })
}

/// 工作区目录的检查结果。
///
/// 三种失败必须分开：以前"环境没绑定"和"沙箱里没有这个路径"都被报成
/// 「工作区目录无效或不存在」，可实际查的是**宿主**路径——报错把人带到完全
/// 错误的方向（见 `docs` 里 admin + 沙箱档位部署的排查记录）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceDirState {
    /// 目录可用。
    Present,
    /// 本线程没有绑定执行环境，而部署是沙箱档位（[`DeniedEnv`]）。
    Unbound,
    /// 环境里确实没有这个目录。
    Missing,
}

impl WorkspaceDirState {
    /// 失败时给出可直接返回给用户的错误；`Present` 返回 `None`。
    ///
    /// `kind` 只用来选文案：`Local` 的路径是宿主路径，其余是沙箱 guest 路径。
    pub fn error(self, kind: EnvKind, workspace: &Path) -> Option<anyhow::Error> {
        match self {
            Self::Present => None,
            Self::Unbound => Some(anyhow::anyhow!(
                "执行环境未绑定：本部署配置了沙箱档位，但当前会话没有绑定执行环境"
            )),
            Self::Missing if kind == EnvKind::Local => Some(anyhow::anyhow!(
                "工作区目录无效或不存在: {}",
                workspace.display()
            )),
            Self::Missing => Some(anyhow::anyhow!(
                "沙箱内不存在工作区：{}",
                workspace.display()
            )),
        }
    }
}

/// 判定工作区目录是否可用。
///
/// `env` 传 `None` 表示"本线程没有绑定环境，按宿主判断"——与
/// `tools::file::path::env_is_dir` 同一套约定，不能在这里自作主张换成
/// [`effective_env`]：后者在未绑定时按**部署默认档位**返回 [`DeniedEnv`]。
pub fn workspace_dir_state(env: Option<&dyn Environment>, path: &Path) -> WorkspaceDirState {
    let Some(env) = env else {
        return if path.is_dir() {
            WorkspaceDirState::Present
        } else {
            WorkspaceDirState::Missing
        };
    };
    if env.is_denied() {
        return WorkspaceDirState::Unbound;
    }
    let present = if env.kind() == EnvKind::Local {
        path.is_dir()
    } else {
        env.metadata(path).map(|m| m.is_dir).unwrap_or(false)
    };
    if present {
        WorkspaceDirState::Present
    } else {
        WorkspaceDirState::Missing
    }
}

/// [`workspace_dir_state`] + 对应文案：可用返回 `None`，否则返回要抛的错误。
pub fn workspace_dir_error(env: Option<&dyn Environment>, path: &Path) -> Option<anyhow::Error> {
    let kind = env.map(|e| e.kind()).unwrap_or(EnvKind::Local);
    workspace_dir_state(env, path).error(kind, path)
}

thread_local! {
    static CURRENT_ENV: RefCell<Option<Arc<dyn Environment>>> = const { RefCell::new(None) };
}

/// 会话级 `Environment` 绑定。与 `ConversationWorkspaceGuard` 同一套语义：
/// tokio 任务换线程后 thread-local 不会跟随，**每个重新进入的分发点都要重放**。
pub struct EnvironmentGuard {
    previous: Option<Arc<dyn Environment>>,
}

impl EnvironmentGuard {
    pub fn enter(env: Arc<dyn Environment>) -> Self {
        let previous = CURRENT_ENV.with(|slot| slot.borrow_mut().replace(env));
        Self { previous }
    }

    /// 清除绑定（会话结束 / 测试收尾），`Drop` 时恢复原值。
    pub fn clear() -> Self {
        let previous = CURRENT_ENV.with(|slot| slot.borrow_mut().take());
        Self { previous }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT_ENV.with(|slot| {
            *slot.borrow_mut() = previous;
        });
    }
}

/// 当前线程绑定的环境；未绑定时返回 `None`（local 直跑）。
pub fn current_env() -> Option<Arc<dyn Environment>> {
    CURRENT_ENV.with(|slot| slot.borrow().clone())
}

/// 是否已绑定非默认环境。
pub fn is_bound() -> bool {
    CURRENT_ENV.with(|slot| slot.borrow().is_some())
}

/// 提示词 `{{workspace_root}}` 的取值来源。
///
/// 已绑定环境时用 `env.workspace_root()`；否则退回调用方给的 settings 值（当前行为）。
pub fn prompt_workspace_root(fallback: &str) -> String {
    match current_env() {
        Some(env) => env.workspace_root().display().to_string(),
        None => fallback.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_env(dir: &Path) -> Arc<dyn Environment> {
        Arc::new(LocalEnv::new(dir))
    }

    #[test]
    fn prompt_workspace_root_falls_back_without_binding() {
        assert!(!is_bound());
        assert_eq!(prompt_workspace_root("  /tmp/ws  "), "/tmp/ws");
    }

    #[test]
    fn require_env_refuses_to_fall_back_when_unbound() {
        // 严格入口：没绑定就报错，绝不按部署档位兜底。
        assert!(!is_bound());
        let err = require_env()
            .map(|_| ())
            .unwrap_err()
            .to_string();
        assert!(err.contains("执行环境未绑定"), "{err}");
        assert!(err.contains("要求显式绑定"), "{err}");
        // 不能复用工作区那套文案：那是「路径不可用」，会把人带偏。
        assert!(!err.contains("工作区"), "{err}");
        assert!(!err.contains("沙箱内不存在"), "{err}");
    }

    #[test]
    fn require_env_returns_the_bound_env() {
        let dir = tempfile::tempdir().unwrap();
        let _g = EnvironmentGuard::enter(temp_env(dir.path()));
        let env = require_env().expect("绑定后必须拿得到");
        assert_eq!(env.kind(), EnvKind::Local);
        assert_eq!(env.workspace_root(), dir.path());
    }

    #[test]
    fn guard_binds_env_and_restores_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let env = temp_env(dir.path());
        {
            let _g = EnvironmentGuard::enter(env.clone());
            assert!(is_bound());
            assert_eq!(
                prompt_workspace_root("/ignored"),
                dir.path().display().to_string()
            );
        }
        assert!(!is_bound());
        assert_eq!(prompt_workspace_root("/fallback"), "/fallback");
    }

    #[test]
    fn nested_guards_restore_outer_binding() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let _outer = EnvironmentGuard::enter(temp_env(a.path()));
        {
            let _inner = EnvironmentGuard::enter(temp_env(b.path()));
            assert_eq!(
                current_env().unwrap().workspace_root(),
                b.path()
            );
        }
        assert_eq!(current_env().unwrap().workspace_root(), a.path());
    }

    #[test]
    fn capabilities_match_kind() {
        assert_eq!(Capabilities::local().read_scope, ReadScope::Host);
        assert!(!Capabilities::headless().desktop);
        assert!(Capabilities::desktop().desktop);
        assert_eq!(EnvKind::Local.as_str(), "local");
    }

    /// 宿主准入判定的全部组合：沙箱会话绝不能放行到宿主。
    #[test]
    fn host_execution_decision_matrix() {
        // local 会话：宿主就是它的环境，任何部署都放行。
        assert!(decide_host_execution(Some(EnvKind::Local), true, "x").is_ok());
        assert!(decide_host_execution(Some(EnvKind::Local), false, "x").is_ok());
        // 沙箱会话：无论部署模式都拒绝（headless / desktop 都不允许落宿主）。
        for kind in [EnvKind::Headless, EnvKind::Desktop] {
            for remote in [true, false] {
                let err = decide_host_execution(Some(kind), remote, "屏幕截图")
                    .expect_err("沙箱会话不能落宿主")
                    .to_string();
                assert!(err.contains("已拒绝在宿主执行"), "{err}");
                assert!(err.contains(kind.as_str()), "{err}");
            }
        }
        // 找不到会话环境记录：远程部署拒绝，standalone 放行。
        let err = decide_host_execution(None, true, "插件 hook 脚本")
            .expect_err("远程部署下无记录必须拒绝")
            .to_string();
        assert!(err.contains("没有环境记录"), "{err}");
        assert!(decide_host_execution(None, false, "插件 hook 脚本").is_ok());
    }

    /// 会话登记为 headless 时，宿主准入必须失败（模拟「沙箱会话请求宿主桌面」）。
    #[test]
    fn headless_conversation_cannot_reach_host_desktop() {
        let cid = "env-mod-headless-host-guard";
        session_env::remember_env_for_test(cid, test_light_env());
        assert!(
            ensure_host_execution_allowed(cid, "键盘操作").is_err(),
            "headless 会话不能被放行到宿主"
        );
        session_env::release(cid);
    }

    /// 会话登记为 local 时，宿主准入放行（admin / standalone 的正常路径）。
    #[test]
    fn local_conversation_may_use_host() {
        let dir = tempfile::tempdir().unwrap();
        let cid = "env-mod-local-host-guard";
        session_env::remember_local_for_test(cid, dir.path());
        assert!(ensure_host_execution_allowed(cid, "键盘操作").is_ok());
        session_env::release(cid);
    }

    #[test]
    fn denied_env_refuses_host_io_instead_of_leaking() {
        // headless 档位下没绑定会话环境时，绝不能静默回落宿主。
        let env = DeniedEnv::new();
        assert_eq!(env.kind(), EnvKind::Headless);
        assert_eq!(env.workspace_root(), Path::new("/data/workspaces"));
        let err = env
            .exec(&["/bin/echo".to_string(), "hi".to_string()], ExecOptions::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("未绑定"), "错误应说明未绑定：{err}");
        assert!(env.read_file(Path::new("/etc/hostname")).is_err());
        assert!(env.write_file(Path::new("/tmp/x"), b"x").is_err());
    }

    #[test]
    fn denied_env_is_identifiable() {
        // 光看 kind() 分不出"沙箱"和"没绑定"——必须能问出来。
        let denied = DeniedEnv::new();
        assert!(denied.is_denied());
        let dir = tempfile::tempdir().unwrap();
        assert!(!LocalEnv::new(dir.path()).is_denied());
    }

    #[test]
    fn workspace_dir_state_separates_unbound_from_missing() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path();
        let missing = dir.path().join("nope");

        // 没绑定环境：按宿主判断（local 账号的老行为）。
        assert_eq!(workspace_dir_state(None, present), WorkspaceDirState::Present);
        assert_eq!(workspace_dir_state(None, &missing), WorkspaceDirState::Missing);

        // 绑了 local 环境：宿主判断。
        let local = LocalEnv::new(dir.path());
        assert_eq!(
            workspace_dir_state(Some(&local), present),
            WorkspaceDirState::Present
        );
        assert_eq!(
            workspace_dir_state(Some(&local), &missing),
            WorkspaceDirState::Missing
        );

        // 没绑定的沙箱档位：必须报「未绑定」，不能报「不存在」。
        let denied = DeniedEnv::new();
        assert_eq!(
            workspace_dir_state(Some(&denied), present),
            WorkspaceDirState::Unbound
        );
    }

    #[test]
    fn workspace_dir_error_names_the_real_cause() {
        let ws = Path::new("/tmp/pointer-ws-error-text");
        let unbound = WorkspaceDirState::Unbound
            .error(EnvKind::Headless, ws)
            .unwrap()
            .to_string();
        assert!(unbound.contains("执行环境未绑定"), "{unbound}");
        assert!(!unbound.contains("工作区目录无效"), "{unbound}");

        let missing_sandbox = WorkspaceDirState::Missing
            .error(EnvKind::Headless, ws)
            .unwrap()
            .to_string();
        assert!(missing_sandbox.contains("沙箱内不存在工作区"), "{missing_sandbox}");

        let missing_host = WorkspaceDirState::Missing
            .error(EnvKind::Local, ws)
            .unwrap()
            .to_string();
        assert!(
            missing_host.contains("工作区目录无效或不存在"),
            "{missing_host}"
        );

        assert!(WorkspaceDirState::Present.error(EnvKind::Local, ws).is_none());
    }

    /// 递归收集被守卫文件里的宿主直连。
    fn scan_bypass(
        dir: &Path,
        root: &Path,
        allowed: &[(&str, &str)],
        patterns: &[&str],
        out: &mut Vec<String>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_bypass(&path, root, allowed, patterns, out);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if allowed.iter().any(|(f, _)| *f == rel) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            // 只看非测试代码：从第一个 `#[cfg(test)]` 起截断
            let prod = match text.find("#[cfg(test)]") {
                Some(i) => &text[..i],
                None => &text[..],
            };
            for (idx, line_text) in prod.lines().enumerate() {
                let trimmed = line_text.trim_start();
                // 注释里提到 API 名字不算违规
                if trimmed.starts_with("//") {
                    continue;
                }
                for pat in patterns {
                    if line_text.contains(pat) {
                        out.push(format!("{rel}:{} 含 `{pat}`", idx + 1));
                        break;
                    }
                }
            }
        }
    }

    /// 架构守卫：工具层 / 媒体层 / coder 技能不得绕过 `Environment` 直连宿主。
    ///
    /// 白名单里的文件都是「本来就该在宿主上」的：应用数据、附件本体、提权交互。
    /// 新增白名单必须在理由里写清楚，否则下次泄露没人能追。
    #[test]
    fn tool_and_media_layers_do_not_bypass_environment() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let allowed: &[(&str, &str)] = &[
            ("media/store.rs", "媒体库落盘在应用数据目录"),
            ("media/oss.rs", "上传缓存，宿主侧"),
            ("media/public_download.rs", "媒体库文件读写"),
            ("media/media_ref.rs", "附件本体路径"),
            ("media/exec.rs", "local 档位暂存读回（remote 分支已走 env）"),
            ("media/image_dir.rs", "附件目录枚举（宿主路径）"),
            ("tools/terminal_elevated.rs", "提权是宿主专属能力"),
            ("tools/terminal_askpass.rs", "提权口令交互，宿主专属"),
            ("media/ffmpeg.rs", "仅 local 档位走宿主 which；headless 已走 env.exec"),
        ];
        let patterns = [
            "std::fs::",
            "tokio::fs::",
            "fs::read",
            "fs::write",
            "fs::metadata",
            "fs::read_dir",
            "Command::new",
            "std::process::Command",
        ];
        let mut violations = Vec::new();
        for dir in ["tools", "media", "agents/coder"] {
            scan_bypass(&root.join(dir), &root, allowed, &patterns, &mut violations);
        }
        violations.sort();
        violations.dedup();
        assert!(
            violations.is_empty(),
            "以下位置绕过 Environment 直连宿主（请改走 env::effective_env() 或加入白名单并写明理由）：\n{}",
            violations.join("\n")
        );
    }

    /// 递归收集「生产代码里出现 `symbol`」的位置（文件 + 行号）。
    ///
    /// `target` 可以是文件也可以是目录；只扫 `.rs`。生产代码 = 第一个**行首**
    /// `#[cfg(test)]` 之前的部分（`#[allow(dead_code)] // 仅 #[cfg(test)] 引用`
    /// 这类注释不算模块边界），注释行本身跳过。
    fn scan_symbol_hits(target: &Path, root: &Path, symbol: &str, out: &mut Vec<(String, usize)>) {
        if target.is_dir() {
            let Ok(entries) = std::fs::read_dir(target) else {
                return;
            };
            for entry in entries.flatten() {
                scan_symbol_hits(&entry.path(), root, symbol, out);
            }
            return;
        }
        if target.extension().and_then(|e| e.to_str()) != Some("rs") {
            return;
        }
        let Ok(text) = std::fs::read_to_string(target) else {
            return;
        };
        let rel = target
            .strip_prefix(root)
            .unwrap_or(target)
            .to_string_lossy()
            .replace('\\', "/");
        // 生产代码：第一个行首 `#[cfg(test)]` 之前。
        let mut prod_end = text.len();
        let mut offset = 0usize;
        for line in text.split_inclusive('\n') {
            if line.trim_start().starts_with("#[cfg(test)]") {
                prod_end = offset;
                break;
            }
            offset += line.len();
        }
        for (idx, line) in text[..prod_end].lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains(symbol) {
                out.push((rel.clone(), idx + 1));
            }
        }
    }

    /// 架构守卫：工具层 / 会话层不得回落到 `effective_env()`。
    ///
    /// `effective_env()` 在「本线程没绑定」时会按**部署档位**兜底：沙箱部署下给出
    /// `DeniedEnv`（把漏绑定伪装成「沙箱里没这个路径」），local 部署下给出一个与
    /// 会话无关的 `LocalEnv`（命令可能跑到宿主上）。工具执行路径上分发点已经绑过
    /// 环境，所以这些入口一律用 [`require_env`]：拿不到就显式失败。
    ///
    /// 白名单是「确实要按部署档位兜底」的位置，每条都要写理由；并且**必须仍然存在**
    /// ——迁移走之后忘删白名单，守卫就静默失效了。
    ///
    /// 扫描范围含 `agents/` 与 `plugins/`：前者是工具实现（`read_lints` / `computer`），
    /// 后者是插件 / MCP / hook 的子进程 spawn 点，都在工具执行路径或会话回合上。
    /// `skills/mod.rs`（技能加载）与 `turn_file_baseline.rs`（回合文件基线）同属会话
    /// 回合路径，一并扫，防迁移后回潮。
    ///
    /// 注意 `env/docker.rs` 里的 `self.effective_env(&opts.env)` 是**同名方法**
    /// （环境变量合并），与本主题无关；`env/` 不在扫描目标里，不会被误判。
    #[test]
    fn tool_and_chat_layers_do_not_fall_back_to_effective_env() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        // 白名单：**空**。工具层 / 会话层已经全部改成显式环境
        // （`env::require_env()` 或显式宿主 / 显式拒绝），不再有「按部署档位兜底」
        // 的合法位置。新增条目必须先写清为什么这条**必须**兜底。
        let allowed: &[(&str, &str)] = &[];
        let mut hits: Vec<(String, usize)> = Vec::new();
        for target in [
            "crates/pointer-core/src/tools",
            "crates/pointer-core/src/chat_service",
            "crates/pointer-core/src/agents",
            "crates/pointer-core/src/plugins",
            "crates/pointer-core/src/console_session.rs",
            // 技能加载 / 回合文件基线也是会话回合路径，曾经各自回落到
            // `effective_env()`；已迁到显式环境，列进来防回潮。
            "crates/pointer-core/src/skills/mod.rs",
            "crates/pointer-core/src/turn_file_baseline.rs",
            "src-tauri/src",
        ] {
            scan_symbol_hits(&repo.join(target), &repo, "effective_env", &mut hits);
        }

        let mut violations: Vec<String> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for (file, line) in &hits {
            if !seen.contains(file) {
                seen.push(file.clone());
            }
            if allowed.iter().any(|(f, _)| f == file) {
                continue;
            }
            violations.push(format!(
                "{file}:{line} 调用了 effective_env（改用 env::require_env()?，或在白名单登记并写理由）"
            ));
        }
        let mut stale: Vec<&str> = Vec::new();
        for (file, _) in allowed {
            if !seen.iter().any(|s| s == file) {
                stale.push(file);
            }
        }

        assert!(
            violations.is_empty() && stale.is_empty(),
            "工具/会话层不得回落到 effective_env()（未绑定时会按部署档位兜底，把漏绑定藏成别的错误）\n\
             违规：\n{}\n白名单里已不存在（请删除该条）：{:?}",
            violations.join("\n"),
            stale
        );
    }
}
