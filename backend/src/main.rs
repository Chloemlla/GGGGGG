use std::{env, path::Path as FsPath};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{OriginalUri, Path, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{any, delete, get},
};
use futures_util::TryStreamExt;
use mongodb::{
    Client as MongoClient, Collection, IndexModel,
    bson::{doc, oid::ObjectId},
    options::IndexOptions,
};
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use uuid::Uuid;

const UPSTREAM_BASE_URL: &str = "https://pixel.yh-mo.xyz";

#[derive(Clone)]
struct AppState {
    http: HttpClient,
    mappings: Collection<CdkMapping>,
    upstream_base_url: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CdkMapping {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    id: Option<ObjectId>,
    distribution_cdk: String,
    upstream_cdk: String,
    note: Option<String>,
    enabled: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct CreateCdkRequest {
    distribution_cdk: Option<String>,
    upstream_cdk: String,
    note: Option<String>,
}

#[derive(Debug, Serialize)]
struct CdkListResponse {
    items: Vec<CdkMappingResponse>,
}

#[derive(Debug, Serialize)]
struct CdkMappingResponse {
    id: String,
    distribution_cdk: String,
    upstream_cdk_masked: String,
    note: Option<String>,
    enabled: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct MessageResponse {
    message: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    detail: String,
}

struct ApiError {
    status: StatusCode,
    detail: String,
}

impl ApiError {
    fn bad_request(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            detail: detail.into(),
        }
    }

    fn conflict(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            detail: detail.into(),
        }
    }

    fn not_found(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            detail: detail.into(),
        }
    }

    fn upstream(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            detail: detail.into(),
        }
    }
}

impl From<mongodb::error::Error> for ApiError {
    fn from(error: mongodb::error::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            detail: format!("MongoDB error: {error}"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                detail: self.detail,
            }),
        )
            .into_response()
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            env::var("RUST_LOG").unwrap_or_else(|_| "pixel_api=debug,tower_http=debug".to_string()),
        )
        .init();

    let mongo_uri = config_value("MONGODB_URI", "mongodb://localhost:27017");
    let mongo_database = config_value("MONGODB_DATABASE", "pixel_remake");
    let mongo = MongoClient::with_uri_str(&mongo_uri)
        .await
        .expect("connect MongoDB");
    let mappings = mongo
        .database(&mongo_database)
        .collection::<CdkMapping>("cdk_mappings");

    ensure_indexes(&mappings)
        .await
        .expect("create MongoDB indexes");

    let state = AppState {
        http: HttpClient::new(),
        mappings,
        upstream_base_url: UPSTREAM_BASE_URL,
    };

    let frontend_dir = frontend_dist_dir();
    let frontend_index = format!("{frontend_dir}/index.html");
    let frontend_assets =
        ServeDir::new(&frontend_dir).not_found_service(ServeFile::new(frontend_index));

    let app = Router::new()
        .route("/api/admin/cdks", get(list_cdks).post(create_cdk))
        .route("/api/admin/cdks/{id}", delete(delete_cdk))
        .route("/api", any(proxy_api))
        .route("/api/{*path}", any(proxy_api))
        .fallback_service(frontend_assets)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let bind_addr = bind_addr();
    println!("serving frontend from: {frontend_dir}");
    println!("proxying user /api requests to Base URL: {UPSTREAM_BASE_URL}");
    println!("admin CDK mappings stored in MongoDB database: {mongo_database}");

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|error| panic!("bind API address {bind_addr}: {error}"));
    let addr = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read bound API address: {error}"));
    println!("pixel-api listening on http://{addr}");
    axum::serve(listener, app).await.expect("run API server");
}

fn frontend_dist_dir() -> String {
    if env::var("FRONTEND_DIST_DIR").is_ok() {
        return config_value("FRONTEND_DIST_DIR", "frontend/dist");
    }

    if FsPath::new("frontend/dist/index.html").exists() {
        "frontend/dist".to_string()
    } else {
        "../frontend/dist".to_string()
    }
}

fn config_value(name: &str, default: &str) -> String {
    env::var(name)
        .map(|value| strip_wrapping_quotes(value.trim()).to_string())
        .unwrap_or_else(|_| default.to_string())
}

fn bind_addr() -> String {
    normalize_bind_addr(&config_value("BIND_ADDR", "0.0.0.0:8080"))
}

fn normalize_bind_addr(value: &str) -> String {
    let without_scheme = value
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(value)
        .trim();
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(without_scheme)
        .trim();

    if authority.starts_with(':') {
        format!("0.0.0.0{authority}")
    } else if authority
        .chars()
        .all(|character| character.is_ascii_digit())
    {
        format!("0.0.0.0:{authority}")
    } else {
        authority.to_string()
    }
}

