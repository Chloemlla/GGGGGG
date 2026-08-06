use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    routing::get,
};
use futures_util::TryStreamExt;
use mongodb::{
    Collection, IndexModel,
    bson::doc,
    options::{IndexOptions, UpdateOptions},
};

use crate::{
    config::AppConfig,
    crypto,
    error::{ApiError, ApiResult},
    models::{
        AppConfigEntry, ConfigFieldResponse, ConfigListResponse, ConfigMutationResponse,
        DeleteConfigRequest, SaveConfigRequest,
    },
    oauth,
    state::AppState,
};

const CONFIG_MAX_LENGTH: usize = 4_096;

#[derive(Clone, Copy)]
enum FieldKind {
    Text,
    Bool,
    Count,
}

struct ConfigFieldMeta {
    key: &'static str,
    label: &'static str,
    group: &'static str,
    kind: FieldKind,
    secret: bool,
    bootstrap: bool,
    hot_reload: bool,
    allow_empty: bool,
}

fn catalog() -> &'static [ConfigFieldMeta] {
    &[
        ConfigFieldMeta {
            key: "BIND_ADDR",
            label: "服务监听地址",
            group: "核心",
            kind: FieldKind::Text,
            secret: false,
            bootstrap: false,
            hot_reload: false,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "FRONTEND_DIST_DIR",
            label: "前端静态资源目录",
            group: "核心",
            kind: FieldKind::Text,
            secret: false,
            bootstrap: false,
            hot_reload: false,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "UPSTREAM_BASE_URL",
            label: "上游代理 Base URL",
            group: "核心",
            kind: FieldKind::Text,
            secret: false,
            bootstrap: false,
            hot_reload: true,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "CORS_ALLOWED_ORIGINS",
            label: "允许跨域来源（逗号分隔，留空关闭）",
            group: "核心",
            kind: FieldKind::Text,
            secret: false,
            bootstrap: false,
            hot_reload: false,
            allow_empty: true,
        },
        ConfigFieldMeta {
            key: "APP_BASE_URL",
            label: "应用对外访问地址",
            group: "核心",
            kind: FieldKind::Text,
            secret: false,
            bootstrap: false,
            hot_reload: false,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "CDK_USAGE_LOG_TTL_SECONDS",
            label: "CDK 使用日志保留时长（秒）",
            group: "核心",
            kind: FieldKind::Count,
            secret: false,
            bootstrap: false,
            hot_reload: false,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "MONGODB_URI",
            label: "MongoDB 连接串",
            group: "数据库",
            kind: FieldKind::Text,
            secret: true,
            bootstrap: true,
            hot_reload: false,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "MONGODB_DATABASE",
            label: "MongoDB 数据库名",
            group: "数据库",
            kind: FieldKind::Text,
            secret: false,
            bootstrap: true,
            hot_reload: false,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "ADMIN_TOKEN_ENCRYPTION_KEY",
            label: "敏感数据加密密钥",
            group: "安全",
            kind: FieldKind::Text,
            secret: true,
            bootstrap: true,
            hot_reload: false,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "TRUST_PROXY_HEADERS",
            label: "信任反向代理头",
            group: "安全",
            kind: FieldKind::Bool,
            secret: false,
            bootstrap: false,
            hot_reload: true,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "PROXY_RATE_LIMIT_PER_MINUTE",
            label: "代理请求限流（次/分钟）",
            group: "安全",
            kind: FieldKind::Count,
            secret: false,
            bootstrap: false,
            hot_reload: true,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "LOGIN_RATE_LIMIT_PER_MINUTE",
            label: "登录请求限流（次/分钟）",
            group: "安全",
            kind: FieldKind::Count,
            secret: false,
            bootstrap: false,
            hot_reload: true,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "ADMIN_SESSION_COOKIE",
            label: "后台会话 Cookie 名",
            group: "安全",
            kind: FieldKind::Text,
            secret: false,
            bootstrap: false,
            hot_reload: false,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "ADMIN_SESSION_TTL_SECONDS",
            label: "后台会话有效期（秒）",
            group: "安全",
            kind: FieldKind::Count,
            secret: false,
            bootstrap: false,
            hot_reload: false,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "SYNAPSE_OAUTH_BASE_URL",
            label: "Synapse 服务地址",
            group: "Synapse OAuth",
            kind: FieldKind::Text,
            secret: false,
            bootstrap: false,
            hot_reload: false,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "SYNAPSE_OAUTH_CLIENT_ID",
            label: "Synapse Client ID",
            group: "Synapse OAuth",
            kind: FieldKind::Text,
            secret: false,
            bootstrap: false,
            hot_reload: false,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "SYNAPSE_OAUTH_CLIENT_SECRET",
            label: "Synapse Client Secret",
            group: "Synapse OAuth",
            kind: FieldKind::Text,
            secret: true,
            bootstrap: false,
            hot_reload: false,
            allow_empty: false,
        },
        ConfigFieldMeta {
            key: "SYNAPSE_OAUTH_REDIRECT_URI",
            label: "授权回调地址（留空自动推导）",
            group: "Synapse OAuth",
            kind: FieldKind::Text,
            secret: false,
            bootstrap: false,
            hot_reload: false,
            allow_empty: true,
        },
        ConfigFieldMeta {
            key: "SYNAPSE_OAUTH_SCOPES",
            label: "授权范围",
            group: "Synapse OAuth",
            kind: FieldKind::Text,
            secret: false,
            bootstrap: false,
            hot_reload: false,
            allow_empty: false,
        },
    ]
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/configs", get(list_configs).post(save_config).delete(delete_config))
}

