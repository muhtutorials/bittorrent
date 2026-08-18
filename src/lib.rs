pub mod bit_vec;
pub mod cache;

pub mod client;
pub use client::Client;

pub mod cmd;
pub mod db;
pub mod dot_torrent;
pub mod download;
pub mod ipc;
pub mod lru_cache;
pub mod peer;
pub mod piece;
pub mod state;
pub mod torrent;
pub mod tracker;

pub(crate) const BLOCK_SIZE: usize = 1 << 14; // 16384 (16kb)
