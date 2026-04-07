use std::ffi::c_void;
use std::ptr::addr_of_mut;
use std::sync::atomic::{AtomicUsize, Ordering};

use embassy_sync::channel::Sender;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use esp32_nimble::{uuid128, BLEAdvertisementData, BLEDevice, NimbleProperties};
use esp_idf_svc::hal::can::Frame;
use esp_idf_svc::sys::{
    ble_gap_event, ble_gap_event_listener, ble_gap_event_listener_register, ble_gap_set_data_len,
    BLE_GAP_EVENT_MTU,
};
use log::info;
use protocol::{CanFrame, ExtendedId, StandardId};

static MTU: AtomicUsize = AtomicUsize::new(23);
const MAX_MTU: usize = 527;

static mut GAP_LISTENER: ble_gap_event_listener = unsafe { std::mem::zeroed() };

unsafe extern "C" fn on_gap_event(event: *mut ble_gap_event, _arg: *mut c_void) -> i32 {
    let event = &*event;
    if event.type_ == BLE_GAP_EVENT_MTU as u8 {
        let mtu = event.__bindgen_anon_1.mtu.value;
        log::info!("MTU updated to {}", mtu);
        MTU.store(mtu as usize, Ordering::Relaxed);
    }
    0
}

pub async fn ble_task<const N: usize>(
    ble_tx_oneshot: &Signal<CriticalSectionRawMutex, BleCanLink>,
    can_write_tx: Sender<'static, CriticalSectionRawMutex, protocol::CanFrame, N>,
) -> anyhow::Result<()> {
    let ble_device = BLEDevice::take();

    ble_device
        .set_preferred_mtu(MAX_MTU.try_into().unwrap())
        .expect("Could not set preferred MTU"); // 502 is multiple of 251 (DLE) and within max of esp_idf_sys::BLE_ATT_MTU_MAX=527
    ble_device
        .set_power(
            esp32_nimble::enums::PowerType::Default,
            esp32_nimble::enums::PowerLevel::P9,
        )
        .expect("Could not set BLE power level");

    // Register GAP event listener for MTU updates
    unsafe {
        let rc = ble_gap_event_listener_register(
            addr_of_mut!(GAP_LISTENER),
            Some(on_gap_event),
            std::ptr::null_mut(),
        );
        assert_eq!(rc, 0, "Failed to register GAP listener");
    }

    let server = ble_device.get_server();
    server.on_connect(|server, desc| {
        ::log::info!("Client connected: {:?}", desc);

        // TODO: properly allow only one connection at a time
        assert_eq!(desc.conn_handle(), 0, "Should only connect to one central");

        // TODO: verify this is actually applied
        unsafe {
            let rc = ble_gap_set_data_len(desc.conn_handle(), 251, 2120);
            if rc != 0 {
                log::warn!("ble_gap_set_data_len failed: {}", rc);
            } else {
                log::info!("Called ble_gap_set_data_len()");
            }
        }

        // TODO: optimize connection interval for range
        server
            .update_conn_params(desc.conn_handle(), 40, 48, 0, 60)
            .unwrap();
    });

    server.on_disconnect(|_desc, reason| {
        ::log::info!("Client disconnected ({:?})", reason);
    });

    let service = server.create_service(uuid128!("fafafafa-fafa-fafa-fafa-fafafafafafa"));

    // Read (notify) and write CAN frames
    let telemetry_characteristic = service.lock().create_characteristic(
        uuid128!("a3c87500-8ed3-4bdf-8a39-a01bebede295"),
        NimbleProperties::WRITE | NimbleProperties::WRITE_NO_RSP | NimbleProperties::NOTIFY,
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
        ::log::info!("BLE char recv: {}", msg);
    });

    let ble_advertising = ble_device.get_advertising();
    ble_advertising.lock().set_data(
        BLEAdvertisementData::new()
            .name("ESP32-GATT-Server")
            .add_service_uuid(uuid128!("fafafafa-fafa-fafa-fafa-fafafafafafa")),
    )?;
    ble_advertising.lock().start()?;

    // crate::can_interface::TX_CHAR.signal(telemetry_characteristic);
    let ble_tx = BleCanLink::new(telemetry_characteristic, can_write_tx);
    ble_tx_oneshot.signal(ble_tx);

    // Listen for SD status updates and notify
    loop {
        let sd_status = crate::sd_logger::SD_STATUS.wait().await;
        let data = sd_status.to_ble_bytes();
        status_characteristic.lock().set_value(&data).notify();
        // ::log::info!(
        //     "SD status: {}/{} KB used",
        //     sd_status.used_kb,
        //     sd_status.total_kb
        // );
    }
}

/// Serialize, batch, rate limit, and send frames over BLE
pub struct BleCanLink {
    tx: std::sync::Arc<esp32_nimble::utilities::mutex::Mutex<esp32_nimble::BLECharacteristic>>,
    send_buf: heapless::Vec<u8, MAX_MTU>,
}

impl BleCanLink {
    pub fn new<const N: usize>(
        tx: std::sync::Arc<esp32_nimble::utilities::mutex::Mutex<esp32_nimble::BLECharacteristic>>,
        can_write_tx: Sender<'static, CriticalSectionRawMutex, protocol::CanFrame, N>,
    ) -> Self {
        tx.lock().on_write(move |args| {
            let data = args.recv_data();

            // Data contains COBS-encoded frames separated by 0x00
            for chunk in data.split(|&b| b == 0x00) {
                if chunk.is_empty() {
                    continue;
                }

                // Need a mutable copy for COBS decoding
                let mut buf: heapless::Vec<u8, MAX_MTU> =
                    heapless::Vec::from_slice(chunk).unwrap();

                match postcard::from_bytes_cobs::<protocol::CanFrame>(&mut buf) {
                    Ok(frame) => {
                        let _ = can_write_tx.try_send(frame);
                    }
                    Err(e) => log::error!("Failed to deserialize BLE CAN frame: {e:?}"),
                }
            }
        });

        BleCanLink {
            tx,
            send_buf: heapless::Vec::new(),
        }
    }

    /// Send frame over BLE (non-blocking)
    pub fn send_can_frame(&mut self, frame: &Frame) {
        const SERIALIZED_LEN: usize = 100;

        let raw_id = frame.identifier();
        let id: protocol::CanId = if frame.is_extended() {
            protocol::CanId::Extended(ExtendedId::new(raw_id).unwrap())
        } else {
            protocol::CanId::Standard(StandardId::new(raw_id.try_into().unwrap()).unwrap())
        };
        let payload = frame.data().try_into().unwrap();

        let frame = CanFrame { id, payload };
        let serialized = postcard::to_vec_cobs::<_, SERIALIZED_LEN>(&frame).unwrap();

        // Fill mtu to force the BLE controller to break up into multiple packets to
        // send in one connection interval.
        let mtu = MTU.load(Ordering::Relaxed);

        if serialized.len() > mtu {
            log::error!("MTU too small for can packet: {:?}", &frame);
            return;
        }

        // Note: if the mtu shrinks the data we send could be truncated. COBS should handle that
        // fine though and MTU should never shrink anyways
        if serialized.len() + self.send_buf.len() > mtu {
            // flush buf to BLE
            self.tx.lock().set_value(&self.send_buf).notify();
            // info!("Sent {} bytes", &self.send_buf.len());
            self.send_buf.clear();
        }

        self.send_buf
            .extend_from_slice(&serialized)
            .expect("send_buf should have room for this extension");
    }
}
