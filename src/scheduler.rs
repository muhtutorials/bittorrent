use crate::BitVec;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddrV4;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{Duration, MissedTickBehavior, interval};
use tokio::{select, spawn};

pub(crate) struct Scheduler {
    pieces: BitVec,
    peers: Peers,
    piece_qu: PieceQu,
}

impl Scheduler {
    pub(crate) fn new(pieces: BitVec) -> Self {
        let peers = Arc::new(Mutex::new(HashMap::new()));
        Self {
            pieces,
            peers,
            piece_qu: PieceQu::new(),
        }
    }

    pub(crate) async fn run(mut self, mut rx: Receiver<SchedulerMessage>) {
        let mut tick = interval(Duration::from_millis(10));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(m) => self.handle_message(m),
                    None => break, // all senders dropped
                },
                _ = tick.tick() => self.request_pieces(),
            }
        }
    }

    fn handle_message(&mut self, msg: SchedulerMessage) {}

    fn request_pieces(&mut self) {
        self.piece_qu.pop_piece()
    }
}

// async fn scheduler_task(mut s: Scheduler, rx: Receiver<SchedulerMessage>) {
//     let mut ticker = tokio::time::interval(Duration::from_millis(100));
//     loop {
//         tokio::select! {
//             msg = rx.recv() => match msg {
//                 Some(SchedulerMessage::Have(addr, idx)) => {
//                     s.piece_qu.add_availability(idx, addr); // update rarity in place
//                 }
//                 Some(SchedulerMessage::Done(idx)) => {
//                     s.piece_qu.remove(idx);                  // no longer needed
//                 }
//                 Some(SchedulerMessage::Peer((addr, bv, tx))) => {
//                     s.on_peer(addr, bv, tx);
//                 }
//                 None => break,
//             },
//             _ = ticker.tick() => {
//                 // pop from the rarity queue and hand out requests
//                 while let Some((idx, peer)) = s.piece_qu.pop_rarest_available() {
//                     if let Some(tx) = s.peers.lock().unwrap().get(&peer) {
//                         let _ = tx.send(idx); // non-blocking send to peer thread
//                     }
//                 }
//             }
//         }
//     }
// }

pub(crate) enum SchedulerMessage {
    Peer(SocketAddrV4, BitVec, Sender<usize>),
    Have(SocketAddrV4, usize),
}

type Peers = Arc<Mutex<HashMap<SocketAddrV4, (BitVec, Sender<usize>)>>>;

struct PieceQu {
    buckets: Vec<HashSet<usize>>,
    // bucket index of piece inside `buckets`
    bucket_of: HashMap<usize, usize>,
    min_bucket: usize,
}

impl PieceQu {
    fn new() -> Self {
        Self {
            // NOTE: First element is always an empty set which signifies zero
            // count of a piece. Only used for correct offset for the rest of
            // piece counts. Second element avoids an if statement when the first
            // instance of a piece is added.
            buckets: vec![HashSet::new(); 2],
            bucket_of: HashMap::new(),
            min_bucket: 0,
        }
    }

    fn add(&mut self, piece_i: usize) {
        match self.bucket_of.entry(piece_i) {
            Entry::Occupied(mut entry) => {
                let bucket_i = *entry.get();
                let new_bucket_i = bucket_i + 1;
                // insert updated `bucket_i` into `bucket_of`
                entry.insert(new_bucket_i);
                self.buckets[bucket_i].remove(&piece_i);
                if self.buckets.len() <= new_bucket_i {
                    self.buckets.push(HashSet::new());
                }
                self.buckets[new_bucket_i].insert(piece_i);
                if self.min_bucket == bucket_i && self.buckets[bucket_i].len() == 0 {
                    self.min_bucket = bucket_i + 1;
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(1);
                self.buckets[1].insert(piece_i);
                self.min_bucket = 1;
            }
        }
    }

    fn remove(&mut self, piece_i: usize) {
        match self.bucket_of.entry(piece_i) {
            Entry::Occupied(mut entry) => {
                let bucket_i = *entry.get();
                if bucket_i > 1 {
                    let new_bucket_i = bucket_i - 1;
                    entry.insert(new_bucket_i);
                    self.buckets[new_bucket_i].insert(piece_i);
                } else {
                    entry.remove();
                }
                self.buckets[bucket_i].remove(&piece_i);
            }
            Entry::Vacant(_) => {
                // TODO: what is the correct way to handle this case?
                println!("PieceQu::remove: `piece_i` is not in the `bucket_of`");
            }
        }
    }

    fn pop(&mut self) -> Option<usize> {
        // Lazily advance min_bucket past empty buckets.
        while self.min_bucket < self.buckets.len() && self.buckets[self.min_bucket].is_empty() {
            self.min_bucket += 1;
        }
        if self.min_bucket >= self.buckets.len() {
            return None; // nothing left
        }
        // Pop any element from the set.
        let &piece_i = self.buckets[self.min_bucket].iter().next()?;
        self.bucket_of.remove(&piece_i);
        self.buckets[self.min_bucket].remove(&piece_i);
        Some(piece_i)
    }
}

// for each peer:
//     while peer.pipeline not full:
//     block = scheduler.pick_next_block(peer)
//     peer.request(block)          # may belong to ANY in-progress piece
//
// on block received from peer:
//     mark block done
//     if piece fully received: verify hash, mark complete/failed
//     scheduler.notify(peer)           # peer will request another block
