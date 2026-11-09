//! 凭证指纹与目录哈希。
//!
//! 客户端用 `modelCatalogHash` 判断「要不要重新拉凭证」
//! （`chat_service/session_inner.rs` 的 `refresh_llm_credentials_if_model_catalog_changed`）。
//! 哈希必须在两端算出同一个值，否则会出现「管理员改了 Key，员工端永远不刷新」——
//! 所以算法放在契约 crate 里，只有一份实现。

use std::collections::{BTreeMap, HashMap};

use sha2::{Digest, Sha256};

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// 单个 provider 的密钥指纹（前 8 位十六进制）。
///
/// **只进哈希，不进任何下发给浏览器的响应。**
pub fn key_fingerprint(api_key: &str) -> String {
    let digest = Sha256::digest(api_key.trim().as_bytes());
    hex(&digest)[..8].to_string()
}

/// 目录哈希：`sha256(规范化(modelCatalog) ‖ 规范化(tierDefaults) ‖ keyFingerprints)` 的前 16 位。
///
/// 规范化 = provider 名排序、模型列表排序、JSON 对象的键排序，保证与插入顺序无关。
/// 把密钥指纹算进来是刻意的：只换 Key、不换模型目录时哈希也会变，客户端才会刷新。
pub fn model_catalog_hash(
    model_catalog: &HashMap<String, Vec<String>>,
    tier_defaults: &serde_json::Value,
    key_fingerprints: &BTreeMap<String, String>,
) -> String {
    let mut providers: Vec<(&String, &Vec<String>)> = model_catalog.iter().collect();
    providers.sort_by(|a, b| a.0.cmp(b.0));

    let catalog: Vec<(String, Vec<String>)> = providers
        .into_iter()
        .map(|(name, models)| {
            let mut sorted = models.clone();
            sorted.sort();
            (name.clone(), sorted)
        })
        .collect();

    let payload = serde_json::json!({
        "modelCatalog": catalog,
        "tierDefaults": canonicalize(tier_defaults),
        "keyFingerprints": key_fingerprints,
    });
    let text = serde_json::to_string(&payload).unwrap_or_default();
    hex(&Sha256::digest(text.as_bytes()))[..16].to_string()
}

/// JSON 对象按键排序，数组保持顺序（数组顺序本身有语义）。
fn canonicalize(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for key in keys {
                out.insert(key.clone(), canonicalize(&map[key]));
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonicalize).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(pairs: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
            .collect()
    }

    #[test]
    fn fingerprint_is_8_hex_chars_and_stable() {
        let a = key_fingerprint("sk-abc");
        assert_eq!(a.len(), 8);
        assert_eq!(a, key_fingerprint("sk-abc"));
        assert_ne!(a, key_fingerprint("sk-abd"));
        assert_eq!(a, key_fingerprint("  sk-abc  "), "应忽略首尾空白");
    }

    #[test]
    fn hash_is_order_independent() {
        let a = catalog(&[("qwen", &["m1", "m2"]), ("deepseek", &["d1"])]);
        let b = catalog(&[("deepseek", &["d1"]), ("qwen", &["m2", "m1"])]);
        let empty = serde_json::json!({});
        let keys = BTreeMap::new();
        assert_eq!(
            model_catalog_hash(&a, &empty, &keys),
            model_catalog_hash(&b, &empty, &keys)
        );
    }

    #[test]
    fn key_rotation_changes_hash() {
        let cat = catalog(&[("qwen", &["m1"])]);
        let empty = serde_json::json!({});
        let mut before = BTreeMap::new();
        before.insert("qwen".to_string(), key_fingerprint("sk-old"));
        let mut after = BTreeMap::new();
        after.insert("qwen".to_string(), key_fingerprint("sk-new"));

        assert_ne!(
            model_catalog_hash(&cat, &empty, &before),
            model_catalog_hash(&cat, &empty, &after),
            "只换 Key 也必须换哈希，否则客户端不会刷新"
        );
    }

    #[test]
    fn tier_change_changes_hash() {
        let cat = catalog(&[("qwen", &["m1"])]);
        let keys = BTreeMap::new();
        let a = serde_json::json!({"agentModeLlm": {"general": {"fast": {"model": "m1"}}}});
        let b = serde_json::json!({"agentModeLlm": {"general": {"fast": {"model": "m2"}}}});
        assert_ne!(
            model_catalog_hash(&cat, &a, &keys),
            model_catalog_hash(&cat, &b, &keys)
        );
    }

    #[test]
    fn tier_key_order_does_not_change_hash() {
        let cat = HashMap::new();
        let keys = BTreeMap::new();
        let a: serde_json::Value =
            serde_json::from_str(r#"{"x":1,"y":2}"#).expect("json");
        let b: serde_json::Value =
            serde_json::from_str(r#"{"y":2,"x":1}"#).expect("json");
        assert_eq!(
            model_catalog_hash(&cat, &a, &keys),
            model_catalog_hash(&cat, &b, &keys)
        );
    }

    #[test]
    fn hash_is_16_hex_chars() {
        let h = model_catalog_hash(&HashMap::new(), &serde_json::Value::Null, &BTreeMap::new());
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
