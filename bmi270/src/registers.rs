use std::{f32::consts::PI, fmt::Display};

use crate::bmi270::Bmi270Register;
use bondrewd::BitfieldEnum;
use defmt::Format;
use embedded_devices_derive::device_register;
use embedded_registers::register;

use crate::{
    types::Error,
    units::{AccelerationUnit, AngularVelocityUnit},
};

mod init_data;

/// Chip ID
#[device_register(Bmi270)]
#[register(address = 0x00, mode = "r")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 1)]
pub struct ChipId {
    id: u8,
}

impl ChipId {
    pub const EXPECTED: u8 = 0x24;

    pub fn ok(self) -> bool {
        self.read_id() == Self::EXPECTED
    }
}

impl Display for ChipId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:X}", self.read_id())
    }
}

#[derive(Debug, Default)]
pub struct Vec3<T> {
    pub x: T,
    pub y: T,
    pub z: T,
}

#[device_register(Bmi270)]
#[register(address = 0x0C, mode = "r")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 6)]
pub struct RawAccelerometerData {
    az: i16,
    ay: i16,
    ax: i16,
}

impl RawAccelerometerData {
    const EARTH_GRAVITY: f32 = 9.80665;

    const fn lsb_to_gs(val: i16, range: AccelerometerRange) -> f32 {
        const ADC_RESOLUTION: u32 = 65536;
        let lsb_per_mps2 = ADC_RESOLUTION / (2 * range.to_value() as u32);
        val as f32 / lsb_per_mps2 as f32
    }

    const fn lsb_to_mps2(val: i16, range: AccelerometerRange) -> f32 {
        Self::EARTH_GRAVITY * Self::lsb_to_gs(val, range)
    }

    const fn lsb_to_unit(val: i16, units: AccelerationUnit, range: AccelerometerRange) -> f32 {
        match units {
            AccelerationUnit::Gs => Self::lsb_to_gs(val, range),
            AccelerationUnit::Mps2 => Self::lsb_to_mps2(val, range),
        }
    }