pub async fn ensure_index(
    configs: &Collection<AppConfigEntry>,
) -> Result<(), mongodb::error::Error> {
    let options = IndexOptions::builder().unique(true).build();
    let index = IndexModel::builder()
        .keys(doc! { "key": 1 })
        .options(options)
        .build();
    configs.create_index(index, None).await?;
    Ok(())
}

/// Decrypts every persisted override. Corrupt entries (e.g. after a key change)
/// are skipped with a warning instead of blocking startup.
pub async fn load_overrides(
    configs: &Collection<AppConfigEntry>,
    secret: &str,
) -> Result<HashMap<String, String>, mongodb::error::Error> {
    let mut cursor = configs.find(doc! {}, None).await?;
    let mut overrides = HashMap::new();
    while let Some(entry) = cursor.try_next().await? {
        match crypto::decrypt_value(secret, &entry.value_encrypted) {
            Ok(value) => {
                overrides.insert(entry.key.clone(), value);
            }
            Err(error) => {
                tracing::warn!("解密配置 {} 失败，已跳过: {error:?}", entry.key);
            }
        }
    }
    Ok(overrides)
}

async fn list_configs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<ConfigListResponse> {
    oauth::require_admin(&state, &headers).await?;
    let secret = state.oauth.token_encryption_secret().to_string();

    let mut stored = HashMap::new();
    let mut cursor = state.configs.find(doc! {}, None).await?;
    while let Some(entry) = cursor.try_next().await? {
        stored.insert(entry.key.clone(), entry);
    }

    let env_defaults = state.env_defaults.as_ref();
    let items = catalog()
        .iter()
        .map(|meta| {
            let default_value = env_default(env_defaults, meta.key);
            let effective = match stored.get(meta.key) {
                Some(entry) => crypto::decrypt_value(&secret, &entry.value_encrypted)
                    .unwrap_or_else(|_| default_value.clone()),
                None => default_value,
            };
            ConfigFieldResponse {
                key: meta.key.to_string(),
                label: meta.label.to_string(),
                group: meta.group.to_string(),
                kind: kind_name(meta.kind),
                secret: meta.secret,
                bootstrap: meta.bootstrap,
                hot_reload: meta.hot_reload,
                effective: if meta.secret {
                    mask_value(&effective)
                } else {
                    effective
                },
                overridden: stored.contains_key(meta.key),
                updated_at: stored.get(meta.key).map(|entry| entry.updated_at.clone()),
            }
        })
        .collect();

    Ok(Json(ConfigListResponse { items }))
}

