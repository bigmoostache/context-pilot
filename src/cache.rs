//! Background cache manager for non-blocking cache operations.
//!
//! This module handles cache invalidation and seeding in background threads
//! to ensure the main UI thread is never blocked.

use std::sync::mpsc::{self, Sender};
use std::thread;

// Re-export shared cache types from cp-base
pub use cp_base::cache::{CacheRequest, CacheUpdate, hash_content};

/// Maximum concurrent cache worker threads
const CACHE_POOL_SIZE: usize = 6;

/// Bounded thread pool for cache operations.
/// Workers pull (CacheRequest, Sender<CacheUpdate>) pairs from a shared channel.
pub struct CachePool {
    job_tx: Sender<(CacheRequest, Sender<CacheUpdate>)>,
}

impl CachePool {
    /// Create a new pool with CACHE_POOL_SIZE worker threads.
    pub fn new() -> Self {
        let (job_tx, job_rx) = mpsc::channel::<(CacheRequest, Sender<CacheUpdate>)>();
        let job_rx = std::sync::Arc::new(std::sync::Mutex::new(job_rx));

        for i in 0..CACHE_POOL_SIZE {
            let rx = std::sync::Arc::clone(&job_rx);
            thread::Builder::new()
                .name(format!("cache-worker-{}", i))
                .spawn(move || {
                    loop {
                        let job = {
                            let lock = rx.lock().unwrap_or_else(|e| e.into_inner());
                            lock.recv()
                        };
                        match job {
                            Ok((request, tx)) => {
                                let context_type = request.context_type();
                                if let Some(panel) = crate::modules::create_panel(&context_type)
                                    && let Some(update) = panel.refresh_cache(request)
                                {
                                    let _ = tx.send(update);
                                }
                            }
                            Err(_) => break, // Channel closed, pool shutting down
                        }
                    }
                })
                .ok(); // If thread spawn fails, pool just has fewer workers
        }

        Self { job_tx }
    }

    /// Submit a cache request to the pool.
    pub fn submit(&self, request: CacheRequest, tx: Sender<CacheUpdate>) {
        let _ = self.job_tx.send((request, tx));
    }
}

/// Global cache pool instance
static CACHE_POOL: std::sync::LazyLock<CachePool> = std::sync::LazyLock::new(CachePool::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_content_empty_deterministic() {
        let h = hash_content("");
        // SHA-256 of empty string is well-known
        assert_eq!(h, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn hash_content_abc() {
        let h = hash_content("abc");
        assert_eq!(h, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn hash_content_different_inputs() {
        assert_ne!(hash_content("hello"), hash_content("world"));
    }

    #[test]
    fn hash_content_idempotent() {
        assert_eq!(hash_content("test"), hash_content("test"));
    }

    #[test]
    fn hash_content_length_64() {
        // SHA-256 hex is always 64 chars
        assert_eq!(hash_content("anything").len(), 64);
    }
}

/// Process a cache request in the background via the bounded thread pool.
pub fn process_cache_request(request: CacheRequest, tx: Sender<CacheUpdate>) {
    CACHE_POOL.submit(request, tx);
}
