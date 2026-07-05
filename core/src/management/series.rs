use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::pb;
use crate::store::{AircraftId, TimeSeriesStore};

const DEFAULT_MAX_POINTS: usize = 2_000;
const MIN_MAX_POINTS: usize = 100;
const MAX_MAX_POINTS: usize = 20_000;
const MAX_SELECTIONS: usize = 64;

/// Selector identifying a single time-series source for an aircraft.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SeriesSelector {
    /// A standard aircraft state field path such as `derived.altitude`.
    Standard {
        /// Dot-separated path like `derived.altitude`.
        path: String,
    },
    /// Throttle lever ratio for the engine at the given index.
    EngineThrottle {
        /// Engine index starting at 1.
        index: u32,
    },
    /// A user-defined numeric custom field.
    Custom {
        /// User-defined field identifier.
        field_id: String,
    },
}

impl SeriesSelector {
    /// Return a stable key string for this selector.
    pub fn key(&self) -> String {
        match self {
            Self::Standard { path } => format!("standard:{path}"),
            Self::EngineThrottle { index } => format!("engine_throttle:{index}"),
            Self::Custom { field_id } => format!("custom:{field_id}"),
        }
    }
}

/// Metadata entry returned by the series catalog for one selectable field.
#[derive(Debug, Clone, Serialize)]
pub struct SeriesCatalogItem {
    /// Field selector.
    pub selector: SeriesSelector,
    /// Display group used to organize fields in the UI.
    pub group: String,
    /// Human-readable label.
    pub label: String,
    /// Optional unit suffix.
    pub unit: Option<String>,
}

/// One selected series as part of a query request.
#[derive(Debug, Clone, Deserialize)]
pub struct SeriesSelection {
    /// Aircraft that owns the series.
    pub aircraft_id: String,
    /// Selected field.
    pub selector: SeriesSelector,
}

/// Time window for a series query.
#[derive(Debug, Clone, Deserialize)]
pub struct SeriesTimeRange {
    /// Inclusive start timestamp in seconds.
    pub start: f64,
    /// Inclusive end timestamp in seconds.
    pub end: f64,
}

/// Request body for `/api/v1/series/query`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesQueryRequest {
    /// Selected series to fetch.
    pub selections: Vec<SeriesSelection>,
    /// Optional time window; defaults to all available data.
    pub time_range: Option<SeriesTimeRange>,
    /// Optional down-sampling target; defaults to `DEFAULT_MAX_POINTS`.
    pub max_points: Option<usize>,
}

/// Simple statistics computed over a sampled series.
#[derive(Debug, Clone, Serialize)]
pub struct SeriesStats {
    /// Minimum y-value.
    pub min: f64,
    /// Maximum y-value.
    pub max: f64,
    /// Last y-value.
    pub last: f64,
    /// First x-value (timestamp).
    pub start: f64,
    /// Last x-value (timestamp).
    pub end: f64,
}

/// One series result returned by a query.
#[derive(Debug, Clone, Serialize)]
pub struct SeriesData {
    /// Composite key `{aircraft_id}::{selector_key}`.
    pub key: String,
    /// Aircraft that owns the series.
    pub aircraft_id: String,
    /// Field selector.
    pub selector: SeriesSelector,
    /// Sampled `[timestamp_secs, value]` points.
    pub points: Vec<[f64; 2]>,
    /// Number of points in the raw time window.
    pub total_points: usize,
    /// Number of points after down-sampling.
    pub returned_points: usize,
    /// Statistics over the raw points, if any exist.
    pub stats: Option<SeriesStats>,
}

/// Response body for `/api/v1/series/query`.
#[derive(Debug, Clone, Serialize)]
pub struct SeriesQueryResponse {
    /// Result series in the same order as the request selections.
    pub series: Vec<SeriesData>,
}

/// Errors that can occur while building a series catalog or query.
#[derive(Debug, thiserror::Error)]
pub enum SeriesError {
    /// The requested aircraft does not exist in the store.
    #[error("aircraft not found: {0}")]
    AircraftNotFound(String),
    /// The request must contain between 1 and `MAX_SELECTIONS` selections.
    #[error("selections must contain between 1 and {MAX_SELECTIONS} entries")]
    InvalidSelectionCount,
    /// `max_points` is outside the allowed range.
    #[error("max_points must be within {MIN_MAX_POINTS}..={MAX_MAX_POINTS}")]
    InvalidMaxPoints,
    /// The requested time range is not finite or is inverted.
    #[error("time range must be finite and start <= end")]
    InvalidTimeRange,
    /// A standard field path is not recognized.
    #[error("unknown standard field: {0}")]
    UnknownStandardField(String),
}

