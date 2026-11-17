//! RemoteEnv：E2B 兼容协议（e2b-local / CubeSandbox）上的 light 模式实现。
//!
//! ## 协议事实（2026-11-15 在 e2b-local + OrbStack 实测）
//!
//! - 控制面 `POST {control}/sandboxes`，body `{"templateID": "...", "timeout": 300}`
//!   → `{"sandboxID": "...", "envdURL": "http://127.0.0.1:PORT"}`；
//!   `DELETE {control}/sandboxes/{id}` → 204。实测创建到可执行约 1.2s。
//! - 文件 IO：`GET|POST {envd}/files?path=<绝对路径>&username=root`，body 是**原始字节**。
//!   目录不能列举（返回 400 `path is a directory`），列举一律走 exec。
//! - 执行：`POST {envd}/process.Process/Start`，`Content-Type: application/connect+json`，
//!   请求体是 Connect 帧 `[0x00][len:4BE][json]`；响应是若干帧 `[flags][len:4BE][json]`，
//!   `flags & 0x02` 表示流结束（trailers）。事件形态：
//!   - `{"event":{"start":{"pid":N}}}`（**pid 直接在 start 下**，没有 process 包一层）
//!   - `{"event":{"data":{"stdout":"<base64>"}}}` / `{"event":{"data":{"stderr":"<base64>"}}}`
//!   - `{"event":{"end":{"exitCode":N,"status":"exit status N"}}}`
//!
//!   **`exitCode` 在退出码为 0 时缺省**，按 0 处理；被信号杀死时没有 `exitCode`，按 -1 处理。
//!
//! 约束：不引 e2b 官方 SDK（它硬编码指向 e2b.cloud），这里只做薄 REST 客户端。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use serde_json::{json, Value};

use super::pathmap::{Mount, PathMap};
use super::{
    Capabilities, ChildHandle, DirEntry, EnvKind, Environment, ExecOptions, ExecOutput, FileMetadata,
    SpawnOptions, WalkEntry, WalkOptions,
};

/// 等 `start` 事件的超时：网关不流式返回时快速失败，而不是挂死。
const START_EVENT_TIMEOUT: Duration = Duration::from_secs(30);

pub const DEFAULT_CONTROL_URL: &str = "http://127.0.0.1:3000";
pub const DEFAULT_TEMPLATE: &str = "debian:bookworm-slim";
/// envd 的 files 接口需要显式用户；light 模式里 agent 以 root 身份跑在容器内。
const FILES_USER: &str = "root";
/// `open_reader` 每次 Range 请求的块大小（envd 支持 `Range`，实测返回 206）。
const READER_CHUNK_BYTES: u64 = 4 * 1024 * 1024;
/// 没显式给 timeout 时的进程上限（envd 默认只有 60s，太短）。
const DEFAULT_EXEC_TIMEOUT_SECS: u64 = 3600;
const READY_WAIT: Duration = Duration::from_secs(30);

const BASE64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

/// 创建一个远端环境的参数。业务层只填这些，不碰 envd 的 metadata JSON。
#[derive(Debug, Clone)]
pub struct RemoteSpec {
    /// 控制面 base URL（e2b-local 默认 `http://127.0.0.1:3000`）。
    pub control_url: String,
    /// 镜像 / 模板 id。
    pub template: String,
    /// 沙箱存活上限（秒）。
    pub timeout_secs: u64,
    /// agent 可见的工作区根（guest 路径）。
    pub workspace_root: PathBuf,
    /// agent 可见的技能根（guest 路径）。
    pub skills_root: PathBuf,
    /// agent 可见的家目录（guest 路径）。
    pub home_root: PathBuf,
    /// 挂载表。由 provider 序列化进请求，业务层不感知。
    pub mounts: Vec<Mount>,
    /// e2b-local 的卷宿主目录（`docker.volume_host_path`）。
    ///
    /// 有值 → 发 `volumeMounts`（每个挂载点在宿主侧建 `<dir>/<slug>` → 宿主目录的符号链接）；
    /// 无值 → 退回 Cube 的 `metadata["host-mount"]`。
    pub volume_host_path: Option<PathBuf>,
}

impl Default for RemoteSpec {
    fn default() -> Self {
        Self {
            control_url: DEFAULT_CONTROL_URL.to_string(),
            template: DEFAULT_TEMPLATE.to_string(),
            timeout_secs: 900,
            workspace_root: PathBuf::from("/workspace"),
            skills_root: PathBuf::from("/skills"),
            home_root: PathBuf::from(crate::env::mounts::GUEST_HOME),
            mounts: Vec::new(),
            volume_host_path: None,
        }
    }
}

/// 远端（light）执行环境。
pub struct RemoteEnv {
    control_url: String,
    sandbox_id: String,
    envd_url: String,
    workspace_root: PathBuf,
    skills_root: PathBuf,
    home_root: PathBuf,
    path_map: PathMap,
    capabilities: Capabilities,
    http: reqwest::blocking::Client,
    has_rg: bool,
    /// 沙箱内是否有 coreutils `timeout`（envd 不认 timeout 字段，只能自己兜底）。
    has_timeout: bool,
}

