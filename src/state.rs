use crate::torrent::Torrent;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use tokio::sync::Notify;
use tokio::time::Instant;

pub(crate) struct State {
    pub(crate) inner: Mutex<Inner>,
    pub(crate) notify: Notify,
}

struct Inner {
    pub(crate) torrents: HashMap<[u8; 20], Torrent>,
    // Intervals that the client should wait between
    // sending regular requests to the tracker(s).
    pub(crate) intervals: BTreeSet<(Instant, [u8; 20])>,
    pub(crate) shutdown: bool,
}

impl State {
    pub(crate) fn new(torrents: HashMap<[u8; 20], Torrent>) -> Self {
        let inner = Inner {
            torrents,
            intervals: BTreeSet::new(),
            shutdown: false,
        };
        Self {
            inner: Mutex::new(inner),
            notify: Notify::new(),
        }
    }

    pub(crate) fn get_state(&self) -> MutexGuard<Inner> {
        self.inner.lock().unwrap()
    }

    pub(crate) fn is_shutdown(&self) -> bool {
        self.inner.lock().unwrap().shutdown
    }
}
