use crate::bmi270::Bmi270Codec;

#[maybe_async_cfg::maybe(
    idents(hal(sync = "embedded_hal", async = "embedded_hal_async")),
    sync(feature = "sync"),
    async(feature = "async")
)]
pub trait Interface<E> {
    #[allow(async_fn_in_trait)]
    async fn write(&mut self, buf: &[u8]) -> Result<(), E>;
}

pub struct Spi<SPI> {
    pub spi: SPI,
}

impl<SPI> Spi<SPI> {
    pub const fn new(spi: SPI) -> Self {
        Self { spi }
    }
}

#[maybe_async_cfg::maybe(
    idents(
        hal(sync = "embedded_hal", async = "embedded_hal_async"),
        Interface,
        SpiDevice
    ),
    sync(feature = "sync"),
    async(feature = "async")
)]
impl<SPI, E> Interface<E> for embedded_registers::spi::SpiDevice<SPI, Bmi270Codec>
where
    SPI: hal::spi::r#SpiDevice<Error = E>,
{
    async fn write(&mut self, buf: &[u8]) -> Result<(), E> {
        self.interface.write(buf).await
    }
}

// I2C interface support
#[maybe_async_cfg::maybe(
    idents(
        hal(sync = "embedded_hal", async = "embedded_hal_async"),
        Interface,
        I2cDevice
    ),
    sync(feature = "sync"),
    async(feature = "async")
)]
impl<I2C, E> Interface<E> for embedded_registers::i2c::I2cDevice<I2C, hal::i2c::SevenBitAddress, crate::bmi270::Bmi270I2cCodec>
where
    I2C: hal::i2c::I2c<hal::i2c::SevenBitAddress, Error = E> + hal::i2c::ErrorType,
{
    async fn write(&mut self, buf: &[u8]) -> Result<(), E> {
        // For BMI270 config file writes (8KB initialization data)
        // The embedded_registers layer handles register addressing
        self.bound_bus.interface.write(self.bound_bus.address, buf).await
    }
}