impl RemoteEnv {
    /// 建一个沙箱并等到它可执行。
    pub fn create(spec: &RemoteSpec) -> Result<Self> {
        super::run_blocking(|| {
        let http = build_client()?;
        // e2b-local：卷目录必须落在宿主可见路径，且 `<volume_host_path>/<slug>` 指向宿主目录。
        let volume_names = match spec.volume_host_path.as_deref() {
            Some(dir) => {
                let names = volume_names_for(dir, &spec.mounts);
                ensure_volumes(&http, &spec.control_url, dir, &names)?;
                names
            }
            None => Vec::new(),
        };
        let body = build_create_body(spec, &volume_names);
        let resp = http
            .post(format!("{}/sandboxes", spec.control_url.trim_end_matches('/')))
            .timeout(Duration::from_secs(120))
            .json(&body)
            .send()
            .context("create sandbox: request failed")?;
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        if !status.is_success() {
            bail!(
                "create sandbox failed: HTTP {} {}",
                status.as_u16(),
                snippet(text.as_bytes())
            );
        }
        let v: Value = serde_json::from_str(&text).context("create sandbox: bad JSON")?;
        let sandbox_id = v
            .get("sandboxID")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("create sandbox: missing sandboxID in {text}"))?
            .to_string();
        let envd_url = v
            .get("envdURL")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("create sandbox: missing envdURL in {text}"))?
            .trim_end_matches('/')
            .to_string();

        let mut env = Self {
            control_url: spec.control_url.trim_end_matches('/').to_string(),
            sandbox_id,
            envd_url,
            workspace_root: spec.workspace_root.clone(),
            skills_root: spec.skills_root.clone(),
            home_root: spec.home_root.clone(),
            path_map: PathMap::new(spec.mounts.clone()),
            capabilities: Capabilities::light(),
            http,
            has_rg: false,
            has_timeout: false,
        };
        env.wait_ready()?;
        env.prepare_dirs()?;
        env.has_rg = env.detect_rg();
        env.has_timeout = env.detect_timeout();
        Ok(env)
        })
    }

    /// 附着到已存在的沙箱（调试 / 测试用）。
    pub fn attach(spec: &RemoteSpec, sandbox_id: impl Into<String>, envd_url: impl Into<String>) -> Result<Self> {
        Ok(Self {
            control_url: spec.control_url.trim_end_matches('/').to_string(),
            sandbox_id: sandbox_id.into(),
            envd_url: envd_url.into().trim_end_matches('/').to_string(),
            workspace_root: spec.workspace_root.clone(),
            skills_root: spec.skills_root.clone(),
            home_root: spec.home_root.clone(),
            path_map: PathMap::new(spec.mounts.clone()),
            capabilities: Capabilities::light(),
            http: build_client()?,
            has_rg: false,
            has_timeout: false,
        })
    }

    pub fn sandbox_id(&self) -> &str {
        &self.sandbox_id
    }

    pub fn envd_url(&self) -> &str {
        &self.envd_url
    }

    /// 销毁沙箱。已经不存在时视为成功。
    pub fn destroy(&self) -> Result<()> {
        super::run_blocking(|| {
        let resp = self
            .http
            .delete(format!(
                "{}/sandboxes/{}",
                self.control_url, self.sandbox_id
            ))
            .timeout(Duration::from_secs(60))
            .send()
            .context("destroy sandbox: request failed")?;
        let status = resp.status();
        if status.is_success() || status.as_u16() == 404 {
            return Ok(());
        }
        let body = resp.bytes().unwrap_or_default();
        bail!(
            "destroy sandbox failed: HTTP {} {}",
            status.as_u16(),
            snippet(&body)
        );
        })
    }

    fn wait_ready(&mut self) -> Result<()> {
        let started = std::time::Instant::now();
        let mut last = String::new();
        while started.elapsed() < READY_WAIT {
            match self.exec(
                &["true".to_string()],
                ExecOptions::new()
                    .with_cwd("/")
                    .with_timeout(Duration::from_secs(5)),
            ) {
                Ok(out) if out.success() => return Ok(()),
                Ok(out) => last = format!("status {}: {}", out.status, out.stderr_str()),
                Err(e) => last = format!("{e:#}"),
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        bail!(
            "sandbox {} not ready after {:?} ({last})",
            self.sandbox_id,
            READY_WAIT
        )
    }

    fn prepare_dirs(&self) -> Result<()> {
        let script = format!(
            "mkdir -p {} {}",
            sh_quote(&self.workspace_root),
            sh_quote(&self.home_root)
        );
        let out = self.run_sh(&script)?;
        if !out.success() {
            bail!("prepare dirs failed: {}", out.stderr_str().trim());
        }
        Ok(())
    }

    fn detect_rg(&self) -> bool {
        self.run_sh("command -v rg >/dev/null 2>&1")
            .map(|o| o.success())
            .unwrap_or(false)
    }

    fn detect_timeout(&self) -> bool {
        self.run_sh("command -v timeout >/dev/null 2>&1")
            .map(|o| o.success())
            .unwrap_or(false)
    }

    fn files_url(&self, path: &Path) -> Result<String> {
        let p = path_to_str(path)?;
        Ok(format!(
            "{}/files?path={}&username={}",
            self.envd_url,
            urlencode(&p),
            FILES_USER
        ))
    }

    /// 内部用：跑一段 shell 脚本（cwd 固定 `/`，避免受工作区影响）。
    fn run_sh(&self, script: &str) -> Result<ExecOutput> {
        self.exec(
            &["/bin/sh".to_string(), "-c".to_string(), script.to_string()],
            ExecOptions::new().with_cwd("/"),
        )
    }

    fn find_entries(
        &self,
        root: &Path,
        max_depth: usize,
        include_hidden: bool,
        exclude_git: bool,
    ) -> Result<Vec<WalkEntry>> {
        let mut script = format!(
            "find {} -mindepth 1 -maxdepth {}",
            sh_quote(root),
            max_depth.max(1)
        );
        if !include_hidden {
            script.push_str(" ! -path '*/.*'");
        }
        if exclude_git {
            script.push_str(" ! -path '*/.git/*' ! -name .git");
        }
        script.push_str(" -printf '%y\\0%p\\0'");
        let out = self.run_sh(&script)?;
        if !out.success() {
            bail!(
                "find {} failed: {}",
                root.display(),
                out.stderr_str().trim()
            );
        }
        Ok(parse_find_nul(&out.stdout)
            .into_iter()
            .filter_map(|(kind, path)| match kind {
                'd' => Some(WalkEntry {
                    path,
                    is_dir: true,
                    is_symlink: false,
                }),
                'l' => Some(WalkEntry {
                    path,
                    is_dir: false,
                    is_symlink: true,
                }),
                'f' => Some(WalkEntry {
                    path,
                    is_dir: false,
                    is_symlink: false,
                }),
                _ => None,
            })
            .collect())
    }
}

impl Environment for RemoteEnv {
    fn kind(&self) -> EnvKind {
        EnvKind::Light
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

    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn path_map(&self) -> &PathMap {
        &self.path_map
    }

    fn exec(&self, argv: &[String], opts: ExecOptions) -> Result<ExecOutput> {
        super::run_blocking(|| {
        if argv.is_empty() {
            bail!("exec: empty argv");
        }
        let cwd = opts
            .cwd
            .clone()
            .unwrap_or_else(|| self.workspace_root.clone());
        let cwd = path_to_str(&cwd)?;

        // stdin：envd 的 Start 不支持直接喂，落成临时文件再重定向。
        let mut stdin_file: Option<String> = None;
        if let Some(data) = opts.stdin.as_ref() {
            let name = format!("/tmp/.pointer-stdin-{}", unique_suffix());
            self.write_file(Path::new(&name), data)?;
            stdin_file = Some(name);
        }

        let wall = opts
            .timeout
            .unwrap_or_else(|| Duration::from_secs(DEFAULT_EXEC_TIMEOUT_SECS));
        // envd 不认 ProcessStartRequest.timeout（实测 sleep 5 + timeout 1 仍跑满 5s），
        // 只有沙箱内有 coreutils `timeout` 时才能自己兜底。
        let wall_guard = opts
            .timeout
            .filter(|_| self.has_timeout)
            .map(|d| d.as_secs().max(1));

        let wrap = !opts.env_remove.is_empty() || stdin_file.is_some() || wall_guard.is_some();
        let (cmd, args) = if wrap {
            let mut script = String::new();
            if !opts.env_remove.is_empty() {
                script.push_str("unset ");
                script.push_str(&opts.env_remove.join(" "));
                script.push_str("; ");
            }
            match wall_guard {
                // envd 不认 ProcessStartRequest.timeout（实测 sleep 5 + timeout 1 仍跑满 5s），
                // 所以在沙箱内用 coreutils `timeout -s KILL` 自己兜底。
                Some(secs) => script.push_str(&format!("exec timeout -s KILL {secs} \"$0\" \"$@\"")),
                None => script.push_str("exec \"$0\" \"$@\""),
            }
            match stdin_file.as_ref() {
                Some(p) => script.push_str(&format!(" < {}", sh_quote(Path::new(p)))),
                None => {}
            }
            let mut a = vec!["-c".to_string(), script, argv[0].clone()];
            a.extend_from_slice(&argv[1..]);
            ("/bin/sh".to_string(), a)
        } else {
            (argv[0].clone(), argv[1..].to_vec())
        };

        let mut process = json!({
            "cmd": cmd,
            "args": args,
            "cwd": cwd,
            "timeout": wall.as_secs().max(1),
        });
        if !opts.env.is_empty() {
            let mut envs = serde_json::Map::new();
            for (k, v) in &opts.env {
                envs.insert(k.clone(), Value::String(v.clone()));
            }
            process["envs"] = Value::Object(envs);
        }

        let payload = serde_json::to_vec(&json!({ "process": process }))?;
        let body = encode_connect_frame(&payload);
        let resp = self
            .http
            .post(format!("{}/process.Process/Start", self.envd_url))
            .header("Content-Type", "application/connect+json")
            .timeout(wall + Duration::from_secs(60))
            .body(body)
            .send();

        let raw = match resp {
            Ok(r) => {
                let status = r.status();
                let bytes = r.bytes().unwrap_or_default().to_vec();
                if !status.is_success() {
                    bail!(
                        "envd exec failed: HTTP {} {}",
                        status.as_u16(),
                        snippet(&bytes)
                    );
                }
                bytes
            }
            Err(e) if e.is_timeout() => {
                return Ok(ExecOutput {
                    status: -1,
                    stdout: Vec::new(),
                    stderr: format!("exec timed out after {wall:?}").into_bytes(),
                    stdout_truncated: false,
                    stderr_truncated: false,
                    timed_out: true,
                });
            }
            Err(e) => return Err(anyhow!("envd exec request failed: {e}")),
        };

        let parsed = parse_exec_frames(&raw);
        let cap = opts.max_output_bytes.unwrap_or(usize::MAX);
        let (stdout, stdout_truncated) = cap_bytes(parsed.stdout, cap);
        let (stderr, stderr_truncated) = cap_bytes(parsed.stderr, cap);
        let mut status = parsed.exit_code.unwrap_or(0);
        let mut timed_out = parsed.timed_out;
        // coreutils `timeout` 被杀时退出码是 124（TERM）或 137（KILL）。
        if wall_guard.is_some() && (status == 124 || status == 137) {
            timed_out = true;
            status = -1;
        }
        Ok(ExecOutput {
            status,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            timed_out,
        })
        })
    }

    fn spawn(&self, argv: &[String], opts: SpawnOptions) -> Result<Box<dyn ChildHandle>> {
        super::run_blocking(|| {
        if argv.is_empty() {
            bail!("spawn: empty argv");
        }
        let cwd = opts
            .cwd
            .clone()
            .unwrap_or_else(|| self.workspace_root.clone());
        let cwd = path_to_str(&cwd)?;

        let mut process = json!({
            "cmd": argv[0],
            "args": argv[1..].to_vec(),
            "cwd": cwd,
        });
        if !opts.env.is_empty() {
            let mut envs = serde_json::Map::new();
            for (k, v) in &opts.env {
                envs.insert(k.clone(), Value::String(v.clone()));
            }
            process["envs"] = Value::Object(envs);
        }

        // Start 是 server-streaming：进程活着流就一直开着，所以这里**不设总超时**。
        let payload = serde_json::to_vec(&json!({ "process": process }))?;
        let resp = self
            .http
            .post(format!("{}/process.Process/Start", self.envd_url))
            .header("Content-Type", "application/connect+json")
            .header("Connect-Protocol-Version", "1")
            .body(encode_connect_frame(&payload))
            .send()
            .map_err(|e| anyhow!("envd spawn 请求失败: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.bytes().unwrap_or_default().to_vec();
            bail!(
                "envd spawn 失败: HTTP {} {}",
                status.as_u16(),
                snippet(&body)
            );
        }

        // Start 是 server-streaming：pid 在第一个 `start` 事件里。
        // 交给后台线程读，主线程最多等 START_EVENT_TIMEOUT，避免网关不流式时挂死。
        let (tx, rx) = channel::<Vec<u8>>();
        let (pid_tx, pid_rx) = channel::<Result<u32>>();
        let exit = Arc::new(Mutex::new(None::<i32>));
        let alive = Arc::new(AtomicBool::new(true));
        {
            let exit = exit.clone();
            let alive = alive.clone();
            let stream = std::io::BufReader::new(resp);
            std::thread::spawn(move || pump_process_frames(stream, tx, pid_tx, exit, alive));
        }

        let pid = match pid_rx.recv_timeout(START_EVENT_TIMEOUT) {
            Ok(Ok(pid)) => pid,
            Ok(Err(e)) => return Err(e),
            Err(_) => bail!(
                "envd spawn: {}s 内没收到 start 事件——网关没有把 Start 响应流式返回                 （e2b-local shim 会缓冲到进程结束；真 E2B / Cube 正常）",
                START_EVENT_TIMEOUT.as_secs()
            ),
        };

        Ok(Box::new(RemoteChild {
            http: self.http.clone(),
            envd_url: self.envd_url.clone(),
            pid,
            stdout_rx: rx,
            exit,
            alive,
        }) as Box<dyn ChildHandle>)
        })
    }

    fn pty_open(&self, spec: crate::env::PtySpec) -> Result<Box<dyn crate::env::PtyHandle>> {
        super::run_blocking(|| {
            if spec.argv.is_empty() {
                bail!("pty 需要可执行文件");
            }
            let mut process = json!({
                "cmd": spec.argv[0],
                "args": spec.argv[1..].to_vec(),
            });
            if let Some(cwd) = &spec.cwd {
                process["cwd"] = Value::String(cwd.display().to_string());
            }
            if !spec.env.is_empty() {
                let mut envs = serde_json::Map::new();
                for (k, v) in &spec.env {
                    envs.insert(k.clone(), Value::String(v.clone()));
                }
                process["envs"] = Value::Object(envs);
            }

            // pty 是 **StartRequest** 的字段，不在 ProcessConfig 里（放错层会被静默忽略）。
            let payload = serde_json::to_vec(&json!({
                "process": process,
                "pty": { "size": { "cols": spec.cols.max(1), "rows": spec.rows.max(1) } }
            }))?;
            let resp = self
                .http
                .post(format!("{}/process.Process/Start", self.envd_url))
                .header("Content-Type", "application/connect+json")
                .header("Connect-Protocol-Version", "1")
                .body(encode_connect_frame(&payload))
                .send()
                .map_err(|e| anyhow!("envd pty 请求失败: {e}"))?;
            let status = resp.status();
            if !status.is_success() {
                let body = resp.bytes().unwrap_or_default().to_vec();
                bail!("envd pty 失败: HTTP {} {}", status.as_u16(), snippet(&body));
            }

            let (tx, rx) = channel::<Vec<u8>>();
            let (pid_tx, pid_rx) = channel::<Result<u32>>();
            let exit = Arc::new(Mutex::new(None::<i32>));
            let alive = Arc::new(AtomicBool::new(true));
            {
                let exit = exit.clone();
                let alive = alive.clone();
                let stream = std::io::BufReader::new(resp);
                std::thread::spawn(move || pump_process_frames(stream, tx, pid_tx, exit, alive));
            }
            let pid = match pid_rx.recv_timeout(START_EVENT_TIMEOUT) {
                Ok(Ok(pid)) => pid,
                Ok(Err(e)) => return Err(e),
                Err(_) => bail!(
                    "envd pty: {}s 内没收到 start 事件",
                    START_EVENT_TIMEOUT.as_secs()
                ),
            };
            Ok(Box::new(RemotePty {
                http: self.http.clone(),
                envd_url: self.envd_url.clone(),
                pid,
                rx: Mutex::new(Some(rx)),
                exit,
                alive,
            }) as Box<dyn crate::env::PtyHandle>)
        })
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        super::run_blocking(|| {
        let resp = self
            .http
            .get(self.files_url(path)?)
            .timeout(Duration::from_secs(120))
            .send()
            .with_context(|| format!("read_file {}", path.display()))?;
        let status = resp.status();
        let bytes = resp.bytes().unwrap_or_default().to_vec();
        if !status.is_success() {
            bail!(
                "read_file {} failed: HTTP {} {}",
                path.display(),
                status.as_u16(),
                snippet(&bytes)
            );
        }
        Ok(bytes)
        })
    }

    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        super::run_blocking(|| {
        let resp = self
            .http
            .post(self.files_url(path)?)
            .header("Content-Type", "application/octet-stream")
            .timeout(Duration::from_secs(300))
            .body(data.to_vec())
            .send()
            .with_context(|| format!("write_file {}", path.display()))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let body = resp.bytes().unwrap_or_default();
        bail!(
            "write_file {} failed: HTTP {} {}",
            path.display(),
            status.as_u16(),
            snippet(&body)
        );
        })
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        super::run_blocking(|| {
        let entries = self.find_entries(path, 1, true, false)?;
        Ok(entries
            .into_iter()
            .map(|e| DirEntry {
                name: e
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: e.path,
                is_dir: e.is_dir,
                is_symlink: e.is_symlink,
            })
            .collect())
        })
    }

    fn metadata(&self, path: &Path) -> Result<FileMetadata> {
        super::run_blocking(|| {
        let script = format!("stat -c '%F|%s' -- {}", sh_quote(path));
        let out = self.run_sh(&script)?;
        if !out.success() {
            bail!(
                "metadata {} failed: {}",
                path.display(),
                out.stderr_str().trim()
            );
        }
        let text = out.stdout_str();
        let line = text.lines().next().unwrap_or("");
        let (kind, len) = line
            .rsplit_once('|')
            .ok_or_else(|| anyhow!("unexpected stat output for {}: {line}", path.display()))?;
        Ok(FileMetadata {
            is_file: kind.starts_with("regular"),
            is_dir: kind.starts_with("directory"),
            is_symlink: kind.contains("symbolic link"),
            len: len.trim().parse().unwrap_or(0),
        })
        })
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        super::run_blocking(|| {
        let script = format!("realpath -- {}", sh_quote(path));
        let out = self.run_sh(&script)?;
        if !out.success() {
            bail!(
                "canonicalize {} failed: {}",
                path.display(),
                out.stderr_str().trim()
            );
        }
        Ok(PathBuf::from(out.stdout_str().trim()))
        })
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        super::run_blocking(|| {
        let script = format!("mkdir -p -- {}", sh_quote(path));
        let out = self.run_sh(&script)?;
        if !out.success() {
            bail!(
                "create_dir_all {} failed: {}",
                path.display(),
                out.stderr_str().trim()
            );
        }
        Ok(())
        })
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        super::run_blocking(|| {
        let bytes = self.read_file(path)?;
        String::from_utf8(bytes).map_err(|e| anyhow!("read_to_string {}: not UTF-8: {e}", path.display()))
        })
    }

    fn open_reader(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>> {
        super::run_blocking(|| {
        let meta = self.metadata(path)?;
        Ok(Box::new(RangeReader::new(
            self.http.clone(),
            self.files_url(path)?,
            meta.len,
        )) as Box<dyn std::io::Read + Send>)
        })
    }

    fn walk(&self, root: &Path, opts: WalkOptions) -> Result<Vec<WalkEntry>> {
        super::run_blocking(|| {
        // 需要 .gitignore 语义时优先用 rg（一次往返拿全批，且天然跳过 .git）；
        // 镜像里没有 rg 就退回 find（语义退化：不读 .gitignore）。
        if opts.respect_gitignore && self.has_rg {
            let mut script = format!("rg --files --no-messages --max-depth {}", opts.max_depth);
            if opts.include_hidden {
                script.push_str(" --hidden");
            }
            script.push_str(&format!(" -- {}", sh_quote(root)));
            let out = self.run_sh(&script)?;
            // rg 退出码 1 = 没有匹配，不算错误。
            if !out.success() && out.status != 1 {
                bail!("rg failed in {}: {}", root.display(), out.stderr_str().trim());
            }
            return Ok(out
                .stdout_str()
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| WalkEntry {
                    path: PathBuf::from(l),
                    is_dir: false,
                    is_symlink: false,
                })
                .collect());
        }
        self.find_entries(root, opts.max_depth, opts.include_hidden, opts.exclude_git_metadata)
        })
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        super::run_blocking(|| {
        let script = format!("rm -f -- {}", sh_quote(path));
        let out = self.run_sh(&script)?;
        if !out.success() {
            bail!(
                "remove_file {} failed: {}",
                path.display(),
                out.stderr_str().trim()
            );
        }
        Ok(())
        })
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        super::run_blocking(|| {
        let script = format!("rm -rf -- {}", sh_quote(path));
        let out = self.run_sh(&script)?;
        if !out.success() {
            bail!(
                "remove_dir_all {} failed: {}",
                path.display(),
                out.stderr_str().trim()
            );
        }
        Ok(())
        })
    }

    fn set_executable(&self, path: &Path) -> Result<()> {
        super::run_blocking(|| {
        let script = format!("chmod 700 -- {}", sh_quote(path));
        let out = self.run_sh(&script)?;
        if !out.success() {
            bail!(
                "set_executable {} failed: {}",
                path.display(),
                out.stderr_str().trim()
            );
        }
        Ok(())
        })
    }
}

// ---------------------------------------------------------------- 协议编解码

/// 按 HTTP Range 分块拉取的只读流。
///
/// envd 的 `/files` 支持 `Range`（实测 `Accept-Ranges: bytes`、请求 206 + `Content-Range`），
/// 所以大文件不再整文件下载；服务端若不支持 Range（返回 200），退化为一次性读完。
pub(crate) struct RangeReader {
    http: reqwest::blocking::Client,
    url: String,
    total: u64,
    offset: u64,
    buf: Vec<u8>,
    pos: usize,
    chunk: u64,
}

impl RangeReader {
    pub(crate) fn new(http: reqwest::blocking::Client, url: String, total: u64) -> Self {
        Self {
            http,
            url,
            total,
            offset: 0,
            buf: Vec::new(),
            pos: 0,
            chunk: READER_CHUNK_BYTES,
        }
    }

    fn fill(&mut self) -> std::io::Result<bool> {
        if self.offset >= self.total {
            return Ok(false);
        }
        let end = (self.offset + self.chunk - 1).min(self.total.saturating_sub(1));
        let resp = self
            .http
            .get(&self.url)
            .header("Range", format!("bytes={}-{}", self.offset, end))
            .timeout(Duration::from_secs(120))
            .send()
            .map_err(|e| std::io::Error::other(format!("range read failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(std::io::Error::other(format!(
                "range read failed: HTTP {}",
                status.as_u16()
            )));
        }
        let body = resp
            .bytes()
            .map_err(|e| std::io::Error::other(format!("range read body failed: {e}")))?
            .to_vec();
        if body.is_empty() {
            return Ok(false);
        }
        self.offset += body.len() as u64;
        self.buf = body;
        self.pos = 0;
        Ok(true)
    }
}

impl std::io::Read for RangeReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.buf.len() && !self.fill()? {
            return Ok(0);
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// 从 Connect 流里读一帧：`(flags, body)`；流结束返回 `None`。
fn read_connect_frame(stream: &mut impl std::io::Read) -> Result<Option<(u8, Vec<u8>)>> {
    let mut header = [0u8; 5];
    match stream.read_exact(&mut header) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(anyhow!("读 connect 帧头失败: {e}")),
    }
    let flags = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    let mut body = vec![0u8; len];
    stream
        .read_exact(&mut body)
        .map_err(|e| anyhow!("读 connect 帧体失败: {e}"))?;
    Ok(Some((flags, body)))
}

/// 后台泵：把 stdout 推进 channel，退出时写 exit code。
fn pump_process_frames(
    mut stream: std::io::BufReader<reqwest::blocking::Response>,
    tx: std::sync::mpsc::Sender<Vec<u8>>,
    pid_tx: std::sync::mpsc::Sender<Result<u32>>,
    exit: Arc<Mutex<Option<i32>>>,
    alive: Arc<AtomicBool>,
) {
    let mut announced_pid = false;
    loop {
        match read_connect_frame(&mut stream) {
            Ok(Some((flags, body))) => {
                if flags & 0x02 != 0 {
                    break;
                }
                let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                if !announced_pid {
                    // 实测 envd 发的是 `{"event":{"start":{"pid":N}}}`；
                    // 兼容个别版本多包一层 `process`。
                    let pid = v
                        .pointer("/event/start/pid")
                        .or_else(|| v.pointer("/event/start/process/pid"))
                        .and_then(|p| p.as_u64());
                    if let Some(pid) = pid {
                        announced_pid = true;
                        let _ = pid_tx.send(Ok(pid as u32));
                    }
                }
                // pty 会话的输出走 `data.pty`（不是 stdout），合并进同一路字节流。
                if let Some(b64) = v.pointer("/event/data/pty").and_then(|s| s.as_str()) {
                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                        if !bytes.is_empty() && tx.send(bytes).is_err() {
                            break;
                        }
                    }
                }
                if let Some(b64) = v.pointer("/event/data/stdout").and_then(|s| s.as_str()) {
                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                        if !bytes.is_empty() && tx.send(bytes).is_err() {
                            break;
                        }
                    }
                }
                if let Some(b64) = v.pointer("/event/data/stderr").and_then(|s| s.as_str()) {
                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                        let text = String::from_utf8_lossy(&bytes);
                        let text = text.trim();
                        if !text.is_empty() {
                            log::debug!("remote spawn stderr: {text}");
                        }
                    }
                }
                if let Some(end) = v.pointer("/event/end") {
                    // envd 在退出码为 0 时**不写** `exitCode`；被信号杀死时也没有，
                    // 但 `status`/`error` 里带 signal/killed 字样。规则与 exec 保持一致。
                    let status = end.get("status").and_then(|x| x.as_str()).unwrap_or("");
                    let error = end.get("error").and_then(|x| x.as_str()).unwrap_or("");
                    let signaled = status.contains("signal")
                        || error.contains("signal")
                        || status.contains("killed");
                    let code = match end.get("exitCode").and_then(|c| c.as_i64()) {
                        Some(c) => c as i32,
                        None if signaled => -1,
                        None => 0,
                    };
                    *exit.lock().unwrap() = Some(code);
                    break;
                }
            }
            Ok(None) => break,
            Err(e) => {
                if !announced_pid {
                    let _ = pid_tx.send(Err(anyhow!("读 connect 帧失败: {e}")));
                } else {
                    log::warn!("remote spawn 流读取失败: {e}");
                }
                break;
            }
        }
    }
    if !announced_pid {
        let _ = pid_tx.send(Err(anyhow!("envd spawn: 流结束但没收到 start 事件")));
    }
    alive.store(false, Ordering::SeqCst);
    if exit.lock().unwrap().is_none() {
        *exit.lock().unwrap() = Some(-1);
    }
}

