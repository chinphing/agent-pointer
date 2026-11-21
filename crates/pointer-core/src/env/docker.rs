//! DockerEnv：`desktop` / `headless` 档位的**本地 Docker 后端**。
//!
//! ## 为什么需要它
//!
//! e2b-local 的卷只能是 `~/.e2b-local/volumes/` 下的真实目录，挂不了用户的项目目录
//! （实测：符号链接被拒 `is not a directory`；`metadata["host-mount"]` 被静默忽略）。
//! 于是开发环境里 `/workspace` 永远是空卷，`git status` / diff / 文件树都验不了。
//!
//! 这里改成和 Hermes / OpenClaw 一样的做法：`docker run -v <项目目录>:/workspace`，
//! 直接 bind 宿主目录。开发和生产的**语义一致**（隔离、无 GUI），只有实现不同。
//!
//! ## 关键设计：文件 IO 不进容器
//!
//! `/workspace` 是项目目录的 bind mount，宿主和容器看到同一份文件。所以
//!
//! - 文件读写：把 guest 路径按 [`PathMap`] 翻回宿主路径，直接交给 [`LocalEnv`]
//! - 命令执行：`docker exec`
//! - 交互终端：宿主 `portable_pty` 包一层 `docker exec -it`（tty 由 `-t` 分配）
//!
//! 少一层 `docker cp`，也少一份"容器内外不一致"的坑。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

use super::{
    Capabilities, ChildHandle, DirEntry, EnvKind, Environment, ExecOptions, ExecOutput,
    FileMetadata, PathMap, PtyHandle, PtySpec, SpawnOptions, WalkEntry, WalkOptions,
};

/// 建容器所需的参数。
#[derive(Debug, Clone)]
pub struct DockerSpec {
    /// 容器名（每会话一个，销毁幂等）。
    pub container: String,
    /// 镜像（= 沙箱模板）。
    pub image: String,
    /// 挂载计划：宿主目录 → guest 路径。
    pub mounts: Vec<super::pathmap::Mount>,
    /// 容器内默认 cwd（`/data/workspaces`）；会话可覆盖为具体项目。
    pub workdir: PathBuf,
    /// 档位：决定能力集（Headless 无桌面 / Container 有 Xvfb 桌面）。
    pub tier: super::provider::Tier,
    /// CPU 上限（`--cpus`）。`None` 用容器默认，即不限。
    pub cpus: Option<f32>,
    /// 内存上限 MB（`--memory`）。`None` 用容器默认，即不限。
    pub memory_mb: Option<u32>,
    /// 磁盘上限 MB（`--storage-opt size=`）。仅 Linux + overlay2/xfs 生效；
    /// 其他平台会记一次告警后忽略（见 `disk_limit_supported()`）。
    pub disk_mb: Option<u32>,
    /// 桌面 VNC 密码。`None` = 建容器时随机生成（容器档位）。
    pub vnc_password: Option<String>,
}

impl Default for DockerSpec {
    fn default() -> Self {
        Self {
            container: String::new(),
            image: String::new(),
            mounts: Vec::new(),
            workdir: PathBuf::from(crate::env::mounts::GUEST_WORKSPACES_ROOT),
            tier: super::provider::Tier::Headless,
            cpus: None,
            memory_mb: None,
            disk_mb: None,
            vnc_password: None,
        }
    }
}

impl DockerSpec {
    /// 规格指纹：镜像 + 档位 + 挂载 + 工作目录。
    ///
    /// 容器名按会话固定，但规格会变（换镜像、换档位、换项目目录）。
    /// 复用时必须比对指纹，否则会拿旧镜像的容器继续用——表现为
    /// "改了配置没生效"。这是 OpenClaw 的 `configHash` 思路。
    pub fn fingerprint(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        // 契约版本：改了入口脚本、注入的环境变量、挂载语义就 +1。
        // 不这么做的话，改完配置旧容器会被继续复用，表现为"改了没生效"。
        SPEC_VERSION.hash(&mut hasher);
        self.image.hash(&mut hasher);
        format!("{:?}", self.tier).hash(&mut hasher);
        self.workdir.hash(&mut hasher);
        // 配额变了也要重建：旧容器没有 --memory，复用等于配额不生效。
        self.cpus.map(|v| (v * 1000.0) as u32).hash(&mut hasher);
        self.memory_mb.hash(&mut hasher);
        self.disk_mb.hash(&mut hasher);
        for mount in &self.mounts {
            mount.host.hash(&mut hasher);
            mount.guest.hash(&mut hasher);
            mount.read_only.hash(&mut hasher);
        }
        format!("{:016x}", hasher.finish())
    }

