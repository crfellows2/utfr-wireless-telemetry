use embedded_devices_derive::{device, device_impl};

// Arnab's imports
use control_loop::ekf::ImuData;
use control_loop::units::{Accel, AccelUnit, Gyro, GyroUnit};
use nalgebra::{Matrix3, Vector3};

use crate::bmi_conf::BMI270_CONFIG_BUF;
use crate::registers::{
    AccelerometerData, AccelerometerRange, AccelerometerRangeData, Command, CommandKind,
    GyroscopeConfig, GyroscopeData, GyroscopeRange, GyroscopeRangeData, InitCtrl, InitCtrlCommand,
    InternalStatus, InternalStatusMessage, OisRange, PowerConfig, PowerMode, RawAccelerometerData,
    RawGyroscopeData, Status,
};

use crate::units::{AccelerationUnit, AngularVelocityUnit};
use crate::{
    registers::{AccelerometerConfig, ChipId},
    types::Error,
};

pub type Bmi270Codec = embedded_registers::spi::codecs::SimpleCodec<1, 7, 0, 7, true, 1>;

/// I2C codec for BMI270 - uses standard 8-bit register addressing
pub type Bmi270I2cCodec = embedded_registers::i2c::codecs::SimpleCodec<1>;

#[device]
#[maybe_async_cfg::maybe(
    idents(
        hal(sync = "embedded_hal", async = "embedded_hal_async"),
        RegisterInterface
    ),
    sync(feature = "sync"),
    async(feature = "async")
)]
pub struct Bmi270<I: embedded_registers::RegisterInterface, D> {
    interface: I,
    delay: D,
    accel_config: Option<AccelerometerConfig>,
    gyro_config: Option<GyroscopeConfig>,
    accel_range: AccelerometerRange,
    gyro_range: GyroscopeRange,
    ois_range: OisRange,
}

#[maybe_async_cfg::maybe(
    idents(hal(sync = "embedded_hal", async = "embedded_hal_async"), SpiDevice),
    sync(feature = "sync"),
    async(feature = "async")
)]
impl<I, D> Bmi270<embedded_registers::spi::SpiDevice<I, Bmi270Codec>, D>
where
    I: hal::spi::r#SpiDevice,
    D: hal::delay::DelayNs,
{
    #[inline]
    pub fn new_spi(interface: I, delay: D) -> Self {
        Self {
            interface: embedded_registers::spi::SpiDevice::new(interface),
            delay,
            accel_config: None,
            gyro_config: None,
            accel_range: AccelerometerRange::FourG,
            gyro_range: GyroscopeRange::TwoK,
            ois_range: OisRange::TwoHundredFifty,
        }
    }
}

// I2C constructor
#[maybe_async_cfg::maybe(
    idents(hal(sync = "embedded_hal", async = "embedded_hal_async"), I2cDevice),
    sync(feature = "sync"),
    async(feature = "async")
)]
impl<I, D> Bmi270<embedded_registers::i2c::I2cDevice<I, hal::i2c::SevenBitAddress, Bmi270I2cCodec>, D>
where
    I: hal::i2c::I2c<hal::i2c::SevenBitAddress> + hal::i2c::ErrorType,
    D: hal::delay::DelayNs,
{
    #[inline]
    pub fn new_i2c(interface: I, address: u8, delay: D) -> Self {
        Self {
            interface: embedded_registers::i2c::I2cDevice::new(interface, address),
            delay,
            accel_config: None,
            gyro_config: None,
            accel_range: AccelerometerRange::FourG,
            gyro_range: GyroscopeRange::TwoK,
            ois_range: OisRange::TwoHundredFifty,
        }
    }
}