/// `ChildHandle` 的 channel 读端。
struct ChannelReader {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
    closed: bool,
}

impl std::io::Read for ChannelReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.pos >= self.buf.len() {
            if self.closed {
                return Ok(0);
            }
            match self.rx.recv() {
                Ok(chunk) => {
                    self.buf = chunk;
                    self.pos = 0;
                }
                Err(_) => {
                    self.closed = true;
                    return Ok(0);
                }
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// 远端常驻子进程。
///
/// 协议（官方 E2B）：
/// - 启动：`Process/Start`（server streaming，不设总超时）；
/// - 写 stdin：`Process/SendInput`，body `{"process":{...},"input":{"stdin":"<base64>"}}`；
/// - 关 stdin：`Process/CloseStdin`（仅非 PTY）；
/// - 收尾：`Process/SendSignal`，`signal: "SIGNAL_SIGKILL"`。
struct RemoteChild {
    http: reqwest::blocking::Client,
    envd_url: String,
    pid: u32,
    stdout_rx: Receiver<Vec<u8>>,
    exit: Arc<Mutex<Option<i32>>>,
    alive: Arc<AtomicBool>,
}

/// 沙箱内交互式终端：pty 由 envd 分配，输入走 `input.pty`，resize 走 `Process.Update`。
struct RemotePty {
    http: reqwest::blocking::Client,
    envd_url: String,
    pid: u32,
    rx: Mutex<Option<Receiver<Vec<u8>>>>,
    exit: Arc<Mutex<Option<i32>>>,
    alive: Arc<AtomicBool>,
}

impl RemotePty {
    fn unary(&self, method: &str, body: Value) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/process.Process/{}", self.envd_url, method))
            .header("Content-Type", "application/json")
            .header("Connect-Protocol-Version", "1")
            .timeout(Duration::from_secs(30))
            .body(serde_json::to_vec(&body)?)
            .send()
            .map_err(|e| anyhow!("envd pty {method} 请求失败: {e}"))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let body = resp.bytes().unwrap_or_default().to_vec();
        bail!(
            "envd pty {method} 失败: HTTP {} {}",
            status.as_u16(),
            snippet(&body)
        );
    }
}

impl crate::env::PtyHandle for RemotePty {
    fn id(&self) -> u32 {
        self.pid
    }
    fn write(&self, data: &[u8]) -> Result<()> {
        if !self.alive.load(Ordering::SeqCst) {
            bail!("pty 已退出，无法写入");
        }
        let encoded = base64::engine::general_purpose::STANDARD.encode(data);
        self.unary(
            "SendInput",
            json!({ "process": { "pid": self.pid }, "input": { "pty": encoded } }),
        )
    }
    fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.unary(
            "Update",
            json!({
                "process": { "pid": self.pid },
                "pty": { "size": { "cols": cols.max(1), "rows": rows.max(1) } }
            }),
        )
    }
    fn take_reader(&self) -> Result<Box<dyn std::io::Read + Send>> {
        let rx = self
            .rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .ok_or_else(|| anyhow!("pty 输出流只能取一次"))?;
        Ok(Box::new(ChannelReader {
            rx,
            buf: Vec::new(),
            pos: 0,
            closed: false,
        }))
    }
    fn try_wait(&self) -> Result<Option<i32>> {
        Ok(*self.exit.lock().unwrap_or_else(|e| e.into_inner()))
    }
    fn kill(&self) -> Result<()> {
        self.unary(
            "SendSignal",
            json!({ "process": { "pid": self.pid }, "signal": "SIGNAL_SIGKILL" }),
        )
    }
}

