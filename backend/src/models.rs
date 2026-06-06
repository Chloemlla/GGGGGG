use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdkMapping {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub distribution_cdk: String,
    pub upstream_cdk: String,
    pub note: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateCdkRequest {
    pub distribution_cdk: Option<String>,
    pub upstream_cdk: String,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CdkListResponse {
    pub items: Vec<CdkMappingResponse>,
}

#[derive(Debug, Serialize)]
pub struct CdkMappingResponse {
    pub id: String,
    pub distribution_cdk: String,
    pub upstream_cdk_masked: String,
    pub note: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminSession {
    #[serde(rename = "_id")]
    pub id: String,
    pub oauth_state: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    pub user: Option<SynapseAdminUser>,
    pub access_expires_at: Option<i64>,
    pub expires_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynapseAdminUser {
    pub sub: Option<String>,
    pub id: Option<String>,
    pub username: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "avatarUrl")]
    pub avatar_url: Option<String>,
    pub role: Option<String>,
    #[serde(rename = "isAdmin")]
    pub is_admin: Option<bool>,
    #[serde(rename = "synapseAdmin")]
    pub synapse_admin: Option<bool>,
    #[serde(rename = "authProvider")]
    pub auth_provider: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<String>,
    #[serde(rename = "accountStatus")]
    pub account_status: Option<String>,
    pub email: Option<String>,
    #[serde(rename = "emailVerified")]
    pub email_verified: Option<bool>,
}

impl SynapseAdminUser {
    pub fn is_authorized_admin(&self) -> bool {
        self.role.as_deref() == Some("admin")
            && self.is_admin == Some(true)
            && self.synapse_admin == Some(true)
            && self.account_status.as_deref() == Some("active")
    }
}

#[derive(Debug, Serialize)]
pub struct AdminAuthStatusResponse {
    pub authenticated: bool,
    pub configured: bool,
    pub login_url: String,
    pub user: Option<SynapseAdminUser>,
}

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub expires_in: Option<i64>,
    pub refresh_expires_in: Option<i64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    pub user: Option<SynapseAdminUser>,
}

#[derive(Debug, Deserialize)]
pub struct OAuthErrorResponse {
    pub error: Option<String>,
    pub error_description: Option<String>,
}
