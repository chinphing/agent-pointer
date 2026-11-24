//! 媒体子进程统一入口（§3.3：agent 可触发的进程一律在环境内跑）。
//!
//! 设计文档 §7.2 债务表里 `media/*` 是"沙箱内"归属：ffmpeg / ffprobe / pdfinfo
//! 必须走 `Environment::exec`，这样 headless 模式下它们跑在沙箱里，而不是宿主。
//!
//! 两个配套抽象：
//! - [`run`] / [`run_ok`]：跑一次工具，超时与输出上限统一；
//! - [`StagedFile`]：临时输入/输出文件。local 用宿主临时文件（`NamedTempFile` 保活），
//!   remote 用沙箱 `/tmp`，读写都走 `Environment`，所以调用方不用关心在哪边。
//!
//! **执行环境一律由调用方显式传入**（`&Arc<dyn Environment>`），模块内不再自己查
//! `effective_env()`：媒体管线常跑在 `spawn_blocking` / 后台任务里，线程上未必有绑定，
//! 兜底出来的环境可能是错的（`DeniedEnv`，或本该进沙箱却用了宿主）。环境是这次操作的
//! 必要输入，和文件路径同级。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::env::{Environment, ExecOptions, ExecOutput};

/// 单个媒体工具调用的上限。转码/抽帧可能很慢，给足余量。
const MEDIA_TIMEOUT: Duration = Duration::from_secs(1800);
/// 媒体工具的输出只用来报错，1 MiB 足够。
const MEDIA_MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// 单个媒体产物（一帧 JPEG / 一页 PNG）的体积上限。
///
/// 依据 Codex 的教训：通用 exec 的输出上限（他们 1 MiB）不能套在媒体产物上，
/// 否则大图被静默截断成"损坏"。这里单独设 6 MiB，超过就报错而不是截断。
pub const MEDIA_ARTIFACT_MAX_BYTES: u64 = 6 * 1024 * 1024;

/// 当前环境是否把进程放到远端（headless）跑。
pub fn is_remote(env: &Arc<dyn Environment>) -> bool {
    env.kind() != crate::env::EnvKind::Local
}

/// 跑一次媒体工具；不检查退出码（调用方自己判）。
pub fn run(env: &Arc<dyn Environment>, program: &str, args: &[String]) -> Result<ExecOutput> {
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push(program.to_string());
    argv.extend(args.iter().cloned());
    env.exec(
        &argv,
        ExecOptions::new()
            .with_timeout(MEDIA_TIMEOUT)
            .with_max_output_bytes(MEDIA_MAX_OUTPUT_BYTES),
    )
    .with_context(|| format!("run {program}"))
}

/// 跑一次媒体工具；非 0 退出直接报错（带 stderr）。
pub fn run_ok(env: &Arc<dyn Environment>, program: &str, args: &[String]) -> Result<ExecOutput> {
    let out = run(env, program, args)?;
    if !out.success() {
        anyhow::bail!(
            "{program} failed (status {}): {}",
            out.status,
            out.stderr_str().trim()
        );
    }
    Ok(out)
}

/// 媒体输入：要么文件已经在环境里（走路径，零搬运），要么只有字节（临时落盘）。
///
/// 路径优先是设计文档 §7.2 的要求：大文件不该整段穿过环境边界进宿主内存。
pub enum MediaInput {
    /// 文件已在环境里（沙箱 `/data/...` 或宿主路径）。
    Existing(PathBuf),
    /// 只有字节，需要临时落盘。
    Staged(StagedFile),
}

impl MediaInput {
    pub fn existing(path: impl Into<PathBuf>) -> Self {
        Self::Existing(path.into())
    }

    pub fn from_bytes(env: &Arc<dyn Environment>, bytes: &[u8], suffix: &str) -> Result<Self> {
        Ok(Self::Staged(StagedFile::write(env, bytes, suffix)?))
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::Existing(p) => p,
            Self::Staged(f) => f.path(),
        }
    }

    /// 命令行参数用的字符串形式。
    pub fn arg(&self) -> &str {
        self.path().to_str().unwrap_or_default()
    }

    pub fn is_staged(&self) -> bool {
        matches!(self, Self::Staged(_))
    }

    /// 输入体积（字节）。
    pub fn len_bytes(&self, env: &Arc<dyn Environment>) -> Result<u64> {
        env.metadata(self.path())
            .map(|m| m.len)
            .with_context(|| format!("stat media input {}", self.path().display()))
    }

    /// 读回整个输入的字节（只在确认体积可控时用）。
    pub fn read(&self, env: &Arc<dyn Environment>) -> Result<Vec<u8>> {
        match self {
            Self::Existing(p) => env
                .read_file(p)
                .with_context(|| format!("read media input {}", p.display())),
            Self::Staged(f) => f.read(),
        }
    }
}

