use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::store::{AircraftId, TimeSeriesStore};
use crate::{pb, Attitude};

const DEFAULT_MAX_POINTS: usize = 2_000;
const MIN_MAX_POINTS: usize = 100;
const MAX_MAX_POINTS: usize = 20_000;
const MAX_SELECTIONS: usize = 64;

/// Numeric field available from a standard propulsor state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropulsorField {
    /// Normalized throttle ratio.
    ThrottleRatio,
    /// Rotational speed in revolutions per minute.
    Rpm,
    /// Blade pitch in radians.
    BladePitchRad,
    /// Thrust in newtons.
    ThrustNewton,
    /// Torque in newton-meters.
    TorqueNewtonMeter,
}

/// Selector identifying a single time-series source for an aircraft.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SeriesSelector {
    /// A standard aircraft state field path such as `derived.altitude`.
    Standard {
        /// Dot-separated path like `derived.altitude`.
        path: String,
    },
    /// A standard numeric field from one stable propulsor identifier.
    Propulsor {
        /// Stable propulsor identifier declared by the state producer.
        propulsor_id: String,
        /// Numeric propulsor field.
        field: PropulsorField,
    },
    /// A schema-registered telemetry field.
    Telemetry {
        /// Stream identifier registered at spawn.
        stream_id: String,
        /// Scalar field identifier within the stream.
        field_id: String,
    },
}

impl SeriesSelector {
    /// Return a stable key string for this selector.
    pub fn key(&self) -> String {
        match self {
            Self::Standard { path } => format!("standard:{path}"),
            Self::Propulsor {
                propulsor_id,
                field,
            } => format!("propulsor:{propulsor_id}:{}", field.key()),
            Self::Telemetry {
                stream_id,
                field_id,
            } => format!("telemetry:{stream_id}:{field_id}"),
        }
    }
}

