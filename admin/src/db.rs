//! `admin.db` 的连接、PRAGMA 与通用工具。

use anyhow::{Context, Result};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::schema;

/// 控制面数据库句柄。单实例、低并发，一把锁足够。
pub struct AdminStore {
    conn: Mutex<Connection>,
}

impl AdminStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("create admin data dir {}", dir.display()))?;
            }
        }
        let conn =
            Connection::open(path).with_context(|| format!("open admin db {}", path.display()))?;
        init_conn(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        init_conn(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 在锁内执行一段操作。
    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("admin db lock poisoned"))?;
        f(&guard)
    }
}

fn init_conn(conn: &Connection) -> Result<()> {
    // WAL 让读写互不阻塞；内存库不支持，失败无妨。
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    schema::migrate(conn)
}

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

pub fn random_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex_encode(&bytes)
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex_encode(Sha256::digest(data).as_slice())
}

/// 测试用：内存库 + 已迁移。
#[cfg(test)]
pub fn test_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory db");
    init_conn(&conn).expect("init schema");
    conn
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_opens_in_memory_and_migrates() {
        let store = AdminStore::open_in_memory().expect("open");
        let version: String = store
            .with(|c| {
                Ok(c.query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| {
                    r.get(0)
                })?)
            })
            .expect("version");
        assert_eq!(version, schema::SCHEMA_VERSION.to_string());
    }

    #[test]
    fn hex_and_sha256_are_lowercase_hex() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0x1a]), "00ff1a");
        let digest = sha256_hex(b"abc");
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    }

    #[test]
    fn random_id_is_32_hex_chars_and_unique() {
        let a = random_id();
        let b = random_id();
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
