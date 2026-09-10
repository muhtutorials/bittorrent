use crate::BitVec;
use crate::DotTorrent;
use anyhow::bail;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::futures::Notified;
use tokio::sync::{Mutex, MutexGuard, Notify};
use tokio::time::Instant;

#[derive(Clone)]
pub(crate) struct State {
    shared: Arc<Shared>,
}

pub(crate) struct Shared {
    inner: Mutex<Inner>,
    notify: Notify,
}

pub(crate) struct Inner {
    // ID of the last added torrent
    id: usize,
    pub(crate) torrents: HashMap<[u8; 20], Torrent>,
    // Intervals that the client should wait between
    // sending regular requests to the tracker(s).
    pub(crate) intervals: BTreeSet<(Instant, [u8; 20])>,
    pub(crate) shutdown: bool,
}

impl State {
    pub(crate) fn new(id: usize, torrents: HashMap<[u8; 20], Torrent>) -> Self {
        let inner = Inner {
            id,
            torrents,
            intervals: BTreeSet::new(),
            shutdown: false,
        };
        let shared = Shared {
            inner: Mutex::new(inner),
            notify: Notify::new(),
        };
        Self {
            shared: Arc::new(shared),
        }
    }

    pub(crate) async fn get(&self) -> MutexGuard<'_, Inner> {
        self.shared.inner.lock().await
    }

    pub(crate) async fn is_shutdown(&self) -> bool {
        self.shared.inner.lock().await.shutdown
    }

    pub(crate) fn notified(&self) -> Notified<'_> {
        self.shared.notify.notified()
    }

    pub(crate) async fn add_torrent(&self, dot_torrent: DotTorrent) -> anyhow::Result<()> {
        let info_hash = dot_torrent.info_hash()?;
        let mut state = self.get().await;
        if state.torrents.get(&info_hash).is_some() {
            bail!("torrent already exists");
        }
        let path = PathBuf::from(format!("./{}", dot_torrent.info.name));
        let torrent = Torrent::new(state.generate_id(), path, dot_torrent)?;
        state.torrents.insert(info_hash.clone(), torrent);
        state.intervals.insert((Instant::now(), info_hash));
        self.shared.notify.notify_one();
        Ok(())
    }
}

impl Inner {
    pub(crate) fn generate_id(&mut self) -> usize {
        self.id += 1;
        self.id
    }
}

#[derive(Deserialize, Clone)]
pub(crate) struct Torrent {
    pub(crate) id: usize,
    pub(crate) path: PathBuf,
    pub(crate) dot_torrent: DotTorrent,
    pub(crate) pieces: BitVec,
    pub(crate) uploaded: usize,
    pub(crate) downloaded: usize,
    pub(crate) left: usize,
    pub(crate) finished: bool,
    pub(crate) file_exists: bool,
}

impl Torrent {
    pub(crate) fn new(id: usize, path: PathBuf, dot_torrent: DotTorrent) -> anyhow::Result<Self> {
        let info_hash = dot_torrent.info_hash()?;
        let n_pieces = dot_torrent.info.pieces.0.len();
        let pieces = BitVec::new(n_pieces);
        let left = dot_torrent.length();
        Ok(Self {
            id,
            path,
            dot_torrent,
            pieces,
            uploaded: 0,
            downloaded: 0,
            left,
            finished: false,
            // TODO: change it to true when file is created.
            file_exists: false,
        })
    }
}