impl RemoteChild {
    fn unary(&self, method: &str, body: Value) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/process.Process/{}", self.envd_url, method))
            .header("Content-Type", "application/json")
            .header("Connect-Protocol-Version", "1")
            .timeout(Duration::from_secs(30))
            .body(serde_json::to_vec(&body)?)
            .send()
            .map_err(|e| anyhow!("envd {method} 请求失败: {e}"))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let body = resp.bytes().unwrap_or_default().to_vec();
        let text = snippet(&body);
        if status.as_u16() == 501 || text.contains("unimplemented") {
            bail!(
                "网关未实现 process.Process/{method}（HTTP {} {}）：                 常驻进程需要真 E2B 或 CubeSandbox，e2b-local shim 不支持",
                status.as_u16(),
                text
            );
        }
        bail!("envd {method} 失败: HTTP {} {}", status.as_u16(), text);
    }
}

impl ChildHandle for RemoteChild {
    fn id(&self) -> u32 {
        self.pid
    }

    fn write_all(&mut self, data: &[u8]) -> Result<()> {
        if !self.alive.load(Ordering::SeqCst) {
            bail!("子进程已退出，无法写入 stdin");
        }
        let encoded = base64::engine::general_purpose::STANDARD.encode(data);
        self.unary(
            "SendInput",
            json!({ "process": { "pid": self.pid }, "input": { "stdin": encoded } }),
        )
    }

