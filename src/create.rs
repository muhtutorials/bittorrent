use crate::torrent::hashes::Hashes;
use crate::torrent::{Info, Key, Torrent};
use memmap2::Mmap;
use sha1::{Digest, Sha1};
use std::fs::File;
use std::io::{Error, ErrorKind, Result};
use std::path::PathBuf;

const PIECE_LENGTH: usize = 32768;

pub async fn create_torrent(path: PathBuf) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .ok_or(Error::new(ErrorKind::NotFound, "invalid path"))?;
    let mut torrent = Torrent {
        announce: "http://bittorrent-test-tracker.codecrafters.io/announce".to_string(),
        info: Info {
            name,
            piece_length: PIECE_LENGTH,
            pieces: Hashes(Vec::new()),
            key: Key::SingleFile { length: 0 },
        },
    };
    if path.is_file() {
        let file = File::open(path).expect("failed to open the file");
        let mmap = unsafe { Mmap::map(&file).expect("failed to map the file") };
        let file_length = mmap.len();
        torrent.info.key = Key::SingleFile {
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
            torrent.info.pieces.0.push(hash);
        }
        let bencoded_torrent = serde_bencode::to_bytes(&torrent)
            .map_err(|_| Error::new(ErrorKind::InvalidData, "invalid data during encoding"))?;
        let mut path = PathBuf::from("./");
        path.push(&torrent.info.name);
        path.set_extension("torrent");
        tokio::fs::write(path, &bencoded_torrent).await?;
    }
    Ok(())
}
