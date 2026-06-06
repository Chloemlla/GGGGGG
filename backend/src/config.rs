use std::{env, path::Path as FsPath};

const DEFAULT_UPSTREAM_BASE_URL: &str = "https://pixel.yh-mo.xyz";
const DEFAULT_SYNAPSE_OAUTH_BASE_URL: &str = "https://tts.chloemlla.com";

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind_addr: String,
    pub frontend_dir: String,
    pub mongo_database: String,
    pub mongo_uri: String,
    pub oauth: OAuthConfig,
    pub upstream_base_url: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            bind_addr: bind_addr(),
            frontend_dir: frontend_dist_dir(),
            mongo_database: config_value("MONGODB_DATABASE", "pixel_remake"),
            mongo_uri: config_value("MONGODB_URI", "mongodb://localhost:27017"),
            oauth: OAuthConfig::from_env(),
            upstream_base_url: config_value("UPSTREAM_BASE_URL", DEFAULT_UPSTREAM_BASE_URL),
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
    use super::{OAuthConfig, normalize_bind_addr, strip_wrapping_quotes};

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
        }
    }
}
