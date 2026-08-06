use std::net::SocketAddr;

use axum::{
    Json, Router,
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use mongodb::bson::{DateTime as BsonDateTime, doc};
use sha2::{Digest, Sha256};
use url::{Url, form_urlencoded};
use uuid::Uuid;

use crate::{
    config::OAuthConfig,
    crypto,
    error::ApiError,
    models::{
        AdminAuthStatusResponse, AdminSession, MessageResponse, OAuthCallbackQuery,
        OAuthErrorResponse, OAuthTokenResponse, SynapseAdminUser,
    },
    state::AppState,
};

const LOGIN_URL: &str = "/api/admin/auth/login";
const PENDING_SESSION_TTL_SECONDS: i64 = 600;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/status", get(auth_status))
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", post(logout))
}

pub async fn require_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<SynapseAdminUser, ApiError> {
    if !state.oauth.enabled() {
        return Err(ApiError::service_unavailable(
            "Synapse OAuth 未配置，后台管理不可用",
        ));
    }

    let session_id = session_id_from_headers(headers, &effective_cookie_name(&state.oauth))
        .ok_or_else(|| ApiError::unauthorized("请先通过 Synapse 授权登录"))?;
    let mut session = load_session(state, &session_id).await?;
    let now = unix_timestamp();

    if session.expires_at <= now {
        delete_stored_session(state, &session.id).await;
        return Err(ApiError::unauthorized("Synapse 授权已过期，请重新登录"));
    }

    let mut refreshed = false;
    if should_refresh_access_token(&session, now) {
        if let Err(error) = refresh_admin_session(state, &mut session).await {
            if is_auth_failure(&error) {
                delete_stored_session(state, &session.id).await;
            }
            return Err(error);
        }
        refreshed = true;
    }

    let user = match userinfo_for_session(state, &mut session).await {
        Ok(user) => user,
        Err(error) if can_retry_with_refresh(&error, refreshed, &session) => {
            if let Err(refresh_error) = refresh_admin_session(state, &mut session).await {
                if is_auth_failure(&refresh_error) {
                    delete_stored_session(state, &session.id).await;
                }
                return Err(refresh_error);
            }
            userinfo_for_session(state, &mut session).await?
        }
        Err(error) => {
            if is_auth_failure(&error) {
                delete_stored_session(state, &session.id).await;
            }
            return Err(error);
        }
    };

    if !user.is_authorized_actor() {
        delete_stored_session(state, &session.id).await;
        return Err(ApiError::forbidden(
            "当前 Synapse 用户不是有效的 active admin 或 trusted",
        ));
    }

    session.user = Some(user.clone());
    session.updated_at = unix_timestamp();
    state
        .admin_sessions
        .replace_one(doc! { "_id": &session.id }, &session, None)
        .await?;

    Ok(user)
}

async fn auth_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if !state.oauth.enabled() {
        return Ok(Json(AdminAuthStatusResponse {
            authenticated: false,
            configured: false,
            login_url: LOGIN_URL.to_string(),
            message: Some("Synapse OAuth 未配置，后台管理不可用".to_string()),
            user: None,
        })
        .into_response());
    }

    match require_admin(&state, &headers).await {
        Ok(user) => Ok(Json(AdminAuthStatusResponse {
            authenticated: true,
            configured: true,
            login_url: LOGIN_URL.to_string(),
            message: None,
            user: Some(user),
        })
        .into_response()),
        Err(error) if is_auth_failure(&error) => {
            let response = Json(AdminAuthStatusResponse {
                authenticated: false,
                configured: true,
                login_url: LOGIN_URL.to_string(),
                message: Some(error.detail().to_string()),
                user: None,
            })
            .into_response();
            Ok(with_set_cookie(response, clear_session_cookie(&state.oauth)))
        }
        Err(error) => Err(error),
    }
}

async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    match login_inner(&state, addr, &headers).await {
        Ok(response) => response,
        Err(error) => redirect_auth_error(&state, error.detail()),
    }
}