/// 暂存文件：local 是宿主临时文件，remote 是沙箱 `/tmp` 下的路径。
pub struct StagedFile {
    path: PathBuf,
    /// local 分支保活用；remote 为 `None`。
    _host: Option<tempfile::NamedTempFile>,
    /// remote 分支专用：读写与 `drop` 回收都要用它（`drop` 拿不到调用方的入参，
    /// 所以这里持有环境本身）。local 为 `None`，走宿主文件系统。
    env: Option<Arc<dyn Environment>>,
}

impl StagedFile {
    /// 写入字节（媒体输入）。
    pub fn write(env: &Arc<dyn Environment>, bytes: &[u8], suffix: &str) -> Result<Self> {
        if is_remote(env) {
            let path = remote_temp_path(suffix);
            env.write_file(&path, bytes)
                .with_context(|| format!("stage input {}", path.display()))?;
            Ok(Self {
                path,
                _host: None,
                env: Some(env.clone()),
            })
        } else {
            use std::io::Write;
            let mut file = tempfile::Builder::new()
                .suffix(suffix)
                .tempfile()
                .context("media temp input")?;
            file.write_all(bytes).context("write media temp input")?;
            file.flush().context("flush media temp input")?;
            let path = file.path().to_path_buf();
            Ok(Self {
                path,
                _host: Some(file),
                env: None,
            })
        }
    }

