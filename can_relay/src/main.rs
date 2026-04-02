use edge_executor::block_on;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs};

use edge_executor::LocalExecutor;
use std::sync::mpsc;
use std::sync::Mutex;

// SD logging channel (uses std::sync::mpsc for blocking thread)
pub static SD_TX: Mutex<Option<mpsc::SyncSender<can_interface::CanFrame>>> = Mutex::new(None);

mod ble;
mod can_interface;
mod rtc;
mod sd_logger;

fn main() {
    // It is necessary to call this function once. Otherwise, some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let pins = peripherals.pins;

    // can transceivers
    let can1_tx = pins.gpio18;
    let can1_rx = pins.gpio14;
    let _can2_tx = pins.gpio20;
    let _can2_rx = pins.gpio19;
    let can_peripheral = peripherals.can;

    // SD card SPI
    let sd_miso = pins.gpio21;
    let sd_mosi = pins.gpio2;
    let sd_cs = pins.gpio23;
    let sd_sclk = pins.gpio22;

    // Real Time Clock
    let rtc_sda = pins.gpio1;
    let rtc_scl = pins.gpio0;

    // Initialize NVS for storing RTC metadata
    let nvs_partition = EspDefaultNvsPartition::take().expect("Failed to get NVS partition");
    let mut nvs =
        EspNvs::new(nvs_partition, "rtc_config", true).expect("Failed to initialize NVS storage");

    // Initialize RTC hardware
    let mut rtc_manager =
        rtc::RtcManager::new(peripherals.i2c0, rtc_sda, rtc_scl).expect("Failed to initialize RTC");

    // Initialize or validate RTC time
    rtc_manager
        .initialize_time(&mut nvs)
        .expect("Failed to initialize RTC time");

    // Sync ESP32 system time with DS3231
    rtc_manager
        .sync_system_time()
        .expect("Failed to sync system time");

    // Create SD logger channel and thread
    let (sd_tx, sd_rx) = mpsc::sync_channel(256);
    *SD_TX.lock().unwrap() = Some(sd_tx);

    sd_logger::start_sd_logger_thread(sd_rx, peripherals.spi2, sd_sclk, sd_mosi, sd_miso, sd_cs)
        .expect("Failed to start SD logger thread");

    let local_ex: LocalExecutor = Default::default();

    // Spawn both BLE and CAN tasks
    let ble_task = local_ex.spawn(async { ble::ble_task().await });
    let can_task_handle = local_ex.spawn(async {
        can_interface::can_task(can_peripheral, can1_tx, can1_rx, 0).await;
        Ok::<(), anyhow::Error>(())
    });

    // Run executor with both tasks
    use embassy_futures::select::select;
    block_on(local_ex.run(async { select(ble_task, can_task_handle).await }));
}
