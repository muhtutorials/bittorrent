use crate::BLOCK_SIZE;
use crate::State;
use crate::dot_torrent::{DotTorrent, File, Key};
use crate::peer::{MessageType, Peer, PieceResponse};
use crate::piece::Piece;
use crate::torrent::Torrent;
use crate::tracker::query_tracker;
use anyhow::{Context, bail};
use futures_util::StreamExt;
use futures_util::stream;
use futures_util::stream::futures_unordered::FuturesUnordered;
use kanal::bounded_async;
use sha1::{Digest, Sha1};
use std::collections::BinaryHeap;
use std::path::PathBuf;
use tokio::sync::mpsc::channel;

// downloads a torrent from a `.torrent` file
pub(crate) async fn download_torrent(path: &str, state: State) -> anyhow::Result<String> {
    let dot_torrent = DotTorrent::read(path).await?;
    let info_hash = dot_torrent.info_hash()?;
    let mut state = state.get().await;
    if state.torrents.get(&info_hash).is_some() {
        bail!("torrent already exists");
    }
    // TODO: it should be handled by query routine.
    let tracker_resp = query_tracker(&dot_torrent)
        .await
        .context("query tracker for peer info")?;
    let path = PathBuf::from(format!("./{}", dot_torrent.info.name));
    let torrent = Torrent::new(state.generate_id(), path, dot_torrent.clone())?;

    Ok(String::new())
}
