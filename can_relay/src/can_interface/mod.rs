mod mock_can;
mod real_can;

use embassy_futures::select::{select, Either};
use futures::stream::pending;
pub use mock_can::MockCan;
pub use real_can::RealCan;

use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Receiver, Sender},
    signal::Signal,
};
use enumset::{enum_set, EnumSet};
use esp_idf_svc::{
    hal::{
        can::{Alert, Flags, Frame, CAN},
        gpio::{InputPin, OutputPin},
    },
    sys::EspError,
};
use log::{error, info, warn};

use crate::{ble::BleCanLink, sd_logger};

/// Timestamped CAN frame
#[derive(Debug, Clone)]
pub struct CanFrameForSd {
    pub timestamp_sec: u64,
    pub timestamp_usec: u32,
    pub bus_id: u8,
    pub can_id: u32,
    pub data_len: u8,
    pub data: [u8; 8],
}

impl CanFrameForSd {
    pub fn from_hardware_frame(frame: &Frame, bus_id: u8) -> Self {
        let (sec, usec) = crate::rtc::get_system_timestamp_us();
        let can_id = frame.identifier();
        let data_slice = frame.data();

        let mut data = [0u8; 8];
        data[..data_slice.len()].copy_from_slice(data_slice);
        let data_len = data_slice.len() as u8;

        Self {
            timestamp_sec: sec as u64,
            timestamp_usec: usec as u32,
            bus_id,
            can_id,
            data_len,
            data,
        }
    }
}

pub trait CanInterface {
    fn start(
        can_peripheral: CAN<'static>,
        tx_pin: impl OutputPin + 'static,
        rx_pin: impl InputPin + 'static,
    ) -> Result<Self, EspError>
    where
        Self: Sized;

    async fn receive(&mut self) -> Result<Frame, EspError>;
    async fn transmit(&self, frame: &Frame) -> Result<(), EspError>;
}

pub async fn can_task<Can, const N: usize>(
    can_peripheral: CAN<'static>,
    tx_pin: impl OutputPin + 'static,
    rx_pin: impl InputPin + 'static,
    sd_log_tx: Sender<'static, CriticalSectionRawMutex, CanFrameForSd, N>,
    ble_tx_oneshot: &Signal<CriticalSectionRawMutex, BleCanLink>,
    can_write_rx: Receiver<'static, CriticalSectionRawMutex, protocol::CanFrame, N>,
) where
    Can: CanInterface,
{
    let mut can = Can::start(can_peripheral, tx_pin, rx_pin).expect("Should be able to start CAN");
    let mut tx: Option<BleCanLink> = None;

    loop {
        match select(can.receive(), can_write_rx.receive()).await {
            Either::First(Ok(frame)) => {
                if let Some(ble_tx) = ble_tx_oneshot.try_take() {
                    if tx.is_some() {
                        warn!("got a second BleTX instance on oneshot channel");
                    }
                    info!("Got BleTX");
                    tx = Some(ble_tx);
                }

                if let Some(tx) = tx.as_mut() {
                    // for _ in 0..68 {
                    tx.send_can_frame(&frame);
                    // }
                }
                let log_frame = CanFrameForSd::from_hardware_frame(&frame, 0);
                if sd_log_tx.try_send(log_frame).is_err() {
                    warn!("Failed to log can frame to channel");
                }
            }
            Either::First(Err(e)) => error!("CAN receive error: {e:?}"),
            Either::Second(ble_frame) => {
                let (id, flags) = match ble_frame.id {
                    protocol::CanId::Standard(standard_id) => {
                        (standard_id.value() as u32, enum_set!(Flags::SingleShot))
                    }
                    protocol::CanId::Extended(extended_id) => (
                        extended_id.value(),
                        enum_set!(Flags::SingleShot | Flags::Extended),
                    ),
                };
                let frame = Frame::new(id, flags, &ble_frame.payload).unwrap();
                if let Err(e) = can.transmit(&frame).await {
                    error!("CAN transmit error: {e:?}");
                }
            }
        }
    }
}