async fn save_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SaveConfigRequest>,
) -> ApiResult<ConfigMutationResponse> {
    oauth::require_admin(&state, &headers).await?;

    let key = payload.key.trim().to_string();
    let meta = catalog()
        .iter()
        .find(|meta| meta.key == key)
        .ok_or_else(|| ApiError::bad_request(format!("未知配置项: {key}")))?;
    if meta.bootstrap {
        return Err(ApiError::bad_request(
            "该配置项必须通过容器环境变量设置，不能在此修改",
        ));
    }

    let value = validate_value(meta, &payload.value)?;
    let secret = state.oauth.token_encryption_secret().to_string();
    let value_encrypted = crypto::encrypt_value(&secret, &value)?;
    let now = unix_timestamp().to_string();

    state
        .configs
        .update_one(
            doc! { "key": &key },
            doc! {
                "$set": {
                    "key": &key,
                    "value_encrypted": &value_encrypted,
                    "updated_at": &now,
                }
            },
            UpdateOptions::builder().upsert(true).build(),
        )
        .await?;

    let applied = apply_hot_reload(&state, meta, &value);
    Ok(Json(ConfigMutationResponse {
        message: if applied {
            "已保存并立即生效".to_string()
        } else {
            "已保存，重启容器后生效".to_string()
        },
        applied,
        restart_required: !applied,
    }))
}

async fn delete_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<DeleteConfigRequest>,
) -> ApiResult<ConfigMutationResponse> {
    oauth::require_admin(&state, &headers).await?;

    let key = payload.key.trim().to_string();
    let meta = catalog()
        .iter()
        .find(|meta| meta.key == key)
        .ok_or_else(|| ApiError::bad_request(format!("未知配置项: {key}")))?;
    if meta.bootstrap {
        return Err(ApiError::bad_request(
            "该配置项必须通过容器环境变量设置，不能删除",
        ));
    }

    let result = state.configs.delete_one(doc! { "key": &key }, None).await?;
    if result.deleted_count == 0 {
        return Ok(Json(ConfigMutationResponse {
            message: "该配置项未设置覆盖，当前使用容器默认值".to_string(),
            applied: false,
            restart_required: false,
        }));
    }

    let default_value = env_default(state.env_defaults.as_ref(), &key);
    let applied = apply_hot_reload(&state, meta, &default_value);
    Ok(Json(ConfigMutationResponse {
        message: if applied {
            "已恢复容器默认值并立即生效".to_string()
        } else {
            "已恢复容器默认值，重启容器后生效".to_string()
        },
        applied,
        restart_required: !applied,
    }))
}

fn validate_value(meta: &ConfigFieldMeta, raw: &str) -> Result<String, ApiError> {
    let value = raw.trim().to_string();
    if !meta.allow_empty && value.is_empty() {
        return Err(ApiError::bad_request(format!("{} 不能为空", meta.label)));
    }
    if value.chars().count() > CONFIG_MAX_LENGTH {
        return Err(ApiError::bad_request(format!(
            "{} 超出长度限制",
            meta.label
        )));
    }

    match meta.kind {
        FieldKind::Bool => {
            let normalized = value.to_ascii_lowercase();
            let valid = matches!(
                normalized.as_str(),
                "1" | "true"
                    | "yes"
                    | "y"
                    | "on"
                    | "t"
                    | "0"
                    | "false"
                    | "no"
                    | "n"
                    | "off"
                    | "f"
            );
            if !valid {
                return Err(ApiError::bad_request(format!(
                    "{} 需要 true/false",
                    meta.label
                )));
            }
        }
        FieldKind::Count => {
            let parsed = value
                .parse::<u64>()
                .map_err(|_| ApiError::bad_request(format!("{} 需要正整数", meta.label)))?;
            if parsed < 1 {
                return Err(ApiError::bad_request(format!(
                    "{} 需要至少为 1",
                    meta.label
                )));
            }
        }
        FieldKind::Text => {}
    }

    Ok(value)
}

