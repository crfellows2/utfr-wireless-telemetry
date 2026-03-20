use axum::{Router, routing::get};
use clap::Parser;
use rumqttc::{AsyncClient, MqttOptions, QoS};
use serde_json::json;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

mod config;

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

async fn mqtt_publisher(broker: String) {
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

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .compact()
        .init();

    dotenvy::dotenv().ok();
    let args = Args::parse();

    config::init(args.config_dir);
    config::load_from_disk().await.unwrap();

    tokio::spawn(mqtt_publisher(args.broker.clone()));

    // Set up Axum web server
    let app = Router::new()
        .route(
            "/",
            get(|| async { axum::response::Html(include_str!("../static/index.html")) }),
        )
        .merge(config::routes::router());
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    info!("Web server listening on http://0.0.0.0:8080");
    axum::serve(listener, app).await.unwrap();
}
