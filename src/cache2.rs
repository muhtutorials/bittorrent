use std::collections::{BTreeMap, HashMap};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use lru::LruCache;
use std::num::NonZeroUsize;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncWriteExt, AsyncSeekExt};
use tokio::sync::RwLock;
use bytes::Bytes;

// ==================== CORE DATA STRUCTURES ====================

/// Represents a cached block with its actual data and size
#[derive(Debug, Clone)]
struct CachedBlock {
    data: Bytes,
    received_at: Instant,
}

/// Represents the state of a piece in the cache
#[derive(Debug)]
struct PieceState {
    /// Map of block offset to cached block data
    blocks: BTreeMap<u32, CachedBlock>,
    /// Total size of this piece (important for last piece)
    total_size: u32,
    /// Whether this piece is complete and ready to be flushed
    is_complete: bool,
    /// The actual data when piece is fully assembled
    assembled_data: Option<Bytes>,
}

/// Main cache configuration
#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub max_memory_bytes: usize,
    pub max_pieces_in_memory: usize,
    pub flush_interval: Duration,
    pub default_block_size: u32,
}

/// The main cache manager
pub struct QBitTorrentCache {
    config: CacheConfig,
    /// LRU cache for pieces (piece_index -> PieceState)
    piece_cache: Arc<Mutex<LruCache<u32, PieceState>>>,
    /// Write queue for pieces ready to be flushed to disk
    write_queue: Arc<Mutex<Vec<WriteTask>>>,
    /// Statistics
    stats: Arc<Mutex<CacheStats>>,
    /// File handles for writing
    file_handles: Arc<RwLock<HashMap<PathBuf, File>>>,
}

/// A task for writing a completed piece to disk
struct WriteTask {
    piece_index: u32,
    data: Bytes,
    file_path: PathBuf,
    offset: u64,
}

/// Cache statistics
#[derive(Debug, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub bytes_written: u64,
    pub pieces_flushed: u64,
    pub cache_evictions: u64,
}

// ==================== IMPLEMENTATION ====================