fn apply_hot_reload(state: &AppState, meta: &ConfigFieldMeta, value: &str) -> bool {
    if !meta.hot_reload {
        return false;
    }

    match meta.key {
        "UPSTREAM_BASE_URL" => {
            state
                .runtime
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .upstream_base_url = value.to_string();
        }
        "TRUST_PROXY_HEADERS" => {
            let parsed = parse_bool(value);
            state
                .runtime
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .trust_proxy_headers = parsed;
        }
        "PROXY_RATE_LIMIT_PER_MINUTE" => {
            if let Ok(limit) = value.parse::<u64>() {
                state.proxy_limiter.set_limit(limit);
            }
        }
        "LOGIN_RATE_LIMIT_PER_MINUTE" => {
            if let Ok(limit) = value.parse::<u64>() {
                state.login_limiter.set_limit(limit);
            }
        }
        _ => return false,
    }
    true
}

fn env_default(config: &AppConfig, key: &str) -> String {
    match key {
        "BIND_ADDR" => config.bind_addr.clone(),
        "FRONTEND_DIST_DIR" => config.frontend_dir.clone(),
        "UPSTREAM_BASE_URL" => config.upstream_base_url.clone(),
        "CORS_ALLOWED_ORIGINS" => config.cors_allowed_origins.join(","),
        "APP_BASE_URL" => config.oauth.app_base_url.clone(),
        "CDK_USAGE_LOG_TTL_SECONDS" => config.usage_log_ttl_seconds.to_string(),
        "MONGODB_URI" => config.mongo_uri.clone(),
        "MONGODB_DATABASE" => config.mongo_database.clone(),
        "ADMIN_TOKEN_ENCRYPTION_KEY" => config.oauth.token_encryption_key.clone(),
        "TRUST_PROXY_HEADERS" => config.trust_proxy_headers.to_string(),
        "PROXY_RATE_LIMIT_PER_MINUTE" => config.proxy_rate_limit_per_minute.to_string(),
        "LOGIN_RATE_LIMIT_PER_MINUTE" => config.login_rate_limit_per_minute.to_string(),
        "ADMIN_SESSION_COOKIE" => config.oauth.session_cookie_name.clone(),
        "ADMIN_SESSION_TTL_SECONDS" => config.oauth.session_ttl_seconds.to_string(),
        "SYNAPSE_OAUTH_BASE_URL" => config.oauth.base_url.clone(),
        "SYNAPSE_OAUTH_CLIENT_ID" => config.oauth.client_id.clone(),
        "SYNAPSE_OAUTH_CLIENT_SECRET" => config.oauth.client_secret.clone(),
        "SYNAPSE_OAUTH_REDIRECT_URI" => config.oauth.redirect_uri_override.clone(),
        "SYNAPSE_OAUTH_SCOPES" => config.oauth.scopes.clone(),
        _ => String::new(),
    }
}

fn kind_name(kind: FieldKind) -> String {
    match kind {
        FieldKind::Text => "text".to_string(),
        FieldKind::Bool => "bool".to_string(),
        FieldKind::Count => "count".to_string(),
    }
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "y" | "on" | "t"
    )
}

fn mask_value(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= 8 {
        return "********".to_string();
    }

    let prefix = chars.iter().take(4).collect::<String>();
    let suffix = chars.iter().rev().take(4).collect::<Vec<_>>();
    let suffix = suffix.into_iter().rev().collect::<String>();
    format!("{prefix}...{suffix}")
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::{FieldKind, validate_value};

    fn text_meta() -> super::ConfigFieldMeta {
        super::ConfigFieldMeta {
            key: "APP_BASE_URL",
            label: "应用对外访问地址",
            group: "核心",
            kind: FieldKind::Text,
            secret: false,
            bootstrap: false,
            hot_reload: false,
            allow_empty: false,
        }
    }

    #[test]
    fn trims_saved_values() {
        assert_eq!(
            validate_value(&text_meta(), "  https://gemini.chloemlla.com  ").expect("valid"),
            "https://gemini.chloemlla.com"
        );
    }

    #[test]
    fn rejects_empty_text_values() {
        assert!(validate_value(&text_meta(), "   ").is_err());
    }

    #[test]
    fn rejects_oversized_values() {
        let huge = "x".repeat(super::CONFIG_MAX_LENGTH + 1);
        assert!(validate_value(&text_meta(), &huge).is_err());
    }
}
