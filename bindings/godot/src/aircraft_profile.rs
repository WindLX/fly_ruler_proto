use std::collections::HashSet;
use std::fs;

use godot::prelude::*;
use serde::{Deserialize, Serialize};

pub const AIRCRAFT_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const MAX_AIRCRAFT_PROFILE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AircraftProfileFile {
    pub schema_version: u32,
    pub profile_id: String,
    pub display_name: String,
    #[serde(default)]
    pub priority: i64,
    #[serde(rename = "match")]
    pub match_rules: MatchRules,
    pub model: ModelConfig,
    pub hud: HudConfig,
    #[serde(default)]
    pub channels: Vec<ChannelConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchRules {
    #[serde(default)]
    pub model_ids: Vec<String>,
    #[serde(default)]
    pub name_prefixes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub scene: String,
    #[serde(default = "one_vec")]
    pub scale: [f64; 3],
    #[serde(default)]
    pub rotation_degrees: [f64; 3],
    #[serde(default)]
    pub ground_offset_m: f64,
    pub bounding_radius_m: f64,
    #[serde(default)]
    pub camera_target_m: [f64; 3],
    #[serde(default)]
    pub cockpit_enabled: bool,
    #[serde(default)]
    pub cockpit_eye_m: [f64; 3],
    #[serde(default = "default_orbit_yaw")]
    pub orbit_yaw_degrees: f64,
    #[serde(default = "default_orbit_pitch")]
    pub orbit_pitch_degrees: f64,
    #[serde(default = "one")]
    pub orbit_distance_multiplier: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HudConfig {
    pub panels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelConfig {
    pub id: String,
    pub panel: String,
    pub label: String,
    pub source: DataSource,
    pub range: ValueRange,
    #[serde(default)]
    pub display: DisplayConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DataSource {
    State {
        path: String,
    },
    Propulsor {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        index: Option<i64>,
        field: String,
    },
    Mean {
        paths: Vec<String>,
    },
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueRange {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisplayConfig {
    pub transform: String,
    pub unit: String,
    pub decimals: i64,
    pub scale: f64,
    pub offset: f64,
    pub invert: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            transform: "identity".to_string(),
            unit: String::new(),
            decimals: 1,
            scale: 1.0,
            offset: 0.0,
            invert: false,
        }
    }
}

fn one_vec() -> [f64; 3] {
    [1.0; 3]
}
fn one() -> f64 {
    1.0
}
fn default_orbit_yaw() -> f64 {
    35.0
}
fn default_orbit_pitch() -> f64 {
    -18.0
}

const PANELS: [&str; 8] = [
    "common_flight",
    "quad_rotors",
    "twin_engine",
    "airliner_actuators",
    "single_engine",
    "fighter_actuators",
    "evtol_propulsion",
    "evtol_actuators",
];
const STATE_PATHS: [&str; 7] = [
    "control_surfaces.aileron_left_rad",
    "control_surfaces.aileron_right_rad",
    "control_surfaces.elevator_rad",
    "control_surfaces.rudder_rad",
    "control_surfaces.flaps_left_ratio",
    "control_surfaces.flaps_right_ratio",
    "control_surfaces.spoilers_ratio",
];
const PROPULSOR_FIELDS: [&str; 5] = [
    "rpm",
    "throttle_ratio",
    "blade_pitch_rad",
    "thrust_newton",
    "torque_newton_meter",
];

impl AircraftProfileFile {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != AIRCRAFT_PROFILE_SCHEMA_VERSION {
            return Err(format!(
                "unsupported schema_version {}; expected {}",
                self.schema_version, AIRCRAFT_PROFILE_SCHEMA_VERSION
            ));
        }
        validate_identifier(&self.profile_id, "profile_id")?;
        if self.display_name.trim().is_empty() {
            return Err("display_name must not be empty".to_string());
        }
        if self.match_rules.model_ids.is_empty() && self.match_rules.name_prefixes.is_empty() {
            return Err("match must contain a model_id or name_prefix".to_string());
        }
        for model_id in &self.match_rules.model_ids {
            validate_identifier(model_id, "model_id")?;
        }
        if self
            .match_rules
            .name_prefixes
            .iter()
            .any(|value| value.trim().is_empty())
        {
            return Err("name_prefixes must not contain empty values".to_string());
        }
        validate_resource_path(&self.model.scene)?;
        for (name, value) in [
            ("bounding_radius_m", self.model.bounding_radius_m),
            (
                "orbit_distance_multiplier",
                self.model.orbit_distance_multiplier,
            ),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("{name} must be finite and greater than zero"));
            }
        }
        for value in self
            .model
            .scale
            .into_iter()
            .chain(self.model.rotation_degrees)
            .chain(self.model.camera_target_m)
            .chain(self.model.cockpit_eye_m)
            .chain([
                self.model.ground_offset_m,
                self.model.orbit_yaw_degrees,
                self.model.orbit_pitch_degrees,
            ])
        {
            if !value.is_finite() {
                return Err("model numeric values must be finite".to_string());
            }
        }
        if self.model.scale.into_iter().any(|value| value == 0.0) {
            return Err("model scale components must not be zero".to_string());
        }
        if self.hud.panels.is_empty()
            || !self.hud.panels.iter().any(|value| value == "common_flight")
        {
            return Err("hud.panels must include common_flight".to_string());
        }
        for panel in &self.hud.panels {
            if !PANELS.contains(&panel.as_str()) {
                return Err(format!("unknown panel {panel}"));
            }
        }
        let enabled_panels: HashSet<_> = self.hud.panels.iter().map(String::as_str).collect();
        let mut ids = HashSet::new();
        for channel in &self.channels {
            validate_identifier(&channel.id, "channel id")?;
            if !ids.insert(channel.id.as_str()) {
                return Err(format!("duplicate channel id {}", channel.id));
            }
            if !enabled_panels.contains(channel.panel.as_str()) || channel.panel == "common_flight"
            {
                return Err(format!(
                    "channel {} references unavailable panel {}",
                    channel.id, channel.panel
                ));
            }
            if channel.label.trim().is_empty() {
                return Err(format!("channel {} label must not be empty", channel.id));
            }
            validate_source(&channel.source)?;
            if !channel.range.min.is_finite()
                || !channel.range.max.is_finite()
                || channel.range.min >= channel.range.max
            {
                return Err(format!("channel {} has an invalid range", channel.id));
            }
            if !matches!(
                channel.display.transform.as_str(),
                "identity" | "rad_to_deg" | "ratio_to_percent"
            ) {
                return Err(format!(
                    "channel {} has an unknown display transform",
                    channel.id
                ));
            }
            if !(0..=6).contains(&channel.display.decimals)
                || !channel.display.scale.is_finite()
                || !channel.display.offset.is_finite()
            {
                return Err(format!(
                    "channel {} has an invalid display mapping",
                    channel.id
                ));
            }
        }
        Ok(())
    }
}

fn validate_identifier(value: &str, name: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err(format!(
            "{name} must contain only ASCII letters, numbers, '_' or '-'"
        ));
    }
    Ok(())
}

