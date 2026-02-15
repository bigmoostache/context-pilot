//! File watcher for detecting changes to open files and directories.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

/// Events sent from the file watcher
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// A watched file changed
    FileChanged(String),
    /// A watched directory changed (file added/removed)
    DirChanged(String),
}

/// File watcher that monitors open files and directories
pub struct FileWatcher {
    watcher: RecommendedWatcher,
    /// Maps canonical path -> original path (for returning original path in events)
    watched_files: Arc<Mutex<HashMap<PathBuf, String>>>,
    /// Maps canonical path -> original path
    watched_dirs: Arc<Mutex<HashMap<PathBuf, String>>>,
    event_rx: Receiver<WatchEvent>,
}

impl FileWatcher {
    pub fn new() -> notify::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let watched_files: Arc<Mutex<HashMap<PathBuf, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let watched_dirs: Arc<Mutex<HashMap<PathBuf, String>>> = Arc::new(Mutex::new(HashMap::new()));

        let files_clone = watched_files.clone();
        let dirs_clone = watched_dirs.clone();

        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    for path in event.paths {
                        // Canonicalize the event path for comparison
                        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());

                        // Check if it's a watched file
                        if let Ok(files) = files_clone.lock()
                            && let Some(original_path) = files.get(&canonical)
                        {
                            let _ = tx.send(WatchEvent::FileChanged(original_path.clone()));
                            continue;
                        }

                        // Check if it's in a watched directory
                        if let Ok(dirs) = dirs_clone.lock()
                            && let Some(parent) = canonical.parent()
                            && let Some(original_path) = dirs.get(&parent.to_path_buf())
                        {
                            let _ = tx.send(WatchEvent::DirChanged(original_path.clone()));
                        }
                    }
                }
            },
            Config::default(),
        )?;

        Ok(Self { watcher, watched_files, watched_dirs, event_rx: rx })
    }

    /// Watch a file for changes
    pub fn watch_file(&mut self, path: &str) -> notify::Result<()> {
        let path_buf = PathBuf::from(path);
        if !path_buf.exists() {
            return Ok(());
        }

        // Canonicalize for storage and comparison
        let canonical = path_buf.canonicalize().unwrap_or_else(|_| path_buf.clone());

        if let Ok(mut files) = self.watched_files.lock()
            && !files.contains_key(&canonical)
        {
            files.insert(canonical.clone(), path.to_string());
            self.watcher.watch(&canonical, RecursiveMode::NonRecursive)?;
        }
        Ok(())
    }

    /// Watch a directory for changes (non-recursive, only immediate children)
    pub fn watch_dir(&mut self, path: &str) -> notify::Result<()> {
        let path_buf = PathBuf::from(path);
        if !path_buf.is_dir() {
            return Ok(());
        }

        // Canonicalize for storage and comparison
        let canonical = path_buf.canonicalize().unwrap_or_else(|_| path_buf.clone());

        if let Ok(mut dirs) = self.watched_dirs.lock()
            && !dirs.contains_key(&canonical)
        {
            dirs.insert(canonical.clone(), path.to_string());
            self.watcher.watch(&canonical, RecursiveMode::NonRecursive)?;
        }
        Ok(())
    }

    /// Watch a directory recursively (for .git/refs/ subdirs)
    pub fn watch_dir_recursive(&mut self, path: &str) -> notify::Result<()> {
        let path_buf = PathBuf::from(path);
        if !path_buf.is_dir() {
            return Ok(());
        }

        let canonical = path_buf.canonicalize().unwrap_or_else(|_| path_buf.clone());

        if let Ok(mut dirs) = self.watched_dirs.lock()
            && !dirs.contains_key(&canonical)
        {
            dirs.insert(canonical.clone(), path.to_string());
            self.watcher.watch(&canonical, RecursiveMode::Recursive)?;
        }
        Ok(())
    }

    /// Re-watch a file that may have been replaced (e.g., by an editor using atomic rename).
    /// Removes the stale watch and creates a new one on the current inode at that path.
    pub fn rewatch_file(&mut self, path: &str) -> notify::Result<()> {
        let path_buf = PathBuf::from(path);
        if !path_buf.exists() {
            return Ok(());
        }

        let canonical = path_buf.canonicalize().unwrap_or_else(|_| path_buf.clone());

        // Unwatch the old inode (may already be gone after kernel removed it)
        let _ = self.watcher.unwatch(&canonical);

        // Re-watch the path (now points to the new inode)
        self.watcher.watch(&canonical, RecursiveMode::NonRecursive)?;

        // Ensure mapping is up-to-date
        if let Ok(mut files) = self.watched_files.lock() {
            files.insert(canonical, path.to_string());
        }

        Ok(())
    }

    /// Poll for watch events (non-blocking)
    pub fn poll_events(&self) -> Vec<WatchEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }
}