/// Build the catalog of selectable series for an aircraft.
pub fn catalog(
    store: &TimeSeriesStore,
    aircraft_id: &str,
) -> Result<Vec<SeriesCatalogItem>, SeriesError> {
    let id = aircraft_id.to_string();
    let mut selectors = BTreeSet::new();
    let found = store.visit_states_range(&id, f64::NEG_INFINITY, f64::INFINITY, |states| {
        for sample in states {
            collect_state_selectors(&sample.state, &mut selectors);
        }
    });
    if found.is_none() {
        return Err(SeriesError::AircraftNotFound(aircraft_id.to_string()));
    }
    Ok(selectors.into_iter().map(catalog_item).collect())
}

/// Query sampled series data for the selections in `request`.
pub fn query(
    store: &TimeSeriesStore,
    request: SeriesQueryRequest,
) -> Result<SeriesQueryResponse, SeriesError> {
    if request.selections.is_empty() || request.selections.len() > MAX_SELECTIONS {
        return Err(SeriesError::InvalidSelectionCount);
    }
    let max_points = request.max_points.unwrap_or(DEFAULT_MAX_POINTS);
    if !(MIN_MAX_POINTS..=MAX_MAX_POINTS).contains(&max_points) {
        return Err(SeriesError::InvalidMaxPoints);
    }
    let (start, end) = request
        .time_range
        .map(|range| (range.start, range.end))
        .unwrap_or((f64::NEG_INFINITY, f64::INFINITY));
    if start.is_nan() || end.is_nan() || start > end {
        return Err(SeriesError::InvalidTimeRange);
    }
    for selection in &request.selections {
        validate_selector(&selection.selector)?;
    }

    let mut grouped: BTreeMap<AircraftId, Vec<(usize, SeriesSelector)>> = BTreeMap::new();
    for (index, selection) in request.selections.iter().enumerate() {
        grouped
            .entry(selection.aircraft_id.clone())
            .or_default()
            .push((index, selection.selector.clone()));
    }
    let mut output: Vec<Option<SeriesData>> = vec![None; request.selections.len()];

    for (aircraft_id, selections) in grouped {
        let mut points = vec![Vec::<[f64; 2]>::new(); selections.len()];
        let found = store.visit_states_range(&aircraft_id, start, end, |states| {
            for sample in states {
                for (slot, (_, selector)) in selections.iter().enumerate() {
                    if let Some(value) = extract_value(&sample.state, selector) {
                        if sample.timestamp_secs.is_finite() && value.is_finite() {
                            points[slot].push([sample.timestamp_secs, value]);
                        }
                    }
                }
            }
        });
        if found.is_none() {
            return Err(SeriesError::AircraftNotFound(aircraft_id));
        }
        for (slot, (output_index, selector)) in selections.into_iter().enumerate() {
            let full = &points[slot];
            let stats = stats(full);
            let sampled = lttb(full, max_points);
            output[output_index] = Some(SeriesData {
                key: format!("{aircraft_id}::{}", selector.key()),
                aircraft_id: aircraft_id.clone(),
                selector,
                total_points: full.len(),
                returned_points: sampled.len(),
                points: sampled,
                stats,
            });
        }
    }

    Ok(SeriesQueryResponse {
        series: output.into_iter().flatten().collect(),
    })
}

