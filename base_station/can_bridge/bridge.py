import paho.mqtt.client as mqtt
import json
import time
import random

BROKER = "mosquitto"
PORT = 1883

client = mqtt.Client()
client.connect(BROKER, PORT)
client.loop_start()

print("Publishing fake CAN data to broker...")

while True:
    signals = {
        "can/bus0/engine_rpm":   round(random.uniform(3000, 8000), 1),
        "can/bus0/vehicle_speed": round(random.uniform(0, 150), 2),
        "can/bus0/coolant_temp":  round(random.uniform(70, 105), 1),
    }

    for topic, value in signals.items():
        payload = json.dumps({"ts": time.time(), "value": value})
        client.publish(topic, payload)

    time.sleep(0.1)
