# UTFR Driver Performance Analysis
#
# Scores driver technique in real-time and generates recommendations.
# Publishes numeric metrics as JSON, text recommendations as raw strings.
#
# USAGE:
#   python3 driver_analysis.py
#   Override broker: MQTT_BROKER=192.168.1.100 python3 driver_analysis.py

import json, math, time, os, collections
import paho.mqtt.client as mqtt

MQTT_BROKER = os.environ.get("MQTT_BROKER", "telemetry.local")
PORT = 1883
latest = {}

# Rolling windows
throttle_hist  = collections.deque(maxlen=200)
brake_hist     = collections.deque(maxlen=200)
steer_hist     = collections.deque(maxlen=200)
overlap_events = collections.deque(maxlen=600)
coast_hist     = collections.deque(maxlen=200)
lat_g_hist     = collections.deque(maxlen=200)
brake_peaks    = collections.deque(maxlen=20)  # last 20 braking events

# State
in_brake_event = False
current_brake_peak = 0
total_samples = 0
last_rec_time = 0

def get(t):
    return latest.get(t)

def pub_num(client, name, label, value):
    if value is not None and math.isfinite(value):
        client.publish(f"driver/{name}", json.dumps({label: round(value, 2)}))

def pub_rec(client, code):
    """Publish recommendation as numeric code. Grafana value mappings display text."""
    client.publish("driver/recommendation", json.dumps({"Advice": code}))

