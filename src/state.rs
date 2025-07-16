use crate::bitfield::Bitfield;
use crate::db::DB;
use crate::dot_torrent::DotTorrent;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct State<T: DB> {
    db: T,
    // torrents' metadata, where key is info hash
    pub data: HashMap<[u8; 20], SharedMetadata>,
}

impl<T: DB> State<T> {
    pub fn new(mut db: T) -> anyhow::Result<Self> {
        let data: HashMap<[u8; 20], Metadata> = serde_json::from_reader(&mut db)?;
        let data = data
            .into_iter()
            .map(|(k, v)| (k, Arc::new(Mutex::new(v))))
            .collect();
        Ok(Self { db, data })
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
