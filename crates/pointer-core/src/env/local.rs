//! `local` 档位实现：宿主真实路径 + `std::fs` + `Command`。
//!
//! 这一层是**唯一**允许直接碰 `std::fs` / `Command::new` 的地方（设计文档 §4.4 验收）。

use anyhow::{anyhow, Context, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{
    Capabilities, ChildHandle, DirEntry, EnvKind, Environment, ExecOptions, ExecOutput,
    FileMetadata, PathMap, SpawnOptions, WalkEntry, WalkOptions,
};

/// 宿主本机环境。路径映射恒等。
#[derive(Debug, Clone)]
pub struct LocalEnv {
    workspace_root: PathBuf,
    skills_root: PathBuf,
    home_root: PathBuf,
    data_root: PathBuf,
    path_map: PathMap,
    capabilities: Capabilities,
}

impl LocalEnv {
    /// 用工作区构造，skills / home 取宿主默认值。
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        let home_root = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
        let skills_root = home_root.join(".pointer").join("skills");
        Self::with_roots(workspace_root, skills_root, home_root)
    }

    pub fn with_roots(
        workspace_root: impl Into<PathBuf>,
        skills_root: impl Into<PathBuf>,
        home_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            skills_root: skills_root.into(),
            home_root: home_root.into(),
            data_root: PathBuf::new(),
            path_map: PathMap::identity(),
            capabilities: Capabilities::local(),
        }
    }

    /// 用户会话的本地环境。可见范围只有这个用户的 home 和 data。
    ///
    /// `home_root()` / `data_root()` 就是这两棵目录的宿主路径。路径表未命中即拒绝。
    pub fn for_user(
        home: impl Into<PathBuf>,
        data: impl Into<PathBuf>,
        workspace: impl Into<PathBuf>,
    ) -> Self {
        let home = existing_canonical(home.into());
        let data = existing_canonical(data.into());
        let workspace = existing_canonical(workspace.into());
        let mounts = vec![
            super::Mount::new(&home, &home, false),
            super::Mount::new(&data, &data, false),
        ];
        Self {
            skills_root: home.join(".pointer").join("skills"),
            home_root: home,
            data_root: data,
            workspace_root: workspace,
            path_map: PathMap::new(mounts),
            capabilities: Capabilities {
                read_scope: super::ReadScope::User,
                write_scope: super::WriteScope::User,
                ..Capabilities::local()
            },
        }
    }

    /// 覆盖能力集。测试与非 Host 权限档位（如「只允许工作区读写」）用它收口路径策略。
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// 从生效设置取工作区（`settings.workspaceRoot`）；为空时退回 home。
    ///
    /// 注意：**不**读会话级 override（那是 `tools::file::path` 的 thread-local），
    /// 会话绑定时由调用方显式传工作区。
    pub fn from_settings() -> Self {
        let raw = crate::platform_config::effective_settings_global()
            .workspace_root
            .trim()
            .to_string();
        if raw.is_empty() {
            Self::new(dirs::home_dir().unwrap_or_else(std::env::temp_dir))
        } else {
            Self::new(raw)
        }
    }

    /// 整机恒等映射直接放行。用户会话先把路径收进 home/data，再执行。
    fn visible(&self, path: &Path) -> Result<PathBuf> {
        if self.path_map.is_host_passthrough() {
            return Ok(path.to_path_buf());
        }
        self.path_map
            .to_host(path)
            .with_context(|| format!("路径不在当前用户的 home 或 data 内: {}", path.display()))
    }

    fn prepare_process(
        &self,
        argv: &[String],
        cwd: Option<&Path>,
    ) -> Result<(Vec<String>, PathBuf)> {
        if self.path_map.is_host_passthrough() {
            let cwd = cwd
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.workspace_root.clone());
            return Ok((argv.to_vec(), cwd));
        }
        let requested = cwd
            .map(Path::to_path_buf)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| self.workspace_root.clone());
        let cwd = self.visible(&requested)?;
        let argv = confine_argv(&self.home_root, &self.data_root, &cwd, argv)?;
        Ok((argv, cwd))
    }

    /// 用户会话里的进程使用这份 home，避免读到操作系统用户的家目录。
    fn confined_env_pairs(&self) -> Vec<(String, String)> {
        if self.path_map.is_host_passthrough() {
            return Vec::new();
        }
        vec![
            ("HOME".into(), self.home_root.display().to_string()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ]
    }
}

