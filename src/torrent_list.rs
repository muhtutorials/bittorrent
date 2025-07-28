use crate::db::FileDB;
use crate::state::State;
use crate::torrent::Torrent;
use std::collections::HashMap;

pub struct TorrentList {
    state: State,
    torrents: HashMap<[u8; 20], Torrent>,
}

impl TorrentList {
    pub fn new(db: FileDB) -> anyhow::Result<Self> {
        Ok(TorrentList {
            state: State::new(db)?,
            torrents: HashMap::new(),
        })
    }

    pub async fn start(&mut self) -> anyhow::Result<()> {
        for (info_hash, metadata) in &self.state.data {
            // metadata passed to torrent is behind `Arc` and `Mutex`
            let torrent = Torrent::new(info_hash.clone(), metadata.clone());
            self.torrents.insert(info_hash.clone(), torrent);
        }
        for (_, torrent) in &mut self.torrents {
            tokio::spawn(async { torrent.run().await });
        }
        Ok(())
    }
}
