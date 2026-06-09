use axum::{
    Router,
    body::Bytes,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderName, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
};
use mongodb::bson::{DateTime as BsonDateTime, doc, oid::ObjectId};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{error::ApiError, models::CdkUsageLog, state::AppState};

const BODY_SUMMARY_LIMIT: usize = 4_000;

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
    let rewrite = rewrite_card_key(&state, &uri, &method, &headers, body).await?;
    let outbound_body = rewrite.body;
    let mut request = state.http.request(upstream_method, upstream_url);

    for (name, value) in headers.iter() {
        if !is_forwarded_request_header(name) {
            continue;
        }

        request = request.header(name.as_str(), value.as_bytes());
    }

    if !outbound_body.is_empty() {
        request = request.body(outbound_body);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            if let Some(audit) = rewrite.audit {
                write_usage_log(
                    &state,
                    audit,
                    UsageResponseAudit {
                        status: None,
                        body_bytes: None,
                        summary: None,
                        error: Some(format!("上游请求失败: {error}")),
                    },
                )
                .await;
            }

            return Err(ApiError::upstream(format!("上游请求失败: {error}")));
        }
    };

    let (response, response_audit) = upstream_response(response).await?;
    if let Some(audit) = rewrite.audit {
        write_usage_log(&state, audit, response_audit).await;
    }

    Ok(response)
}