    fn close_stdin(&mut self) -> Result<()> {
        self.unary("CloseStdin", json!({ "process": { "pid": self.pid } }))
    }

    fn reader(&mut self) -> Result<Box<dyn std::io::BufRead + Send>> {
        // 每个句柄只能取一次：把 channel 读端交出去。
        let rx = std::mem::replace(&mut self.stdout_rx, {
            let (_tx, rx) = channel();
            rx
        });
        Ok(Box::new(std::io::BufReader::new(ChannelReader {
            rx,
            buf: Vec::new(),
            pos: 0,
            closed: false,
        })))
    }

    fn try_wait(&mut self) -> Result<Option<i32>> {
        Ok(*self.exit.lock().unwrap())
    }

    fn wait(&mut self) -> Result<i32> {
        loop {
            if let Some(code) = *self.exit.lock().unwrap() {
                return Ok(code);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn kill(&mut self) -> Result<()> {
        self.unary(
            "SendSignal",
            json!({ "process": { "pid": self.pid }, "signal": "SIGNAL_SIGKILL" }),
        )?;
        self.alive.store(false, Ordering::SeqCst);
        Ok(())
    }
}

fn build_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .context("build http client")
}

/// 组装建沙箱请求体。
///
/// - 有卷名（e2b-local）→ `volumeMounts: [{name, path, readOnly}]`
/// - 无卷名（Cube）→ `metadata["host-mount"]`（Cube 专有）
///
/// `readOnly` 在 Cube 上生效，e2b-local 会忽略（实测仍是 rw）。
fn build_create_body(spec: &RemoteSpec, volume_names: &[Option<String>]) -> Value {
    let mut body = json!({
        "templateID": spec.template,
        "timeout": spec.timeout_secs,
    });
    // 按下标配对：`volume_names` 与 `spec.mounts` 等长，`None` 表示该挂载点没有卷。
    // 不能用 `zip`——卷名是过滤后的列表，zip 会把挂载安到别人的卷上（或静默丢掉）。
    let volume_mounts: Vec<Value> = spec
        .mounts
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let name = volume_names.get(i).and_then(|n| n.as_ref())?;
            Some(json!({
                "name": name,
                "path": m.guest.to_string_lossy(),
                "readOnly": m.read_only,
            }))
        })
        .collect();
    if !volume_mounts.is_empty() {
        body["volumeMounts"] = Value::Array(volume_mounts);
    } else if !spec.mounts.is_empty() {
        let mounts: Vec<Value> = spec
            .mounts
            .iter()
            .map(|m| {
                json!({
                    "hostPath": m.host.to_string_lossy(),
                    "mountPath": m.guest.to_string_lossy(),
                    "readOnly": m.read_only,
                })
            })
            .collect();
        body["metadata"] = json!({
            "host-mount": serde_json::to_string(&mounts).unwrap_or_else(|_| "[]".into()),
        });
    }
    body
}

