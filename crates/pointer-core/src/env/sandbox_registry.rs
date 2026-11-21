//! 会话 → 沙箱 id 的持久映射（envd 后端用）。
//!
//! docker 后端的容器名由会话 id 决定（`pointer-sbx-<id>`），应用重启后
//! `DockerEnv::attach()` 直接就能认回来；envd 的 `sandboxID` 是网关生成的，
//! 映射只在内存缓存里——进程一重启就丢了，结果重启后新建一个，旧的要挂到
//! 超时才消失（实测攒了 26 个 `e2b-envd-*` 容器）。
//!
//! 这里把映射落到 `<app data>/sandbox-registry.json`，重启后先查表再决定
//! attach 还是新建。记录同时带上模板名：换模板后旧沙箱不该被认回。

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 一条映射。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub sandbox_id: String,
    /// envd 地址（随沙箱变化，必须一起存）。
    pub envd_url: String,
    /// 建沙箱时用的模板；不一致就不认回。
    pub template: String,
}

/// 注册表文件路径。`POINTER_SANDBOX_REGISTRY` 覆盖（测试用）。
fn registry_path() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("POINTER_SANDBOX_REGISTRY") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    crate::storage::app_data_dir()
        .ok()
        .map(|dir| dir.join("sandbox-registry.json"))
}

fn load() -> HashMap<String, Entry> {
    let Some(path) = registry_path() else {
        return HashMap::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    // 文件坏了当空表：宁可多建一个沙箱，也不能因为一行坏 JSON 让会话起不来。
    serde_json::from_str(&text).unwrap_or_default()
}

fn store(map: &HashMap<String, Entry>) {
    let Some(path) = registry_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(text) = serde_json::to_string_pretty(map) else {
        return;
    };
    // 先写临时文件再改名：进程在写一半时被杀，不会留下半截 JSON。
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// 重启后能否认回旧沙箱。
///
/// 三条否决：没记录、模板变了（镜像和挂载都对不上）、`attach` + 探活失败
/// （地址是上一次进程留下的死沙箱）。否决时顺手清掉记录，免得下次再试。
///
/// `attach` 返回 `None` 表示认不回来；返回值同样是 `None` 时调用方新建沙箱。
pub fn reattach<T, F>(conversation_id: &str, template: &str, attach: F) -> Option<T>
where
    F: FnOnce(&Entry) -> Option<T>,
{
    let entry = lookup(conversation_id)?;
    if entry.template != template {
        log::info!(
            "sandbox registry template changed conversation={conversation_id} {} -> {template}",
            entry.template
        );
        forget(conversation_id);
        return None;
    }
    match attach(&entry) {
        Some(value) => {
            log::info!(
                "sandbox reattached conversation={conversation_id} sandbox={}",
                entry.sandbox_id
            );
            Some(value)
        }
        None => {
            log::info!(
                "sandbox registry entry stale conversation={conversation_id} sandbox={}",
                entry.sandbox_id
            );
            forget(conversation_id);
            None
        }
    }
}

/// 查会话对应的沙箱。
pub fn lookup(conversation_id: &str) -> Option<Entry> {
    load().remove(conversation_id)
}

/// 记下会话新建的沙箱。
pub fn record(conversation_id: &str, entry: Entry) {
    let mut map = load();
    map.insert(conversation_id.to_string(), entry);
    store(&map);
}

/// 会话释放 / 沙箱失效时清掉。
pub fn forget(conversation_id: &str) {
    let mut map = load();
    if map.remove(conversation_id).is_some() {
        store(&map);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 环境变量是进程级的，两个用例并行会互相改路径——串行执行。
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_temp_registry<F: FnOnce()>(f: F) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("registry.json");
        std::env::set_var("POINTER_SANDBOX_REGISTRY", &path);
        f();
        std::env::remove_var("POINTER_SANDBOX_REGISTRY");
    }

    #[test]
    fn records_and_forgets_roundtrip() {
        with_temp_registry(|| {
            assert!(lookup("conv-a").is_none());
            record(
                "conv-a",
                Entry {
                    sandbox_id: "sbx123".into(),
                    envd_url: "http://127.0.0.1:1234".into(),
                    template: "pointer-sandbox-headless:latest".into(),
                },
            );
            let hit = lookup("conv-a").expect("应能查回");
            assert_eq!(hit.sandbox_id, "sbx123");
            assert_eq!(hit.template, "pointer-sandbox-headless:latest");

            forget("conv-a");
            assert!(lookup("conv-a").is_none());
        });
    }

    #[test]
    fn reattach_rejects_template_change_and_dead_sandbox() {
        with_temp_registry(|| {
            record(
                "conv-r",
                Entry {
                    sandbox_id: "sbx1".into(),
                    envd_url: "http://127.0.0.1:1".into(),
                    template: "old:latest".into(),
                },
            );

            // 模板变了：不认回，并清掉记录。
            let mut called = false;
            let got: Option<()> = reattach("conv-r", "new:latest", |_| {
                called = true;
                Some(())
            });
            assert!(got.is_none());
            assert!(!called, "模板不一致时不该去 attach");
            assert!(lookup("conv-r").is_none(), "过期记录应被清掉");

            // 地址死了（attach 返回 None）：也不认回。
            record(
                "conv-r",
                Entry {
                    sandbox_id: "sbx2".into(),
                    envd_url: "http://127.0.0.1:2".into(),
                    template: "new:latest".into(),
                },
            );
            let got: Option<()> = reattach("conv-r", "new:latest", |_| None);
            assert!(got.is_none());
            assert!(lookup("conv-r").is_none());

            // 活着：认回，记录保留。
            record(
                "conv-r",
                Entry {
                    sandbox_id: "sbx3".into(),
                    envd_url: "http://127.0.0.1:3".into(),
                    template: "new:latest".into(),
                },
            );
            let got = reattach("conv-r", "new:latest", |entry| Some(entry.sandbox_id.clone()));
            assert_eq!(got.as_deref(), Some("sbx3"));
            assert_eq!(lookup("conv-r").unwrap().sandbox_id, "sbx3");
        });
    }

    #[test]
    fn broken_file_reads_as_empty() {
        with_temp_registry(|| {
            let path = std::env::var("POINTER_SANDBOX_REGISTRY").unwrap();
            std::fs::write(&path, "{not json").unwrap();
            // 坏文件不能连带会话起不来。
            assert!(lookup("conv-a").is_none());
            record(
                "conv-b",
                Entry {
                    sandbox_id: "sbx9".into(),
                    envd_url: "http://127.0.0.1:9".into(),
                    template: "t".into(),
                },
            );
            assert_eq!(lookup("conv-b").unwrap().sandbox_id, "sbx9");
        });
    }
}
