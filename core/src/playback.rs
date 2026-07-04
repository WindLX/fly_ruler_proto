//! Shared live/replay timeline state.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use serde::Serialize;
use thiserror::Error;

use crate::config::ReplayConfig;
use crate::store::{active_time_bounds, AircraftId, TimeSeriesStore, TimestampedState};

/// Current global timeline mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackMode {
    /// Resolve every aircraft at its latest received state.
    Live,
    /// Resolve every aircraft at a fixed historical cursor.
    ReplayPaused,
    /// Advance the historical cursor using wall-clock time.
    ReplayPlaying,
}

/// Serializable playback state exposed to bindings and management clients.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaybackSnapshot {
    /// Current playback mode.
    pub mode: PlaybackMode,
    /// Effective global cursor, or `None` for an empty store.
    pub cursor_secs: Option<f64>,
    /// Positive playback speed multiplier.
    pub speed: f64,
    /// Current global store time bounds.
    pub bounds: Option<(f64, f64)>,
    /// Monotonic revision incremented by explicit timeline mutations.
    pub revision: u64,
}

/// A state resolved against a playback cursor.
#[derive(Debug, Clone)]
pub struct ResolvedState {
    /// State sample selected using previous-value hold.
    pub sample: TimestampedState,
    /// Whether the aircraft lifecycle is spawned at this cursor.
    pub spawned: bool,
}

/// Playback control errors.
#[derive(Debug, Error, PartialEq)]
pub enum PlaybackError {
    /// A timeline operation requires at least one state or lifecycle event.
    #[error("the store contains no timeline data")]
    EmptyStore,
    /// A timestamp must be finite.
    #[error("timestamp must be finite")]
    InvalidTimestamp,
    /// A playback speed is outside the configured positive range.
    #[error("speed must be finite and within {min}..={max}")]
    InvalidSpeed {
        /// Minimum accepted speed.
        min: f64,
        /// Maximum accepted speed.
        max: f64,
    },
}

#[derive(Debug)]
struct PlaybackInner {
    mode: PlaybackMode,
    cursor_secs: Option<f64>,
    speed: f64,
    anchor: Option<Instant>,
    revision: u64,
}

/// Thread-safe global playback state shared by HTTP, WebSocket, and renderers.
pub struct PlaybackController {
    store: Arc<TimeSeriesStore>,
    config: ReplayConfig,
    inner: Mutex<PlaybackInner>,
}

impl PlaybackController {
    /// Create a live playback controller over the given store.
    pub fn new(store: Arc<TimeSeriesStore>, config: ReplayConfig) -> Self {
        Self {
            store,
            inner: Mutex::new(PlaybackInner {
                mode: PlaybackMode::Live,
                cursor_secs: None,
                speed: config.default_speed,
                anchor: None,
                revision: 0,
            }),
            config,
        }
    }

    /// Return the effective current playback state.
    pub fn snapshot(&self) -> PlaybackSnapshot {
        let bounds = active_time_bounds(&self.store);
        let mut inner = lock_unpoisoned(&self.inner);
        update_effective_cursor(&mut inner, bounds);
        snapshot_from_inner(&inner, bounds)
    }

    /// Switch to live mode.
    pub fn live(&self) -> PlaybackSnapshot {
        let bounds = active_time_bounds(&self.store);
        let mut inner = lock_unpoisoned(&self.inner);
        inner.mode = PlaybackMode::Live;
        inner.cursor_secs = bounds.map(|bounds| bounds.1);
        inner.anchor = None;
        inner.revision = inner.revision.wrapping_add(1);
        snapshot_from_inner(&inner, bounds)
    }

    /// Pause at the effective current cursor.
    pub fn pause(&self) -> Result<PlaybackSnapshot, PlaybackError> {
        let bounds = active_time_bounds(&self.store).ok_or(PlaybackError::EmptyStore)?;
        let mut inner = lock_unpoisoned(&self.inner);
        update_effective_cursor(&mut inner, Some(bounds));
        inner.mode = PlaybackMode::ReplayPaused;
        inner.cursor_secs = Some(inner.cursor_secs.unwrap_or(bounds.1));
        inner.anchor = None;
        inner.revision = inner.revision.wrapping_add(1);
        Ok(snapshot_from_inner(&inner, Some(bounds)))
    }

    /// Seek to a clamped historical timestamp and pause.
    pub fn seek(&self, timestamp_secs: f64) -> Result<PlaybackSnapshot, PlaybackError> {
        if !timestamp_secs.is_finite() {
            return Err(PlaybackError::InvalidTimestamp);
        }
        let bounds = active_time_bounds(&self.store).ok_or(PlaybackError::EmptyStore)?;
        let mut inner = lock_unpoisoned(&self.inner);
        inner.mode = PlaybackMode::ReplayPaused;
        inner.cursor_secs = Some(timestamp_secs.clamp(bounds.0, bounds.1));
        inner.anchor = None;
        inner.revision = inner.revision.wrapping_add(1);
        Ok(snapshot_from_inner(&inner, Some(bounds)))
    }

