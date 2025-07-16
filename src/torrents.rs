use crate::db::DB;
use crate::peer::Peer;
use crate::state::{SharedMetadata, State};
use crate::tracker::PeerList;
use crate::tracker::query_tracker;
use std::collections::HashMap;
use anyhow::Context;

pub struct Torrents<T: DB> {
    state: State<T>,
    torrents: HashMap<[u8; 20], Torrent>,
}

impl<T: DB> Torrents<T> {
    pub fn new(db: T) -> Self {
        Torrents {
            state: State::new(db)?,
            torrents: HashMap::new(),
        }
    }

    pub async fn start(&mut self) -> anyhow::Result<()> {
        for (info_hash, metadata) in &self.state.data {
            let torrent = Torrent::new(info_hash.clone(), metadata.clone());
            self.torrents.insert(info_hash.clone(), torrent);
        }
        for (_, mut torrent) in &self.torrents {
            torrent.get_uploaders()?
        }
        Ok(())
    }
}

pub struct Torrent {
    pub info_hash: [u8; 20],
    pub metadata: SharedMetadata,
    pub interval: usize,
    pub peer_list: PeerList,
    pub uploaders: Vec<Peer>,
    pub downloaders: Vec<Peer>,
}

impl Torrent {
    pub fn new(info_hash: [u8; 20], metadata: SharedMetadata) -> Self {
        Self {
            info_hash,
            metadata,
            interval: 0,
            peer_list: PeerList(Vec::new()),
            uploaders: Vec::new(),
            downloaders: Vec::new(),
        }
    }

    pub async fn get_uploaders(&mut self) -> anyhow::Result<()> {
        let metadata = &self.metadata.lock().context("failed to get lock on metadata")?;
        let resp = query_tracker(&metadata.dot_torrent).await?;
        self.interval = resp.interval;
        self.peer_list = resp.peers;
        Ok(())
    }
}