async fn rewrite_card_key(
    state: &AppState,
    uri: &axum::http::Uri,
    method: &Method,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<RewriteResult, ApiError> {
    if body.is_empty() {
        return Ok(RewriteResult {
            body: Vec::new(),
            audit: None,
        });
    }

    let Ok(mut value) = serde_json::from_slice::<Value>(&body) else {
        return Ok(RewriteResult {
            body: body.to_vec(),
            audit: None,
        });
    };

    let Some(card_key) = value
        .get("card_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .map(ToOwned::to_owned)
    else {
        return Ok(RewriteResult {
            body: serde_json::to_vec(&value)
                .map_err(|error| ApiError::bad_request(error.to_string()))?,
            audit: None,
        });
    };

    if card_key.is_empty() {
        return Ok(RewriteResult {
            body: serde_json::to_vec(&value)
                .map_err(|error| ApiError::bad_request(error.to_string()))?,
            audit: None,
        });
    }

    let request_summary = summarize_json_body(&value);
    let service_type = value
        .get("service_type")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let account_count = account_count(&value);
    let mapping = state
        .mappings
        .find_one(
            doc! { "distribution_cdk": &card_key, "enabled": true },
            None,
        )
        .await?;

    let audit = if let Some(mapping) = mapping {
        value["card_key"] = json!(mapping.upstream_cdk);
        Some(PendingUsageAudit {
            request_id: Uuid::new_v4().simple().to_string(),
            distribution_cdk: Some(card_key.clone()),
            requested_cdk_masked: mask_secret(&card_key),
            mapping_id: mapping.id,
            cdk_note: mapping.note,
            matched_mapping: true,
            request_method: method.as_str().to_string(),
            request_path: uri.path().to_string(),
            request_query: uri.query().map(ToOwned::to_owned),
            client_ip: client_ip(headers),
            forwarded_for: header_string(headers, "x-forwarded-for")
                .or_else(|| header_string(headers, "forwarded")),
            user_agent: header_string(headers, "user-agent"),
            referer: header_string(headers, "referer"),
            origin: header_string(headers, "origin"),
            accept_language: header_string(headers, "accept-language"),
            service_type,
            account_count,
            request_body_bytes: body.len() as i64,
            request_summary,
            started_ms: unix_millis(),
            created_at: timestamp(),
            created_at_date: bson_now(),
        })
    } else {
        Some(PendingUsageAudit {
            request_id: Uuid::new_v4().simple().to_string(),
            distribution_cdk: None,
            requested_cdk_masked: mask_secret(&card_key),
            mapping_id: None,
            cdk_note: None,
            matched_mapping: false,
            request_method: method.as_str().to_string(),
            request_path: uri.path().to_string(),
            request_query: uri.query().map(ToOwned::to_owned),
            client_ip: client_ip(headers),
            forwarded_for: header_string(headers, "x-forwarded-for")
                .or_else(|| header_string(headers, "forwarded")),
            user_agent: header_string(headers, "user-agent"),
            referer: header_string(headers, "referer"),
            origin: header_string(headers, "origin"),
            accept_language: header_string(headers, "accept-language"),
            service_type,
            account_count,
            request_body_bytes: body.len() as i64,
            request_summary,
            started_ms: unix_millis(),
            created_at: timestamp(),
            created_at_date: bson_now(),
        })
    };

    Ok(RewriteResult {
        body: serde_json::to_vec(&value)
            .map_err(|error| ApiError::bad_request(error.to_string()))?,
        audit,
    })
}

async fn write_usage_log(state: &AppState, audit: PendingUsageAudit, response: UsageResponseAudit) {
    let log = CdkUsageLog {
        id: None,
        request_id: audit.request_id,
        distribution_cdk: audit.distribution_cdk,
        requested_cdk_masked: audit.requested_cdk_masked,
        mapping_id: audit.mapping_id,
        cdk_note: audit.cdk_note,
        matched_mapping: audit.matched_mapping,
        request_method: audit.request_method,
        request_path: audit.request_path,
        request_query: audit.request_query,
        client_ip: audit.client_ip,
        forwarded_for: audit.forwarded_for,
        user_agent: audit.user_agent,
        referer: audit.referer,
        origin: audit.origin,
        accept_language: audit.accept_language,
        service_type: audit.service_type,
        account_count: audit.account_count,
        request_body_bytes: audit.request_body_bytes,
        request_summary: audit.request_summary,
        response_status: response.status,
        response_body_bytes: response.body_bytes,
        response_summary: response.summary,
        error: response.error,
        duration_ms: unix_millis().saturating_sub(audit.started_ms),
        created_at: audit.created_at,
        created_at_date: Some(audit.created_at_date),
    };

    let _ = state.usage_logs.insert_one(log, None).await;
}

struct RewriteResult {
    body: Vec<u8>,
    audit: Option<PendingUsageAudit>,
}

struct PendingUsageAudit {
    request_id: String,
    distribution_cdk: Option<String>,
    requested_cdk_masked: String,
    mapping_id: Option<ObjectId>,
    cdk_note: Option<String>,
    matched_mapping: bool,
    request_method: String,
    request_path: String,
    request_query: Option<String>,
    client_ip: Option<String>,
    forwarded_for: Option<String>,
    user_agent: Option<String>,
    referer: Option<String>,
    origin: Option<String>,
    accept_language: Option<String>,
    service_type: Option<String>,
    account_count: Option<i64>,
    request_body_bytes: i64,
    request_summary: Option<String>,
    started_ms: i64,
    created_at: String,
    created_at_date: BsonDateTime,
}

struct UsageResponseAudit {
    status: Option<i32>,
    body_bytes: Option<i64>,
    summary: Option<String>,
    error: Option<String>,
}

fn header_string(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn client_ip(headers: &HeaderMap) -> Option<String> {
    header_string(headers, "cf-connecting-ip")
        .or_else(|| header_string(headers, "x-real-ip"))
        .or_else(|| {
            header_string(headers, "x-forwarded-for").and_then(|value| {
                value
                    .split(',')
                    .next()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
            })
        })
}

fn account_count(value: &Value) -> Option<i64> {
    value
        .get("accounts_text")
        .and_then(Value::as_str)
        .map(non_empty_line_count)
        .or_else(|| {
            value
                .get("accounts")
                .and_then(Value::as_array)
                .map(|items| items.len() as i64)
        })
}

fn non_empty_line_count(value: &str) -> i64 {
    value.lines().filter(|line| !line.trim().is_empty()).count() as i64
}

fn summarize_json_body(value: &Value) -> Option<String> {
    serde_json::to_string(&redact_value(value))
        .ok()
        .map(truncate)
}

fn redact_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut redacted = serde_json::Map::new();
            for (key, value) in map {
                let lower = key.to_ascii_lowercase();
                let next = if lower == "card_key" {
                    value
                        .as_str()
                        .map(|text| json!(mask_secret(text)))
                        .unwrap_or_else(|| json!("<redacted>"))
                } else if lower == "accounts_text" {
                    value
                        .as_str()
                        .map(|text| {
                            json!(format!("<redacted {} lines>", non_empty_line_count(text)))
                        })
                        .unwrap_or_else(|| json!("<redacted>"))
                } else if is_sensitive_key(&lower) {
                    json!("<redacted>")
                } else {
                    redact_value(value)
                };
                redacted.insert(key.clone(), next);
            }
            Value::Object(redacted)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_value).collect()),
        _ => value.clone(),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    key.contains("account")
        || key.contains("address")
        || key.contains("authorization")
        || key.contains("auxiliary")
        || key.contains("cookie")
        || key.contains("cdk")
        || key.contains("email")
        || key.contains("link")
        || key.contains("mail")
        || key.contains("mobile")
        || key.contains("name")
        || key.contains("phone")
        || key.contains("recovery")
        || key.contains("result")
        || key.contains("secret")
        || key.contains("token")
        || key.contains("totp")
        || key.contains("url")
        || key.contains("user")
        || key.contains("2fa")
        || key.contains("password")
        || key.contains("passwd")
        || key.ends_with("_key")
        || key.ends_with("key")
}