async fn login_inner(
    state: &AppState,
    addr: SocketAddr,
    headers: &HeaderMap,
) -> Result<Response, ApiError> {
    if !state.oauth.enabled() {
        return Err(ApiError::service_unavailable(
            "Synapse OAuth 未配置，无法发起授权",
        ));
    }

    let client_ip = effective_client_ip(state, headers, addr);
    let rate_status = state.login_limiter.check(&client_ip);
    if !rate_status.allowed {
        return Err(ApiError::rate_limited(rate_status.retry_after_secs));
    }

    let now = unix_timestamp();
    let session_id = Uuid::new_v4().simple().to_string();
    let oauth_state = Uuid::new_v4().simple().to_string();
    let session = AdminSession {
        id: session_storage_id(&session_id),
        oauth_state: oauth_state.clone(),
        access_token_encrypted: None,
        refresh_token_encrypted: None,
        scope: None,
        user: None,
        access_expires_at: None,
        expires_at: now + PENDING_SESSION_TTL_SECONDS,
        expires_at_date: Some(bson_from_unix_seconds(now + PENDING_SESSION_TTL_SECONDS)),
        created_at: now,
        updated_at: now,
    };

    state.admin_sessions.insert_one(session, None).await?;

    let authorize_url = authorization_url(state, &oauth_state)?;
    let response = Redirect::to(&authorize_url).into_response();
    Ok(with_set_cookie(
        response,
        session_cookie(&state.oauth, &session_id, PENDING_SESSION_TTL_SECONDS),
    ))
}

async fn callback(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<OAuthCallbackQuery>,
) -> Response {
    match callback_inner(&state, addr, &headers, query).await {
        Ok(response) => response,
        Err(error) => with_set_cookie(
            redirect_auth_error(&state, error.detail()),
            clear_session_cookie(&state.oauth),
        ),
    }
}

async fn callback_inner(
    state: &AppState,
    addr: SocketAddr,
    headers: &HeaderMap,
    query: OAuthCallbackQuery,
) -> Result<Response, ApiError> {
    let client_ip = effective_client_ip(state, headers, addr);
    let rate_status = state.login_limiter.check(&client_ip);
    if !rate_status.allowed {
        return Err(ApiError::rate_limited(rate_status.retry_after_secs));
    }

    if let Some(provider_error) = query.error {
        let detail = query.error_description.unwrap_or(provider_error);
        return Err(ApiError::forbidden(detail));
    }

    let code = query
        .code
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("OAuth 回调缺少 authorization code"))?;
    let returned_state = query
        .state
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("OAuth 回调缺少 state"))?;
    let session_id = session_id_from_headers(headers, &effective_cookie_name(&state.oauth))
        .ok_or_else(|| ApiError::unauthorized("OAuth 登录会话不存在"))?;
    let mut session = load_session(state, &session_id).await?;

    if session.oauth_state != returned_state {
        delete_stored_session(state, &session.id).await;
        return Err(ApiError::forbidden("OAuth state 校验失败"));
    }

    if session.expires_at <= unix_timestamp() {
        delete_stored_session(state, &session.id).await;
        return Err(ApiError::unauthorized("OAuth 登录会话已过期"));
    }

    let token = exchange_authorization_code(state, &code).await?;
    let user = match token.user.clone() {
        Some(user) => user,
        None => fetch_userinfo(state, &token.access_token).await?,
    };

    if !user.is_authorized_actor() {
        delete_stored_session(state, &session.id).await;
        return Err(ApiError::forbidden(
            "当前 Synapse 用户不是有效的 active admin 或 trusted",
        ));
    }

    apply_token_to_session(state, &mut session, token)?;
    session.user = Some(user);
    session.updated_at = unix_timestamp();
    state
        .admin_sessions
        .replace_one(doc! { "_id": &session.id }, &session, None)
        .await?;

    let max_age = (session.expires_at - unix_timestamp()).max(60);
    let response = Redirect::to("/admin").into_response();
    Ok(with_set_cookie(
        response,
        session_cookie(&state.oauth, &session_id, max_age),
    ))
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(session_id) = session_id_from_headers(&headers, &effective_cookie_name(&state.oauth))
    {
        delete_session_by_cookie_id(&state, &session_id).await;
    }

    with_set_cookie(
        Json(MessageResponse {
            message: "已退出 Synapse 授权".to_string(),
        })
        .into_response(),
        clear_session_cookie(&state.oauth),
    )
}

async fn load_session(state: &AppState, session_id: &str) -> Result<AdminSession, ApiError> {
    state
        .admin_sessions
        .find_one(doc! { "_id": session_storage_id(session_id) }, None)
        .await?
        .ok_or_else(|| ApiError::unauthorized("请先通过 Synapse 授权登录"))
}

