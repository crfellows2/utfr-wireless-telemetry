use heapless::Vec;
use serde::{Deserialize, Deserializer, Serializer, ser::SerializeSeq};

pub fn serialize<S: Serializer>(bytes: &Vec<u8, 64>, s: S) -> Result<S::Ok, S::Error> {
    if s.is_human_readable() {
        let mut seq = s.serialize_seq(Some(bytes.len()))?;
        for byte in bytes {
            seq.serialize_element(&format!("{byte:02x}"))?;
        }
        seq.end()
    } else {
        serde::Serialize::serialize(&bytes, s)
    }
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8, 64>, D::Error> {
    if d.is_human_readable() {
        let hex_strs = <heapless::Vec<&str, 64>>::deserialize(d)?;
        let mut out = Vec::new();
        for s in hex_strs {
            let byte = u8::from_str_radix(s, 16).map_err(serde::de::Error::custom)?;
            out.push(byte)
                .map_err(|_| serde::de::Error::custom("too many bytes"))?;
        }
        Ok(out)
    } else {
        heapless::Vec::deserialize(d)
    }
}
