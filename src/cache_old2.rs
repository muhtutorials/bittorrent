use crate::BLOCK_SIZE;
use crate::bit_vec::BitVec;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

const CACHE_SIZE: usize = 1 << 28;

const BLOCK_NUM: usize = CACHE_SIZE / BLOCK_SIZE;

type Buf = [u8; BLOCK_SIZE];

struct Block {
    torrent_id: usize,
    // hash of the piece
    hash: [u8; 20],
    piece_i: usize,
    // offset inside piece
    offset: usize,
    length: usize,
    data: Buf
}

struct WritePiece {
    index: usize,
    length: usize,
    hash: [u8; 20],
    blocks: BitVec,
    bufs: Vec<usize>,
}

struct Cache {
    shared: Arc<Mutex<Shared>>,
}

struct Shared {
    bufs: Vec<Vec<u8>>,
    free_bufs: VecDeque<usize>,
    write_bufs: BitVec,
    write_pieces: HashMap<[u8; 20], WritePiece>,
    write_files: HashSet<PathBuf>,
}

impl Cache {
    pub fn new() -> Self {
        let mut bufs = Vec::with_capacity(BLOCK_NUM);
        for _ in 0..BLOCK_NUM {
            bufs.push(Vec::with_capacity(BLOCK_SIZE));
        }
        let mut free_bufs = VecDeque::with_capacity(BLOCK_NUM);
        for i in 0..BLOCK_NUM {
            free_bufs.push_back(i);
        }
        Self {
            shared: Arc::new(Mutex::new(Shared {
                bufs,
                free_bufs,
                write_bufs: BitVec::new(BLOCK_NUM),
                write_pieces: HashMap::new(),
            })),
        }
    }

    pub fn add_piece(&self) {

    }

    pub async fn acquire_block(&self) -> (usize, Buf) {
        loop {
            let shared = self.shared.lock().await;
            if let Some(index) = shared.free_bufs.pop_back() {
                return (index, [0u8; 20]);
            };
            sleep(Duration::from_secs(1)).await;
        }
    }

    pub fn receive_block(&self, block: Block) {

    }
}