/// 从挂载表挑出 e2b-local 可用的卷名。
///
/// e2b-local 只接受**卷根下的真实目录**且必须经 `POST /volumes` 注册
/// （否则报 `refusing to use unmanaged docker local volume directory`），
/// 所以：宿主目录直接在 `volume_host_path` 下 → 目录名即卷名；否则跳过并告警。
fn volume_names_for(volume_host_path: &Path, mounts: &[Mount]) -> Vec<Option<String>> {
    mounts
        .iter()
        .map(|mount| {
            if mount.host.parent() != Some(volume_host_path) {
                log::warn!(
                    "skip mount {} -> {}: e2b-local 只支持 volume_host_path 下的真实目录",
                    mount.host.display(),
                    mount.guest.display()
                );
                return None;
            }
            let name = mount
                .host
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())?;
            if !is_valid_volume_name(&name) {
                log::warn!("skip mount {}: 目录名不是合法卷名", mount.host.display());
                return None;
            }
            Some(name)
        })
        .collect()
}

/// 注册一个卷（幂等：已注册视为成功）。
///
/// e2b-local 会创建卷目录并写注册信息；如果目录已存在但没注册，网关会拒绝
/// （`refusing to use unmanaged docker local volume directory`），需要人工删掉残留目录。
pub fn ensure_volume(control_url: &str, name: &str) -> Result<()> {
    let http = build_client()?;
    ensure_volume_with(&http, control_url, name)
}

fn ensure_volume_with(
    http: &reqwest::blocking::Client,
    control_url: &str,
    name: &str,
) -> Result<()> {
    let resp = http
        .post(format!("{}/volumes", control_url.trim_end_matches('/')))
        .timeout(Duration::from_secs(60))
        .json(&json!({ "name": name }))
        .send()
        .with_context(|| format!("create volume {name}: request failed"))?;
    let status = resp.status();
    if status.is_success() || status.as_u16() == 409 {
        return Ok(());
    }
    let body = resp.bytes().unwrap_or_default();
    let text = snippet(&body);
    if text.contains("unmanaged") {
        bail!(
            "create volume {name} failed: HTTP {} {text}（该目录已存在但未注册；先删掉残留目录再重试）",
            status.as_u16()
        );
    }
    bail!("create volume {name} failed: HTTP {} {text}", status.as_u16());
}

/// 批量注册卷，并确保卷根存在。
fn ensure_volumes(
    http: &reqwest::blocking::Client,
    control_url: &str,
    volume_host_path: &Path,
    names: &[Option<String>],
) -> Result<()> {
    if names.iter().all(Option::is_none) {
        return Ok(());
    }
    std::fs::create_dir_all(volume_host_path).with_context(|| {
        format!("create volume host path {}", volume_host_path.display())
    })?;
    for name in names.iter().flatten() {
        ensure_volume_with(http, control_url, name)?;
    }
    Ok(())
}

fn is_valid_volume_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}



/// Connect 请求帧：`[0x00][len:4BE][payload]`。
pub(crate) fn encode_connect_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 5);
    out.push(0);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// 解 Connect 响应帧，返回 `(flags, body)`；遇到 `flags & 0x02`（流结束）后停止。