impl QBitTorrentCache {
    pub fn new(config: CacheConfig) -> Self {
        let cap = NonZeroUsize::new(config.max_pieces_in_memory.max(1)).unwrap();

        Self {
            config,
            piece_cache: Arc::new(Mutex::new(LruCache::new(cap))),
            write_queue: Arc::new(Mutex::new(Vec::new())),
            stats: Arc::new(Mutex::new(CacheStats::default())),
            file_handles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a block to the cache
    pub async fn put_block(
        &self,
        piece_index: u32,
        block_offset: u32,
        data: Bytes,
        piece_total_size: u32,
    ) -> Result<bool, io::Error> {
        let mut cache = self.piece_cache.lock().unwrap();

        // Get or create piece state
        let piece_state = cache.get_or_insert_mut(piece_index, || PieceState {
            blocks: BTreeMap::new(),
            total_size: piece_total_size,
            is_complete: false,
            assembled_data: None,
        });

        // Insert the block with its actual size
        piece_state.blocks.insert(block_offset, CachedBlock {
            data: data.clone(),
            received_at: Instant::now(),
        });

        // Check if piece is complete
        let is_complete = Self::is_piece_complete(piece_state);
        piece_state.is_complete = is_complete;

        if is_complete {
            // Assemble the complete piece
            if let Some(assembled_data) = self.assemble_piece(piece_state) {
                piece_state.assembled_data = Some(assembled_data.clone());

                // Schedule for writing to disk
                self.schedule_write(piece_index, assembled_data, piece_total_size).await?;

                // Remove from cache to free memory (optional)
                cache.pop(&piece_index);
                self.stats.lock().unwrap().cache_evictions += 1;

                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Get a block from cache
    pub fn get_block(&self, piece_index: u32, block_offset: u32) -> Option<Bytes> {
        let mut cache = self.piece_cache.lock().unwrap();
        let mut stats = self.stats.lock().unwrap();

        if let Some(piece_state) = cache.get(&piece_index) {
            if let Some(block) = piece_state.blocks.get(&block_offset) {
                stats.hits += 1;
                return Some(block.data.clone());
            }
        }

        stats.misses += 1;
        None
    }

    /// Check if a piece is complete by verifying all blocks are present
    fn is_piece_complete(piece_state: &PieceState) -> bool {
        let mut current_offset = 0;
        let total_size = piece_state.total_size;

        for (&offset, block) in &piece_state.blocks {
            // Check if blocks are contiguous
            if offset != current_offset {
                return false;
            }
            current_offset += block.data.len() as u32;

            // If we've reached or exceeded the total size, we're done
            if current_offset >= total_size {
                return current_offset == total_size;
            }
        }

        current_offset == total_size
    }

    /// Assemble all blocks into a complete piece
    fn assemble_piece(&self, piece_state: &PieceState) -> Option<Bytes> {
        let total_size = piece_state.total_size as usize;
        let mut buffer = Vec::with_capacity(total_size);

        for (_, block) in &piece_state.blocks {
            buffer.extend_from_slice(&block.data);
        }

        if buffer.len() == total_size {
            Some(Bytes::from(buffer))
        } else {
            // This shouldn't happen if is_piece_complete returned true
            None
        }
    }

    /// Schedule a completed piece for writing to disk
    async fn schedule_write(
        &self,
        piece_index: u32,
        data: Bytes,
        piece_size: u32,
    ) -> Result<(), io::Error> {
        // In a real implementation, you'd determine the correct file and offset
        // based on the piece index and torrent metadata
        let file_path = self.get_file_path_for_piece(piece_index).await?;
        let offset = self.get_file_offset_for_piece(piece_index).await?;

        let write_task = WriteTask {
            piece_index,
            data,
            file_path,
            offset,
        };

        self.write_queue.lock().unwrap().push(write_task);
        Ok(())
    }

    /// Flush all completed pieces to disk
    pub async fn flush(&self) -> Result<(), io::Error> {
        let mut queue = self.write_queue.lock().unwrap();
        let mut stats = self.stats.lock().unwrap();

        while let Some(task) = queue.pop() {
            let mut file_handles = self.file_handles.write().await;

            // Get or create file handle
            let file = file_handles.entry(task.file_path.clone())
                .or_insert_with(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        OpenOptions::new()
                            .write(true)
                            .create(true)
                            .open(&task.file_path)
                            .await
                            .unwrap()
                    })
                });

            // Seek to correct position and write
            file.seek(std::io::SeekFrom::Start(task.offset)).await?;
            file.write_all(&task.data).await?;
            file.flush().await?;

            stats.bytes_written += task.data.len() as u64;
            stats.pieces_flushed += 1;
        }

        Ok(())
    }

    // Helper methods (simplified for example)
    async fn get_file_path_for_piece(&self, _piece_index: u32) -> Result<PathBuf, io::Error> {
        // Real implementation would map piece to file based on torrent metadata
        Ok(PathBuf::from("/tmp/torrent.data"))
    }

    async fn get_file_offset_for_piece(&self, piece_index: u32) -> Result<u64, io::Error> {
        // Real implementation would calculate based on piece index and piece size
        Ok((piece_index as u64) * (self.config.default_block_size as u64) * 1024)
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        self.stats.lock().unwrap().clone()
    }

    /// Clean up expired cache entries
    pub fn cleanup(&self, max_age: Duration) {
        let mut cache = self.piece_cache.lock().unwrap();
        let now = Instant::now();

        cache.iter_mut().for_each(|(_, piece_state)| {
            piece_state.blocks.retain(|_, block| {
                now.duration_since(block.received_at) < max_age
            });
        });
    }
}

// ==================== USAGE EXAMPLE ====================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create cache configuration
    let config = CacheConfig {
        max_memory_bytes: 256 * 1024 * 1024, // 256 MB
        max_pieces_in_memory: 1000,
        flush_interval: Duration::from_secs(5),
        default_block_size: 16384, // 16 KB
    };

    let cache = QBitTorrentCache::new(config);

    // Example: Adding a normal block
    let normal_data = Bytes::from(vec![0xAB; 16384]);
    cache.put_block(0, 0, normal_data, 16384).await?;

    // Example: Adding the last block (smaller size)
    let last_block_data = Bytes::from(vec![0xCD; 12345]); // Smaller than default
    cache.put_block(99, 0, last_block_data, 12345).await?; // Last piece total size = 12345

    // Try to get a block from cache
    if let Some(data) = cache.get_block(0, 0) {
        println!("Got block from cache: {} bytes", data.len());
    }

    // Flush completed pieces to disk
    cache.flush().await?;

    // Print statistics
    let stats = cache.stats();
    println!("Cache stats: {:#?}", stats);

    Ok(())
}

impl Clone for CacheStats {
    fn clone(&self) -> Self {
        Self {
            hits: self.hits,
            misses: self.misses,
            bytes_written: self.bytes_written,
            pieces_flushed: self.pieces_flushed,
            cache_evictions: self.cache_evictions,
        }
    }
}