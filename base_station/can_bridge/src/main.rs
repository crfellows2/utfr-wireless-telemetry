use std::{collections::HashMap, convert::Infallible, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use clap::Parser;
use futures::stream::{self, Stream, StreamExt};
use nusb::hotplug::HotplugEvent;
use serde::Serialize;
use tokio::{sync::watch, time::sleep};
use tower_http::services::ServeDir;
use tracing::info;
use tracing_subscriber::EnvFilter;

mod config;
mod mqtt;

#[derive(Clone, Serialize)]
struct DeviceState {
    devices: Vec<String>,
    connected: Option<String>,
}

struct AppState {
    device_tx: watch::Sender<DeviceState>,
}

#[derive(Parser, Debug)]
#[command(name = "can_bridge")]
#[command(about = "CAN bridge MQTT publisher with web server", long_about = None)]
struct Args {
    /// MQTT broker address (e.g., "mosquitto", "telemetry.local", or an IP address)
    #[arg(short, long, env = "MQTT_BROKER")]
    broker: String,

    /// Configuration directory path
    #[arg(long, env = "CONFIG_DIR")]
    config_dir: String,
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .compact()
        .init();

    dotenvy::dotenv().ok();
    let args = Args::parse();

    config::init(args.config_dir);
    config::load_from_disk().await.unwrap();

    // Create watch channel for USB device updates (always holds latest state)
    let (device_tx, _) = watch::channel(DeviceState {
        devices: Vec::new(),
        connected: None,
    });
    let state = Arc::new(AppState {
        device_tx: device_tx.clone(),
    });

    tokio::spawn(ls_usb(device_tx));
    // tokio::spawn(mqtt::mqtt_publisher(args.broker.clone()));

    // Set up Axum web server
    let app = Router::new()
        .route("/api/devices/stream", get(sse_devices))
        .route("/api/devices/:id/connect", post(connect_device))
        .route("/api/devices/disconnect", post(disconnect_device))
        .with_state(state)
        .merge(config::routes::router())
        .nest_service("/", ServeDir::new("frontend/dist"));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    info!("Web server listening on http://0.0.0.0:8080");
    axum::serve(listener, app).await.unwrap();
}

async fn sse_devices(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.device_tx.subscribe();
    rx.mark_changed();

    let stream = stream::unfold(rx, |mut rx| async move {
        if rx.changed().await.is_err() {
            return None; // Channel closed
        }
        let device_state = rx.borrow_and_update().clone();
        let event = Event::default().json_data(&device_state).unwrap();
        info!(
            "Sending device state: {:?} devices, connected: {:?}",
            device_state.devices.len(),
            device_state.connected
        );
        Some((Ok(event), rx))
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn connect_device(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    info!("Connecting to device: {}", id);

    // Find and open the USB device
    match nusb::list_devices().await {
        Ok(devices) => {
            for device_info in devices {
                if device_info.vendor_id() == 0x303a && device_info.product_id() == 0x1001 {
                    let device_id = format!("Bus {} Device {}", device_info.busnum(), device_info.device_address());

                    if device_id == id {
                        info!("Found device, attempting to open...");

                        match device_info.open().await {
                            Ok(device) => {
                                info!("Device opened successfully!");

                                // Try to read some data
                                match device.claim_interface(0).await {
                                    Ok(interface) => {
                                        info!("Claimed interface 0");

                                        // Try bulk read from endpoint (adjust endpoint as needed)
                                        match interface.bulk_in::<64>(0x81).await {
                                            Ok(data) => {
                                                info!("Read {} bytes: {:?}", data.actual_data().len(), data.actual_data());
                                            }
                                            Err(e) => {
                                                info!("Bulk read error: {:?}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        info!("Failed to claim interface: {:?}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                info!("Failed to open device: {:?}", e);
                                return StatusCode::INTERNAL_SERVER_ERROR;
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            info!("Failed to list devices: {:?}", e);
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    }

    state.device_tx.send_modify(|device_state| {
        device_state.connected = Some(id.clone());
    });

    StatusCode::OK
}

async fn disconnect_device(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    info!("Disconnecting device");

    // TODO: Actually close USB device connection here

    state.device_tx.send_modify(|device_state| {
        device_state.connected = None;
    });

    StatusCode::OK
}

async fn ls_usb(tx: watch::Sender<DeviceState>) {
    let mut devices: HashMap<nusb::DeviceId, String> = HashMap::new();

    // Get initial device list
    for device in nusb::list_devices().await.unwrap() {
        if device.vendor_id() == 0x303a && device.product_id() == 0x1001 {
            let id = format!("Bus {} Device {}", device.busnum(), device.device_address());
            devices.insert(device.id(), id);
        }
    }

    info!("Initial device scan: {} devices", devices.len());
    tx.send_modify(|state| {
        state.devices = devices.values().cloned().collect();
    });

    // Watch for hotplug events
    let mut hotplug = nusb::watch_devices().unwrap();

    while let Some(event) = hotplug.next().await {
        match event {
            HotplugEvent::Connected(device_info) => {
                if device_info.vendor_id() == 0x303a && device_info.product_id() == 0x1001 {
                    let id = format!(
                        "Bus {} Device {}",
                        device_info.busnum(),
                        device_info.device_address()
                    );
                    info!("Device connected: {}", id);
                    devices.insert(device_info.id(), id.clone());

                    tx.send_modify(|state| {
                        state.devices = devices.values().cloned().collect();
                    });
                }
            }
            HotplugEvent::Disconnected(device_id) => {
                if let Some(removed_id) = devices.remove(&device_id) {
                    info!("Device disconnected: {}", removed_id);

                    tx.send_modify(|state| {
                        state.devices = devices.values().cloned().collect();

                        // If the disconnected device was the connected one, clear it
                        if state.connected.as_ref() == Some(&removed_id) {
                            info!("Connected device was unplugged, clearing connection");
                            state.connected = None;
                        }
                    });
                }
            }
        }
    }
}
