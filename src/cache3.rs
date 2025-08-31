use std::collections::{HashMap, VecDeque, BTreeMap};
use std::sync::{Arc, Mutex, RwLock, Condvar};
use std::time::{Duration, Instant};
use std::path::{Path, PathBuf};
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write, Read, ErrorKind};
use std::cmp::{min, max};
use sha1::{Sha1, Digest};
use bit_vec::BitVec;
use thiserror::Error;
use crossbeam_channel::{Sender, Receiver, bounded, unbounded};
use lru::LruCache;
use parking_lot::{RwLock, Mutex as ParkingMutex};

#[derive(Debug, Error)]
pub enum QBitCacheError {
    #[error("Cache full")]
    CacheFull,
    #[error("Block not found")]
    BlockNotFound,
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Invalid piece hash")]
    InvalidPieceHash,
    #[error("File error: {0}")]
    FileError(String),
    #[error("Timeout")]
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockKey {
    pub piece_index: u32,
    pub block_offset: u32,
}

#[derive(Debug, Clone)]
pub struct CacheBlock {
    pub data: Vec<u8>,
    pub last_accessed: Instant,
    pub dirty: bool,
}

#[derive(Debug)]
pub struct PieceState {
    pub hash: [u8; 20],
    pub verified: bool,
    pub blocks_received: BitVec,
    pub total_blocks: u32,
    pub complete: bool,
}

pub struct QBitTorrentCache {
    // Configuration
    config: CacheConfig,

    // Memory cache
    block_cache: ParkingMutex<LruCache<BlockKey, CacheBlock>>,
    piece_states: RwLock<HashMap<u32, PieceState>>,

    // Disk I/O
    file_handles: ParkingMutex<HashMap<PathBuf, File>>,

    // Async operations
    io_tx: Sender<IoOperation>,
    io_rx: Receiver<IoOperation>,

    // Statistics
    stats: ParkingMutex<CacheStats>,
}

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub max_memory_size: usize,
    pub max_disk_queue: usize,
    pub write_buffer_size: usize,
    pub read_ahead_blocks: u32,
    pub flush_interval: Duration,
    pub cache_expiry: Duration,
    pub use_direct_io: bool,
    pub piece_size: u32,
    pub block_size: u32,
}

#[derive(Debug, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub writes: u64,
    pub reads: u64,
    pub evictions: u64,
    pub flush_operations: u64,
    pub current_memory_usage: usize,
    pub current_disk_queue: usize,
}

#[derive(Debug)]
enum IoOperation {
    WriteBlock {
        key: BlockKey,
        data: Vec<u8>,
        file_path: PathBuf,
        file_offset: u64,
    },
    ReadBlock {
        key: BlockKey,
        file_path: PathBuf,
        file_offset: u64,
        length: usize,
        reply: Sender<Result<Vec<u8>, QBitCacheError>>,
    },
    FlushFile {
        file_path: PathBuf,
    },
    SyncAll,
}

impl QBitTorrentCache {
    pub fn new(config: CacheConfig) -> Self {
        let (io_tx, io_rx) = bounded(config.max_disk_queue);

        let cache = Self {
            config: config.clone(),
            block_cache: ParkingMutex::new(LruCache::new(config.max_memory_size)),
            piece_states: RwLock::new(HashMap::new()),
            file_handles: ParkingMutex::new(HashMap::new()),
            io_tx,
            io_rx,
            stats: ParkingMutex::new(CacheStats::default()),
        };

        cache.start_io_threads();
        cache
    }

    pub fn start_io_threads(&self) {
        let config = self.config.clone();
        let rx = self.io_rx.clone();
        let stats = self.stats.clone();

        // Start multiple I/O threads (like qBittorrent)
        for _ in 0..num_cpus::get().max(1) {
            let rx = rx.clone();
            let config = config.clone();
            let stats = stats.clone();

            std::thread::spawn(move || {
                Self::io_worker_thread(rx, config, stats);
            });
        }
    }

