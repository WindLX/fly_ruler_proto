use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use fly_ruler_proto_core::pb;
use fly_ruler_proto_core::store::{aircraft_count_for, event_count_for, state_count_for};
use fly_ruler_proto_core::{
    Client, KernelRuntime, LoggingConfig, RuntimeConfig, TimeSeriesStore, TransportConfig,
};

fn uuid(seed: u8) -> pb::Uuid {
    pb::Uuid {
        value: vec![seed; 16],
    }
}

fn handshake_message(client_uuid: pb::Uuid) -> pb::Message {
    handshake_message_with_version(client_uuid, "0.2.4")
}

fn handshake_message_with_version(client_uuid: pb::Uuid, version: &str) -> pb::Message {
    pb::Message {
        envelope: Some(pb::message::Envelope::Request(pb::Request {
            id: Some(uuid(0x10)),
            timestamp: 1.0,
            command: Some(pb::RequestCommand {
                kind: Some(pb::request_command::Kind::Handshake(pb::Handshake {
                    version: version.to_string(),
                    client_uuid: Some(client_uuid),
                })),
            }),
        })),
    }
}

fn state(x: f64) -> pb::AircraftState {
    pb::AircraftState {
        position: Some(pb::Vector3 { x, y: 0.0, z: 0.0 }),
        velocity: None,
        attitude: None,
        angular_velocity: None,
        derived: None,
        control_surfaces: None,
        engines: vec![],
        custom_fields: vec![pb::CustomField {
            field_id: "throttle".to_string(),
            value: Some(pb::FieldValue {
                kind: Some(pb::field_value::Kind::F64Value(0.75)),
            }),
        }],
    }
}

fn spawn_message(aircraft_id: pb::Uuid) -> pb::Message {
    pb::Message {
        envelope: Some(pb::message::Envelope::Request(pb::Request {
            id: Some(uuid(0x11)),
            timestamp: 2.0,
            command: Some(pb::RequestCommand {
                kind: Some(pb::request_command::Kind::AircraftEvent(
                    pb::AircraftEvent {
                        aircraft_id: Some(aircraft_id),
                        info: Some(pb::AircraftCommandInfo {
                            kind: Some(pb::aircraft_command_info::Kind::Spawn(
                                pb::AircraftSpawnInfo {
                                    name: "F-15".to_string(),
                                    toml_config: "[aircraft]\nname='F-15'".to_string(),
                                    initial_state: Some(state(1.0)),
                                },
                            )),
                        }),
                    },
                )),
            }),
        })),
    }
}

fn state_update_message(aircraft_id: pb::Uuid, timestamp: f64, x: f64) -> pb::Message {
    pb::Message {
        envelope: Some(pb::message::Envelope::Request(pb::Request {
            id: Some(uuid(0x13)),
            timestamp,
            command: Some(pb::RequestCommand {
                kind: Some(pb::request_command::Kind::AircraftEvent(
                    pb::AircraftEvent {
                        aircraft_id: Some(aircraft_id),
                        info: Some(pb::AircraftCommandInfo {
                            kind: Some(pb::aircraft_command_info::Kind::StateUpdate(state(x))),
                        }),
                    },
                )),
            }),
        })),
    }
}

fn custom_event_message(aircraft_id: pb::Uuid, timestamp: f64, name: &str) -> pb::Message {
    pb::Message {
        envelope: Some(pb::message::Envelope::Request(pb::Request {
            id: Some(uuid(0x14)),
            timestamp,
            command: Some(pb::RequestCommand {
                kind: Some(pb::request_command::Kind::AircraftEvent(
                    pb::AircraftEvent {
                        aircraft_id: Some(aircraft_id),
                        info: Some(pb::AircraftCommandInfo {
                            kind: Some(pb::aircraft_command_info::Kind::CustomEvent(
                                name.to_string(),
                            )),
                        }),
                    },
                )),
            }),
        })),
    }
}

fn despawn_message(aircraft_id: pb::Uuid, timestamp: f64) -> pb::Message {
    pb::Message {
        envelope: Some(pb::message::Envelope::Request(pb::Request {
            id: Some(uuid(0x15)),
            timestamp,
            command: Some(pb::RequestCommand {
                kind: Some(pb::request_command::Kind::AircraftEvent(
                    pb::AircraftEvent {
                        aircraft_id: Some(aircraft_id),
                        info: Some(pb::AircraftCommandInfo {
                            kind: Some(pb::aircraft_command_info::Kind::Despawn(pb::DespawnInfo {
                                reason: Some("test".to_string()),
                            })),
                        }),
                    },
                )),
            }),
        })),
    }
}

fn heartbeat_message(seq: u64, client_uuid: pb::Uuid) -> pb::Message {
    pb::Message {
        envelope: Some(pb::message::Envelope::Request(pb::Request {
            id: Some(uuid(0x12)),
            timestamp: 3.0,
            command: Some(pb::RequestCommand {
                kind: Some(pb::request_command::Kind::Heartbeat(pb::Heartbeat {
                    seq_num: seq,
                    client_uuid: Some(client_uuid),
                })),
            }),
        })),
    }
}

fn assert_ack(msg: pb::Message) {
    let Some(pb::message::Envelope::Response(resp)) = msg.envelope else {
        panic!("expected response envelope");
    };
    assert!(matches!(
        resp.result,
        Some(pb::response::Result::Ok(pb::ResponseData {
            kind: Some(pb::response_data::Kind::Ack(true))
        }))
    ));
}

