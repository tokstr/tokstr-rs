use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::Result;

use axum::{Router};
use axum::routing::{get, post};
use nostr_sdk::Client;
use tokio::sync::Mutex;
use crate::discovery::fetchers::ContentDiscovery;
use crate::download::manager::DownloadManager;
use crate::service::state::AppState;
use tracing::{info};
use crate::handlers::handlers::{dashboard, get_status, get_thumbnail, set_index, stream_video};

pub async fn start_axum_server(
    max_parallel_downloads: usize,
    max_storage_bytes: u64,
    address: Option<String>) -> Result<(String, Arc<AppState>)> {
    let addr_str = address.unwrap_or_else(|| "127.0.0.1:3000".to_string());
    let addr = addr_str.parse().expect("Invalid address");

    info!("Starting server at {}", addr_str);

    // Create the content discovery
    let relays = vec![
        "wss://relay.damus.io".to_string(),
        "wss://relay.snort.social".to_string(),
    ];
    let client = Arc::new(Client::default());
    let api = ContentDiscovery::new(relays, client).await?;

    // Create the global service state
    let state = AppState::new(
        api,
        max_parallel_downloads,                         // max_downloads
        2,                         // max_ahead
        60,                        // max_behind_seconds
        max_storage_bytes,        // max_storage_bytes
    );

    // Wrap in an Arc
    let shared_state = Arc::new(state);
    let manager = Arc::new(DownloadManager::new(shared_state.clone()));

    tokio::spawn(manager.clone().run());


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
    Ok((addr_str, shared_state.clone()))
}
