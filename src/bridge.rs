use std::sync::Arc;
use once_cell::sync::OnceCell;

use flutter_rust_bridge::frb;
use log::{info, warn};
use tokio::sync::Mutex;
use crate::discovery::models::NostrVideo;
use crate::service::main_axum::start_axum_server;
use crate::models::models::VideoDownload;
use crate::service::state::AppState;

// 1) A global static for storing the Arc<AppState>
static GLOBAL_STATE: OnceCell<Arc<AppState>> = OnceCell::new();

// 2) Define an FFI-safe struct that mirrors `VideoDownload`
#[derive(Debug, Clone)]
pub struct FfiVideoDownload {
    pub id: String,
    pub url: String,
    pub title: Option<String>,
    pub local_path: Option<String>,
    pub nostr: NostrVideo
}

/// Start the Axum server and store the AppState in GLOBAL_STATE.
/// Return the bound address as a String.
#[frb]
pub async fn ffi_start_server(
    max_parallel_downloads: usize,
    max_storage_bytes: u64) -> String {
    match start_axum_server(max_parallel_downloads, max_storage_bytes).await {
        Ok((addr, state)) => {
            // Store the Arc<AppState> in the static if not already set
            // (Usually you'd only call this function once.)
            GLOBAL_STATE.set(state).ok();
            addr
        }
        Err(e) => format!("Error starting server: {e}"),
    }
}

/// Return the discovered videos from the stored AppState.
#[frb]
pub async fn ffi_get_discovered_videos() -> Vec<FfiVideoDownload> {
    // Get the Arc<AppState> from the static
    let app_state = GLOBAL_STATE
        .get()
        .expect("Axum server not started or state not set");

    // Lock the discovered_videos
    let discovered = app_state.playlist.lock().await.new_content();
    warn!("Discovered videos: {:?}", discovered);

    discovered
        .iter()
        .map(|vid| {
            let is_fully_downloaded = match vid.content_length {
                Some(total) if total > 0 => vid.downloaded_bytes >= total,
                _ => false,
            };

            let local_path = if is_fully_downloaded {
                vid.local_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
            } else {
                None
            };

            FfiVideoDownload {
                id: vid.id.to_string(),
                url: vid.url.clone(),
                title: Some(vid.nostr.title.clone()),
                local_path,
                nostr: vid.nostr.clone(),
            }
        })
        .collect()
}

