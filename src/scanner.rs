use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::SystemTime;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Debug, Clone)]
pub struct DiscoveredItem {
    pub parent_path: PathBuf,
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Debug)]
pub enum ScannerMsg {
    Batch(Vec<DiscoveredItem>),
    Progress { scanned_items: usize, scanned_bytes: u64, current_path: String },
    Finished { total_items: usize, total_bytes: u64 },
    LiveChange { path: PathBuf, kind: EventKind },
    Error(String),
}

pub struct ScannerController {
    stop_flag: Arc<AtomicBool>,
    pub scan_tx: Sender<ScannerMsg>,
    _watcher: Option<RecommendedWatcher>,
}

impl ScannerController {
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }

    pub fn rescan_subtree(&self, sub_path: PathBuf) {
        let tx = self.scan_tx.clone();
        let stop_flag = Arc::new(AtomicBool::new(false));
        thread::spawn(move || {
            scan_directory(sub_path, tx, stop_flag);
        });
    }
}

pub fn start_scanner(root_path: PathBuf) -> (Receiver<ScannerMsg>, ScannerController) {
    let (tx, rx) = mpsc::channel();
    let stop_flag = Arc::new(AtomicBool::new(false));

    // Spawn directory traversal thread
    let scan_tx = tx.clone();
    let scan_stop = Arc::clone(&stop_flag);
    let scan_root = root_path.clone();

    thread::spawn(move || {
        scan_directory(scan_root, scan_tx, scan_stop);
    });

    // Spawn live filesystem watcher
    let watch_tx = tx.clone();
    let watch_root = root_path.clone();
    let watcher = setup_watcher(watch_root, watch_tx);

    (
        rx,
        ScannerController {
            stop_flag,
            scan_tx: tx,
            _watcher: watcher,
        },
    )
}

fn scan_directory(root: PathBuf, tx: Sender<ScannerMsg>, stop_flag: Arc<AtomicBool>) {
    let mut total_items = 0usize;
    let mut total_bytes = 0u64;
    let mut batch = Vec::with_capacity(128);

    // Breadth-first / Work queue traversal
    let mut dirs_to_visit = vec![root.clone()];

    while let Some(current_dir) = dirs_to_visit.pop() {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }

        let entries = match fs::read_dir(&current_dir) {
            Ok(e) => e,
            Err(err) => {
                let _ = tx.send(ScannerMsg::Error(format!("Error reading {}: {}", current_dir.display(), err)));
                continue;
            }
        };

        for entry_res in entries {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            let entry = match entry_res {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            let is_dir = metadata.is_dir();
            let size = if is_dir { 0 } else { metadata.len() };
            let modified = metadata.modified().ok();

            total_items += 1;
            total_bytes += size;

            batch.push(DiscoveredItem {
                parent_path: current_dir.clone(),
                path: path.clone(),
                name,
                is_dir,
                size,
                modified,
            });

            if is_dir {
                dirs_to_visit.push(path);
            }

            if batch.len() >= 64 {
                let _ = tx.send(ScannerMsg::Batch(std::mem::take(&mut batch)));
                let _ = tx.send(ScannerMsg::Progress {
                    scanned_items: total_items,
                    scanned_bytes: total_bytes,
                    current_path: current_dir.to_string_lossy().to_string(),
                });
            }
        }
    }

    if !batch.is_empty() {
        let _ = tx.send(ScannerMsg::Batch(batch));
    }

    let _ = tx.send(ScannerMsg::Finished {
        total_items,
        total_bytes,
    });
}

fn setup_watcher(path: PathBuf, tx: Sender<ScannerMsg>) -> Option<RecommendedWatcher> {
    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                for p in event.paths {
                    let _ = tx.send(ScannerMsg::LiveChange {
                        path: p,
                        kind: event.kind,
                    });
                }
            }
        },
        notify::Config::default(),
    ).ok()?;

    if watcher.watch(&path, RecursiveMode::Recursive).is_ok() {
        Some(watcher)
    } else {
        None
    }
}
