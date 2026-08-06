use std::{env, path::Path as FsPath};

const DEFAULT_UPSTREAM_BASE_URL: &str = "https://pixel.yh-mo.xyz";
const DEFAULT_SYNAPSE_OAUTH_BASE_URL: &str = "https://tts.chloemlla.com";

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind_addr: String,
    pub cors_allowed_origins: Vec<String>,
    pub frontend_dir: String,
    pub login_rate_limit_per_minute: u64,
    pub mongo_database: String,
    pub mongo_uri: String,
    pub oauth: OAuthConfig,
    pub proxy_rate_limit_per_minute: u64,
    pub trust_proxy_headers: bool,
    pub upstream_base_url: String,
    pub usage_log_ttl_seconds: u64,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            bind_addr: bind_addr(),
            cors_allowed_origins: config_csv("CORS_ALLOWED_ORIGINS"),
            frontend_dir: frontend_dist_dir(),
            login_rate_limit_per_minute: config_u64("LOGIN_RATE_LIMIT_PER_MINUTE", 15),
            mongo_database: config_value("MONGODB_DATABASE", "pixel_remake"),
            mongo_uri: config_value("MONGODB_URI", "mongodb://localhost:27017"),
            oauth: OAuthConfig::from_env(),
            proxy_rate_limit_per_minute: config_u64("PROXY_RATE_LIMIT_PER_MINUTE", 30),
            trust_proxy_headers: config_bool("TRUST_PROXY_HEADERS", false),
            upstream_base_url: config_value("UPSTREAM_BASE_URL", DEFAULT_UPSTREAM_BASE_URL),
            usage_log_ttl_seconds: config_u64("CDK_USAGE_LOG_TTL_SECONDS", 2_592_000),
        }
    }

    /// Merges persisted admin overrides (plaintext) over the env-derived values.
    /// ADMIN_TOKEN_ENCRYPTION_KEY is intentionally skipped: it is the bootstrap
    /// key used to decrypt stored overrides and CDK mappings.
    pub fn apply_overrides(&mut self, overrides: &std::collections::HashMap<String, String>) {
        let value = |key: &str| {
            overrides
                .get(key)
                .map(|entry| strip_wrapping_quotes(entry.trim()).to_string())
        };

        if let Some(value) = value("BIND_ADDR")
            && !value.is_empty()
        {
            self.bind_addr = normalize_bind_addr(&value);
        }
        if let Some(value) = value("FRONTEND_DIST_DIR")
            && !value.is_empty()
        {
            self.frontend_dir = value;
        }
        if let Some(value) = value("MONGODB_URI")
            && !value.is_empty()
        {
            self.mongo_uri = value;
        }
        if let Some(value) = value("MONGODB_DATABASE")
            && !value.is_empty()
        {
            self.mongo_database = value;
        }
        if let Some(value) = value("UPSTREAM_BASE_URL")
            && !value.is_empty()
        {
            self.upstream_base_url = value;
        }
        if let Some(value) = value("TRUST_PROXY_HEADERS")
            && let Some(parsed) = parse_override_bool(&value)
        {
            self.trust_proxy_headers = parsed;
        }
        if let Some(value) = value("PROXY_RATE_LIMIT_PER_MINUTE")
            && let Ok(parsed) = value.parse::<u64>()
        {
            self.proxy_rate_limit_per_minute = parsed.max(1);
        }
        if let Some(value) = value("LOGIN_RATE_LIMIT_PER_MINUTE")
            && let Ok(parsed) = value.parse::<u64>()
        {
            self.login_rate_limit_per_minute = parsed.max(1);
        }
        if let Some(value) = value("CDK_USAGE_LOG_TTL_SECONDS")
            && let Ok(parsed) = value.parse::<u64>()
        {
            self.usage_log_ttl_seconds = parsed.max(60);
        }
        if let Some(value) = value("CORS_ALLOWED_ORIGINS") {
            self.cors_allowed_origins = parse_csv(&value);
        }
        if let Some(value) = value("APP_BASE_URL")
            && !value.is_empty()
        {
            self.oauth.app_base_url = value;
        }
        if let Some(value) = value("SYNAPSE_OAUTH_BASE_URL")
            && !value.is_empty()
        {
            self.oauth.base_url = value;
        }
        if let Some(value) = value("SYNAPSE_OAUTH_CLIENT_ID") {
            self.oauth.client_id = value;
        }
        if let Some(value) = value("SYNAPSE_OAUTH_CLIENT_SECRET") {
            self.oauth.client_secret = value;
        }
        if let Some(value) = value("SYNAPSE_OAUTH_REDIRECT_URI") {
            self.oauth.redirect_uri_override = value;
        }
        if let Some(value) = value("SYNAPSE_OAUTH_SCOPES")
            && !value.is_empty()
        {
            self.oauth.scopes = value;
        }
        if let Some(value) = value("ADMIN_SESSION_COOKIE")
            && !value.is_empty()
        {
            self.oauth.session_cookie_name = value;
        }
        if let Some(value) = value("ADMIN_SESSION_TTL_SECONDS")
            && let Ok(parsed) = value.parse::<i64>()
        {
            self.oauth.session_ttl_seconds = parsed.max(60);
        }
    }
}

