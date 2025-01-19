use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::discovery::models::NostrVideo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoDownload {
    /// Unique ID for referencing
    pub id: String,

    /// Original URL of the video
    pub url: String,

    pub nostr: NostrVideo,

    /// Local path where the file is stored (if downloaded)
    pub local_path: Option<PathBuf>,

    /// Whether we are currently downloading
    pub downloading: bool,


    /// Video length in seconds (if known)
    pub length_seconds: Option<f64>,

    /// Format (e.g., "H.264", "MPEG4", etc.), if known
    pub format: Option<String>,

    /// Width/Height, if known
    pub width: Option<u32>,
    pub height: Option<u32>,

    pub downloaded_bytes: u64,
    pub content_length: Option<u64>,

    // We'll store the current computed speed in bytes/second, updated every chunk or so.
    pub download_speed_bps: f64,
    // Also track the last time we updated the speed, so we can measure intervals.
    #[serde(skip_serializing, skip_deserializing)]
    pub last_speed_update_instant: Option<std::time::Instant>,
    // Keep track of how many bytes had been downloaded last time we measured.
    #[serde(skip_serializing, skip_deserializing)]
    pub last_speed_update_bytes: u64,

    pub thumbnail_path: Option<PathBuf>,
}

impl VideoDownload {
    pub fn from_nostr_video(nostr: NostrVideo) -> Self {
        Self {
            id: nostr.id.clone(),
            url: nostr.url.clone(),
            nostr,
            local_path: None,
            downloading: false,
            length_seconds: None,
            format: None,
            width: None,
            height: None,
            downloaded_bytes: 0,
            content_length: None,
            download_speed_bps: 0.0,
            last_speed_update_instant: None,
            last_speed_update_bytes: 0,
            thumbnail_path: None,
        }
    }
}