use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use super::series::SeriesSelector;
use crate::utils::now_secs;

const MAX_WORKSPACE_BYTES: usize = 1024 * 1024;
const MAX_CHARTS: usize = 64;
const MAX_CURVES_PER_CHART: usize = 64;

/// Color theme for the management console.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceTheme {
    /// Light color scheme.
    #[default]
    Light,
    /// Dark color scheme.
    Dark,
}

/// Visual styling for one curve inside a chart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceCurveStyle {
    /// Aircraft that owns the series.
    pub aircraft_id: String,
    /// Field selector for the series.
    pub selector: SeriesSelector,
    /// Display name override; empty means use the catalog label.
    pub alias: String,
    /// Hex color such as `#4b92f0`.
    pub color: String,
    /// Line style: `solid`, `dashed`, or `dotted`.
    pub line_pattern: String,
    /// Line width in pixels.
    pub line_width: f64,
    /// Opacity from 0.0 to 1.0.
    pub opacity: f64,
    /// Whether the curve is currently visible.
    pub visible: bool,
    /// Y-axis side: `left` or `right`.
    pub y_axis: String,
    /// Whether to smooth the line.
    pub smooth: bool,
    /// Whether to show symbols at each data point.
    pub show_symbol: bool,
    /// Scale factor applied to raw values.
    pub scale: f64,
    /// Offset applied after scaling.
    pub offset: f64,
    /// Optional unit override; empty means use the catalog unit.
    pub unit: Option<String>,
    /// Number format: `auto`, `fixed`, or `scientific`.
    pub value_format: String,
    /// Decimal precision for value labels.
    pub precision: u8,
}

impl Default for WorkspaceCurveStyle {
    fn default() -> Self {
        Self {
            aircraft_id: String::new(),
            selector: SeriesSelector::Standard {
                path: "derived.altitude".to_string(),
            },
            alias: String::new(),
            color: "#4b92f0".to_string(),
            line_pattern: "solid".to_string(),
            line_width: 2.0,
            opacity: 1.0,
            visible: true,
            y_axis: "left".to_string(),
            smooth: false,
            show_symbol: false,
            scale: 1.0,
            offset: 0.0,
            unit: None,
            value_format: "auto".to_string(),
            precision: 3,
        }
    }
}

/// Zoom and viewport state for a single chart.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceChartView {
    /// Optional start timestamp ratio for zoom (0.0..=1.0).
    pub zoom_start: Option<f64>,
    /// Optional end timestamp ratio for zoom (0.0..=1.0).
    pub zoom_end: Option<f64>,
    /// Optional fixed start timestamp for zoom.
    pub zoom_start_value: Option<f64>,
    /// Optional fixed end timestamp for zoom.
    pub zoom_end_value: Option<f64>,
}

/// One chart tile in the workspace dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceChart {
    /// Stable chart identifier.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Grid x position.
    pub x: i32,
    /// Grid y position.
    pub y: i32,
    /// Grid width in columns.
    pub w: u32,
    /// Grid height in rows.
    pub h: u32,
    /// Whether the chart legend is visible.
    pub legend_visible: bool,
    /// Curves drawn on this chart.
    pub curves: Vec<WorkspaceCurveStyle>,
    /// Viewport state.
    pub view: WorkspaceChartView,
}

impl Default for WorkspaceChart {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            x: 0,
            y: 0,
            w: 6,
            h: 5,
            legend_visible: true,
            curves: Vec::new(),
            view: WorkspaceChartView::default(),
        }
    }
}

/// Full workspace snapshot saved by the management console.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceSnapshot {
    /// Schema version; currently 1.
    pub version: u32,
    /// Active color theme.
    pub theme: WorkspaceTheme,
    /// UI locale such as `zh-CN`.
    pub locale: String,
    /// Whether the left panel is open.
    pub left_panel_open: bool,
    /// Whether the right panel is open.
    pub right_panel_open: bool,
    /// Whether the curve basket is open.
    pub basket_open: bool,
    /// Currently selected aircraft, if any.
    pub selected_aircraft_id: Option<String>,
    /// Currently selected chart, if any.
    pub selected_chart_id: Option<String>,
    /// Whether charts share a synchronized time axis.
    pub sync_charts: bool,
    /// Optional global query start timestamp.
    pub query_start: Option<f64>,
    /// Optional global query end timestamp.
    pub query_end: Option<f64>,
    /// Default down-sampling target for series queries.
    pub max_points: usize,
    /// Dashboard charts.
    pub charts: Vec<WorkspaceChart>,
    /// Curves held in the basket for quick add.
    pub basket: Vec<WorkspaceCurveStyle>,
}

impl Default for WorkspaceSnapshot {
    fn default() -> Self {
        Self {
            version: 1,
            theme: WorkspaceTheme::Dark,
            locale: "zh-CN".to_string(),
            left_panel_open: true,
            right_panel_open: true,
            basket_open: true,
            selected_aircraft_id: None,
            selected_chart_id: None,
            sync_charts: true,
            query_start: None,
            query_end: None,
            max_points: 2_000,
            charts: Vec::new(),
            basket: Vec::new(),
        }
    }
}

/// Persisted workspace document including revision metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDocument {
    /// Monotonic revision counter incremented on each save.
    pub revision: u64,
    /// Last modification timestamp in seconds.
    pub updated_at_secs: f64,
    /// Saved workspace snapshot.
    pub workspace: WorkspaceSnapshot,
}

