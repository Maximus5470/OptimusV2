mod handlers;
mod routes;

use axum::Router;
use redis::aio::ConnectionManager;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

#[derive(Clone)]
pub struct AppState {
    pub redis: ConnectionManager,
}

#[tokio::main]
async fn main() {
    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .with_target(false)
        .init();

    info!("Optimus API booting...");

    // Connect to Redis
    let redis_url = std::env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    
    let client = redis::Client::open(redis_url.as_str())
        .expect("Failed to create Redis client");
    
    let redis_conn = ConnectionManager::new(client).await
        .expect("Failed to connect to Redis");
    
    info!("Connected to Redis: {}", redis_url);

    let state = Arc::new(AppState {
        redis: redis_conn,
    });

    // Build router
    let app = Router::new()
        .merge(routes::routes())
        .with_state(state);

    // Start server
    let addr = "0.0.0.0:3000";
    let listener = TcpListener::bind(addr).await
        .expect("Failed to bind to address");
    
    info!("HTTP server listening on {}", addr);
    info!("Ready to accept jobs");

    axum::serve(listener, app).await
        .expect("Server error");
}