async fn userinfo_for_session(
    state: &AppState,
    session: &mut AdminSession,
) -> Result<SynapseAdminUser, ApiError> {
    let access_token = session
        .access_token_encrypted
        .as_deref()
        .ok_or_else(|| ApiError::unauthorized("Synapse 授权 token 不存在"))?;
    let access_token = crypto::decrypt_value(state.oauth.token_encryption_secret(), access_token)?;
    fetch_userinfo(state, &access_token).await
}

async fn fetch_userinfo(
    state: &AppState,
    access_token: &str,
) -> Result<SynapseAdminUser, ApiError> {
    let response = state
        .http
        .get(state.oauth.userinfo_endpoint())
        .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|error| ApiError::upstream(format!("Synapse userinfo 请求失败: {error}")))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| ApiError::upstream(format!("读取 Synapse userinfo 响应失败: {error}")))?;

    if !status.is_success() {
        return Err(oauth_status_error(status, &text));
    }

    if looks_like_oauth_error(&text) {
        return Err(oauth_status_error(StatusCode::UNAUTHORIZED, &text));
    }

    serde_json::from_str::<SynapseAdminUser>(&text)
        .map_err(|error| ApiError::upstream(format!("Synapse userinfo 响应格式无效: {error}")))
}

async fn exchange_authorization_code(
    state: &AppState,
    code: &str,
) -> Result<OAuthTokenResponse, ApiError> {
    exchange_token(
        state,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", state.oauth.client_id.as_str()),
            ("client_secret", state.oauth.client_secret.as_str()),
            ("code", code),
            ("redirect_uri", state.oauth.redirect_uri().as_str()),
        ],
    )
    .await
}

async fn exchange_refresh_token(
    state: &AppState,
    refresh_token: &str,
) -> Result<OAuthTokenResponse, ApiError> {
    exchange_token(
        state,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", state.oauth.client_id.as_str()),
            ("client_secret", state.oauth.client_secret.as_str()),
            ("refresh_token", refresh_token),
        ],
    )
    .await
}

async fn exchange_token(
    state: &AppState,
    params: &[(&str, &str)],
) -> Result<OAuthTokenResponse, ApiError> {
    let response = state
        .http
        .post(state.oauth.token_endpoint())
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(form_body(params))
        .send()
        .await
        .map_err(|error| ApiError::upstream(format!("Synapse token 请求失败: {error}")))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| ApiError::upstream(format!("读取 Synapse token 响应失败: {error}")))?;

    if !status.is_success() {
        return Err(oauth_status_error(status, &text));
    }

    if looks_like_oauth_error(&text) {
        return Err(oauth_status_error(StatusCode::UNAUTHORIZED, &text));
    }

    serde_json::from_str::<OAuthTokenResponse>(&text)
        .map_err(|error| ApiError::upstream(format!("Synapse token 响应格式无效: {error}")))
}

async fn refresh_admin_session(
    state: &AppState,
    session: &mut AdminSession,
) -> Result<(), ApiError> {
    let refresh_token = session
        .refresh_token_encrypted
        .as_deref()
        .ok_or_else(|| ApiError::unauthorized("Synapse 授权 refresh token 不存在"))?;
    let refresh_token =
        crypto::decrypt_value(state.oauth.token_encryption_secret(), refresh_token)?;
    let token = exchange_refresh_token(state, &refresh_token).await?;
    apply_token_to_session(state, session, token)?;
    Ok(())
}

fn apply_token_to_session(
    state: &AppState,
    session: &mut AdminSession,
    token: OAuthTokenResponse,
) -> Result<(), ApiError> {
    let now = unix_timestamp();
    let access_ttl = token.expires_in.unwrap_or(7_200).max(60);
    let session_ttl = token
        .refresh_expires_in
        .or(token.expires_in)
        .unwrap_or(state.oauth.session_ttl_seconds)
        .min(state.oauth.session_ttl_seconds)
        .max(60);

    session.access_token_encrypted =
        Some(crypto::encrypt_value(state.oauth.token_encryption_secret(), &token.access_token)?);
    if let Some(refresh_token) = token.refresh_token {
        session.refresh_token_encrypted =
            Some(crypto::encrypt_value(state.oauth.token_encryption_secret(), &refresh_token)?);
    }
    if let Some(scope) = token.scope {
        session.scope = Some(scope);
    }
    if let Some(user) = token.user {
        session.user = Some(user);
    }
    session.access_expires_at = Some(now + access_ttl);
    session.expires_at = now + session_ttl;
    session.expires_at_date = Some(bson_from_unix_seconds(session.expires_at));
    session.updated_at = now;
    Ok(())
}