/// `local` 档位的常驻子进程：直接包一层 `std::process::Child`。
struct LocalChild {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    stdout: Option<std::process::ChildStdout>,
}

impl ChildHandle for LocalChild {
    fn id(&self) -> u32 {
        self.child.id()
    }

    fn write_all(&mut self, data: &[u8]) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("子进程 stdin 已关闭"))?;
        stdin.write_all(data)?;
        stdin.flush()?;
        Ok(())
    }

    fn close_stdin(&mut self) -> Result<()> {
        self.stdin = None;
        Ok(())
    }

    fn reader(&mut self) -> Result<Box<dyn std::io::BufRead + Send>> {
        let stdout = self
            .stdout
            .take()
            .ok_or_else(|| anyhow!("子进程 stdout 已被取走"))?;
        Ok(Box::new(std::io::BufReader::new(stdout)))
    }

    fn try_wait(&mut self) -> Result<Option<i32>> {
        Ok(self.child.try_wait()?.map(|s| s.code().unwrap_or(-1)))
    }

    fn wait(&mut self) -> Result<i32> {
        Ok(self.child.wait()?.code().unwrap_or(-1))
    }

    fn kill(&mut self) -> Result<()> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

/// 宿主 PTY 句柄（portable_pty）。
struct LocalPty {
    pid: u32,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    writer: Mutex<Box<dyn std::io::Write + Send>>,
    reader_taken: AtomicBool,
}

