use std::sync::Arc;
use tokio::fs::{remove_file, File};
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use mp4parse::{read_mp4, Error as Mp4Error, TrackType};

use crate::service::state::AppState;
use crate::models::models::VideoDownload;
use crate::utils::utils::write_image_to_jpeg;

pub struct DownloadManager {
    state: Arc<AppState>,
}

impl DownloadManager {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    /// Main loop: fetch newly discovered videos, enforce behind limit,
    /// start downloads, etc.
    pub async fn run(self) {
        loop {
            // 1) Fetch any *new* videos from `content_discovery`.
            self.sync_new_videos().await;

            // 2) Enforce behind-limit
            self.enforce_behind_limit().await;

            // 3) Trigger downloads
            self.download_videos().await;

            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }

    /// Grabs newly arrived videos from `content_discovery` and
    /// appends them to our in-memory `discovered_videos` list.
    async fn sync_new_videos(&self) {
        // 1) Call the new content discovery manager to fetch videos
        let mut discovery = self.state.content_discovery.lock().await;
        let new_videos = discovery.fetch_new_videos(); // returns Vec<Video>

        // 2) Convert each to VideoDownload (if your `Video` has a `to_download()` method)
        let mut discovered = self.state.discovered_videos.lock().await;
        for video in new_videos {
            discovered.push(video.to_download());
        }
    }

    async fn enforce_behind_limit(&self) {
        let current_idx = *self.state.current_index.lock().await;

        // Lock the discovered_videos list
        let mut videos = self.state.discovered_videos.lock().await;

        let mut i = current_idx as isize - 1;
        while i >= 0 {
            let should_remove = {
                if let Some(video) = videos.get(i as usize) {
                    let length = video.length_seconds.unwrap_or(0.0);
                    length > self.state.max_behind_seconds as f64
                } else {
                    false
                }
            };

            if should_remove {
                if let Some(video) = videos.get_mut(i as usize) {
                    if let Some(local_path) = &video.local_path {
                        let _ = remove_file(local_path).await;
                        video.local_path = None;
                    }
                }
            }
            i -= 1;
        }
    }

    async fn download_videos(&self) {
        let current_idx = *self.state.current_index.lock().await;

        // Lock the discovered_videos list
        let mut videos = self.state.discovered_videos.lock().await;

        // Count how many are currently downloading
        let mut concurrent_downloads = videos.iter().filter(|v| v.downloading).count();

        for (idx, video) in videos.iter_mut().enumerate().skip(current_idx) {
            if concurrent_downloads >= self.state.max_downloads {
                break;
            }

            if video.local_path.is_none() && !video.downloading {
                video.downloading = true;
                let url = video.url.clone();
                let state_clone = self.state.clone();

                // Spawn an async task to download this video
                tokio::spawn(async move {
                    if let Err(e) = download_video_progressive(&state_clone, idx, &url).await {
                        error!("Failed to download {url}: {e}");
                        let mut list = state_clone.discovered_videos.lock().await;
                        if let Some(v) = list.get_mut(idx) {
                            v.downloading = false;
                        }
                    }
                });

                concurrent_downloads += 1;
            }
        }
    }
}

/// Progressive download of a single MP4 file.
/// (Same as your original code, but references `discovered_videos` instead of the old discovery list).
async fn download_video_progressive(
    state: &AppState,
    index: usize,
    url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let mut resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(format!("HTTP request failed with status: {}", resp.status()).into());
    }

    // Possibly store content_length if available:
    if let Some(cl) = resp.content_length() {
        let mut videos_guard = state.discovered_videos.lock().await;
        if let Some(video) = videos_guard.get_mut(index) {
            video.content_length = Some(cl);
        }
    }

    // Create a unique file path
    let file_name = format!("{}.mp4", Uuid::new_v4());
    let file_path = std::env::temp_dir().join(file_name);

    // Immediately store the local_path so we can stream partial data
    {
        let mut list = state.discovered_videos.lock().await;
        if let Some(video) = list.get_mut(index) {
            video.local_path = Some(file_path.clone());
        }
    }

    // Open file for writing
    let mut file = File::create(&file_path).await?;

    let mut parse_buffer: Vec<u8> = Vec::new();
    let mut downloaded_bytes = 0u64;
    let mut metadata_extracted = false;