    /// 8 位随机密码（去掉容易看错的 0/O/1/l）。
    pub fn random_vnc_password() -> String {
        use rand::Rng;
        const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
        let mut rng = rand::thread_rng();
        (0..8)
            .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
            .collect()
    }

    pub fn container_name_for(conversation_id: &str) -> String {
        let safe: String = conversation_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        format!("pointer-sbx-{safe}")
    }

    /// Authenticated users share one container; anonymous stays conversation-scoped.
    pub fn container_name_for_identity(session_user_id: &str, conversation_id: &str) -> String {
        if session_user_id.trim().is_empty() {
            return Self::container_name_for(conversation_id);
        }
        let key = crate::user_key::user_key(session_user_id);
        format!("pointer-sbx-u-{key}")
    }

}

/// 容器规格版本。**改动容器契约就 +1**：入口脚本、注入的环境变量、挂载语义。
///
/// v1 → v2：容器档位增加 VNC 端口映射 + `POINTER_VNC_PASSWORD` 注入。
/// v2 → v3：home 挂载点 `/home/u` → `/home/pointer`；非 root 运行。
/// v3 → v4：增加 6080 端口映射（websockify / noVNC，面板内接管电脑）。
/// v4 → v5：桌面从"纯色空屏"改成底部 dock（plank）+ 常用程序快捷方式。
///   指纹只 hash 镜像**名字**（`:latest` 不变），所以必须靠版本号把老容器挤掉，
///   否则升级镜像后用户看到的还是旧桌面。
/// v5 → v6：浏览器包装脚本剥掉调用方塞的 `--no-sandbox`（非 root 下不需要，
///   带上只会弹"不受支持的参数"警告并拆掉沙箱）。
/// v6 → v7：x11vnc 改成把光标画进画面（-nocursorshape），
///   宿主鼠标不在画布上时也能看见指针位置。
/// v7 → v8：光标模式 arrow → most，画的是真实形状（输入框变工字形、
///   链接变手形），不再永远一个箭头。
/// v8 → v9：镜像改成分层构建（base → headless → desktop），desktop 档位镜像名
///   `pointer-sandbox-desktop:latest`，内容变了但名字没变，必须靠版本号让旧容器重建。
const SPEC_VERSION: u32 = 9;

/// 构造 `docker run` 参数。
///
/// `include_disk_limit` 单独拿出来，是因为 `--storage-opt size=` 只在
/// Linux + overlay2/xfs(pquota) 上支持，macOS 会直接报错——调用方需要能重试。
fn run_args(
    spec: &DockerSpec,
    fingerprint: &str,
    vnc_password: Option<&str>,
    include_disk_limit: bool,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "run".into(),
        "-d".into(),
        "--name".into(),
        spec.container.clone(),
        "--label".into(),
        format!("pointer.spec={fingerprint}"),
    ];
    for mount in &spec.mounts {
        let mut bind = format!("{}:{}", mount.host.display(), mount.guest.display());
        if mount.read_only {
            bind.push_str(":ro");
        }
        args.push("-v".into());
        args.push(bind);
    }
    if spec.tier == super::provider::Tier::Desktop {
        // 桌面观察 / 接管通道，都只绑 127.0.0.1 的宿主随机端口：
        //   5900 → x11vnc（系统屏幕共享直连）
        //   6080 → websockify（面板内 noVNC 走 WebSocket）
        args.push("-p".into());
        args.push("127.0.0.1::5900".into());
        args.push("-p".into());
        args.push("127.0.0.1::6080".into());
        if let Some(password) = vnc_password.filter(|p| !p.is_empty()) {
            args.push("-e".into());
            args.push(format!("POINTER_VNC_PASSWORD={password}"));
        }
    }
    if let Some(cpus) = spec.cpus {
        args.push("--cpus".into());
        args.push(format!("{cpus}"));
    }
    if let Some(mb) = spec.memory_mb {
        args.push("--memory".into());
        args.push(format!("{mb}m"));
    }
    if include_disk_limit {
        if let Some(mb) = spec.disk_mb {
            args.push("--storage-opt".into());
            args.push(format!("size={mb}m"));
        }
    }
    args.push("--workdir".into());
    args.push(spec.workdir.display().to_string());
    args.push(spec.image.clone());
    args.push("sleep".into());
    args.push("infinity".into());
    args
}

