use crate::bit_vec::BitVec;
use crate::dot_torrent::DotTorrent;
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

pub struct Torrent {
    pub metadata: Metadata,
    // addresses of available peers sent by tracker
    pub peer_addrs: PeerAddrs,
    pub peers: HashMap<SocketAddrV4, Peer>,
}

impl Torrent {
    pub fn new(metadata: Metadata) -> Self {
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
pub struct Metadata {
    pub id: usize,
    pub path: PathBuf,
    pub dot_torrent: DotTorrent,
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub port: u16,
    pub pieces: BitVec,
    pub uploaded: usize,
    pub downloaded: usize,
    pub left: usize,
    pub finished: bool,
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