pub(crate) fn decode_connect_frames(raw: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 5 <= raw.len() {
        let flags = raw[i];
        let len = u32::from_be_bytes([raw[i + 1], raw[i + 2], raw[i + 3], raw[i + 4]]) as usize;
        let start = i + 5;
        if start + len > raw.len() {
            break;
        }
        out.push((flags, raw[start..start + len].to_vec()));
        i = start + len;
        if flags & 0x02 != 0 {
            break;
        }
    }
    out
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ParsedExec {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// `None` = 流里没有 end 事件（进程没跑起来 / 连接被掐断）。
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

pub(crate) fn parse_exec_frames(raw: &[u8]) -> ParsedExec {
    let mut out = ParsedExec::default();
    for (flags, body) in decode_connect_frames(raw) {
        if flags & 0x02 != 0 {
            break;
        }
        let Ok(v) = serde_json::from_slice::<Value>(&body) else {
            continue;
        };
        let Some(event) = v.get("event") else {
            continue;
        };
        if let Some(data) = event.get("data") {
            if let Some(s) = data.get("stdout").and_then(|x| x.as_str()) {
                if let Ok(bytes) = BASE64.decode(s) {
                    out.stdout.extend_from_slice(&bytes);
                }
            }
            if let Some(s) = data.get("stderr").and_then(|x| x.as_str()) {
                if let Ok(bytes) = BASE64.decode(s) {
                    out.stderr.extend_from_slice(&bytes);
                }
            }
        }
        if let Some(end) = event.get("end") {
            let code = end.get("exitCode").and_then(|x| x.as_i64()).map(|v| v as i32);
            let status = end.get("status").and_then(|x| x.as_str()).unwrap_or("");
            let error = end.get("error").and_then(|x| x.as_str()).unwrap_or("");
            if error.contains("timed out")
                || error.contains("timeout")
                || status.contains("timed out")
                || status.contains("timeout")
            {
                out.timed_out = true;
            }
            let signaled = status.contains("signal")
                || error.contains("signal")
                || status.contains("killed")
                || error.contains("killed");
            out.exit_code = Some(match code {
                Some(c) => c,
                None if signaled => -1,
                None => 0,
            });
        }
    }
    out
}

/// 解析 `find -printf '%y\0%p\0'` 的输出。
fn parse_find_nul(raw: &[u8]) -> Vec<(char, PathBuf)> {
    let mut parts = raw.split(|b| *b == 0);
    let mut out = Vec::new();
    loop {
        let Some(kind) = parts.next() else { break };
        if kind.is_empty() {
            break;
        }
        let Some(path) = parts.next() else { break };
        if path.is_empty() {
            break;
        }
        let kind = String::from_utf8_lossy(kind).chars().next().unwrap_or('?');
        out.push((kind, PathBuf::from(String::from_utf8_lossy(path).into_owned())));
    }
    out
}

fn cap_bytes(mut data: Vec<u8>, cap: usize) -> (Vec<u8>, bool) {
    if data.len() > cap {
        data.truncate(cap);
        (data, true)
    } else {
        (data, false)
    }
}

fn snippet(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    let clipped: String = trimmed.chars().take(400).collect();
    if trimmed.chars().count() > 400 {
        format!("{clipped}…")
    } else {
        clipped
    }
}

fn path_to_str(path: &Path) -> Result<String> {
    if !path.is_absolute() {
        bail!("remote 环境只接受绝对路径: {}", path.display());
    }
    Ok(path.to_string_lossy().into_owned())
}

fn sh_quote(path: &Path) -> String {
    let s = path.to_string_lossy();
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn urlencode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}", std::process::id(), nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_frame_roundtrip() {
        let payload = br#"{"process":{"cmd":"true"}}"#;
        let framed = encode_connect_frame(payload);
        assert_eq!(framed[0], 0);
        assert_eq!(
            u32::from_be_bytes([framed[1], framed[2], framed[3], framed[4]]) as usize,
            payload.len()
        );
        let frames = decode_connect_frames(&framed);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].1, payload);
    }

    #[test]
    fn decode_stops_at_end_of_stream() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&encode_connect_frame(b"a"));
        raw.push(0x02);
        raw.extend_from_slice(&2u32.to_be_bytes());
        raw.extend_from_slice(b"{}");
        raw.extend_from_slice(&encode_connect_frame(b"b"));
        let frames = decode_connect_frames(&raw);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1].0, 0x02);
    }

    #[test]
    fn decode_ignores_truncated_tail() {
        let mut raw = encode_connect_frame(b"abc");
        raw.extend_from_slice(&[0, 0, 0, 0]); // 只有 4 字节头
        let frames = decode_connect_frames(&raw);
        assert_eq!(frames.len(), 1);
    }

    fn frames_for(events: &[&str]) -> Vec<u8> {
        let mut raw = Vec::new();
        for e in events {
            raw.extend_from_slice(&encode_connect_frame(e.as_bytes()));
        }
        raw.push(0x02);
        raw.extend_from_slice(&2u32.to_be_bytes());
        raw.extend_from_slice(b"{}");
        raw
    }

    #[test]
    fn parse_exec_collects_base64_stdout_and_stderr() {
        // "out\n" / "err\n" 的 base64
        let raw = frames_for(&[
            r#"{"event":{"start":{"pid":13}}}"#,
            r#"{"event":{"data":{"stdout":"b3V0Cg=="}}}"#,
            r#"{"event":{"data":{"stderr":"ZXJyCg=="}}}"#,
            r#"{"event":{"end":{"exitCode":7,"exited":true,"status":"exit status 7","error":"exit status 7"}}}"#,
        ]);
        let parsed = parse_exec_frames(&raw);
        assert_eq!(parsed.stdout, b"out\n");
        assert_eq!(parsed.stderr, b"err\n");
        assert_eq!(parsed.exit_code, Some(7));
        assert!(!parsed.timed_out);
    }

    #[test]
    fn parse_exec_treats_missing_exit_code_as_zero() {
        let raw = frames_for(&[
            r#"{"event":{"start":{"pid":1}}}"#,
            r#"{"event":{"end":{"exited":true,"status":"exit status 0"}}}"#,
        ]);
        assert_eq!(parse_exec_frames(&raw).exit_code, Some(0));
    }

    #[test]
    fn parse_exec_maps_signal_to_minus_one() {
        let raw = frames_for(&[r#"{"event":{"end":{"exited":false,"status":"signal: killed"}}}"#]);
        assert_eq!(parse_exec_frames(&raw).exit_code, Some(-1));
    }

    #[test]
    fn parse_exec_flags_timeout() {
        let raw = frames_for(&[r#"{"event":{"end":{"status":"process timed out"}}}"#]);
        let parsed = parse_exec_frames(&raw);
        assert!(parsed.timed_out);
        assert_eq!(parsed.exit_code, Some(0));
    }

    #[test]
    fn parse_exec_without_end_event_has_no_exit_code() {
        let raw = encode_connect_frame(br#"{"event":{"start":{"pid":1}}}"#);
        assert_eq!(parse_exec_frames(&raw).exit_code, None);
    }

    #[test]
    fn find_output_parses_type_and_path_pairs() {
        let raw = b"d\0/workspace/sub\0f\0/workspace/sub/a.txt\0l\0/workspace/link\0";
        let parsed = parse_find_nul(raw);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0], ('d', PathBuf::from("/workspace/sub")));
        assert_eq!(parsed[1], ('f', PathBuf::from("/workspace/sub/a.txt")));
        assert_eq!(parsed[2], ('l', PathBuf::from("/workspace/link")));
    }

    #[test]
    fn find_output_ignores_empty_tail() {
        assert!(parse_find_nul(b"").is_empty());
        assert_eq!(parse_find_nul(b"f\0/a\0").len(), 1);
    }

    #[test]
    fn cap_bytes_marks_truncation() {
        let (data, truncated) = cap_bytes(b"abcdef".to_vec(), 3);
        assert_eq!(data, b"abc");
        assert!(truncated);
        let (data, truncated) = cap_bytes(b"ab".to_vec(), 3);
        assert_eq!(data, b"ab");
        assert!(!truncated);
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(sh_quote(Path::new("/a b/c")), "'/a b/c'");
        assert_eq!(sh_quote(Path::new("/a'b")), "'/a'\\''b'");
    }

    #[test]
    fn urlencode_keeps_slashes_and_escapes_spaces() {
        assert_eq!(urlencode("/workspace/a b.txt"), "/workspace/a%20b.txt");
    }

    #[test]
    fn relative_paths_are_rejected() {
        assert!(path_to_str(Path::new("relative/x")).is_err());
        assert!(path_to_str(Path::new("/abs/x")).is_ok());
    }

    #[test]
    fn spec_defaults_match_light_layout() {
        let spec = RemoteSpec::default();
        assert_eq!(spec.workspace_root, PathBuf::from("/workspace"));
        assert_eq!(spec.skills_root, PathBuf::from("/skills"));
        assert_eq!(spec.home_root, PathBuf::from("/home/pointer"));
        assert!(spec.mounts.is_empty());
        assert!(spec.volume_host_path.is_none());
    }

    fn mount_pair() -> (Mount, Mount) {
        (
            Mount::new("/data/sb/alice/home", "/home/pointer", false),
            Mount::new("/home/me/.pointer/skills", "/skills/user", true),
        )
    }

    #[test]
    fn create_body_uses_volume_mounts_when_names_present() {
        let (home, skills) = mount_pair();
        let spec = RemoteSpec {
            mounts: vec![home, skills],
            volume_host_path: Some(PathBuf::from("/host/volumes")),
            ..Default::default()
        };
        let names = vec![Some("pv-a".to_string()), Some("pv-b".to_string())];
        let body = build_create_body(&spec, &names);
        let mounts = body["volumeMounts"].as_array().expect("volumeMounts");
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0]["name"], "pv-a");
        assert_eq!(mounts[0]["path"], "/home/pointer");
        assert_eq!(mounts[0]["readOnly"], false);
        assert_eq!(mounts[1]["path"], "/skills/user");
        assert_eq!(mounts[1]["readOnly"], true);
        assert!(body.get("metadata").is_none(), "有卷名就不该再发 host-mount");
    }

    #[test]
    fn create_body_falls_back_to_metadata_host_mount() {
        let (home, skills) = mount_pair();
        let spec = RemoteSpec {
            mounts: vec![home, skills],
            ..Default::default()
        };
        let body = build_create_body(&spec, &[]);
        let raw = body["metadata"]["host-mount"]
            .as_str()
            .expect("host-mount 字符串");
        let parsed: Value = serde_json::from_str(raw).expect("host-mount JSON");
        assert_eq!(parsed.as_array().map(|a| a.len()), Some(2));
        assert_eq!(parsed[1]["readOnly"], true);
        assert!(body.get("volumeMounts").is_none());
    }

    #[test]
    fn create_body_without_mounts_has_neither_field() {
        let spec = RemoteSpec::default();
        let body = build_create_body(&spec, &[]);
        assert!(body.get("volumeMounts").is_none());
        assert!(body.get("metadata").is_none());
    }

    #[test]
    fn volume_names_use_directory_names_under_root() {
        let volumes = tempfile::tempdir().unwrap();
        let home = volumes.path().join("alice-home");
        let skills = volumes.path().join("alice-skills");
        let mounts = vec![
            Mount::new(&home, "/home/pointer", false),
            Mount::new(&skills, "/skills", true),
        ];
        let names = volume_names_for(volumes.path(), &mounts);
        assert_eq!(
            names,
            vec![Some("alice-home".to_string()), Some("alice-skills".to_string())]
        );
    }

    #[test]
    fn volume_names_skip_mounts_outside_root() {
        let volumes = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let mounts = vec![Mount::new(outside.path(), "/skills", true)];
        let names = volume_names_for(volumes.path(), &mounts);
        assert!(
            names.iter().all(Option::is_none),
            "卷根外的宿主目录应被跳过: {names:?}"
        );
    }

    #[test]
    fn volume_names_reject_invalid_names() {
        let volumes = tempfile::tempdir().unwrap();
        let weird = volumes.path().join("bad name!");
        std::fs::create_dir_all(&weird).unwrap();
        let mounts = vec![Mount::new(&weird, "/skills", true)];
        assert!(volume_names_for(volumes.path(), &mounts)
            .iter()
            .all(Option::is_none));
    }

    #[test]
    fn volume_names_without_mounts_is_empty() {
        let volumes = tempfile::tempdir().unwrap();
        assert!(volume_names_for(volumes.path(), &[]).is_empty());
    }

    /// 卷名是**按下标**和挂载配对的。旧实现用 `zip`，过滤掉一个挂载之后
    /// 后面的挂载会被安到前一个卷名上（路径和内容全错），或者整条被丢掉。
    #[test]
    fn create_body_keeps_mounts_aligned_when_some_have_no_volume() {
        let volumes = tempfile::tempdir().unwrap();
        let inside = volumes.path().join("alice-home");
        let outside = tempfile::tempdir().unwrap();
        let mounts = vec![
            Mount::new(outside.path(), "/workspace", false),
            Mount::new(&inside, "/home/pointer", false),
        ];

        let names = volume_names_for(volumes.path(), &mounts);
        assert_eq!(names.len(), mounts.len(), "卷名必须与挂载等长");
        assert!(names[0].is_none(), "卷根外的挂载没有卷");
        assert_eq!(names[1].as_deref(), Some("alice-home"));

        let spec = RemoteSpec {
            mounts,
            volume_host_path: Some(volumes.path().to_path_buf()),
            ..Default::default()
        };
        let body = build_create_body(&spec, &names);
        let sent = body["volumeMounts"].as_array().expect("volumeMounts");
        assert_eq!(sent.len(), 1, "只发有卷的挂载");
        assert_eq!(sent[0]["path"], "/home/pointer", "不能错位成 /workspace");
        assert_eq!(sent[0]["name"], "alice-home");
    }

    #[test]
    fn read_connect_frame_parses_frames() {
        let mut raw = Vec::new();
        raw.extend(encode_connect_frame(
            br#"{"event":{"start":{"process":{"pid":7}}}}"#,
        ));
        raw.extend(encode_connect_frame(br#"{"event":{"end":{"exitCode":3}}}"#));
        let mut cur = std::io::Cursor::new(raw);
        let (flags, body) = read_connect_frame(&mut cur).unwrap().expect("frame 1");
        assert_eq!(flags, 0);
        assert!(String::from_utf8_lossy(&body).contains("\"pid\":7"));
        let (_, body2) = read_connect_frame(&mut cur).unwrap().expect("frame 2");
        assert!(String::from_utf8_lossy(&body2).contains("exitCode"));
        assert!(read_connect_frame(&mut cur).unwrap().is_none(), "EOF 返回 None");
    }
}
