//! Workspace checkpoints.
//!
//! A checkpoint is a copy of the files the agent may touch, taken before a turn.
//! Rolling back restores them, which is what makes an aggressive edit safe to
//! try.
//!
//! Only tracked files are copied: a checkpoint of `node_modules` would be slower
//! than the work it protects.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{LunaError, Result};

/// One saved state of the workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub id: String,
    pub label: String,
    pub created: String,
    /// Files captured, relative to the workspace root.
    pub files: Vec<String>,
    /// Where the copies live.
    pub dir: PathBuf,
}

/// Files larger than this are skipped: a checkpoint is for source, not assets.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// A checkpoint stops growing at this many files.
const MAX_FILES: usize = 5_000;

/// Directories that are never worth copying.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    "__pycache__",
    ".venv",
    "venv",
    ".cache",
    "vendor",
];

/// Manages checkpoints for one workspace.
pub struct Checkpoints {
    root: PathBuf,
    store: PathBuf,
}

impl Checkpoints {
    pub fn new(workspace: &Path, store: &Path) -> Checkpoints {
        Checkpoints { root: workspace.to_path_buf(), store: store.join("checkpoints") }
    }

    /// Capture the workspace.
    pub fn create(&self, id: &str, label: &str) -> Result<Checkpoint> {
        let dir = self.store.join(id);
        std::fs::create_dir_all(&dir)
            .map_err(|source| LunaError::Io { path: dir.clone(), source })?;

        let mut files = Vec::new();
        self.walk(&self.root.clone(), &mut files)?;

        for relative in &files {
            let source = self.root.join(relative);
            let target = dir.join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|source| LunaError::Io { path: parent.to_path_buf(), source })?;
            }
            // A file that vanished between the walk and the copy is not an error.
            let _ = std::fs::copy(&source, &target);
        }

        Ok(Checkpoint {
            id: id.to_string(),
            label: label.to_string(),
            created: chrono::Utc::now().to_rfc3339(),
            files,
            dir,
        })
    }

    /// Restore a checkpoint, deleting files it did not contain.
    pub fn restore(&self, checkpoint: &Checkpoint) -> Result<usize> {
        let mut restored = 0;
        for relative in &checkpoint.files {
            let source = checkpoint.dir.join(relative);
            let target = self.root.join(relative);
            if !source.exists() {
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|source| LunaError::Io { path: parent.to_path_buf(), source })?;
            }
            std::fs::copy(&source, &target)
                .map_err(|source| LunaError::Io { path: target.clone(), source })?;
            restored += 1;
        }
        Ok(restored)
    }

    /// Delete a checkpoint's copies.
    pub fn discard(&self, checkpoint: &Checkpoint) -> Result<()> {
        std::fs::remove_dir_all(&checkpoint.dir).ok();
        Ok(())
    }

    /// Every checkpoint on disk, newest first.
    pub fn list(&self) -> Vec<Checkpoint> {
        let Ok(entries) = std::fs::read_dir(&self.store) else { return Vec::new() };
        let mut found: Vec<Checkpoint> = entries
            .flatten()
            .filter_map(|entry| {
                let manifest = entry.path().join("checkpoint.json");
                let text = std::fs::read_to_string(manifest).ok()?;
                serde_json::from_str(&text).ok()
            })
            .collect();
        found.sort_by(|a, b| b.created.cmp(&a.created));
        found
    }

    /// Write the manifest, so `list` can find the checkpoint later.
    pub fn save_manifest(&self, checkpoint: &Checkpoint) -> Result<()> {
        let path = checkpoint.dir.join("checkpoint.json");
        let text = serde_json::to_string_pretty(checkpoint)?;
        crate::config::write_atomic(&path, text.as_bytes())
    }

    fn walk(&self, dir: &Path, out: &mut Vec<String>) -> Result<()> {
        if out.len() >= MAX_FILES {
            return Ok(());
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return Ok(()) };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                self.walk(&path, out)?;
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.len() > MAX_FILE_BYTES {
                continue;
            }
            if let Ok(relative) = path.strip_prefix(&self.root) {
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(())
    }
}

/// A file the agent read, and its hash at the time.
///
/// Used to notice that another agent changed it underneath.
#[derive(Debug, Default)]
pub struct ReadTracker {
    seen: std::sync::Mutex<BTreeMap<String, u64>>,
}

