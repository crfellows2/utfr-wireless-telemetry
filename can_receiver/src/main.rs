mod ble;
mod stdin;
mod telemetry;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Timer;
use esp32_nimble::{BLEDevice, BLEScan};
use esp_idf_svc::hal::task::block_on;
use log::info;
use protocol::Command;

static STDIN_COMMAND_CHANNEL: Channel<CriticalSectionRawMutex, Command, 32> = Channel::new();

async fn main_loop() -> anyhow::Result<()> {
    let ble_device = BLEDevice::take();
    let mut ble_scan = BLEScan::new();
    let command_rx = STDIN_COMMAND_CHANNEL.receiver();

    loop {
        match ble::scan_for_device(ble_device, &mut ble_scan).await {
            Ok(Some(device)) => {
                if let Err(e) = ble::connect_and_run(ble_device, &device, command_rx).await {
                    log::warn!("Connection ended: {}", e);
                }
            }
            Ok(None) => {
                info!("No device found");
            }
            Err(e) => {
                log::warn!("Scan failed: {}", e);
            }
        }

        info!("Reconnecting in {:?}...", ble::reconnect_delay());
        Timer::after(ble::reconnect_delay()).await;
    }
}

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // Setup USB Serial and spawn stdin reader thread
    let usb_serial = stdin::setup_usb_serial()?;
    let stdin_tx = STDIN_COMMAND_CHANNEL.sender();
    stdin::spawn_stdin_reader(usb_serial, stdin_tx);

    let local_ex: edge_executor::LocalExecutor = Default::default();
    let task = local_ex.spawn(main_loop());
    block_on(local_ex.run(task))
}
