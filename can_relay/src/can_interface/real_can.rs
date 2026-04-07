use esp_idf_svc::{
    hal::{
        can::{self, AsyncCanDriver, CAN},
        gpio::{InputPin, OutputPin},
    },
    sys::EspError,
};

use crate::can_interface::CanInterface;

pub struct RealCan {
    can: AsyncCanDriver<'static, can::CanDriver<'static>>,
}

impl CanInterface for RealCan {
    fn start(
        can_peripheral: CAN<'static>,
        tx_pin: impl OutputPin + 'static,
        rx_pin: impl InputPin + 'static,
    ) -> Result<Self, EspError> {
        let timing = can::config::Timing::Custom {
            baudrate_prescaler: 2,
            timing_segment_1: 15,
            timing_segment_2: 4,
            synchronization_jump_width: 3,
            triple_sampling: false,
        };
        let config = can::config::Config::new().timing(timing);
        let mut can: AsyncCanDriver<'_, can::CanDriver<'_>> =
            AsyncCanDriver::new(can_peripheral, tx_pin, rx_pin, &config)?;
        can.start()?;

        Ok(Self { can })
    }

    #[inline]
    async fn receive(&mut self) -> Result<can::Frame, EspError> {
        self.can.receive().await
    }

    #[inline]
    async fn transmit(&self, frame: &can::Frame) -> Result<(), EspError> {
        self.can.transmit(frame).await
    }
}