/// Errors that can occur while loading or saving a workspace.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    /// Filesystem I/O failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization or deserialization failure.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// The workspace document exceeds the maximum size.
    #[error("workspace document is too large")]
    TooLarge,
    /// The workspace contains too many charts or curves.
    #[error("workspace contains too many charts or curves")]
    TooComplex,
    /// The workspace contains invalid numeric or enum values.
    #[error("workspace contains invalid numeric or enum values")]
    InvalidValue,
}

/// Atomic file-backed store for a single workspace snapshot.
pub struct WorkspaceStore {
    path: PathBuf,
    revision: AtomicU64,
    writer: Mutex<()>,
}

impl WorkspaceStore {
    /// Create a new store that persists under `{data_root}/.fly-ruler/workspace.json`.
    pub fn new(data_root: &Path) -> Self {
        let path = data_root.join(".fly-ruler").join("workspace.json");
        let revision = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<WorkspaceDocument>(&bytes).ok())
            .map_or(0, |document| document.revision);
        Self {
            path,
            revision: AtomicU64::new(revision),
            writer: Mutex::new(()),
        }
    }

    /// Load the persisted workspace document, if one exists.
    pub fn load(&self) -> Result<Option<WorkspaceDocument>, WorkspaceError> {
        match fs::read(&self.path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Validate and atomically save a workspace snapshot.
    pub fn save(&self, workspace: WorkspaceSnapshot) -> Result<WorkspaceDocument, WorkspaceError> {
        validate(&workspace)?;
        let _writer = lock_unpoisoned(&self.writer);
        let document = WorkspaceDocument {
            revision: self.revision.fetch_add(1, Ordering::AcqRel) + 1,
            updated_at_secs: now_secs(),
            workspace,
        };
        let bytes = serde_json::to_vec_pretty(&document)?;
        if bytes.len() > MAX_WORKSPACE_BYTES {
            return Err(WorkspaceError::TooLarge);
        }
        let parent = self.path.parent().expect("workspace path has parent");
        fs::create_dir_all(parent)?;
        let temporary = parent.join("workspace.json.tmp");
        fs::write(&temporary, &bytes)?;
        if self.path.exists() {
            let backup = parent.join("workspace.json.bak");
            let _ = fs::remove_file(&backup);
            fs::rename(&self.path, &backup)?;
            if let Err(error) = fs::rename(&temporary, &self.path) {
                let _ = fs::rename(&backup, &self.path);
                return Err(error.into());
            }
            let _ = fs::remove_file(backup);
        } else {
            fs::rename(&temporary, &self.path)?;
        }
        Ok(document)
    }
}

fn validate(workspace: &WorkspaceSnapshot) -> Result<(), WorkspaceError> {
    if workspace.version != 1
        || !(100..=20_000).contains(&workspace.max_points)
        || workspace.charts.len() > MAX_CHARTS
        || workspace
            .charts
            .iter()
            .any(|chart| chart.curves.len() > MAX_CURVES_PER_CHART)
    {
        return Err(WorkspaceError::TooComplex);
    }
    if workspace
        .query_start
        .is_some_and(|value| !value.is_finite())
        || workspace.query_end.is_some_and(|value| !value.is_finite())
        || workspace
            .query_start
            .zip(workspace.query_end)
            .is_some_and(|(start, end)| start > end)
    {
        return Err(WorkspaceError::InvalidValue);
    }
    for curve in workspace
        .charts
        .iter()
        .flat_map(|chart| &chart.curves)
        .chain(&workspace.basket)
    {
        if !curve.line_width.is_finite()
            || !(0.5..=8.0).contains(&curve.line_width)
            || !curve.opacity.is_finite()
            || !(0.0..=1.0).contains(&curve.opacity)
            || !curve.scale.is_finite()
            || !curve.offset.is_finite()
            || curve.precision > 8
            || !matches!(curve.line_pattern.as_str(), "solid" | "dashed" | "dotted")
            || !matches!(curve.y_axis.as_str(), "left" | "right")
            || !matches!(curve.value_format.as_str(), "auto" | "fixed" | "scientific")
        {
            return Err(WorkspaceError::InvalidValue);
        }
    }
    Ok(())
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
    fn validates_default_workspace() {
        validate(&WorkspaceSnapshot::default()).unwrap();
    }

    #[test]
    fn rejects_invalid_max_points() {
        let workspace = WorkspaceSnapshot {
            max_points: 1,
            ..WorkspaceSnapshot::default()
        };
        assert!(validate(&workspace).is_err());
    }

    #[test]
    fn save_load_increments_revision_and_rejects_corruption() {
        let root = std::env::temp_dir().join(format!(
            "fly-ruler-workspace-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let store = WorkspaceStore::new(&root);
        assert!(store.load().unwrap().is_none());

        let first = store.save(WorkspaceSnapshot::default()).unwrap();
        let second = store.save(WorkspaceSnapshot::default()).unwrap();
        assert_eq!(first.revision, 1);
        assert_eq!(second.revision, 2);
        assert_eq!(store.load().unwrap().unwrap().revision, 2);

        fs::write(&store.path, b"{broken").unwrap();
        assert!(matches!(store.load(), Err(WorkspaceError::Json(_))));
        let _ = fs::remove_dir_all(root);
    }
}
