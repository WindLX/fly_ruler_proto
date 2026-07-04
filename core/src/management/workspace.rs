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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceTheme {
    Light,
    #[default]
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceCurveStyle {
    pub aircraft_id: String,
    pub selector: SeriesSelector,
    pub alias: String,
    pub color: String,
    pub line_pattern: String,
    pub line_width: f64,
    pub opacity: f64,
    pub visible: bool,
    pub y_axis: String,
    pub smooth: bool,
    pub show_symbol: bool,
    pub scale: f64,
    pub offset: f64,
    pub unit: Option<String>,
    pub value_format: String,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceChartView {
    pub zoom_start: Option<f64>,
    pub zoom_end: Option<f64>,
    pub zoom_start_value: Option<f64>,
    pub zoom_end_value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceChart {
    pub id: String,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub legend_visible: bool,
    pub curves: Vec<WorkspaceCurveStyle>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceSnapshot {
    pub version: u32,
    pub theme: WorkspaceTheme,
    pub locale: String,
    pub left_panel_open: bool,
    pub right_panel_open: bool,
    pub basket_open: bool,
    pub selected_aircraft_id: Option<String>,
    pub selected_chart_id: Option<String>,
    pub sync_charts: bool,
    pub query_start: Option<f64>,
    pub query_end: Option<f64>,
    pub max_points: usize,
    pub charts: Vec<WorkspaceChart>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDocument {
    pub revision: u64,
    pub updated_at_secs: f64,
    pub workspace: WorkspaceSnapshot,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("workspace document is too large")]
    TooLarge,
    #[error("workspace contains too many charts or curves")]
    TooComplex,
    #[error("workspace contains invalid numeric or enum values")]
    InvalidValue,
}

pub struct WorkspaceStore {
    path: PathBuf,
    revision: AtomicU64,
    writer: Mutex<()>,
}

impl WorkspaceStore {
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

    pub fn load(&self) -> Result<Option<WorkspaceDocument>, WorkspaceError> {
        match fs::read(&self.path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

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
