use crate::peer::Peer;
use crate::piece::Piece;
use crate::state::SharedMetadata;
use crate::tracker::{PeerAddrs, query_tracker};
use futures_util::{StreamExt, stream};
use std::collections::BinaryHeap;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, Notify, Semaphore, mpsc};
use tokio::time::sleep;

pub struct TorrentManager {
    pub info_hash: [u8; 20],
    pub stream_tx: mpsc::Sender<TcpStream>,
}

impl TorrentManager {
    pub fn new(info_hash: [u8; 20], stream_tx: mpsc::Sender<TcpStream>) -> Self {
        Self {
            info_hash,
            stream_tx,
        }
    }

    // pub fn run() {
    //     tokio::spawn()
    // }
}

pub struct Torrent {
    pub info_hash: [u8; 20],
    pub metadata: SharedMetadata,
    // addresses of available peers sent by tracker
    pub peer_addrs: SharedPeerAddrs,
    pub peers: SharedPeers,
    pub max_peers: Arc<Semaphore>,
    // notifies after fetching peer addresses
    notify: Arc<Notify>,
}

impl Torrent {
    pub fn new(info_hash: [u8; 20], metadata: SharedMetadata) -> Self {
        Self {
            info_hash,
            metadata,
            peer_addrs: Arc::new(Mutex::new(PeerAddrs(Vec::new()))),
            peers: Arc::new(Mutex::new(Vec::new())),
            max_peers: Arc::new(Semaphore::new(5)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub async fn run(&mut self) {
        tokio::spawn(heartbeat(
            self.metadata.clone(),
            self.peer_addrs.clone(),
            self.notify.clone(),
        ));
        let info_hash = self.info_hash.clone();
        loop {
            self.notify.notified().await;
            let peer_addrs = self.peer_addrs.lock().await;
            let mut stream = stream::iter(peer_addrs.0.iter())
                .map(|peer_addr| async move {
                    let peer = Peer::new(*peer_addr, info_hash).await;
                    (peer_addr, peer)
                })
                .buffer_unordered(self.max_peers.available_permits());
            while let Some((peer_addr, peer)) = stream.next().await {
                match peer {
                    Ok(peer) => {
                        let mut peers = self.peers.lock().await;
                        peers.push(peer);
                    }
                    Err(err) => println!("failed to connect to peer {peer_addr}: {err}"),
                }
            }
            drop(stream);

            let mut available_pieces = BinaryHeap::new();
            let mut unavailable_pieces = Vec::new();
            let metadata = self.metadata.lock().await;
            let peers = self.peers.lock().await;
            for piece_i in metadata.pieces.zeros() {
                let piece = Piece::new(piece_i, &metadata.dot_torrent, peers.as_slice());
                if piece.peers().is_empty() {
                    unavailable_pieces.push(piece);
                } else {
                    // TODO: handle unavailable pieces
                    available_pieces.push(piece);
                }
            }
        }
    }
}

pub type SharedPeerAddrs = Arc<Mutex<PeerAddrs>>;

pub type SharedPeers = Arc<Mutex<Vec<Peer>>>;

async fn connect_to_peers(addrs: SharedPeerAddrs) {}

// sends regular requests to the tracker at an interval specified by it
async fn heartbeat(metadata: SharedMetadata, peer_addrs: SharedPeerAddrs, notify: Arc<Notify>) {
    let mut interval = 0;
    loop {
        sleep(Duration::from_secs(interval)).await;
        let mut backoff = 1;
        loop {
            let metadata = metadata.lock().await;
            let resp = query_tracker(&metadata.dot_torrent).await;
            drop(metadata);
            if let Ok(resp) = resp {
                interval = resp.interval;
                let mut peer_addrs = peer_addrs.lock().await;
                *peer_addrs = resp.peers;
                notify.notify_one();
                break;
            }
            sleep(Duration::from_secs(backoff)).await;
            backoff *= 2;
        }
    }
}
