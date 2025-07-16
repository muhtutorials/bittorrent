use crate::dot_torrent::hashes::Hashes;
use crate::dot_torrent::{Info, Key, DotTorrent};
use anyhow::Context;
use memmap2::Mmap;
use sha1::{Digest, Sha1};
use std::fs::File;
use std::path::PathBuf;

const PIECE_LENGTH: usize = 32768;

pub async fn create_torrent(path: PathBuf) -> anyhow::Result<()> {
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
        let file = File::open(path).context("failed to open the file")?;
        let mmap = unsafe { Mmap::map(&file).context("failed to map the file")? };
        let file_length = mmap.len();
        dot_torrent.info.key = Key::SingleFile {
            length: file_length,
        };
        let n_pieces = (file_length + PIECE_LENGTH - 1) / PIECE_LENGTH;
        for piece_i in 0..n_pieces {
            let piece_size = if piece_i == n_pieces - 1 {
                // calculate last piece's size
                let modulo = file_length % PIECE_LENGTH;
                if modulo == 0 { PIECE_LENGTH } else { modulo }
            } else {
                PIECE_LENGTH
            };
            let piece = &mmap[piece_i * PIECE_LENGTH..piece_i * PIECE_LENGTH + piece_size];
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
        tokio::fs::write(path, &bencoded_dot_torrent)
            .await
            .context("failed to write `.torrent` file")?;
    }
    Ok(())
}
