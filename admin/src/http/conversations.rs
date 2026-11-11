//! 管理端「会话」：查看客户端上报上来的对话快照。
//!
//! 这是**敏感数据**（对话里可能夹带患者信息），所以：
//! - 单独一个权限键 `conversation:read`，内置 `viewer` 角色默认不给
//! - 每次打开详情都写一条审计，记「谁、什么时候、看了谁的哪个会话」

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
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

/// `GET /api/admin/conversations` —— 快照列表（不含正文，只有预览）。
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
    let (rows, total) = conversations::list(
        &state.store,
        &state.config.tenant_id,
        user_id,
        query.limit.unwrap_or(50),
        query.offset.unwrap_or(0),
    )
    .await?;

    let items: Vec<Value> = rows
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "userId": s.user_id,
                "userName": s.user_name,
                "conversationId": s.conversation_id,
                "agentInstanceId": s.agent_instance_id,
                "agentRoleId": s.agent_role_id,
                "model": s.model,
                "promptTokens": s.prompt_tokens,
                "completionTokens": s.completion_tokens,
                "messageCount": s.message_count,
                "preview": s.preview,
                "sizeBytes": s.size_bytes,
                "createdAt": s.created_at,
            })
        })
        .collect();
    Ok(Json(json!({ "items": items, "total": total })))
}

/// `GET /api/admin/conversations/:id` —— 打开一份快照的完整对话。
pub async fn detail(
    State(state): State<Shared>,
    Extension(auth): Extension<AdminAuth>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let Some((snapshot, content)) =
        conversations::detail(&state.store, &state.config.tenant_id, &id).await?
    else {
        return Err(ApiError::NotFound("会话不存在".into()));
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
            target_id: &snapshot.conversation_id,
            before_json: None,
            after_json: Some(&json!({
                "snapshotId": snapshot.id,
                "userId": snapshot.user_id,
            })
            .to_string()),
        },
    )
    .await?;

    Ok(Json(json!({
        "id": snapshot.id,
        "userId": snapshot.user_id,
        "userName": snapshot.user_name,
        "conversationId": snapshot.conversation_id,
        "agentInstanceId": snapshot.agent_instance_id,
        "agentRoleId": snapshot.agent_role_id,
        "model": snapshot.model,
        "promptTokens": snapshot.prompt_tokens,
        "completionTokens": snapshot.completion_tokens,
        "messageCount": snapshot.message_count,
        "sizeBytes": snapshot.size_bytes,
        "createdAt": snapshot.created_at,
        "messages": content.get("messages").cloned().unwrap_or_else(|| json!([])),
    })))
}
