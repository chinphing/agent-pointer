//! 媒体子进程统一入口（§3.3：agent 可触发的进程一律在环境内跑）。
//!
//! 设计文档 §7.2 债务表里 `media/*` 是"沙箱内"归属：ffmpeg / ffprobe / pdfinfo
//! 必须走 `Environment::exec`，这样 headless 模式下它们跑在沙箱里，而不是宿主。
//!
//! 两个配套抽象：
//! - [`run`] / [`run_ok`]：跑一次工具，超时与输出上限统一；
//! - [`StagedFile`]：临时输入/输出文件。local 用宿主临时文件（`NamedTempFile` 保活），
//!   remote 用沙箱 `/tmp`，读写都走 `Environment`，所以调用方不用关心在哪边。

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::env::{effective_env, ExecOptions, ExecOutput};

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
pub fn is_remote() -> bool {
    effective_env().kind() != crate::env::EnvKind::Local
}

/// 跑一次媒体工具；不检查退出码（调用方自己判）。
pub fn run(program: &str, args: &[String]) -> Result<ExecOutput> {
    let env = effective_env();
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
pub fn run_ok(program: &str, args: &[String]) -> Result<ExecOutput> {
    let out = run(program, args)?;
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
    /// 文件已在环境里（沙箱 `/workspace/x.mp4` 或宿主路径）。
    Existing(PathBuf),
    /// 只有字节，需要临时落盘。
    Staged(StagedFile),
}

impl MediaInput {
    pub fn existing(path: impl Into<PathBuf>) -> Self {
        Self::Existing(path.into())
    }

    pub fn from_bytes(bytes: &[u8], suffix: &str) -> Result<Self> {
        Ok(Self::Staged(StagedFile::write(bytes, suffix)?))
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
    pub fn len_bytes(&self) -> Result<u64> {
        effective_env()
            .metadata(self.path())
            .map(|m| m.len)
            .with_context(|| format!("stat media input {}", self.path().display()))
    }

    /// 读回整个输入的字节（只在确认体积可控时用）。
    pub fn read(&self) -> Result<Vec<u8>> {
        match self {
            Self::Existing(p) => effective_env()
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
    remote: bool,
}

impl StagedFile {
    /// 写入字节（媒体输入）。
    pub fn write(bytes: &[u8], suffix: &str) -> Result<Self> {
        if is_remote() {
            let path = remote_temp_path(suffix);
            effective_env()
                .write_file(&path, bytes)
                .with_context(|| format!("stage input {}", path.display()))?;
            Ok(Self {
                path,
                _host: None,
                remote: true,
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
                remote: false,
            })
        }
    }

    /// 只占一个路径（给工具写输出用）。
    pub fn empty(suffix: &str) -> Result<Self> {
        if is_remote() {
            Ok(Self {
                path: remote_temp_path(suffix),
                _host: None,
                remote: true,
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
                remote: false,
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
        if self.remote {
            effective_env()
                .read_file(&self.path)
                .with_context(|| format!("read staged output {}", self.path.display()))
        } else {
            std::fs::read(&self.path)
                .with_context(|| format!("read staged output {}", self.path.display()))
        }
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if self.remote {
            let _ = effective_env().remove_file(&self.path);
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

    #[test]
    fn local_staged_file_roundtrips() {
        let staged = StagedFile::write(b"hello", ".txt").expect("write");
        assert!(staged.path().is_absolute());
        assert_eq!(staged.read().expect("read"), b"hello");
    }

    #[test]
    fn local_empty_staged_file_exists() {
        let staged = StagedFile::empty(".bin").expect("empty");
        assert!(staged.path().exists());
        assert!(staged.read().expect("read").is_empty());
    }

    #[test]
    fn staged_file_is_removed_on_drop() {
        let path = {
            let staged = StagedFile::write(b"x", ".bin").expect("write");
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
        let input = MediaInput::existing("/workspace/a.mp4");
        assert_eq!(input.path(), Path::new("/workspace/a.mp4"));
        assert_eq!(input.arg(), "/workspace/a.mp4");
        assert!(!input.is_staged());
    }

    #[test]
    fn media_input_from_bytes_stages_and_cleans_up() {
        let path = {
            let input = MediaInput::from_bytes(b"abc", ".mp4").expect("stage");
            assert!(input.is_staged());
            assert!(input.path().exists());
            input.path().to_path_buf()
        };
        assert!(!path.exists(), "staged 输入应在 drop 时删除");
    }

    #[test]
    fn local_env_reports_not_remote() {
        // 单元测试里没有绑定 EnvironmentGuard，走进程级 local
        assert!(!is_remote());
    }
}
