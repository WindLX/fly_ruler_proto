// Generated protobuf module for the Fly Ruler wire schema.
//
// The generated code is produced by `core/build.rs` from `proto/fly_ruler.proto` at workspace root.

#![allow(clippy::large_enum_variant)]
#![allow(missing_docs)]

include!(concat!(env!("OUT_DIR"), "/flyruler.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message as ProstMessage;

    #[test]
    fn heartbeat_carries_client_uuid_roundtrip() {
        let msg = Message {
            envelope: Some(message::Envelope::Request(Request {
                id: Some(Uuid {
                    value: vec![0x12; 16],
                }),
                timestamp: 1.5,
                command: Some(RequestCommand {
                    kind: Some(request_command::Kind::Heartbeat(Heartbeat {
                        seq_num: 99,
                        client_uuid: Some(Uuid {
                            value: vec![0xab; 16],
                        }),
                    })),
                }),
            })),
        };

        let bytes = msg.encode_to_vec();
        let decoded = Message::decode(bytes.as_slice()).unwrap();
        let Some(message::Envelope::Request(req)) = decoded.envelope else {
            panic!("expected request envelope");
        };
        let Some(request_command::Kind::Heartbeat(hb)) = req.command.and_then(|c| c.kind) else {
            panic!("expected heartbeat command");
        };
        assert_eq!(hb.seq_num, 99);
        assert_eq!(hb.client_uuid.unwrap().value, vec![0xab; 16]);
    }

    #[test]
    fn aircraft_state_new_fields_roundtrip_and_old_payload_defaults() {
        let state = AircraftState {
            derived: Some(DerivedState {
                ias: Some(45.0),
                cas: Some(46.0),
                mach: Some(0.14),
                ..Default::default()
            }),
            control_surfaces: Some(ControlSurfaceState {
                elevator_rad: Some(0.1),
                spoilers_ratio: Some(0.25),
                ..Default::default()
            }),
            engines: vec![
                EngineState {
                    index: 1,
                    throttle_lever_ratio: Some(0.3),
                },
                EngineState {
                    index: 2,
                    throttle_lever_ratio: Some(0.7),
                },
            ],
            ..Default::default()
        };

        let decoded = AircraftState::decode(state.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded.derived.unwrap().mach, Some(0.14));
        assert_eq!(decoded.control_surfaces.unwrap().elevator_rad, Some(0.1));
        assert_eq!(decoded.engines[1].throttle_lever_ratio, Some(0.7));

        // A legacy AircraftState containing only field 1 remains valid.
        let legacy_position_only = [
            0x0a, 0x1b, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f, 0x11, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x19, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08,
            0x40,
        ];
        let legacy = AircraftState::decode(legacy_position_only.as_slice()).unwrap();
        assert_eq!(legacy.position.unwrap().x, 1.0);
        assert!(legacy.control_surfaces.is_none());
        assert!(legacy.engines.is_empty());
    }
}
