use crate::BLOCK_SIZE;
use core::time::Duration;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::spawn;
use tokio::time;

const BLOCK_POOL_SIZE: u32 = 1 << 28; // 256MB

struct Cache {
    new_pieces: HashMap<PieceKey, Piece>,
}

impl Cache {
    pub fn new() -> Self {
        spawn(async move {
            let mut interval = time::interval(Duration::from_millis(50));
            loop {
                interval.tick().await;
            }
        });
        Self {
            new_pieces: HashMap::new(),
        }
    }
}

struct PieceKey {
    // path to the file containing the piece
    path: PathBuf,
    // piece index inside the file
    index: usize,
}

struct Piece {
    index: usize,
    length: usize,
    hash: [u8; 20],
    blocks: Vec<Option<Vec<u8>>>,
    state: PieceState,
}

enum PieceState {
    Incomplete, // in process of receiving blocks
    Assembled,  // all blocks received (in memory)
    Verified,   // hash verified (in memory)
    Flushed,    // written to disk (persisted)
}

impl Piece {
    pub fn new(index: usize, length: usize, hash: [u8; 20]) -> Self {
        // Rounding up when doing division:
        // (16384 + 16384 - 1) / 16384
        //  = (32767) / 16384
        //  = 1 (integer division truncates)
        // (16385 + 16384 - 1) / 16384
        //  = (32768) / 16384
        //  = 2
        let n_blocks = (length + BLOCK_SIZE - 1) / BLOCK_SIZE;
        Self {
            index,
            length,
            hash,
            blocks: vec![None; n_blocks as usize],
            state: PieceState::Incomplete,
        }
    }

    pub fn add_block(&mut self, offset: usize, data: Vec<u8>) -> Result<(), String> {
        let index = offset / BLOCK_SIZE;
        if index >= self.blocks.len() {
            return Err("Invalid block offset".to_string());
        }
        self.blocks[index] = Some(data);
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.blocks.iter().all(|block| block.is_some())
    }

    pub fn assemble(&self) -> Option<Vec<u8>> {
        if !self.is_complete() {
            return None;
        }
        let mut data = Vec::with_capacity(self.length as usize);
        for block in &self.blocks {
            data.extend_from_slice(block.as_ref().unwrap());
        }
        Some(data)
    }
}

// use crossbeam::queue::ArrayQueue;
// use std::sync::Arc;

// pub struct BufferPool {
//     queue: ArrayQueue<Vec<u8>>,
//     block_len: usize,
// }

// impl BufferPool {
//     pub fn new(capacity: usize, block_len: usize) -> Arc<Self> {
//         let queue = ArrayQueue::new(capacity);
//         for _ in 0..capacity {
//             // Pre-fill so `get` never fails under normal load.
//             let _ = queue.push(vec![0u8; block_len]);
//         }
//         Arc::new(Self { queue, block_len })
//     }

//     /// Take a buffer. Allocates if the pool is empty (and logs/grows).
//     pub fn get(&self) -> Vec<u8> {
//         self.queue.pop().unwrap_or_else(|| {
//             // Pool exhausted — either block, grow, or allocate.
//             vec![0u8; self.block_len]
//         })
//     }

//     /// Return a buffer. Dropped if the pool is full.
//     pub fn put(&self, mut buf: Vec<u8>) {
//         if buf.len() != self.block_len {
//             buf.resize(self.block_len, 0);
//         }
//         let _ = self.queue.push(buf); // ignore if full
//     }
// }
