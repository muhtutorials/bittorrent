use crate::bit_vec::BitVec;
use crate::db::FileDB;
use crate::dot_torrent::DotTorrent;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct State {
    db: FileDB,
    // Torrents' metadata, where key is info hash.
    pub data: Vec<SharedMetadata>,
}

impl State {
    pub fn new(db: FileDB) -> anyhow::Result<Self> {
        let data: Vec<Metadata> = serde_json::from_slice(db.data())?;
        let data = data
            .into_iter()
            .map(|value| Arc::new(Mutex::new(value)))
            .collect();
        Ok(Self { db, data })
    }

    // pub fn save(&self) -> anyhow::Result<Self> {
    // }
}

#[derive(Deserialize, Clone)]
pub struct Metadata {
    pub id: usize,
    pub path: PathBuf,
    pub dot_torrent: DotTorrent,
    pub peer_id: [u8; 20],
    pub port: u16,
    pub uploaded: usize,
    pub downloaded: usize,
    pub left: usize,
    pub pieces: BitVec,
    pub finished: bool,
}

pub type SharedMetadata = Arc<Mutex<Metadata>>;
