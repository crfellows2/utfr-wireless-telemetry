use heapless::Vec;
use serde::{Deserialize, Serialize};

mod hex_bytes;
mod hex_id;

// --- CAN ID ---

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct StandardId(
    #[serde(
        serialize_with = "hex_id::serialize_u16",
        deserialize_with = "hex_id::deserialize_u16"
    )]
    u16,
);

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct ExtendedId(
    #[serde(
        serialize_with = "hex_id::serialize_u32",
        deserialize_with = "hex_id::deserialize_u32"
    )]
    u32,
);

impl StandardId {
    pub const EXACT_MASK: u16 = 0x7FF;

    pub fn new(id: u16) -> Option<Self> {
        (id <= 0x7FF).then_some(Self(id))
    }

    pub fn value(self) -> u16 {
        self.0
    }
}

impl ExtendedId {
    pub const EXACT_MASK: u32 = 0x1FFF_FFFF;

    pub fn new(id: u32) -> Option<Self> {
        (id <= 0x1FFF_FFFF).then_some(Self(id))
    }

    pub fn value(self) -> u32 {
        self.0
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CanId {
    Standard(StandardId),
    Extended(ExtendedId),
}

// --- CAN Frame ---

#[derive(Serialize, Deserialize, Debug)]
pub struct CanFrame {
    pub id: CanId,
    #[serde(with = "hex_bytes")]
    pub payload: Vec<u8, 64>,
}

// --- Filter ---

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CanFilter {
    Standard {
        id: StandardId,
        #[serde(
            serialize_with = "hex_id::serialize_u16",
            deserialize_with = "hex_id::deserialize_u16"
        )]
        mask: u16,
    },
    Extended {
        id: ExtendedId,
        #[serde(
            serialize_with = "hex_id::serialize_u32",
            deserialize_with = "hex_id::deserialize_u32"
        )]
        mask: u32,
    },
}

// --- Protocol ---

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Command {
    Subscribe(CanFilter),
    Unsubscribe(CanFilter),
    Write(CanFrame),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_json() {
        let mut payload = heapless::Vec::new();
        payload
            .extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF])
            .unwrap();

        let commands = [
            Command::Subscribe(CanFilter::Standard {
                id: StandardId::new(0x123).unwrap(),
                mask: StandardId::EXACT_MASK,
            }),
            Command::Unsubscribe(CanFilter::Extended {
                id: ExtendedId::new(0x18DB33F1).unwrap(),
                mask: ExtendedId::EXACT_MASK,
            }),
            Command::Write(CanFrame {
                id: CanId::Extended(ExtendedId::new(0x18DB33F1).unwrap()),
                payload,
            }),
        ];

        for cmd in &commands {
            println!("{}", serde_json::to_string_pretty(cmd).unwrap());
        }
    }
}