    /// 只占一个路径（给工具写输出用）。
    pub fn empty(env: &Arc<dyn Environment>, suffix: &str) -> Result<Self> {
        if is_remote(env) {
            Ok(Self {
                path: remote_temp_path(suffix),
                _host: None,
                env: Some(env.clone()),
            })
        } else {
            let file = tempfile::Builder::new()
                .suffix(suffix)
                .tempfile()
                .context("media temp output")?;
            let path = file.path().to_path_buf();
            Ok(Self {
                path,
                _host: Some(file),
                env: None,
            })
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 给命令行参数用的字符串形式。
    pub fn arg(&self) -> &str {
        self.path.to_str().unwrap_or_default()
    }

    /// 读回工具产出的字节。
    pub fn read(&self) -> Result<Vec<u8>> {
        if let Some(env) = &self.env {
            env.read_file(&self.path)
                .with_context(|| format!("read staged output {}", self.path.display()))
        } else {
            std::fs::read(&self.path)
                .with_context(|| format!("read staged output {}", self.path.display()))
        }
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if let Some(env) = &self.env {
            let _ = env.remove_file(&self.path);
        }
        // local：NamedTempFile 自己删
    }
}

fn remote_temp_path(suffix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    PathBuf::from(format!(
        "/tmp/pointer-media-{}-{nanos}{suffix}",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的宿主环境：local 档位，临时文件走宿主临时目录。
    fn host() -> Arc<dyn Environment> {
        crate::env::host_env()
    }

    #[test]
    fn local_staged_file_roundtrips() {
        let staged = StagedFile::write(&host(), b"hello", ".txt").expect("write");
        assert!(staged.path().is_absolute());
        assert_eq!(staged.read().expect("read"), b"hello");
    }

    #[test]
    fn local_empty_staged_file_exists() {
        let staged = StagedFile::empty(&host(), ".bin").expect("empty");
        assert!(staged.path().exists());
        assert!(staged.read().expect("read").is_empty());
    }

    #[test]
    fn staged_file_is_removed_on_drop() {
        let path = {
            let staged = StagedFile::write(&host(), b"x", ".bin").expect("write");
            staged.path().to_path_buf()
        };
        assert!(!path.exists(), "临时文件应在 drop 时删除");
    }

    #[test]
    fn remote_temp_path_is_under_tmp() {
        let p = remote_temp_path(".mp4");
        assert!(p.starts_with("/tmp/"));
        assert!(p.to_string_lossy().ends_with(".mp4"));
    }

    #[test]
    fn media_input_existing_keeps_path() {
        let input = MediaInput::existing("/data/workspaces/a.mp4");
        assert_eq!(input.path(), Path::new("/data/workspaces/a.mp4"));
        assert_eq!(input.arg(), "/data/workspaces/a.mp4");
        assert!(!input.is_staged());
    }

    #[test]
    fn media_input_from_bytes_stages_and_cleans_up() {
        let path = {
            let input = MediaInput::from_bytes(&host(), b"abc", ".mp4").expect("stage");
            assert!(input.is_staged());
            assert!(input.path().exists());
            input.path().to_path_buf()
        };
        assert!(!path.exists(), "staged 输入应在 drop 时删除");
    }

    #[test]
    fn local_env_reports_not_remote() {
        assert!(!is_remote(&host()));
    }

    /// 记录型环境替身：`exec` 只记 argv，文件操作落到宿主临时目录。
    ///
    /// 用来断言「环境是显式传进来的那一个」——命令与路径都由调用方给的环境决定。
    struct RecordingEnv {
        workspace: PathBuf,
        map: crate::env::PathMap,
        exec_calls: std::sync::Mutex<Vec<Vec<String>>>,
        written: std::sync::Mutex<Vec<PathBuf>>,
        removed: std::sync::Mutex<Vec<PathBuf>>,
    }

    impl RecordingEnv {
        fn new() -> Self {
            Self {
                workspace: PathBuf::from("/data/workspaces"),
                map: crate::env::PathMap::identity(),
                exec_calls: std::sync::Mutex::new(Vec::new()),
                written: std::sync::Mutex::new(Vec::new()),
                removed: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn exec_calls(&self) -> Vec<Vec<String>> {
            self.exec_calls.lock().expect("exec_calls").clone()
        }

        fn written(&self) -> Vec<PathBuf> {
            self.written.lock().expect("written").clone()
        }

        fn removed(&self) -> Vec<PathBuf> {
            self.removed.lock().expect("removed").clone()
        }
    }

    impl crate::env::Environment for RecordingEnv {
        fn kind(&self) -> crate::env::EnvKind {
            crate::env::EnvKind::Headless
        }
        fn workspace_root(&self) -> &Path {
            &self.workspace
        }
        fn skills_root(&self) -> &Path {
            Path::new("/skills")
        }
        fn home_root(&self) -> &Path {
            Path::new("/home/pointer")
        }
        fn capabilities(&self) -> crate::env::Capabilities {
            crate::env::Capabilities::headless()
        }
        fn path_map(&self) -> &crate::env::PathMap {
            &self.map
        }
        fn exec(&self, argv: &[String], _opts: ExecOptions) -> Result<ExecOutput> {
            self.exec_calls
                .lock()
                .expect("exec_calls")
                .push(argv.to_vec());
            Ok(ExecOutput {
                status: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                timed_out: false,
            })
        }
        fn spawn(
            &self,
            _argv: &[String],
            _opts: crate::env::SpawnOptions,
        ) -> Result<Box<dyn crate::env::ChildHandle>> {
            anyhow::bail!("recording env: spawn not supported")
        }
        fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
            Ok(std::fs::read(path)?)
        }
        fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
            self.written
                .lock()
                .expect("written")
                .push(path.to_path_buf());
            std::fs::write(path, data)?;
            Ok(())
        }
        fn list_dir(&self, _path: &Path) -> Result<Vec<crate::env::DirEntry>> {
            Ok(Vec::new())
        }
        fn metadata(&self, path: &Path) -> Result<crate::env::FileMetadata> {
            let meta = std::fs::metadata(path)?;
            Ok(crate::env::FileMetadata {
                is_file: meta.is_file(),
                is_dir: meta.is_dir(),
                is_symlink: meta.file_type().is_symlink(),
                len: meta.len(),
            })
        }
        fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
            Ok(path.to_path_buf())
        }
        fn create_dir_all(&self, path: &Path) -> Result<()> {
            std::fs::create_dir_all(path)?;
            Ok(())
        }
        fn read_to_string(&self, path: &Path) -> Result<String> {
            Ok(std::fs::read_to_string(path)?)
        }
        fn open_reader(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>> {
            Ok(Box::new(std::fs::File::open(path)?))
        }
        fn walk(
            &self,
            _root: &Path,
            _opts: crate::env::WalkOptions,
        ) -> Result<Vec<crate::env::WalkEntry>> {
            Ok(Vec::new())
        }
        fn remove_file(&self, path: &Path) -> Result<()> {
            self.removed
                .lock()
                .expect("removed")
                .push(path.to_path_buf());
            let _ = std::fs::remove_file(path);
            Ok(())
        }
        fn remove_dir_all(&self, path: &Path) -> Result<()> {
            let _ = std::fs::remove_dir_all(path);
            Ok(())
        }
        fn set_executable(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn run_uses_the_env_passed_in() {
        let recording = Arc::new(RecordingEnv::new());
        let env: Arc<dyn Environment> = recording.clone();
        assert!(is_remote(&env), "记录型替身是 headless，应走环境内执行");

        let staged = StagedFile::write(&env, b"media", ".mp4").expect("stage input");
        assert!(
            staged.path().to_string_lossy().starts_with("/tmp/pointer-media-"),
            "headless 下暂存必须落在环境内路径，实际 {}",
            staged.path().display()
        );
        assert_eq!(
            recording.written(),
            vec![staged.path().to_path_buf()],
            "落盘必须经过调用方传入的环境"
        );

        let args = vec!["-i".to_string(), staged.arg().to_string()];
        let out = run(&env, "ffmpeg", &args).expect("run");
        assert!(out.success());

        let calls = recording.exec_calls();
        assert_eq!(calls.len(), 1, "只应产生一次 exec");
        assert_eq!(
            calls[0],
            vec![
                "ffmpeg".to_string(),
                "-i".to_string(),
                staged.path().display().to_string()
            ],
            "exec 收到的命令与路径必须来自调用方给的环境"
        );
        assert_eq!(staged.read().expect("read staged"), b"media");

        let staged_path = staged.path().to_path_buf();
        drop(staged);
        assert_eq!(
            recording.removed(),
            vec![staged_path],
            "drop 时用同一个环境回收临时文件"
        );
    }

    #[test]
    fn media_input_len_bytes_uses_the_env_passed_in() {
        let env: Arc<dyn Environment> = Arc::new(RecordingEnv::new());
        let path = std::env::temp_dir().join(format!(
            "pointer-media-len-{}-{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, b"0123456789").expect("write fixture");
        let input = MediaInput::existing(path.clone());
        assert_eq!(input.len_bytes(&env).expect("len"), 10);
        assert_eq!(input.read(&env).expect("read"), b"0123456789");
        let _ = std::fs::remove_file(&path);
    }

    /// 架构守卫：媒体执行层不得再自己查执行环境。
    ///
    /// `effective_env()` 在未绑定线程上会按部署档位兜底（沙箱档位下是 `DeniedEnv`，
    /// local 档位下是宿主），而媒体管线常跑在 `spawn_blocking` / 后台任务里——兜底出来
    /// 的环境可能是错的（沙箱里的文件用宿主 ffmpeg 去读必然失败）。环境是这次操作的
    /// 必要输入，必须由调用方显式传入；谁再写回来自查就红。
    #[test]
    fn media_exec_layer_takes_env_explicitly() {
        let needle = concat!("effective", "_env");
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/media");
        let mut violations = Vec::new();
        for file in ["exec.rs", "ffmpeg.rs"] {
            let path = root.join(file);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            // 只看生产代码：`#[cfg(test)]` 之后是测试，允许提到这个名字
            let prod = match text.find("#[cfg(test)]") {
                Some(i) => &text[..i],
                None => &text[..],
            };
            for (idx, line) in prod.lines().enumerate() {
                // 注释里提到 API 名字不算违规
                if line.trim_start().starts_with("//") {
                    continue;
                }
                if line.contains(needle) {
                    violations.push(format!("media/{file}:{}", idx + 1));
                }
            }
        }
        assert!(
            violations.is_empty(),
            "媒体执行层不得自己取执行环境（`{needle}()`）：改为由调用方传入 env\n{}",
            violations.join("\n")
        );
    }
}
