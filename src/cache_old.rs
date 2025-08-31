use crate::BLOCK_SIZE;
use anyhow::anyhow;
use std::collections::{BTreeMap, HashMap, VecDeque};

const BLOCK_COUNT: usize = 1 << 14; // 16384

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct BlockKey {
    torrent_id: usize,
    piece_i: usize,
    // block offset within piece
    offset: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BlockState {
    // block is free to use
    Free,
    // holds data, not yet scheduled for write
    Active,
    // scheduled for write
    Dirty,
    // currently being written to disk
    Writing,
}

// metadata and storage for a single cache block
struct Block {
    // The key this block is currently associated with.
    // This is None if the block is Free or Writing (after being queued).
    key: Option<BlockKey>,
    state: BlockState,
    data: Vec<u8>,
    // how many bytes in `data` are actually valid
    len: usize,
    // The absolute position in the file where this data belongs.
    // This is stored so the I/O thread knows where to write it.
    file_offset: u64,
}

pub struct Piece<'a> {
    // Map of block offset to block data.
    blocks: BTreeMap<u32, &'a [u8]>,
    // Total size of this piece (important for last piece).
    total_size: u32,
    // Whether this piece is complete and ready to be flushed.
    is_complete: bool,
}

pub struct Cache {
    // The pool of all cache blocks. Their index in this vector is their ID.
    blocks: Vec<Block>,
    // A list of indices pointing to blocks in the `blocks` vec that are free.
    free_blocks: Vec<usize>,
    // A list of indices pointing to blocks that are dirty and need to be written.
    dirty_blocks: VecDeque<usize>,
    // A hash map to quickly find if a logical piece of data is already in cache.
    // Maps (torrent id, piece index, offset) -> index in `blocks`
    lookup_table: HashMap<BlockKey, usize>,
}

impl Cache {
    fn new() -> Self {
        let mut blocks = Vec::with_capacity(BLOCK_COUNT);
        let mut free_blocks = Vec::with_capacity(BLOCK_COUNT);

        // initialize all blocks as free and add them to the free list
        for i in 0..BLOCK_COUNT {
            blocks.push(Block {
                state: BlockState::Free,
                data: vec![0; BLOCK_SIZE],
                len: 0,
                key: None,
                file_offset: 0,
            });
            // The index of the block is its ID.
            free_blocks.push(i);
        }
        Self {
            blocks,
            free_blocks,
            dirty_blocks: VecDeque::new(),
            lookup_table: HashMap::new(),
        }
    }

    pub fn get_block(&mut self, key: BlockKey, file_offset: u64) -> Option<&mut [u8]> {
        let block_id = self.free_blocks.pop()?;
        let block = &mut self.blocks[block_id];
        // Sanity check: ensure it was actually free.
        if block.state != BlockState::Free {
            return None;
        }
        block.key = Some(key);
        block.state = BlockState::Active;
        block.file_offset = file_offset;
        // Insert this block into the lookup table so we can find it later.
        self.lookup_table.insert(key, block_id);
        Some(block.data.as_mut_slice())
    }

    // Marks an `Active` block as `Dirty`, sets its length and schedules it for writing.
    pub fn mark_as_dirty(&mut self, key: &BlockKey, len: usize) -> anyhow::Result<()> {
        let &block_id = self
            .lookup_table
            .get(key)
            .ok_or(anyhow!("couldn't mark block as dirty"))?;
        let block = &mut self.blocks[block_id];

        if block.state != BlockState::Active {
            return Err(anyhow!("invalid block state"));
        }
        block.state = BlockState::Dirty;
        block.len = len;
        // Add this block's ID to the end of the dirty queue.
        self.dirty_blocks.push_back(block_id);
        Ok(())
    }

    // Gets the next batch of dirty blocks ready for writing.
    // This is called by the I/O thread. Moves blocks from `Dirty` to `Writing` state.
    pub fn get_blocks_for_write(&mut self) -> Vec<(usize, usize, usize, u64)> {
        let mut batch = Vec::new();
        while let Some(block_id) = self.dirty_blocks.pop_front() {
            let block = &mut self.blocks[block_id];
            // Check the state before proceeding.
            if block.state != BlockState::Dirty {
                continue;
            }
            let torrent_id = block.key.as_ref().unwrap().torrent_id;
            block.state = BlockState::Writing;
            batch.push((torrent_id, block_id, block.len, block.file_offset));
        };
        batch
    }

    // Called by the I/O thread after successfully writing a block to disk.
    // This releases the block back to the free pool.
    pub fn write_complete(&mut self, block_id: usize) -> anyhow::Result<()> {
        let block = &mut self.blocks[block_id];
        if block.state != BlockState::Writing {
            return Err(anyhow!("invalid block state"));
        }
        // If this block was still in the lookup table, remove it.
        // The data is now on disk, so we don't need to cache it anymore.
        if let Some(key) = block.key.take() {
            self.lookup_table.remove(&key);
        }
        // Reset the block's state and add it back to the free blocks.
        block.state = BlockState::Free;
        block.len = 0;
        block.file_offset = 0;
        self.free_blocks.push(block_id);
        Ok(())
    }
}
