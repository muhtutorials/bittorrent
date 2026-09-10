use crate::BitVec;
use crate::peer::Peer;
use crate::tracker::PeerAddrs;
use std::collections::VecDeque;
use std::net::SocketAddrV4;
use std::sync::Arc;
use tokio::spawn;
use tokio::sync::mpsc::Receiver;
use tokio::sync::{Mutex, Notify, Semaphore};

pub(crate) struct Downloader {
    info_hash: [u8; 20],
    // pieces to download
    pieces: Arc<Mutex<BitVec>>,
    addrs_rx: Receiver<PeerAddrs>,
    addrs: Mutex<VecDeque<SocketAddrV4>>,
    // available peer addresses
    addrs_in_use: Vec<SocketAddrV4>,
    // notifies that new peer addresses have been received
    notify: Notify,
    sem: Arc<Semaphore>,
}

impl Downloader {
    pub(crate) fn new(info_hash: [u8; 20], pieces: BitVec, addrs_rx: Receiver<PeerAddrs>) {
        let dl = Downloader {
            info_hash,
            pieces: Arc::new(Mutex::new(pieces)),
            addrs_rx,
            addrs: Mutex::new(VecDeque::new()),
            addrs_in_use: Vec::new(),
            notify: Notify::new(),
            sem: Arc::new(Semaphore::new(5)),
        };
    }

    pub(crate) async fn run(&self) {
        let pieces = self.pieces.lock().await;
        while Some(n) = pieces.next() {}
    }

    async fn connect_to_peers(&mut self) {
        loop {
            let has_addresses = {
                let addrs = self.addrs.lock().await;
                !addrs.is_empty()
            };
            if !has_addresses {
                // wait for an updated address list
                self.notify.notified().await;
                continue;
            }
            let permit = match self.sem.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(_) => {
                    // wait for a permit to become available
                    let _ = self.sem.clone().acquire_owned().await.unwrap();
                    continue;
                }
            };
            let addr = {
                let mut addrs = self.addrs.lock().await;
                addrs
                    .pop_front()
                    .expect("address should always be Some, because it's checked before")
            };
            spawn(async move {
                let peer = Peer::new(addr, self.info_hash).await?;
                peer.participate(piece_i, piece_size, n_blocks, job_tx, job_rx, done_tx)
            })
        }
    }
}