#[device_impl]
#[maybe_async_cfg::maybe(
    idents(
        hal(sync = "embedded_hal", async = "embedded_hal_async"),
        RegisterInterface,
        Interface
    ),
    sync(feature = "sync"),
    async(feature = "async")
)]
impl<I, D> Bmi270<I, D>
where
    I: embedded_registers::RegisterInterface + crate::interface::Interface<I::Error>,
    D: hal::delay::DelayNs,
{
    pub const fn new(interface: I, delay: D) -> Self {
        Self {
            interface,
            delay,
            accel_config: None,
            gyro_config: None,
            accel_range: AccelerometerRange::FourG,
            gyro_range: GyroscopeRange::TwoK,
            ois_range: OisRange::TwoHundredFifty,
        }
    }

    /// Chip ID is used to verify communication between MCU and ASIC
    pub async fn get_chip_id(&mut self) -> Result<ChipId, I::Error> {
        self.interface.read_register().await
    }

    /// Initialize the ASIC
    ///
    /// 1. Set power mode
    /// 2. Write config file
    /// 3. Check status register
    pub async fn init(
        &mut self,
        accel: Option<AccelerometerConfig>,
        gyro: Option<GyroscopeConfig>,
    ) -> Result<(), Error<I::Error>> {
        self.interface
            .read_register::<ChipId>()
            .await
            .map_err(Error::Bus)?;
        let chip_id = self
            .interface
            .read_register::<ChipId>()
            .await
            .map_err(Error::Bus)?;
        if !chip_id.ok() {
            // TODO: Check this error for the proper one
            return Err(Error::Asic(InternalStatusMessage::StartupError));
        }
        self.soft_reset().await?;

        self.enable_sensors(
            PowerMode::default()
                .with_accelerometer(accel.is_some().into())
                .with_gyroscope(gyro.is_some().into()),
        )
        .await?;

        if let Some(config) = accel {
            self.configure_accelerometer(config).await?;
        }
        self.accel_config = accel;
        if let Some(config) = gyro {
            self.configure_gyroscope(config).await?;
        }
        self.gyro_config = gyro;

        self.set_accelerometer_range(
            AccelerometerRangeData::default().with_range(self.accel_range),
        )
        .await?;
        self.set_gyroscope_range(self.gyro_range, self.ois_range)
            .await?;

        Ok(())
    }

    pub async fn read_accelerometer_data(
        &mut self,
        units: AccelerationUnit,
    ) -> Result<AccelerometerData, Error<I::Error>> {
        let raw_data = self
            .interface
            .read_register::<RawAccelerometerData>()
            .await
            .map_err(Error::Bus)?;
        Ok(raw_data.to_processed(units, self.accel_range))
    }

    pub async fn read_gyroscope_data(
        &mut self,
        units: AngularVelocityUnit,
    ) -> Result<GyroscopeData, Error<I::Error>> {
        let raw_data = self
            .interface
            .read_register::<RawGyroscopeData>()
            .await
            .map_err(Error::Bus)?;
        Ok(raw_data.to_processed(units, self.gyro_range))
    }

    async fn soft_reset(&mut self) -> Result<(), Error<I::Error>> {
        self.interface
            .write_register::<Command>(Command::default().with_cmd(CommandKind::SoftReset))
            .await
            .map_err(Error::Bus)?;
        self.delay.delay_us(2000).await;
        self.interface
            .read_register::<ChipId>()
            .await
            .map_err(Error::Bus)?;
        self.write_config_file().await?;
        Ok(())
    }

    // pub fn get_accel_config(&mut self) -> Result<AccelerometerConfig, I::Error> {
    //     self.interface.read_register(AccelerometerConfig::ADDR)
    // }

    // pub fn get_gyro_config(&mut self) -> Result<GyroscopeConfig, I::Error> {
    //     self.interface.read_register(GyroscopeConfig::ADDR)
    // }

    async fn write_config_file(&mut self) -> Result<(), Error<I::Error>> {
        self.set_advanced_power_save(Status::Disabled).await?;

        self.delay.delay_us(450).await;

        self.interface
            .write_register::<InitCtrl>(InitCtrl::default().with_cmd(InitCtrlCommand::Start))
            .await
            .map_err(Error::Bus)?;

        self.interface
            .write(&BMI270_CONFIG_BUF)
            .await
            .map_err(Error::Bus)?;

        self.interface
            .write_register::<InitCtrl>(InitCtrl::default().with_cmd(InitCtrlCommand::End))
            .await
            .map_err(Error::Bus)?;

        self.delay.delay_ms(50).await;

        // self.set_advanced_power_save(Status::Enabled).await?;

        self.delay.delay_us(20000).await;

        match self.get_internal_status().await?.read_message() {
            InternalStatusMessage::InitOk => Ok(()),
            err => Err(Error::Asic(err)),
        }
    }

    async fn get_internal_status(&mut self) -> Result<InternalStatus, Error<I::Error>> {
        self.interface
            .read_register::<InternalStatus>()
            .await
            .map_err(Error::Bus)
    }

    async fn set_advanced_power_save(
        &mut self,
        advanced_power_save: Status,
    ) -> Result<(), Error<I::Error>> {
        let mut power_config = self.get_power_config().await?;
        power_config.write_advanced_power_save(advanced_power_save);
        self.set_power_config(power_config).await
    }

    pub async fn enable_sensors(&mut self, power_mode: PowerMode) -> Result<(), Error<I::Error>> {
        self.interface
            .write_register(power_mode)
            .await
            .map_err(Error::Bus)
    }

    async fn get_power_config(&mut self) -> Result<PowerConfig, Error<I::Error>> {
        self.interface
            .read_register::<PowerConfig>()
            .await
            .map_err(Error::Bus)
    }

    async fn set_power_config(&mut self, config: PowerConfig) -> Result<(), Error<I::Error>> {
        self.interface
            .write_register::<PowerConfig>(config)
            .await
            .map_err(Error::Bus)
    }

    pub async fn configure_accelerometer(
        &mut self,
        config: AccelerometerConfig,
    ) -> Result<(), Error<I::Error>> {
        self.interface
            .write_register(config)
            .await
            .map_err(Error::Bus)
    }

    pub async fn configure_gyroscope(
        &mut self,
        config: GyroscopeConfig,
    ) -> Result<(), Error<I::Error>> {
        self.interface
            .write_register(config)
            .await
            .map_err(Error::Bus)
    }

    // pub fn read_accel_gyro_data(&mut self) -> Result<AccelGyroData, I::Error> {
    //     let mut buf = [0u8; AccelGyroData::SIZE];
    //     self.interface
    //         .burst_read_register(AccelGyroData::BASE_ADDR, &mut buf)?;
    //     Ok(AccelGyroData::from_bytes(buf))
    // }

    // pub fn configure_features(&mut self, features: Features) -> Result<(), I::Error> {
    //     self.interface
    //         .write_register(FeaturePage::ADDR, features.to_feature_page())?;
    //     self.interface
    //         .burst_write_register(Features::BASE_ADDR, &features.to_bytes())
    // }

    // pub fn enable_gyroscope_self_offset_correction(&mut self) -> Result<(), I::Error> {
    //     self.interface
    //         .write_register(FeaturePage::ADDR, FeaturePage::Page1)?;

    //     let mut buf = [0u8; 2];
    //     self.interface
    //         .burst_read_register(FeaturePage1::GEN_SET_1_ADDR, &mut buf)?;
    //     let mut settings = GeneralSettings1::from_raw(u16::from_be_bytes(buf));
    //     println!("got settings {:?}", settings);
    //     settings.enable_gyroscope_self_offset_correction();
    //     println!("enabled gyr self offset correction {:?}", settings);

    //     self.interface
    //         .write_register(FeaturePage::ADDR, FeaturePage::Page1)?;
    //     self.interface
    //         .burst_write_register(GeneralSettings1::BASE_ADDR, &settings.to_bytes())?;
    //     println!("wrote new settings");

    //     self.interface
    //         .write_register(FeaturePage::ADDR, FeaturePage::Page1)?;
    //     self.interface
    //         .burst_read_register(FeaturePage1::GEN_SET_1_ADDR, &mut buf)?;
    //     let settings = GeneralSettings1::from_raw(u16::from_be_bytes(buf));
    //     println!("new settings {:?}", settings);

    //     Ok(())
    // }

    pub async fn set_gyroscope_range(
        &mut self,
        range: GyroscopeRange,
        ois_range: OisRange,
    ) -> Result<(), Error<I::Error>> {
        self.interface
            .write_register(
                GyroscopeRangeData::default()
                    .with_range(range)
                    .with_ois_range(ois_range),
            )
            .await
            .map_err(Error::Bus)
    }

    pub async fn set_accelerometer_range(
        &mut self,
        range: AccelerometerRangeData,
    ) -> Result<(), Error<I::Error>> {
        self.interface
            .write_register(range)
            .await
            .map_err(Error::Bus)
    }

    /// read for ekf and control loop application
    pub async fn imu_read(
        &mut self,
        r_matrix: Matrix3<f32>,
        dt: f32,
    ) -> Result<ImuData, Error<I::Error>> {
        let a = self.read_accelerometer_data(AccelerationUnit::Gs).await?;
        let g = self.read_gyroscope_data(AngularVelocityUnit::Radps).await?;

        let a_sensor = Vector3::new(a.ax, a.ay, a.az);
        let g_sensor = Vector3::new(g.gx, g.gy, g.gz);

        let a_user = r_matrix * a_sensor;
        let g_user = r_matrix * g_sensor;

        Ok(ImuData {
            ax: Accel::new(a_user.x, AccelUnit::Gs),
            ay: Accel::new(a_user.y, AccelUnit::Gs),
            az: Accel::new(a_user.z, AccelUnit::Gs),
            gx: Gyro::new(g_user.x, GyroUnit::Radps),
            gy: Gyro::new(g_user.y, GyroUnit::Radps),
            gz: Gyro::new(g_user.z, GyroUnit::Radps),
            dt,
        })
    }
}