#[tokio::test]
async fn udp_runtime_ingest_and_session_visibility() {
    let store = Arc::new(TimeSeriesStore::new());
    let mut runtime = KernelRuntime::new(Arc::clone(&store));
    runtime.start_server("127.0.0.1:0").await.unwrap();
    assert!(runtime.active_sessions().await.is_empty());

    let server_addr = runtime.udp_local_addr().unwrap();
    let mut client = Client::connect(&server_addr.to_string(), &LoggingConfig::default())
        .await
        .unwrap();

    let client_uuid = uuid(0xaa);
    let aircraft_uuid = uuid(0xbb);

    client
        .send(handshake_message(client_uuid.clone()))
        .await
        .unwrap();
    assert_ack(client.recv().await.unwrap().unwrap());

    client
        .send(spawn_message(aircraft_uuid.clone()))
        .await
        .unwrap();
    client
        .send(state_update_message(aircraft_uuid.clone(), 4.0, 4.0))
        .await
        .unwrap();
    client
        .send(custom_event_message(
            aircraft_uuid.clone(),
            5.0,
            "hud_mode_changed",
        ))
        .await
        .unwrap();
    client
        .send(despawn_message(aircraft_uuid.clone(), 6.0))
        .await
        .unwrap();

    client
        .send(heartbeat_message(1, client_uuid.clone()))
        .await
        .unwrap();
    assert_ack(client.recv().await.unwrap().unwrap());

    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    assert_eq!(aircraft_count_for(&store), 1);
    assert_eq!(state_count_for(&store), 2);
    assert_eq!(event_count_for(&store), 3);

    let aircraft_id = "bb".repeat(16);
    let latest = store.get_latest(&aircraft_id).unwrap();
    assert_eq!(latest.timestamp_secs, 4.0);
    assert_eq!(latest.state.custom_fields.len(), 1);

    let events = store.get_events_range(&aircraft_id, 0.0, 10.0).unwrap();
    assert!(matches!(
        events[0].event,
        fly_ruler_proto_core::Event::Spawn(_)
    ));
    assert!(matches!(
        events[1].event,
        fly_ruler_proto_core::Event::Custom(_)
    ));
    assert!(matches!(
        events[2].event,
        fly_ruler_proto_core::Event::Despawn(_)
    ));

    let sessions = runtime.active_sessions().await;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].client_uuid_hex.len(), 32);

    runtime.stop_server().await;
}

#[tokio::test]
async fn protocol_version_mismatch_returns_error_response() {
    let store = Arc::new(TimeSeriesStore::new());
    let mut runtime = KernelRuntime::new(store);
    runtime.start_server("127.0.0.1:0").await.unwrap();

    let server_addr = runtime.udp_local_addr().unwrap();
    let mut client = Client::connect(&server_addr.to_string(), &LoggingConfig::default())
        .await
        .unwrap();

    client
        .send(handshake_message_with_version(uuid(0xcc), "0.0.0"))
        .await
        .unwrap();
    let msg = client.recv().await.unwrap().unwrap();
    let Some(pb::message::Envelope::Response(resp)) = msg.envelope else {
        panic!("expected response envelope");
    };
    let Some(pb::response::Result::Err(err)) = resp.result else {
        panic!("expected error result");
    };
    assert_eq!(err.code, pb::ErrorCode::ProtocolVersionMismatch as i32);
    assert!(runtime.active_sessions().await.is_empty());

    runtime.stop_server().await;
}

#[tokio::test]
async fn inactive_session_expires_after_timeout() {
    let store = Arc::new(TimeSeriesStore::new());
    let config = RuntimeConfig {
        transport: TransportConfig {
            heartbeat_interval_secs: 1,
            heartbeat_timeout_secs: 1,
        },
        ..RuntimeConfig::default()
    };
    let mut runtime = KernelRuntime::with_config(store, config);
    runtime.start_server("127.0.0.1:0").await.unwrap();

    let server_addr = runtime.udp_local_addr().unwrap();
    let mut client = Client::connect(&server_addr.to_string(), &LoggingConfig::default())
        .await
        .unwrap();
    client.send(handshake_message(uuid(0xdd))).await.unwrap();
    assert_ack(client.recv().await.unwrap().unwrap());
    assert_eq!(runtime.active_sessions().await.len(), 1);

    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;

    let mut second_client = Client::connect(&server_addr.to_string(), &LoggingConfig::default())
        .await
        .unwrap();
    second_client
        .send(handshake_message(uuid(0xee)))
        .await
        .unwrap();
    assert_ack(second_client.recv().await.unwrap().unwrap());

    let sessions = runtime.active_sessions().await;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].client_uuid_hex, "ee".repeat(16));

    runtime.stop_server().await;
}

#[test]
fn explicit_save_load_flow_through_public_api() {
    let store = Arc::new(TimeSeriesStore::new());
    let runtime = KernelRuntime::new(Arc::clone(&store));

    store.append_event(
        "a1".to_string(),
        1.0,
        fly_ruler_proto_core::Event::Custom("e".to_string()),
    );

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("fly_ruler_integration_{nanos}"));

    runtime.save_session(&dir).unwrap();
    store.append_event(
        "transient".to_string(),
        2.0,
        fly_ruler_proto_core::Event::Custom("drop_me".to_string()),
    );
    assert_eq!(event_count_for(&store), 2);

    runtime.clear_session();
    assert_eq!(event_count_for(&store), 0);

    runtime.load_session(&dir).unwrap();
    assert_eq!(event_count_for(&store), 1);
    assert_eq!(store.get_aircraft_ids(), vec!["a1".to_string()]);

    let _ = std::fs::remove_dir_all(dir);
}