impl PropulsorField {
    fn key(self) -> &'static str {
        match self {
            Self::ThrottleRatio => "throttle_ratio",
            Self::Rpm => "rpm",
            Self::BladePitchRad => "blade_pitch_rad",
            Self::ThrustNewton => "thrust_newton",
            Self::TorqueNewtonMeter => "torque_newton_meter",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ThrottleRatio => "Throttle",
            Self::Rpm => "RPM",
            Self::BladePitchRad => "Blade pitch",
            Self::ThrustNewton => "Thrust",
            Self::TorqueNewtonMeter => "Torque",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Self::ThrottleRatio => "ratio",
            Self::Rpm => "rpm",
            Self::BladePitchRad => "rad",
            Self::ThrustNewton => "N",
            Self::TorqueNewtonMeter => "N·m",
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
    /// A telemetry selector is not present in the registered schema.
    #[error("unknown telemetry field: {0}:{1}")]
    UnknownTelemetryField(String, String),
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
    let schemas = store.get_telemetry_schemas(&id).unwrap_or_default();
    for schema in &schemas {
        for field in &schema.fields {
            selectors.insert(SeriesSelector::Telemetry {
                stream_id: schema.stream_id.clone(),
                field_id: field.field_id.clone(),
            });
        }
    }
    Ok(selectors
        .into_iter()
        .map(|selector| {
            if let SeriesSelector::Telemetry {
                stream_id,
                field_id,
            } = &selector
            {
                let (schema, index) = telemetry_field(&schemas, stream_id, field_id)
                    .expect("telemetry selector comes from the registered schema");
                let field = &schema.fields[index];
                return SeriesCatalogItem {
                    selector,
                    group: if field.group.is_empty() {
                        schema.name.clone()
                    } else {
                        field.group.clone()
                    },
                    label: if field.label.is_empty() {
                        field.field_id.clone()
                    } else {
                        field.label.clone()
                    },
                    unit: (!field.unit.is_empty()).then(|| field.unit.clone()),
                };
            }
            catalog_item(selector)
        })
        .collect())
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
        let schemas = store
            .get_telemetry_schemas(&aircraft_id)
            .ok_or_else(|| SeriesError::AircraftNotFound(aircraft_id.clone()))?;
        for (_, selector) in &selections {
            if let SeriesSelector::Telemetry {
                stream_id,
                field_id,
            } = selector
            {
                telemetry_field(&schemas, stream_id, field_id).ok_or_else(|| {
                    SeriesError::UnknownTelemetryField(stream_id.clone(), field_id.clone())
                })?;
            }
        }
        store.visit_telemetry_range(&aircraft_id, start, end, |frames| {
            for (slot, (_, selector)) in selections.iter().enumerate() {
                let SeriesSelector::Telemetry {
                    stream_id,
                    field_id,
                } = selector
                else {
                    continue;
                };
                let Some((schema, field_index)) = telemetry_field(&schemas, stream_id, field_id)
                else {
                    continue;
                };
                for sample in frames {
                    if sample.frame.stream_id != schema.stream_id {
                        continue;
                    }
                    let Some(value) = sample
                        .frame
                        .values
                        .get(field_index)
                        .and_then(telemetry_numeric)
                    else {
                        continue;
                    };
                    if sample.timestamp_secs.is_finite() && value.is_finite() {
                        points[slot].push([sample.timestamp_secs, value]);
                    }
                }
            }
        });
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
        ("attitude.roll", "attitude", "Roll", Some("rad")),
        ("attitude.pitch", "attitude", "Pitch", Some("rad")),
        ("attitude.yaw", "attitude", "Yaw", Some("rad")),
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
            "derived.ground_speed",
            "derived",
            "Ground speed",
            Some("m/s"),
        ),
        (
            "derived.vertical_speed",
            "derived",
            "Vertical speed",
            Some("m/s"),
        ),
        (
            "derived.dynamic_pressure",
            "derived",
            "Dynamic pressure",
            Some("Pa"),
        ),
        (
            "derived.normal_load_factor",
            "derived",
            "Normal load factor",
            Some("g"),
        ),
        (
            "linear_acceleration_body.x",
            "acceleration",
            "Body acceleration X",
            Some("m/s²"),
        ),
        (
            "linear_acceleration_body.y",
            "acceleration",
            "Body acceleration Y",
            Some("m/s²"),
        ),
        (
            "linear_acceleration_body.z",
            "acceleration",
            "Body acceleration Z",
            Some("m/s²"),
        ),
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
    for propulsor in &state.propulsors {
        for field in [
            PropulsorField::ThrottleRatio,
            PropulsorField::Rpm,
            PropulsorField::BladePitchRad,
            PropulsorField::ThrustNewton,
            PropulsorField::TorqueNewtonMeter,
        ] {
            if propulsor_value(propulsor, field).is_some() {
                output.insert(SeriesSelector::Propulsor {
                    propulsor_id: propulsor.propulsor_id.clone(),
                    field,
                });
            }
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
        SeriesSelector::Propulsor {
            propulsor_id,
            field,
        } => SeriesCatalogItem {
            selector: selector.clone(),
            group: "propulsors".to_string(),
            label: format!("{propulsor_id} {}", field.label()),
            unit: Some(field.unit().to_string()),
        },
        SeriesSelector::Telemetry {
            stream_id,
            field_id,
        } => SeriesCatalogItem {
            selector: selector.clone(),
            group: stream_id.clone(),
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
        SeriesSelector::Propulsor {
            propulsor_id,
            field,
        } => state
            .propulsors
            .iter()
            .rev()
            .find(|propulsor| propulsor.propulsor_id == *propulsor_id)
            .and_then(|propulsor| propulsor_value(propulsor, *field)),
        SeriesSelector::Telemetry { .. } => None,
    }
}

fn telemetry_field<'a>(
    schemas: &'a [pb::TelemetryStreamSchema],
    stream_id: &str,
    field_id: &str,
) -> Option<(&'a pb::TelemetryStreamSchema, usize)> {
    let schema = schemas
        .iter()
        .find(|schema| schema.stream_id == stream_id)?;
    let index = schema
        .fields
        .iter()
        .position(|field| field.field_id == field_id)?;
    Some((schema, index))
}

fn telemetry_numeric(value: &pb::TelemetryValue) -> Option<f64> {
    match value.kind.as_ref()? {
        pb::telemetry_value::Kind::F64Value(value) => Some(*value),
        pb::telemetry_value::Kind::I64Value(value) => Some(*value as f64),
        pb::telemetry_value::Kind::BoolValue(value) => Some(u8::from(*value) as f64),
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
        "attitude.roll" => Attitude::try_from(state.attitude.as_ref()?)
            .ok()
            .map(|value| value.euler()[0]),
        "attitude.pitch" => Attitude::try_from(state.attitude.as_ref()?)
            .ok()
            .map(|value| value.euler()[1]),
        "attitude.yaw" => Attitude::try_from(state.attitude.as_ref()?)
            .ok()
            .map(|value| value.euler()[2]),
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
        "derived.ground_speed" => state.derived.as_ref()?.ground_speed,
        "derived.vertical_speed" => state.derived.as_ref()?.vertical_speed,
        "derived.dynamic_pressure" => state.derived.as_ref()?.dynamic_pressure,
        "derived.normal_load_factor" => state.derived.as_ref()?.normal_load_factor,
        "linear_acceleration_body.x" => {
            state.linear_acceleration_body.as_ref().map(|value| value.x)
        }
        "linear_acceleration_body.y" => {
            state.linear_acceleration_body.as_ref().map(|value| value.y)
        }
        "linear_acceleration_body.z" => {
            state.linear_acceleration_body.as_ref().map(|value| value.z)
        }
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

fn propulsor_value(propulsor: &pb::PropulsorState, field: PropulsorField) -> Option<f64> {
    match field {
        PropulsorField::ThrottleRatio => propulsor.throttle_ratio,
        PropulsorField::Rpm => propulsor.rpm,
        PropulsorField::BladePitchRad => propulsor.blade_pitch_rad,
        PropulsorField::ThrustNewton => propulsor.thrust_newton,
        PropulsorField::TorqueNewtonMeter => propulsor.torque_newton_meter,
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
    fn removed_engine_and_custom_selectors_are_rejected() {
        for selector in [
            serde_json::json!({"kind": "engine_throttle", "index": 1}),
            serde_json::json!({"kind": "custom", "field_id": "legacy"}),
        ] {
            assert!(serde_json::from_value::<SeriesSelector>(selector).is_err());
        }
    }

    #[test]
    fn telemetry_catalog_exists_before_samples_and_query_keeps_all_frames() {
        let store = TimeSeriesStore::new();
        store.append_state("aircraft".to_string(), 0.0, pb::AircraftState::default());
        store.append_event(
            "aircraft".to_string(),
            0.0,
            crate::store::Event::Spawn(Box::new(pb::AircraftSpawnInfo {
                name: "test".to_string(),
                toml_config: String::new(),
                initial_state: None,
                telemetry_schemas: vec![pb::TelemetryStreamSchema {
                    stream_id: "controller".to_string(),
                    name: "Controller".to_string(),
                    nominal_rate_hz: Some(100.0),
                    fields: vec![pb::TelemetryField {
                        field_id: "controller.pitch.error".to_string(),
                        label: "Pitch error".to_string(),
                        group: "Pitch".to_string(),
                        unit: "rad".to_string(),
                        description: String::new(),
                        value_type: pb::TelemetryValueType::F64 as i32,
                    }],
                }],
            })),
        );

        let catalog = catalog(&store, "aircraft").unwrap();
        let telemetry = catalog
            .iter()
            .find(|item| matches!(item.selector, SeriesSelector::Telemetry { .. }))
            .unwrap();
        assert_eq!(telemetry.label, "Pitch error");
        assert_eq!(telemetry.unit.as_deref(), Some("rad"));

        let selection = SeriesSelection {
            aircraft_id: "aircraft".to_string(),
            selector: telemetry.selector.clone(),
        };
        let empty = query(
            &store,
            SeriesQueryRequest {
                selections: vec![selection.clone()],
                time_range: None,
                max_points: Some(100),
            },
        )
        .unwrap();
        assert!(empty.series[0].points.is_empty());

        for (sequence, value) in [(1, 0.25), (2, -0.5)] {
            store
                .append_telemetry(
                    "aircraft".to_string(),
                    sequence as f64,
                    pb::TelemetryFrame {
                        stream_id: "controller".to_string(),
                        sequence,
                        values: vec![pb::TelemetryValue {
                            kind: Some(pb::telemetry_value::Kind::F64Value(value)),
                        }],
                    },
                )
                .unwrap();
        }
        let response = query(
            &store,
            SeriesQueryRequest {
                selections: vec![selection],
                time_range: None,
                max_points: Some(100),
            },
        )
        .unwrap();
        assert_eq!(response.series[0].points, vec![[1.0, 0.25], [2.0, -0.5]]);
    }

    #[test]
    fn catalog_and_query_cover_standard_and_propulsor_fields() {
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
                    attitude: Some(pb::Quaternion::from(
                        &Attitude::from_euler([index as f64 / 100.0, 0.0, 0.0]).unwrap(),
                    )),
                    propulsors: vec![pb::PropulsorState {
                        propulsor_id: "engine.left".to_string(),
                        throttle_ratio: Some(index as f64 / 250.0),
                        rpm: Some(index as f64 * 100.0),
                        blade_pitch_rad: Some(index as f64 / 1_000.0),
                        thrust_newton: Some(index as f64 * 10.0),
                        torque_newton_meter: Some(index as f64),
                        index: Some(1),
                        ..Default::default()
                    }],
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
                == SeriesSelector::Propulsor {
                    propulsor_id: "engine.left".to_string(),
                    field: PropulsorField::ThrottleRatio,
                }
        }));
        for field in [
            PropulsorField::Rpm,
            PropulsorField::BladePitchRad,
            PropulsorField::ThrustNewton,
            PropulsorField::TorqueNewtonMeter,
        ] {
            assert!(fields.iter().any(|item| {
                item.selector
                    == SeriesSelector::Propulsor {
                        propulsor_id: "engine.left".to_string(),
                        field,
                    }
            }));
        }
        assert!(fields.iter().any(|item| {
            item.selector
                == SeriesSelector::Standard {
                    path: "attitude.roll".to_string(),
                }
        }));

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
                    SeriesSelection {
                        aircraft_id: "alpha".to_string(),
                        selector: SeriesSelector::Propulsor {
                            propulsor_id: "engine.left".to_string(),
                            field: PropulsorField::ThrottleRatio,
                        },
                    },
                    SeriesSelection {
                        aircraft_id: "alpha".to_string(),
                        selector: SeriesSelector::Standard {
                            path: "attitude.roll".to_string(),
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
        assert_eq!(response.series[2].total_points, 231);
        assert!((response.series[2].stats.as_ref().unwrap().max - 0.96).abs() < 1e-12);
        assert!((response.series[3].stats.as_ref().unwrap().max - 2.4).abs() < 1e-12);
    }
}
