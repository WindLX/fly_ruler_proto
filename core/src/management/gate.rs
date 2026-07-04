use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, RwLock};

/// Gate used to briefly pause high-frequency ingestion during maintenance.
pub struct IngestionGate {
    enabled: AtomicBool,
    dropped: AtomicU64,
    maintenance: Mutex<()>,
    activity: RwLock<()>,
}

impl Default for IngestionGate {
    fn default() -> Self {
        Self::new()
    }
}

impl IngestionGate {
    /// Create an enabled ingestion gate.
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(true),
            dropped: AtomicU64::new(0),
            maintenance: Mutex::new(()),
            activity: RwLock::new(()),
        }
    }

    /// Run one append while holding a shared ingestion permit.
    pub fn with_ingestion<T>(&self, action: impl FnOnce() -> T) -> Option<T> {
        if !self.enabled.load(Ordering::Acquire) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let activity = self
            .activity
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !self.enabled.load(Ordering::Acquire) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let result = action();
        drop(activity);
        Some(result)
    }

    /// Execute a short critical section with ingestion disabled.
    pub fn with_paused<T>(&self, action: impl FnOnce() -> T) -> T {
        let lock = lock_unpoisoned(&self.maintenance);
        self.enabled.store(false, Ordering::Release);
        let activity = self
            .activity
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let guard = ResumeIngestion {
            gate: self,
            lock,
            activity,
        };
        let result = action();
        drop(guard);
        result
    }

    /// Return whether ingestion is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Return the number of datagrams dropped during maintenance windows.
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

struct ResumeIngestion<'a> {
    gate: &'a IngestionGate,
    #[allow(dead_code)]
    lock: MutexGuard<'a, ()>,
    #[allow(dead_code)]
    activity: std::sync::RwLockWriteGuard<'a, ()>,
}

impl Drop for ResumeIngestion<'_> {
    fn drop(&mut self) {
        self.gate.enabled.store(true, Ordering::Release);
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_dropped_messages() {
        let gate = IngestionGate::new();
        gate.with_paused(|| assert!(gate.with_ingestion(|| ()).is_none()));
        assert!(gate.with_ingestion(|| ()).is_some());
        assert_eq!(gate.dropped_count(), 1);
    }
}
