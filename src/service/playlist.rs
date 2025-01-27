#[derive(Debug)]
pub struct Playlist {
    id: String,
    current_position: Option<usize>,
    items: Vec<String>,
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

    fn add(&mut self, video: String) {
        let idx = self.items.len();
        self.items.push(video.clone());
        self.items_by_id.insert(video, idx);
    }

    fn current(&self) -> Option<&String> {
        if let Some(pos) = self.current_position {
            return self.items.get(pos);
        }
        None
    }

    fn next(&mut self) -> Option<&String> {
        if let Some(pos) = self.current_position {
            if pos + 1 < self.items.len() {
                self.current_position = Some(pos + 1);
                return Some(&self.items[pos + 1]);
            }
        }
        None
    }

    fn prev(&mut self) -> Option<&String> {
        if let Some(pos) = self.current_position {
            if pos > 0 {
                self.current_position = Some(pos - 1);
                return Some(&self.items[pos - 1]);
            }
        }
        None
    }
}
