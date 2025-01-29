use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{remove_file, File};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use futures::stream::{self, StreamExt};
use uuid::Uuid;
use tracing::{debug, error, info, warn};
use reqwest::header::CONTENT_LENGTH;

use mp4parse::{read_mp4, Error as Mp4Error, TrackType};
use crate::models::models::VideoDownload;
use crate::service::state::AppState;


/// A simple struct that holds the final MP4 metadata for demonstration.
pub struct VideoMetadata {
    pub duration_seconds: f64,
    pub codec: String,
    pub width: u32,
    pub height: u32,
}

// ===========================
// Download Manager
// ===========================

#[derive(Debug, Clone)]
pub struct DownloadManager {
    state: Arc<AppState>,

    /// We keep the queue of downloads in a separate list; only downloads in progress or
    /// ready to be downloaded are in here. This is distinct from `discovered_videos`,
    /// which can hold a larger set of known videos (including completed).
    download_queue: Arc<Mutex<Vec<VideoDownload>>>,
    client: Arc<reqwest::Client>,
}

impl DownloadManager {
    pub fn new(state: Arc<AppState>) -> Self {
        let client = Arc::new(reqwest::Client::new());
        let download_queue = Arc::new(Mutex::new(Vec::new()));
        Self { state, download_queue, client }
    }