/// 本地 Docker 后端。
pub struct DockerEnv {
    container: String,
    workdir: PathBuf,
    tier: super::provider::Tier,
    vnc_password: Option<String>,
    map: PathMap,
    /// 文件 IO 直接走宿主（bind 挂载 = 同一份文件）。
    host_fs: super::local::LocalEnv,
}

impl std::fmt::Debug for DockerEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockerEnv")
            .field("container", &self.container)
            .field("workdir", &self.workdir)
            .finish()
    }
}

/// 容器内的交互终端：写/读直通宿主 pty，**resize 额外改内层 tty**。
struct DockerPty {
    inner: Box<dyn PtyHandle>,
    container: String,
    /// 内层 pts 路径（启动时由包装脚本写入，如 `/tmp/.pointer-pty-<token>`）。
    pts_file: String,
}

impl PtyHandle for DockerPty {
    fn id(&self) -> u32 {
        self.inner.id()
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        self.inner.write(data)
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        // 宿主侧先改：让 docker 客户端自己的 tty 视图是对的。
        self.inner.resize(cols, rows)?;
        let script = format!(
            "stty -F {} rows {rows} cols {cols} 2>/dev/null || true",
            self.pts_file
        );
        let out = Command::new("docker")
            .args(["exec", &self.container, "/bin/sh", "-c", &script])
            .output()
            .context("docker exec stty（调整终端大小）")?;
        if !out.status.success() {
            log::debug!(
                "pty resize 未生效 container={} status={:?}",
                self.container,
                out.status.code()
            );
        }
        Ok(())
    }

    fn take_reader(&self) -> Result<Box<dyn std::io::Read + Send>> {
        self.inner.take_reader()
    }

    fn try_wait(&self) -> Result<Option<i32>> {
        self.inner.try_wait()
    }

    fn kill(&self) -> Result<()> {
        self.inner.kill()
    }
}

impl DockerEnv {
    /// 测试用：造一个不连真实容器的实例（只验证缓存 / 淘汰逻辑）。
    #[cfg(test)]
    pub(crate) fn fake_for_tests(container: &str) -> Self {
        Self {
            container: container.to_string(),
            workdir: PathBuf::from(crate::env::mounts::GUEST_WORKSPACES_ROOT),
            tier: super::provider::Tier::Desktop,
            vnc_password: None,
            map: PathMap::new(Vec::new()),
            host_fs: super::local::LocalEnv::new("/tmp"),
        }
    }

    /// 收养一个**已在运行**且指纹匹配的容器；不存在或不匹配返回 `None`。
    ///
    /// 与 `create` 的区别：绝不 `docker run`，也绝不 `rm -f`。给"应用重启后
    /// 缓存空了、但容器还在跑"的场景用（面板接管入口靠它恢复）。
    pub fn attach(spec: DockerSpec) -> Result<Option<Self>> {
        if spec.tier != super::provider::Tier::Desktop {
            return Ok(None);
        }
        let container = spec.container.clone();
        if !Self::container_running(&container) {
            return Ok(None);
        }
        let fingerprint = spec.fingerprint();
        if Self::container_fingerprint(&container).as_deref() != Some(fingerprint.as_str()) {
            // 规格变了（换了镜像/挂载/配额），旧容器不该被接管——交给 create 重建。
            return Ok(None);
        }
        let workspace_host = spec
            .mounts
            .iter()
            .find(|m| m.guest == Path::new(crate::env::mounts::GUEST_DATA))
            .map(|m| m.host.clone())
            .ok_or_else(|| anyhow!("docker: 挂载计划里没有 /data 对应的宿主目录"))?;
        let map = PathMap::new(spec.mounts.clone());
        Ok(Some(Self {
            vnc_password: Self::container_vnc_password(&container),
            container,
            workdir: spec.workdir,
            tier: spec.tier,
            map,
            host_fs: super::local::LocalEnv::new(&workspace_host),
        }))
    }

