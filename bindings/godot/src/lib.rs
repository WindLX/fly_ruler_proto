use std::path::Path;
use std::sync::{Mutex, OnceLock};

use fly_ruler_proto_core::pb;
use fly_ruler_proto_core::{init_logging, Event, KernelRuntime, LoggingConfig, TimeSeriesStore};
use godot::prelude::*;
use tokio::runtime::Runtime;

static GODOT_RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn get_runtime() -> &'static Runtime {
    init_logging(&LoggingConfig::default());
    GODOT_RUNTIME.get_or_init(|| Runtime::new().expect("failed to create tokio runtime"))
}

fn vector3_to_dict(v: Option<&pb::Vector3>) -> VarDictionary {
    let mut d = VarDictionary::new();
    if let Some(value) = v {
        d.set("x", value.x);
        d.set("y", value.y);
        d.set("z", value.z);
    }
    d
}

fn quaternion_to_dict(v: Option<&pb::Quaternion>) -> VarDictionary {
    let mut d = VarDictionary::new();
    if let Some(value) = v {
        d.set("w", value.w);
        d.set("x", value.x);
        d.set("y", value.y);
        d.set("z", value.z);
    }
    d
}

fn derived_to_dict(v: Option<&pb::DerivedState>) -> VarDictionary {
    let mut d = VarDictionary::new();
    if let Some(value) = v {
        d.set("lat", value.lat);
        d.set("lon", value.lon);
        d.set("altitude", value.altitude);
        d.set("alpha", value.alpha);
        d.set("beta", value.beta);
        d.set("tas", value.tas);
        d.set("eas", value.eas);
        d.set("gamma", value.gamma);
        d.set("chi", value.chi);
        if let Some(ias) = value.ias {
            d.set("ias", ias);
        }
        if let Some(cas) = value.cas {
            d.set("cas", cas);
        }
        if let Some(mach) = value.mach {
            d.set("mach", mach);
        }
    }
    d
}

fn control_surfaces_to_dict(v: Option<&pb::ControlSurfaceState>) -> VarDictionary {
    let mut d = VarDictionary::new();
    if let Some(value) = v {
        for (name, field) in [
            ("aileron_left_rad", value.aileron_left_rad),
            ("aileron_right_rad", value.aileron_right_rad),
            ("elevator_rad", value.elevator_rad),
            ("rudder_rad", value.rudder_rad),
            ("flaps_left_ratio", value.flaps_left_ratio),
            ("flaps_right_ratio", value.flaps_right_ratio),
            ("spoilers_ratio", value.spoilers_ratio),
        ] {
            if let Some(field) = field {
                d.set(name, field);
            }
        }
    }
    d
}

fn engines_to_array(engines: &[pb::EngineState]) -> Array<VarDictionary> {
    let mut out = Array::new();
    for engine in engines {
        let mut d = VarDictionary::new();
        d.set("index", engine.index);
        if let Some(throttle) = engine.throttle_lever_ratio {
            d.set("throttle_lever_ratio", throttle);
        }
        out.push(&d);
    }
    out
}

fn aircraft_state_to_dict(state: &pb::AircraftState) -> VarDictionary {
    let mut d = VarDictionary::new();
    d.set("position", &vector3_to_dict(state.position.as_ref()));
    d.set("velocity", &vector3_to_dict(state.velocity.as_ref()));
    d.set("attitude", &quaternion_to_dict(state.attitude.as_ref()));
    d.set(
        "angular_velocity",
        &vector3_to_dict(state.angular_velocity.as_ref()),
    );
    d.set("derived", &derived_to_dict(state.derived.as_ref()));
    d.set(
        "control_surfaces",
        &control_surfaces_to_dict(state.control_surfaces.as_ref()),
    );
    d.set("engines", &engines_to_array(&state.engines));
    d
}

#[derive(GodotClass)]
#[class(base = RefCounted)]
struct FlyRulerServer {
    base: Base<RefCounted>,
    runtime: Mutex<KernelRuntime>,
}

#[godot_api]
impl IRefCounted for FlyRulerServer {
    fn init(base: Base<RefCounted>) -> Self {
        let store = std::sync::Arc::new(TimeSeriesStore::new());
        let runtime = KernelRuntime::new(store);
        Self {
            base,
            runtime: Mutex::new(runtime),
        }
    }
}

#[godot_api]
impl FlyRulerServer {
    #[func]
    fn start_server(&self, addr: GString) -> bool {
        let runtime = get_runtime();
        let mut guard = match self.runtime.lock() {
            Ok(v) => v,
            Err(_) => {
                godot_error!("FlyRulerServer runtime lock poisoned");
                return false;
            }
        };

        match runtime.block_on(guard.start_server(&addr.to_string())) {
            Ok(_) => true,
            Err(err) => {
                godot_error!("start_server failed: {}", err);
                false
            }
        }
    }

    #[func]
    fn stop_server(&self) {
        let runtime = get_runtime();
        let mut guard = match self.runtime.lock() {
            Ok(v) => v,
            Err(_) => {
                godot_error!("FlyRulerServer runtime lock poisoned");
                return;
            }
        };
        runtime.block_on(guard.stop_server());
    }

