use crate::BitVec;
use crate::DotTorrent;
use crate::peer::{Peer, PeerMessage};
use crate::scheduler::{Scheduler, SchedulerMessage};
use crate::tracker::PeerAddrs;
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddrV4;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::spawn;
use tokio::sync::Semaphore;
use tokio::sync::mpsc::{Receiver, Sender, channel};

const MAX_CONNS: usize = 5;

pub(crate) struct Torrent {
    meta: Metadata,
    tx: Sender<TorrentMessage>,
    rx: Receiver<TorrentMessage>,
    routes: Routes,
}

impl Torrent {
    pub(crate) fn new(
        meta: Metadata,
        tx: Sender<TorrentMessage>,
        rx: Receiver<TorrentMessage>,
    ) -> Self {
        let (peer_manager_tx, peer_manager_rx) = channel(1024);
        let mut pm = PeerManager::new(meta.info_hash, MAX_CONNS, peer_manager_rx, tx.clone());
        tokio::spawn(async move {
            pm.run().await;
        });
        let (scheduler_tx, scheduler_rx) = channel(1024);
        let routes = Routes {
            peer_manager: peer_manager_tx,
            scheduler: scheduler_tx,
        };
        Self {
            meta,
            tx,
            rx,
            routes,
        }
    }

    async fn run(&mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                // TODO: maybe eliminate this branch and send directly
                TorrentMessage::PeerAddrs(addrs) => {
                    self.routes.peer_manager.send(addrs);
                }
                TorrentMessage::AddPeer(msg) => {
                    self.routes.piece_manager.send(msg);
                }
                TorrentMessage::RemovePeer(addr) => {
                    self.peers.remove(&addr);
                }
            }
        }
    }
}

#[derive(Deserialize, Clone)]
pub(crate) struct Metadata {
    pub(crate) id: usize,
    pub(crate) path: PathBuf,
    pub(crate) dot_torrent: DotTorrent,
    pub(crate) info_hash: [u8; 20],
    pub(crate) pieces: BitVec,
    pub(crate) uploaded: usize,
    pub(crate) downloaded: usize,
    pub(crate) left: usize,
    pub(crate) finished: bool,
    pub(crate) file_exists: bool,
}

impl Metadata {
    pub(crate) fn new(id: usize, path: PathBuf, dot_torrent: DotTorrent) -> anyhow::Result<Self> {
        let info_hash = dot_torrent.info_hash()?;
        let n_pieces = dot_torrent.info.pieces.0.len();
        let pieces = BitVec::new(n_pieces);
        let left = dot_torrent.length();
        Ok(Self {
            id,
            path,
            dot_torrent,
            info_hash,
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

pub enum TorrentMessage {
    PeerAddrs(PeerAddrs),
    AddPeer((SocketAddrV4, BitVec, Sender<PeerMessage>)),
    RemovePeer(SocketAddrV4),
}

struct PeerManager {
    info_hash: [u8; 20],
    addrs: VecDeque<SocketAddrV4>,
    in_use: Vec<SocketAddrV4>,
    sem: Arc<Semaphore>,
    rx: Receiver<PeerAddrs>,
    tor_tx: Sender<TorrentMessage>,
}

impl PeerManager {
    fn new(
        info_hash: [u8; 20],
        permits: usize,
        rx: Receiver<PeerAddrs>,
        tor_tx: Sender<TorrentMessage>,
    ) -> Self {
        Self {
            info_hash,
            addrs: VecDeque::new(),
            in_use: Vec::new(),
            sem: Arc::new(Semaphore::new(permits)),
            rx,
            tor_tx,
        }
    }

    async fn run(&mut self) {
        loop {
            tokio::select! {
                Some(addrs) = self.rx.recv() => {
                    self.addrs = VecDeque::from(addrs.0);
                }
                Ok(permit) = self.sem.clone().acquire_owned(), if !self.addrs.is_empty() => {
                    let addr = self.addrs.pop_front().unwrap();
                    if self.in_use.contains(&addr) {
                        continue; // permit dropped, loop picks next
                    }
                    tokio::spawn(async move {
                        let _permit = permit;
                        if let Ok(peer) = Peer::from_addr(addr, self.info_hash, self.tor_tx.clone()).await {
                            peer.run().await;
                        };
                    });
                }
                else => break,
            }
        }
    }
}

struct Routes {
    peer_manager: Sender<PeerAddrs>,
    scheduler: Sender<SchedulerMessage>,
}
