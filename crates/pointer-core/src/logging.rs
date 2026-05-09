//! 运行时日志：按日轮转写入文件，并镜像到 stderr（`RUST_LOG` 语法与 env_logger 类似）。

use flexi_logger::{Age, Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use std::backtrace::Backtrace;
use std::path::{Path, PathBuf};
use std::sync::{Once, OnceLock};

static LOGGER_HANDLE: OnceLock<flexi_logger::LoggerHandle> = OnceLock::new();
static BACKTRACE_ENV: Once = Once::new();
static PANIC_HOOK: Once = Once::new();

/// 若未设置环境变量，则启用 **全量** panic 栈（`RUST_BACKTRACE=full`）与库错误栈（`RUST_LIB_BACKTRACE=1`）。
pub fn init_backtrace_defaults() {
    BACKTRACE_ENV.call_once(|| {
        if std::env::var_os("RUST_BACKTRACE").is_none() {
            std::env::set_var("RUST_BACKTRACE", "full");
        }
        if std::env::var_os("RUST_LIB_BACKTRACE").is_none() {
            std::env::set_var("RUST_LIB_BACKTRACE", "1");
        }
    });
}

/// 在 logger 可用后调用：panic 时写入 `log`、stderr，并保留默认 hook。
pub fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let loc = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "?".into());
            let payload = info.payload();
            let msg = if let Some(s) = payload.downcast_ref::<&'static str>() {
                (*s).to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "(non-string panic payload)".into()
            };
            let bt = Backtrace::capture();
            log::error!("thread panicked at {loc}: {msg}\nBacktrace:\n{bt}");
            eprintln!("thread panicked at {loc}: {msg}\nBacktrace:\n{bt}");
            default_hook(info);
        }));
    });
}

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
    init_backtrace_defaults();

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
    install_panic_hook();
    Ok(())
}
