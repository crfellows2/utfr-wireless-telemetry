use embassy_time::{Duration, Ticker};
use enumset::enum_set;
use esp_idf_svc::{
    hal::{
        can::{Flags, Frame, CAN},
        gpio::{InputPin, OutputPin},
    },
    sys::EspError,
};
use log::info;

use crate::can_interface::CanInterface;

#[allow(unused)]
pub struct MockCan {
    ticker: Ticker,
    count: u32,
}

impl CanInterface for MockCan {
    fn start(
        _can_peripheral: CAN<'static>,
        _tx_pin: impl OutputPin + 'static,
        _rx_pin: impl InputPin + 'static,
    ) -> Result<Self, EspError> {
        let ticker = Ticker::every(Duration::from_millis(10));
        Ok(Self { ticker, count: 0 })
    }

    async fn receive(&mut self) -> Result<Frame, EspError> {
        self.ticker.next().await;

        let mut data = [0u8; 8];
        data[..4].copy_from_slice(&self.count.to_be_bytes());

        let frame = Frame::new(0x55, enum_set!(Flags::SingleShot), &data).unwrap();

        self.count += 1;

        Ok(frame)
    }

    async fn transmit(&self, frame: &Frame) -> Result<(), EspError> {
        info!(
            "Write can frame [{:02X}][{:?}]",
            &frame.identifier(),
            &frame.data()
        );
        Ok(())
    }
}
