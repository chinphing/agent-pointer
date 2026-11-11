//! 管理端「会话」：查看客户端上报上来的对话。
//!
//! 这是**敏感数据**（对话里可能夹带患者信息），所以：
//! - 单独一个权限键 `conversation:read`，内置 `viewer` 角色默认不给
//! - 每次真正读到对话正文都写一条审计，记「谁、什么时候、看了谁的哪个会话」
//!
//! 三级读取，越往后越贵：
//! 1. `list`   —— 会话列表，只扫 `conversation_threads` 的小字段
//! 2. `tabs`   —— 一个会话下的主/子会话
//! 3. `turns`  —— 某个会话的轮次，只有预览，不读 zip
//! 4. `turn`   —— 展开某一轮，读那一个 zip

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::audit::{self, AuditInput};
use crate::conversations;
use crate::http::auth::AdminAuth;
use crate::http::error::{ApiError, ApiResult};
use crate::state::Shared;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default, alias = "userId")]
    pub user_id: Option<String>,
    /// 起始日期，`YYYY-MM-DD`（含当天）。
    #[serde(default)]
    pub from: Option<String>,
    /// 结束日期，`YYYY-MM-DD`（含当天）。
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct TurnQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

/// `YYYY-MM-DD` → 当天零点；`to` 再加一天，让区间含结束日整天。
fn day_start(raw: &str) -> Option<i64> {
    let day = raw.trim();
    if day.is_empty() {
        return None;
    }
    crate::oauth::token::ts_from_day(day)
}

/// `GET /api/admin/conversations` —— 会话列表，一个会话一行。
pub async fn list(
    State(state): State<Shared>,
    Extension(_auth): Extension<AdminAuth>,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let user_id = query
        .user_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let from_ts = query.from.as_deref().and_then(day_start);
    let to_ts = query
        .to
        .as_deref()
        .and_then(day_start)
        .map(|ts| ts + 86_400);

    let (rows, total, total_capped) = conversations::list(
        &state.store,
        &state.config.tenant_id,
        conversations::ListFilter {
            user_id,
            from_ts,
            to_ts,
        },
        query.limit.unwrap_or(50),
        query.offset.unwrap_or(0),
    )
    .await?;

    let items: Vec<Value> = rows
        .iter()
        .map(|c| {
            json!({
                "conversationId": c.conversation_id,
                "userId": c.user_id,
                "userName": c.user_name,
                "title": c.title,
                "model": c.model,
                "threadCount": c.thread_count,
                "turnCount": c.turn_count,
                "messageCount": c.message_count,
                "firstAt": c.first_at,
                "lastAt": c.last_at,
            })
        })
        .collect();
    Ok(Json(json!({ "items": items, "total": total, "totalCapped": total_capped })))
}

/// `GET /api/admin/conversations/:conversationId/tabs` —— 主会话与各子会话。
pub async fn tabs(
    State(state): State<Shared>,
    Extension(_auth): Extension<AdminAuth>,
    Path(conversation_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let rows = conversations::threads(
        &state.store,
        &state.config.tenant_id,
        &conversation_id,
    )
    .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "agentInstanceId": t.agent_instance_id,
                "agentRoleId": t.agent_role_id,
                "agentName": t.agent_name,
                "isMain": t.is_main,
                "turnCount": t.turn_count,
                "messageCount": t.message_count,
                "firstAt": t.first_at,
                "lastAt": t.last_at,
            })
        })
        .collect();
    Ok(Json(json!({ "items": items })))
}

/// `GET /api/admin/threads/:threadId/turns` —— 某个会话的轮次（只有预览）。
pub async fn turns(
    State(state): State<Shared>,
    Extension(_auth): Extension<AdminAuth>,
    Path(thread_id): Path<String>,
    Query(query): Query<TurnQuery>,
) -> ApiResult<Json<Value>> {
    let (rows, total) = conversations::turns(
        &state.store,
        &state.config.tenant_id,
        &thread_id,
        query.limit.unwrap_or(20),
        query.offset.unwrap_or(0),
    )
    .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "seq": t.seq,
                "turnAt": t.turn_at,
                "preview": t.preview,
                "messageCount": t.message_count,
                "sizeBytes": t.size_bytes,
            })
        })
        .collect();
    Ok(Json(json!({ "items": items, "total": total })))
}

/// `GET /api/admin/turns/:turnId` —— 展开某一轮，读它那一个 zip。
pub async fn turn(
    State(state): State<Shared>,
    Extension(auth): Extension<AdminAuth>,
    Path(turn_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let Some((turn, messages)) =
        conversations::turn_messages(&state.store, &state.config.tenant_id, &turn_id).await?
    else {
        return Err(ApiError::NotFound("轮次不存在".into()));
    };

    // 看的是别人的对话 → 必须留痕。这是审计要求，不是可选项。
    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "conversation_view",
            target_type: "conversation",
            target_id: &turn.thread_id,
            before_json: None,
            after_json: Some(&json!({ "turnId": turn.id, "seq": turn.seq }).to_string()),
        },
    )
    .await?;

    Ok(Json(json!({
        "id": turn.id,
        "threadId": turn.thread_id,
        "seq": turn.seq,
        "turnAt": turn.turn_at,
        "preview": turn.preview,
        "messageCount": turn.message_count,
        "messages": messages,
    })))
}