impl crate::env::PtyHandle for LocalPty {
    fn id(&self) -> u32 {
        self.pid
    }
    fn write(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }
    fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .resize(portable_pty::PtySize {
                cols: cols.max(1),
                rows: rows.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })?;
        Ok(())
    }
    fn take_reader(&self) -> Result<Box<dyn std::io::Read + Send>> {
        if self.reader_taken.swap(true, Ordering::SeqCst) {
            anyhow::bail!("pty 输出流只能取一次");
        }
        let reader = self
            .master
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_clone_reader()?;
        Ok(reader)
    }
    fn try_wait(&self) -> Result<Option<i32>> {
        match self
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_wait()
        {
            Ok(Some(status)) => Ok(Some(status.exit_code() as i32)),
            Ok(None) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
    fn kill(&self) -> Result<()> {
        self.child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .kill()
            .map_err(|e| anyhow!("kill pty: {e}"))
    }
}

impl Environment for LocalEnv {
    fn kind(&self) -> EnvKind {
        EnvKind::Local
    }

    fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    fn skills_root(&self) -> &Path {
        &self.skills_root
    }

    fn home_root(&self) -> &Path {
        &self.home_root
    }

    fn data_root(&self) -> &Path {
        &self.data_root
    }

    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn path_map(&self) -> &PathMap {
        &self.path_map
    }

    fn exec(&self, argv: &[String], opts: ExecOptions) -> Result<ExecOutput> {
        let (argv, cwd) = self.prepare_process(argv, opts.cwd.as_deref())?;
        let mut opts = opts;
        opts.cwd = Some(cwd);
        for (key, value) in self.confined_env_pairs() {
            opts.env.entry(key).or_insert(value);
        }
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| anyhow!("exec 需要非空 argv"))?;
        let mut cmd = Command::new(program);
        cmd.args(args);
        if let Some(cwd) = &opts.cwd {
            cmd.current_dir(cwd);
        }
        for (key, value) in &opts.env {
            cmd.env(key, value);
        }
        for key in &opts.env_remove {
            cmd.env_remove(key);
        }
        // Windows 上不能弹控制台窗口（所有工具共用的语义）。
        crate::env::process::suppress_console_window(&mut cmd);
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if opts.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        let mut child = cmd
            .spawn()
            .with_context(|| format!("无法启动命令: {program}"))?;

        // 管道必须并发读，否则子进程写满缓冲区后永远不退出。
        let cap = opts.max_output_bytes;
        let out_handle = child
            .stdout
            .take()
            .map(|pipe| std::thread::spawn(move || copy_limited(pipe, cap)));
        let err_handle = child
            .stderr
            .take()
            .map(|pipe| std::thread::spawn(move || copy_limited(pipe, cap)));
        let stdin_handle = match (opts.stdin.clone(), child.stdin.take()) {
            (Some(data), Some(mut pipe)) => Some(std::thread::spawn(move || {
                let _ = pipe.write_all(&data);
            })),
            _ => None,
        };

        let (status, timed_out) = wait_with_timeout(&mut child, opts.timeout)?;
        if let Some(handle) = stdin_handle {
            let _ = handle.join();
        }
        let (stdout, stdout_truncated) = out_handle.and_then(|h| h.join().ok()).unwrap_or_default();
        let (stderr, stderr_truncated) = err_handle.and_then(|h| h.join().ok()).unwrap_or_default();
        Ok(ExecOutput {
            status,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            timed_out,
        })
    }

    fn spawn(&self, argv: &[String], opts: SpawnOptions) -> Result<Box<dyn ChildHandle>> {
        let (argv, cwd) = self.prepare_process(argv, opts.cwd.as_deref())?;
        let program = argv
            .first()
            .ok_or_else(|| anyhow!("spawn 需要可执行文件"))?;
        let mut cmd = Command::new(program);
        cmd.args(&argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(&cwd);
        for (k, v) in self.confined_env_pairs() {
            cmd.env(k, v);
        }
        for (k, v) in &opts.env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().with_context(|| format!("spawn {program}"))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        // stderr 在后台排空：不阻塞管道，也不会让子进程写 stderr 时拿到 EPIPE。
        if let Some(mut stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = std::io::Read::read(&mut stderr, &mut buf) {
                    if n == 0 {
                        break;
                    }
                }
            });
        }
        Ok(Box::new(LocalChild {
            child,
            stdin,
            stdout,
        }))
    }

    fn pty_open(&self, spec: crate::env::PtySpec) -> Result<Box<dyn crate::env::PtyHandle>> {
        use portable_pty::{native_pty_system, CommandBuilder, PtySize};
        let (argv, cwd) = self.prepare_process(&spec.argv, spec.cwd.as_deref())?;
        let mut spec = spec;
        spec.argv = argv;
        spec.cwd = Some(cwd);
        for pair in self.confined_env_pairs() {
            spec.env.push(pair);
        }
        let program = spec
            .argv
            .first()
            .ok_or_else(|| anyhow!("pty 需要可执行文件"))?;
        let pair = native_pty_system().openpty(PtySize {
            cols: spec.cols.max(1),
            rows: spec.rows.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut builder = CommandBuilder::new(program);
        for arg in &spec.argv[1..] {
            builder.arg(arg);
        }
        if let Some(cwd) = &spec.cwd {
            builder.cwd(cwd.clone());
        }
        for (k, v) in &spec.env {
            builder.env(k, v);
        }
        let child = pair.slave.spawn_command(builder)?;
        drop(pair.slave);
        let writer = pair.master.take_writer()?;
        Ok(Box::new(LocalPty {
            pid: child.process_id().unwrap_or(0),
            child: Mutex::new(child),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            reader_taken: AtomicBool::new(false),
        }))
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let path = self.visible(path)?;
        std::fs::read(&path).with_context(|| format!("读取失败: {}", path.display()))
    }

    /// 与现有 `file_write` 语义一致：父目录不存在时自动创建。
    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        let path = self.visible(path)?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("无法创建目录: {}", parent.display()))?;
            }
        }
        std::fs::write(&path, data).with_context(|| format!("写入失败: {}", path.display()))
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let path = self.visible(path)?;
        let mut out = Vec::new();
        let entries = std::fs::read_dir(&path)
            .with_context(|| format!("无法列出目录: {}", path.display()))?;
        for entry in entries.flatten() {
            let file_type = entry.file_type().ok();
            out.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path(),
                is_dir: file_type.as_ref().is_some_and(|t| t.is_dir()),
                is_symlink: file_type.as_ref().is_some_and(|t| t.is_symlink()),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn metadata(&self, path: &Path) -> Result<FileMetadata> {
        let path = self.visible(path)?;
        let meta = std::fs::symlink_metadata(&path)
            .with_context(|| format!("读取元数据失败: {}", path.display()))?;
        let is_symlink = meta.file_type().is_symlink();
        // 符号链接按目标类型判定（与 `Path::is_file` / `is_dir` 一致）。
        let resolved = if is_symlink {
            std::fs::metadata(&path).ok()
        } else {
            None
        };
        let effective = resolved.as_ref().unwrap_or(&meta);
        Ok(FileMetadata {
            is_file: effective.is_file(),
            is_dir: effective.is_dir(),
            is_symlink,
            len: effective.len(),
        })
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        let path = self.visible(path)?;
        std::fs::canonicalize(&path).with_context(|| format!("无法解析路径: {}", path.display()))
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        let path = self.visible(path)?;
        std::fs::create_dir_all(&path).with_context(|| format!("无法创建目录: {}", path.display()))
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        let path = self.visible(path)?;
        std::fs::read_to_string(&path).with_context(|| format!("读取失败: {}", path.display()))
    }

    fn open_reader(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>> {
        let path = self.visible(path)?;
        let file =
            std::fs::File::open(&path).with_context(|| format!("读取失败: {}", path.display()))?;
        Ok(Box::new(file))
    }

    fn walk(&self, root: &Path, opts: WalkOptions) -> Result<Vec<WalkEntry>> {
        let root = self.visible(root)?;
        let mut builder = ignore::WalkBuilder::new(&root);
        builder.max_depth(Some(opts.max_depth.max(1)));
        builder.hidden(!opts.include_hidden);
        // grep 需要 .gitignore 语义；glob 只需要目录遍历，关掉以免结果与既有行为不同。
        builder.git_ignore(opts.respect_gitignore);
        builder.git_global(opts.respect_gitignore);
        builder.git_exclude(opts.respect_gitignore);
        builder.ignore(opts.respect_gitignore);
        builder.parents(opts.respect_gitignore);
        let mut out = Vec::new();
        for entry in builder.build().filter_map(|e| e.ok()) {
            if entry.depth() == 0 {
                continue;
            }
            let path = entry.path();
            // `.git` 元数据目录：grep / glob 跳过，`file_list` 不跳（保持"列目录所见即所得"）。
            if opts.exclude_git_metadata
                && path.to_string_lossy().replace('\\', "/").contains("/.git/")
            {
                continue;
            }
            let file_type = entry.file_type();
            out.push(WalkEntry {
                path: path.to_path_buf(),
                is_dir: file_type.is_some_and(|t| t.is_dir()),
                is_symlink: file_type.is_some_and(|t| t.is_symlink()),
            });
        }
        Ok(out)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        let path = self.visible(path)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(anyhow!("删除失败 {}: {e}", path.display())),
        }
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        let path = self.visible(path)?;
        match std::fs::remove_dir_all(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(anyhow!("删除目录失败 {}: {e}", path.display())),
        }
    }

    fn set_executable(&self, path: &Path) -> Result<()> {
        let path = self.visible(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .with_context(|| format!("设置权限失败: {}", path.display()))?;
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }
        Ok(())
    }
}

