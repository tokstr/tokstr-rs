use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{io::SeekFrom};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;
use crate::service::state::AppState;
use crate::models::models::VideoDownload;

#[derive(Debug, Deserialize)]
pub struct VideoQuery {
    pub index: usize,
}

/// Serve video in partial content (Range) if requested, or full if no Range is given.
///
/// Example usage: GET /video.mp4?index=0
pub async fn stream_video(
    State(state): State<AppState>,
    Query(query): Query<VideoQuery>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    let index = query.index;
    let maybe_path = {
        let list = state.discovered_videos.lock().await.to_vec();
        list.get(index).and_then(|v| v.local_path.clone())
    };

    let Some(path) = maybe_path else {
        return Err(StatusCode::NOT_FOUND);
    };

    let meta = tokio::fs::metadata(&path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let file_size = meta.len();

    // Check if we have a Range header
    let range_header = headers.get(header::RANGE).and_then(|val| val.to_str().ok());

    // If no Range header, return entire file
    if range_header.is_none() {
        let file = File::open(&path).await.map_err(|_| StatusCode::NOT_FOUND)?;
        let stream = ReaderStream::new(file);
        let body = Body::from_stream(stream);

        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "video/mp4")
            .body(body)
            .unwrap());
    }

    // We do have a Range header, parse it
    let range_str = range_header.unwrap();
    let (start, end) = parse_range_header(range_str, file_size)?;

    // Ensure start < file_size
    if start >= file_size {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }

    // If end is beyond the current downloaded size, clamp it
    let end = end.min(file_size - 1);
    let chunk_size = end - start + 1;

    // Seek file to 'start'
    let mut file = File::open(&path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    file.seek(SeekFrom::Start(start)).await.map_err(|_| StatusCode::NOT_FOUND)?;

    // We only read `chunk_size` bytes
    let limited_reader = file.take(chunk_size);
    let stream = ReaderStream::new(limited_reader).map(|res| {
        res.map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
            .map(Bytes::from)
    });

    let body = Body::from_stream(stream);

    // Build partial content response
    let content_range = format!("bytes {}-{}/{}", start, end, file_size);

    Ok(Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, "video/mp4")
        .header(header::CONTENT_RANGE, content_range)
        .header(header::ACCEPT_RANGES, "bytes")
        .body(body)
        .unwrap())
}

/// A simple Range header parser that expects: "bytes=start-end".
/// Example: "bytes=0-1023" => (0, 1023).
/// If "bytes=100-" => (100, file_size-1).
fn parse_range_header(range_str: &str, file_size: u64) -> Result<(u64, u64), StatusCode> {
    // Ensure format
    if !range_str.starts_with("bytes=") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let no_prefix = &range_str[6..];
    let parts: Vec<&str> = no_prefix.split('-').collect();
    if parts.len() != 2 {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Parse start
    let start: u64 = parts[0].parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    // Parse end
    if parts[1].is_empty() {
        // "bytes=100-" means from 100 to the end
        let end = file_size - 1;
        Ok((start, end))
    } else {
        let end: u64 = parts[1].parse().map_err(|_| StatusCode::BAD_REQUEST)?;
        Ok((start, end))
    }
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub current_index: usize,
    pub videos: Vec<VideoDownload>,
    pub used_storage_bytes: u64,
    pub max_storage_bytes: u64,
    pub total_download_speed_bps: f64,
    pub total_downloaded_minutes: f64,
}

/// Returns JSON status of the system.
pub async fn get_status(State(state): State<AppState>) -> impl IntoResponse {
    let list = state.discovered_videos.lock().await.to_vec();
    let current_idx = *state.current_index.lock().await;
    let used_storage = *state.current_storage_bytes.lock().await;

    let total_speed = list.iter().map(|v| v.download_speed_bps).sum();

    // Simple approach: if we have length_seconds, we consider that “fully downloaded”
    // if local_path.is_some() OR v.downloaded_bytes >= v.content_length.unwrap_or(u64::MAX).
    // Summation:
    let mut total_minutes = 0.0;
    for v in list.iter() {
        if let Some(length) = v.length_seconds {
            // either partial or full...
            // if you want to be partial, do ratio = (v.downloaded_bytes as f64 / v.content_length as f64),
            // but for simplicity, let's just add the full length if we have it.
            total_minutes += length / 60.0;
        }
    }

    let status = StatusResponse {
        current_index: current_idx,
        videos: list.clone(),
        used_storage_bytes: used_storage,
        max_storage_bytes: state.max_storage_bytes,
        total_download_speed_bps: total_speed,
        total_downloaded_minutes: total_minutes,
    };

    Json(status)
}

#[derive(Debug, Deserialize)]
pub struct SetIndexRequest {
    pub index: usize,
}

pub async fn set_index(
    State(state): State<AppState>,
    Json(payload): Json<SetIndexRequest>,
) -> impl IntoResponse {
    let mut idx = state.current_index.lock().await;
    *idx = payload.index;
    "OK"
}


#[derive(Debug, Deserialize)]
pub struct ThumbnailQuery {
    pub index: usize,
}

pub async fn get_thumbnail(
    State(state): State<AppState>,
    Query(query): Query<ThumbnailQuery>,
) -> Result<Response, StatusCode> {
    let index = query.index;

    let maybe_thumb = {
        let list = state.discovered_videos.lock().await.to_vec();
        list.get(index)
            .and_then(|v| v.thumbnail_path.clone())
    };

    let Some(thumb_path) = maybe_thumb else {
        return Err(StatusCode::NOT_FOUND);
    };

    // read the file
    let data = match tokio::fs::read(&thumb_path).await {
        Ok(b) => b,
        Err(_) => return Err(StatusCode::NOT_FOUND),
    };

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "image/jpeg")
        .body(Body::from(data))
        .unwrap())
}