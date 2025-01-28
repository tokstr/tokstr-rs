use crate::models::models::VideoDownload;

#[derive(Debug)]
pub struct Playlist {
    id: String,
    current_position: Option<usize>,
    items: Vec<VideoDownload>,
    items_by_id: std::collections::HashMap<String, usize>,
}

impl Playlist {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            current_position: None,
            items: Vec::new(),
            items_by_id: std::collections::HashMap::new(),
        }
    }

    pub fn add(&mut self, video: VideoDownload) {
        let idx = self.items.len();
        self.items.push(video.clone());
        self.items_by_id.insert(video.id, idx);
    }

    pub fn current(&self) -> Option<&VideoDownload> {
        if let Some(pos) = self.current_position {
            return self.items.get(pos);
        }
        None
    }

    pub fn next(&mut self) -> Option<&VideoDownload> {
        if let Some(pos) = self.current_position {
            if pos + 1 < self.items.len() {
                self.current_position = Some(pos + 1);
                return Some(&self.items[pos + 1]);
            }
        }
        None
    }

    pub fn prev(&mut self) -> Option<&VideoDownload> {
        if let Some(pos) = self.current_position {
            if pos > 0 {
                self.current_position = Some(pos - 1);
                return Some(&self.items[pos - 1]);
            }
        }
        None
    }

    pub fn as_vec(&self) -> Vec<VideoDownload> {
        self.items.clone()
    }
}
