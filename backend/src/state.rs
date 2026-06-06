use mongodb::Collection;
use reqwest::Client as HttpClient;

use crate::{
    config::OAuthConfig,
    models::{AdminSession, CdkMapping, CdkUsageLog},
};

#[derive(Clone)]
pub struct AppState {
    pub admin_sessions: Collection<AdminSession>,
    pub http: HttpClient,
    pub mappings: Collection<CdkMapping>,
    pub oauth: OAuthConfig,
    pub usage_logs: Collection<CdkUsageLog>,
    pub upstream_base_url: String,
}
