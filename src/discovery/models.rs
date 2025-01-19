use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrVideo {
    pub id: String,
    pub user: UserData,
    pub title: String,
    pub song_name: String,
    pub likes: String,
    pub comments: String,
    pub url: String,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserData {
    pub npub: Option<String>,
    pub name: Option<String>,
    pub profile_picture: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VideoVariant {
    pub title: Option<String>,
    pub resolution: Option<String>,
    pub url: Option<String>,
    pub hash: Option<String>,
    pub mime_type: Option<String>,
    pub images: Vec<String>,
    pub fallbacks: Vec<String>,
    pub service: Option<String>,
}
