//! 组装下发给 server / 客户端的凭证。
//!
//! 这是控制面的核心接缝：把 `provider` / `model` / `tier_profile` 三类资源，
//! 组装成 `pointer_contract::PlatformLoginCredentials`，由 partner 接口返回。
//! 客户端与 server 一行都不用改——它们本来就在等这个形状。

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use pointer_contract::{
    key_fingerprint, model_catalog_hash, PlatformLoginCredentials, PlatformProviderModel,
    PlatformProviderTemplate,
};

use crate::crypto::decrypt_secret;
use crate::db::AdminStore;
use crate::grants::{self, Principal};
use crate::resources::{self, Resource, KIND_MODEL, KIND_PROVIDER, KIND_TIER_PROFILE};

/// 档位映射的固定 slug。
pub const DEFAULT_TIER_PROFILE: &str = "default";

/// 组装某用户可见的凭证。没有可见资源时返回空凭证（不报错）。
pub async fn build_for_user(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
) -> Result<PlatformLoginCredentials> {
    let visible: HashSet<String> = grants::visible_resource_ids(store, tenant_id, user_id)
        .await?
        .into_iter()
        .collect();

    let providers: Vec<Resource> = resources::list(store, tenant_id, Some(KIND_PROVIDER))
        .await?
        .into_iter()
        .filter(|r| r.enabled && visible.contains(&r.id))
        .collect();

    let models: Vec<Resource> = resources::list(store, tenant_id, Some(KIND_MODEL))
        .await?
        .into_iter()
        .filter(|r| r.enabled)
        .filter(|r| {
            // 模型继承服务商的可见性：授权了服务商就等于授权了它下面的模型，
            // 否则每加一个模型都要再授权一次，实际不可维护。
            visible.contains(&r.id)
                || r.parent_id
                    .as_deref()
                    .is_some_and(|parent| visible.contains(parent))
        })
        .collect();

    let mut catalog: HashMap<String, Vec<String>> = HashMap::new();
    let mut templates: Vec<PlatformProviderTemplate> = Vec::new();
    let mut keys: HashMap<String, String> = HashMap::new();
    let mut fingerprints: BTreeMap<String, String> = BTreeMap::new();

    for provider in &providers {
        let payload = parse_payload(&provider.payload_json);

        let own: Vec<&Resource> = models
            .iter()
            .filter(|m| m.parent_id.as_deref() == Some(provider.id.as_str()))
            .collect();
        let mut names: Vec<String> = own.iter().map(|m| m.slug.clone()).collect();
        if names.is_empty() {
            // 回退：provider 的 payload 里直接写了 models 数组
            names = payload
                .get("models")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
        }
        names.sort();
        names.dedup();
        if names.is_empty() {
            log::warn!(
                "admin credentials: provider {} has no models; skipped",
                provider.slug
            );
            continue;
        }

        let secret = match resources::secret(store, &provider.id).await? {
            Some(enc) => decrypt_secret(&enc)?,
            None => {
                log::warn!(
                    "admin credentials: provider {} has no key; skipped",
                    provider.slug
                );
                continue;
            }
        };

        let base_url = payload
            .get("baseUrl")
            .or_else(|| payload.get("base_url"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        templates.push(PlatformProviderTemplate {
            id: provider.slug.clone(),
            name: provider.name.clone(),
            base_url,
            models: model_templates(&own, &names),
            ..Default::default()
        });
        catalog.insert(provider.slug.clone(), names);
        keys.insert(provider.slug.clone(), secret.clone());
        fingerprints.insert(provider.slug.clone(), key_fingerprint(&secret));
    }

    let tier_defaults = load_tier_defaults(store, tenant_id, user_id, &visible).await?;
    let hash = model_catalog_hash(&catalog, &tier_defaults, &fingerprints);

    Ok(PlatformLoginCredentials {
        api_key: None,
        llm_provider: None,
        provider_api_keys: keys,
        model_catalog: catalog,
        platform_providers: templates,
        tier_defaults,
        model_catalog_hash: Some(hash),
        media_oss: None,
    })
}

async fn load_tier_defaults(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
    visible: &HashSet<String>,
) -> Result<serde_json::Value> {
    let profiles: Vec<Resource> = resources::list(store, tenant_id, Some(KIND_TIER_PROFILE))
        .await?
        .into_iter()
        .filter(|r| r.enabled && visible.contains(&r.id))
        .collect();

    // 一个用户可能同时命中多条档位映射（全员默认 + 部门专属）。
    // 规则：用户直授 > 角色授 > 全员，同级按 slug 字典序取第一个。
    let mut best: Option<(u8, String, Resource)> = None;
    for profile in profiles {
        let specificity = tier_specificity(store, &profile.id, user_id).await?;
        let candidate = (specificity, profile.slug.clone(), profile);
        best = match best {
            None => Some(candidate),
            Some(current) => {
                let better = candidate.0 > current.0
                    || (candidate.0 == current.0 && candidate.1 < current.1);
                Some(if better { candidate } else { current })
            }
        };
    }

    Ok(match best {
        Some((_, _, profile)) => parse_payload(&profile.payload_json),
        None => serde_json::Value::Null,
    })
}

/// 3 = 直接授给这个用户；2 = 授给某个角色；1 = 全员；0 = 无授权（不应出现）。
async fn tier_specificity(store: &AdminStore, resource_id: &str, user_id: &str) -> Result<u8> {
    let list = grants::grants(store, resource_id).await?;
    let mut score = 0u8;
    for principal in list {
        match principal {
            Principal::User(id) if id == user_id => return Ok(3),
            Principal::User(_) => {}
            Principal::Role(_) => score = score.max(2),
            Principal::All => score = score.max(1),
        }
    }
    Ok(score)
}

/// 模型资源 → 下发给客户端的模型模板。
///
/// payload 里存的就是 `PlatformProviderModel` 那套字段（supportsVision /
/// contextBudgetTokens / thinkingIntensity…），这里原样反序列化；`name` 一律取
/// slug —— 它是真正发给模型接口的 id，显示名走资源自身的 `name`，两者不能混。
///
/// 之前这里只填了 name，模型级参数被整个丢掉：管理端配了视觉能力、上下文长度，
/// 客户端一个也收不到。
fn model_templates(models: &[&Resource], names: &[String]) -> Vec<PlatformProviderModel> {
    names
        .iter()
        .map(|name| {
            let payload = models
                .iter()
                .find(|m| &m.slug == name)
                .map(|m| parse_payload(&m.payload_json))
                .unwrap_or(serde_json::Value::Null);
            let mut tpl: PlatformProviderModel =
                serde_json::from_value(payload).unwrap_or_default();
            tpl.name = name.clone();
            tpl
        })
        .collect()
}

fn parse_payload(raw: &str) -> serde_json::Value {
    if raw.trim().is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::from_str(raw).unwrap_or(serde_json::Value::Null)
}

#[cfg(test)]
// 这些用例故意把全局环境锁持有到用例结束（跨 await）：环境变量是进程级的，
// 不这么做就会和 crypto 用例互相污染。测试跑在 current_thread 运行时上，
// 不存在跨线程持锁死锁的问题，clippy 这条在这里是误报。
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;
    use crate::crypto::encrypt_secret;
    use crate::db::AdminStore;
    use crate::grants::Principal;
    use crate::schema::DEFAULT_TENANT;
    use crate::users;

    /// 解密会读 `POINTER_ADMIN_MASTER_KEY`，而 crypto 的用例会临时设置它。
    /// 这里必须拿同一把全局锁并**持有到用例结束**，否则并行跑时偶发解密失败。
    async fn setup() -> (AdminStore, String, std::sync::MutexGuard<'static, ()>) {
        let guard = crate::test_support::env_lock();
        let store = AdminStore::connect_in_memory().await.expect("store");
        let uid = users::create(&store, DEFAULT_TENANT, "alice", "", "pw-123456")
            .await
            .expect("user")
            .id;
        (store, uid, guard)
    }

    async fn add_provider(
        store: &AdminStore,
        slug: &str,
        key: &str,
        models: &[&str],
    ) -> (String, Vec<String>) {
        let enc = encrypt_secret(key).expect("encrypt");
        let provider = resources::create(
            store,
            DEFAULT_TENANT,
            KIND_PROVIDER,
            slug,
            slug,
            "",
            None,
            r#"{"baseUrl":"https://example.invalid/v1"}"#,
            Some(&enc),
        )
        .await
        .expect("provider");
        resources::set_enabled(store, &provider.id, true)
            .await
            .expect("enable provider");

        let mut ids = Vec::new();
        for model in models {
            let m = resources::create(
                store,
                DEFAULT_TENANT,
                KIND_MODEL,
                model,
                model,
                "",
                Some(&provider.id),
                "{}",
                None,
            )
            .await
            .expect("model");
            resources::set_enabled(store, &m.id, true).await.expect("enable model");
            ids.push(m.id);
        }
        (provider.id, ids)
    }

    /// 模型级参数（视觉 / 上下文长度 / 思考强度）必须跟着下发。
    /// 之前这里只填 name，管理端配了这些字段客户端一个都收不到。
    #[tokio::test]
    async fn model_level_params_are_distributed() {
        let (store, uid, _guard) = setup().await;
        let (pid, ids) = add_provider(&store, "bailian", "sk-test", &["qwen-vl"]).await;
        resources::update_meta(
            &store,
            &ids[0],
            "qwen-vl",
            "",
            r#"{"supportsVision":true,"contextBudgetTokens":131072,"thinkingIntensity":"high"}"#,
        )
        .await
        .expect("payload");
        grants::replace_grants(&store, &pid, &[Principal::All])
            .await
            .expect("grant provider");
        grants::replace_grants(&store, &ids[0], &[Principal::All])
            .await
            .expect("grant model");

        let creds = build_for_user(&store, DEFAULT_TENANT, &uid)
            .await
            .expect("build");
        let tpl = creds
            .platform_providers
            .iter()
            .find(|p| p.id == "bailian")
            .expect("provider");
        let model = tpl.models.iter().find(|m| m.name == "qwen-vl").expect("model");
        assert_eq!(model.supports_vision, Some(true));
        assert_eq!(model.context_budget_tokens, Some(131072));
        assert_eq!(model.thinking_intensity.as_deref(), Some("high"));
    }

    #[tokio::test]
    async fn no_visible_resources_yields_empty_credentials() {
        let (store, uid, _guard) = setup().await;
        let creds = build_for_user(&store, DEFAULT_TENANT, &uid)
            .await
            .expect("build");
        assert!(creds.model_catalog.is_empty());
        assert!(creds.platform_providers.is_empty());
        assert!(creds.provider_api_keys.is_empty());
        assert!(creds.model_catalog_hash.is_some(), "即使空目录也应有哈希");
    }

    #[tokio::test]
    async fn builds_catalog_and_keys_from_resources() {
        let (store, uid, _guard) = setup().await;
        let (provider_id, _models) = add_provider(&store, "qwen", "sk-live", &["m1", "m2"]).await;

        // 授权：provider 与它的模型都要可见
        let model_ids: Vec<String> = resources::list(&store, DEFAULT_TENANT, Some(KIND_MODEL))
            .await
            .expect("models")
            .into_iter()
            .map(|m| m.id)
            .collect();
        grants::replace_grants(&store, &provider_id, &[Principal::All])
            .await
            .expect("grant provider");
        for id in &model_ids {
            grants::replace_grants(&store, id, &[Principal::All])
                .await
                .expect("grant model");
        }

        let creds = build_for_user(&store, DEFAULT_TENANT, &uid)
            .await
            .expect("build");
        assert_eq!(creds.model_catalog["qwen"], vec!["m1", "m2"]);
        assert_eq!(creds.provider_api_keys["qwen"], "sk-live");
        assert_eq!(creds.platform_providers.len(), 1);
        assert_eq!(
            creds.platform_providers[0].base_url,
            "https://example.invalid/v1"
        );
        assert_eq!(creds.platform_providers[0].models.len(), 2);
        assert_eq!(creds.model_catalog_hash.as_deref().map(str::len), Some(16));
    }

    #[tokio::test]
    async fn model_inherits_provider_visibility() {
        let (store, uid, _guard) = setup().await;
        let (provider_id, _models) = add_provider(&store, "qwen", "sk", &["m1", "m2"]).await;
        // 只授权服务商，模型本身不单独授权
        grants::replace_grants(&store, &provider_id, &[Principal::All])
            .await
            .expect("grant provider");

        let creds = build_for_user(&store, DEFAULT_TENANT, &uid)
            .await
            .expect("build");
        assert_eq!(
            creds.model_catalog["qwen"],
            vec!["m1", "m2"],
            "授权服务商应连带其模型"
        );
    }

    #[tokio::test]
    async fn provider_without_key_is_skipped() {
        let (store, uid, _guard) = setup().await;
        let provider = resources::create(
            &store,
            DEFAULT_TENANT,
            KIND_PROVIDER,
            "nokey",
            "nokey",
            "",
            None,
            r#"{"baseUrl":"https://x"}"#,
            None,
        )
        .await
        .expect("provider");
        resources::set_enabled(&store, &provider.id, true)
            .await
            .expect("enable");
        grants::replace_grants(&store, &provider.id, &[Principal::All])
            .await
            .expect("grant");

        let creds = build_for_user(&store, DEFAULT_TENANT, &uid)
            .await
            .expect("build");
        assert!(creds.model_catalog.is_empty());
    }

    #[tokio::test]
    async fn disabled_provider_is_not_shipped() {
        let (store, uid, _guard) = setup().await;
        let (provider_id, _) = add_provider(&store, "qwen", "sk", &["m1"]).await;
        resources::set_enabled(&store, &provider_id, false)
            .await
            .expect("disable");
        grants::replace_grants(&store, &provider_id, &[Principal::All])
            .await
            .expect("grant");

        let creds = build_for_user(&store, DEFAULT_TENANT, &uid)
            .await
            .expect("build");
        assert!(creds.model_catalog.is_empty());
    }

    #[tokio::test]
    async fn tier_profile_is_shipped_when_visible() {
        let (store, uid, _guard) = setup().await;
        let profile = resources::create(
            &store,
            DEFAULT_TENANT,
            KIND_TIER_PROFILE,
            DEFAULT_TIER_PROFILE,
            "默认档位",
            "",
            None,
            r#"{"agentModeLlm":{"general":{"fast":{"providerId":"qwen","model":"m1"}}}}"#,
            None,
        )
        .await
        .expect("profile");
        resources::set_enabled(&store, &profile.id, true)
            .await
            .expect("enable");
        grants::replace_grants(&store, &profile.id, &[Principal::All])
            .await
            .expect("grant");

        let creds = build_for_user(&store, DEFAULT_TENANT, &uid)
            .await
            .expect("build");
        assert_eq!(
            creds.tier_defaults["agentModeLlm"]["general"]["fast"]["model"],
            "m1"
        );
    }

    #[tokio::test]
    async fn user_specific_tier_profile_beats_all_grant() {
        let (store, uid, _guard) = setup().await;

        let global = resources::create(
            &store,
            DEFAULT_TENANT,
            KIND_TIER_PROFILE,
            "a-global",
            "全员档位",
            "",
            None,
            r#"{"agentModeLlm":{"general":{"fast":{"model":"global-model"}}}}"#,
            None,
        )
        .await
        .expect("global");
        resources::set_enabled(&store, &global.id, true).await.expect("enable");
        grants::replace_grants(&store, &global.id, &[Principal::All])
            .await
            .expect("grant global");

        let personal = resources::create(
            &store,
            DEFAULT_TENANT,
            KIND_TIER_PROFILE,
            "z-personal",
            "个人档位",
            "",
            None,
            r#"{"agentModeLlm":{"general":{"fast":{"model":"personal-model"}}}}"#,
            None,
        )
        .await
        .expect("personal");
        resources::set_enabled(&store, &personal.id, true).await.expect("enable");
        grants::replace_grants(&store, &personal.id, &[Principal::User(uid.clone())])
            .await
            .expect("grant personal");

        let creds = build_for_user(&store, DEFAULT_TENANT, &uid)
            .await
            .expect("build");
        assert_eq!(
            creds.tier_defaults["agentModeLlm"]["general"]["fast"]["model"],
            "personal-model",
            "用户直授的档位应压过全员档位"
        );
    }

    #[tokio::test]
    async fn tier_profile_without_grant_is_ignored() {
        let (store, uid, _guard) = setup().await;
        let profile = resources::create(
            &store,
            DEFAULT_TENANT,
            KIND_TIER_PROFILE,
            DEFAULT_TIER_PROFILE,
            "默认档位",
            "",
            None,
            r#"{"agentModeLlm":{}}"#,
            None,
        )
        .await
        .expect("profile");
        resources::set_enabled(&store, &profile.id, true)
            .await
            .expect("enable");
        // 故意不授权

        let creds = build_for_user(&store, DEFAULT_TENANT, &uid)
            .await
            .expect("build");
        assert!(creds.tier_defaults.is_null());
    }

    #[tokio::test]
    async fn rotating_key_changes_hash() {
        let (store, uid, _guard) = setup().await;
        let (provider_id, _) = add_provider(&store, "qwen", "sk-old", &["m1"]).await;
        grants::replace_grants(&store, &provider_id, &[Principal::All])
            .await
            .expect("grant provider");
        let model_id = resources::list(&store, DEFAULT_TENANT, Some(KIND_MODEL))
            .await
            .expect("models")[0]
            .id
            .clone();
        grants::replace_grants(&store, &model_id, &[Principal::All])
            .await
            .expect("grant model");

        let before = build_for_user(&store, DEFAULT_TENANT, &uid)
            .await
            .expect("before")
            .model_catalog_hash;

        let new_secret = encrypt_secret("sk-new").expect("encrypt");
        resources::set_secret(&store, &provider_id, Some(&new_secret))
            .await
            .expect("rotate");

        let after = build_for_user(&store, DEFAULT_TENANT, &uid)
            .await
            .expect("after")
            .model_catalog_hash;
        assert_ne!(before, after, "换 Key 必须换哈希");
    }
}
