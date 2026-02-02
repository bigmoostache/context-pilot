//! File watcher for detecting changes to open files and directories.

use std::collections::HashSet;
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
    watched_files: Arc<Mutex<HashSet<PathBuf>>>,
    watched_dirs: Arc<Mutex<HashSet<PathBuf>>>,
    event_rx: Receiver<WatchEvent>,
}

impl FileWatcher {
    pub fn new() -> notify::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let watched_files = Arc::new(Mutex::new(HashSet::new()));
        let watched_dirs = Arc::new(Mutex::new(HashSet::new()));

        let files_clone = watched_files.clone();
        let dirs_clone = watched_dirs.clone();

        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    for path in event.paths {
                        // Check if it's a watched file
                        if let Ok(files) = files_clone.lock() {
                            if files.contains(&path) {
                                let _ = tx.send(WatchEvent::FileChanged(
                                    path.to_string_lossy().to_string()
                                ));
                                continue;
                            }
                        }

                        // Check if it's in a watched directory
                        if let Ok(dirs) = dirs_clone.lock() {
                            if let Some(parent) = path.parent() {
                                if dirs.contains(&parent.to_path_buf()) {
                                    let _ = tx.send(WatchEvent::DirChanged(
                                        parent.to_string_lossy().to_string()
                                    ));
                                }
                            }
                        }
                    }
                }
            },
            Config::default(),
        )?;

        Ok(Self {
            watcher,
            watched_files,
            watched_dirs,
            event_rx: rx,
        })
    }

    /// Watch a file for changes
    pub fn watch_file(&mut self, path: &str) -> notify::Result<()> {
        let path_buf = PathBuf::from(path);
        if !path_buf.exists() {
            return Ok(());
        }

        if let Ok(mut files) = self.watched_files.lock() {
            if files.insert(path_buf.clone()) {
                self.watcher.watch(&path_buf, RecursiveMode::NonRecursive)?;
            }
        }
        Ok(())
    }

    /// Watch a directory for changes (non-recursive, only immediate children)
    pub fn watch_dir(&mut self, path: &str) -> notify::Result<()> {
        let path_buf = PathBuf::from(path);
        if !path_buf.is_dir() {
            return Ok(());
        }

        if let Ok(mut dirs) = self.watched_dirs.lock() {
            if dirs.insert(path_buf.clone()) {
                self.watcher.watch(&path_buf, RecursiveMode::NonRecursive)?;
            }
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
