use std::sync::{Arc, RwLock};

use mongodb::Collection;
use reqwest::Client as HttpClient;

use crate::{
    config::{AppConfig, OAuthConfig},
    models::{AdminSession, AppConfigEntry, CdkMapping, CdkUsageLog},
    rate_limit::FixedWindow,
};

#[derive(Clone)]
pub struct AppState {
    pub admin_sessions: Collection<AdminSession>,
    pub configs: Collection<AppConfigEntry>,
    pub env_defaults: Arc<AppConfig>,
    pub http: HttpClient,
    pub login_limiter: Arc<FixedWindow>,
    pub mappings: Collection<CdkMapping>,
    pub oauth: OAuthConfig,
    pub proxy_limiter: Arc<FixedWindow>,
    pub runtime: Arc<RwLock<RuntimeConfig>>,
    pub usage_logs: Collection<CdkUsageLog>,
}

#[derive(Clone)]
pub struct RuntimeConfig {
    pub upstream_base_url: String,
    pub trust_proxy_headers: bool,
}

impl AppState {
    pub fn trust_proxy_headers(&self) -> bool {
        self.runtime
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .trust_proxy_headers
    }

    pub fn upstream_base_url(&self) -> String {
        self.runtime
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .upstream_base_url
            .clone()
    }
}
