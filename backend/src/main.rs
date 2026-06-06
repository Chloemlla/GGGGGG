use std::env;

use axum::Router;
use mongodb::Client as MongoClient;
use reqwest::Client as HttpClient;
use tower_http::{
    cors::{AllowOrigin, Any, CorsLayer},
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
use models::{AdminSession, CdkMapping};
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

    admin::ensure_indexes(&mappings)
        .await
        .expect("create MongoDB indexes");

    let state = AppState {
        admin_sessions,
        http: HttpClient::new(),
        mappings,
        oauth: config.oauth.clone(),
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
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::mirror_request())
                .allow_methods(Any)
                .allow_headers(Any)
                .allow_credentials(true),
        )
        .layer(TraceLayer::new_for_http())
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

    let listener = tokio::net::TcpListener::bind(&config.bind_addr)
        .await
        .unwrap_or_else(|error| panic!("bind API address {}: {error}", config.bind_addr));
    let addr = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read bound API address: {error}"));
    println!("pixel-api listening on http://{addr}");
    axum::serve(listener, app).await.expect("run API server");
}