fn strip_wrapping_quotes(value: &str) -> &str {
    value
        .strip_prefix('\'')
        .and_then(|inner| inner.strip_suffix('\''))
        .or_else(|| {
            value
                .strip_prefix('"')
                .and_then(|inner| inner.strip_suffix('"'))
        })
        .unwrap_or(value)
}

async fn ensure_indexes(collection: &Collection<CdkMapping>) -> Result<(), mongodb::error::Error> {
    let options = IndexOptions::builder().unique(true).build();
    let index = IndexModel::builder()
        .keys(doc! { "distribution_cdk": 1 })
        .options(options)
        .build();
    collection.create_index(index, None).await?;
    Ok(())
}

async fn list_cdks(State(state): State<AppState>) -> ApiResult<CdkListResponse> {
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
    Json(payload): Json<CreateCdkRequest>,
) -> ApiResult<CdkMappingResponse> {
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
    Path(id): Path<String>,
) -> ApiResult<MessageResponse> {
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

async fn proxy_api(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match proxy_api_inner(state, uri, method, headers, body).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn proxy_api_inner(
    state: AppState,
    uri: axum::http::Uri,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let upstream_url = format!(
        "{}{}",
        state.upstream_base_url.trim_end_matches('/'),
        uri.path_and_query()
            .map(|value| value.as_str())
            .unwrap_or("/api")
    );

    let upstream_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .map_err(|error| ApiError::bad_request(format!("请求方法不支持: {error}")))?;
    let outbound_body = rewrite_card_key(&state, body).await?;
    let mut request = state.http.request(upstream_method, upstream_url);

    for (name, value) in headers.iter() {
        if is_hop_by_hop_header(name.as_str()) {
            continue;
        }
        request = request.header(name.as_str(), value.as_bytes());
    }

    if !outbound_body.is_empty() {
        request = request.body(outbound_body);
    }

    let response = request
        .send()
        .await
        .map_err(|error| ApiError::upstream(format!("上游请求失败: {error}")))?;
    upstream_response(response).await
}

async fn rewrite_card_key(state: &AppState, body: Bytes) -> Result<Vec<u8>, ApiError> {
    if body.is_empty() {
        return Ok(Vec::new());
    }

    let Ok(mut value) = serde_json::from_slice::<Value>(&body) else {
        return Ok(body.to_vec());
    };

    let Some(card_key) = value.get("card_key").and_then(Value::as_str).map(str::trim) else {
        return serde_json::to_vec(&value)
            .map_err(|error| ApiError::bad_request(error.to_string()));
    };

    if card_key.is_empty() {
        return serde_json::to_vec(&value)
            .map_err(|error| ApiError::bad_request(error.to_string()));
    }

    if let Some(mapping) = state
        .mappings
        .find_one(doc! { "distribution_cdk": card_key, "enabled": true }, None)
        .await?
    {
        value["card_key"] = json!(mapping.upstream_cdk);
    }

    serde_json::to_vec(&value).map_err(|error| ApiError::bad_request(error.to_string()))
}

async fn upstream_response(response: reqwest::Response) -> Result<Response, ApiError> {
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut headers = HeaderMap::new();

    for (name, value) in response.headers().iter() {
        if is_hop_by_hop_header(name.as_str()) {
            continue;
        }

        if let (Ok(name), Ok(value)) = (
            header::HeaderName::from_bytes(name.as_str().as_bytes()),
            header::HeaderValue::from_bytes(value.as_bytes()),
        ) {
            headers.insert(name, value);
        }
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| ApiError::upstream(format!("读取上游响应失败: {error}")))?;
    Ok((status, headers, bytes).into_response())
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
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.to_string()
}

fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
    )
}

#[cfg(test)]
mod tests {
    use super::{normalize_bind_addr, strip_wrapping_quotes};

    #[test]
    fn strips_matching_wrapping_quotes() {
        assert_eq!(
            strip_wrapping_quotes("'mongodb://localhost:27017'"),
            "mongodb://localhost:27017"
        );
        assert_eq!(
            strip_wrapping_quotes("\"mongodb://localhost:27017\""),
            "mongodb://localhost:27017"
        );
    }

    #[test]
    fn keeps_unwrapped_or_unmatched_values() {
        assert_eq!(
            strip_wrapping_quotes("mongodb://localhost:27017"),
            "mongodb://localhost:27017"
        );
        assert_eq!(
            strip_wrapping_quotes("'mongodb://localhost:27017"),
            "'mongodb://localhost:27017"
        );
    }

    #[test]
    fn normalizes_common_bind_addr_values() {
        assert_eq!(normalize_bind_addr("8080"), "0.0.0.0:8080");
        assert_eq!(normalize_bind_addr(":8080"), "0.0.0.0:8080");
        assert_eq!(
            normalize_bind_addr("http://localhost:8080/"),
            "localhost:8080"
        );
        assert_eq!(
            normalize_bind_addr("https://0.0.0.0:8080/api"),
            "0.0.0.0:8080"
        );
    }
}
