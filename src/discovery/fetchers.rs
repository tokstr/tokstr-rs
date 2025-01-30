use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use nostr_sdk::{Client, Filter, FromBech32, Kind, PublicKey, RelayPoolNotification, SubscriptionId, ToBech32};
use nostr_sdk::client::Error;
use nostr_sdk::pool::Output;
use tokio::sync::{mpsc::{self, UnboundedReceiver}, Mutex, MutexGuard};

use crate::discovery::models::{UserData, NostrVideo};
use crate::discovery::parsers::{parse_event_as_video, parse_user_metadata};

#[derive(Debug, Clone)]
pub struct ContentDiscovery {
    _client: Arc<Client>,
    _video_subscription_id: SubscriptionId,
    video_receiver: Arc<Mutex<UnboundedReceiver<NostrVideo>>>,

    /// In-memory map of "author bech32 => user metadata".
    /// We store it so we only fetch each author’s metadata once.
    known_authors: Arc<Mutex<HashMap<String, UserData>>>,
}

impl ContentDiscovery {
    /// Creates a `ContentDiscovery`, connects to given relays, subscribes to video kinds, and
    /// spawns a background task that automatically enriches each video with author
    /// metadata. The final `Video` (with metadata) is then queued in `video_receiver`.
    pub async fn new(relays: Vec<String>, client: Arc<Client>) -> Result<Self, Error> {
        // 2) Add and connect to relays
        let _cloned = client.clone();

        for url in &relays {
            client.add_relay(url).await?;
        }
        client.connect().await;

        // 3) Subscribe to the “video” kinds (34235 & 34236).
        let filter = Filter::new().kinds(vec![Kind::Custom(34235), Kind::Custom(34236)]);
        let subscription_output: Output<SubscriptionId> = client.subscribe(vec![filter], None).await?;
        let video_subscription_id = subscription_output.val;

        // 4) Set up a channel for “finished” videos
        let (video_sender, video_receiver_) = mpsc::unbounded_channel::<NostrVideo>();

        let video_receiver = Arc::new(Mutex::new(video_receiver_));

        // 5) Shared cache for metadata
        let known_authors = Arc::new(Mutex::new(HashMap::new()));

        // 6) Spawn a background task that:
        //    - continuously reads from `client.notifications()`
        //    - for each “video” event, fetches the metadata (if needed),
        //    - enriches the `Video`,
        //    - sends it into `video_sender`.
        let known_authors_bg = Arc::clone(&known_authors);

        let cloned_ = client.clone();
        tokio::spawn(async move {
            let mut notifications = cloned_.notifications();
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
                                // Pull out the npub into a separate variable so we don’t keep an immutable reference to `video`
                                let npub_opt = video.user.npub.clone();

                                if let Some(npub_str) = npub_opt {
                                    maybe_fetch_and_set_metadata(
                                        cloned_.clone(),
                                        &npub_str,
                                        &known_authors_bg,
                                        &mut video,
                                    ).await;
                                }

                                // Now the immutable borrow is gone, so we can safely send `video`
                                let _ = video_sender.send(video);
                            }
                        }
                    _ => { /* ignore other events */ }
                }
            }
        });

        Ok(Self {
            _client: client.clone(),
            _video_subscription_id: video_subscription_id,
            video_receiver,
            known_authors,
        })
    }

    /// Fetch newly discovered “videos” that have *already* been enriched
    /// with the author’s metadata. Because we drain `video_receiver`,
    /// each returned `Video` is new (no duplication).
    pub async fn fetch_new_videos(&self) -> Vec<NostrVideo> {
        let mut result = Vec::new();
        while let Ok(video) = self.video_receiver.lock().await.try_recv() {
            result.push(video);
        }
        result
    }


}


/// Called by the background task to fetch metadata for a given author
/// if we don’t already have it in `known_authors_bg`.
/// Then we update the `video.user` field.
async fn maybe_fetch_and_set_metadata(
    client: Arc<Client>,
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
        video.user = user_data;
        return;
    }

    let pubkey = match PublicKey::from_bech32(npub_str).ok()

    {
        Some(pk) => pk,
        None => return
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