/// 把用户会话的命令包进只含 home 与 data 的视图。没有可用沙箱时拒绝执行。
fn confine_argv(home: &Path, data: &Path, cwd: &Path, argv: &[String]) -> Result<Vec<String>> {
    if argv.is_empty() {
        bail_empty_exec();
    }
    if cfg!(target_os = "linux") && command_on_path("bwrap") {
        let mut wrapped = vec!["bwrap".to_string(), "--die-with-parent".to_string()];
        for spec in ["/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc", "/opt"] {
            let path = Path::new(spec);
            if path.is_dir() {
                wrapped.push("--ro-bind".into());
                wrapped.push(spec.into());
                wrapped.push(spec.into());
            }
        }
        wrapped.extend([
            "--dev".into(),
            "/dev".into(),
            "--proc".into(),
            "/proc".into(),
            "--tmpfs".into(),
            "/tmp".into(),
            "--bind".into(),
            home.display().to_string(),
            home.display().to_string(),
            "--bind".into(),
            data.display().to_string(),
            data.display().to_string(),
            "--chdir".into(),
            cwd.display().to_string(),
            "--".into(),
        ]);
        wrapped.extend(argv.iter().cloned());
        log::info!("local env: confined exec via bwrap cwd={}", cwd.display());
        return Ok(wrapped);
    }
    if cfg!(target_os = "macos") && command_on_path("sandbox-exec") {
        let profile = macos_profile(home, data);
        let mut wrapped = vec!["sandbox-exec".to_string(), "-p".into(), profile];
        wrapped.extend(argv.iter().cloned());
        log::info!(
            "local env: confined exec via sandbox-exec cwd={}",
            cwd.display()
        );
        return Ok(wrapped);
    }
    anyhow::bail!("当前平台没有可用的目录沙箱，已拒绝在用户会话里执行命令")
}