#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub app_base_url: String,
    pub base_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri_override: String,
    pub scopes: String,
    pub session_cookie_name: String,
    pub session_ttl_seconds: i64,
    pub token_encryption_key: String,
}

impl OAuthConfig {
    fn from_env() -> Self {
        Self {
            app_base_url: config_value("APP_BASE_URL", "http://localhost:8080"),
            base_url: config_value("SYNAPSE_OAUTH_BASE_URL", DEFAULT_SYNAPSE_OAUTH_BASE_URL),
            client_id: config_value("SYNAPSE_OAUTH_CLIENT_ID", ""),
            client_secret: config_value("SYNAPSE_OAUTH_CLIENT_SECRET", ""),
            redirect_uri_override: config_value("SYNAPSE_OAUTH_REDIRECT_URI", ""),
            scopes: config_value(
                "SYNAPSE_OAUTH_SCOPES",
                "openid profile email admin:identity",
            ),
            session_cookie_name: config_value("ADMIN_SESSION_COOKIE", "synapse_admin_session"),
            session_ttl_seconds: config_i64("ADMIN_SESSION_TTL_SECONDS", 2_592_000),
            token_encryption_key: config_value("ADMIN_TOKEN_ENCRYPTION_KEY", ""),
        }
    }

    pub fn authorize_endpoint(&self) -> String {
        self.endpoint("/oauth/authorize")
    }

    pub fn cookie_secure(&self) -> bool {
        self.redirect_uri()
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("https://")
    }

    pub fn enabled(&self) -> bool {
        !self.client_id.is_empty() && !self.client_secret.is_empty()
    }

    pub fn redirect_uri(&self) -> String {
        if !self.redirect_uri_override.is_empty() {
            return self.redirect_uri_override.clone();
        }

        format!(
            "{}/api/admin/auth/callback",
            self.app_base_url.trim_end_matches('/')
        )
    }

    pub fn token_endpoint(&self) -> String {
        self.endpoint("/api/oauth/token")
    }

    pub fn token_encryption_secret(&self) -> &str {
        if self.token_encryption_key.is_empty() {
            &self.client_secret
        } else {
            &self.token_encryption_key
        }
    }

    pub fn userinfo_endpoint(&self) -> String {
        self.endpoint("/api/oauth/userinfo")
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }
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

fn config_i64(name: &str, default: i64) -> i64 {
    config_value(name, &default.to_string())
        .parse::<i64>()
        .unwrap_or(default)
}

fn config_u64(name: &str, default: u64) -> u64 {
    config_value(name, &default.to_string())
        .parse::<u64>()
        .unwrap_or(default)
}

fn config_bool(name: &str, default: bool) -> bool {
    match config_value(name, "") {
        value if value.is_empty() => default,
        value => parse_override_bool(&value).unwrap_or(false),
    }
}

fn parse_override_bool(value: &str) -> Option<bool> {
    match strip_wrapping_quotes(value.trim()).to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "y" | "on" | "t" => Some(true),
        "0" | "false" | "no" | "n" | "off" | "f" | "" => Some(false),
        _ => None,
    }
}

fn config_csv(name: &str) -> Vec<String> {
    parse_csv(&config_value(name, ""))
}

fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        AppConfig, OAuthConfig, config_bool, normalize_bind_addr, parse_csv, strip_wrapping_quotes,
    };

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

    #[test]
    fn uses_explicit_oauth_redirect_uri_when_configured() {
        let config = oauth_config(
            "http://localhost:8080",
            "https://gemini.chloemlla.com/api/admin/auth/callback",
        );

        assert_eq!(
            config.redirect_uri(),
            "https://gemini.chloemlla.com/api/admin/auth/callback"
        );
        assert!(config.cookie_secure());
    }

    #[test]
    fn derives_oauth_redirect_uri_from_app_base_url() {
        let config = oauth_config("https://gemini.chloemlla.com", "");

        assert_eq!(
            config.redirect_uri(),
            "https://gemini.chloemlla.com/api/admin/auth/callback"
        );
        assert!(config.cookie_secure());
    }

    #[test]
    fn uses_oauth_secret_as_default_token_encryption_secret() {
        let config = oauth_config("https://gemini.chloemlla.com", "");

        assert_eq!(config.token_encryption_secret(), "syn_secret_test");
    }

    #[test]
    fn parses_comma_separated_origins() {
        assert_eq!(
            parse_csv("https://a.example, https://b.example ,,"),
            vec!["https://a.example", "https://b.example"]
        );
    }

    #[test]
    fn parses_truthy_and_falsy_bool_values() {
        assert!(!config_bool("", false));
        for value in ["1", "true", "yes", "y", "on", "TRUE", "Yes"] {
            unsafe {
                std::env::set_var("TEST_TRUST_PROXY_HEADERS", value);
            }
            assert!(config_bool("TEST_TRUST_PROXY_HEADERS", false));
        }
        for value in ["0", "false", "no", "n", "off", ""] {
            unsafe {
                std::env::set_var("TEST_TRUST_PROXY_HEADERS", value);
            }
            assert!(!config_bool("TEST_TRUST_PROXY_HEADERS", false));
        }
        unsafe {
            std::env::remove_var("TEST_TRUST_PROXY_HEADERS");
        }
        assert!(!config_bool("TEST_TRUST_PROXY_HEADERS", false));
    }

    fn oauth_config(app_base_url: &str, redirect_uri_override: &str) -> OAuthConfig {
        OAuthConfig {
            app_base_url: app_base_url.to_string(),
            base_url: "https://tts.chloemlla.com".to_string(),
            client_id: "syn_client_test".to_string(),
            client_secret: "syn_secret_test".to_string(),
            redirect_uri_override: redirect_uri_override.to_string(),
            scopes: "openid profile admin:identity".to_string(),
            session_cookie_name: "synapse_admin_session".to_string(),
            session_ttl_seconds: 2_592_000,
            token_encryption_key: String::new(),
        }
    }

    fn app_config() -> AppConfig {
        AppConfig {
            bind_addr: "0.0.0.0:8080".to_string(),
            cors_allowed_origins: vec![],
            frontend_dir: "frontend/dist".to_string(),
            login_rate_limit_per_minute: 15,
            mongo_database: "pixel_remake".to_string(),
            mongo_uri: "mongodb://localhost:27017".to_string(),
            oauth: oauth_config("http://localhost:8080", ""),
            proxy_rate_limit_per_minute: 30,
            trust_proxy_headers: false,
            upstream_base_url: "https://pixel.yh-mo.xyz".to_string(),
            usage_log_ttl_seconds: 2_592_000,
        }
    }

    #[test]
    fn applies_overrides_but_never_the_encryption_key() {
        let mut config = app_config();
        let overrides = HashMap::from([
            ("UPSTREAM_BASE_URL".to_string(), "https://new.example".to_string()),
            ("TRUST_PROXY_HEADERS".to_string(), "true".to_string()),
            ("LOGIN_RATE_LIMIT_PER_MINUTE".to_string(), "7".to_string()),
            (
                "ADMIN_TOKEN_ENCRYPTION_KEY".to_string(),
                "must-not-apply".to_string(),
            ),
        ]);

        config.apply_overrides(&overrides);

        assert_eq!(config.upstream_base_url, "https://new.example");
        assert!(config.trust_proxy_headers);
        assert_eq!(config.login_rate_limit_per_minute, 7);
        assert_eq!(config.oauth.token_encryption_key, "");
    }
}