fn standard_fields() -> &'static [(
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
)] {
    &[
        ("position.x", "position", "Position X", Some("m")),
        ("position.y", "position", "Position Y", Some("m")),
        ("position.z", "position", "Position Z", Some("m")),
        ("velocity.x", "velocity", "Velocity X", Some("m/s")),
        ("velocity.y", "velocity", "Velocity Y", Some("m/s")),
        ("velocity.z", "velocity", "Velocity Z", Some("m/s")),
        ("attitude.w", "attitude", "Quaternion W", None),
        ("attitude.x", "attitude", "Quaternion X", None),
        ("attitude.y", "attitude", "Quaternion Y", None),
        ("attitude.z", "attitude", "Quaternion Z", None),
        (
            "angular_velocity.x",
            "angular_velocity",
            "Roll rate",
            Some("rad/s"),
        ),
        (
            "angular_velocity.y",
            "angular_velocity",
            "Pitch rate",
            Some("rad/s"),
        ),
        (
            "angular_velocity.z",
            "angular_velocity",
            "Yaw rate",
            Some("rad/s"),
        ),
        ("derived.lat", "derived", "Latitude", Some("deg")),
        ("derived.lon", "derived", "Longitude", Some("deg")),
        ("derived.altitude", "derived", "Altitude", Some("m")),
        ("derived.alpha", "derived", "Angle of attack", Some("rad")),
        ("derived.beta", "derived", "Sideslip angle", Some("rad")),
        ("derived.tas", "derived", "True airspeed", Some("m/s")),
        ("derived.eas", "derived", "Equivalent airspeed", Some("m/s")),
        ("derived.gamma", "derived", "Flight path angle", Some("rad")),
        ("derived.chi", "derived", "Track angle", Some("rad")),
        ("derived.ias", "derived", "Indicated airspeed", Some("m/s")),
        ("derived.cas", "derived", "Calibrated airspeed", Some("m/s")),
        ("derived.mach", "derived", "Mach", None),
        (
            "control_surfaces.aileron_left_rad",
            "control_surfaces",
            "Left aileron",
            Some("rad"),
        ),
        (
            "control_surfaces.aileron_right_rad",
            "control_surfaces",
            "Right aileron",
            Some("rad"),
        ),
        (
            "control_surfaces.elevator_rad",
            "control_surfaces",
            "Elevator",
            Some("rad"),
        ),
        (
            "control_surfaces.rudder_rad",
            "control_surfaces",
            "Rudder",
            Some("rad"),
        ),
        (
            "control_surfaces.flaps_left_ratio",
            "control_surfaces",
            "Left flaps",
            Some("ratio"),
        ),
        (
            "control_surfaces.flaps_right_ratio",
            "control_surfaces",
            "Right flaps",
            Some("ratio"),
        ),
        (
            "control_surfaces.spoilers_ratio",
            "control_surfaces",
            "Spoilers",
            Some("ratio"),
        ),
    ]
}

fn collect_state_selectors(state: &pb::AircraftState, output: &mut BTreeSet<SeriesSelector>) {
    for (path, _, _, _) in standard_fields() {
        let selector = SeriesSelector::Standard {
            path: (*path).to_string(),
        };
        if extract_value(state, &selector).is_some() {
            output.insert(selector);
        }
    }
    for engine in &state.engines {
        if engine.throttle_lever_ratio.is_some() {
            output.insert(SeriesSelector::EngineThrottle {
                index: engine.index,
            });
        }
    }
    for field in &state.custom_fields {
        if custom_numeric(field).is_some() {
            output.insert(SeriesSelector::Custom {
                field_id: field.field_id.clone(),
            });
        }
    }
}

fn catalog_item(selector: SeriesSelector) -> SeriesCatalogItem {
    match &selector {
        SeriesSelector::Standard { path } => {
            let (_, group, label, unit) = standard_fields()
                .iter()
                .find(|(candidate, _, _, _)| candidate == path)
                .expect("catalog standard selector is validated");
            SeriesCatalogItem {
                selector,
                group: (*group).to_string(),
                label: (*label).to_string(),
                unit: unit.map(str::to_string),
            }
        }
        SeriesSelector::EngineThrottle { index } => SeriesCatalogItem {
            selector: selector.clone(),
            group: "engines".to_string(),
            label: format!("Engine {index} throttle"),
            unit: Some("ratio".to_string()),
        },
        SeriesSelector::Custom { field_id } => SeriesCatalogItem {
            selector: selector.clone(),
            group: "custom".to_string(),
            label: field_id.clone(),
            unit: None,
        },
    }
}

fn validate_selector(selector: &SeriesSelector) -> Result<(), SeriesError> {
    if let SeriesSelector::Standard { path } = selector {
        if !standard_fields()
            .iter()
            .any(|(candidate, _, _, _)| candidate == path)
        {
            return Err(SeriesError::UnknownStandardField(path.clone()));
        }
    }
    Ok(())
}

