//! Minimal, MSFS 2024 SDK-matched SimConnect wrapper.

use std::collections::VecDeque;
use std::ffi::{c_char, c_void, CString};
use std::mem;
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

use crate::{GearCommand, MsfsAirData, MsfsPose, Simulator, Surface};
use thiserror::Error;

type Handle = *mut c_void;
type HResult = i32;

const S_OK: HResult = 0;
pub type MsfsObjectId = u32;

pub const OBJECT_USER: MsfsObjectId = 0;
const UNUSED: u32 = u32::MAX;
const OPEN_CONFIG_LOCAL: u32 = u32::MAX;
const GROUP_PRIORITY_HIGHEST: u32 = 1;
const EVENT_FLAG_GROUP_IS_PRIORITY: u32 = 0x10;
const DATATYPE_INT32: i32 = 1;
const DATATYPE_FLOAT64: i32 = 4;
const PERIOD_ONCE: i32 = 1;

const RECV_ID_EXCEPTION: u32 = 1;
const RECV_ID_QUIT: u32 = 3;
const RECV_ID_SIMOBJECT_DATA: u32 = 8;
const RECV_ID_ASSIGNED_OBJECT_ID: u32 = 12;

const DEFINE_POSE: u32 = 1;
const DEFINE_FREEZE_STATE: u32 = 2;
const DEFINE_AIRDATA: u32 = 3;
const DEFINE_ENGINE_COUNT: u32 = 4;
const REQUEST_FREEZE_STATE: u32 = 1;
const REQUEST_ENGINE_COUNT: u32 = 2;
const REQUEST_AI_BASE: u32 = 10_000;

const EVENT_FREEZE_LAT_LON: u32 = 100;
const EVENT_FREEZE_ALTITUDE: u32 = 101;
const EVENT_FREEZE_ATTITUDE: u32 = 102;
const EVENT_GEAR_UP: u32 = 103;
const EVENT_GEAR_DOWN: u32 = 104;

#[link(name = "SimConnect")]
unsafe extern "C" {
    fn SimConnect_Open(
        handle: *mut Handle,
        name: *const c_char,
        window: Handle,
        user_event: u32,
        event: Handle,
        config_index: u32,
    ) -> HResult;
    fn SimConnect_Close(handle: Handle) -> HResult;
    fn SimConnect_GetNextDispatch(
        handle: Handle,
        data: *mut *const Recv,
        byte_count: *mut u32,
    ) -> HResult;
    fn SimConnect_GetLastSentPacketID(handle: Handle, send_id: *mut u32) -> HResult;
    fn SimConnect_AddToDataDefinition(
        handle: Handle,
        definition_id: u32,
        datum_name: *const c_char,
        units_name: *const c_char,
        datum_type: i32,
        epsilon: f32,
        datum_id: u32,
    ) -> HResult;
    fn SimConnect_RequestDataOnSimObject(
        handle: Handle,
        request_id: u32,
        definition_id: u32,
        object_id: u32,
        period: i32,
        flags: u32,
        origin: u32,
        interval: u32,
        limit: u32,
    ) -> HResult;
    fn SimConnect_SetDataOnSimObject(
        handle: Handle,
        definition_id: u32,
        object_id: u32,
        flags: u32,
        array_count: u32,
        unit_size: u32,
        data: *const c_void,
    ) -> HResult;
    fn SimConnect_MapClientEventToSimEvent(
        handle: Handle,
        event_id: u32,
        event_name: *const c_char,
    ) -> HResult;
    fn SimConnect_TransmitClientEvent(
        handle: Handle,
        object_id: u32,
        event_id: u32,
        data: u32,
        group_id: u32,
        flags: u32,
    ) -> HResult;
    fn SimConnect_AICreateNonATCAircraft_EX1(
        handle: Handle,
        container_title: *const c_char,
        livery: *const c_char,
        tail_number: *const c_char,
        init_position: InitPosition,
        request_id: u32,
    ) -> HResult;
    fn SimConnect_AIReleaseControl(handle: Handle, object_id: u32, request_id: u32) -> HResult;
    fn SimConnect_AIRemoveObject(handle: Handle, object_id: u32, request_id: u32) -> HResult;
}

