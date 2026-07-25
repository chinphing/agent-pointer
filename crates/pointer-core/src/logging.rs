//! 运行时日志：按日轮转写入文件，并镜像到 stderr（`RUST_LOG` 语法与 env_logger 类似）。
//!
//! 所有经 `log` 宏输出的行（含 `task_board_obs:`）使用统一前缀：`[本地时间] [LEVEL] target - message`。

use flexi_logger::{Age, Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use std::backtrace::Backtrace;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Once, OnceLock};

/// Shared timestamp layout for file, stderr (flexi_logger), and env_logger fallback.
pub const LOG_TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.3f";

static LOGGER_HANDLE: OnceLock<flexi_logger::LoggerHandle> = OnceLock::new();
static BACKTRACE_ENV: Once = Once::new();
static PANIC_HOOK: Once = Once::new();

fn local_timestamp() -> String {
    chrono::Local::now().format(LOG_TIMESTAMP_FORMAT).to_string()
}

/// flexi_logger: `[2026-05-21 12:34:56.789] [INFO ] pointer_core::... - …`
pub fn unified_log_format(
    w: &mut dyn Write,
    _now: &mut flexi_logger::DeferredNow,
    record: &log::Record,
) -> Result<(), std::io::Error> {
    writeln!(
        w,
        "[{}] [{:5}] {} - {}",
        local_timestamp(),
        record.level(),
        record.target(),
        record.args()
    )
}

fn env_logger_unified_format(
    buf: &mut env_logger::fmt::Formatter,
    record: &log::Record,
) -> std::io::Result<()> {
    writeln!(
        buf,
        "[{}] [{:5}] {} - {}",
        local_timestamp(),
        record.level(),
        record.target(),
        record.args()
    )
}

/// stderr-only fallback when file logging cannot start (same timestamp layout as [`init_runtime_logging`]).
pub fn init_stderr_only_logging(default_filter: &str) {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(default_filter),
    )
    .format(env_logger_unified_format)
    .try_init();
}

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

/// Human-readable text from a panic payload (`catch_unwind` / panic hook).
pub fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "(non-string panic payload)".into()
    }
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
            let msg = panic_payload_message(info.payload());
            let bt = Backtrace::capture();
            log::error!("thread panicked at {loc}: {msg}\nBacktrace:\n{bt}");
            eprintln!(
                "[{}] [{:5}] panic - thread panicked at {loc}: {msg}\nBacktrace:\n{bt}",
                local_timestamp(),
                log::Level::Error
            );
            default_hook(info);
        }));
    });
}

/// Whether to log chat internals: model id, reasoning/output text, tool names/args, stream bodies.
///
/// - **Release** (`not(debug_assertions)`): only when settings `debugMenusEnabled` is true.
/// - **Dev cargo build**: on unless `POINTER_INTERNAL_RUNTIME_LOG=0`.
pub fn internal_runtime_log_enabled() -> bool {
    if crate::platform_config::effective_settings_global().debug_menus_enabled {
        return true;
    }
    #[cfg(debug_assertions)]
    {
        match std::env::var("POINTER_INTERNAL_RUNTIME_LOG") {
            Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
            _ => true,
        }
    }
    #[cfg(not(debug_assertions))]
    {
        false
    }
}

/// Same gate as decision stream raw output (`provider.rs` `raw_llm_stream_to_console_enabled`).
pub fn raw_llm_console_segments_enabled() -> bool {
    if !internal_runtime_log_enabled() {
        return false;
    }
    match std::env::var("POINTER_STREAM_RAW_LLM_TO_STDOUT") {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
        _ => true,
    }
}

/// Non-streaming pipeline LLM (Position / Verify): print reasoning + output to stderr
/// with the same lane markers as decision streaming.
pub fn write_pipeline_llm_segments_to_stderr(
    scope: &str,
    reasoning: Option<&str>,
    output: &str,
) {
    if !raw_llm_console_segments_enabled() {
        return;
    }
    let reasoning_text = reasoning
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let output_text = output.trim();
    if reasoning_text.is_empty() && output_text.is_empty() {
        return;
    }
    let mut err = std::io::stderr().lock();
    let _ = std::io::Write::write_all(&mut err, format!("\n[pipeline_llm scope={scope}]\n").as_bytes());
    if !reasoning_text.is_empty() {
        let _ = std::io::Write::write_all(&mut err, "[推理|reasoning]\n".as_bytes());
        let _ = std::io::Write::write_all(&mut err, reasoning_text.as_bytes());
        let _ = std::io::Write::write_all(&mut err, b"\n");
    }
    if !output_text.is_empty() {
        let _ = std::io::Write::write_all(&mut err, "[输出|output]\n".as_bytes());
        let _ = std::io::Write::write_all(&mut err, output_text.as_bytes());
        let _ = std::io::Write::write_all(&mut err, b"\n");
    }
    let _ = err.flush();
}

/// Default `RUST_LOG` filter for desktop / dev when the env var is unset.
pub fn default_runtime_log_filter() -> &'static str {
    if internal_runtime_log_enabled() {
        "warn,pointer_core=info,pointer_core::provider=debug,pointer_app_lib=info,pointer_channels=info"
    } else {
        "warn,pointer_core=info,pointer_app_lib=info,pointer_channels=info"
    }
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
        .format(unified_log_format)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_default_filter_omits_provider_debug() {
        #[cfg(not(debug_assertions))]
        {
            assert!(
                !default_runtime_log_filter().contains("provider=debug"),
                "release default filter must not enable provider debug: {}",
                default_runtime_log_filter()
            );
        }
    }
}
