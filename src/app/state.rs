use std::sync::Arc;
use tokio::sync::Mutex;
use crate::models::models::VideoDownload;

#[derive(Debug, Clone)]
pub struct AppState {
    /// List of videos in watch order
    pub videos: Arc<Mutex<Vec<VideoDownload>>>,

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
        videos: Vec<VideoDownload>,
        max_downloads: usize,
        max_ahead: usize,
        max_behind_seconds: u64,
        max_storage_bytes: u64,
    ) -> Self {
        Self {
            videos: Arc::new(Mutex::new(videos)),
            max_downloads,
            max_ahead,
            max_behind_seconds,
            max_storage_bytes,
            current_storage_bytes: Arc::new(Mutex::new(0)),
            current_index: Arc::new(Mutex::new(0)),
        }
    }
}
