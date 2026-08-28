use crate::bit_vec::BitVec;
use crate::dot_torrent::{self, DotTorrent};
use crate::peer::Peer;
use crate::piece::Piece;
use crate::tracker::PeerAddrs;
use futures_util::{StreamExt, stream};
use serde::Deserialize;
use std::collections::{BinaryHeap, HashMap};
use std::net::SocketAddrV4;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::sleep;

pub(crate) struct Torrent {
    pub(crate) metadata: Metadata,
    // addresses of available peers sent by tracker
    pub(crate) peer_addrs: PeerAddrs,
    pub(crate) peers: HashMap<SocketAddrV4, Peer>,
}

impl Torrent {
    pub(crate) fn new(id: usize, path: PathBuf, dot_torrent: DotTorrent) -> anyhow::Result<Self> {
        let metadata = Metadata::new(id, path, dot_torrent)?;
        Ok(Self::from_metadata(metadata))
    }

    pub(crate) fn from_metadata(metadata: Metadata) -> Self {
        Self {
            metadata,
            peer_addrs: PeerAddrs(Vec::new()),
            peers: HashMap::new(),
        }
    }

    // pub async fn run(&mut self) {
    //     tokio::spawn(heartbeat(
    //         self.metadata.clone(),
    //         self.peer_addrs.clone(),
    //         self.notify.clone(),
    //     ));
    //     let info_hash = self.info_hash.clone();
    //     loop {
    //         self.notify.notified().await;
    //         let peer_addrs = self.peer_addrs.lock().await;
    //         let mut stream = stream::iter(peer_addrs.0.iter())
    //             .map(|peer_addr| async move {
    //                 let peer = Peer::new(*peer_addr, info_hash).await;
    //                 (peer_addr, peer)
    //             })
    //             .buffer_unordered(self.max_peers.available_permits());
    //         while let Some((peer_addr, peer)) = stream.next().await {
    //             match peer {
    //                 Ok(peer) => {
    //                     let mut peers = self.peers.lock().await;
    //                     peers.push(peer);
    //                 }
    //                 Err(err) => println!("failed to connect to peer {peer_addr}: {err}"),
    //             }
    //         }
    //         drop(stream);
    //         let mut available_pieces = BinaryHeap::new();
    //         let mut unavailable_pieces = Vec::new();
    //         let metadata = self.metadata.lock().await;
    //         let peers = self.peers.lock().await;
    //         for piece_i in metadata.pieces.zeros() {
    //             let piece = Piece::new(piece_i, &metadata.dot_torrent, peers.as_slice());
    //             if piece.peers().is_empty() {
    //                 unavailable_pieces.push(piece);
    //             } else {
    //                 // TODO: handle unavailable pieces
    //                 available_pieces.push(piece);
    //             }
    //         }
    //     }
    // }
}

// Torrent's metadata.
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

// sends regular requests to the tracker at an interval specified by it
// async fn heartbeat(metadata: SharedMetadata, peer_addrs: SharedPeerAddrs, notify: Arc<Notify>) {
//     let mut interval = 0;
//     loop {
//         sleep(Duration::from_secs(interval)).await;
//         let mut backoff = 1;
//         loop {
//             let metadata = metadata.lock().await;
//             let resp = query_tracker(&metadata.dot_torrent).await;
//             drop(metadata);
//             if let Ok(resp) = resp {
//                 interval = resp.interval;
//                 let mut peer_addrs = peer_addrs.lock().await;
//                 *peer_addrs = resp.peers;
//                 notify.notify_one();
//                 break;
//             }
//             sleep(Duration::from_secs(backoff)).await;
//             backoff *= 2;
//         }
//     }
// }

// pub type SharedMetadata = Arc<Mutex<Metadata>>;

// pub type SharedPeerAddrs = Arc<Mutex<PeerAddrs>>;

// pub type SharedPeers = Arc<Mutex<HashMap<SocketAddrV4, Peer>>>;
