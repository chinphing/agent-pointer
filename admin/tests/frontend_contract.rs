//! 前后端请求体契约测试。
//!
//! 这些用例的 JSON **逐字抄自 `admin/web` 真实发出的载荷**。
//!
//! 背景：`CreateUserBody.display_name` 曾经只写了 `alias = "display_name"`
//! （等于没写），而前端发的是 `displayName`——字段被静默丢弃，
//! 后端于是拿用户名兜底，界面上「姓名」列显示成账号名。
//! 这类 bug 不会报错、不会 500，只会悄悄少一个字段，必须靠契约测试兜住。
//!
//! 规则：前端用 camelCase，后端字段用 snake_case，**每个跨词字段都要
//! `rename` 或 `alias`**。新增字段时把真实载荷加到这里。

use pointer_admin::http::{insight, resources, users};
use serde_json::json;

fn parse<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> T {
    serde_json::from_value(v).expect("载荷应能被解析")
}

#[test]
fn create_user_body_keeps_display_name_from_frontend() {
    // Members.vue: api.post('/api/admin/users', { username, displayName, password, roles })
    let body: users::CreateUserBody = parse(json!({
        "username": "lisi",
        "displayName": "李四",
        "password": "Init@12345",
        "roles": ["member"]
    }));
    assert_eq!(body.display_name, "李四", "displayName 不能被静默丢弃");
    assert_eq!(body.roles, vec!["member".to_string()]);
}

#[test]
fn create_user_body_still_accepts_snake_case() {
    // 脚本/curl 调用方用的是 snake_case，不能被这次修复打破。
    let body: users::CreateUserBody = parse(json!({
        "username": "lisi",
        "display_name": "李四",
        "password": "Init@12345"
    }));
    assert_eq!(body.display_name, "李四");
}

#[test]
fn import_body_accepts_camel_case_default_role() {
    let body: users::ImportBody = parse(json!({
        "csv": "username,display_name,password\nwangwu,王五,Init@12345",
        "defaultRole": "member"
    }));
    assert_eq!(body.default_role.as_deref(), Some("member"));
}

#[test]
fn create_resource_body_keeps_parent_id_and_activation() {
    // Resources.vue 新建模型时会带 parentId；activation 用于技能/插件。
    let body: resources::CreateBody = parse(json!({
        "kind": "model",
        "slug": "qwen-plus",
        "name": "qwen-plus",
        "enabled": true,
        "parentId": "provider-1",
        "activation": "default_on",
        "payload": {}
    }));
    assert_eq!(body.parent_id.as_deref(), Some("provider-1"));
    assert_eq!(body.activation.as_deref(), Some("default_on"));
}

#[test]
fn grant_item_uses_type_not_principal_type() {
    // Resources.vue: { items: [{ type: 'all' | 'role' | 'user', id }] }
    let body: resources::GrantsBody = parse(json!({
        "items": [{ "type": "role", "id": "role-1" }]
    }));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].principal_type, "role");
    assert_eq!(body.items[0].id, "role-1");
}

#[test]
fn usage_query_accepts_camel_case_params() {
    let q: insight::UsageQuery = parse(json!({
        "from": "2026-11-01",
        "to": "2026-11-30",
        "groupBy": "model",
        "userId": "u1"
    }));
    assert_eq!(q.group_by.as_deref(), Some("model"));
    assert_eq!(q.user_id.as_deref(), Some("u1"));
}

#[test]
fn quota_body_accepts_camel_case_fields() {
    let body: insight::QuotaBody = parse(json!({
        "monthlyTokenLimit": 1000000,
        "overAction": "block"
    }));
    assert_eq!(body.monthly_token_limit, Some(1_000_000));
    assert_eq!(body.over_action.as_deref(), Some("block"));
}