fn extract_value(state: &pb::AircraftState, selector: &SeriesSelector) -> Option<f64> {
    match selector {
        SeriesSelector::Standard { path } => standard_value(state, path),
        SeriesSelector::EngineThrottle { index } => {
            state
                .engines
                .iter()
                .rev()
                .find(|engine| engine.index == *index)?
                .throttle_lever_ratio
        }
        SeriesSelector::Custom { field_id } => state
            .custom_fields
            .iter()
            .rev()
            .find(|field| field.field_id == *field_id)
            .and_then(custom_numeric),
    }
}

fn standard_value(state: &pb::AircraftState, path: &str) -> Option<f64> {
    match path {
        "position.x" => state.position.as_ref().map(|value| value.x),
        "position.y" => state.position.as_ref().map(|value| value.y),
        "position.z" => state.position.as_ref().map(|value| value.z),
        "velocity.x" => state.velocity.as_ref().map(|value| value.x),
        "velocity.y" => state.velocity.as_ref().map(|value| value.y),
        "velocity.z" => state.velocity.as_ref().map(|value| value.z),
        "attitude.w" => state.attitude.as_ref().map(|value| value.w),
        "attitude.x" => state.attitude.as_ref().map(|value| value.x),
        "attitude.y" => state.attitude.as_ref().map(|value| value.y),
        "attitude.z" => state.attitude.as_ref().map(|value| value.z),
        "angular_velocity.x" => state.angular_velocity.as_ref().map(|value| value.x),
        "angular_velocity.y" => state.angular_velocity.as_ref().map(|value| value.y),
        "angular_velocity.z" => state.angular_velocity.as_ref().map(|value| value.z),
        "derived.lat" => state.derived.as_ref().map(|value| value.lat),
        "derived.lon" => state.derived.as_ref().map(|value| value.lon),
        "derived.altitude" => state.derived.as_ref().map(|value| value.altitude),
        "derived.alpha" => state.derived.as_ref().map(|value| value.alpha),
        "derived.beta" => state.derived.as_ref().map(|value| value.beta),
        "derived.tas" => state.derived.as_ref().map(|value| value.tas),
        "derived.eas" => state.derived.as_ref().map(|value| value.eas),
        "derived.gamma" => state.derived.as_ref().map(|value| value.gamma),
        "derived.chi" => state.derived.as_ref().map(|value| value.chi),
        "derived.ias" => state.derived.as_ref()?.ias,
        "derived.cas" => state.derived.as_ref()?.cas,
        "derived.mach" => state.derived.as_ref()?.mach,
        "control_surfaces.aileron_left_rad" => state.control_surfaces.as_ref()?.aileron_left_rad,
        "control_surfaces.aileron_right_rad" => state.control_surfaces.as_ref()?.aileron_right_rad,
        "control_surfaces.elevator_rad" => state.control_surfaces.as_ref()?.elevator_rad,
        "control_surfaces.rudder_rad" => state.control_surfaces.as_ref()?.rudder_rad,
        "control_surfaces.flaps_left_ratio" => state.control_surfaces.as_ref()?.flaps_left_ratio,
        "control_surfaces.flaps_right_ratio" => state.control_surfaces.as_ref()?.flaps_right_ratio,
        "control_surfaces.spoilers_ratio" => state.control_surfaces.as_ref()?.spoilers_ratio,
        _ => None,
    }
}

fn custom_numeric(field: &pb::CustomField) -> Option<f64> {
    match field.value.as_ref()?.kind.as_ref()? {
        pb::field_value::Kind::F64Value(value) => Some(*value),
        pb::field_value::Kind::I64Value(value) => Some(*value as f64),
        pb::field_value::Kind::BoolValue(value) => Some(u8::from(*value) as f64),
        pb::field_value::Kind::StringValue(_) | pb::field_value::Kind::BytesValue(_) => None,
    }
}

fn stats(points: &[[f64; 2]]) -> Option<SeriesStats> {
    let first = points.first()?;
    let last = points.last()?;
    let (min, max) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), point| {
            (min.min(point[1]), max.max(point[1]))
        });
    Some(SeriesStats {
        min,
        max,
        last: last[1],
        start: first[0],
        end: last[0],
    })
}

