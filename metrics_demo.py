# Subscribes to decoded CAN signals over MQTT, computes a metric, and publishes
# the result back to the broker.
#
# TOPIC STRUCTURE
#   Raw signals:     can/<bus>/<message>/<signal>
#                    e.g. can/bus0/EngineData/engine_rpm
#   Unknown frames:  can/<bus>/unknown/<id>
#                    e.g. can/bus0/unknown/0x1A3
#   Metrics:         metrics/<author>/<metric>
#                    e.g. metrics/john/power_estimate
#
#   MQTT wildcards work as expected:
#     can/#                      all signals
#     can/bus0/EngineData/#      all signals in one message
#     can/+/+/engine_rpm         one signal across all buses and messages
#
# PAYLOAD FORMAT
#   Every signal topic carries a JSON payload:
#     {"ts": <unix timestamp (float)>, "value": <float>}
#   Signal values are always numeric — DBC decoding always produces a float
#   via: decoded = raw * scale + offset.
#   JSON is used for Grafana compatibility. Publish your metrics in the same
#   format for consistency.
#
# RUNNING
#   On the LAN:       set BROKER = "telemetry.local"  (recommended)
#   On the Pi:        set BROKER = "localhost"
#   In the compose:   set BROKER = "mosquitto"
#
#   This script is not Python-specific — any MQTT client works.
#   Connect on port 1883 (TCP) or port 9001 (WebSockets) for browser/MATLAB.
#
# DEPENDENCIES
#   pip install paho-mqtt

import json

import paho.mqtt.client as mqtt

BROKER = "telemetry.local"

latest = {}


def on_message(client, userdata, msg):
    data = json.loads(msg.payload)
    latest[msg.topic] = data["value"]

    rpm = latest.get("can/bus0/EngineData/engine_rpm")
    speed = latest.get("can/bus0/WheelSpeeds/fl_wheel_speed")

    if rpm is not None and speed is not None:
        metric = rpm / max(speed, 0.1)  # avoid division by zero
        client.publish(
            "metrics/demo/rpm_per_speed",
            json.dumps(
                {
                    "ts": data["ts"],
                    "value": metric,
                }
            ),
        )


client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2)
client.on_message = on_message
client.connect(BROKER, 1883)
client.subscribe("can/bus0/EngineData/engine_rpm")
client.subscribe("can/bus0/WheelSpeeds/fl_wheel_speed")
client.loop_forever()