    /// 创建并启动容器（已存在则复用）。
    pub fn create(spec: DockerSpec) -> Result<Self> {
        let map = PathMap::new(spec.mounts.clone());
        let workspace_host = spec
            .mounts
            .iter()
            .find(|m| m.guest == Path::new(crate::env::mounts::GUEST_DATA))
            .map(|m| m.host.clone())
            .ok_or_else(|| anyhow!("docker: 挂载计划里没有 /data 对应的宿主目录"))?;

        let fingerprint = spec.fingerprint();
        let reusable = Self::container_running(&spec.container)
            && Self::container_fingerprint(&spec.container).as_deref() == Some(fingerprint.as_str());

        // 容器档位：每会话随机 VNC 密码（空字符串 = 显式要求无密码）。
        // 复用已有容器时密码已经在容器里定死了，必须读回来；这里再随机一个就会对不上。
        let vnc_password = if spec.tier == super::provider::Tier::Desktop {
            if reusable {
                Self::container_vnc_password(&spec.container)
            } else {
                spec.vnc_password
                    .clone()
                    .or_else(|| Some(DockerSpec::random_vnc_password()))
            }
        } else {
            None
        };

        if !reusable {
            // 残留容器（停止/异常）先清掉，避免名字冲突。
            let _ = Command::new("docker")
                .args(["rm", "-f", &spec.container])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            let mut args = run_args(&spec, &fingerprint, vnc_password.as_deref(), true);
            let mut out = Command::new("docker")
                .args(&args)
                .output()
                .context("启动 docker 失败（docker 是否可用？）")?;

            // `--storage-opt size=` 只有 Linux + overlay2/xfs(pquota) 支持。
            // macOS（Docker Desktop / OrbStack）会直接报错，这时退回不限磁盘，
            // 并且只提示一次——不能让"磁盘限制"把整个沙箱建不起来。
            if !out.status.success() && spec.disk_mb.is_some() {
                let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
                if stderr.contains("storage-opt") || stderr.contains("storage option") {
                    log::warn!(
                        "docker: 当前平台不支持 --storage-opt size（磁盘限制不生效），已按不限磁盘创建容器"
                    );
                    let _ = Command::new("docker")
                        .args(["rm", "-f", &spec.container])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                    args = run_args(&spec, &fingerprint, vnc_password.as_deref(), false);
                    out = Command::new("docker")
                        .args(&args)
                        .output()
                        .context("启动 docker 失败（docker 是否可用？）")?;
                }
            }

            if !out.status.success() {
                bail!(
                    "docker run 失败: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
        }

        let env = Self {
            container: spec.container,
            workdir: spec.workdir,
            tier: spec.tier,
            vnc_password,
            map,
            host_fs: super::local::LocalEnv::new(&workspace_host),
        };
        // 容器档位：Xvfb 是异步起的，立刻 exec 会拿到 "cannot open display"。
        // 这里等桌面真正可用再返回，避免调用方第一次执行就失败。
        if env.tier == super::provider::Tier::Desktop {
            env.wait_until_desktop_ready()?;
        }
        Ok(env)
    }

    /// 查容器某个端口映射到宿主的哪个端口（只绑 127.0.0.1）。
    fn published_port(&self, container_port: &str) -> Option<u16> {
        if self.tier != super::provider::Tier::Desktop {
            return None;
        }
        let out = Command::new("docker")
            .args(["port", &self.container, container_port])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .next()?
            .trim()
            .rsplit(':')
            .next()?
            .trim()
            .parse()
            .ok()
    }

    /// 桌面观察地址（容器档位才有）。返回 `vnc://127.0.0.1:<port>`。
    pub fn vnc_endpoint(&self) -> Option<String> {
        let port = self.published_port("5900/tcp")?;
        Some(format!("vnc://127.0.0.1:{port}"))
    }

    /// 面板接管地址：websockify 的 WebSocket 端点。容器档位才有。
    ///
    /// 老镜像没装 websockify 时 6080 没在监听，`docker port` 仍会返回映射（端口是
    /// 起容器时声明的），所以这里查得到不代表连得上——真正连不上由前端报错兜底。
    pub fn ws_endpoint(&self) -> Option<String> {
        let port = self.published_port("6080/tcp")?;
        Some(format!("ws://127.0.0.1:{port}/websockify"))
    }

    /// 等 Xvfb 起来（最多 10s）。起不来就报错，别让调用方拿到半成品环境。
    fn wait_until_desktop_ready(&self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let out = Command::new("docker")
                .args(["exec", &self.container, "xdpyinfo"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if matches!(out, Ok(s) if s.success()) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "docker: 容器 {} 的 Xvfb 10 秒内没起来（DISPLAY=:99 不可用）",
                    self.container
                );
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    pub fn container_name(&self) -> &str {
        &self.container
    }

    /// 销毁容器（幂等）。文件在宿主上，不受影响。
    pub fn destroy(&self) -> Result<()> {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.container])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        Ok(())
    }

    /// 清理**已退出**的孤儿容器（进程崩溃/重启后残留）。
    ///
    /// 只动 `status=exited` 的：还在跑的可能是另一个会话正在用的，不能碰。
    pub fn cleanup_orphans() {
        let out = Command::new("docker")
            .args([
                "ps",
                "-a",
                "--filter",
                "name=pointer-sbx-",
                "--filter",
                "status=exited",
                "--format",
                "{{.Names}}",
            ])
            .output();
        let Ok(out) = out else { return };
        if !out.status.success() {
            return;
        }
        for name in String::from_utf8_lossy(&out.stdout).lines() {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            log::info!("docker: 清理残留沙箱容器 {name}");
            let _ = Command::new("docker")
                .args(["rm", "-f", name])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    fn container_fingerprint(name: &str) -> Option<String> {
        let out = Command::new("docker")
            .args([
                "inspect",
                "-f",
                "{{index .Config.Labels \"pointer.spec\"}}",
                name,
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if value.is_empty() || value == "<no value>" {
            None
        } else {
            Some(value)
        }
    }

    /// 容器是否存在（不论运行状态）。规格变了要重建时用来区分"没有"和"过期"。
    pub fn exists(container: &str) -> bool {
        matches!(
            Command::new("docker")
                .args(["inspect", "-f", "{{.Id}}", container])
                .output(),
            Ok(o) if o.status.success()
        )
    }

    fn container_running(name: &str) -> bool {
        let out = Command::new("docker")
            .args(["inspect", "-f", "{{.State.Running}}", name])
            .output();
        matches!(out, Ok(o) if o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
    }

    /// 读回容器里实际生效的 VNC 密码。
    ///
    /// 密码在 `docker run` 时通过 `POINTER_VNC_PASSWORD` 注入，容器一旦建好就定死了。
    /// 复用容器时必须读回来——否则这里新生成一个，容器里还是旧的，接管时直接
    /// `password check failed`（应用重启后必然踩到）。
    fn container_vnc_password(name: &str) -> Option<String> {
        let out = Command::new("docker")
            .args([
                "inspect",
                "-f",
                "{{range .Config.Env}}{{println .}}{{end}}",
                name,
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .find_map(|line| line.trim().strip_prefix("POINTER_VNC_PASSWORD="))
            .map(str::to_string)
    }

    /// 容器档位默认带 `DISPLAY=:99`（镜像入口起的 Xvfb），调用方显式给了就不覆盖。
    fn effective_env(&self, env: &std::collections::BTreeMap<String, String>) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if self.tier == super::provider::Tier::Desktop
            && !env.contains_key("DISPLAY")
            && !out.iter().any(|(k, _)| k == "DISPLAY")
        {
            out.push(("DISPLAY".to_string(), ":99".to_string()));
        }
        out
    }

    /// 把 guest 路径翻成宿主路径（文件 IO 用）。
    fn host_path(&self, path: &Path) -> Result<PathBuf> {
        self.map.to_host(path)
    }

    fn exec_docker(&self, argv: &[String], opts: &ExecOptions) -> Result<ExecOutput> {
        let mut cmd = Command::new("docker");
        cmd.arg("exec").arg("-i");
        if let Some(cwd) = &opts.cwd {
            cmd.arg("-w").arg(cwd);
        }
        for (k, v) in self.effective_env(&opts.env) {
            cmd.arg("-e").arg(format!("{k}={v}"));
        }
        cmd.arg(&self.container);
        cmd.args(argv);
        cmd.stdin(if opts.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn().context("docker exec 启动失败")?;
        if let Some(input) = &opts.stdin {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(input)?;
            }
        }
        let out = child.wait_with_output()?;
        let limit = opts.max_output_bytes.unwrap_or(usize::MAX);
        let mut stdout = out.stdout;
        let mut stderr = out.stderr;
        let stdout_truncated = stdout.len() > limit;
        let stderr_truncated = stderr.len() > limit;
        stdout.truncate(limit);
        stderr.truncate(limit);
        Ok(ExecOutput {
            status: out.status.code().unwrap_or(-1),
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            timed_out: false,
        })
    }
}

impl Environment for DockerEnv {
    fn kind(&self) -> EnvKind {
        match self.tier {
            super::provider::Tier::Local => EnvKind::Local,
            super::provider::Tier::Headless => EnvKind::Headless,
            super::provider::Tier::Desktop => EnvKind::Desktop,
        }
    }
    fn workspace_root(&self) -> &Path {
        &self.workdir
    }
    fn skills_root(&self) -> &Path {
        Path::new("/skills")
    }
    fn home_root(&self) -> &Path {
        Path::new(crate::env::mounts::GUEST_HOME)
    }
    fn data_root(&self) -> &Path {
        Path::new(crate::env::mounts::GUEST_DATA)
    }
    fn capabilities(&self) -> Capabilities {
        match self.tier {
            super::provider::Tier::Desktop => Capabilities::desktop(),
            _ => Capabilities::headless(),
        }
    }
    fn path_map(&self) -> &PathMap {
        &self.map
    }

    fn desktop_endpoint(&self) -> Option<super::DesktopEndpoint> {
        // 只有容器档位有桌面。vnc 端口拿不到说明容器不在/没起，直接当作没有桌面。
        let vnc_url = self.vnc_endpoint()?;
        Some(super::DesktopEndpoint {
            vnc_url,
            ws_url: self.ws_endpoint(),
            password: self.vnc_password.clone(),
        })
    }

    fn exec(&self, argv: &[String], opts: ExecOptions) -> Result<ExecOutput> {
        let started = Instant::now();
        let out = self.exec_docker(argv, &opts)?;
        if let Some(timeout) = opts.timeout {
            if started.elapsed() > timeout {
                return Ok(ExecOutput {
                    timed_out: true,
                    status: -1,
                    ..out
                });
            }
        }
        Ok(out)
    }

    fn spawn(&self, argv: &[String], opts: SpawnOptions) -> Result<Box<dyn ChildHandle>> {
        let mut cmd = Command::new("docker");
        cmd.arg("exec").arg("-i");
        if let Some(cwd) = &opts.cwd {
            cmd.arg("-w").arg(cwd);
        }
        for (k, v) in &opts.env {
            cmd.arg("-e").arg(format!("{k}={v}"));
        }
        cmd.arg(&self.container).args(argv);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().context("docker exec 启动失败")?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
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
        Ok(Box::new(DockerChild {
            child,
            stdin,
            stdout,
        }))
    }

    fn pty_open(&self, mut spec: PtySpec) -> Result<Box<dyn PtyHandle>> {
        // tty 由 `docker exec -t` 分配；宿主侧再包一层 pty（和 OpenClaw 一致）。
        //
        // resize 要单独处理：改宿主 pty 尺寸只会给 `docker` 客户端发 SIGWINCH，
        // 内层 tty 不变（实测 resize 后容器里 `stty size` 仍是旧值，vim/htop
        // 会按错的行列渲染）。所以启动时把内层 pts 路径记下来，resize 时按路径
        // 直接 `stty -F <pts>` 改——不依赖 SIGWINCH 传递。
        let token = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let pts_file = format!("/tmp/.pointer-pty-{token}");
        let mut argv = vec![
            "docker".to_string(),
            "exec".to_string(),
            "-it".to_string(),
        ];
        if let Some(cwd) = &spec.cwd {
            argv.push("-w".to_string());
            argv.push(cwd.display().to_string());
        }
        for (k, v) in &spec.env {
            argv.push("-e".to_string());
            argv.push(format!("{k}={v}"));
        }
        argv.push(self.container.clone());
        // 用 sh 包一层：先记下自己的 pts，再 exec 真正的 shell（argv 原样传递）。
        let mut wrapped = vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            format!("tty > {pts_file} 2>/dev/null; exec \"$@\""),
            "pointer-pty".to_string(),
        ];
        wrapped.extend(spec.argv.clone());
        argv.extend(wrapped);
        spec.argv = argv;
        spec.cwd = None;
        spec.env.clear();
        let inner = super::local::LocalEnv::new(self.workdir.clone()).pty_open(spec)?;
        Ok(Box::new(DockerPty {
            inner,
            container: self.container.clone(),
            pts_file,
        }))
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let host = self.host_path(path)?;
        self.host_fs.read_file(&host)
    }

    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        let host = self.host_path(path)?;
        self.host_fs.write_file(&host, data)
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let host = self.host_path(path)?;
        let mut entries = self.host_fs.list_dir(&host)?;
        // 名字对外仍是 guest 视角（host_fs 返回的是宿主路径，这里翻回去）。
        for entry in &mut entries {
            if let Ok(guest) = self.map.to_agent(&entry.path) {
                entry.path = guest;
            }
        }
        Ok(entries)
    }

    fn metadata(&self, path: &Path) -> Result<FileMetadata> {
        let host = self.host_path(path)?;
        self.host_fs.metadata(&host)
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        let host = self.host_path(path)?;
        let canonical = self.host_fs.canonicalize(&host)?;
        self.map.to_agent(&canonical)
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        let host = self.host_path(path)?;
        self.host_fs.create_dir_all(&host)
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        let host = self.host_path(path)?;
        self.host_fs.read_to_string(&host)
    }

    fn open_reader(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>> {
        let host = self.host_path(path)?;
        self.host_fs.open_reader(&host)
    }

    fn walk(&self, root: &Path, opts: WalkOptions) -> Result<Vec<WalkEntry>> {
        let host = self.host_path(root)?;
        let mut entries = self.host_fs.walk(&host, opts)?;
        for entry in &mut entries {
            if let Ok(guest) = self.map.to_agent(&entry.path) {
                entry.path = guest;
            }
        }
        Ok(entries)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        let host = self.host_path(path)?;
        self.host_fs.remove_file(&host)
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        let host = self.host_path(path)?;
        self.host_fs.remove_dir_all(&host)
    }

    fn set_executable(&self, path: &Path) -> Result<()> {
        let host = self.host_path(path)?;
        self.host_fs.set_executable(&host)
    }


}

/// `docker exec` 起的常驻进程。
struct DockerChild {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    stdout: Option<std::process::ChildStdout>,
}

impl ChildHandle for DockerChild {
    fn id(&self) -> u32 {
        self.child.id()
    }
    fn write_all(&mut self, data: &[u8]) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("stdin 已关闭"))?;
        stdin.write_all(data)?;
        stdin.flush()?;
        Ok(())
    }
    fn close_stdin(&mut self) -> Result<()> {
        self.stdin.take();
        Ok(())
    }
    fn reader(&mut self) -> Result<Box<dyn std::io::BufRead + Send>> {
        let stdout = self
            .stdout
            .take()
            .ok_or_else(|| anyhow!("stdout 已被取走"))?;
        Ok(Box::new(std::io::BufReader::new(stdout)))
    }
    fn try_wait(&mut self) -> Result<Option<i32>> {
        Ok(self.child.try_wait()?.map(|s| s.code().unwrap_or(-1)))
    }
    fn wait(&mut self) -> Result<i32> {
        Ok(self.child.wait()?.code().unwrap_or(-1))
    }
    fn kill(&mut self) -> Result<()> {
        self.child.kill().map_err(|e| anyhow!("kill: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_name_is_docker_safe() {
        let name = DockerSpec::container_name_for("9df6e0c2-f409-43b2-8e52-087c7479607d");
        assert_eq!(name, "pointer-sbx-9df6e0c2-f409-43b2-8e52-087c7479607d");
        let weird = DockerSpec::container_name_for("a/b:c d");
        assert!(!weird.contains('/') && !weird.contains(':') && !weird.contains(' '));
    }

    fn spec_with_tier(tier: super::super::provider::Tier) -> DockerSpec {
        DockerSpec {
            container: "pointer-test".into(),
            image: "pointer-sandbox-headless:latest".into(),
            mounts: Vec::new(),
            workdir: crate::env::mounts::GUEST_WORKSPACES_ROOT.into(),
            tier,
            cpus: None,
            memory_mb: None,
            disk_mb: None,
            vnc_password: None,
        }
    }

    #[test]
    fn spec_fingerprint_covers_resource_limits() {
        let base = DockerSpec {
            container: "c1".into(),
            image: "img".into(),
            mounts: Vec::new(),
            workdir: crate::env::mounts::GUEST_WORKSPACES_ROOT.into(),
            tier: crate::env::provider::Tier::Headless,
            cpus: Some(2.0),
            memory_mb: Some(4096),
            disk_mb: Some(10240),
            vnc_password: None,
        };
        let mut tighter = base.clone();
        tighter.memory_mb = Some(1024);
        assert_ne!(
            base.fingerprint(),
            tighter.fingerprint(),
            "配额变了必须重建，否则旧容器没配额却一直复用"
        );
    }

    #[test]
    fn capabilities_follow_tier() {
        // 直接构造结构体（不真的起容器），只验证能力映射。
        let headless = DockerEnv {
            container: "c".into(),
            workdir: crate::env::mounts::GUEST_WORKSPACES_ROOT.into(),
            tier: super::super::provider::Tier::Headless,
            vnc_password: None,
            map: PathMap::new(Vec::new()),
            host_fs: super::super::local::LocalEnv::new("/tmp"),
        };
        assert_eq!(headless.kind(), EnvKind::Headless);
        assert!(!headless.capabilities().desktop, "headless 档不应有桌面");
        assert!(!headless.capabilities().snapshot, "headless 档不应有快照");

        let container = DockerEnv {
            container: "c".into(),
            workdir: crate::env::mounts::GUEST_WORKSPACES_ROOT.into(),
            tier: super::super::provider::Tier::Desktop,
            vnc_password: None,
            map: PathMap::new(Vec::new()),
            host_fs: super::super::local::LocalEnv::new("/tmp"),
        };
        assert_eq!(container.kind(), EnvKind::Desktop);
        assert!(container.capabilities().desktop, "container 档必须有桌面");
        assert!(
            !container.capabilities().snapshot,
            "快照还没实现，能力位必须是 false（声明了做不到比不声明更糟）"
        );

        // DockerSpec::default() 是 headless，避免调用方漏传档位时静默开桌面。
        assert_eq!(
            DockerSpec::default().tier,
            super::super::provider::Tier::Headless
        );
        assert_eq!(spec_with_tier(super::super::provider::Tier::Desktop).tier, super::super::provider::Tier::Desktop);
    }

    #[test]
    fn vnc_password_is_random_and_unambiguous() {
        let a = DockerSpec::random_vnc_password();
        let b = DockerSpec::random_vnc_password();
        assert_eq!(a.len(), 8);
        assert_ne!(a, b, "两次生成不应相同");
        for ch in a.chars() {
            assert!(!matches!(ch, '0' | 'O' | '1' | 'l' | 'i'), "不该出现易混字符: {ch}");
            assert!(ch.is_ascii_alphanumeric(), "只允许字母数字: {ch}");
        }
    }

    fn spec_for_args() -> DockerSpec {
        DockerSpec {
            container: "pointer-args".into(),
            image: "pointer-sandbox-headless:latest".into(),
            mounts: vec![super::super::pathmap::Mount::new("/host/data", crate::env::mounts::GUEST_DATA, false)],
            workdir: crate::env::mounts::GUEST_WORKSPACES_ROOT.into(),
            tier: super::super::provider::Tier::Headless,
            cpus: Some(2.0),
            memory_mb: Some(2048),
            disk_mb: Some(8192),
            vnc_password: None,
        }
    }

    #[test]
    fn run_args_carry_limits_and_mounts() {
        let spec = spec_for_args();
        let args = run_args(&spec, "abc123", None, true).join(" ");
        assert!(args.contains("-v /host/data:/data"), "{args}");
        assert!(args.contains("--cpus 2"), "{args}");
        assert!(args.contains("--memory 2048m"), "{args}");
        assert!(args.contains("--storage-opt size=8192m"), "{args}");
        assert!(args.contains("--label pointer.spec=abc123"), "{args}");
    }

    #[test]
    fn disk_limit_can_be_dropped_for_unsupported_platforms() {
        // macOS 不支持 --storage-opt，重试时必须能干净地去掉它。
        let spec = spec_for_args();
        let args = run_args(&spec, "abc123", None, false).join(" ");
        assert!(!args.contains("--storage-opt"), "{args}");
        assert!(args.contains("--memory 2048m"), "其他限制不该被连带丢掉：{args}");
    }

    #[test]
    fn desktop_tier_publishes_vnc_with_password() {
        let mut spec = spec_for_args();
        spec.tier = super::super::provider::Tier::Desktop;
        let args = run_args(&spec, "h", Some("abcd2345"), true).join(" ");
        assert!(args.contains("-p 127.0.0.1::5900"), "{args}");
        assert!(args.contains("-p 127.0.0.1::6080"), "{args}");
        assert!(args.contains("-e POINTER_VNC_PASSWORD=abcd2345"), "{args}");
    }

    /// 端口没映射出来时不能返回半截地址：vnc 拿不到就整体当作没有桌面。
    #[test]
    fn light_tier_has_no_desktop_endpoint() {
        let headless = DockerEnv {
            container: "c".into(),
            workdir: crate::env::mounts::GUEST_WORKSPACES_ROOT.into(),
            tier: super::super::provider::Tier::Headless,
            vnc_password: Some("abcd2345".into()),
            map: PathMap::new(Vec::new()),
            host_fs: super::super::local::LocalEnv::new("/tmp"),
        };
        assert!(
            headless.desktop_endpoint().is_none(),
            "headless 档没有桌面，不能因为带了密码就返回端点"
        );
    }
}
