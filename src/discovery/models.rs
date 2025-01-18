use crate::models::models::VideoDownload;

#[derive(Debug, Clone)]
pub struct Video {
    pub id: String,
    pub user: UserData,
    pub title: String,
    pub song_name: String,
    pub likes: String,
    pub comments: String,
    pub url: String,
}


impl Video {
    pub fn to_download(&self) -> VideoDownload {
       VideoDownload{
           url: self.url.clone(),
           local_path: None,
           downloading: false,
           video_id: Default::default(),
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
#[derive(Debug, Clone)]
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
