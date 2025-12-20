mod handlers;
mod routes;
mod metrics;
mod language_config;

use axum::Router;
use futures_util::StreamExt;
use redis::aio::ConnectionManager;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

#[derive(Clone)]
pub struct AppState {
    pub redis: ConnectionManager,
    pub start_time: Arc<std::time::Instant>,
    pub language_registry: Arc<language_config::LanguageRegistry>,
}

#[tokio::main]
async fn main() {
    // Load environment variables from .env file
    dotenvy::dotenv().ok();
    
    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .with_target(false)
        .init();

    info!("Optimus API booting...");

    // Initialize metrics
    metrics::init_metrics();
    info!("Metrics registry initialized");

    // Connect to Redis
    let redis_url = std::env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    
    let client = redis::Client::open(redis_url.as_str())
        .expect("Failed to create Redis client");
    
    let redis_conn = ConnectionManager::new(client).await
        .expect("Failed to connect to Redis");
    
    info!("Connected to Redis: {}", redis_url);

    // Load language configuration
    let config_path = std::env::var("LANGUAGE_CONFIG_PATH")
        .unwrap_or_else(|_| "config/languages.json".to_string());
    
    let language_registry = language_config::LanguageRegistry::load_from_file(&config_path)
        .unwrap_or_else(|e| {
            panic!("Failed to load language configuration from {}: {}", config_path, e);
        });
    
    let enabled_langs: Vec<String> = language_registry.enabled_languages()
        .iter()
        .map(|l| l.to_string())
        .collect();
    info!("Loaded language configuration: enabled languages = {:?}", enabled_langs);

    let state = Arc::new(AppState {
        redis: redis_conn.clone(),
        start_time: Arc::new(std::time::Instant::now()),
        language_registry: Arc::new(language_registry),
    });

    // Start background metrics subscriber
    tokio::spawn(metrics_subscriber());

    // Build router
    let app = Router::new()
        .merge(routes::routes())
        .with_state(state);

    // Start server
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "4001".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await
        .expect("Failed to bind to address");
    
    info!("HTTP server listening on {}", addr);
    info!("Ready to accept jobs");

    axum::serve(listener, app).await
        .expect("Server error");
}

/// Background task to subscribe to job completion events and update metrics
async fn metrics_subscriber() {
    let client = match redis::Client::open(
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string()).as_str()
    ) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to create Redis client for metrics subscriber: {}", e);
            return;
        }
    };
    
    let mut pubsub = match client.get_async_connection().await {
        Ok(conn) => conn.into_pubsub(),
        Err(e) => {
            tracing::error!("Failed to create pubsub connection: {}", e);
            return;
        }
    };
    
    if let Err(e) = pubsub.subscribe("optimus:metrics:completions").await {
        tracing::error!("Failed to subscribe to metrics channel: {}", e);
        return;
    }
    
    info!("Metrics subscriber started - listening for job completions");
    
    loop {
        match pubsub.on_message().next().await {
            Some(msg) => {
                let payload: String = match msg.get_payload() {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(&payload) {
                    let language = event["language"].as_str().unwrap_or("unknown");
                    let status = event["status"].as_str().unwrap_or("unknown");
                    let exec_time = event["execution_time_ms"].as_f64().unwrap_or(0.0);
                    
                    metrics::record_job_completed(language, status, exec_time);
                    
                    tracing::debug!(
                        job_id = event["job_id"].as_str().unwrap_or("unknown"),
                        language = language,
                        status = status,
                        "Recorded job completion metrics"
                    );
                }
            }
            None => break,
        }
    }
}

