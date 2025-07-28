use crate::bitfield::Bitfield;
use crate::peer::Peer;
use crate::state::SharedMetadata;
use crate::tracker::PeerList;
use crate::tracker::query_tracker;
use futures_util::{StreamExt, stream};

pub struct Torrent {
    pub info_hash: [u8; 20],
    pub metadata: SharedMetadata,
    pub pieces: Bitfield,
    pub interval: usize,
    pub peer_list: PeerList,
    pub uploaders: Vec<Peer>,
    pub downloaders: Vec<Peer>,
}

impl Torrent {
    pub fn new(info_hash: [u8; 20], metadata: SharedMetadata) -> Self {
        let pieces = metadata.lock().unwrap().pieces.clone();
        Self {
            info_hash,
            metadata,
            pieces,
            interval: 0,
            peer_list: PeerList(Vec::new()),
            uploaders: Vec::new(),
            downloaders: Vec::new(),
        }
    }

    pub async fn run(&mut self) {
        self.get_uploaders().await.unwrap();
        let info_hash = self.info_hash.clone();
        let mut stream = stream::iter(self.peer_list.0.iter())
            .map(|peer_addr| async move {
                let peer = Peer::new(*peer_addr, info_hash).await;
                (peer_addr, peer)
            })
            .buffer_unordered(5);
        let mut peers = Vec::new();
        while let Some((peer_addr, peer)) = stream.next().await {
            match peer {
                Ok(peer) => {
                    peers.push(peer);
                    if peers.len() >= 5 {
                        break;
                    }
                }
                Err(err) => println!("failed to connect to peer {peer_addr}: {err}"),
            }
        }
        drop(stream);
    }

    async fn get_uploaders(&mut self) -> anyhow::Result<()> {
        let metadata = &self.metadata.lock().unwrap();
        let resp = query_tracker(&metadata.dot_torrent).await?;
        self.interval = resp.interval;
        self.peer_list = resp.peers;
        Ok(())
    }
}
