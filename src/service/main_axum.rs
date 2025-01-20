use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;

use axum::{Router, ServiceExt};
use axum::routing::{get, post};
use crate::discovery::fetchers::ContentDiscovery;
use crate::download::manager::DownloadManager;
use crate::service::state::AppState;
use tracing::{info, Level};
use tracing_subscriber::{fmt, EnvFilter};
use crate::handlers::handlers::{dashboard, get_status, get_thumbnail, set_index, stream_video};
use crate::utils::log::init_logger_once;

pub async fn start_axum_server(address: Option<String>) -> Result<(String, Arc<AppState>)> {
    init_logger_once();

    let addr_str = address.unwrap_or_else(|| "127.0.0.1:3000".to_string());
    let addr = addr_str.parse().expect("Invalid address");

    info!("Starting server at {}", addr_str);

    // Create the content discovery
    let relays = vec![
        "wss://relay.damus.io".to_string(),
        "wss://relay.snort.social".to_string(),
    ];
    let api = ContentDiscovery::new(relays).await?;

    // Create the global service state
    let state = AppState::new(
        api,
        2,                         // max_downloads
        2,                         // max_ahead
        60,                        // max_behind_seconds
        1024 * 1024 * 1024,        // max_storage_bytes
    );

    // Wrap in an Arc
    let shared_state = Arc::new(state);

    // Start the download manager in background
    let manager = DownloadManager::new(shared_state.clone());
    tokio::spawn(async move {
        manager.run().await;
    });

    // Build the router
    let app = Router::new()
        .route("/dashboard", get(dashboard))
        .route("/video.mp4", get(stream_video))
        .route("/status", get(get_status))
        .route("/set_index", post(set_index))
        .route("/thumbnail", get(get_thumbnail))
        .with_state(shared_state.clone()); // shared_state is Arc<AppState>

    // Spawn Axum server in the background
    tokio::spawn(async move {
        axum_server::Server::bind(SocketAddr::V4(addr))
            .serve(app.into_make_service())
            .await
            .unwrap();
    });

    // Return (the address, the state)
    Ok((addr_str, shared_state))
}