fn validate_resource_path(value: &str) -> Result<(), String> {
    if !value.starts_with("res://") || !value.ends_with(".tscn") || value.contains("..") {
        return Err("model.scene must be a safe res:// .tscn path".to_string());
    }
    Ok(())
}

fn validate_source(source: &DataSource) -> Result<(), String> {
    match source {
        DataSource::State { path } => validate_state_path(path),
        DataSource::Propulsor { id, index, field } => {
            if id.is_some() == index.is_some() {
                return Err("propulsor source must specify exactly one of id or index".to_string());
            }
            if id.as_ref().is_some_and(|value| value.trim().is_empty())
                || index.is_some_and(|value| value < 1)
            {
                return Err("propulsor selector is invalid".to_string());
            }
            if !PROPULSOR_FIELDS.contains(&field.as_str()) {
                return Err(format!("unknown propulsor field {field}"));
            }
            Ok(())
        }
        DataSource::Mean { paths } => {
            if paths.len() < 2 {
                return Err("mean source requires at least two paths".to_string());
            }
            for path in paths {
                validate_state_path(path)?;
            }
            Ok(())
        }
        DataSource::Unavailable => Ok(()),
    }
}

fn validate_state_path(path: &str) -> Result<(), String> {
    if STATE_PATHS.contains(&path) {
        Ok(())
    } else {
        Err(format!("unknown state path {path}"))
    }
}

pub fn load_aircraft_profile(path: &str) -> Result<AircraftProfileFile, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_AIRCRAFT_PROFILE_BYTES {
        return Err("aircraft profile exceeds 64 KiB".to_string());
    }
    let source = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let profile: AircraftProfileFile =
        toml::from_str(&source).map_err(|error| error.to_string())?;
    profile.validate()?;
    Ok(profile)
}

pub fn vec3_dictionary(value: [f64; 3]) -> VarDictionary {
    let mut dictionary = VarDictionary::new();
    dictionary.set("x", value[0]);
    dictionary.set("y", value[1]);
    dictionary.set("z", value[2]);
    dictionary
}