    // Download in chunks
    while let Some(chunk) = resp.chunk().await? {
        // 1) Check storage
        {
            let mut storage = state.current_storage_bytes.lock().await;
            if *storage + chunk.len() as u64 > state.max_storage_bytes {
                warn!("Storage budget exceeded for URL: {url}");
                return Err("Storage budget exceeded".into());
            }
            *storage += chunk.len() as u64;
        }

        // 2) Write chunk to disk
        file.write_all(&chunk).await?;
        file.flush().await?;

        downloaded_bytes += chunk.len() as u64;

        // Update progress, speed, etc.
        {
            let mut videos_guard = state.discovered_videos.lock().await;
            if let Some(video) = videos_guard.get_mut(index) {
                video.downloaded_bytes = downloaded_bytes;
                if video.content_length.is_none() {
                    if let Some(cl) = resp.content_length() {
                        video.content_length = Some(cl);
                    }
                }
                let now = std::time::Instant::now();
                match video.last_speed_update_instant {
                    None => {
                        video.last_speed_update_instant = Some(now);
                        video.last_speed_update_bytes = downloaded_bytes;
                        video.download_speed_bps = 0.0;
                    }
                    Some(prev_time) => {
                        let dt = now.duration_since(prev_time).as_secs_f64();
                        if dt >= 1.0 {
                            let bytes_diff = downloaded_bytes - video.last_speed_update_bytes;
                            video.download_speed_bps = bytes_diff as f64 / dt;
                            video.last_speed_update_instant = Some(now);
                            video.last_speed_update_bytes = downloaded_bytes;
                        }
                    }
                }
            }
        }

        // 3) Keep a copy for partial parsing
        parse_buffer.extend_from_slice(&chunk);

        // Attempt to parse on-the-fly if we haven't yet succeeded
        if !metadata_extracted {
            match parse_mp4_entire(&parse_buffer) {
                Ok(Some((duration, codec, width, height))) => {
                    update_metadata(state, index, &file_path, duration, &codec, width, height).await;
                    metadata_extracted = true;

                    // Attempt to decode a thumbnail
                    if let Ok(vec) = ffmpeg_extractor::extract_first_frame_to_jpeg(&parse_buffer) {
                        let thumb_path = std::env::temp_dir()
                            .join(format!("thumb_{}.jpg", Uuid::new_v4()));
                        if let Err(e) = write_image_to_jpeg(&vec, &thumb_path) {
                            tracing::warn!("Could not write thumbnail to JPEG: {}", e);
                        } else {
                            let mut list = state.discovered_videos.lock().await;
                            if let Some(video) = list.get_mut(index) {
                                video.thumbnail_path = Some(thumb_path);
                            }
                        }
                    }
                }
                Ok(None) => {
                    // No moov yet, or incomplete data - continue
                }
                Err(_e) => {
                    // Parse error - optionally log or ignore
                }
            }
        }
    }

    // Now the entire download loop is finished
    file.flush().await?;
    drop(file); // close the file

    // If never extracted metadata, do a final parse on the full buffer
    if !metadata_extracted {
        match parse_mp4_entire(&parse_buffer) {
            Ok(Some((duration, codec, width, height))) => {
                info!(
                    "Parsed final MP4 metadata for {url}: duration={duration}, codec={codec}, \
                     width={width}, height={height}"
                );
                update_metadata(state, index, &file_path, duration, &codec, width, height).await;
            }
            Ok(None) => {
                warn!("Could not parse MP4 metadata for {url}. Possibly missing moov box.");
            }
            Err(e) => {
                warn!("Error parsing final MP4 data for {url}: {e}");
            }
        }
    }

    // Mark downloading = false
    {
        let mut list = state.discovered_videos.lock().await;
        if let Some(video) = list.get_mut(index) {
            video.downloading = false;
        }
    }

    debug!(
        "Downloaded video #{index} => {}, size: {} bytes",
        file_path.display(),
        downloaded_bytes,
    );

    Ok(())
}

/// Parse the entire MP4 buffer. Returns (duration, codec, width, height).
fn parse_mp4_entire(
    parse_buffer: &[u8],
) -> Result<Option<(f64, String, u32, u32)>, Mp4Error> {
    let mut context = read_mp4(&mut std::io::Cursor::new(parse_buffer))?;

    if let Some(track) = context.tracks.first() {
        let timescale = track.timescale.map_or(0, |t| t.0);
        let raw_duration = track.duration.map_or(0, |d| d.0);
        let duration_seconds = if timescale > 0 {
            raw_duration as f64 / timescale as f64
        } else {
            0.0
        };

        if let Some(tkhd) = &track.tkhd {
            let width = (tkhd.width >> 16) as u32;
            let height = (tkhd.height >> 16) as u32;
            let codec = if let Some(stsd) = &track.stsd {
                match &track.track_type {
                    TrackType::Video => {
                        if let Some(description) = stsd.descriptions.first() {
                            format!("{:?}", description)
                        } else {
                            "unknown".to_string()
                        }
                    },
                    _ => "non-video".to_string(),
                }
            } else {
                "unknown".to_string()
            };
            return Ok(Some((duration_seconds, codec, width, height)));
        }
    }
    Ok(None)
}

async fn update_metadata(
    state: &AppState,
    index: usize,
    file_path: &std::path::Path,
    duration: f64,
    codec: &str,
    width: u32,
    height: u32,
) {
    let mut list = state.discovered_videos.lock().await;
    if let Some(video) = list.get_mut(index) {
        video.local_path = Some(file_path.to_path_buf());
        video.length_seconds = Some(duration);
        video.format = Some(codec.to_string());
        if width > 0 {
            video.width = Some(width);
        }
        if height > 0 {
            video.height = Some(height);
        }
    }
}
