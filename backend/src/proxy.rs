use axum::{
    Router,
    body::Bytes,
    extract::{OriginalUri, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
};
use mongodb::bson::doc;
use serde_json::{Value, json};

use crate::{error::ApiError, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api", any(proxy_api))
        .route("/api/{*path}", any(proxy_api))
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
