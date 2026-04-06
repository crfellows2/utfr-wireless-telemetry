use std::{collections::HashSet, convert::Infallible, io::BufRead, sync::Arc, time::Duration};

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
use futures::stream::{self, Stream};
use serde::Serialize;
use tokio::sync::{mpsc, watch};
use tokio::time::sleep;
use tower_http::services::ServeDir;
use tracing::info;
use tracing_subscriber::EnvFilter;

mod can_recv;
mod config;
mod mqtt;
mod usb;

#[derive(Clone, Serialize)]
struct DeviceState {
    devices: Vec<String>,
    connected: Option<String>,
}

struct AppState {
    device_tx: watch::Sender<DeviceState>,
    telemetry_tx: mpsc::Sender<mqtt::TelemetryMessage>,
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
    init_tracing().expect("Could not init tracing");
    dotenvy::dotenv().ok();
    let args = Args::parse();

    config::init(args.config_dir);
    config::load_from_disk().await.unwrap();

    // Create watch channel for USB device updates (always holds latest state)
    let (device_tx, _) = watch::channel(DeviceState {
        devices: Vec::new(),
        connected: None,
    });

    // Create channel for telemetry data from serial to MQTT
    let (telemetry_tx, telemetry_rx) = mqtt::create_channel();

    let state = Arc::new(AppState {
        device_tx: device_tx.clone(),
        telemetry_tx,
    });

    tokio::spawn(ls_usb(device_tx));
    tokio::spawn(mqtt::mqtt_publisher(args.broker.clone(), telemetry_rx));

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

fn init_tracing() -> Result<(), Box<dyn core::error::Error + Send + Sync + 'static>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .compact()
        .try_init()
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

    // Open serial port
    match serialport::new(&id, 115200)
        .timeout(Duration::from_secs(10))
        .open()
    {
        Ok(port) => {
            info!("Serial port opened: {}", id);

            // Spawn task to read and parse serial data
            let port_name = id.clone();
            let telemetry_tx = state.telemetry_tx.clone();
            tokio::task::spawn_blocking(move || {
                let mut buf = String::new();
                let mut reader = std::io::BufReader::new(port);

                loop {
                    buf.clear();
                    match reader.read_line(&mut buf) {
                        Ok(0) => {
                            info!("[{}] Device disconnected", port_name);
                            break;
                        }
                        Ok(_) => {
                            if let Some(msg) = mqtt::parse_line(buf.trim_end()) {
                                if telemetry_tx.blocking_send(msg).is_err() {
                                    break;
                                }
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                        Err(e) => {
                            info!("[{}] Read error: {:?}", port_name, e);
                            break;
                        }
                    }
                }
            });

            state.device_tx.send_modify(|device_state| {
                device_state.connected = Some(id.clone());
            });

            StatusCode::OK
        }
        Err(e) => {
            info!("Failed to open serial port: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
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
    let mut previous_devices: HashSet<String> = HashSet::new();

    loop {
        let mut current_devices: HashSet<String> = HashSet::new();

        // Poll for serial ports
        if let Ok(ports) = serialport::available_ports() {
            for port in ports {
                // Filter for ESP32 devices (VID: 0x303a, PID: 0x1001)
                if let serialport::SerialPortType::UsbPort(usb_info) = &port.port_type
                    && usb_info.vid == 0x303a
                    && usb_info.pid == 0x1001
                {
                    current_devices.insert(port.port_name.clone());
                }
            }
        }

        // Detect changes
        let added: Vec<_> = current_devices
            .difference(&previous_devices)
            .cloned()
            .collect();
        let removed: Vec<_> = previous_devices
            .difference(&current_devices)
            .cloned()
            .collect();

        if !added.is_empty() || !removed.is_empty() {
            for port in &added {
                info!("Device connected: {}", port);
            }
            for port in &removed {
                info!("Device disconnected: {}", port);
            }

            tx.send_modify(|state| {
                state.devices = current_devices.iter().cloned().collect();

                // If the disconnected device was the connected one, clear it
                if let Some(ref connected_id) = state.connected
                    && !current_devices.contains(connected_id)
                {
                    info!("Connected device was unplugged, clearing connection");
                    state.connected = None;
                }
            });
        }

        previous_devices = current_devices;

        sleep(Duration::from_millis(1000)).await;
    }
}
