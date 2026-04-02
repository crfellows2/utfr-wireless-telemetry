use bstr::ByteSlice;
use embassy_time::{Duration, Ticker};
use esp32_nimble::{uuid128, BLEDevice, BLEScan};
use esp_idf_svc::hal::task::block_on;
use log::info;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let task = async {
        let ble_device = BLEDevice::take();
        let mut ble_scan = BLEScan::new();
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

        if let Some(device) = device {
            let mut client = ble_device.new_client();
            client.on_connect(|client| {
                client.update_conn_params(120, 120, 0, 60).unwrap();
            });
            client.connect(&device.addr()).await?;

            let service = client
                .get_service(uuid128!("fafafafa-fafa-fafa-fafa-fafafafafafa"))
                .await?;

            // Subscribe to telemetry characteristic (CAN frames)
            let telemetry_uuid = uuid128!("a3c87500-8ed3-4bdf-8a39-a01bebede295");
            let telemetry_char = service.get_characteristic(telemetry_uuid).await?;

            if !telemetry_char.can_notify() {
                ::log::error!("telemetry characteristic can't notify: {}", telemetry_char);
                return anyhow::Ok(());
            }

            ::log::info!("subscribe to telemetry: {}", telemetry_char);
            telemetry_char
                .on_notify(|data| {
                    // Parse CAN frame: [sec:8][usec:4][bus:1][id:4][len:1][data:8] = 26 bytes
                    if data.len() == 26 {
                        let timestamp_sec = u64::from_le_bytes([
                            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                        ]);
                        let timestamp_usec = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                        let bus_id = data[12];
                        let can_id = u32::from_le_bytes([data[13], data[14], data[15], data[16]]);
                        let data_len = data[17] as usize;
                        let can_data = &data[18..18 + data_len.min(8)];

                        // Format data bytes as hex
                        let mut hex_str = [0u8; 24]; // max 8 bytes * 2 chars + spaces
                        let mut hex_pos = 0;
                        for (i, byte) in can_data.iter().enumerate() {
                            if i > 0 {
                                hex_str[hex_pos] = b' ';
                                hex_pos += 1;
                            }
                            hex_str[hex_pos] = b"0123456789ABCDEF"[(byte >> 4) as usize];
                            hex_str[hex_pos + 1] = b"0123456789ABCDEF"[(byte & 0xF) as usize];
                            hex_pos += 2;
                        }
                        let hex = core::str::from_utf8(&hex_str[..hex_pos]).unwrap_or("?");

                        ::log::info!(
                            "CAN{} {:03X} [{}] {} @ {}.{:06}",
                            bus_id,
                            can_id,
                            data_len,
                            hex,
                            timestamp_sec,
                            timestamp_usec
                        );
                    } else {
                        ::log::info!("Telemetry: unexpected {} bytes", data.len());
                    }
                })
                .subscribe_notify(false)
                .await?;

            // Subscribe to status characteristic (SD card usage)
            let status_uuid = uuid128!("b4d98600-8ed3-4bdf-8a39-a01bebede295");
            let status_char = service.get_characteristic(status_uuid).await?;

            if !status_char.can_notify() {
                ::log::error!("status characteristic can't notify: {}", status_char);
                return anyhow::Ok(());
            }

            ::log::info!("subscribe to status: {}", status_char);
            status_char
                .on_notify(|data| {
                    if data.len() == 8 {
                        let used_kb = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                        let total_kb = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                        let used_mb = used_kb / 1024;
                        let total_mb = total_kb / 1024;
                        let percent = if total_kb > 0 {
                            (used_kb as f32 / total_kb as f32) * 100.0
                        } else {
                            0.0
                        };
                        ::log::info!(
                            "SD Card: {} / {} MB ({:.1}% used)",
                            used_mb,
                            total_mb,
                            percent
                        );
                    } else {
                        ::log::info!("Status: {} bytes", data.len());
                    }
                })
                .subscribe_notify(false)
                .await?;

            let tx_char = service
                .get_characteristic(uuid128!("3c9a3f00-8ed3-4bdf-8a39-a01bebede295"))
                .await?;

            let mut ticker = Ticker::every(Duration::from_millis(1000));
            let mut counter = 0;
            loop {
                tx_char
                    .write_value(format!("Counter: {counter}").as_bytes(), false)
                    .await?;

                counter += 1;

                ticker.next().await;
            }
        }

        anyhow::Ok(())
    };

    let local_ex: edge_executor::LocalExecutor = Default::default();

    let ble_task = local_ex.spawn(task);
    let res = block_on(local_ex.run(ble_task));
    res
}