    /// Main loop for scheduling new downloads, removing old content, etc.
    pub async fn run(self: Arc<Self>) {
        loop {
            // 1) Fetch new videos & gather HEAD content_length, add them to discovered
            self.discovery_new_videos().await;

            // 2) Re-sort the entire discovered set according to your multi-criteria
            //    then push the next candidates to the `download_queue`.
            self.update_download_queue().await;

            // 3) Enforce behind-limit, removing old files
            self.enforce_behind_limit().await;

            // 4) Trigger actual downloads if below concurrency limit
            self.download_videos().await;

            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }

    /// Method to stop/drop a given download in progress or queued.
    /// This removes it from the `download_queue`, and marks it as not `downloading`.
    /// If you want to actually remove partial data from disk, do so here as well.
    pub async fn stop_download(&self, video_id: &str) -> bool {
        let mut queue = self.download_queue.lock().await;
        if let Some(pos) = queue.iter().position(|v| v.id == video_id) {
            let removed = queue.remove(pos);

            // Mark as not downloading in discovered_videos as well
            let mut discovered = self.state.discovered_videos.lock().await;
            if let Some(dv) = discovered.get_mut(video_id) {
                dv.downloading = false;
            }

            // Optionally remove partial file from disk:
            if let Some(local_path) = removed.local_path {
                let _ = remove_file(local_path).await;
            }
            true
        } else {
            false
        }
    }

    /// Pull new videos from `ContentDiscovery` and enrich with HEAD requests.
    async fn discovery_new_videos(&self) {
        // 1) Retrieve newly discovered videos
        let new_batch: Vec<VideoDownload> = self
            .state
            .content_discovery
            .fetch_new_videos()
            .await
            .into_iter()
            .map(VideoDownload::from_nostr_video)
            .collect();

        // 2) HEAD-check content_length in parallel
        let enriched_batch =
            fetch_content_lengths_in_parallel(self.client.clone(), new_batch, 20).await;

        // 3) Merge into the main discovered list
        let mut discovered = self.state.discovered_videos.lock().await;
        for mut vid in enriched_batch {
            discovered.insert(vid.id.clone(), vid);
        }
        // End of `discovery_new_videos`.
    }

    /// Decide which videos should be in the `download_queue` and in what order, based
    /// on a multi-criteria stable-sorting.
    /// 1) Collect all not-yet-downloaded videos.
    /// 2) Sort them in the special "two-phase" stable order:
    ///    (a) Until we meet the `target_videos_ahead` or `target_minutes_ahead`,
    ///        prioritize small size, then high score.
    ///    (b) After meeting that threshold, prioritize high score, then small size.
    /// 3) Update the local `download_queue` with this sorted subset (only keep videos
    ///    that are actually missing or incomplete).
    async fn update_download_queue(&self) {
        let discovered_map = self.state.discovered_videos.lock().await;
        let all_videos: Vec<VideoDownload> = discovered_map.values().cloned().collect();
        drop(discovered_map); // drop lock so we can do the sorting below

        // Filter for only videos that do NOT have a local file and are not done
        // (a real check might confirm partial downloads as well).
        let mut candidates: Vec<VideoDownload> = all_videos
            .into_iter()
            .filter(|v| !has_local_file(v) /* or v.local_path.is_none() */ )
            .collect();

        // Sort them with the two-phase stable approach:
        sort_videos_for_download(
            &mut candidates,
            self.state.target_videos_ahead,
            self.state.target_minutes_ahead,
        );

        // Now update the queue. For simplicity, we replace the entire queue with the new ordering.
        let mut queue = self.download_queue.lock().await;
        *queue = candidates;
    }

    /// Remove behind-limit videos from disk. This example simply checks how far behind
    /// our current index we are, and removes anything older than `max_behind_seconds`.
    async fn enforce_behind_limit(&self) {
        let current_idx = *self.state.current_index.lock().await;
        let mut discovered = self.state.discovered_videos.lock().await;

        let mut paths_to_remove = Vec::new();
        for (vid_id, video) in discovered.iter_mut() {
            if let Some(length) = video.length_seconds {
                if length > self.state.max_behind_seconds as f64 {
                    // schedule removal
                    if let Some(local_path) = video.local_path.take() {
                        paths_to_remove.push(local_path);
                    }
                }
            }
        }
        drop(discovered);

        // Remove files outside the lock
        for path in paths_to_remove {
            let _ = remove_file(path).await;
        }
    }

    /// Start downloads if we're below concurrency limit, taking them in the order from
    /// `download_queue`.
    async fn download_videos(&self) {
        // We'll see how many are currently downloading
        let queue_snapshot = {
            let queue = self.download_queue.lock().await;
            queue.clone()
        };

        let concurrent_downloads = queue_snapshot.iter().filter(|v| v.downloading).count();

        let max_downloads = self.state.max_parallel_downloads;

        // If already at concurrency limit, do nothing
        if concurrent_downloads >= max_downloads {
            return;
        }

        // Now pick the top candidates that are NOT downloading
        let to_start = queue_snapshot
            .into_iter()
            .filter(|v| !v.downloading)
            .take(max_downloads - concurrent_downloads);

        for video in to_start {
            // Mark it as downloading in the queue + discovered_videos
            {
                let mut discovered = self.state.discovered_videos.lock().await;
                if let Some(v) = discovered.get_mut(&video.id) {
                    v.downloading = true;
                }
            }
            {
                let mut queue = self.download_queue.lock().await;
                if let Some(qv) = queue.iter_mut().find(|qv| qv.id == video.id) {
                    qv.downloading = true;
                }
            }

            let dm_state = Arc::clone(&self.state);
            let dm_queue = Arc::clone(&self.download_queue);
            let dm_client = Arc::clone(&self.client);
            let video_clone = video.clone();

            let dm = self.clone();
            tokio::spawn(async move {
                match download_video_progressive(
                    Arc::clone(&dm_state),
                    dm_client.clone(),
                    video_clone.clone(),
                )
                    .await
                {
                    Err(e) => {
                        error!("Failed to download {}: {e}", video_clone.url);
                        let mut discovered = dm_state.discovered_videos.lock().await;
                        if let Some(v) = discovered.get_mut(&video_clone.id) {
                            v.downloading = false;
                        }
                        let mut queue = dm_queue.lock().await;
                        if let Some(pos) = queue.iter().position(|qv| qv.id == video_clone.id) {
                            queue.remove(pos);
                        }
                    }

                    Ok(_) => {
                        let mut queue = dm_queue.lock().await;
                        if let Some(pos) = queue.iter().position(|qv| qv.id == video_clone.id) {
                            queue.remove(pos);
                        }
                        let mut playlist = dm_state.playlist.lock().await;
                        playlist.add(video_clone);
                    }
                }
            });
        }
    }
}

// ===========================
// The two-phase stable sorting
// ===========================

/// Utility to check if a `VideoDownload` effectively has a local file.
fn has_local_file(video: &VideoDownload) -> bool {
    video.local_path.is_some()
}

/// Sort videos in a stable manner such that:
///
/// 1. We first take enough videos to meet:
///    - `target_videos_ahead` count, OR
///    - `target_minutes_ahead` total length
///    sorting them by **small content_length ascending, then high score descending**.
///
/// 2. Once we have satisfied the target, subsequent videos are sorted
///    by **score descending, then small content_length ascending**.
///
/// We do this by:
///    - Partitioning the videos into (needed_for_target, leftover)
///      using a running count of how many videos we've added and how many total minutes
///      we've accumulated.
///    - Doing two stable sorts on each group, then concatenating them.
///
/// NOTE: Because we’re doing separate stable sorts and then concatenating, the
/// overall result is stable as well (the partitioning preserves original order).
pub fn sort_videos_for_download(
    videos: &mut Vec<VideoDownload>,
    target_videos_ahead: usize,
    target_minutes_ahead: f64,
) {
    // Step 1) partition into "needed" vs "leftover"
    let (mut needed, mut leftover) = partition_for_target(videos, target_videos_ahead, target_minutes_ahead);

    // Step 2) stable sort within each partition
    //  needed:  by content_length ASC, then score DESC
    needed.sort_by(|a, b| {
        // content_length ASC
        let a_len = a.content_length.unwrap_or(u64::MAX);
        let b_len = b.content_length.unwrap_or(u64::MAX);
        match a_len.cmp(&b_len) {
            std::cmp::Ordering::Equal => {
                // score DESC
                b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)
            }
            other => other,
        }
    });