fn existing_canonical(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn bail_empty_exec() -> Result<Vec<String>> {
    anyhow::bail!("exec 需要非空 argv")
}

fn command_on_path(name: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    let sep = if cfg!(windows) { ';' } else { ':' };
    path.split(sep).any(|dir| {
        if dir.is_empty() {
            return false;
        }
        Path::new(dir).join(name).is_file()
    })
}

fn macos_profile(home: &Path, data: &Path) -> String {
    let home = home.display();
    let data = data.display();
    // `(literal "/")` 只放行根目录这一项，用来解析 `/var`、`/tmp`、`/etc` 这些符号链接。
    // `/var` 只给元数据：`/usr/bin/git` 要能发现 `/var/select/developer_dir` 不存在，
    // 再读到 `/var/db/xcode_select_link`。不放行 `/var/folders` 的内容。
    // `/dev/null` 需要读写，git 会打开它。
    format!(
        r#"(version 1)
(deny default)
(allow process*)
(allow signal)
(allow sysctl-read)
(allow file-read* (literal "/"))
(allow file-read-metadata
    (subpath "/var")
    (subpath "/private/var")
    (subpath "/etc")
    (subpath "/tmp"))
(allow file-read*
    (subpath "/usr")
    (subpath "/bin")
    (subpath "/sbin")
    (subpath "/System")
    (subpath "/Library")
    (subpath "/opt")
    (subpath "/dev")
    (subpath "/private/etc")
    (subpath "/etc")
    (subpath "/.vol")
    (subpath "/private/var/select")
    (subpath "/var/select")
    (literal "/var/db/xcode_select_link")
    (literal "/private/var/db/xcode_select_link"))
(allow file-read* file-write*
    (subpath "{home}")
    (subpath "{data}")
    (subpath "/private/tmp")
    (subpath "/tmp")
    (literal "/dev/null"))
(allow file-ioctl)
(allow network*)
"#
    )
}

/// 读到上限就停，但继续把剩余字节读掉丢弃，避免子进程阻塞在写管道上。
fn copy_limited<R: Read>(mut reader: R, cap: Option<usize>) -> (Vec<u8>, bool) {
    let Some(cap) = cap else {
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        return (buf, false);
    };
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let mut truncated = false;
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() < cap {
                    let take = n.min(cap - buf.len());
                    buf.extend_from_slice(&chunk[..take]);
                    if take < n {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Err(_) => break,
        }
    }
    (buf, truncated)
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Option<Duration>,
) -> Result<(i32, bool)> {
    let Some(limit) = timeout else {
        let status = child.wait().context("等待命令结束失败")?;
        return Ok((status.code().unwrap_or(-1), false));
    };
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().context("等待命令结束失败")? {
            return Ok((status.code().unwrap_or(-1), false));
        }
        if started.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            return Ok((-1, true));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_for(dir: &Path) -> LocalEnv {
        LocalEnv::new(dir)
    }

    /// 并行跑整套测试时 fork/线程创建偶发失败（EAGAIN / fd 压力），会把环境噪声
    /// 当成产品回归。exec 类用例统一走这个小重试；断言逻辑本身不放宽。
    fn retry_exec(
        env: &LocalEnv,
        argv: &[String],
        opts: ExecOptions,
        check: impl Fn(&ExecOutput) -> bool,
    ) -> ExecOutput {
        let mut last: Option<ExecOutput> = None;
        for _ in 0..4 {
            if let Ok(out) = env.exec(argv, opts.clone()) {
                if check(&out) {
                    return out;
                }
                last = Some(out);
            }
        }
        panic!("exec 未达到预期（重试 4 次）: {last:?}");
    }

    #[test]
    fn reports_local_kind_and_identity_map() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_for(dir.path());
        assert_eq!(env.kind(), EnvKind::Local);
        assert_eq!(env.workspace_root(), dir.path());
        assert!(env.path_map().is_host_passthrough());
        assert_eq!(env.capabilities().read_scope, super::super::ReadScope::Host);
    }

    #[test]
    fn write_then_read_roundtrips_and_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_for(dir.path());
        let target = dir.path().join("nested/deep/note.txt");
        env.write_file(&target, b"hello").unwrap();
        assert_eq!(env.read_file(&target).unwrap(), b"hello");
    }

    #[test]
    fn read_missing_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_for(dir.path());
        assert!(env.read_file(&dir.path().join("absent.txt")).is_err());
    }

    #[test]
    fn list_dir_is_sorted_and_marks_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_for(dir.path());
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        env.write_file(&dir.path().join("b.txt"), b"b").unwrap();
        let names: Vec<String> = env
            .list_dir(dir.path())
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["b.txt".to_string(), "sub".to_string()]);
        let sub = env
            .list_dir(dir.path())
            .unwrap()
            .into_iter()
            .find(|e| e.name == "sub")
            .unwrap();
        assert!(sub.is_dir && !sub.is_symlink);
    }

    #[cfg(unix)]
    #[test]
    fn exec_captures_stdout_and_exit_code() {
        // 其他用例会临时改写 PATH（dotenv / terminal_elevated），
        // 不持锁就可能在 `Command::new("sh")` 上偶发失败。
        let _path_lock = crate::test_support::path_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let env = env_for(dir.path());
        let out = env
            .exec(
                &["sh".into(), "-c".into(), "echo hi; exit 3".into()],
                ExecOptions::default(),
            )
            .unwrap();
        assert_eq!(out.status, 3);
        assert_eq!(out.stdout_str().trim(), "hi");
        assert!(!out.success());
    }

    #[cfg(unix)]
    #[test]
    fn exec_honours_cwd_and_env_and_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_for(dir.path());
        let cwd_display = dir.path().canonicalize().unwrap().display().to_string();
        let out = retry_exec(
            &env,
            &["sh".into(), "-c".into(), "pwd; echo $POINTER_T; cat".into()],
            ExecOptions::new()
                .with_cwd(dir.path())
                .with_env("POINTER_T", "42")
                .with_stdin("from-stdin"),
            |out| {
                let s = out.stdout_str();
                s.contains(&cwd_display) && s.contains("42") && s.contains("from-stdin")
            },
        );
        let stdout = out.stdout_str();
        assert!(stdout.contains(&cwd_display), "{stdout}");
        assert!(stdout.contains("42"), "{stdout}");
        assert!(stdout.contains("from-stdin"), "{stdout}");
    }

    #[cfg(unix)]
    #[test]
    fn exec_timeout_kills_command() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_for(dir.path());
        let out = retry_exec(
            &env,
            &["sh".into(), "-c".into(), "sleep 5".into()],
            ExecOptions::new().with_timeout(Duration::from_millis(120)),
            |out| out.timed_out,
        );
        assert!(out.timed_out, "预期超时终止: {out:?}");
        assert_eq!(out.status, -1);
        assert!(!out.success());
    }

    #[cfg(unix)]
    #[test]
    fn exec_caps_output_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_for(dir.path());
        let out = retry_exec(
            &env,
            &["sh".into(), "-c".into(), "printf 'aaaaaaaaaa'".into()],
            ExecOptions::new().with_max_output_bytes(4),
            |out| out.stdout.len() == 4 && out.stdout_truncated,
        );
        assert_eq!(out.stdout.len(), 4);
        assert!(out.stdout_truncated);
        assert!(!out.stderr_truncated);
    }

    #[test]
    fn metadata_reports_kind_and_len() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_for(dir.path());
        env.write_file(&dir.path().join("f.txt"), b"abc").unwrap();
        let meta = env.metadata(&dir.path().join("f.txt")).unwrap();
        assert!(meta.is_file && !meta.is_dir && !meta.is_symlink);
        assert_eq!(meta.len, 3);
        assert!(env.metadata(dir.path()).unwrap().is_dir);
        assert!(env.metadata(&dir.path().join("missing")).is_err());
        assert!(env.exists(dir.path()));
    }

    #[test]
    fn walk_batches_entries_and_skips_hidden_and_git() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_for(dir.path());
        env.write_file(&dir.path().join("a.txt"), b"a").unwrap();
        env.write_file(&dir.path().join("sub/b.txt"), b"b").unwrap();
        env.write_file(&dir.path().join(".hidden.txt"), b"h")
            .unwrap();
        env.write_file(&dir.path().join(".git/config"), b"g")
            .unwrap();

        let entries = env.walk(dir.path(), WalkOptions::new(4)).unwrap();
        let rel: Vec<String> = entries
            .iter()
            .map(|e| {
                e.path
                    .strip_prefix(dir.path())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(rel.contains(&"a.txt".to_string()), "{rel:?}");
        assert!(rel.contains(&"sub/b.txt".to_string()), "{rel:?}");
        assert!(!rel.iter().any(|p| p.contains("hidden")), "{rel:?}");
        assert!(!rel.iter().any(|p| p.starts_with(".git")), "{rel:?}");
        assert!(entries.iter().any(|e| e.is_dir && e.path.ends_with("sub")));

        let with_hidden = env
            .walk(dir.path(), WalkOptions::new(4).with_hidden(true))
            .unwrap();
        assert!(with_hidden.iter().any(|e| e.path.ends_with(".hidden.txt")));
    }

    #[test]
    fn read_to_string_and_reader_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_for(dir.path());
        let target = dir.path().join("t.txt");
        env.write_file(&target, b"line1\nline2\n").unwrap();
        assert_eq!(env.read_to_string(&target).unwrap(), "line1\nline2\n");
        let mut reader = env.open_reader(&target).unwrap();
        let mut buf = String::new();
        reader.read_to_string(&mut buf).unwrap();
        assert_eq!(buf, "line1\nline2\n");
    }

    #[test]
    fn exec_rejects_empty_argv() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_for(dir.path());
        assert!(env.exec(&[], ExecOptions::default()).is_err());
    }

    #[test]
    fn spawn_echo_child_roundtrips_lines() {
        use std::io::BufRead;
        let dir = tempfile::tempdir().unwrap();
        let env = LocalEnv::new(dir.path());
        let argv: Vec<String> = ["/bin/sh", "-c", "cat"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut child = env
            .spawn(&argv, SpawnOptions::default())
            .expect("spawn cat");
        child.write_all(b"hello\n").expect("write");
        let mut reader = child.reader().expect("reader");
        let mut line = String::new();
        reader.read_line(&mut line).expect("read line");
        assert_eq!(line, "hello\n");
        child.kill().expect("kill");
        assert!(child.try_wait().expect("wait").is_some(), "kill 后应已退出");
    }

    #[test]
    fn spawn_respects_env_and_cwd() {
        use std::io::BufRead;
        let dir = tempfile::tempdir().unwrap();
        let env = LocalEnv::new(dir.path());
        let argv: Vec<String> = ["/bin/sh", "-c", "echo $PT_SPAWN_PROBE; pwd"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let opts = SpawnOptions {
            cwd: Some(dir.path().to_path_buf()),
            env: vec![("PT_SPAWN_PROBE".into(), "probe-ok".into())],
        };
        let mut child = env.spawn(&argv, opts).expect("spawn sh");
        let mut reader = child.reader().expect("reader");
        let mut first = String::new();
        reader.read_line(&mut first).expect("read env line");
        assert_eq!(first.trim(), "probe-ok");
        let mut second = String::new();
        reader.read_line(&mut second).expect("read cwd line");
        assert!(second.trim().ends_with(
            &dir.path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        ));
        child.kill().expect("kill");
    }

    #[test]
    fn read_file_capped_rejects_oversize() {
        let dir = tempfile::tempdir().unwrap();
        let env = LocalEnv::new(dir.path());
        let path = dir.path().join("big.bin");
        std::fs::write(&path, vec![0u8; 1024]).unwrap();
        let err = env
            .read_file_capped(&path, 512)
            .expect_err("超过上限应报错");
        assert!(err.to_string().contains("超过单次读取上限"), "{err}");
        assert_eq!(env.read_file_capped(&path, 1024).unwrap().len(), 1024);
    }

    #[test]
    fn pty_open_gives_a_real_tty_and_honors_resize() {
        let dir = tempfile::tempdir().unwrap();
        let env = LocalEnv::new(dir.path());
        let mut spec = crate::env::PtySpec::new(vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "tty; stty size; sleep 0.4; stty size".to_string(),
        ]);
        spec.cwd = Some(dir.path().to_path_buf());
        spec.cols = 100;
        spec.rows = 40;
        let pty = env.pty_open(spec).expect("open pty");
        let mut reader = pty.take_reader().expect("reader");
        // 中途改尺寸，第二次 `stty size` 应反映新值
        std::thread::sleep(std::time::Duration::from_millis(120));
        pty.resize(120, 50).expect("resize");
        let mut out = String::new();
        use std::io::Read;
        reader.read_to_string(&mut out).expect("read pty");
        assert!(out.contains("/dev/"), "应分配到真实 tty：{out}");
        assert!(out.contains("40 100"), "初始尺寸应为 40 100：{out}");
        assert!(out.contains("50 120"), "resize 后应为 50 120：{out}");
        let _ = pty.kill();
    }

    #[test]
    fn pty_reader_can_only_be_taken_once() {
        let dir = tempfile::tempdir().unwrap();
        let env = LocalEnv::new(dir.path());
        let spec = crate::env::PtySpec::new(vec!["/bin/sh".to_string()]);
        let pty = env.pty_open(spec).expect("open pty");
        assert!(pty.take_reader().is_ok());
        assert!(pty.take_reader().is_err(), "第二次取 reader 必须失败");
        let _ = pty.kill();
    }
}
