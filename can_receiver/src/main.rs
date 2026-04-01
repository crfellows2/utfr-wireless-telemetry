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

            let uuid = uuid128!("a3c87500-8ed3-4bdf-8a39-a01bebede295");
            let characteristic = service.get_characteristic(uuid).await?;

            if !characteristic.can_notify() {
                ::log::error!("characteristic can't notify: {}", characteristic);
                return anyhow::Ok(());
            }

            ::log::info!("subscribe to {}", characteristic);
            characteristic
                .on_notify(|data| {
                    ::log::info!("{}", core::str::from_utf8(data).unwrap());
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