    // leftover: by score DESC, then content_length ASC
    leftover.sort_by(|a, b| {
        // score DESC
        match b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal) {
            std::cmp::Ordering::Equal => {
                // content_length ASC
                let a_len = a.content_length.unwrap_or(u64::MAX);
                let b_len = b.content_length.unwrap_or(u64::MAX);
                a_len.cmp(&b_len)
            }
            other => other,
        }
    });

    // Step 3) combine them back
    needed.append(&mut leftover);
    *videos = needed;
}

/// Given a list of videos, pick as many as needed to satisfy either the
/// `target_videos_ahead` count or the `target_minutes_ahead` total length.
/// Return (needed, leftover).
fn partition_for_target(
    videos: &[VideoDownload],
    target_videos_ahead: usize,
    target_minutes_ahead: f64,
) -> (Vec<VideoDownload>, Vec<VideoDownload>) {
    let mut needed = Vec::new();
    let mut leftover = Vec::new();

    let mut accumulated_count = 0usize;
    let mut accumulated_minutes = 0f64;

    for v in videos {
        // Once we've met *both* conditions, the rest go to leftover
        if accumulated_count >= target_videos_ahead
            && accumulated_minutes >= target_minutes_ahead
        {
            leftover.push(v.clone());
        } else {
            needed.push(v.clone());
            accumulated_count += 1;
            // use length_seconds or approximate
            if let Some(secs) = v.length_seconds {
                accumulated_minutes += secs / 60.0;
            }
        }
    }

    (needed, leftover)
}

async fn download_video_progressive(
    state: Arc<AppState>,
    client: Arc<reqwest::Client>,
    video: VideoDownload,
) -> Result<(VideoDownload), Box<dyn Error + Send + Sync>> {
    let mut resp = client.get(&video.url).send().await?;
    if !resp.status().is_success() {
        return Err(format!("HTTP request failed with status: {}", resp.status()).into());
    }

    // Possibly store content_length if available:
    if let Some(cl) = resp.content_length() {
        let mut videos_guard = state.discovered_videos.lock().await;
        if let Some(video_mut) = videos_guard.get_mut(&video.id) {
            video_mut.content_length = Some(cl);
        }
    }

    // Create a unique file path
    let file_name = format!("{}.mp4", Uuid::new_v4());
    let file_path = std::env::temp_dir().join(file_name);

    // Store the local_path
    {
        let mut discovered = state.discovered_videos.lock().await;
        if let Some(video_mut) = discovered.get_mut(&video.id) {
            video_mut.local_path = Some(file_path.clone());
        }
    }

    let mut file = File::create(&file_path).await?;
    let mut parse_buffer: Vec<u8> = Vec::new();
    let mut downloaded_bytes = 0u64;
    let mut metadata_extracted = false;

    // Download in chunks
    while let Some(chunk) = resp.chunk().await? {
        // 1) Check storage budget
        {
            let mut storage = state.current_storage_bytes.lock().await;
            if *storage + (chunk.len() as u64) > state.max_storage_bytes {
                warn!("Storage budget exceeded while downloading {}", video.url);
                return Err("Storage budget exceeded".into());
            }
            *storage += chunk.len() as u64;
        }

        // 2) Write to disk
        file.write_all(&chunk).await?;
        downloaded_bytes += chunk.len() as u64;

        // 3) Update progress
        {
            let mut discovered = state.discovered_videos.lock().await;
            if let Some(video_mut) = discovered.get_mut(&video.id) {
                video_mut.downloaded_bytes = downloaded_bytes;
                if video_mut.content_length.is_none() {
                    if let Some(cl) = resp.content_length() {
                        video_mut.content_length = Some(cl);
                    }
                }

                let now = std::time::Instant::now();
                match video_mut.last_speed_update_instant {
                    None => {
                        video_mut.last_speed_update_instant = Some(now);
                        video_mut.last_speed_update_bytes = downloaded_bytes;
                        video_mut.download_speed_bps = 0.0;
                    }
                    Some(prev_time) => {
                        let dt = now.duration_since(prev_time).as_secs_f64();
                        if dt >= 1.0 {
                            let bytes_diff = downloaded_bytes - video_mut.last_speed_update_bytes;
                            video_mut.download_speed_bps = bytes_diff as f64 / dt;
                            video_mut.last_speed_update_instant = Some(now);
                            video_mut.last_speed_update_bytes = downloaded_bytes;
                        }
                    }
                }
            }
        }

        // Attempt to parse partial metadata (moov box)
        if !metadata_extracted {
            parse_buffer.extend_from_slice(&chunk);
            match try_parse_mp4_in_blocking_thread(parse_buffer.clone()).await {
                Ok(Some(metadata)) => {
                    update_metadata(state.clone(), &video.id, &file_path, metadata).await;
                    metadata_extracted = true;

                    #[cfg(debug_server)]
                    if let Ok(jpeg_data) = ffmpeg_extractor::extract_first_frame_to_jpeg(&parse_buffer) {
                        let thumb_path = std::env::temp_dir()
                            .join(format!("thumb_{}.jpg", Uuid::new_v4()));
                        if let Err(e) = write_image_to_jpeg(&jpeg_data, &thumb_path).await {
                            warn!("Could not write thumbnail: {}", e);
                        } else {
                            // Update discovered
                            let mut list = state.discovered_videos.lock().await;
                            if let Some(video_mut) = list.get_mut(&video.id) {
                                video_mut.thumbnail_path = Some(thumb_path);
                            }
                        }
                    }

                }
                Ok(None) => { /* not enough data yet */ }
                Err(_) => { /* parse error is non-fatal here, ignore */ }
            }
        }
    }

    file.flush().await?;
    drop(file);

    // If never extracted metadata, parse final buffer
    if !metadata_extracted {
        match try_parse_mp4_in_blocking_thread(parse_buffer).await {
            Ok(Some(metadata)) => {
                info!("Parsed final MP4 for {} ({}s)", video.url, metadata.duration_seconds);
                update_metadata(state.clone(), &video.id, &file_path, metadata).await;
            }
            Ok(None) => {
                warn!("Could not parse MP4 metadata for {} (possibly no moov box)", video.url);
            }
            Err(e) => {
                warn!("Error parsing final MP4 data for {}: {e}", video.url);
            }
        }
    }

    // Mark downloading = false in discovered
    {
        let mut list = state.discovered_videos.lock().await;
        if let Some(video_mut) = list.get_mut(&video.id) {
            video_mut.downloading = false;
        }
    }

    debug!("Downloaded {} => size: {} bytes as {}", video.url, downloaded_bytes, video.id);
    Ok((video))
}