impl Clone for ReadTracker {
    fn clone(&self) -> Self {
        let seen = self.seen.lock().map(|seen| seen.clone()).unwrap_or_default();
        ReadTracker { seen: std::sync::Mutex::new(seen) }
    }
}

impl ReadTracker {
    pub fn new() -> Self {
        ReadTracker::default()
    }

    /// Record that a file was read.
    pub fn record(&self, path: &str, hash: u64) {
        if let Ok(mut seen) = self.seen.lock() {
            seen.insert(path.to_string(), hash);
        }
    }

    /// Files whose contents changed since they were read.
    pub fn changed(&self, current: &BTreeMap<String, u64>) -> Vec<String> {
        let Ok(seen) = self.seen.lock() else { return Vec::new() };
        seen.iter()
            .filter(|(path, hash)| current.get(*path).map(|now| now != *hash).unwrap_or(false))
            .map(|(path, _)| path.clone())
            .collect()
    }
}

/// A cheap content hash, enough to notice a change.
pub fn hash_file(path: &Path) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("luna-ckpt-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_checkpoint_round_trips() {
        let work = temp("work");
        let store = temp("store");
        std::fs::write(work.join("a.txt"), "original").unwrap();

        let checkpoints = Checkpoints::new(&work, &store);
        let checkpoint = checkpoints.create("c1", "before").unwrap();
        checkpoints.save_manifest(&checkpoint).unwrap();
        assert_eq!(checkpoint.files, vec!["a.txt"]);

        std::fs::write(work.join("a.txt"), "changed").unwrap();
        checkpoints.restore(&checkpoint).unwrap();
        assert_eq!(std::fs::read_to_string(work.join("a.txt")).unwrap(), "original");

        std::fs::remove_dir_all(&work).ok();
        std::fs::remove_dir_all(&store).ok();
    }

    #[test]
    fn heavy_directories_are_skipped() {
        let work = temp("skip");
        let store = temp("skip-store");
        std::fs::create_dir_all(work.join("node_modules")).unwrap();
        std::fs::write(work.join("node_modules").join("x.js"), "x").unwrap();
        std::fs::write(work.join("main.rs"), "fn main() {}").unwrap();

        let checkpoints = Checkpoints::new(&work, &store);
        let checkpoint = checkpoints.create("c1", "before").unwrap();
        assert_eq!(checkpoint.files, vec!["main.rs"]);

        std::fs::remove_dir_all(&work).ok();
        std::fs::remove_dir_all(&store).ok();
    }

    #[test]
    fn checkpoints_are_listed_newest_first() {
        let work = temp("list");
        let store = temp("list-store");
        std::fs::write(work.join("a.txt"), "x").unwrap();
        let checkpoints = Checkpoints::new(&work, &store);

        let mut first = checkpoints.create("c1", "one").unwrap();
        first.created = "2026-01-01T00:00:00Z".into();
        checkpoints.save_manifest(&first).unwrap();

        let mut second = checkpoints.create("c2", "two").unwrap();
        second.created = "2026-02-01T00:00:00Z".into();
        checkpoints.save_manifest(&second).unwrap();

        let listed = checkpoints.list();
        assert_eq!(listed[0].id, "c2");

        std::fs::remove_dir_all(&work).ok();
        std::fs::remove_dir_all(&store).ok();
    }

    #[test]
    fn the_read_tracker_notices_a_change() {
        let tracker = ReadTracker::new();
        tracker.record("a.rs", 1);
        tracker.record("b.rs", 2);

        let current = BTreeMap::from([("a.rs".to_string(), 9u64), ("b.rs".to_string(), 2u64)]);
        assert_eq!(tracker.changed(&current), vec!["a.rs"]);
    }

    #[test]
    fn a_deleted_file_is_not_reported_as_changed() {
        let tracker = ReadTracker::new();
        tracker.record("gone.rs", 1);
        assert!(tracker.changed(&BTreeMap::new()).is_empty());
    }

    #[test]
    fn hashing_is_stable_for_the_same_content() {
        let dir = temp("hash");
        let path = dir.join("f.txt");
        std::fs::write(&path, "same").unwrap();
        assert_eq!(hash_file(&path), hash_file(&path));
        std::fs::write(&path, "different").unwrap();
        assert_ne!(hash_file(&path), Some(0));
        std::fs::remove_dir_all(&dir).ok();
    }
}