    /// Start forward playback at the current cursor.
    pub fn play(&self, speed: Option<f64>) -> Result<PlaybackSnapshot, PlaybackError> {
        if let Some(speed) = speed {
            validate_speed(&self.config, speed)?;
        }
        let bounds = active_time_bounds(&self.store).ok_or(PlaybackError::EmptyStore)?;
        let mut inner = lock_unpoisoned(&self.inner);
        update_effective_cursor(&mut inner, Some(bounds));
        if let Some(speed) = speed {
            inner.speed = speed;
        }
        inner.cursor_secs = Some(
            inner
                .cursor_secs
                .unwrap_or(bounds.0)
                .clamp(bounds.0, bounds.1),
        );
        inner.mode = PlaybackMode::ReplayPlaying;
        inner.anchor = Some(Instant::now());
        inner.revision = inner.revision.wrapping_add(1);
        update_effective_cursor(&mut inner, Some(bounds));
        Ok(snapshot_from_inner(&inner, Some(bounds)))
    }

    /// Set the speed while preserving the effective cursor.
    pub fn set_speed(&self, speed: f64) -> Result<PlaybackSnapshot, PlaybackError> {
        validate_speed(&self.config, speed)?;
        let bounds = active_time_bounds(&self.store);
        let mut inner = lock_unpoisoned(&self.inner);
        update_effective_cursor(&mut inner, bounds);
        inner.speed = speed;
        if inner.mode == PlaybackMode::ReplayPlaying {
            inner.anchor = Some(Instant::now());
        }
        inner.revision = inner.revision.wrapping_add(1);
        Ok(snapshot_from_inner(&inner, bounds))
    }

    /// Reset after clearing all data.
    pub fn reset_empty(&self) -> PlaybackSnapshot {
        let mut inner = lock_unpoisoned(&self.inner);
        inner.mode = PlaybackMode::ReplayPaused;
        inner.cursor_secs = None;
        inner.anchor = None;
        inner.revision = inner.revision.wrapping_add(1);
        snapshot_from_inner(&inner, None)
    }

    /// Reset after loading data, paused at the first global sample.
    pub fn reset_after_load(&self) -> PlaybackSnapshot {
        let bounds = active_time_bounds(&self.store);
        let mut inner = lock_unpoisoned(&self.inner);
        inner.mode = PlaybackMode::ReplayPaused;
        inner.cursor_secs = bounds.map(|bounds| bounds.0);
        inner.anchor = None;
        inner.revision = inner.revision.wrapping_add(1);
        snapshot_from_inner(&inner, bounds)
    }

    /// Resolve one aircraft according to the effective global mode.
    pub fn resolve_aircraft(&self, id: &AircraftId) -> Option<ResolvedState> {
        let playback = self.snapshot();
        self.resolve_aircraft_with(&playback, id)
    }

    /// Resolve one aircraft against an already captured playback snapshot.
    ///
    /// This keeps multi-aircraft WebSocket frames and renderer updates on one
    /// exact global cursor.
    pub fn resolve_aircraft_with(
        &self,
        playback: &PlaybackSnapshot,
        id: &AircraftId,
    ) -> Option<ResolvedState> {
        let sample = match playback.mode {
            PlaybackMode::Live => self.store.get_latest(id),
            PlaybackMode::ReplayPaused | PlaybackMode::ReplayPlaying => playback
                .cursor_secs
                .and_then(|cursor| self.store.get_state_at_or_before(id, cursor)),
        }?;
        let spawned = match playback.mode {
            PlaybackMode::Live => self.store.is_spawned_at(id, f64::INFINITY),
            PlaybackMode::ReplayPaused | PlaybackMode::ReplayPlaying => playback
                .cursor_secs
                .is_some_and(|cursor| self.store.is_spawned_at(id, cursor)),
        };
        Some(ResolvedState { sample, spawned })
    }

    /// Return the underlying store.
    pub fn store(&self) -> Arc<TimeSeriesStore> {
        Arc::clone(&self.store)
    }
}

fn validate_speed(config: &ReplayConfig, speed: f64) -> Result<(), PlaybackError> {
    if speed.is_finite() && (config.min_speed..=config.max_speed).contains(&speed) {
        Ok(())
    } else {
        Err(PlaybackError::InvalidSpeed {
            min: config.min_speed,
            max: config.max_speed,
        })
    }
}

