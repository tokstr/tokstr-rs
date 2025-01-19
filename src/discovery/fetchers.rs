use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use nostr_sdk::{
    Client, Event, Filter, Kind, PublicKey, RelayPoolNotification,
    SubscriptionId, ToBech32,
};
use nostr_sdk::client::Error;
use nostr_sdk::pool::Output;
use tokio::sync::{
    mpsc::{self, UnboundedReceiver},
    Mutex,
};

use crate::discovery::models::{UserData, NostrVideo};
use crate::discovery::parsers::{parse_event_as_video, parse_user_metadata};

#[derive(Debug)]
pub struct ContentDiscovery {
    client: Client,
    _video_subscription_id: SubscriptionId,
    video_receiver: UnboundedReceiver<NostrVideo>,

    /// In-memory map of "author bech32 => user metadata".
    /// We store it so we only fetch each author’s metadata once.
    known_authors: Arc<Mutex<HashMap<String, UserData>>>,
}

impl ContentDiscovery {
    /// Creates a `ContentDiscovery`, connects to given relays, subscribes to video kinds, and
    /// spawns a background task that automatically enriches each video with author
    /// metadata. The final `Video` (with metadata) is then queued in `video_receiver`.
    pub async fn new(relays: Vec<String>) -> Result<Self, Error> {
        // 1) Create client
        let client = Client::default();

        // 2) Add and connect to relays
        for url in &relays {
            client.add_relay(url).await?;
        }
        client.connect().await;

        // 3) Subscribe to the “video” kinds (34235 & 34236).
        let filter = Filter::new().kinds(vec![Kind::Custom(34235), Kind::Custom(34236)]);
        let subscription_output: Output<SubscriptionId> =
            client.subscribe(vec![filter], None).await?;
        let video_subscription_id = subscription_output.val;

        // 4) Set up a channel for “finished” videos
        let (video_sender, video_receiver) = mpsc::unbounded_channel::<NostrVideo>();

        // 5) Shared cache for metadata
        let known_authors = Arc::new(Mutex::new(HashMap::new()));

        // 6) Spawn a background task that:
        //    - continuously reads from `client.notifications()`
        //    - for each “video” event, fetches the metadata (if needed),
        //    - enriches the `Video`,
        //    - sends it into `video_sender`.
        let mut notifications = client.notifications();
        let known_authors_bg = Arc::clone(&known_authors);
        let client_bg = client.clone();

        tokio::spawn(async move {
            while let Ok(notification) = notifications.recv().await {
                match notification {
                    RelayPoolNotification::Event {
                        relay_url: _relay_url,
                        subscription_id,
                        event,
                    }
                    if matches!(event.kind, Kind::Custom(34235) | Kind::Custom(34236)) =>
                        {
                            // Parse into zero or more Videos
                            let videos = parse_event_as_video(&event);
                            for mut video in videos {
                                // Enrich with metadata
                                if let Some(npub_str) = &video.user.npub {
                                    Self::maybe_fetch_and_set_metadata(
                                        &client_bg,
                                        npub_str,
                                        &known_authors_bg,
                                        &mut video.clone(),
                                    )
                                        .await;
                                }

                                // Send final, enriched Video to the service
                                let _ = video_sender.send(video);
                            }
                        }
                    _ => { /* ignore other events */ }
                }
            }
        });

        Ok(Self {
            client,
            _video_subscription_id: video_subscription_id,
            video_receiver,
            known_authors,
        })
    }

    /// Called by the background task to fetch metadata for a given author
    /// if we don’t already have it in `known_authors_bg`.
    /// Then we update the `video.user` field.
    async fn maybe_fetch_and_set_metadata(
        client: &Client,
        npub_str: &str,
        known_authors_bg: &Arc<Mutex<HashMap<String, UserData>>>,
        video: &mut NostrVideo,
    ) {
        // Already have user in cache?
        let cached = {
            let map = known_authors_bg.lock().await;
            map.get(npub_str).cloned()
        };

        if let Some(user_data) = cached {
            // Found in cache: just set it
            video.user = user_data;
            return;
        }

        // Otherwise, we parse `npub_str` -> `PublicKey`. Try bech32 or hex:
        let pubkey = match PublicKey::parse(npub_str)
            .ok()
            .or_else(|| PublicKey::from_hex(npub_str).ok())
        {
            Some(pk) => pk,
            None => return, // cannot parse author
        };

        // Ephemeral fetch of kind = Metadata for that author, with 10s timeout
        let filter = Filter::new().kind(Kind::Metadata).author(pubkey);
        if let Ok(events) = client.fetch_events(vec![filter], Duration::from_secs(10)).await {
            // If we found something, parse user metadata
            let user_data_map = parse_user_metadata(&events);
            if let Ok(pubkey_bech32) = pubkey.to_bech32() {
                if let Some(user_data) = user_data_map.get(&pubkey_bech32) {
                    // Cache it
                    let user_data_cloned = user_data.clone();
                    {
                        let mut map = known_authors_bg.lock().await;
                        map.insert(pubkey_bech32.clone(), user_data_cloned.clone());
                    }
                    // Update the video
                    video.user = user_data_cloned;
                }
            }
        }
    }

    /// Fetch newly discovered “videos” that have *already* been enriched
    /// with the author’s metadata. Because we drain `video_receiver`,
    /// each returned `Video` is new (no duplication).
    pub fn fetch_new_videos(&mut self) -> Vec<NostrVideo> {
        let mut result = Vec::new();
        while let Ok(video) = self.video_receiver.try_recv() {
            result.push(video);
        }
        result
    }

    /// Shutdown the client
    /// (Closes subscriptions, disconnects relays, etc.)
    pub async fn shutdown(&self) -> Result<(), Error> {
        self.client.shutdown().await
    }
}
