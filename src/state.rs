use crate::bitfield::Bitfield;
use crate::db::FileDB;
use crate::dot_torrent::DotTorrent;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct State {
    db: FileDB,
    // torrents' metadata, where key is info hash
    pub data: BTreeMap<[u8; 20], SharedMetadata>,
}

impl State {
    pub fn new(db: FileDB) -> anyhow::Result<Self> {
        let data: BTreeMap<[u8; 20], Metadata> = serde_json::from_slice(db.data())?;
        let data = data
            .into_iter()
            .map(|(k, v)| (k, Arc::new(Mutex::new(v))))
            .collect();
        Ok(Self { db, data })
    }

    pub fn save(&self) -> anyhow::Result<Self> {
        
    }
}

#[derive(Deserialize, Clone)]
pub struct Metadata {
    pub path: PathBuf,
    pub dot_torrent: DotTorrent,
    pub peer_id: [u8; 20],
    pub port: u16,
    pub uploaded: usize,
    pub downloaded: usize,
    pub left: usize,
    pub pieces: Bitfield,
    pub finished: bool,
}

pub type SharedMetadata = Arc<Mutex<Metadata>>;