async fn upstream_response(
    response: reqwest::Response,
) -> Result<(Response, UsageResponseAudit), ApiError> {
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut headers = HeaderMap::new();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);

    for (name, value) in response.headers().iter() {
        if !is_forwarded_response_header(name) {
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
    let audit = UsageResponseAudit {
        status: Some(status.as_u16() as i32),
        body_bytes: Some(bytes.len() as i64),
        summary: summarize_response_body(&bytes, content_type.as_deref()),
        error: None,
    };

    Ok(((status, headers, bytes).into_response(), audit))
}

fn summarize_response_body(bytes: &Bytes, content_type: Option<&str>) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }

    let content_type = content_type.unwrap_or_default().to_ascii_lowercase();
    if content_type.contains("json")
        && let Ok(value) = serde_json::from_slice::<Value>(bytes)
    {
        return summarize_json_body(&value);
    }

    if content_type.contains("text")
        || content_type.contains("json")
        || content_type.contains("xml")
        || content_type.contains("html")
    {
        return Some(format!(
            "<redacted textual response, {} bytes>",
            bytes.len()
        ));
    }

    Some(format!("<{} bytes binary response>", bytes.len()))
}

fn mask_secret(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= 8 {
        return "********".to_string();
    }

    let prefix = chars.iter().take(4).collect::<String>();
    let suffix = chars.iter().rev().take(4).collect::<Vec<_>>();
    let suffix = suffix.into_iter().rev().collect::<String>();
    format!("{prefix}...{suffix}")
}

fn truncate(value: String) -> String {
    let mut result = String::new();
    for character in value.chars().take(BODY_SUMMARY_LIMIT) {
        result.push(character);
    }

    if result.len() < value.len() {
        result.push_str("...");
    }

    result
}

fn timestamp() -> String {
    unix_seconds().to_string()
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn bson_now() -> BsonDateTime {
    BsonDateTime::now()
}

fn is_forwarded_request_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "accept" | "accept-language" | "content-type" | "user-agent" | "x-requested-with"
    )
}

fn is_forwarded_response_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "cache-control"
            | "content-disposition"
            | "content-language"
            | "content-type"
            | "etag"
            | "expires"
            | "last-modified"
            | "vary"
    )
}
