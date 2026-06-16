use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn uuid_to_hex(uuid: &crate::pb::Uuid) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(uuid.value.len() * 2);
    for b in &uuid.value {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub(crate) fn now_secs() -> f64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(v) => v.as_secs_f64(),
        Err(_) => 0.0,
    }
}
