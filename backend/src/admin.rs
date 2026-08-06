use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{delete, get},
};
use futures_util::TryStreamExt;
use mongodb::{
    Collection, IndexModel,
    bson::{doc, oid::ObjectId},
    options::{FindOptions, IndexOptions},
};
use uuid::Uuid;

use crate::{
    crypto,
    error::{ApiError, ApiResult},
    models::{
        AdminSession, CdkListResponse, CdkMapping, CdkMappingResponse, CdkUsageListResponse,
        CdkUsageLog, CdkUsageLogResponse, CdkUsageQuery, CreateCdkRequest, MessageResponse,
    },
    oauth,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/cdks", get(list_cdks).post(create_cdk))
        .route("/cdks/{id}", delete(delete_cdk))
        .route("/cdk-usage", get(list_cdk_usage))
        .route("/cdk-usage/{id}", get(get_cdk_usage))
}

pub async fn ensure_indexes(
    mappings: &Collection<CdkMapping>,
    admin_sessions: &Collection<AdminSession>,
    usage_logs: &Collection<CdkUsageLog>,
    usage_log_ttl_seconds: u64,
) -> Result<(), mongodb::error::Error> {
    let options = IndexOptions::builder().unique(true).build();
    let index = IndexModel::builder()
        .keys(doc! { "distribution_cdk": 1 })
        .options(options)
        .build();
    mappings.create_index(index, None).await?;

    let usage_created_index = IndexModel::builder()
        .keys(doc! { "created_at": -1 })
        .build();
    usage_logs.create_index(usage_created_index, None).await?;

    let usage_cdk_index = IndexModel::builder()
        .keys(doc! { "distribution_cdk": 1, "created_at": -1 })
        .build();
    usage_logs.create_index(usage_cdk_index, None).await?;

    let usage_request_index = IndexModel::builder().keys(doc! { "request_id": 1 }).build();
    usage_logs.create_index(usage_request_index, None).await?;

    let usage_ttl_index = IndexModel::builder()
        .keys(doc! { "created_at_date": 1 })
        .options(
            IndexOptions::builder()
                .expire_after(Duration::from_secs(usage_log_ttl_seconds))
                .build(),
        )
        .build();
    usage_logs.create_index(usage_ttl_index, None).await?;

    let session_ttl_index = IndexModel::builder()
        .keys(doc! { "expires_at_date": 1 })
        .options(
            IndexOptions::builder()
                .expire_after(Duration::from_secs(0))
                .build(),
        )
        .build();
    admin_sessions.create_index(session_ttl_index, None).await?;

    Ok(())
}

async fn list_cdks(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<CdkListResponse> {
    oauth::require_admin(&state, &headers).await?;

    let mut cursor = state.mappings.find(doc! {}, None).await?;
    let mut items = Vec::new();

    while let Some(mapping) = cursor.try_next().await? {
        items.push(mapping_response(&state, mapping));
    }

    items.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(Json(CdkListResponse { items }))
}

async fn create_cdk(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateCdkRequest>,
) -> ApiResult<CdkMappingResponse> {
    oauth::require_admin(&state, &headers).await?;

    let upstream_cdk = payload.upstream_cdk.trim().to_string();
    if upstream_cdk.is_empty() {
        return Err(ApiError::bad_request("上游 CDK 不能为空"));
    }

    let distribution_cdk = match payload.distribution_cdk.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => {
            if value.chars().count() < 12 {
                return Err(ApiError::bad_request(
                    "分发 CDK 长度至少 12 位，建议留空自动生成",
                ));
            }
            value.to_string()
        }
        _ => generate_distribution_cdk(),
    };

    let existing = state
        .mappings
        .find_one(doc! { "distribution_cdk": &distribution_cdk }, None)
        .await?;

    if existing.is_some() {
        return Err(ApiError::conflict("分发 CDK 已存在"));
    }

    let upstream_cdk_encrypted =
        crypto::encrypt_value(state.oauth.token_encryption_secret(), &upstream_cdk)?;
    let now = timestamp();
    let mapping = CdkMapping {
        id: None,
        distribution_cdk,
        upstream_cdk: upstream_cdk_encrypted,
        note: payload
            .note
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        enabled: true,
        created_at: now.clone(),
        updated_at: now,
    };

    let insert = state.mappings.insert_one(mapping, None).await?;
    let id = insert
        .inserted_id
        .as_object_id()
        .ok_or_else(|| ApiError::bad_request("MongoDB 未返回有效 ID"))?;
    let created = state
        .mappings
        .find_one(doc! { "_id": id }, None)
        .await?
        .ok_or_else(|| ApiError::not_found("创建后未找到 CDK 映射"))?;

    Ok(Json(mapping_response(&state, created)))
}

