//! Statically linked sqlite-cjk-fts extension (`cjk_bigram` tokenizer).
//!
//! Registered via `sqlite3_auto_extension` before opening connections so we avoid
//! runtime `load_extension` / DLL extraction (fragile on Windows).

use anyhow::{Context, Result};
use std::sync::OnceLock;

static REGISTERED: OnceLock<Result<(), &'static str>> = OnceLock::new();

extern "C" {
    fn sqlite3_cjkfts_init(
        db: *mut libsqlite3_sys::sqlite3,
        pz_err_msg: *mut *mut std::ffi::c_char,
        p_api: *const libsqlite3_sys::sqlite3_api_routines,
    ) -> std::ffi::c_int;
}

/// Must run before the first `sqlite3_open` / `Connection::open` in this process.
pub fn ensure_registered() -> Result<()> {
    let res = REGISTERED.get_or_init(|| unsafe {
        let rc = libsqlite3_sys::sqlite3_auto_extension(Some(sqlite3_cjkfts_init));
        if rc != libsqlite3_sys::SQLITE_OK {
            log::warn!("conversation_store: sqlite3_auto_extension(cjk_bigram) failed rc={rc}");
            return Err("sqlite3_auto_extension(cjk_bigram) failed");
        }
        log::info!("conversation_store: registered cjk_bigram tokenizer (static)");
        Ok(())
    });
    res.map_err(|e| anyhow::anyhow!("{e}"))
        .context("register cjk fts extension")
}
