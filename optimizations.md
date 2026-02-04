# Cache System Optimizations

## Current Architecture

```rust
// Every request spawns a NEW thread
pub fn process_cache_request(request: CacheRequest, tx: Sender<CacheUpdate>) {
    thread::spawn(move || {
        match request { ... }
    });
}
```

**Issues:**
1. Thread spawn overhead (~10-50μs per spawn)
2. No request deduplication (same file could be refreshed twice)
3. No priority system
4. Sequential operations within some requests (git does 5+ commands)
5. No batching of similar requests

---

## Optimization Opportunities

### 1. Thread Pool Instead of Spawning

```rust
// Current: spawns thread per request
thread::spawn(move || { ... });

// Better: use a thread pool
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

pub struct CacheWorkerPool {
    request_tx: mpsc::Sender<(CacheRequest, Sender<CacheUpdate>)>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl CacheWorkerPool {
    pub fn new(num_workers: usize) -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let request_rx = Arc::new(Mutex::new(request_rx));

        let workers: Vec<_> = (0..num_workers)
            .map(|_| {
                let rx = Arc::clone(&request_rx);
                thread::spawn(move || {
                    loop {
                        let (request, tx) = rx.lock().unwrap().recv().unwrap();
                        process_request(request, tx);
                    }
                })
            })
            .collect();

        Self { request_tx, workers }
    }

    pub fn submit(&self, request: CacheRequest, tx: Sender<CacheUpdate>) {
        self.request_tx.send((request, tx)).ok();
    }
}
```

**Benefit:** Eliminates thread spawn overhead, controls concurrency

---

### 2. Request Deduplication (In-Flight Tracking)

```rust
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub struct CacheManager {
    pool: CacheWorkerPool,
    in_flight: Arc<Mutex<HashSet<String>>>,  // context_ids being processed
}

impl CacheManager {
    pub fn request(&self, request: CacheRequest, tx: Sender<CacheUpdate>) {
        let context_id = request.context_id();

        // Skip if already processing this context
        {
            let mut in_flight = self.in_flight.lock().unwrap();
            if in_flight.contains(&context_id) {
                return;  // Already in progress, skip
            }
            in_flight.insert(context_id.clone());
        }

        // Wrap tx to remove from in_flight when done
        let in_flight = Arc::clone(&self.in_flight);
        let wrapped_tx = /* ... */;

        self.pool.submit(request, wrapped_tx);
    }
}
```

**Benefit:** Prevents duplicate work for rapid file changes

---

### 3. Parallel Git Operations

Current git refresh does commands sequentially:

```rust
// Current: sequential (slow)
let status = Command::new("git").args(["status", "--porcelain"]).output();
let branch = Command::new("git").args(["rev-parse", "--abbrev-ref", "HEAD"]).output();
let numstat_staged = Command::new("git").args(["diff", "--cached", "--numstat"]).output();
let numstat_unstaged = Command::new("git").args(["diff", "--numstat"]).output();
let branches = Command::new("git").args(["branch", "--format=..."]).output();
let diff = Command::new("git").args(["diff", "HEAD"]).output();
```

```rust
// Better: parallel with rayon or threads
use rayon::prelude::*;

fn refresh_git_status_parallel(...) {
    // Fast check first
    if !is_git_repo() { return; }

    // Run independent commands in parallel
    let (status, branch, branches) = rayon::join(
        || git_status_porcelain(),
        || rayon::join(
            || git_current_branch(),
            || git_all_branches(),
        ),
    );

    // Check hash - early exit if unchanged
    let new_hash = hash_content(&format!("{}\n{}", branch, status));
    if current_hash.as_ref() == Some(&new_hash) {
        return send_unchanged();
    }

    // Only now do expensive operations (in parallel)
    let (numstat_staged, numstat_unstaged, diff) = rayon::join(
        || git_diff_numstat_cached(),
        || rayon::join(
            || git_diff_numstat(),
            || git_diff_head(),
        ),
    );
}
```

**Benefit:** 5 sequential git commands -> 2-3 parallel batches

---

### 4. Parallel File Reading for Diffs

