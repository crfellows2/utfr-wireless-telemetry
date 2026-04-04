use embassy_sync::blocking_mutex::{raw::CriticalSectionRawMutex, Mutex};
use esp32_nimble::{uuid128, BLEAdvertisementData, BLEDevice, NimbleProperties};
use std::sync::Arc;

pub async fn ble_task() -> anyhow::Result<()> {
    let ble_device = BLEDevice::take();
    let ble_advertising = ble_device.get_advertising();

    let server = ble_device.get_server();
    server.on_connect(|server, desc| {
        ::log::info!("Client connected: {:?}", desc);

        server
            .update_conn_params(desc.conn_handle(), 24, 48, 0, 60)
            .unwrap();
    });

    server.on_disconnect(|_desc, reason| {
        ::log::info!("Client disconnected ({:?})", reason);
    });

    let service = server.create_service(uuid128!("fafafafa-fafa-fafa-fafa-fafafafafafa"));

    // Telemetry data characteristic (CAN frames)
    let telemetry_characteristic = service.lock().create_characteristic(
        uuid128!("a3c87500-8ed3-4bdf-8a39-a01bebede295"),
        NimbleProperties::READ | NimbleProperties::NOTIFY,
    );

    // Status characteristic (SD card usage, etc.)
    let status_characteristic = service.lock().create_characteristic(
        uuid128!("b4d98600-8ed3-4bdf-8a39-a01bebede295"),
        NimbleProperties::READ | NimbleProperties::NOTIFY,
    );

    // A writable characteristic.
    let writable_characteristic = service.lock().create_characteristic(
        uuid128!("3c9a3f00-8ed3-4bdf-8a39-a01bebede295"),
        NimbleProperties::WRITE,
    );
    writable_characteristic.lock().on_write(|args| {
        let msg = core::str::from_utf8(args.recv_data()).unwrap();
        ::log::info!("Wrote to writable characteristic: {}", msg);
    });

    ble_advertising.lock().set_data(
        BLEAdvertisementData::new()
            .name("ESP32-GATT-Server")
            .add_service_uuid(uuid128!("fafafafa-fafa-fafa-fafa-fafafafafafa")),
    )?;
    ble_advertising.lock().start()?;

    crate::can_interface::TX_CHAR.signal(telemetry_characteristic);

    // Listen for SD status updates and notify
    loop {
        let sd_status = crate::sd_logger::SD_STATUS.wait().await;
        let data = sd_status.to_ble_bytes();
        status_characteristic.lock().set_value(&data).notify();
        ::log::info!(
            "SD status: {}/{} KB used",
            sd_status.used_kb,
            sd_status.total_kb
        );
    }
}

pub static TX_CHAR: Mutex<
    CriticalSectionRawMutex,
    Option<Arc<esp32_nimble::utilities::mutex::Mutex<esp32_nimble::BLECharacteristic>>>,
> = Mutex::new(None);

pub fn notify(value: &[u8]) -> Result<(), ()> {
    Ok(())
}
