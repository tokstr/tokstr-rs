use std::time::Duration;
use nostr_sdk::{Client, Filter, Kind, PublicKey, ToBech32};
use nostr_sdk::client::Error;
use crate::discovery::models::{UserData, Video};
use crate::discovery::parsers::{parse_event_as_video, parse_user_metadata};
use crate::models::models::VideoDownload;

#[derive(Debug)]
pub struct VideosAPI {
    /// Stores the relay URLs you connected to (if you need to refer to them).
    relays: Vec<String>,

    /// The final list of retrieved videos.
    pub list_videos: Vec<VideoDownload>,
}

impl VideosAPI {
    /// Create a new `VideosAPI` and **immediately** fetch videos from the provided relays.
    ///
    /// Once this returns, `list_videos` is already populated with the retrieved videos.
    pub async fn new(relays: Vec<String>) -> Result<Self, Error> {
        let list_videos = Self::fetch_from_relays(&relays).await?;
        let downloads: Vec<VideoDownload> = list_videos
            .iter()
            .map(|v| v.to_download())
            .collect();

        Ok(Self { relays, list_videos: downloads })
    }

    /// Fetch from the Nostr relays (kinds 34235 & 34236 for videos, plus metadata).
    async fn fetch_from_relays(relays: &[String]) -> Result<Vec<Video>, Error> {
        let client = Client::default();

        // 1) Connect to each relay
        for relay_url in relays {
            client.add_relay(relay_url).await?;
        }
        client.connect().await;

        // 2) Fetch events for the "video" kinds
        let filter = Filter::new()
            .kinds(vec![Kind::Custom(34235), Kind::Custom(34236)]);
        let events = client
            .fetch_events(vec![filter], Duration::from_secs(10))
            .await?;

        // 3) Parse each event into zero or more `Video`s
        let mut videos: Vec<Video> = events
            .iter()
            .flat_map(|event| parse_event_as_video(event))
            .collect();

        if videos.is_empty() {
            // If no videos found, just disconnect and return
            client.disconnect().await?;
            return Ok(videos);
        }

        // 4) Fetch metadata for the authors (pubkeys) we found
        let authors: Vec<PublicKey> = videos
            .iter()
            .filter_map(|v| v.user.npub.as_ref())
            .filter_map(|npub_str| PublicKey::parse(npub_str).ok())
            .collect();

        let metadata_filter = Filter::new().kind(Kind::Metadata).authors(authors);
        let metadata_events = client
            .fetch_events(vec![metadata_filter], Duration::from_secs(10))
            .await?;

        // 5) Parse user metadata
        let user_data_map = parse_user_metadata(&metadata_events);

        // 6) Enrich each video with user data (name, profile picture, etc.)
        for video in &mut videos {
            if let Some(npub_str) = &video.user.npub {
                if let Ok(pubkey) = PublicKey::parse(npub_str) {
                    if let Ok(pubkey_bech32) = pubkey.to_bech32() {
                        if let Some(user_data) = user_data_map.get(&pubkey_bech32) {
                            video.user = UserData {
                                npub: Some(pubkey_bech32),
                                name: user_data.name.clone(),
                                profile_picture: user_data.profile_picture.clone(),
                            };
                        }
                    }
                }
            }
        }

        // 7) Disconnect once done
        client.disconnect().await?;
        Ok(videos)
    }

}

