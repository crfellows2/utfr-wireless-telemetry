use core::fmt::Write;
use protocol::{CanFrame, CanId};
use std::sync::atomic::AtomicUsize;

pub static TELEMETRY_BYTES: AtomicUsize = AtomicUsize::new(0);

pub fn format_frame(frame: &CanFrame, out: &mut heapless::String<512>) {
    out.push_str(r#"{"id":{"#).ok();
    match frame.id {
        CanId::Standard(id) => write!(out, r#""standard":"0x{:03x}""#, id.value()).ok(),
        CanId::Extended(id) => write!(out, r#""extended":"0x{:08x}""#, id.value()).ok(),
    };
    out.push_str(r#"},"payload":["#).ok();
    for (i, b) in frame.payload.iter().enumerate() {
        if i > 0 {
            out.push(',').ok();
        }
        write!(out, r#""{:02x}""#, b).ok();
    }
    out.push_str("]}\n").ok();
}