    #[func]
    fn is_running(&self) -> bool {
        let guard = match self.runtime.lock() {
            Ok(v) => v,
            Err(_) => return false,
        };
        guard.udp_local_addr().is_ok()
    }

    #[func]
    fn local_addr(&self) -> GString {
        let guard = match self.runtime.lock() {
            Ok(v) => v,
            Err(_) => return GString::new(),
        };

        match guard.udp_local_addr() {
            Ok(addr) => {
                let value = addr.to_string();
                GString::from(value.as_str())
            }
            Err(_) => GString::new(),
        }
    }

    #[func]
    fn active_sessions(&self) -> Array<VarDictionary> {
        let runtime = get_runtime();
        let guard = match self.runtime.lock() {
            Ok(v) => v,
            Err(_) => return Array::new(),
        };

        let sessions = runtime.block_on(guard.active_sessions());
        let mut out = Array::new();
        for session in sessions {
            let mut d = VarDictionary::new();
            d.set("addr", session.addr.to_string());
            d.set("client_uuid_hex", session.client_uuid_hex);
            d.set("last_seen_secs", session.last_seen_secs);
            out.push(&d);
        }
        out
    }

    #[func]
    fn get_aircraft_ids(&self) -> PackedStringArray {
        let guard = match self.runtime.lock() {
            Ok(v) => v,
            Err(_) => return PackedStringArray::new(),
        };

        let mut out = PackedStringArray::new();
        for id in guard.store().get_aircraft_ids() {
            out.push(id.as_str());
        }
        out
    }

    #[func]
    fn get_latest_state(&self, aircraft_id: GString) -> VarDictionary {
        let guard = match self.runtime.lock() {
            Ok(v) => v,
            Err(_) => return VarDictionary::new(),
        };

        let store = guard.store();
        if let Some(value) = store.get_latest(&aircraft_id.to_string()) {
            let mut out = aircraft_state_to_dict(&value.state);
            out.set("timestamp_secs", value.timestamp_secs);
            return out;
        }
        VarDictionary::new()
    }

    #[func]
    fn get_states_in_range(
        &self,
        aircraft_id: GString,
        start: f64,
        end: f64,
    ) -> Array<VarDictionary> {
        let guard = match self.runtime.lock() {
            Ok(v) => v,
            Err(_) => return Array::new(),
        };

        let store = guard.store();
        let mut out = Array::new();
        if let Some(states) = store.get_states_range(&aircraft_id.to_string(), start, end) {
            for item in states {
                let mut d = aircraft_state_to_dict(&item.state);
                d.set("timestamp_secs", item.timestamp_secs);
                out.push(&d);
            }
        }
        out
    }

    #[func]
    fn get_events_in_range(
        &self,
        aircraft_id: GString,
        start: f64,
        end: f64,
    ) -> Array<VarDictionary> {
        let guard = match self.runtime.lock() {
            Ok(v) => v,
            Err(_) => return Array::new(),
        };

        let store = guard.store();
        let mut out = Array::new();
        if let Some(events) = store.get_events_range(&aircraft_id.to_string(), start, end) {
            for item in events {
                let mut d = VarDictionary::new();
                d.set("timestamp_secs", item.timestamp_secs);
                match item.event {
                    Event::Spawn(spawn) => {
                        d.set("event_type", "spawn");
                        d.set("name", spawn.name);
                        d.set("toml_config", spawn.toml_config);
                    }
                    Event::Despawn(despawn) => {
                        d.set("event_type", "despawn");
                        if let Some(reason) = despawn.reason {
                            d.set("reason", reason);
                        }
                    }
                    Event::Custom(name) => {
                        d.set("event_type", "custom");
                        d.set("name", name);
                    }
                }
                out.push(&d);
            }
        }
        out
    }

    #[func]
    fn save_session(&self, path: GString) -> bool {
        let guard = match self.runtime.lock() {
            Ok(v) => v,
            Err(_) => {
                godot_error!("FlyRulerServer runtime lock poisoned");
                return false;
            }
        };

        match guard.save_session(Path::new(&path.to_string())) {
            Ok(_) => true,
            Err(err) => {
                godot_error!("save_session failed: {}", err);
                false
            }
        }
    }

    #[func]
    fn load_session(&self, path: GString) -> bool {
        let guard = match self.runtime.lock() {
            Ok(v) => v,
            Err(_) => {
                godot_error!("FlyRulerServer runtime lock poisoned");
                return false;
            }
        };

        match guard.load_session(Path::new(&path.to_string())) {
            Ok(_) => true,
            Err(err) => {
                godot_error!("load_session failed: {}", err);
                false
            }
        }
    }

    #[func]
    fn clear_session(&self) {
        let guard = match self.runtime.lock() {
            Ok(v) => v,
            Err(_) => {
                godot_error!("FlyRulerServer runtime lock poisoned");
                return;
            }
        };
        guard.clear_session();
    }
}

struct FlyRulerGodotExtension;

#[gdextension]
unsafe impl ExtensionLibrary for FlyRulerGodotExtension {}
