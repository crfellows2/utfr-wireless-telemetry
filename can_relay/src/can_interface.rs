use std::sync::Arc;

use embassy_futures::select::select;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use enumset::enum_set;
use esp_idf_svc::hal::{
    can::{self, config, AsyncCanDriver, Flags, Frame, CAN},
    gpio::{InputPin, OutputPin},
};
use log::{debug, info};

pub static TX_CHAR: Signal<
    CriticalSectionRawMutex,
    Arc<esp32_nimble::utilities::mutex::Mutex<esp32_nimble::BLECharacteristic>>,
> = Signal::new();

pub async fn can_task(can_peripheral: CAN<'_>, tx_pin: impl OutputPin, rx_pin: impl InputPin) {
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

    loop {
        select(
            async {
                match can.receive().await {
                    Ok(frame) => {
                        debug!("CAN RX: {frame}");
                        if let Some(tx) = TX_CHAR.try_take() {
                            // TODO: filter and relay on BLE
                        }
                        // TODO: write to SD
                    }
                    Err(e) => log::error!("Can RX Error: {e:?}"),
                }
            },
            async {
                match can.read_alerts().await {
                    Ok(alerts) => debug!("Alerts: {alerts:?}"),
                    Err(e) => debug!("Alerts Err: {e:?}"),
                }
            },
        )
        .await;
    }
}