```rust
// Current: sequential file reads for untracked/deleted
for path in untracked_files {
    let content = fs::read_to_string(&path)?;  // Sequential!
    // ...
}

// Better: parallel with rayon
use rayon::prelude::*;

let untracked_contents: Vec<_> = untracked_files
    .par_iter()
    .filter_map(|path| {
        fs::read_to_string(path).ok().map(|c| (path.clone(), c))
    })
    .collect();
```

**Benefit:** N files read in parallel instead of sequential

---

### 5. Priority Queue for Requests

```rust
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low = 0,      // Timer-based refresh
    Normal = 1,   // File watcher events
    High = 2,     // User-triggered (tool execution)
    Critical = 3, // Wait-for-panels blocking
}

pub struct PriorityCacheRequest {
    priority: Priority,
    request: CacheRequest,
}

// Use a priority queue instead of simple channel
use std::collections::BinaryHeap;
```

**Benefit:** User actions feel snappier, background work doesn't block interactive use

---

### 6. Debouncing Rapid Changes

```rust
use std::time::{Duration, Instant};
use std::collections::HashMap;

pub struct DebouncedCache {
    pending: HashMap<String, (CacheRequest, Instant)>,
    debounce_ms: u64,
}

impl DebouncedCache {
    pub fn request(&mut self, request: CacheRequest) {
        let id = request.context_id();
        self.pending.insert(id, (request, Instant::now()));
    }

    pub fn flush(&mut self, tx: &Sender<CacheUpdate>) {
        let now = Instant::now();
        let ready: Vec<_> = self.pending
            .iter()
            .filter(|(_, (_, time))| now.duration_since(*time) > Duration::from_millis(self.debounce_ms))
            .map(|(id, (req, _))| (id.clone(), req.clone()))
            .collect();

        for (id, req) in ready {
            self.pending.remove(&id);
            process_cache_request(req, tx.clone());
        }
    }
}
```

**Benefit:** Coalesces rapid file saves (editor auto-save, formatter, etc.)

---

### 7. Lazy Diff Loading

```rust
// Current: loads ALL diffs upfront
if show_diffs && !changes.is_empty() {
    let diff = Command::new("git").args(["diff", "HEAD"]).output();  // ALL diffs
}

// Better: load diffs on-demand per file
pub enum CacheUpdate {
    GitStatus {
        // ...
        // Don't include diff_content here
    },
    GitFileDiff {
        path: String,
        diff_content: String,
    },
}

// In UI, when user expands a file:
fn on_expand_file(path: &str) {
    cache_tx.send(CacheRequest::RefreshFileDiff { path: path.to_string() });
}
```

**Benefit:** Don't load 100KB of diffs for 50 files when user only looks at 2

---

### 8. Batch Multiple File Refreshes

```rust
// Current: one request per file
CacheRequest::RefreshFile { context_id, file_path, current_hash }

// Better: batch multiple files
CacheRequest::RefreshFiles {
    files: Vec<(String, String, Option<String>)>,  // (context_id, path, hash)
}

fn refresh_files_batch(files: Vec<...>, tx: Sender<CacheUpdate>) {
    // Read all files in parallel
    let results: Vec<_> = files
        .par_iter()
        .filter_map(|(ctx_id, path, hash)| {
            let content = fs::read_to_string(path).ok()?;
            let new_hash = hash_content(&content);
            if hash.as_ref() != Some(&new_hash) {
                Some(CacheUpdate::FileContent { ... })
            } else {
                None
            }
        })
        .collect();

    // Send all updates
    for update in results {
        tx.send(update).ok();
    }
}
```

**Benefit:** Single thread spawn for N files, parallel I/O

---

## Implementation Phases

### Phase 1: Quick Wins
1. Add request deduplication (in-flight tracking)
2. Debounce rapid file changes (50-100ms window)
3. Parallelize git commands with `rayon::join`

### Phase 2: Medium Effort
4. Thread pool instead of spawning
5. Priority queue for requests
6. Batch file refreshes

### Phase 3: Larger Refactor
7. Lazy diff loading
8. Incremental updates for large files
