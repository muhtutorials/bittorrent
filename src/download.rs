use crate::BLOCK_MAX;
use crate::peer::{MessageType, Peer, PieceResponse};
use crate::piece::Piece;
use crate::dot_torrent::{File, Key, DotTorrent};
use crate::tracker::query_tracker;
use anyhow::Context;
use futures_util::StreamExt;
use futures_util::stream;
use futures_util::stream::futures_unordered::FuturesUnordered;
use kanal::bounded_async;
use sha1::{Digest, Sha1};
use std::collections::BinaryHeap;
use tokio::sync::mpsc::channel;

pub(crate) async fn all(dot_torrent: &DotTorrent) -> anyhow::Result<Downloaded> {
    let tracker_resp = query_tracker(dot_torrent)
        .await
        .context("query tracker for peer info")?;
    let info_hash = dot_torrent.info_hash()?;
    let mut stream = stream::iter(tracker_resp.peers.0.iter())
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

    // TODO: since it's stored in memory, should be implemented differently
    // write every piece to disk so we can resume downloads and seed later on
    let mut pieces_to_download = BinaryHeap::new();
    // pieces which peers don't have
    let mut unavailable_pieces = Vec::new();
    for piece_i in 0..dot_torrent.info.pieces.0.len() {
        let piece = Piece::new(piece_i, dot_torrent, &peers);
        if piece.peers().is_empty() {
            unavailable_pieces.push(piece);
        } else {
            pieces_to_download.push(piece);
        }
    }
    // TODO: handle unavailable pieces
    assert!(unavailable_pieces.is_empty());

    let mut downloaded_pieces = vec![0; dot_torrent.length()];
    while let Some(piece) = pieces_to_download.pop() {
        let peers: Vec<_> = peers
            .iter_mut()
            .enumerate()
            .filter_map(|(peer_i, peer)| piece.peers().contains(&peer_i).then_some(peer))
            .collect();

        let piece_size = piece.length();
        // "+ BLOCK_MAX - 1" rounds up the number
        let n_blocks = (piece_size + BLOCK_MAX - 1) / BLOCK_MAX;
        let (job_tx, job_rx) = bounded_async(n_blocks);
        for block_i in 0..n_blocks {
            job_tx
                .send(block_i)
                .await
                .expect("all peers already exited");
        }

        let (done_tx, mut done_rx) = channel(n_blocks);
        let mut participants = FuturesUnordered::new();
        for peer in peers {
            participants.push(peer.participate(
                piece.index(),
                piece_size,
                n_blocks,
                job_tx.clone(),
                job_rx.clone(),
                done_tx.clone(),
            ));
        }
        // drop our copies of handles
        drop(job_tx);
        drop(done_tx);
        drop(job_rx);

        let mut downloaded_blocks = vec![0u8; piece_size];
        let mut bytes_received = 0;
        loop {
            tokio::select! {
                joined = participants.next(), if !participants.is_empty() => {
                    // if a participant ends early, it's either slow or failed
                    // match joined {
                    //     None => {
                    //         // There are no peers.
                    //         // This must mean we are about to get `None` from `done_rx.recv()`,
                    //         // so we'll handle it there.
                    //     }
                    //     Some(Ok(_)) => {
                    //         // The peer gave up because it timed out.
                    //         // Nothing to do, except maybe to de-prioritize this peer
                    //         // for later.
                    //     }
                    //     Some(Err(_)) => {
                    //         // Peer failed and should be removed later.
                    //         // It already isn't participating in this piece.
                    //         // We should remove it from global peer list.
                    //     }
                    // }
                }
                msg = done_rx.recv() => {
                    if let Some(msg) = msg {
                        assert_eq!(msg.typ, MessageType::Piece);
                        assert!(!msg.payload.is_empty());
                        // keep track of the bytes in message
                        let piece_response = PieceResponse::ref_from_bytes(&msg.payload)
                            .expect("always get all `PieceResponse` fields from peer");
                        downloaded_blocks[piece_response.begin() as usize..][..piece_response.block().len()]
                            .copy_from_slice(piece_response.block());
                        bytes_received += piece_response.block().len();
                        if bytes_received == piece_size {
                            // we got all the bytes
                            // This must mean that all participants have either exited or
                            // are waiting for more work. In either case, it's OK to drop
                            // all the participant futures.
                            break;
                        }
                    } else {
                        // there are no peer left so we can't progress
                        assert_eq!(bytes_received, piece_size);
                        break;
                    }
                }
            }
        }
        drop(participants);

        if bytes_received == piece_size {
            // we got all the bytes
        } else {
            // We'll need to connect to more peers, and make sure that those additional peers also
            // have this piece, and then download the pieces we didn't get from them.
            // Probably also stick this back onto the pieces_heap.
            anyhow::bail!("no peers left to get piece {}", piece.index());
        }

        assert_eq!(downloaded_blocks.len(), piece_size);
        let mut hasher = Sha1::new();
        hasher.update(&downloaded_blocks);
        let hash: [u8; 20] = hasher.finalize().into();
        assert_eq!(hash, piece.hash());

        downloaded_pieces[piece.index() * dot_torrent.info.piece_length..][..piece_size]
            .copy_from_slice(&downloaded_blocks)
    }

    let files = match &dot_torrent.info.key {
        Key::SingleFile { length } => vec![File {
            length: *length,
            path: vec![dot_torrent.info.name.clone()],
        }],
        Key::MultipleFiles { files } => files.clone(),
    };

    Ok(Downloaded {
        bytes: downloaded_pieces,
        files,
    })
}

pub struct Downloaded {
    files: Vec<File>,
    bytes: Vec<u8>,
}

impl<'d> IntoIterator for &'d Downloaded {
    type Item = DownloadedFile<'d>;
    type IntoIter = DownloadedIter<'d>;

    fn into_iter(self) -> Self::IntoIter {
        DownloadedIter::new(self)
    }
}

pub struct DownloadedIter<'d> {
    downloaded: &'d Downloaded,
    files_iter: std::slice::Iter<'d, File>,
    offset: usize,
}

impl<'d> DownloadedIter<'d> {
    fn new(downloaded: &'d Downloaded) -> Self {
        Self {
            downloaded,
            files_iter: downloaded.files.iter(),
            offset: 0,
        }
    }
}

impl<'d> Iterator for DownloadedIter<'d> {
    type Item = DownloadedFile<'d>;

    fn next(&mut self) -> Option<Self::Item> {
        let file = self.files_iter.next()?;
        // slicing twice here
        let bytes = &self.downloaded.bytes[self.offset..self.offset + file.length];
        Some(DownloadedFile { file, bytes })
    }
}

pub struct DownloadedFile<'d> {
    file: &'d File,
    bytes: &'d [u8],
}

impl<'d> DownloadedFile<'d> {
    pub fn path(&self) -> &'d [String] {
        &self.file.path
    }

    pub fn bytes(&self) -> &'d [u8] {
        self.bytes
    }
}
