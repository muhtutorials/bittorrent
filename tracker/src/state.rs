use crate::torrents::Torrents;
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
pub struct AppState {
    pub torrents: Arc<Mutex<Torrents>>,
}