#[derive(Debug, Error)]
pub enum SimConnectError {
    #[error("SimConnect call {operation} failed with HRESULT 0x{code:08x}")]
    Call { operation: &'static str, code: u32 },
    #[error("Microsoft Flight Simulator closed the SimConnect connection")]
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimConnectException {
    pub exception: u32,
    pub send_id: u32,
    pub index: u32,
}

impl SimConnectException {
    pub fn name(self) -> &'static str {
        match self.exception {
            22 => "CREATE_OBJECT_FAILED",
            33 => "OBJECT_OUTSIDE_REALITY_BUBBLE",
            34 => "OBJECT_CONTAINER",
            35 => "OBJECT_AI",
            36 => "OBJECT_ATC",
            37 => "OBJECT_SCHEDULE",
            _ => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiCreateRequest {
    pub request_id: u32,
    pub send_id: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Recv {
    size: u32,
    version: u32,
    id: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct RecvException {
    base: Recv,
    exception: u32,
    send_id: u32,
    index: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct RecvSimObjectData {
    base: Recv,
    request_id: u32,
    object_id: u32,
    define_id: u32,
    flags: u32,
    entry_number: u32,
    out_of: u32,
    define_count: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct RecvFreezeData {
    base: RecvSimObjectData,
    values: [i32; 3],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct RecvEngineCount {
    base: RecvSimObjectData,
    value: i32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct RecvAssignedObjectId {
    base: Recv,
    request_id: u32,
    object_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct InitPosition {
    latitude: f64,
    longitude: f64,
    altitude: f64,
    pitch: f64,
    bank: f64,
    heading: f64,
    on_ground: u32,
    airspeed: u32,
}

pub struct SimConnectClient {
    handle: Handle,
    original_freeze: [bool; 3],
    acquired_freeze: bool,
    engine_count: u32,
    reported_unsupported_engines: [bool; 4],
    next_ai_request_id: u32,
    assigned_objects: VecDeque<(u32, MsfsObjectId)>,
    exceptions: VecDeque<SimConnectException>,
}

enum Dispatch {
    Empty,
    Other,
    FreezeState([bool; 3]),
    EngineCount(u32),
    AssignedObject {
        request_id: u32,
        object_id: MsfsObjectId,
    },
    Exception(SimConnectException),
}

impl SimConnectClient {
    pub fn connect() -> Result<Self, SimConnectError> {
        let name = CString::new("FlyRuler MSFS 2024 Bridge").expect("static CString");
        let mut handle = ptr::null_mut();
        // SAFETY: all pointers are valid for the duration of the call and the SDK owns the handle.
        check("SimConnect_Open", unsafe {
            SimConnect_Open(
                &mut handle,
                name.as_ptr(),
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                OPEN_CONFIG_LOCAL,
            )
        })?;

        let mut client = Self {
            handle,
            original_freeze: [false; 3],
            acquired_freeze: false,
            engine_count: 4,
            reported_unsupported_engines: [false; 4],
            next_ai_request_id: REQUEST_AI_BASE,
            assigned_objects: VecDeque::new(),
            exceptions: VecDeque::new(),
        };
        if let Err(error) = client.configure() {
            // SAFETY: the handle was returned successfully by SimConnect_Open.
            unsafe {
                SimConnect_Close(handle);
            }
            return Err(error);
        }
        client.original_freeze = client.read_original_freeze(Duration::from_secs(2))?;
        client.engine_count = client.read_engine_count(Duration::from_secs(2))?;
        Ok(client)
    }

    fn configure(&mut self) -> Result<(), SimConnectError> {
        for (name, units) in [
            ("PLANE LATITUDE", "degrees"),
            ("PLANE LONGITUDE", "degrees"),
            ("PLANE ALTITUDE", "meters"),
            ("PLANE PITCH DEGREES", "radians"),
            ("PLANE BANK DEGREES", "radians"),
            ("PLANE HEADING DEGREES TRUE", "radians"),
        ] {
            self.add_definition(DEFINE_POSE, name, units, DATATYPE_FLOAT64)?;
        }

        for name in [
            "IS LATITUDE LONGITUDE FREEZE ON",
            "IS ALTITUDE FREEZE ON",
            "IS ATTITUDE FREEZE ON",
        ] {
            self.add_definition(DEFINE_FREEZE_STATE, name, "Bool", DATATYPE_INT32)?;
        }

        for (event_id, name) in [
            (EVENT_FREEZE_LAT_LON, "FREEZE_LATITUDE_LONGITUDE_SET"),
            (EVENT_FREEZE_ALTITUDE, "FREEZE_ALTITUDE_SET"),
            (EVENT_FREEZE_ATTITUDE, "FREEZE_ATTITUDE_SET"),
            (EVENT_GEAR_UP, "GEAR_UP"),
            (EVENT_GEAR_DOWN, "GEAR_DOWN"),
        ] {
            let event_name = CString::new(name).expect("static CString");
            // SAFETY: handle and string pointer are valid.
            check("SimConnect_MapClientEventToSimEvent", unsafe {
                SimConnect_MapClientEventToSimEvent(self.handle, event_id, event_name.as_ptr())
            })?;
        }

        for surface in [
            Surface::AileronLeft,
            Surface::AileronRight,
            Surface::Elevator,
            Surface::Rudder,
            Surface::FlapsLeft,
            Surface::FlapsRight,
            Surface::Spoilers,
        ] {
            let (definition, name, units) = surface_definition(surface);
            self.add_definition(definition, name, units, DATATYPE_FLOAT64)?;
        }

        for (name, units) in [
            ("VELOCITY BODY X", "meters per second"),
            ("VELOCITY BODY Y", "meters per second"),
            ("VELOCITY BODY Z", "meters per second"),
            ("ROTATION VELOCITY BODY X", "radians per second"),
            ("ROTATION VELOCITY BODY Y", "radians per second"),
            ("ROTATION VELOCITY BODY Z", "radians per second"),
        ] {
            self.add_definition(DEFINE_AIRDATA, name, units, DATATYPE_FLOAT64)?;
        }

        self.add_definition(
            DEFINE_ENGINE_COUNT,
            "NUMBER OF ENGINES",
            "Number",
            DATATYPE_INT32,
        )?;
        for index in 1..=4 {
            self.add_definition(
                engine_definition(index),
                &format!("GENERAL ENG THROTTLE LEVER POSITION:{index}"),
                "Percent Over 100",
                DATATYPE_FLOAT64,
            )?;
        }
        Ok(())
    }

    fn add_definition(
        &self,
        definition: u32,
        name: &str,
        units: &str,
        data_type: i32,
    ) -> Result<(), SimConnectError> {
        let name = CString::new(name).expect("SimVar name has no NUL");
        let units = CString::new(units).expect("unit name has no NUL");
        // SAFETY: handle and strings are valid; scalar arguments match SimConnect.h.
        check("SimConnect_AddToDataDefinition", unsafe {
            SimConnect_AddToDataDefinition(
                self.handle,
                definition,
                name.as_ptr(),
                units.as_ptr(),
                data_type,
                0.0,
                UNUSED,
            )
        })
    }

    fn read_original_freeze(&mut self, timeout: Duration) -> Result<[bool; 3], SimConnectError> {
        // SAFETY: handle is valid and IDs match the previously registered definition.
        check("SimConnect_RequestDataOnSimObject", unsafe {
            SimConnect_RequestDataOnSimObject(
                self.handle,
                REQUEST_FREEZE_STATE,
                DEFINE_FREEZE_STATE,
                OBJECT_USER,
                PERIOD_ONCE,
                0,
                0,
                0,
                0,
            )
        })?;

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Dispatch::FreezeState(values) = self.next_dispatch()? {
                return Ok(values);
            }
            thread::sleep(Duration::from_millis(10));
        }
        tracing::warn!(
            target: "fly_ruler_proto_msfs.bridge",
            "timed out reading MSFS freeze state; assuming all axes unfrozen"
        );
        Ok([false; 3])
    }

    fn read_engine_count(&mut self, timeout: Duration) -> Result<u32, SimConnectError> {
        check("SimConnect_RequestDataOnSimObject(engine_count)", unsafe {
            SimConnect_RequestDataOnSimObject(
                self.handle,
                REQUEST_ENGINE_COUNT,
                DEFINE_ENGINE_COUNT,
                OBJECT_USER,
                PERIOD_ONCE,
                0,
                0,
                0,
                0,
            )
        })?;

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Dispatch::EngineCount(value) = self.next_dispatch()? {
                let value = value.min(4);
                tracing::info!(
                    target: "fly_ruler_proto_msfs.bridge",
                    engine_count = value,
                    "MSFS engine count detected; bridge supports up to 4"
                );
                return Ok(value);
            }
            thread::sleep(Duration::from_millis(10));
        }
        tracing::warn!(
            target: "fly_ruler_proto_msfs.bridge",
            "timed out reading MSFS engine count; assuming 4 engines"
        );
        Ok(4)
    }

    pub fn pump(&mut self) -> Result<(), SimConnectError> {
        loop {
            match self.next_dispatch()? {
                Dispatch::Empty => return Ok(()),
                Dispatch::AssignedObject {
                    request_id,
                    object_id,
                } => self.assigned_objects.push_back((request_id, object_id)),
                Dispatch::Exception(exception) => self.exceptions.push_back(exception),
                Dispatch::Other | Dispatch::FreezeState(_) | Dispatch::EngineCount(_) => {}
            }
        }
    }

    pub fn drain_exceptions(&mut self) -> Vec<SimConnectException> {
        self.exceptions.drain(..).collect()
    }

    pub fn take_assigned_object(&mut self, request_id: u32) -> Option<MsfsObjectId> {
        let index = self
            .assigned_objects
            .iter()
            .position(|(candidate, _)| *candidate == request_id)?;
        self.assigned_objects
            .remove(index)
            .map(|(_, object_id)| object_id)
    }

    pub fn create_ai_aircraft(
        &mut self,
        container_title: &str,
        tail_number: &str,
        pose: MsfsPose,
        airspeed_knots: Option<f64>,
    ) -> Result<AiCreateRequest, SimConnectError> {
        let request_id = self.next_ai_request_id;
        self.next_ai_request_id = self.next_ai_request_id.wrapping_add(1).max(REQUEST_AI_BASE);
        let title = CString::new(container_title).expect("aircraft title has no NUL");
        let livery = CString::new("").expect("empty CString");
        let tail_number = CString::new(tail_number).expect("tail number has no NUL");
        let init_position = InitPosition {
            latitude: pose.latitude_deg,
            longitude: pose.longitude_deg,
            altitude: pose.altitude_m * 3.280_839_895,
            pitch: pose.pitch_rad.to_degrees(),
            bank: pose.bank_rad.to_degrees(),
            heading: pose.heading_true_rad.to_degrees(),
            on_ground: 0,
            airspeed: init_airspeed(airspeed_knots),
        };
        check("SimConnect_AICreateNonATCAircraft_EX1", unsafe {
            SimConnect_AICreateNonATCAircraft_EX1(
                self.handle,
                title.as_ptr(),
                livery.as_ptr(),
                tail_number.as_ptr(),
                init_position,
                request_id,
            )
        })?;
        let send_id = self.last_sent_packet_id()?;
        Ok(AiCreateRequest {
            request_id,
            send_id,
        })
    }

    pub fn release_ai_control(&self, object_id: MsfsObjectId) -> Result<(), SimConnectError> {
        check("SimConnect_AIReleaseControl", unsafe {
            SimConnect_AIReleaseControl(self.handle, object_id, UNUSED)
        })
    }

    pub fn remove_ai_object(&self, object_id: MsfsObjectId) -> Result<(), SimConnectError> {
        check("SimConnect_AIRemoveObject", unsafe {
            SimConnect_AIRemoveObject(self.handle, object_id, UNUSED)
        })
    }

    pub fn set_frozen_for_object(
        &mut self,
        object_id: MsfsObjectId,
        frozen: bool,
    ) -> Result<(), SimConnectError> {
        let values = if object_id == OBJECT_USER && !frozen {
            self.original_freeze
        } else {
            [frozen; 3]
        };
        for (event_id, value) in [
            (EVENT_FREEZE_LAT_LON, values[0]),
            (EVENT_FREEZE_ALTITUDE, values[1]),
            (EVENT_FREEZE_ATTITUDE, values[2]),
        ] {
            self.transmit_freeze_for_object(object_id, event_id, value)?;
        }
        if object_id == OBJECT_USER {
            self.acquired_freeze = frozen;
        }
        Ok(())
    }

    pub fn set_pose_for_object(
        &self,
        object_id: MsfsObjectId,
        pose: MsfsPose,
    ) -> Result<(), SimConnectError> {
        // SAFETY: MsfsPose is repr(C), fully initialized, and exactly matches DEFINE_POSE.
        check("SimConnect_SetDataOnSimObject(pose)", unsafe {
            SimConnect_SetDataOnSimObject(
                self.handle,
                DEFINE_POSE,
                object_id,
                0,
                0,
                mem::size_of::<MsfsPose>() as u32,
                (&pose as *const MsfsPose).cast(),
            )
        })
    }

    pub fn set_surface_for_object(
        &self,
        object_id: MsfsObjectId,
        surface: Surface,
        value: f64,
    ) -> Result<(), SimConnectError> {
        let (definition, _, _) = surface_definition(surface);
        // SAFETY: a single f64 exactly matches each surface data definition.
        check("SimConnect_SetDataOnSimObject(surface)", unsafe {
            SimConnect_SetDataOnSimObject(
                self.handle,
                definition,
                object_id,
                0,
                0,
                mem::size_of::<f64>() as u32,
                (&value as *const f64).cast(),
            )
        })
    }

    pub fn set_airdata_for_object(
        &self,
        object_id: MsfsObjectId,
        airdata: MsfsAirData,
    ) -> Result<(), SimConnectError> {
        // SAFETY: MsfsAirData is repr(C), fully initialized, and matches DEFINE_AIRDATA.
        check("SimConnect_SetDataOnSimObject(airdata)", unsafe {
            SimConnect_SetDataOnSimObject(
                self.handle,
                DEFINE_AIRDATA,
                object_id,
                0,
                0,
                mem::size_of::<MsfsAirData>() as u32,
                (&airdata as *const MsfsAirData).cast(),
            )
        })
    }

    pub fn set_engine_throttle_for_object(
        &mut self,
        object_id: MsfsObjectId,
        index: u32,
        ratio: f64,
    ) -> Result<(), SimConnectError> {
        if index == 0 || index > 4 {
            return Ok(());
        }
        if object_id == OBJECT_USER && index > self.engine_count {
            let reported = &mut self.reported_unsupported_engines[index as usize - 1];
            if !*reported {
                tracing::warn!(
                    target: "fly_ruler_proto_msfs.bridge",
                    engine_index = index,
                    engine_count = self.engine_count,
                    "ignoring throttle for engine not present on current aircraft"
                );
                *reported = true;
            }
            return Ok(());
        }
        check("SimConnect_SetDataOnSimObject(engine_throttle)", unsafe {
            SimConnect_SetDataOnSimObject(
                self.handle,
                engine_definition(index),
                object_id,
                0,
                0,
                mem::size_of::<f64>() as u32,
                (&ratio as *const f64).cast(),
            )
        })
    }

    pub fn set_landing_gear_for_object(
        &self,
        object_id: MsfsObjectId,
        command: GearCommand,
    ) -> Result<(), SimConnectError> {
        let event_id = match command {
            GearCommand::Up => EVENT_GEAR_UP,
            GearCommand::Down => EVENT_GEAR_DOWN,
        };
        self.transmit_event_for_object(object_id, event_id, 0)
    }

    fn next_dispatch(&mut self) -> Result<Dispatch, SimConnectError> {
        let mut data = ptr::null();
        let mut bytes = 0_u32;
        // SAFETY: out-pointers are valid and SimConnect owns the returned message.
        let result = unsafe { SimConnect_GetNextDispatch(self.handle, &mut data, &mut bytes) };
        if result != S_OK || data.is_null() {
            return Ok(Dispatch::Empty);
        }

        // SAFETY: a successful dispatch always starts with SIMCONNECT_RECV.
        let header = unsafe { ptr::read_unaligned(data) };
        match header.id {
            RECV_ID_EXCEPTION => {
                // SAFETY: message ID determines the concrete SDK structure.
                let exception = unsafe { ptr::read_unaligned(data.cast::<RecvException>()) };
                Ok(Dispatch::Exception(SimConnectException {
                    exception: exception.exception,
                    send_id: exception.send_id,
                    index: exception.index,
                }))
            }
            RECV_ID_QUIT => Err(SimConnectError::Disconnected),
            RECV_ID_ASSIGNED_OBJECT_ID => {
                // SAFETY: message ID determines the concrete SDK structure.
                let assigned = unsafe { ptr::read_unaligned(data.cast::<RecvAssignedObjectId>()) };
                Ok(Dispatch::AssignedObject {
                    request_id: assigned.request_id,
                    object_id: assigned.object_id,
                })
            }
            RECV_ID_SIMOBJECT_DATA => {
                // SAFETY: every SIMOBJECT_DATA response begins with this fixed header.
                let message = unsafe { ptr::read_unaligned(data.cast::<RecvSimObjectData>()) };
                match message.request_id {
                    REQUEST_FREEZE_STATE if message.define_count == 3 => {
                        // SAFETY: request and count determine the three-value response layout.
                        let freeze = unsafe { ptr::read_unaligned(data.cast::<RecvFreezeData>()) };
                        Ok(Dispatch::FreezeState(freeze.values.map(|value| value != 0)))
                    }
                    REQUEST_ENGINE_COUNT => {
                        // SAFETY: request ID determines the one-value response layout.
                        let engines =
                            unsafe { ptr::read_unaligned(data.cast::<RecvEngineCount>()) };
                        Ok(Dispatch::EngineCount(engines.value.max(0) as u32))
                    }
                    _ => Ok(Dispatch::Other),
                }
            }
            _ => Ok(Dispatch::Other),
        }
    }

    fn transmit_freeze_for_object(
        &self,
        object_id: MsfsObjectId,
        event_id: u32,
        value: bool,
    ) -> Result<(), SimConnectError> {
        self.transmit_event_for_object(object_id, event_id, u32::from(value))
    }

    fn transmit_event_for_object(
        &self,
        object_id: MsfsObjectId,
        event_id: u32,
        data: u32,
    ) -> Result<(), SimConnectError> {
        // SAFETY: handle is valid and event ID was mapped during configuration.
        check("SimConnect_TransmitClientEvent", unsafe {
            SimConnect_TransmitClientEvent(
                self.handle,
                object_id,
                event_id,
                data,
                GROUP_PRIORITY_HIGHEST,
                EVENT_FLAG_GROUP_IS_PRIORITY,
            )
        })
    }

    fn last_sent_packet_id(&self) -> Result<u32, SimConnectError> {
        let mut send_id = 0;
        check("SimConnect_GetLastSentPacketID", unsafe {
            SimConnect_GetLastSentPacketID(self.handle, &mut send_id)
        })?;
        Ok(send_id)
    }
}

impl Simulator for SimConnectClient {
    type Error = SimConnectError;

    fn set_frozen(&mut self, frozen: bool) -> Result<(), Self::Error> {
        self.set_frozen_for_object(OBJECT_USER, frozen)
    }

    fn set_pose(&mut self, pose: MsfsPose) -> Result<(), Self::Error> {
        self.set_pose_for_object(OBJECT_USER, pose)
    }

    fn set_surface(&mut self, surface: Surface, value: f64) -> Result<(), Self::Error> {
        self.set_surface_for_object(OBJECT_USER, surface, value)
    }

    fn set_airdata(&mut self, airdata: MsfsAirData) -> Result<(), Self::Error> {
        self.set_airdata_for_object(OBJECT_USER, airdata)
    }

    fn set_engine_throttle(&mut self, index: u32, ratio: f64) -> Result<(), Self::Error> {
        self.set_engine_throttle_for_object(OBJECT_USER, index, ratio)
    }

    fn set_landing_gear(&mut self, command: GearCommand) -> Result<(), Self::Error> {
        self.set_landing_gear_for_object(OBJECT_USER, command)
    }
}

impl Drop for SimConnectClient {
    fn drop(&mut self) {
        if self.acquired_freeze {
            for (event_id, value) in [
                (EVENT_FREEZE_LAT_LON, self.original_freeze[0]),
                (EVENT_FREEZE_ALTITUDE, self.original_freeze[1]),
                (EVENT_FREEZE_ATTITUDE, self.original_freeze[2]),
            ] {
                let _ = self.transmit_freeze_for_object(OBJECT_USER, event_id, value);
            }
        }
        // SAFETY: this object exclusively owns the handle.
        unsafe {
            SimConnect_Close(self.handle);
        }
    }
}

fn surface_definition(surface: Surface) -> (u32, &'static str, &'static str) {
    match surface {
        Surface::AileronLeft => (10, "AILERON LEFT DEFLECTION", "radians"),
        Surface::AileronRight => (11, "AILERON RIGHT DEFLECTION", "radians"),
        Surface::Elevator => (12, "ELEVATOR DEFLECTION", "radians"),
        Surface::Rudder => (13, "RUDDER DEFLECTION", "radians"),
        Surface::FlapsLeft => (14, "TRAILING EDGE FLAPS LEFT PERCENT", "Percent Over 100"),
        Surface::FlapsRight => (15, "TRAILING EDGE FLAPS RIGHT PERCENT", "Percent Over 100"),
        Surface::Spoilers => (16, "SPOILERS HANDLE POSITION", "Percent Over 100"),
    }
}

fn engine_definition(index: u32) -> u32 {
    20 + index
}

fn init_airspeed(airspeed_knots: Option<f64>) -> u32 {
    airspeed_knots
        .filter(|value| value.is_finite() && *value > 1.0)
        .map(|value| value.round().clamp(1.0, u32::MAX as f64 - 1.0) as u32)
        // SIMCONNECT_DATA_INITPOSITION supports -1 for design cruise speed.
        .unwrap_or(u32::MAX)
}

fn check(operation: &'static str, result: HResult) -> Result<(), SimConnectError> {
    if result >= 0 {
        Ok(())
    } else {
        Err(SimConnectError::Call {
            operation,
            code: result as u32,
        })
    }
}