fn parse_mp4_entire(parse_buffer: &[u8]) -> Result<Option<VideoMetadata>, Mp4Error> {
    let context = read_mp4(&mut std::io::Cursor::new(parse_buffer))?;
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
                    }
                    _ => "non-video".to_string(),
                }
            } else {
                "unknown".to_string()
            };
            let metadata = VideoMetadata {
                duration_seconds,
                codec,
                width,
                height,
            };
            return Ok(Some(metadata));
        }
    }
    Ok(None)
}

async fn try_parse_mp4_in_blocking_thread(
    parse_buffer: Vec<u8>,
) -> Result<Option<VideoMetadata>, Mp4Error> {
    let parse_result = tokio::task::spawn_blocking(move || parse_mp4_entire(&parse_buffer))
        .await
        .map_err(|join_err| {
            Mp4Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("spawn_blocking join error: {join_err}"),
            ))
        })?;

    parse_result
}

/// Update metadata in discovered_videos
async fn update_metadata(
    state: Arc<AppState>,
    video_id: &str,
    file_path: &std::path::Path,
    metadata: VideoMetadata,
) {
    let mut list = state.discovered_videos.lock().await;
    if let Some(video) = list.get_mut(video_id) {
        video.local_path = Some(file_path.to_path_buf());
        video.length_seconds = Some(metadata.duration_seconds);
        video.format = Some(metadata.codec.to_string());
        if metadata.width > 0 {
            video.width = Some(metadata.width);
        }
        if metadata.height > 0 {
            video.height = Some(metadata.height);
        }
    }
}

// ===========================
// HEAD fetch utility
// ===========================
pub async fn fetch_content_lengths_in_parallel(
    client: Arc<reqwest::Client>,
    videos: Vec<VideoDownload>,
    parallel_calls: usize,
) -> Vec<VideoDownload> {
    stream::iter(videos)
        .map(|mut video| {
            let client = client.clone();
            async move {
                if video.content_length.is_some() {
                    return video;
                }

                let response = match client.head(&video.url).send().await {
                    Ok(resp) => resp,
                    Err(e) => {
                        warn!("HEAD request error for {}: {}", video.url, e);
                        return video;
                    }
                };

                if !response.status().is_success() {
                    warn!("HEAD request failed for {}: status={}", video.url, response.status());
                    return video;
                }

                if let Some(length) = response
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|val| val.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    video.content_length = Some(length);
                }

                video
            }
        })
        .buffered(parallel_calls)
        .collect()
        .await
}
