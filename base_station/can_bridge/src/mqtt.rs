use rumqttc::{AsyncClient, MqttOptions, QoS};
use serde_json::json;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{error, info};

pub async fn mqtt_publisher(broker: String) {
    let mqtt_options = MqttOptions::new("can_bridge", &broker, 1883);

    info!("Connecting to MQTT broker at {}:1883", &broker);

    let (client, mut eventloop) = AsyncClient::new(mqtt_options, 10);

    info!("Publishing fake CAN data to broker...");

    // Spawn event loop handler
    // We have to poll the client to drive it and make progress
    tokio::spawn(async move {
        loop {
            if let Err(e) = eventloop.poll().await {
                error!("MQTT error: {}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    });

    // Test values (constants for connection testing)
    let signals = [
        ("can/bus0/HIGHSPEED/MotorSpeed", 3200.0),
        ("can/bus0/BMS_TEMPERATURES/AccuCellHighTemp", 32.5),
        ("can/bus0/INVTEMPS3/CoolantTemp", 38.0),
    ];

    loop {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();

        for (topic, value) in &signals {
            let payload = json!({
                "ts": timestamp,
                "value": value
            });

            if let Err(e) = client
                .publish(
                    *topic,
                    QoS::AtLeastOnce,
                    false,
                    payload.to_string().as_bytes(),
                )
                .await
            {
                error!("Failed to publish: {}", e);
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