fn lttb(points: &[[f64; 2]], threshold: usize) -> Vec<[f64; 2]> {
    if points.len() <= threshold || threshold < 3 {
        return points.to_vec();
    }
    let mut sampled = Vec::with_capacity(threshold);
    sampled.push(points[0]);
    let every = (points.len() - 2) as f64 / (threshold - 2) as f64;
    let mut selected = 0usize;
    for bucket in 0..(threshold - 2) {
        let average_start = (((bucket + 1) as f64 * every).floor() as usize + 1).min(points.len());
        let average_end = (((bucket + 2) as f64 * every).floor() as usize + 1).min(points.len());
        let average_slice = &points[average_start..average_end];
        let (avg_x, avg_y) = if average_slice.is_empty() {
            let point = points[average_start.saturating_sub(1).min(points.len() - 1)];
            (point[0], point[1])
        } else {
            let sum = average_slice.iter().fold([0.0, 0.0], |sum, point| {
                [sum[0] + point[0], sum[1] + point[1]]
            });
            (
                sum[0] / average_slice.len() as f64,
                sum[1] / average_slice.len() as f64,
            )
        };
        let range_start = (bucket as f64 * every).floor() as usize + 1;
        let range_end = (((bucket + 1) as f64 * every).floor() as usize + 1).min(points.len() - 1);
        let anchor = points[selected];
        let mut max_area = -1.0;
        let mut next = range_start;
        for index in range_start..range_end.max(range_start + 1) {
            let point = points[index.min(points.len() - 2)];
            let area = ((anchor[0] - avg_x) * (point[1] - anchor[1])
                - (anchor[0] - point[0]) * (avg_y - anchor[1]))
                .abs();
            if area > max_area {
                max_area = area;
                next = index.min(points.len() - 2);
            }
        }
        sampled.push(points[next]);
        selected = next;
    }
    sampled.push(*points.last().expect("non-empty points"));
    sampled
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numeric_custom(field_id: &str, value: f64) -> pb::CustomField {
        pb::CustomField {
            field_id: field_id.to_string(),
            value: Some(pb::FieldValue {
                kind: Some(pb::field_value::Kind::F64Value(value)),
            }),
        }
    }

    #[test]
    fn lttb_respects_limit_and_endpoints() {
        let points = (0..1_000)
            .map(|index| [index as f64, (index as f64 / 10.0).sin()])
            .collect::<Vec<_>>();
        let sampled = lttb(&points, 100);
        assert_eq!(sampled.len(), 100);
        assert_eq!(sampled.first(), points.first());
        assert_eq!(sampled.last(), points.last());
    }

    #[test]
    fn catalog_and_query_cover_standard_engine_and_custom_fields() {
        let store = TimeSeriesStore::new();
        for index in 0..250 {
            store.append_state(
                "alpha".to_string(),
                index as f64,
                pb::AircraftState {
                    position: Some(pb::Vector3 {
                        x: index as f64,
                        y: f64::NAN,
                        z: 0.0,
                    }),
                    engines: vec![pb::EngineState {
                        index: 2,
                        throttle_lever_ratio: Some(index as f64 / 250.0),
                    }],
                    custom_fields: vec![numeric_custom(
                        "controller.cost.total",
                        index as f64 * 2.0,
                    )],
                    ..Default::default()
                },
            );
        }
        store.append_state(
            "bravo".to_string(),
            1.0,
            pb::AircraftState {
                position: Some(pb::Vector3 {
                    x: 999.0,
                    y: 0.0,
                    z: 0.0,
                }),
                ..Default::default()
            },
        );

        let fields = catalog(&store, "alpha").unwrap();
        assert!(fields.iter().any(|item| {
            item.selector
                == SeriesSelector::Custom {
                    field_id: "controller.cost.total".to_string(),
                }
        }));
        assert!(fields
            .iter()
            .any(|item| { item.selector == SeriesSelector::EngineThrottle { index: 2 } }));

        let response = query(
            &store,
            SeriesQueryRequest {
                selections: vec![
                    SeriesSelection {
                        aircraft_id: "alpha".to_string(),
                        selector: SeriesSelector::Standard {
                            path: "position.x".to_string(),
                        },
                    },
                    SeriesSelection {
                        aircraft_id: "alpha".to_string(),
                        selector: SeriesSelector::Standard {
                            path: "position.y".to_string(),
                        },
                    },
                ],
                time_range: Some(SeriesTimeRange {
                    start: 10.0,
                    end: 240.0,
                }),
                max_points: Some(100),
            },
        )
        .unwrap();
        assert_eq!(response.series[0].total_points, 231);
        assert_eq!(response.series[0].returned_points, 100);
        assert_eq!(response.series[0].stats.as_ref().unwrap().max, 240.0);
        assert!(response.series[1].points.is_empty());
    }
}
