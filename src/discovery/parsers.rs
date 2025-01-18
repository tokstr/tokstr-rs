use std::collections::HashMap;
use nostr_sdk::prelude::*;
use url::Url;

use crate::discovery::models::{UserData, Video, VideoVariant};

/// A module containing all parsing-related code.
/// We could also structure it as a struct with methods, but here's a simple approach.
pub fn parse_event_as_video(event: &Event) -> Vec<Video> {
    // 1) Gather all video variants from the event tags
    let video_variants = parse_video_variants(event);

    // 2) Filter them to only valid (hash + URL) combos and build `Video`.
    let mut videos = Vec::new();
    for variant in video_variants {
        if let (Some(hash), Some(url)) = (&variant.hash, &variant.url) {
            if is_valid_http_url(url) {
                let user_npub = event.pubkey.to_bech32().ok();
                videos.push(Video {
                    id: hash.clone(),
                    user: UserData {
                        npub: user_npub,
                        name: None,
                        profile_picture: None,
                    },
                    title: variant.title.clone().unwrap_or_default(),
                    song_name: "Unknown".to_string(),
                    comments: "".to_string(),
                    likes: "".to_string(),
                    url: url.clone(),
                });
            }
        }
    }
    videos
}


pub fn parse_video_variants(event: &Event) -> Vec<VideoVariant> {
    let mut variants = Vec::new();

    // `event.tags` is of type `Tags` in nostr 0.38+
    // We can iterate over it by calling `.iter()`
    for tag in event.tags.iter() {
        // Each `tag` is a `Tag` struct. We can call `tag.as_slice()` to get `&[String]`.
        let slices = tag.as_slice();

        // We need at least one string to check "imeta"
        if !slices.is_empty() && slices[0] == "imeta" {
            let mut fields: HashMap<String, Vec<String>> = HashMap::new();

            // Skip the first item ("imeta"), and parse the rest
            for chunk in slices.iter().skip(1) {
                let parts: Vec<&str> = chunk.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }
                let key = parts[0].trim().to_string();
                let value = parts[1..].join(" ").trim().to_string();
                fields.entry(key).or_default().push(value);
            }

            // Extract fields
            let dim       = fields.get("dim").and_then(|v| v.first()).cloned();
            let title     = fields.get("title").and_then(|v| v.first()).cloned();
            let url       = fields.get("url").and_then(|v| v.first()).cloned();
            let hash      = fields.get("x").and_then(|v| v.first()).cloned();
            let mime_type = fields.get("m").and_then(|v| v.first()).cloned();
            let service   = fields.get("service").and_then(|v| v.first()).cloned();
            let images    = fields.get("image").cloned().unwrap_or_default();
            let fallbacks = fields.get("fallback").cloned().unwrap_or_default();

            variants.push(VideoVariant {
                title,
                resolution: dim,
                url,
                hash,
                mime_type,
                images,
                fallbacks,
                service,
            });
        }
    }

    variants
}

pub fn parse_user_metadata(metadata_events: &Events) -> HashMap<String, UserData> {
    let mut map: HashMap<String, UserData> = HashMap::new();
    for meta_event in metadata_events.iter() {
        if let Ok(pubkey_bech32) = meta_event.pubkey.to_bech32() {
            // Attempt to parse JSON content for name/picture
            if let Ok(json_val) = serde_json::from_str::<Value>(&meta_event.content) {
                let name = json_val["name"].as_str().map(|s| s.to_string());
                let picture_url = json_val["picture"].as_str().map(|s| s.to_string());

                map.insert(
                    pubkey_bech32,
                    UserData {
                        npub: None,
                        name,
                        profile_picture: picture_url,
                    },
                );
            }
        }
    }
    map
}

pub fn is_valid_http_url(url: &str) -> bool {
    if let Ok(parsed) = Url::parse(url) {
        let scheme = parsed.scheme();
        let host = parsed.host_str().unwrap_or("");
        (scheme == "http" || scheme == "https") && !host.is_empty()
    } else {
        false
    }
}