fn authorization_url(state: &AppState, oauth_state: &str) -> Result<String, ApiError> {
    let mut url = Url::parse(&state.oauth.authorize_endpoint())
        .map_err(|error| ApiError::service_unavailable(format!("OAuth 授权地址无效: {error}")))?;

    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &state.oauth.client_id)
        .append_pair("redirect_uri", &state.oauth.redirect_uri())
        .append_pair("scope", &state.oauth.scopes)
        .append_pair("state", oauth_state);

    Ok(url.to_string())
}

fn form_body(params: &[(&str, &str)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in params {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

fn oauth_status_error(status: StatusCode, text: &str) -> ApiError {
    let parsed_error = serde_json::from_str::<OAuthErrorResponse>(text).ok();
    let error_code = parsed_error
        .as_ref()
        .and_then(|error| error.error.as_deref())
        .map(ToOwned::to_owned);
    let detail = parsed_error
        .and_then(|error| error.error_description.or(error.error))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            if text.trim().is_empty() {
                "Synapse OAuth 请求失败".to_string()
            } else {
                text.to_string()
            }
        });

    if is_oauth_session_rejection(status, error_code.as_deref(), &detail) {
        ApiError::unauthorized(detail)
    } else {
        tracing::error!("Synapse OAuth 请求失败，upstream 响应: {text}");
        ApiError::upstream("Synapse OAuth 请求失败")
    }
}

fn looks_like_oauth_error(text: &str) -> bool {
    serde_json::from_str::<OAuthErrorResponse>(text)
        .ok()
        .and_then(|error| error.error)
        .is_some()
}

fn is_oauth_session_rejection(status: StatusCode, error_code: Option<&str>, detail: &str) -> bool {
    let code = error_code.unwrap_or_default();
    let detail = detail.to_ascii_lowercase();

    matches!(
        code,
        "invalid_token" | "invalid_grant" | "access_denied" | "insufficient_scope"
    ) || matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        || (status == StatusCode::BAD_REQUEST
            && (detail.contains("invalid_token")
                || detail.contains("invalid_grant")
                || detail.contains("access_denied")
                || detail.contains("revoked")
                || detail.contains("expired")))
}

fn can_retry_with_refresh(error: &ApiError, refreshed: bool, session: &AdminSession) -> bool {
    !refreshed && session.refresh_token_encrypted.is_some() && is_auth_failure(error)
}

fn should_refresh_access_token(session: &AdminSession, now: i64) -> bool {
    session.access_token_encrypted.is_none() || session.access_expires_at.unwrap_or(0) <= now + 30
}

fn is_auth_failure(error: &ApiError) -> bool {
    matches!(
        error.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    )
}

async fn delete_session_by_cookie_id(state: &AppState, session_id: &str) {
    delete_stored_session(state, &session_storage_id(session_id)).await;
}

async fn delete_stored_session(state: &AppState, storage_id: &str) {
    let _ = state
        .admin_sessions
        .delete_one(doc! { "_id": storage_id }, None)
        .await;
}

fn session_storage_id(session_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn session_id_from_headers(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;

    cookie_header.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == cookie_name && !value.is_empty()).then(|| value.to_string())
    })
}

fn effective_cookie_name(oauth: &OAuthConfig) -> String {
    if oauth.cookie_secure() {
        format!("__Host-{}", oauth.session_cookie_name)
    } else {
        oauth.session_cookie_name.clone()
    }
}

fn session_cookie(oauth: &OAuthConfig, session_id: &str, max_age: i64) -> String {
    let cookie_name = effective_cookie_name(oauth);
    if oauth.cookie_secure() {
        format!(
            "{cookie_name}={session_id}; Path=/; Max-Age={}; HttpOnly; Secure; SameSite=Lax",
            max_age.max(0)
        )
    } else {
        format!(
            "{cookie_name}={session_id}; Path=/api/admin; Max-Age={}; HttpOnly; SameSite=Lax",
            max_age.max(0)
        )
    }
}

