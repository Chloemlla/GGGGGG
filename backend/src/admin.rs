use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{delete, get},
};
use futures_util::TryStreamExt;
use mongodb::{
    Collection, IndexModel,
    bson::{doc, oid::ObjectId},
    options::IndexOptions,
};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    models::{CdkListResponse, CdkMapping, CdkMappingResponse, CreateCdkRequest, MessageResponse},
    oauth,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/cdks", get(list_cdks).post(create_cdk))
        .route("/cdks/{id}", delete(delete_cdk))
}

pub async fn ensure_indexes(
    collection: &Collection<CdkMapping>,
) -> Result<(), mongodb::error::Error> {
    let options = IndexOptions::builder().unique(true).build();
    let index = IndexModel::builder()
        .keys(doc! { "distribution_cdk": 1 })
        .options(options)
        .build();
    collection.create_index(index, None).await?;
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
        items.push(mapping_response(mapping));
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

    let distribution_cdk = payload
        .distribution_cdk
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(generate_distribution_cdk);

    let existing = state
        .mappings
        .find_one(doc! { "distribution_cdk": &distribution_cdk }, None)
        .await?;

    if existing.is_some() {
        return Err(ApiError::conflict("分发 CDK 已存在"));
    }

    let now = timestamp();
    let mapping = CdkMapping {
        id: None,
        distribution_cdk,
        upstream_cdk,
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

    Ok(Json(mapping_response(created)))
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

fn mapping_response(mapping: CdkMapping) -> CdkMappingResponse {
    CdkMappingResponse {
        id: mapping.id.map(|id| id.to_hex()).unwrap_or_default(),
        distribution_cdk: mapping.distribution_cdk,
        upstream_cdk_masked: mask_cdk(&mapping.upstream_cdk),
        note: mapping.note,
        enabled: mapping.enabled,
        created_at: mapping.created_at,
        updated_at: mapping.updated_at,
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
