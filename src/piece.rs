use crate::dot_torrent::DotTorrent;
use crate::peer::Peer;
use std::cmp::Ordering;
use std::collections::HashSet;

#[derive(Debug, Eq, PartialEq)]
pub struct Piece {
    index: usize,
    length: usize,
    hash: [u8; 20],
    peers: HashSet<usize>,
}

impl Ord for Piece {
    fn cmp(&self, other: &Self) -> Ordering {
        self.peers
            .len()
            .cmp(&other.peers.len())
            // tie-break by random ordering of HashSet to avoid deterministic contention
            .then(self.peers.iter().cmp(other.peers.iter()))
    }
}

impl PartialOrd for Piece {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Piece {
    pub(crate) fn new(index: usize, dot_torrent: &DotTorrent, peers: &[Peer]) -> Self {
        let length = if index == dot_torrent.info.pieces.0.len() - 1 {
            // calculates last piece's size
            dot_torrent.length() % dot_torrent.info.piece_length
        } else {
            dot_torrent.info.piece_length
        };
        let hash = dot_torrent.info.pieces.0[index];
        let peers = peers
            .iter()
            .enumerate()
            .filter_map(|(peer_i, peer)| peer.has_piece(index).then_some(peer_i))
            .collect();
        Self {
            index,
            length,
            hash,
            peers,
        }
    }

    pub(crate) fn index(&self) -> usize {
        self.index
    }

    pub(crate) fn length(&self) -> usize {
        self.length
    }

    pub(crate) fn hash(&self) -> [u8; 20] {
        self.hash
    }

    pub(crate) fn peers(&self) -> &HashSet<usize> {
        &self.peers
    }
}
