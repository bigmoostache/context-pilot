//! Background cache manager for non-blocking cache operations.
//!
//! This module handles cache invalidation and seeding in background threads
//! to ensure the main UI thread is never blocked.

use std::sync::mpsc::{self, Sender};
use std::thread;

// Re-export shared cache types from cp-base
pub(crate) use cp_base::panels::{CacheRequest, CacheUpdate, hash_content};

/// Maximum concurrent cache worker threads
const CACHE_POOL_SIZE: usize = 6;

/// Bounded thread pool for cache operations.
/// Workers pull (`CacheRequest`, Sender<CacheUpdate>) pairs from a shared channel.
pub(crate) struct CachePool {
    /// Sender half of the job channel feeding worker threads.
    job_tx: Sender<(CacheRequest, Sender<CacheUpdate>)>,
}

impl CachePool {
    /// Create a new pool with `CACHE_POOL_SIZE` worker threads.
    pub(crate) fn new() -> Self {
        let (job_tx, job_rx) = mpsc::channel::<(CacheRequest, Sender<CacheUpdate>)>();
        let job_rx = std::sync::Arc::new(std::sync::Mutex::new(job_rx));

        for i in 0..CACHE_POOL_SIZE {
            let rx = std::sync::Arc::clone(&job_rx);
            let _r = thread::Builder::new()
                .name(format!("cache-worker-{i}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let lock = rx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                            lock.recv()
                        };
                        match job {
                            Ok((request, tx)) => {
                                let context_type = request.context_type.clone();
                                if let Some(panel) = crate::modules::create_panel(&context_type)
                                    && let Some(update) = panel.refresh_cache(request)
                                {
                                    let _r = tx.send(update);
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
    pub(crate) fn submit(&self, request: CacheRequest, tx: Sender<CacheUpdate>) {
        let _r = self.job_tx.send((request, tx));
    }
}

/// Global cache pool instance
static CACHE_POOL: std::sync::LazyLock<CachePool> = std::sync::LazyLock::new(CachePool::new);

/// Process a cache request in the background via the bounded thread pool.
pub(crate) fn process_cache_request(request: CacheRequest, tx: Sender<CacheUpdate>) {
    CACHE_POOL.submit(request, tx);
}