    fn io_worker_thread(
        rx: Receiver<IoOperation>,
        config: CacheConfig,
        stats: ParkingMutex<CacheStats>,
    ) {
        let mut file_handles: HashMap<PathBuf, File> = HashMap::new();

        while let Ok(op) = rx.recv() {
            match op {
                IoOperation::WriteBlock { key, data, file_path, file_offset } => {
                    let result = Self::write_block_to_disk(
                        &mut file_handles,
                        &file_path,
                        file_offset,
                        &data,
                        &config,
                    );

                    if let Ok(()) = result {
                        stats.lock().writes += 1;
                    }
                }
                IoOperation::ReadBlock { key, file_path, file_offset, length, reply } => {
                    let result = Self::read_block_from_disk(
                        &mut file_handles,
                        &file_path,
                        file_offset,
                        length,
                        &config,
                    );

                    stats.lock().reads += 1;
                    let _ = reply.send(result);
                }
                IoOperation::FlushFile { file_path } => {
                    if let Some(file) = file_handles.get_mut(&file_path) {
                        let _ = file.sync_all();
                    }
                }
                IoOperation::SyncAll => {
                    for file in file_handles.values_mut() {
                        let _ = file.sync_all();
                    }
                    stats.lock().flush_operations += 1;
                }
            }
        }
    }

    pub fn write_block(
        &self,
        piece_index: u32,
        block_offset: u32,
        data: Vec<u8>,
        file_path: &Path,
        file_offset: u64,
    ) -> Result<(), QBitCacheError> {
        let key = BlockKey { piece_index, block_offset };

        // Update piece state
        self.update_piece_state(piece_index, block_offset, data.len() as u32);

        // Store in memory cache
        let mut cache = self.block_cache.lock();
        let block = CacheBlock {
            data: data.clone(),
            last_accessed: Instant::now(),
            dirty: true,
        };

        cache.put(key, block);
        self.stats.lock().current_memory_usage += data.len();

        // Queue for disk write (asynchronous)
        self.queue_disk_write(key, data, file_path.to_path_buf(), file_offset)?;

        Ok(())
    }

    pub fn read_block(
        &self,
        piece_index: u32,
        block_offset: u32,
        file_path: &Path,
        file_offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, QBitCacheError> {
        let key = BlockKey { piece_index, block_offset };

        // Try memory cache first
        if let Some(block) = self.block_cache.lock().get(&key) {
            self.stats.lock().hits += 1;
            return Ok(block.data.clone());
        }

        self.stats.lock().misses += 1;

        // Read from disk (synchronous for now, could be async)
        let (tx, rx) = bounded(1);
        let op = IoOperation::ReadBlock {
            key,
            file_path: file_path.to_path_buf(),
            file_offset,
            length,
            reply: tx,
        };

        self.io_tx.send(op)?;

        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(result) => {
                if let Ok(data) = result {
                    // Cache the read block
                    let mut cache = self.block_cache.lock();
                    let block = CacheBlock {
                        data: data.clone(),
                        last_accessed: Instant::now(),
                        dirty: false,
                    };
                    cache.put(key, block);
                    self.stats.lock().current_memory_usage += data.len();

                    Ok(data)
                } else {
                    Err(QBitCacheError::BlockNotFound)
                }
            }
            Err(_) => Err(QBitCacheError::Timeout),
        }
    }

    pub fn verify_piece(
        &self,
        piece_index: u32,
        expected_hash: &[u8; 20],
        file_path: &Path,
        piece_offset: u64,
        piece_length: u32,
    ) -> Result<bool, QBitCacheError> {
        // Read entire piece (could be optimized)
        let mut piece_data = Vec::with_capacity(piece_length as usize);
        let block_size = self.config.block_size as usize;
        let blocks = (piece_length + self.config.block_size - 1) / self.config.block_size;

        for block in 0..blocks {
            let block_offset = (block * self.config.block_size) as u32;
            let read_length = min(block_size, (piece_length - block * self.config.block_size) as usize);

            let file_offset = piece_offset + (block * self.config.block_size) as u64;
            let data = self.read_block(piece_index, block_offset, file_path, file_offset, read_length)?;
            piece_data.extend_from_slice(&data);
        }

        // Verify hash
        let mut hasher = Sha1::new();
        hasher.update(&piece_data);
        let actual_hash = hasher.finalize();

        let is_valid = &actual_hash[..] == expected_hash;

        if is_valid {
            let mut states = self.piece_states.write();
            if let Some(state) = states.get_mut(&piece_index) {
                state.verified = true;
                state.complete = true;
            }
        }

        Ok(is_valid)
    }

