//! 运行时日志：按日轮转写入文件，并镜像到 stderr（`RUST_LOG` 语法与 env_logger 类似）。

use flexi_logger::{Age, Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static LOGGER_HANDLE: OnceLock<flexi_logger::LoggerHandle> = OnceLock::new();

/// 桌面端：与 [`crate::storage::app_data_dir`] 一致的数据目录下的 `logs`。
pub fn desktop_log_dir() -> PathBuf {
    crate::storage::app_data_dir()
        .map(|d| d.join("logs"))
        .unwrap_or_else(|_| std::env::temp_dir().join("PointerApp").join("logs"))
}

/// 在 `log_dir` 下写入 `pointer_*.log`（按日轮转，保留 7 个文件），并 **`duplicate` 到 stderr**。
/// 若已初始化过，直接返回 `Ok(())`。
///
/// 优先使用环境变量 `RUST_LOG`；未设置时使用 `default_filter`（例如 `warn,pointer_core=info`）。
pub fn init_runtime_logging(log_dir: &Path, default_filter: &str) -> Result<(), String> {
    if LOGGER_HANDLE.get().is_some() {
        return Ok(());
    }

    std::fs::create_dir_all(log_dir)
        .map_err(|e| format!("create log dir {}: {e}", log_dir.display()))?;

    let handle = Logger::try_with_env_or_str(default_filter)
        .map_err(|e| format!("log filter / RUST_LOG: {e}"))?
        .log_to_file(
            FileSpec::default()
                .directory(log_dir)
                .basename("pointer")
                .suffix("log"),
        )
        .rotate(
            Criterion::Age(Age::Day),
            Naming::Timestamps,
            Cleanup::KeepLogFiles(7),
        )
        .duplicate_to_stderr(Duplicate::All)
        .start()
        .map_err(|e| format!("flexi_logger: {e}"))?;

    LOGGER_HANDLE
        .set(handle)
        .map_err(|_| "logger handle storage failed".to_string())?;

    log::info!("runtime log directory: {}", log_dir.display());
    Ok(())
}
