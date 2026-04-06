use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize_u16<S: Serializer>(val: &u16, s: S) -> Result<S::Ok, S::Error> {
    if s.is_human_readable() {
        s.serialize_str(&format!("{val:#05x}"))
    } else {
        s.serialize_u16(*val)
    }
}

pub fn deserialize_u16<'de, D: Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
    if d.is_human_readable() {
        let s = <&str>::deserialize(d)?;
        let s = s.trim_start_matches("0x");
        u16::from_str_radix(s, 16).map_err(serde::de::Error::custom)
    } else {
        u16::deserialize(d)
    }
}

pub fn serialize_u32<S: Serializer>(val: &u32, s: S) -> Result<S::Ok, S::Error> {
    if s.is_human_readable() {
        s.serialize_str(&format!("{val:#010x}"))
    } else {
        s.serialize_u32(*val)
    }
}

pub fn deserialize_u32<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    if d.is_human_readable() {
        let s = <&str>::deserialize(d)?;
        let s = s.trim_start_matches("0x");
        u32::from_str_radix(s, 16).map_err(serde::de::Error::custom)
    } else {
        u32::deserialize(d)
    }
}
