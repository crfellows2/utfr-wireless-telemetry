use std::sync::Arc;

use embassy_futures::select::select3;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use esp_idf_svc::hal::{
    can::{self, AsyncCanDriver, Frame, CAN},
    gpio::{InputPin, OutputPin},
};
use log::info;

/// Timestamped CAN frame
#[derive(Debug, Clone)]
pub struct CanFrame {
    pub timestamp_sec: u64,
    pub timestamp_usec: u32,
    pub bus_id: u8,
    pub can_id: u32,
    pub data: [u8; 8],
    pub data_len: usize,
}

impl CanFrame {
    pub fn from_hardware_frame(frame: &Frame, bus_id: u8) -> Self {
        let (sec, usec) = crate::rtc::get_system_timestamp_us();
        let can_id = frame.identifier();
        let data_slice = frame.data();

        let mut data = [0u8; 8];
        data[..data_slice.len()].copy_from_slice(data_slice);

        Self {
            timestamp_sec: sec as u64,
            timestamp_usec: usec as u32,
            bus_id,
            can_id,
            data,
            data_len: data_slice.len(),
        }
    }

    /// Binary format: [sec:8][usec:4][bus:1][id:4][len:1][data:8] = 26 bytes
    pub fn to_ble_bytes(&self) -> [u8; 26] {
        let mut buf = [0u8; 26];
        buf[0..8].copy_from_slice(&self.timestamp_sec.to_le_bytes());
        buf[8..12].copy_from_slice(&self.timestamp_usec.to_le_bytes());
        buf[12] = self.bus_id;
        buf[13..17].copy_from_slice(&self.can_id.to_le_bytes());
        buf[17] = self.data_len as u8;
        buf[18..26].copy_from_slice(&self.data);
        buf
    }
}

pub static TX_CHAR: Signal<
    CriticalSectionRawMutex,
    Arc<esp32_nimble::utilities::mutex::Mutex<esp32_nimble::BLECharacteristic>>,
> = Signal::new();

pub async fn can_task(
    can_peripheral: CAN<'_>,
    tx_pin: impl OutputPin,
    rx_pin: impl InputPin,
    bus_id: u8,
) {
    info!("Can task started");
    let timing = can::config::Timing::Custom {
        baudrate_prescaler: 2,
        timing_segment_1: 15,
        timing_segment_2: 4,
        synchronization_jump_width: 3,
        triple_sampling: false,
    };
    let config = can::config::Config::new().timing(timing);
    let mut can = AsyncCanDriver::new(can_peripheral, tx_pin, rx_pin, &config).unwrap();
    can.start().unwrap();

    let mut tx_char: Option<
        Arc<esp32_nimble::utilities::mutex::Mutex<esp32_nimble::BLECharacteristic>>,
    > = None;

    loop {
        // Clone the BLE characteristic for this iteration to avoid borrow conflicts
        let tx_char_clone = tx_char.clone();

        select3(
            async {
                match can.receive().await {
                    Ok(frame) => {
                        // info!("CAN RX: {frame}");
                        // Timestamp frame
                        let can_frame = CanFrame::from_hardware_frame(&frame, bus_id);

                        // Send to BLE (binary format, 26 bytes)
                        if let Some(tx) = &tx_char_clone {
                            let ble_data = can_frame.to_ble_bytes();
                            tx.lock().set_value(&ble_data).notify();
                        }

                        // Send to SD logger (non-blocking)
                        if let Some(sd_tx) = crate::SD_TX.lock().unwrap().as_ref() {
                            if sd_tx.try_send(can_frame).is_err() {
                                // Channel full, frame dropped
                                use core::sync::atomic::{AtomicU32, Ordering};
                                static DROP_COUNT: AtomicU32 = AtomicU32::new(0);
                                let count = DROP_COUNT.fetch_add(1, Ordering::Relaxed);
                                if count % 100 == 0 {
                                    log::warn!("Dropped {} frames (SD buffer full)", count);
                                }
                            }
                        }
                    }
                    Err(e) => log::error!("CAN RX error on bus {}: {:?}", bus_id, e),
                }
            },
            async {
                match can.read_alerts().await {
                    Ok(alerts) => info!("Alerts: {alerts:?}"),
                    Err(e) => info!("Alerts Err: {e:?}"),
                }
            },
            async {
                let new_char = TX_CHAR.wait().await;
                info!("CAN task got BLE TX Char");
                tx_char = Some(new_char);
            },
        )
        .await;
    }
}
