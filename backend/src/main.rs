use std::env;

use axum::{
    Router,
    http::{HeaderValue, Method, header},
};
use mongodb::Client as MongoClient;
use reqwest::Client as HttpClient;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

mod admin;
mod config;
mod error;
mod models;
mod oauth;
mod proxy;
mod state;

use config::AppConfig;
use models::{AdminSession, CdkMapping, CdkUsageLog};
use state::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            env::var("RUST_LOG").unwrap_or_else(|_| "pixel_api=debug,tower_http=debug".to_string()),
        )
        .init();

    let config = AppConfig::from_env();
    let mongo = MongoClient::with_uri_str(&config.mongo_uri)
        .await
        .expect("connect MongoDB");
    let mappings = mongo
        .database(&config.mongo_database)
        .collection::<CdkMapping>("cdk_mappings");
    let admin_sessions = mongo
        .database(&config.mongo_database)
        .collection::<AdminSession>("admin_sessions");
    let usage_logs = mongo
        .database(&config.mongo_database)
        .collection::<CdkUsageLog>("cdk_usage_logs");

    admin::ensure_indexes(
        &mappings,
        &admin_sessions,
        &usage_logs,
        config.usage_log_ttl_seconds,
    )
    .await
    .expect("create MongoDB indexes");

    let state = AppState {
        admin_sessions,
        http: HttpClient::new(),
        mappings,
        oauth: config.oauth.clone(),
        usage_logs,
        upstream_base_url: config.upstream_base_url.clone(),
    };

    let frontend_dir = config.frontend_dir.clone();
    let frontend_index = format!("{frontend_dir}/index.html");
    let frontend_assets =
        ServeDir::new(&frontend_dir).not_found_service(ServeFile::new(frontend_index));

    let app = Router::new()
        .nest("/api/admin", oauth::router().merge(admin::router()))
        .merge(proxy::router())
        .fallback_service(frontend_assets)
        .layer(TraceLayer::new_for_http());
    let app = if config.cors_allowed_origins.is_empty() {
        app
    } else {
        app.layer(cors_layer(&config.cors_allowed_origins))
    }
    .with_state(state);

    println!("serving frontend from: {frontend_dir}");
    println!(
        "proxying user /api requests to Base URL: {}",
        config.upstream_base_url
    );
    println!(
        "admin CDK mappings stored in MongoDB database: {}",
        config.mongo_database
    );
    println!(
        "Synapse OAuth admin auth configured: {}",
        config.oauth.enabled()
    );
    println!(
        "credentialed CORS allowed origins: {}",
        if config.cors_allowed_origins.is_empty() {
            "(disabled)".to_string()
        } else {
            config.cors_allowed_origins.join(", ")
        }
    );

    let listener = tokio::net::TcpListener::bind(&config.bind_addr)
        .await
        .unwrap_or_else(|error| panic!("bind API address {}: {error}", config.bind_addr));
    let addr = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read bound API address: {error}"));
    println!("pixel-api listening on http://{addr}");
    axum::serve(listener, app).await.expect("run API server");
}

fn cors_layer(allowed_origins: &[String]) -> CorsLayer {
    let origins = allowed_origins
        .iter()
        .map(|origin| {
            HeaderValue::from_str(origin)
                .unwrap_or_else(|_| panic!("invalid CORS_ALLOWED_ORIGINS entry: {origin}"))
        })
        .collect::<Vec<_>>();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::ACCEPT,
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::HeaderName::from_static("x-requested-with"),
        ])
        .allow_credentials(true)
}
