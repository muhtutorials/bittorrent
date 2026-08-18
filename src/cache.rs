use crate::BLOCK_SIZE;
use core::time::Duration;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::spawn;
use tokio::task::JoinHandle;
use tokio::time;

const BLOCK_POOL_SIZE: u32 = 1 << 28; // 256MB

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

struct Cache {
    new_pieces: HashMap<PieceKey, Piece>,
    assembler: JoinHandle<()>,
}

impl Cache {
    pub fn new() -> Self {
        let assembler = spawn(async move {
            let mut interval = time::interval(Duration::from_millis(50));
            loop {
                interval.tick().await;
            }
        });
        Self {
            new_pieces: HashMap::new(),
            assembler,
        }
    }
}
