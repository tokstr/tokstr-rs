use std::sync::Arc;
use tokio::sync::Mutex;
use crate::discovery::fetchers::ContentDiscovery;
use crate::models::models::VideoDownload;

#[derive(Debug, Clone)]
pub struct AppState {
    /// List of videos in watch order
    pub content_discovery: Arc<Mutex<ContentDiscovery>>,
    pub discovered_videos: Arc<Mutex<Vec<VideoDownload>>>,

    /// Concurrency settings
    pub max_downloads: usize,
    pub max_ahead: usize,
    pub max_behind_seconds: u64,

    /// Storage
    pub max_storage_bytes: u64,
    pub current_storage_bytes: Arc<Mutex<u64>>,

    /// The user's current watch index
    pub current_index: Arc<Mutex<usize>>,
}

impl AppState {
    pub fn new(
        content_discovery: ContentDiscovery,
        max_downloads: usize,
        max_ahead: usize,
        max_behind_seconds: u64,
        max_storage_bytes: u64,
    ) -> Self {
        Self {
            content_discovery: Arc::new(Mutex::new(content_discovery)),
            discovered_videos: Arc::new(Mutex::new(vec![])),
            max_downloads,
            max_ahead,
            max_behind_seconds,
            max_storage_bytes,
            current_storage_bytes: Arc::new(Mutex::new(0)),
            current_index: Arc::new(Mutex::new(0)),
        }
    }
}
