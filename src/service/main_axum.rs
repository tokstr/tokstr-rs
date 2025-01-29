use std::net::{SocketAddr, TcpListener};
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
    let bind_str = address.unwrap_or_else(|| "127.0.0.1:0".to_string());

    // Create a TcpListener so we can retrieve the actual bound address
    let listener = TcpListener::bind(&bind_str)?;
    let local_addr = listener.local_addr()?;
    info!("Starting server at {}", local_addr);

    let relays = vec![
        "wss://relay.damus.io".to_string(),
        "wss://relay.snort.social".to_string(),
    ];
    let client = Arc::new(Client::default());
    let content_discovery = ContentDiscovery::new(relays, client).await?;

    // Create the global service state
    let state = AppState::new(
        content_discovery,
        max_parallel_downloads,
        60,
        max_storage_bytes,
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
        axum_server::Server::from_tcp(listener)
            .serve(app.into_make_service())
            .await
            .unwrap();
    });

    // Return (the address, the state)
    Ok((local_addr.to_string(), shared_state))
}
