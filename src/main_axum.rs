use std::net::SocketAddr;
// src/main_axum.rs
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result; // or your error type of choice

use axum::{Router};
use crate::discovery::fetchers::ContentDiscovery;
use crate::download::manager::DownloadManager;
use crate::service::state::AppState;
use tracing::{info, Level};
use tracing_subscriber::{fmt, EnvFilter};

// A function that starts Axum, returns (address, Arc<AppState>)
pub async fn start_axum_server(address: Option<String>) -> Result<(String, Arc<AppState>)> {
    let env_filter = EnvFilter::from_default_env()
        .add_directive(Level::DEBUG.into())
        .add_directive("mp4parse=off".parse().unwrap());

    fmt().with_env_filter(env_filter).init();

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
    let shared_state = Arc::new(state);

    // Start the download manager in background
    let manager = DownloadManager::new(shared_state.clone());
    tokio::spawn(async move {
        manager.run().await;
    });

    // Build the router
    let app = Router::new()
        // .route("/dashboard", get(dashboard)) // If you have a dashboard
        // .route("/video.mp4", get(stream_video)) // etc.
        // .with_state(shared_state.clone())
        // ...
        .with_state(shared_state.clone());


    // Spawn Axum server in background
    tokio::spawn(async move {
        axum_server::Server::bind(SocketAddr::V4(addr))
            .serve(app.into_make_service())
            .await
            .unwrap();
    });

    // Return (the address, the state)
    Ok((addr_str, shared_state))
}
