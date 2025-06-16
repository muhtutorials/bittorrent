pub mod download;
pub mod peer;
pub mod piece;
pub mod torrent;
pub mod tracker;

pub(crate) const BLOCK_MAX: usize = 1<<14; // 16384 (16kb)