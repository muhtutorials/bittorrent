use crate::dot_torrent::hashes::Hashes;
use crate::dot_torrent::{DotTorrent, Info, Key};
use anyhow::Context;
use sha1::{Digest, Sha1};
use std::path::PathBuf;
use tokio::fs::{File, write};
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

const PIECE_LENGTH: usize = 32768;

pub async fn create_torrent(path_str: &str) -> anyhow::Result<String> {
    let path = PathBuf::from(path_str);
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .context("couldn't get the final component of the Path")?;
    let mut dot_torrent = DotTorrent {
        // URL for tests with a "real" tracker
        // http://bittorrent-test-tracker.codecrafters.io/announce
        announce: "http://127.0.0.1:8000/announce".to_string(),
        info: Info {
            name,
            piece_length: PIECE_LENGTH,
            pieces: Hashes(Vec::new()),
            key: Key::SingleFile { length: 0 },
        },
    };
    if path.is_file() {
        let mut file = File::open(path).await.context("failed to open the file")?;
        let file_length = file.seek(SeekFrom::End(0)).await? as usize;
        dot_torrent.info.key = Key::SingleFile {
            length: file_length,
        };
        let n_pieces = (file_length + PIECE_LENGTH - 1) / PIECE_LENGTH;
        let mut buf = [0u8; PIECE_LENGTH];
        for piece_i in 0..n_pieces {
            let piece_size = if piece_i == n_pieces - 1 {
                // calculate last piece's size
                let modulo = file_length % PIECE_LENGTH;
                if modulo == 0 { PIECE_LENGTH } else { modulo }
            } else {
                PIECE_LENGTH
            };
            let piece = &mut buf[..piece_size];
            let offset = (piece_i * PIECE_LENGTH) as u64;
            file.seek(SeekFrom::Start(offset)).await?;
            file.read_exact(piece).await?;
            let mut hasher = Sha1::new();
            hasher.update(piece);
            let hash: [u8; 20] = hasher.finalize().into();
            dot_torrent.info.pieces.0.push(hash);
        }
        let bencoded_dot_torrent =
            serde_bencode::to_bytes(&dot_torrent).context("invalid data during encoding")?;
        let mut path = PathBuf::from("./");
        path.push(&dot_torrent.info.name);
        path.set_extension("torrent");
        write(path, &bencoded_dot_torrent)
            .await
            .context("failed to write `.torrent` file")?;
    }
    Ok(format!("created torrent from path: {path_str}"))
}
