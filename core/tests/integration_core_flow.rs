use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use fly_ruler_proto_core::pb;
use fly_ruler_proto_core::store::{aircraft_count_for, event_count_for, state_count_for};
use fly_ruler_proto_core::{Client, KernelRuntime, TimeSeriesStore};

fn uuid(seed: u8) -> pb::Uuid {
    pb::Uuid {
        value: vec![seed; 16],
    }
}

fn handshake_message(client_uuid: pb::Uuid) -> pb::Message {
    pb::Message {
        envelope: Some(pb::message::Envelope::Request(pb::Request {
            id: Some(uuid(0x10)),
            timestamp: 1.0,
            command: Some(pb::RequestCommand {
                kind: Some(pb::request_command::Kind::Handshake(pb::Handshake {
                    version: "1.0.0".to_string(),
                    client_uuid: Some(client_uuid),
                })),
            }),
        })),
    }
}

fn spawn_message(client_uuid: pb::Uuid, aircraft_id: pb::Uuid) -> pb::Message {
    let _ = client_uuid;
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
                                    initial_state: Some(pb::AircraftState {
                                        position: None,
                                        velocity: None,
                                        attitude: None,
                                        angular_velocity: None,
                                        derived: None,
                                        custom_fields: vec![],
                                    }),
                                },
                            )),
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

#[tokio::test]
async fn udp_runtime_ingest_and_session_visibility() {
    let store = Arc::new(TimeSeriesStore::new());
    let mut runtime = KernelRuntime::new(Arc::clone(&store));
    runtime.start_server("127.0.0.1:0").await.unwrap();
    assert!(runtime.active_sessions().await.is_empty());

    let server_addr = runtime.udp_local_addr().unwrap();
    let mut client = Client::connect(&server_addr.to_string()).await.unwrap();

    let client_uuid = uuid(0xaa);
    let aircraft_uuid = uuid(0xbb);

    client
        .send(handshake_message(client_uuid.clone()))
        .await
        .unwrap();
    let _ack1 = client.recv().await.unwrap().unwrap();

    client
        .send(spawn_message(client_uuid.clone(), aircraft_uuid.clone()))
        .await
        .unwrap();

    client
        .send(heartbeat_message(1, client_uuid.clone()))
        .await
        .unwrap();
    let _ack2 = client.recv().await.unwrap().unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    assert_eq!(aircraft_count_for(&store), 1);
    assert!(state_count_for(&store) >= 1);
    assert!(event_count_for(&store) >= 1);

    let sessions = runtime.active_sessions().await;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].client_uuid_hex.len(), 32);

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
    runtime.clear_session();
    assert_eq!(event_count_for(&store), 0);

    runtime.load_session(&dir).unwrap();
    assert_eq!(event_count_for(&store), 1);

    let _ = std::fs::remove_dir_all(dir);
}
