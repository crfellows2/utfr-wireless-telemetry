use rumqttc::{AsyncClient, MqttOptions, QoS};
use serde_json::json;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info};

/// Messages from serial reader to MQTT publisher
#[derive(Debug)]
pub enum TelemetryMessage {
    /// CAN frame: bus_id, can_id, data (hex string), timestamp
    Can {
        bus_id: u8,
        can_id: u32,
        data_len: u8,
        data_hex: String,
        timestamp: f64,
    },
    /// SD card status: used_kb, total_kb
    SdStatus { used_kb: u32, total_kb: u32 },
}

/// Parse a line from serial, returns Some(message) if it's a $ prefixed data line
pub fn parse_line(line: &str) -> Option<TelemetryMessage> {
    let line = line.trim();
    if !line.starts_with('$') {
        return None;
    }

    let line = &line[1..]; // strip $
    let parts: Vec<&str> = line.split_whitespace().collect();

    if parts.is_empty() {
        return None;
    }

    // $CAN<bus> <id_hex> <len> <data_hex> <timestamp>
    // e.g. $CAN0 055 4 000001FB 946685603.097562
    if let Some(rest) = parts[0].strip_prefix("CAN") {
        if parts.len() >= 5 {
            let bus_id = rest.parse().ok()?;
            let can_id = u32::from_str_radix(parts[1], 16).ok()?;
            let data_len = parts[2].parse().ok()?;
            let data_hex = parts[3].to_string();
            let timestamp = parts[4].parse().ok()?;
            return Some(TelemetryMessage::Can {
                bus_id,
                can_id,
                data_len,
                data_hex,
                timestamp,
            });
        }
    }

    // $SD <used_kb> <total_kb>
    if parts[0] == "SD" && parts.len() >= 3 {
        let used_kb = parts[1].parse().ok()?;
        let total_kb = parts[2].parse().ok()?;
        return Some(TelemetryMessage::SdStatus { used_kb, total_kb });
    }

    None
}

/// Create a channel for telemetry messages
pub fn create_channel() -> (mpsc::Sender<TelemetryMessage>, mpsc::Receiver<TelemetryMessage>) {
    mpsc::channel(256)
}

pub async fn mqtt_publisher(broker: String, mut rx: mpsc::Receiver<TelemetryMessage>) {
    let mqtt_options = MqttOptions::new("can_bridge", &broker, 1883);

    info!("Connecting to MQTT broker at {}:1883", &broker);

    let (client, mut eventloop) = AsyncClient::new(mqtt_options, 10);

    // Spawn event loop handler
    tokio::spawn(async move {
        loop {
            if let Err(e) = eventloop.poll().await {
                error!("MQTT error: {}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    });

    info!("MQTT publisher ready, waiting for telemetry data...");

    while let Some(msg) = rx.recv().await {
        match msg {
            TelemetryMessage::Can {
                bus_id,
                can_id,
                data_len: _,
                data_hex,
                timestamp,
            } => {
                // Hardcode ID 0x55 to heartbeat for now
                let topic = if can_id == 0x55 {
                    format!("can/bus{}/heartbeat", bus_id)
                } else {
                    format!("can/bus{}/{:03X}", bus_id, can_id)
                };

                let payload = json!({
                    "ts": timestamp,
                    "data": data_hex
                });

                if let Err(e) = client
                    .publish(&topic, QoS::AtMostOnce, false, payload.to_string().as_bytes())
                    .await
                {
                    error!("Failed to publish CAN: {}", e);
                }
            }
            TelemetryMessage::SdStatus { used_kb, total_kb } => {
                let payload = json!({
                    "used_kb": used_kb,
                    "total_kb": total_kb,
                    "used_mb": used_kb / 1024,
                    "total_mb": total_kb / 1024,
                });

                if let Err(e) = client
                    .publish("link/storage", QoS::AtMostOnce, false, payload.to_string().as_bytes())
                    .await
                {
                    error!("Failed to publish SD status: {}", e);
                }
            }
        }
    }

    info!("MQTT publisher channel closed");
}