fn clear_session_cookie(oauth: &OAuthConfig) -> String {
    let cookie_name = effective_cookie_name(oauth);
    if oauth.cookie_secure() {
        format!("{cookie_name}=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax")
    } else {
        format!("{cookie_name}=; Path=/api/admin; Max-Age=0; HttpOnly; SameSite=Lax")
    }
}

fn effective_client_ip(state: &AppState, headers: &HeaderMap, addr: SocketAddr) -> String {
    if !state.trust_proxy_headers() {
        return addr.ip().to_string();
    }

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
        .unwrap_or_else(|| addr.ip().to_string())
}

fn header_string(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn bson_from_unix_seconds(value: i64) -> BsonDateTime {
    BsonDateTime::from_millis(value.saturating_mul(1000))
}

fn with_set_cookie(mut response: Response, cookie: String) -> Response {
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

fn redirect_auth_error(state: &AppState, detail: &str) -> Response {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    serializer.append_pair("auth_error", detail);
    let target = format!("/admin?{}", serializer.finish());
    let response = Redirect::to(&target).into_response();

    if state.oauth.enabled() {
        response
    } else {
        with_set_cookie(response, clear_session_cookie(&state.oauth))
    }
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, StatusCode, header};

    use crate::config::OAuthConfig;

    use super::{
        clear_session_cookie, effective_cookie_name, is_auth_failure, oauth_status_error,
        session_cookie, session_id_from_headers,
    };

    #[test]
    fn treats_oauth_invalid_token_as_auth_failure() {
        let error = oauth_status_error(
            StatusCode::BAD_GATEWAY,
            r#"{"error":"invalid_token","error_description":"token revoked"}"#,
        );

        assert!(is_auth_failure(&error));
    }

    #[test]
    fn keeps_provider_outage_as_upstream_error() {
        let error = oauth_status_error(StatusCode::BAD_GATEWAY, "upstream timeout");

        assert_eq!(error.status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn uses_host_prefix_only_when_secure() {
        assert_eq!(effective_cookie_name(&dev_oauth()), "synapse_admin_session");
        assert_eq!(
            effective_cookie_name(&secure_oauth()),
            "__Host-synapse_admin_session"
        );
    }

    #[test]
    fn builds_secure_session_cookie_with_host_prefix() {
        let cookie = session_cookie(&secure_oauth(), "session-id-1", 120);
        assert_eq!(
            cookie,
            "__Host-synapse_admin_session=session-id-1; Path=/; Max-Age=120; HttpOnly; Secure; SameSite=Lax"
        );
    }

    #[test]
    fn builds_dev_session_cookie_without_prefix() {
        let cookie = session_cookie(&dev_oauth(), "session-id-1", 120);
        assert_eq!(
            cookie,
            "synapse_admin_session=session-id-1; Path=/api/admin; Max-Age=120; HttpOnly; SameSite=Lax"
        );
    }

    #[test]
    fn clears_secure_session_cookie_with_matching_rules() {
        let cookie = clear_session_cookie(&secure_oauth());
        assert_eq!(
            cookie,
            "__Host-synapse_admin_session=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax"
        );
    }

    #[test]
    fn reads_session_id_using_host_prefixed_cookie_name() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "__Host-synapse_admin_session=abc-123; other=1".parse().unwrap(),
        );
        assert_eq!(
            session_id_from_headers(&headers, &effective_cookie_name(&secure_oauth())),
            Some("abc-123".to_string())
        );
    }

    fn dev_oauth() -> OAuthConfig {
        OAuthConfig {
            app_base_url: "http://localhost:8080".to_string(),
            base_url: "https://tts.chloemlla.com".to_string(),
            client_id: "syn_client_test".to_string(),
            client_secret: "syn_secret_test".to_string(),
            redirect_uri_override: String::new(),
            scopes: "openid profile email admin:identity".to_string(),
            session_cookie_name: "synapse_admin_session".to_string(),
            session_ttl_seconds: 2_592_000,
            token_encryption_key: String::new(),
        }
    }

    fn secure_oauth() -> OAuthConfig {
        let mut oauth = dev_oauth();
        oauth.redirect_uri_override =
            "https://gemini.chloemlla.com/api/admin/auth/callback".to_string();
        oauth
    }
}
