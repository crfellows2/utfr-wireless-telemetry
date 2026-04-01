use embassy_time::{Duration, Ticker};
use esp32_nimble::{uuid128, BLEAdvertisementData, BLEDevice, NimbleProperties};
use std::format;

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

    // A characteristic that notifies every second.
    let notifying_characteristic = service.lock().create_characteristic(
        uuid128!("a3c87500-8ed3-4bdf-8a39-a01bebede295"),
        NimbleProperties::READ | NimbleProperties::NOTIFY,
    );
    notifying_characteristic.lock().set_value(b"Initial value.");

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

    server.ble_gatts_show_local();

    let mut ticker = Ticker::every(Duration::from_millis(1000));
    let mut counter = 0;
    loop {
        notifying_characteristic
            .lock()
            .set_value(format!("Counter: {counter}").as_bytes())
            .notify();

        counter += 1;

        ticker.next().await;
    }
}
