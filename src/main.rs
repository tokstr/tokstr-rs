mod discovery;
mod service;
mod handlers;
mod models;
mod utils;
mod download;

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
use tracing_subscriber::{fmt};

use tracing_subscriber::{EnvFilter};
use crate::service::state::AppState;
use crate::discovery::fetchers::{ContentDiscovery};
use crate::download::manager::DownloadManager;
use crate::handlers::handlers::{dashboard, get_status, get_thumbnail, set_index, stream_video};
use crate::models::models::VideoDownload;
use crate::utils::log::init_logger_once;

#[tokio::main]
async fn main() {
    init_logger_once();


    let relays = vec![
        "wss://relay.damus.io".into(),
        "wss://relay.snort.social".into()
    ];

    // 2) Create the API -- it automatically fetches videos on creation
    let api = ContentDiscovery::new(relays).await.unwrap();


    // Create the global service state
    let state = AppState::new(
        api,
        2,
        2,
        60,
        1024 * 1024 * 1024,
    );

    let state_shared = Arc::new(state);
    // Start the DownloadManager in the background
    let manager = DownloadManager::new(state_shared.clone());
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

    let addr = "127.0.0.1:3000".parse().unwrap();
    info!("Listening on http://{}", addr);

    // Run Axum server
    axum_server::Server::bind(addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