    pub fn to_processed(
        self,
        units: AccelerationUnit,
        range: AccelerometerRange,
    ) -> AccelerometerData {
        let RawAccelerometerDataBitfield { ax, ay, az } = self.read_all();
        AccelerometerData {
            az: Self::lsb_to_unit(ax, units, range), // let z = x
            ay: Self::lsb_to_unit(-az, units, range), // let y = -z
            ax: Self::lsb_to_unit(-ay, units, range), // let x = -y
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct AccelerometerData {
    pub az: f32,
    pub ay: f32,
    pub ax: f32,
}

#[device_register(Bmi270)]
#[register(address = 0x12, mode = "r")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 6)]
pub struct RawGyroscopeData {
    gz: i16,
    gy: i16,
    gx: i16,
}

impl RawGyroscopeData {
    const fn lsb_to_dps(val: i16, range: GyroscopeRange) -> f32 {
        const ADC_RESOLUTION: u32 = 65536;
        let lsb_per_dps = ADC_RESOLUTION / (2 * range.to_value());
        val as f32 / lsb_per_dps as f32
    }

    const fn lsb_to_radps(val: i16, range: GyroscopeRange) -> f32 {
        Self::lsb_to_dps(val, range) * (PI / 180.0)
    }

    const fn lsb_to_unit(val: i16, units: AngularVelocityUnit, range: GyroscopeRange) -> f32 {
        match units {
            AngularVelocityUnit::Dps => Self::lsb_to_dps(val, range),
            AngularVelocityUnit::Radps => Self::lsb_to_radps(val, range),
        }
    }

    pub fn to_processed(self, units: AngularVelocityUnit, range: GyroscopeRange) -> GyroscopeData {
        let RawGyroscopeDataBitfield { gx, gy, gz } = self.read_all();
        GyroscopeData {
            gz: Self::lsb_to_unit(gx, units, range), // let z = x
            gy: Self::lsb_to_unit(-gz, units, range), // let y = -z
            gx: Self::lsb_to_unit(-gy, units, range), // let x = -y
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct GyroscopeData {
    pub gz: f32,
    pub gy: f32,
    pub gx: f32,
}

#[derive(
    BitfieldEnum, Format, Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[bondrewd_enum(u8)]
pub enum InternalStatusMessage {
    #[default]
    NotInit = 0x0,
    InitOk = 0x1,
    InitErr = 0x2,
    InvalidDriver = 0x3,
    SensorStopped = 0x4,
    NvmError = 0x5,
    StartupError = 0x6,
    CompatError = 0x7,
}

/// Error bits and message indicating internal status
#[device_register(Bmi270)]
#[register(address = 0x21, mode = "r")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 1)]
pub struct InternalStatus {
    #[bondrewd(bit_length = 1, reserve)]
    #[allow(dead_code)]
    reserved1: bool,
    odr_50hz_error: bool,
    axes_remap_error: bool,
    #[bondrewd(bit_length = 1, reserve)]
    #[allow(dead_code)]
    reserved0: bool,
    #[bondrewd(enum_primitive = "u8", bit_length = 4)]
    message: InternalStatusMessage,
}

pub enum SensorKind {
    Aux,
    Accelerometer,
    Gyroscope,
}

#[derive(
    BitfieldEnum, Format, Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[bondrewd_enum(u8)]
pub enum OutputDataRate {
    /// 25/32
    #[default]
    ZeroP78 = 0x1,
    /// 25/16
    OneP5 = 0x2,
    /// 25/8
    ThreeP1 = 0x3,
    /// 25/4
    SixP25 = 0x4,
    /// 25/2
    TwelveP5 = 0x5,
    /// 25
    TwentyFive = 0x6,
    /// 50
    Fifty = 0x7,
    /// 100
    Hundred = 0x8,
    /// 200
    TwoHundred = 0x9,
    /// 400
    FourHundred = 0xA,
    /// 800
    EightHundred = 0xB,
    /// 1600
    OneK6 = 0xC,
    /// 3200
    ThreeK2 = 0xD,
    /// 6400
    SixK4 = 0xE,
    /// 12800
    TwelveK8 = 0xF,
}

impl OutputDataRate {
    const RESERVED_ACCEL: [Self; 3] = [Self::ThreeK2, Self::SixK4, Self::TwelveK8];
    const RESERVED_GYRO: [Self; 7] = [
        Self::ZeroP78,
        Self::OneP5,
        Self::ThreeP1,
        Self::SixP25,
        Self::TwelveP5,
        Self::SixK4,
        Self::TwelveK8,
    ];

    pub const fn reserved(sensor: SensorKind) -> &'static [Self] {
        match sensor {
            SensorKind::Aux => todo!(),
            SensorKind::Accelerometer => &Self::RESERVED_ACCEL,
            SensorKind::Gyroscope => &Self::RESERVED_GYRO,
        }
    }
}

impl From<u8> for OutputDataRate {
    fn from(value: u8) -> Self {
        match value {
            0x1 => Self::ZeroP78,
            0x2 => Self::OneP5,
            0x3 => Self::ThreeP1,
            0x4 => Self::SixP25,
            0x5 => Self::TwelveP5,
            0x6 => Self::TwentyFive,
            0x7 => Self::Fifty,
            0x8 => Self::Hundred,
            0x9 => Self::TwoHundred,
            0xA => Self::FourHundred,
            0xB => Self::EightHundred,
            0xC => Self::OneK6,
            _ => panic!("invalid accelerometer odr"),
        }
    }
}

#[derive(
    BitfieldEnum, Format, Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[bondrewd_enum(u8)]
pub enum AccelerometerBandwidth {
    #[default]
    Osr4Avg1 = 0x0,
    Osr2Avg2 = 0x1,
    NormAvg4 = 0x2,
    CicAvg8 = 0x3,
    ResAvg16 = 0x4,
    ResAvg32 = 0x5,
    ResAvg64 = 0x6,
    ResAvg128 = 0x7,
}

impl From<u8> for AccelerometerBandwidth {
    fn from(value: u8) -> Self {
        match value {
            0x0 => Self::Osr4Avg1,
            0x1 => Self::Osr2Avg2,
            0x2 => Self::NormAvg4,
            0x3 => Self::CicAvg8,
            0x4 => Self::ResAvg16,
            0x5 => Self::ResAvg32,
            0x6 => Self::ResAvg64,
            0x7 => Self::ResAvg128,
            _ => panic!("invalid accelerometer bandwidth"),
        }
    }
}

#[derive(
    BitfieldEnum, Format, Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[bondrewd_enum(u8)]
pub enum PerformanceKind {
    #[default]
    Power = 0x0,
    Performance = 0x1,
}

impl From<u8> for PerformanceKind {
    fn from(value: u8) -> Self {
        match value {
            0x0 => PerformanceKind::Power,
            0x1 => PerformanceKind::Performance,
            _ => panic!("invalid performance kind"),
        }
    }
}

/// Sets the output data rate, the bandwidth, and the read mode of the accelerometer
#[device_register(Bmi270)]
#[register(address = 0x40, mode = "rw")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 1)]
pub struct AccelerometerConfig {
    #[bondrewd(enum_primitive = "u8", bit_length = 1)]
    filter_performance: PerformanceKind,
    #[bondrewd(enum_primitive = "u8", bit_length = 3)]
    bandwidth: AccelerometerBandwidth,
    #[bondrewd(enum_primitive = "u8", bit_length = 4)]
    odr: OutputDataRate,
}

impl AccelerometerConfig {
    pub fn new_checked<E>(
        odr: OutputDataRate,
        bandwidth: AccelerometerBandwidth,
        filter_performance: PerformanceKind,
    ) -> Result<Self, Error<E>> {
        if OutputDataRate::reserved(SensorKind::Accelerometer).contains(&odr) {
            Err(Error::Reserved)
        } else {
            Ok(Self::default()
                .with_odr(odr)
                .with_bandwidth(bandwidth)
                .with_filter_performance(filter_performance))
        }
    }
}

#[derive(
    BitfieldEnum, Format, Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[bondrewd_enum(u8)]
pub enum AccelerometerRange {
    
    TwoG = 0x0,
    #[default]
    FourG = 0x1,
    EightG = 0x2,
    SixteenG = 0x3,
}

#[device_register(Bmi270)]
#[register(address = 0x41, mode = "rw")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 1)]
pub struct AccelerometerRangeData {
    #[bondrewd(bit_length = 6, reserve)]
    #[allow(dead_code)]
    reserved: u8,
    #[bondrewd(enum_primitive = "u8", bit_length = 2)]
    range: AccelerometerRange,
}

impl AccelerometerRange {
    pub const fn to_value(&self) -> u8 {
        match self {
            AccelerometerRange::TwoG => 2,
            AccelerometerRange::FourG => 4,
            AccelerometerRange::EightG => 8,
            AccelerometerRange::SixteenG => 16,
        }
    }
}

#[derive(
    BitfieldEnum, Format, Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[bondrewd_enum(u8)]
pub enum GyroscopeBandwidth {
    #[default]
    Osr4 = 0x0,
    Osr2 = 0x1,
    Norm = 0x2,
}

/// Sets the output data rate, the bandwidth, and the read mode of the accelerometer
#[device_register(Bmi270)]
#[register(address = 0x42, mode = "rw")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 1)]
pub struct GyroscopeConfig {
    #[bondrewd(enum_primitive = "u8", bit_length = 1)]
    filter_performance: PerformanceKind,
    #[bondrewd(enum_primitive = "u8", bit_length = 1)]
    noise_performance: PerformanceKind,
    #[bondrewd(enum_primitive = "u8", bit_length = 2)]
    bandwidth: GyroscopeBandwidth,
    #[bondrewd(enum_primitive = "u8", bit_length = 4)]
    odr: OutputDataRate,
}

impl GyroscopeConfig {
    pub fn new_checked<E>(
        odr: OutputDataRate,
        bandwidth: GyroscopeBandwidth,
        noise_performance: PerformanceKind,
        filter_performance: PerformanceKind,
    ) -> Result<Self, Error<E>> {
        if OutputDataRate::reserved(SensorKind::Gyroscope).contains(&odr) {
            Err(Error::Reserved)
        } else {
            Ok(Self::default()
                .with_odr(odr)
                .with_bandwidth(bandwidth)
                .with_noise_performance(noise_performance)
                .with_filter_performance(filter_performance))
        }
    }
}

#[derive(
    BitfieldEnum, Format, Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[bondrewd_enum(u8)]
pub enum GyroscopeRange {
    #[default]
    TwoK = 0x0,
    OneK = 0x1,
    FiveHundred = 0x2,
    TwoHundredFifty = 0x3,
    HundredTwentyFive = 0x4,
}

impl GyroscopeRange {
    pub const fn to_value(&self) -> u32 {
        match self {
            GyroscopeRange::TwoK => 2000,
            GyroscopeRange::OneK => 1000,
            GyroscopeRange::FiveHundred => 500,
            GyroscopeRange::TwoHundredFifty => 250,
            GyroscopeRange::HundredTwentyFive => 125,
        }
    }
}

#[device_register(Bmi270)]
#[register(address = 0x43, mode = "rw")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 1)]
pub struct GyroscopeRangeData {
    #[bondrewd(bit_length = 4, reserve)]
    #[allow(dead_code)]
    reserved: u8,
    #[bondrewd(enum_primitive = "u8", bit_length = 1)]
    ois_range: OisRange,
    #[bondrewd(enum_primitive = "u8", bit_length = 3)]
    range: GyroscopeRange,
}

#[derive(
    BitfieldEnum, Format, Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[bondrewd_enum(u8)]
pub enum OisRange {
    #[default]
    TwoHundredFifty = 0x0,
    TwoK = 0x1,
}

#[device_register(Bmi270)]
#[register(address = 0x43, mode = "rw")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 1)]
pub struct GyroscopeRangeReg {
    #[bondrewd(bit_length = 4, reserve)]
    #[allow(dead_code)]
    reserved: u8,
    #[bondrewd(enum_primitive = "u8", bit_length = 1)]
    ois_range: OisRange,
    #[bondrewd(enum_primitive = "u8", bit_length = 3)]
    range: GyroscopeRange,
}

/// Start initialization
#[derive(
    BitfieldEnum, Format, Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[bondrewd_enum(u8)]
pub enum InitCtrlCommand {
    #[default]
    Start = 0x0,
    End = 0x1,
}

#[device_register(Bmi270)]
#[register(address = 0x59, mode = "rw")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 1)]
pub struct InitCtrl {
    #[bondrewd(enum_primitive = "u8", bit_length = 8)]
    cmd: InitCtrlCommand,
}

#[device_register(Bmi270)]
#[register(address = 0x5B, mode = "rw")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 2)]
pub struct InitAddr {
    #[bondrewd(bit_length = 8)]
    base4_11: u16,
    #[bondrewd(bit_length = 4, reserve)]
    #[allow(dead_code)]
    reserved: u8,
    #[bondrewd(bit_length = 4)]
    base0_3: u16,
}

#[derive(
    BitfieldEnum, Format, Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[bondrewd_enum(u8)]
pub enum Status {
    #[default]
    Disabled = 0x0,
    Enabled = 0x1,
}

impl From<bool> for Status {
    fn from(value: bool) -> Self {
        if value {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

/// Power mode configuration
#[device_register(Bmi270)]
#[register(address = 0x7C, mode = "rw")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 1)]
pub struct PowerConfig {
    #[bondrewd(bit_length = 5, reserve)]
    #[allow(dead_code)]
    reserved: u8,
    #[bondrewd(enum_primitive = "u8", bit_length = 1)]
    fast_power_up: Status,
    #[bondrewd(enum_primitive = "u8", bit_length = 1)]
    fifo_read: Status,
    #[bondrewd(enum_primitive = "u8", bit_length = 1)]
    advanced_power_save: Status,
}

/// Power mode control
#[device_register(Bmi270)]
#[register(address = 0x7D, mode = "rw")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 1)]
pub struct PowerMode {
    #[bondrewd(bit_length = 4, reserve)]
    #[allow(dead_code)]
    reserved: u8,
    #[bondrewd(enum_primitive = "u8", bit_length = 1)]
    temp: Status,
    #[bondrewd(enum_primitive = "u8", bit_length = 1)]
    accelerometer: Status,
    #[bondrewd(enum_primitive = "u8", bit_length = 1)]
    gyroscope: Status,
    #[bondrewd(enum_primitive = "u8", bit_length = 1)]
    aux: Status,
}

#[derive(
    BitfieldEnum, Format, Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[bondrewd_enum(u8)]
pub enum CommandKind {
    Trigger = 0x02,
    UserGain = 0x03,
    NvmProg = 0xA0,
    #[default]
    FifoFlush = 0xB0,
    SoftReset = 0xB6,
}

#[device_register(Bmi270)]
#[register(address = 0x7E, mode = "w")]
#[derive(Copy, PartialOrd, Ord, Hash)]
#[bondrewd(default_endianness = "le", enforce_bytes = 1)]
pub struct Command {
    #[bondrewd(enum_primitive = "u8", bit_length = 8)]
    cmd: CommandKind,
}