    pub fn flush(&self) -> Result<(), QBitCacheError> {
        // Flush all dirty blocks to disk
        let mut cache = self.block_cache.lock();
        let mut to_flush = Vec::new();

        for (key, block) in cache.iter() {
            if block.dirty {
                to_flush.push((*key, block.data.clone()));
            }
        }

        // In real implementation, you'd have file mapping information
        for (key, data) in to_flush {
            // This would use actual file mapping logic
            let file_path = PathBuf::from(format!("/tmp/piece_{}.bin", key.piece_index));
            let file_offset = (key.piece_index as u64 * self.config.piece_size as u64) + key.block_offset as u64;

            self.queue_disk_write(key, data, file_path, file_offset)?;
        }

        // Sync all files
        self.io_tx.send(IoOperation::SyncAll)?;

        Ok(())
    }

    pub fn cleanup_expired(&self) {
        let mut cache = self.block_cache.lock();
        let now = Instant::now();
        let expiry = self.config.cache_expiry;

        cache.iter_mut().for_each(|(_, block)| {
            if now.duration_since(block.last_accessed) > expiry && !block.dirty {
                // Mark for eviction
            }
        });

        // LRU cache will handle eviction automatically
    }

    pub fn prefetch_blocks(
        &self,
        piece_index: u32,
        current_block: u32,
        file_path: &Path,
        piece_offset: u64,
    ) {
        let read_ahead = self.config.read_ahead_blocks;
        let block_size = self.config.block_size;

        for offset in 1..=read_ahead {
            let block_offset = current_block + offset * block_size;
            let file_offset = piece_offset + block_offset as u64;

            // Asynchronous prefetch
            let _ = self.read_block(piece_index, block_offset, file_path, file_offset, block_size as usize);
        }
    }

    // Helper methods
    fn queue_disk_write(
        &self,
        key: BlockKey,
        data: Vec<u8>,
        file_path: PathBuf,
        file_offset: u64,
    ) -> Result<(), QBitCacheError> {
        let op = IoOperation::WriteBlock {
            key,
            data,
            file_path,
            file_offset,
        };

        self.io_tx.send(op).map_err(|_| QBitCacheError::CacheFull)
    }

    fn update_piece_state(&self, piece_index: u32, block_offset: u32, block_length: u32) {
        let mut states = self.piece_states.write();
        let state = states.entry(piece_index).or_insert_with(|| PieceState {
            hash: [0; 20],
            verified: false,
            blocks_received: BitVec::from_elem((self.config.piece_size / self.config.block_size) as usize, false),
            total_blocks: self.config.piece_size / self.config.block_size,
            complete: false,
        });

        let block_index = (block_offset / self.config.block_size) as usize;
        if block_index < state.blocks_received.len() {
            state.blocks_received.set(block_index, true);

            // Check if piece is complete
            state.complete = state.blocks_received.all();
        }
    }

    fn write_block_to_disk(
        file_handles: &mut HashMap<PathBuf, File>,
        file_path: &Path,
        offset: u64,
        data: &[u8],
        config: &CacheConfig,
    ) -> Result<(), QBitCacheError> {
        let file = file_handles.entry(file_path.to_path_buf())
            .or_insert_with(|| {
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .read(true)
                    .open(file_path)
                    .unwrap()
            });

        file.seek(SeekFrom::Start(offset))?;
        file.write_all(data)?;

        Ok(())
    }

    fn read_block_from_disk(
        file_handles: &mut HashMap<PathBuf, File>,
        file_path: &Path,
        offset: u64,
        length: usize,
        config: &CacheConfig,
    ) -> Result<Vec<u8>, QBitCacheError> {
        let file = file_handles.entry(file_path.to_path_buf())
            .or_insert_with(|| {
                OpenOptions::new()
                    .read(true)
                    .open(file_path)
                    .unwrap()
            });

        file.seek(SeekFrom::Start(offset))?;
        let mut buffer = vec![0; length];
        file.read_exact(&mut buffer)?;

        Ok(buffer)
    }
}