use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;

type PieceKey = (PathBuf, usize);

struct Piece {
    piece_i: usize,
    offset: usize,
    data: Vec<u8>,
    path: PathBuf,
}

struct Cache {
    shared: Arc<Mutex<Shared>>,
}

struct Shared {
    len: usize,
    cap: usize,
    pieces: HashMap<PieceKey, Piece>,
    files: HashMap<PathBuf, Vec<PieceKey>>,
    pieces_rx: Receiver<Piece>
}

impl Cache {
    pub fn new(cap: usize, pieces_rx: Receiver<Piece>) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                len: 0,
                cap,
                pieces: HashMap::new(),
                files: HashMap::new(),
                pieces_rx,
            }))
        }
    }

    fn receive_pieces() {

    }
}