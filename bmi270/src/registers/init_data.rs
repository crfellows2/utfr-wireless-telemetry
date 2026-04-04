// use bytemuck::{Pod, Zeroable};

// use crate::{bmi270::Bmi270Register, bmi_conf::BMI270_CONFIG_BUF};

// #[derive(Clone, PartialEq, Eq, core::fmt::Debug, defmt::Format, Copy, PartialOrd, Ord, Hash)]
// pub struct InitData {
//     data: [u8; 8192],
// }

// pub struct InitDataBitfield {
//     data: [u8; 8192],
// }

// impl Default for InitDataBitfield {
//     fn default() -> Self {
//         Self {
//             data: BMI270_CONFIG_BUF,
//         }
//     }
// }

// impl InitData {
//     pub fn read_data(&self) -> [u8; 8192] {
//         self.data
//     }

//     pub fn new(value: InitDataBitfield) -> Self {
//         Self {
//             data: value.into_bytes(),
//         }
//     }
// }

// impl InitDataBitfield {
//     pub fn into_bytes(self) -> [u8; 8192] {
//         self.data
//     }

//     pub fn from_bytes(value: [u8; 8192]) -> Self {
//         Self { data: value }
//     }
// }

// impl Default for InitData {
//     fn default() -> Self {
//         Self {
//             data: InitDataBitfield::default().into_bytes(),
//         }
//     }
// }

// impl AsRef<InitDataBitfield> for InitDataBitfield {
//     #[inline]
//     fn as_ref(&self) -> &InitDataBitfield {
//         self
//     }
// }

// impl AsRef<InitData> for InitData {
//     #[inline]
//     fn as_ref(&self) -> &InitData {
//         self
//     }
// }

// impl From<&InitData> for InitDataBitfield {
//     #[inline]
//     fn from(value: &InitData) -> Self {
//         InitDataBitfield::from_bytes(value.data)
//     }
// }

// unsafe impl Pod for InitData {}

// unsafe impl Zeroable for InitData {}

// impl embedded_registers::Register for InitData {
//     type Bitfield = InitDataBitfield;
//     type I2cCodec = embedded_registers::i2c::codecs::NoCodec;

//     const REGISTER_SIZE: usize = 8192;
//     const ADDRESS: u64 = 0x5E;

//     #[inline]
//     fn data(&self) -> &[u8] {
//         &self.data
//     }

//     #[inline]
//     fn data_mut(&mut self) -> &mut [u8] {
//         &mut self.data
//     }
// }

// impl embedded_registers::ReadableRegister for InitData {}

// impl embedded_registers::WritableRegister for InitData {}

// impl Bmi270Register for InitData {}