fn update_effective_cursor(inner: &mut PlaybackInner, bounds: Option<(f64, f64)>) {
    let Some((start, end)) = bounds else {
        inner.cursor_secs = None;
        inner.anchor = None;
        if inner.mode == PlaybackMode::Live {
            return;
        }
        inner.mode = PlaybackMode::ReplayPaused;
        return;
    };

    match inner.mode {
        PlaybackMode::Live => {
            inner.cursor_secs = Some(end);
            inner.anchor = None;
        }
        PlaybackMode::ReplayPaused => {
            inner.cursor_secs = Some(inner.cursor_secs.unwrap_or(start).clamp(start, end));
            inner.anchor = None;
        }
        PlaybackMode::ReplayPlaying => {
            let base = inner.cursor_secs.unwrap_or(start);
            let elapsed = inner
                .anchor
                .map_or(0.0, |anchor| anchor.elapsed().as_secs_f64());
            let next = base + elapsed * inner.speed;
            if next >= end {
                inner.cursor_secs = Some(end);
                inner.mode = PlaybackMode::ReplayPaused;
                inner.anchor = None;
                inner.revision = inner.revision.wrapping_add(1);
            } else {
                inner.cursor_secs = Some(next.max(start));
                inner.anchor = Some(Instant::now());
            }
        }
    }
}

fn snapshot_from_inner(inner: &PlaybackInner, bounds: Option<(f64, f64)>) -> PlaybackSnapshot {
    PlaybackSnapshot {
        mode: inner.mode,
        cursor_secs: inner.cursor_secs,
        speed: inner.speed,
        bounds,
        revision: inner.revision,
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
    use crate::pb;
    use std::time::Duration;

    fn state(x: f64) -> pb::AircraftState {
        pb::AircraftState {
            position: Some(pb::Vector3 { x, y: 0.0, z: 0.0 }),
            ..Default::default()
        }
    }

    fn controller() -> PlaybackController {
        let store = Arc::new(TimeSeriesStore::new());
        store.append_state("a".to_string(), 10.0, state(1.0));
        store.append_state("a".to_string(), 20.0, state(2.0));
        PlaybackController::new(store, ReplayConfig::default())
    }

    #[test]
    fn live_pause_seek_and_live_transitions() {
        let playback = controller();
        assert_eq!(playback.snapshot().cursor_secs, Some(20.0));
        assert_eq!(playback.pause().unwrap().mode, PlaybackMode::ReplayPaused);
        assert_eq!(playback.seek(15.0).unwrap().cursor_secs, Some(15.0));
        assert_eq!(
            playback
                .resolve_aircraft(&"a".to_string())
                .unwrap()
                .sample
                .timestamp_secs,
            10.0
        );
        assert_eq!(playback.live().mode, PlaybackMode::Live);
    }

    #[test]
    fn invalid_speed_and_empty_store_are_rejected() {
        let empty =
            PlaybackController::new(Arc::new(TimeSeriesStore::new()), ReplayConfig::default());
        assert_eq!(empty.pause().unwrap_err(), PlaybackError::EmptyStore);
        assert!(matches!(
            controller().set_speed(0.0),
            Err(PlaybackError::InvalidSpeed { .. })
        ));
    }

    #[test]
    fn resets_increment_revision() {
        let playback = controller();
        let initial = playback.snapshot().revision;
        assert!(playback.reset_empty().revision > initial);
        assert!(playback.reset_after_load().revision > initial);
    }

    #[test]
    fn playing_advances_and_pauses_at_the_end() {
        let playback = controller();
        playback.seek(19.99).unwrap();
        let started = playback.play(Some(16.0)).unwrap();
        assert_eq!(started.mode, PlaybackMode::ReplayPlaying);
        std::thread::sleep(Duration::from_millis(2));
        let ended = playback.snapshot();
        assert_eq!(ended.mode, PlaybackMode::ReplayPaused);
        assert_eq!(ended.cursor_secs, Some(20.0));
        assert!(ended.revision > started.revision);
    }

    #[test]
    fn replay_does_not_block_live_ingestion_and_live_jumps_to_latest() {
        let playback = controller();
        playback.seek(10.0).unwrap();
        let store = playback.store();
        store.append_state("a".to_string(), 30.0, state(3.0));
        assert_eq!(playback.snapshot().cursor_secs, Some(10.0));
        assert_eq!(
            playback
                .resolve_aircraft(&"a".to_string())
                .unwrap()
                .sample
                .timestamp_secs,
            10.0
        );
        assert_eq!(playback.live().cursor_secs, Some(30.0));
        assert_eq!(
            playback
                .resolve_aircraft(&"a".to_string())
                .unwrap()
                .sample
                .timestamp_secs,
            30.0
        );
    }
}
