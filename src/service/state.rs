use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::discovery::fetchers::ContentDiscovery;
use crate::models::models::VideoDownload;
use crate::service::playlist::Playlist;

#[derive(Debug, Clone)]
pub struct AppState {
    /// List of videos in watch order
    pub content_discovery: Arc<ContentDiscovery>,
    pub discovered_videos: Arc<Mutex<HashMap<String, VideoDownload>>>,
    /// The user's current watch index
    pub current_index: Arc<Mutex<usize>>,
    pub playlist: Arc<Mutex<Playlist>>,

    /// Concurrency settings
    pub max_parallel_downloads: usize,
    pub max_ahead: usize,
    pub max_behind_seconds: u64,
    pub target_minutes_ahead: f64,
    pub target_videos_ahead: usize,

    /// Storage
    pub max_storage_bytes: u64,
    pub current_storage_bytes: Arc<Mutex<u64>>,
}

impl AppState {
    pub fn new(
        content_discovery: ContentDiscovery,
        max_parallel_downloads: usize,
        max_ahead: usize,
        max_behind_seconds: u64,
        max_storage_bytes: u64,
    ) -> Self {
        Self {
            content_discovery: Arc::new(content_discovery),
            discovered_videos: Arc::new(Mutex::new(HashMap::new())),
            current_index: Arc::new(Mutex::new(0)),
            playlist: Arc::new(Mutex::new(Playlist::new())),
            max_parallel_downloads,
            max_ahead,
            max_behind_seconds,
            target_minutes_ahead: 60.0,
            target_videos_ahead: 15,
            max_storage_bytes,
            current_storage_bytes: Arc::new(Mutex::new(0)),
        }
    }
}

