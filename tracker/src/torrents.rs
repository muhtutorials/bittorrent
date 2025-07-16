use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;

#[derive(Debug, Default, Clone)]
pub struct Torrents {
    pub items: HashMap<[u8; 20], VecDeque<SocketAddr>>
}