pub fn source_dictionary(source: &DataSource) -> VarDictionary {
    let mut dictionary = VarDictionary::new();
    match source {
        DataSource::State { path } => {
            dictionary.set("kind", "state");
            dictionary.set("path", &GString::from(path.as_str()));
        }
        DataSource::Propulsor { id, index, field } => {
            dictionary.set("kind", "propulsor");
            if let Some(id) = id {
                dictionary.set("id", &GString::from(id.as_str()));
            }
            if let Some(index) = index {
                dictionary.set("index", *index);
            }
            dictionary.set("field", &GString::from(field.as_str()));
        }
        DataSource::Mean { paths } => {
            dictionary.set("kind", "mean");
            let values: PackedStringArray = paths
                .iter()
                .map(|value| GString::from(value.as_str()))
                .collect();
            dictionary.set("paths", &values);
        }
        DataSource::Unavailable => dictionary.set("kind", "unavailable"),
    }
    dictionary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_source() -> &'static str {
        r#"schema_version = 1
profile_id = "boeing_737_800"
display_name = "Boeing 737-800"
priority = 100
[match]
model_ids = ["boeing_737_800", "b737"]
name_prefixes = ["Boeing 737"]
[model]
scene = "res://models/airliners/boeing_737_800/boeing_737_800_visual.tscn"
bounding_radius_m = 20.0
[hud]
panels = ["common_flight", "twin_engine", "airliner_actuators"]
[[channels]]
id = "engine_1_rpm"
panel = "twin_engine"
label = "ENG 1"
source = { kind = "propulsor", index = 1, field = "rpm" }
range = { min = 0.0, max = 15000.0 }
display = { unit = "rpm", decimals = 0 }
"#
    }

    #[test]
    fn parses_complete_profile_and_applies_display_defaults() {
        let profile: AircraftProfileFile = toml::from_str(profile_source()).unwrap();
        profile.validate().unwrap();
        assert_eq!(profile.channels[0].display.scale, 1.0);
        assert_eq!(profile.model.scale, [1.0; 3]);
    }

    #[test]
    fn accepts_fighter_specific_panels() {
        let source = profile_source()
            .replace(
                "panels = [\"common_flight\", \"twin_engine\", \"airliner_actuators\"]",
                "panels = [\"common_flight\", \"single_engine\", \"fighter_actuators\"]",
            )
            .replace("panel = \"twin_engine\"", "panel = \"single_engine\"");
        let profile: AircraftProfileFile = toml::from_str(&source).unwrap();
        profile.validate().unwrap();
    }

    #[test]
    fn accepts_evtol_specific_panels() {
        let source = profile_source()
            .replace(
                "panels = [\"common_flight\", \"twin_engine\", \"airliner_actuators\"]",
                "panels = [\"common_flight\", \"evtol_propulsion\", \"evtol_actuators\"]",
            )
            .replace("panel = \"twin_engine\"", "panel = \"evtol_propulsion\"");
        let profile: AircraftProfileFile = toml::from_str(&source).unwrap();
        profile.validate().unwrap();
    }

    #[test]
    fn rejects_unknown_fields_paths_ranges_and_duplicate_channels() {
        assert!(toml::from_str::<AircraftProfileFile>(
            &profile_source().replace("priority = 100", "priority = 100\nextra = true")
        )
        .is_err());
        let mut profile: AircraftProfileFile = toml::from_str(profile_source()).unwrap();
        profile.model.scene = "user://unsafe.tscn".to_string();
        assert!(profile.validate().unwrap_err().contains("res://"));
        let mut profile: AircraftProfileFile = toml::from_str(profile_source()).unwrap();
        profile.channels[0].range.max = 0.0;
        assert!(profile.validate().unwrap_err().contains("range"));
        let mut profile: AircraftProfileFile = toml::from_str(profile_source()).unwrap();
        profile.channels.push(profile.channels[0].clone());
        assert!(profile.validate().unwrap_err().contains("duplicate"));
    }

    #[test]
    fn profile_roundtrip_rejects_versions_sources_and_oversized_files() {
        let profile: AircraftProfileFile = toml::from_str(profile_source()).unwrap();
        let serialized = toml::to_string_pretty(&profile).unwrap();
        assert_eq!(
            toml::from_str::<AircraftProfileFile>(&serialized).unwrap(),
            profile
        );
        let mut invalid = profile.clone();
        invalid.schema_version = 2;
        assert!(invalid.validate().unwrap_err().contains("schema_version"));
        let mut invalid = profile;
        invalid.channels[0].source = DataSource::State {
            path: "custom.unsupported".to_string(),
        };
        assert!(invalid.validate().unwrap_err().contains("state path"));

        let root =
            std::env::temp_dir().join(format!("fly-ruler-aircraft-profile-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("oversized.toml");
        fs::write(&path, "x".repeat(MAX_AIRCRAFT_PROFILE_BYTES as usize + 1)).unwrap();
        assert!(load_aircraft_profile(path.to_str().unwrap())
            .unwrap_err()
            .contains("64 KiB"));
        fs::remove_dir_all(root).unwrap();
    }
}