def analyze(client):
    global in_brake_event, current_brake_peak, total_samples, last_rec_time
    now = time.time()
    total_samples += 1

    throttle = get("can/bus0/APPS/ProcessedThrottle")
    fb = get("can/bus0/BRAKE_PRESSURE/FrontBrakePressure")
    rb = get("can/bus0/BRAKE_PRESSURE/RearBrakePressure")
    steer = get("can/bus0/SAS_DATA/LWS_ANGLE")
    acc_y = get("can/bus0/GPS_ACCELERATION/accY")
    acc_x = get("can/bus0/GPS_ACCELERATION/accX")
    fl = get("can/bus1/FRONT_WHEELSPEEDS/FL_WHEELSPEED")
    fr = get("can/bus1/FRONT_WHEELSPEEDS/FR_WHEELSPEED")
    rl = get("can/bus0/RC_STATE/RL_WHEELSPEED")
    rr = get("can/bus0/RC_STATE/RR_WHEELSPEED")

    brake_total = (fb or 0) + (rb or 0)

    # ═══ 1. THROTTLE SMOOTHNESS ═══
    if throttle is not None:
        throttle_hist.append((now, throttle))
        if len(throttle_hist) >= 15:
            vals = [v for _, v in throttle_hist][-50:]
            diffs = [abs(vals[i] - vals[i-1]) for i in range(1, len(vals))]
            avg_jerk = sum(diffs) / len(diffs) / 0.1  # per second
            # Map: jerk 0 → 100, jerk 500+ → 0
            score = max(0, min(100, 100 - avg_jerk * 0.18))
            pub_num(client, "throttle_smoothness", "Throttle Smoothness", score)

    # ═══ 2. STEERING SMOOTHNESS ═══
    if steer is not None:
        steer_hist.append((now, steer))
        if len(steer_hist) >= 15:
            vals = [v for _, v in steer_hist][-50:]
            diffs = [abs(vals[i] - vals[i-1]) for i in range(1, len(vals))]
            avg_rate = sum(diffs) / len(diffs) / 0.1
            score = max(0, min(100, 100 - (avg_rate - 15) * 0.3))
            pub_num(client, "steer_smoothness", "Steering Smoothness", score)

    # ═══ 3. PEDAL OVERLAP ═══
    if throttle is not None:
        is_overlap = throttle > 8 and brake_total > 30
        overlap_events.append((now, 1 if is_overlap else 0))
        recent = [(t, v) for t, v in overlap_events if now - t < 30]
        if len(recent) > 20:
            pct = sum(v for _, v in recent) / len(recent) * 100
            pub_num(client, "overlap_pct", "Overlap (%)", pct)
            score = max(0, min(100, 100 - pct * 4))
            pub_num(client, "overlap_score", "Pedal Discipline", score)

    # ═══ 4. COASTING ═══
    if throttle is not None:
        is_coasting = throttle < 5 and brake_total < 20
        coast_hist.append((now, 1 if is_coasting else 0))
        recent = [(t, v) for t, v in coast_hist if now - t < 15]
        if len(recent) > 10:
            pct = sum(v for _, v in recent) / len(recent) * 100
            pub_num(client, "coast_pct", "Coasting (%)", pct)

    # ═══ 5. BRAKE CONSISTENCY ═══
    if fb is not None:
        if fb > 200 and not in_brake_event:
            in_brake_event = True
            current_brake_peak = fb
        elif fb > 200 and in_brake_event:
            current_brake_peak = max(current_brake_peak, fb)
        elif fb < 50 and in_brake_event:
            # End of braking event — record peak
            brake_peaks.append(current_brake_peak)
            in_brake_event = False
            current_brake_peak = 0

        if len(brake_peaks) >= 3:
            mean_p = sum(brake_peaks) / len(brake_peaks)
            if mean_p > 50:
                variance = sum((p - mean_p)**2 for p in brake_peaks) / len(brake_peaks)
                cv = math.sqrt(variance) / mean_p  # coefficient of variation
                # CV of 0 → 100, CV of 0.5+ → 0
                score = max(0, min(100, 100 - cv * 200))
                pub_num(client, "brake_consistency", "Brake Consistency", score)

    # ═══ 6. GRIP UTILIZATION ═══
    if acc_y is not None and acc_x is not None:
        lat_g = abs(acc_y / 9.81)
        long_g = abs(acc_x / 9.81)
        lat_g_hist.append(lat_g)
        combined = math.sqrt(lat_g**2 + long_g**2)
        util = min(100, combined / 1.6 * 100)
        pub_num(client, "grip_util", "Grip Used (%)", util)
        if len(lat_g_hist) >= 20:
            pub_num(client, "peak_lat_g", "Peak Lat G", max(list(lat_g_hist)[-100:]))

    # ═══ 7. WHEELSPIN DETECTION ═══
    if fl and fr and rl and rr:
        f_avg = (fl + fr) / 2
        r_avg = (rl + rr) / 2
        if f_avg > 50:
            slip_pct = (r_avg - f_avg) / f_avg * 100
            pub_num(client, "current_slip", "Rear Slip (%)", slip_pct)

    # ═══ 8. OVERALL SCORE ═══
    scores = {}
    # Recalculate from recent data
    if len(throttle_hist) >= 15:
        vals = [v for _, v in throttle_hist][-50:]
        diffs = [abs(vals[i] - vals[i-1]) for i in range(1, len(vals))]
        scores['throttle'] = max(0, min(100, 100 - sum(diffs)/len(diffs)/0.1 * 0.18))

    if len(steer_hist) >= 15:
        vals = [v for _, v in steer_hist][-50:]
        diffs = [abs(vals[i] - vals[i-1]) for i in range(1, len(vals))]
        scores['steering'] = max(0, min(100, 100 - (sum(diffs)/len(diffs)/0.1 - 15) * 0.3))

    recent_ol = [(t, v) for t, v in overlap_events if now - t < 30]
    if len(recent_ol) > 20:
        pct = sum(v for _, v in recent_ol) / len(recent_ol) * 100
        scores['pedals'] = max(0, min(100, 100 - pct * 4))

    if scores:
        w = {'throttle': 0.3, 'steering': 0.3, 'pedals': 0.4}
        tw = sum(w.get(k, 0) for k in scores)
        if tw > 0:
            overall = sum(scores.get(k, 50) * w.get(k, 0) for k in scores) / tw
            pub_num(client, "overall_score", "Driver Score", overall)

    # ═══ 9. RECOMMENDATIONS (every 3s) ═══
    # Publishes highest-priority issue as numeric code (0-5)
    # Grafana value mappings convert to readable text
    if now - last_rec_time > 3:
        code = 0  # default: all good

        if 'throttle' in scores and scores['throttle'] < 65:
            code = 1  # throttle jerk

        if 'steering' in scores and scores['steering'] < 60:
            code = 2  # steering corrections

        recent_ol = [(t, v) for t, v in overlap_events if now - t < 10]
        if len(recent_ol) > 5:
            ol = sum(v for _, v in recent_ol) / len(recent_ol) * 100
            if ol > 10:
                code = 3  # pedal overlap

        recent_c = [(t, v) for t, v in coast_hist if now - t < 10]
        if len(recent_c) > 5:
            cp = sum(v for _, v in recent_c) / len(recent_c) * 100
            if cp > 25:
                code = 4  # coasting

        if fl and fr and rl and rr:
            f_avg = (fl + fr) / 2
            r_avg = (rl + rr) / 2
            if f_avg > 50 and (r_avg - f_avg) / f_avg > 0.12:
                code = 5  # wheelspin (highest priority)

        pub_rec(client, code)
        last_rec_time = now


def on_message(client, userdata, msg):
    try:
        payload = msg.payload.decode()
        try:
            data = json.loads(payload)
            if isinstance(data, dict):
                for k, v in data.items():
                    latest[msg.topic] = float(v); break
            else:
                latest[msg.topic] = float(data)
        except (json.JSONDecodeError, ValueError):
            try:
                latest[msg.topic] = float(payload)
            except ValueError:
                pass
    except Exception:
        pass

def on_connect(client, userdata, flags, rc):
    print(f"Connected to {MQTT_BROKER}:{PORT}")
    client.subscribe("can/#")
    print("Analyzing driver performance at 10Hz...")

client = mqtt.Client()
client.on_connect = on_connect
client.on_message = on_message
client.connect(MQTT_BROKER, PORT)
client.loop_start()

print(f"UTFR Driver Analysis — connecting to {MQTT_BROKER}:{PORT}")
try:
    while True:
        analyze(client)
        time.sleep(0.1)
except KeyboardInterrupt:
    print("\nStopped.")
    client.loop_stop()
