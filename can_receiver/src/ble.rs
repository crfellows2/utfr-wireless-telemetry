use bstr::ByteSlice;
use embassy_time::{Duration, Ticker};
use esp32_nimble::{
    uuid128, BLEAdvertisedDevice, BLEClient, BLEDevice, BLERemoteCharacteristic, BLERemoteService,
    BLEScan,
};
use esp_idf_svc::sys::ble_gap_set_data_len;
use log::info;
use protocol::CanFrame;
use std::sync::atomic::Ordering;

use crate::telemetry::{format_frame, TELEMETRY_BYTES};

const RECONNECT_DELAY_MS: u64 = 1000;

pub async fn scan_for_device(
    ble_device: &BLEDevice,
    ble_scan: &mut BLEScan,
) -> anyhow::Result<Option<BLEAdvertisedDevice>> {
    info!("Scanning for devices...");
    let device = ble_scan
        .active_scan(true)
        .interval(100)
        .window(99)
        .start(ble_device, 10000, |device, data| {
            if let Some(name) = data.name() {
                info!("Device Advertisement: {}", name);
                if name.contains_str("ESP32") {
                    return Some(*device);
                }
            }
            None
        })
        .await?;
    Ok(device)
}

async fn subscribe_to_telemetry(
    service: &mut BLERemoteService,
) -> anyhow::Result<BLERemoteCharacteristic> {
    let telemetry_uuid = uuid128!("a3c87500-8ed3-4bdf-8a39-a01bebede295");
    let telemetry_char = service.get_characteristic(telemetry_uuid).await?;

    if !telemetry_char.can_notify() {
        anyhow::bail!("telemetry characteristic can't notify: {}", telemetry_char);
    }

    info!("Subscribing to telemetry: {}", telemetry_char);
    telemetry_char
        .on_notify(|data| {
            TELEMETRY_BYTES.fetch_add(data.len(), Ordering::Relaxed);

            let start = std::time::Instant::now();

            // Build output for all frames in this notification (one allocation, reused)
            let mut output = String::with_capacity(4096);

            // Data contains COBS-encoded frames separated by 0x00
            for chunk in data.split(|&b| b == 0x00) {
                if chunk.is_empty() {
                    continue;
                }

                // Need a mutable copy for COBS decoding
                let mut buf = [0u8; 256];
                let len = chunk.len().min(buf.len());
                buf[..len].copy_from_slice(&chunk[..len]);

                match postcard::from_bytes_cobs::<CanFrame>(&mut buf[..len]) {
                    Ok(frame) => {
                        let mut frame_buf: heapless::String<512> = heapless::String::new();
                        format_frame(&frame, &mut frame_buf);
                        output.push_str(&frame_buf);
                    }
                    Err(e) => log::warn!("Postcard decode error: {}", e),
                }
            }

            // Single print per notification
            if !output.is_empty() {
                print!("{}", output);
            }

            println!("Took {:?}", start.elapsed());
        })
        .subscribe_notify(false)
        .await?;

    Ok(telemetry_char.clone())
}

async fn subscribe_to_status(service: &mut BLERemoteService) -> anyhow::Result<()> {
    let status_uuid = uuid128!("b4d98600-8ed3-4bdf-8a39-a01bebede295");
    let status_char = service.get_characteristic(status_uuid).await?;

    if !status_char.can_notify() {
        anyhow::bail!("status characteristic can't notify: {}", status_char);
    }

    info!("Subscribing to status: {}", status_char);
    status_char
        .on_notify(|data| {
            if data.len() == 8 {
                let used_kb = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let total_kb = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                // Output with $ prefix for can_bridge parsing
                // Format: $SD <used_kb> <total_kb>
                println!("$SD {} {}", used_kb, total_kb);
            }
        })
        .subscribe_notify(false)
        .await?;

    Ok(())
}

async fn run_connection(
    client: &mut BLEClient,
    command_rx: embassy_sync::channel::Receiver<'static, embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, protocol::Command, 32>,
) -> anyhow::Result<()> {
    use embassy_futures::select::{select, Either};

    let service = client
        .get_service(uuid128!("fafafafa-fafa-fafa-fafa-fafafafafafa"))
        .await?;

    let mut telemetry_char = subscribe_to_telemetry(&mut *service).await?;
    subscribe_to_status(&mut *service).await?;

    info!("Connected and subscribed, running main loop");

    let mut ticker = Ticker::every(Duration::from_millis(1000));

    loop {
        match select(command_rx.receive(), ticker.next()).await {
            Either::First(protocol::Command::Write(frame)) => {
                log::info!("Received Write command from channel, sending over BLE...");
                // Serialize frame to COBS
                let serialized = postcard::to_vec_cobs::<_, 100>(&frame)?;
                telemetry_char.write_value(&serialized, false).await?;
                log::info!("Write command sent successfully");
            }
            Either::First(_) => {
                // Other command types - ignore
            }
            Either::Second(_) => {
                // Ticker fired - report bitrate
                let bytes = TELEMETRY_BYTES.swap(0, Ordering::Relaxed);
                let kbps = (bytes as f64 * 8.0) / 1000.0;
                println!("$RATE {:.2}", kbps);
            }
        }
    }
}

pub async fn connect_and_run(
    ble_device: &BLEDevice,
    device: &BLEAdvertisedDevice,
    command_rx: embassy_sync::channel::Receiver<'static, embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, protocol::Command, 32>,
) -> anyhow::Result<()> {
    let mut client = ble_device.new_client();
    client.on_connect(|_client| unsafe {
        let rc = ble_gap_set_data_len(0, 251, 2120);
        if rc != 0 {
            log::warn!("ble_gap_set_data_len failed: {}", rc);
        } else {
            log::info!("Called ble_gap_set_data_len()");
        }
    });

    info!("Connecting to {}...", device.addr());
    client.connect(&device.addr()).await?;

    run_connection(&mut client, command_rx).await
}

pub fn reconnect_delay() -> Duration {
    Duration::from_millis(RECONNECT_DELAY_MS)
}
