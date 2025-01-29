mod discovery;
mod service;
mod handlers;
mod models;
mod utils;
mod download;

use std::net::TcpListener;
use axum::{
    routing::{get, post},
    Router,
};
use tracing::{info, Level};
use std::sync::Arc;
use std::thread::Builder;
use tokio::sync::Mutex;
use uuid::Uuid;
use tracing_subscriber::{Layer};

use axum::{response::Html};
use nostr_sdk::Alphabet::M;
use nostr_sdk::Client;
use tracing_subscriber::{fmt};

use tracing_subscriber::{EnvFilter};
use crate::service::state::AppState;
use crate::discovery::fetchers::{ContentDiscovery};
use crate::download::manager::DownloadManager;
use crate::handlers::handlers::{dashboard, get_status, get_thumbnail, set_index, stream_video};
use crate::models::models::VideoDownload;
use crate::utils::log::init_logger_once;
use crate::utils::utils::find_available_port;

#[tokio::main]
async fn main() {
    init_logger_once();
    // 1) Set up the relays
    let relays = vec![
        "wss://relay.damus.io".into(),
        "wss://relay.snort.social".into()
    ];

    // 2) Create the API -- it automatically fetches videos on creation
    let client = Arc::new(Client::default());
    let api = ContentDiscovery::new(relays, client).await.unwrap();


    // Create the global service state
    let state = AppState::new(
        api,
        10,
        60,
        1024 * 1024 * 1024,
    );

    let state_shared = Arc::new(state);
    // Start the DownloadManager in the background

    let manager = Arc::new(DownloadManager::new(state_shared.clone()));
    tokio::spawn(async move {
        manager.run().await;
    });

    // Build Axum router
    let app = Router::new()
        .route("/dashboard", get(dashboard))
        .route("/video.mp4", get(stream_video))
        .route("/status", get(get_status))
        .route("/set_index", post(set_index))
        .route("/thumbnail", get(get_thumbnail))
        .with_state(state_shared.clone());


    let listener = find_available_port().unwrap();
    let local_addr = listener.local_addr().unwrap();
    info!("Starting server at {}", local_addr);

    info!("Listening on http://{}", local_addr);

    // Run Axum server
    axum_server::Server::from_tcp(listener)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
