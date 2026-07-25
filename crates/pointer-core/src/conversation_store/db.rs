//! SQLite connection tuning aligned with Hermes `SessionDB` (WAL, write retry).

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use rand::Rng;
use rusqlite::Connection;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

pub const WRITE_MAX_RETRIES: u32 = 15;
pub const WRITE_RETRY_MIN_MS: u64 = 20;
pub const WRITE_RETRY_MAX_MS: u64 = 150;
pub const CHECKPOINT_EVERY_N_WRITES: u32 = 50;

pub struct DbHandle {
    pub conn: Mutex<Connection>,
    write_count: AtomicU32,
}

impl DbHandle {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create conversation db dir {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("open conversation db {}", path.display()))?;
        apply_wal_with_fallback(&conn, "conversations.db")?;
        conn.execute_batch(
            "PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
            write_count: AtomicU32::new(0),
        })
    }

    pub fn execute_write<F, T>(&self, op: F) -> Result<T>
    where
        F: Fn(&Connection) -> Result<T>,
    {
        let mut last_err = None;
        for attempt in 0..WRITE_MAX_RETRIES {
            match self.try_write(&op) {
                Ok(value) => {
                    let n = self.write_count.fetch_add(1, Ordering::Relaxed) + 1;
                    if n % CHECKPOINT_EVERY_N_WRITES == 0 {
                        self.try_wal_checkpoint();
                    }
                    return Ok(value);
                }
                Err(e) => {
                    if is_lock_error_str(&e.to_string()) && attempt + 1 < WRITE_MAX_RETRIES {
                        last_err = Some(e);
                        let sleep_ms =
                            rand::thread_rng().gen_range(WRITE_RETRY_MIN_MS..=WRITE_RETRY_MAX_MS);
                        thread::sleep(Duration::from_millis(sleep_ms));
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow!("database is locked after max retries")))
    }

    fn try_write<F, T>(&self, op: &F) -> Result<T>
    where
        F: Fn(&Connection) -> Result<T>,
    {
        let conn = self.conn.lock();
        conn.execute_batch("BEGIN IMMEDIATE")?;
        match op(&conn) {
            Ok(value) => {
                conn.execute_batch("COMMIT")?;
                Ok(value)
            }
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }

    fn try_wal_checkpoint(&self) {
        let conn = self.conn.lock();
        if let Ok(row) = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get::<_, i32>(1)?, r.get::<_, i32>(2)?))
        }) {
            if row.0 > 0 {
                log::debug!(
                    "conversation_store: WAL checkpoint {}/{} pages",
                    row.1,
                    row.0
                );
            }
        }
    }
}

fn is_lock_error_str(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("locked") || lower.contains("busy")
}

fn apply_wal_with_fallback(conn: &Connection, label: &str) -> Result<()> {
    match conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0)) {
        Ok(mode) if mode.eq_ignore_ascii_case("wal") => Ok(()),
        Ok(_) => Ok(()),
        Err(err) => {
            let msg = err.to_string().to_ascii_lowercase();
            if msg.contains("disk i/o")
                || msg.contains("readonly")
                || msg.contains("unable to open")
                || msg.contains("not supported")
            {
                log::warn!("{label}: WAL unavailable ({err}); falling back to journal_mode=DELETE");
                conn.execute_batch("PRAGMA journal_mode=DELETE")?;
                Ok(())
            } else {
                Err(err.into())
            }
        }
    }
}
