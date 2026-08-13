//! Activity cache: persistent local history of push/pull/delete/label ops.
//!
//! Storage: `~/.cache/oci-sync/activity.json` (XDG-aware).
//! Keeps at most [`MAX_ACTIVITIES`] entries, newest first.

use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::xdg;

pub const CACHE_FILE_NAME: &str = "activity.json";
pub const MAX_ACTIVITIES: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivityType {
    Push,
    Pull,
    Delete,
    Label,
}

impl ActivityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActivityType::Push => "push",
            ActivityType::Pull => "pull",
            ActivityType::Delete => "delete",
            ActivityType::Label => "label",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    #[serde(rename = "type")]
    pub kind: ActivityType,
    pub timestamp: DateTime<Local>,
    pub remote_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ActivityCache {
    #[serde(default)]
    pub activities: Vec<Activity>,
}

/// Path of the activity cache file.
pub fn cache_file_path() -> PathBuf {
    xdg::cache_dir().join("oci-sync").join(CACHE_FILE_NAME)
}

/// Load the cache; a missing file yields an empty cache (not an error).
pub fn load() -> Result<ActivityCache> {
    let path = cache_file_path();
    match std::fs::read_to_string(&path) {
        Ok(data) => {
            serde_json::from_str(&data).with_context(|| format!("parse cache {}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ActivityCache::default()),
        Err(e) => Err(e).with_context(|| format!("read cache {}", path.display())),
    }
}

fn save(cache: &ActivityCache) -> Result<()> {
    let path = cache_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create cache dir {}", parent.display()))?;
    }
    let data = serde_json::to_string_pretty(cache).context("serialize cache")?;
    std::fs::write(&path, data).with_context(|| format!("write cache {}", path.display()))?;
    Ok(())
}

/// Prepend an activity, trimming to [`MAX_ACTIVITIES`].
pub fn add(activity: Activity) -> Result<()> {
    let mut cache = load()?;
    cache.activities.insert(0, activity);
    cache.activities.truncate(MAX_ACTIVITIES);
    save(&cache)
}

/// Newest-first activities, limited to `limit` (0 = all).
pub fn recent(limit: usize) -> Result<Vec<Activity>> {
    let cache = load()?;
    Ok(if limit == 0 {
        cache.activities
    } else {
        cache.activities.into_iter().take(limit).collect()
    })
}

/// Count activities per type.
pub fn stats() -> Result<std::collections::BTreeMap<String, usize>> {
    let cache = load()?;
    let mut m = std::collections::BTreeMap::new();
    for a in &cache.activities {
        *m.entry(a.kind.as_str().to_string()).or_insert(0) += 1;
    }
    Ok(m)
}

/// Wipe all history.
pub fn clear() -> Result<()> {
    save(&ActivityCache::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static DIR_ID: AtomicUsize = AtomicUsize::new(0);
    /// Serializes cache tests: they mutate the process-wide XDG_CACHE_HOME,
    /// which is not safe to do concurrently.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Run the closure with a fresh XDG_CACHE_HOME so tests never touch the
    /// real user cache.
    fn with_isolated_cache(f: impl FnOnce()) {
        let _guard = TEST_LOCK.lock().unwrap();
        let id = DIR_ID.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("oci-sync-cache-test-{id}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: guarded by TEST_LOCK — no other cache test runs concurrently.
        unsafe { std::env::set_var("XDG_CACHE_HOME", &dir) };
        f();
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn mk_activity(kind: ActivityType) -> Activity {
        Activity {
            kind,
            timestamp: Local::now(),
            remote_ref: "reg/repo:tag".into(),
            local_path: None,
            labels: vec![],
            success: true,
            error: None,
        }
    }

    #[test]
    fn missing_file_is_empty_cache() {
        with_isolated_cache(|| {
            assert!(load().unwrap().activities.is_empty());
        });
    }

    #[test]
    fn add_prepends_and_truncates() {
        with_isolated_cache(|| {
            for _ in 0..(MAX_ACTIVITIES + 10) {
                add(mk_activity(ActivityType::Push)).unwrap();
            }
            assert_eq!(load().unwrap().activities.len(), MAX_ACTIVITIES);
        });
    }

    #[test]
    fn recent_limits_and_orders() {
        with_isolated_cache(|| {
            add(mk_activity(ActivityType::Push)).unwrap();
            add(mk_activity(ActivityType::Delete)).unwrap();
            let r = recent(1).unwrap();
            assert_eq!(r.len(), 1);
            assert_eq!(r[0].kind, ActivityType::Delete);
            assert_eq!(stats().unwrap()["push"], 1);
        });
    }

    #[test]
    fn clear_empties() {
        with_isolated_cache(|| {
            add(mk_activity(ActivityType::Push)).unwrap();
            clear().unwrap();
            assert!(load().unwrap().activities.is_empty());
        });
    }
}
