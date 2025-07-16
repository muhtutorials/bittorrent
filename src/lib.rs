pub mod bitfield;
pub mod client;
pub mod create;
pub mod db;
pub mod dot_torrent;
pub mod download;
pub mod peer;
pub mod piece;
pub mod state;
pub mod torrents;
pub mod tracker;

pub(crate) const BLOCK_MAX: usize = 1 << 14; // 16384 (16kb)