async fn delete_cdk(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<MessageResponse> {
    oauth::require_admin(&state, &headers).await?;

    let object_id =
        ObjectId::parse_str(&id).map_err(|_| ApiError::bad_request("CDK 映射 ID 无效"))?;
    let result = state
        .mappings
        .delete_one(doc! { "_id": object_id }, None)
        .await?;

    if result.deleted_count == 0 {
        return Err(ApiError::not_found("CDK 映射不存在"));
    }

    Ok(Json(MessageResponse {
        message: "已删除".to_string(),
    }))
}

async fn list_cdk_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CdkUsageQuery>,
) -> ApiResult<CdkUsageListResponse> {
    oauth::require_admin(&state, &headers).await?;

    let mut filter = doc! {};
    if let Some(distribution_cdk) = query
        .distribution_cdk
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        filter.insert("distribution_cdk", distribution_cdk);
    }

    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let options = FindOptions::builder()
        .sort(doc! { "created_at": -1 })
        .limit(limit)
        .build();
    let mut cursor = state.usage_logs.find(filter, options).await?;
    let mut items = Vec::new();

    while let Some(log) = cursor.try_next().await? {
        items.push(usage_log_response(log));
    }

    Ok(Json(CdkUsageListResponse { items }))
}

async fn get_cdk_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<CdkUsageLogResponse> {
    oauth::require_admin(&state, &headers).await?;

    let object_id =
        ObjectId::parse_str(&id).map_err(|_| ApiError::bad_request("CDK 使用记录 ID 无效"))?;
    let log = state
        .usage_logs
        .find_one(doc! { "_id": object_id }, None)
        .await?
        .ok_or_else(|| ApiError::not_found("CDK 使用记录不存在"))?;

    Ok(Json(usage_log_response(log)))
}

fn mapping_response(state: &AppState, mapping: CdkMapping) -> CdkMappingResponse {
    let upstream_cdk_masked =
        match crypto::decrypt_value(state.oauth.token_encryption_secret(), &mapping.upstream_cdk) {
            Ok(value) => mask_cdk(&value),
            Err(error) => {
                tracing::warn!("解密上游 CDK 失败: {error:?}");
                mask_cdk(&mapping.upstream_cdk)
            }
        };

    CdkMappingResponse {
        id: mapping.id.map(|id| id.to_hex()).unwrap_or_default(),
        distribution_cdk: mapping.distribution_cdk,
        upstream_cdk_masked,
        note: mapping.note,
        enabled: mapping.enabled,
        created_at: mapping.created_at,
        updated_at: mapping.updated_at,
    }
}

fn usage_log_response(log: CdkUsageLog) -> CdkUsageLogResponse {
    CdkUsageLogResponse {
        id: log.id.map(|id| id.to_hex()).unwrap_or_default(),
        request_id: log.request_id,
        distribution_cdk: log.distribution_cdk,
        requested_cdk_masked: log.requested_cdk_masked,
        mapping_id: log.mapping_id.map(|id| id.to_hex()),
        cdk_note: log.cdk_note,
        matched_mapping: log.matched_mapping,
        request_method: log.request_method,
        request_path: log.request_path,
        request_query: log.request_query,
        client_ip: log.client_ip,
        forwarded_for: log.forwarded_for,
        user_agent: log.user_agent,
        referer: log.referer,
        origin: log.origin,
        accept_language: log.accept_language,
        service_type: log.service_type,
        account_count: log.account_count,
        request_body_bytes: log.request_body_bytes,
        request_summary: log.request_summary,
        response_status: log.response_status,
        response_body_bytes: log.response_body_bytes,
        response_summary: log.response_summary,
        error: log.error,
        duration_ms: log.duration_ms,
        created_at: log.created_at,
    }
}

fn generate_distribution_cdk() -> String {
    format!("dist-{}", Uuid::new_v4().simple())
}

fn mask_cdk(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= 8 {
        return "********".to_string();
    }

    let prefix = chars.iter().take(4).collect::<String>();
    let suffix = chars.iter().rev().take(4).collect::<Vec<_>>();
    let suffix = suffix.into_iter().rev().collect::<String>();
    format!("{prefix}...{suffix}")
}

fn timestamp() -> String {
    unix_timestamp().to_string()
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
