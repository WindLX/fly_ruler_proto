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
}
