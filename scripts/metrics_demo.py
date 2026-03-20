# Subscribes to decoded CAN signals over MQTT, computes a metric, and publishes
# the result back to the broker.
#
# TOPIC STRUCTURE
#   Raw signals:     can/<bus>/<message>/<signal>
#                    e.g. can/bus0/HIGHSPEED/MotorSpeed
#   Unknown frames:  can/<bus>/unknown/<id>
#                    e.g. can/bus0/unknown/0x1A3
#   Metrics:         metrics/<author>/<metric>
#                    e.g. metrics/john/power_estimate
#
#   MQTT wildcards work as expected:
#     can/#                           all signals
#     can/bus0/HIGHSPEED/#            all signals in one message
#     can/+/+/MotorSpeed             one signal across all buses and messages
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
#   python3 metrics_demo.py
#   Override broker: MQTT_BROKER=192.168.1.100 python3 metrics_demo.py
#
#   This script is not Python-specific — any MQTT client works.
#   Connect on port 1883 (TCP) or port 9001 (WebSockets) for browser/MATLAB.
#
# DEPENDENCIES
#   pip install paho-mqtt

import json
import os

import paho.mqtt.client as mqtt

MQTT_BROKER = os.environ.get("MQTT_BROKER", "telemetry.local")

latest = {}


def on_message(client, userdata, msg):
    data = json.loads(msg.payload)
    latest[msg.topic] = data["value"]

    motor_rpm = latest.get("can/bus0/HIGHSPEED/MotorSpeed")
    fl_speed = latest.get("can/bus1/FRONT_WHEELSPEEDS/FL_WHEELSPEED")

    if motor_rpm is not None and fl_speed is not None:
        # Simple slip ratio: (driven wheel speed - free wheel speed) / free wheel speed
        slip = (motor_rpm - fl_speed) / max(fl_speed, 0.1)
        client.publish(
            "metrics/demo/rear_slip_ratio",
            json.dumps(
                {
                    "ts": data["ts"],
                    "value": round(slip, 4),
                }
            ),
        )


client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2)
client.on_message = on_message
client.connect(MQTT_BROKER, 1883)
client.subscribe("can/bus0/HIGHSPEED/MotorSpeed")
client.subscribe("can/bus1/FRONT_WHEELSPEEDS/FL_WHEELSPEED")
client.loop_forever()
