# UTFR Math Channels — Computed Telemetry Metrics
#
# Subscribes to raw CAN signals, computes derived metrics at 10Hz,
# and publishes them back to metrics/<name> topics.
#
# USAGE:
#   python3 math_channels.py                        (uses localhost)
#   BROKER=telemetry.local python3 math_channels.py (LAN)

import json, math, time, os, collections
import paho.mqtt.client as mqtt

BROKER = os.environ.get("BROKER", "localhost")
PORT = 1883

# ─── Vehicle Parameters (UTFR UT26) ───
TIRE_RADIUS_M       = 0.232
TIRE_CIRC_M         = 2 * math.pi * TIRE_RADIUS_M
WHEELBASE_M          = 1.53
STEERING_RATIO       = 4.0
GEAR_RATIO           = 4.0
TOTAL_PACK_WH        = 6500.0     # ~6.5 kWh nominal pack
TOTAL_PACK_AH        = 18.5       # Ah capacity
CELL_COUNT_SERIES    = 96         # cells in series
NOMINAL_CELL_V       = 3.6
BMS_TEMP_LIMIT       = 55.0      # °C cutoff
LAP_DISTANCE_KM      = 1.1       # Michigan endurance approx

# ─── State ───
latest = {}
energy_start = None
coulomb_start = None
distance_m = 0.0
last_time = None
last_speed = 0.0

# Rolling windows for rate-of-change calculations
temp_history = collections.deque(maxlen=60)      # 6s at 10Hz
voltage_sag_history = collections.deque(maxlen=50)
power_history = collections.deque(maxlen=100)     # 10s rolling average

def get(topic):
    return latest.get(topic)

def pub(client, name, label, value):
    if value is not None and math.isfinite(value):
        client.publish(f"metrics/{name}", json.dumps({label: round(value, 4)}))

def compute(client):
    global energy_start, coulomb_start, distance_m, last_time, last_speed
    now = time.time()

    # ── Wheel speeds (RPM → m/s) ──
    def rpm2ms(rpm):
        return rpm * TIRE_CIRC_M / 60.0 if rpm is not None else None

    fl = rpm2ms(get("can/bus1/FRONT_WHEELSPEEDS/FL_WHEELSPEED"))
    fr = rpm2ms(get("can/bus1/FRONT_WHEELSPEEDS/FR_WHEELSPEED"))
    rl = rpm2ms(get("can/bus0/RC_STATE/RL_WHEELSPEED"))
    rr = rpm2ms(get("can/bus0/RC_STATE/RR_WHEELSPEED"))

    speed = None
    if fl is not None and fr is not None:
        speed = (fl + fr) / 2.0
        pub(client, "vehicle_speed", "Speed (km/h)", speed * 3.6)

        # Accumulate distance
        if last_time is not None:
            dt = now - last_time
            if 0 < dt < 2.0:
                distance_m += speed * dt
        last_time = now
        last_speed = speed

    # ── Slip Ratios ──
    if fl and fr and rl and rr:
        front_avg = (fl + fr) / 2.0
        rear_avg = (rl + rr) / 2.0
        if front_avg > 0.5:
            pub(client, "rear_slip", "Rear Slip", (rear_avg - front_avg) / front_avg)
            pub(client, "slip_rl", "RL Slip", (rl - front_avg) / front_avg)
            pub(client, "slip_rr", "RR Slip", (rr - front_avg) / front_avg)
        # Left-right speed difference (shows differential behavior)
        if rear_avg > 0.5:
            pub(client, "rear_lr_diff", "Rear L-R Δ (m/s)", rl - rr)

    # ── Brake Bias ──
    fb = get("can/bus0/BRAKE_PRESSURE/FrontBrakePressure")
    rb = get("can/bus0/BRAKE_PRESSURE/RearBrakePressure")
    if fb is not None and rb is not None and (fb + rb) > 20:
        pub(client, "brake_bias", "Brake Bias (% front)", fb / (fb + rb) * 100)

    # ── Cell Deltas ──
    cv_hi = get("can/bus0/BMS_VOLTAGES/AccuCellHighVolt")
    cv_lo = get("can/bus0/BMS_VOLTAGES/AccuCellLowVolt")
    if cv_hi is not None and cv_lo is not None:
        pub(client, "cell_dv", "Cell ΔV (mV)", (cv_hi - cv_lo) * 1000)

    ct_hi = get("can/bus0/BMS_TEMPERATURES/AccuCellHighTemp")
    ct_lo = get("can/bus0/BMS_TEMPERATURES/AccuCellLowTemp")
    if ct_hi is not None and ct_lo is not None:
        pub(client, "cell_dt", "Cell ΔT (°C)", ct_hi - ct_lo)

    # ── Thermal Headroom & Rate ──
    if ct_hi is not None:
        headroom = BMS_TEMP_LIMIT - ct_hi
        pub(client, "thermal_headroom", "Margin to Limit (°C)", headroom)
        temp_history.append((now, ct_hi))
        if len(temp_history) >= 20:
            t0, v0 = temp_history[0]
            dt = now - t0
            if dt > 1.0:
                rate = (ct_hi - v0) / dt * 60.0  # °C per minute
                pub(client, "temp_rise_rate", "Temp Rise (°C/min)", rate)
                if rate > 0.01 and headroom > 0:
                    mins_to_limit = headroom / (rate / 60.0) / 60.0
                    pub(client, "mins_to_thermal", "Mins to Thermal Limit", mins_to_limit)

    # ── Inverter Power Cross-Check ──
    dc_v = get("can/bus0/HIGHSPEED/DcBusVoltage")
    dc_i = get("can/bus0/CURRENTINFO/DcBusCurrent")
    if dc_v is not None and dc_i is not None:
        pwr = dc_v * dc_i / 1000.0
        pub(client, "inverter_power", "Inverter Power (kW)", pwr)
        power_history.append(pwr)

    # ── Torque Efficiency ──
    t_cmd = get("can/bus0/HIGHSPEED/TorqueCommanded")
    t_act = get("can/bus0/HIGHSPEED/TorqueFeedback")
    if t_cmd is not None and t_act is not None and abs(t_cmd) > 2.0:
        pub(client, "torque_eff", "Torque Eff (%)", t_act / t_cmd * 100)

    # ── Voltage Sag (internal resistance indicator) ──
    pack_v = get("can/bus0/PACK_CURRENT/BMSVoltageSummed")
    pack_i = get("can/bus0/PACK_CURRENT/PackCurrent")
    if pack_v is not None and pack_i is not None:
        voltage_sag_history.append((pack_v, pack_i))
        if len(voltage_sag_history) >= 10:
            # Compare high-current vs low-current voltage
            sorted_pts = sorted(voltage_sag_history, key=lambda x: abs(x[1]))
            low_i = sorted_pts[:5]
            high_i = sorted_pts[-5:]
            avg_v_low = sum(p[0] for p in low_i) / 5
            avg_v_high = sum(p[0] for p in high_i) / 5
            avg_i_low = sum(abs(p[1]) for p in low_i) / 5
            avg_i_high = sum(abs(p[1]) for p in high_i) / 5
            di = avg_i_high - avg_i_low
            if di > 5.0:
                r_int = (avg_v_low - avg_v_high) / di * 1000  # mΩ
                if 0 < r_int < 500:
                    pub(client, "pack_resistance", "Pack R (mΩ)", r_int)

    # ── Energy & Range ──
    energy_now = get("can/bus0/PACK_ENERGY/EnergyCount")  # mWh
    coulomb_now = get("can/bus0/PACK_ENERGY/CoulombCount")  # mAh
    soc = get("can/bus0/PACK_SOC/PackSOCCoulomb")

    if energy_now is not None:
        if energy_start is None:
            energy_start = energy_now
        energy_used_wh = (energy_start - energy_now) / 1000.0
        distance_km = distance_m / 1000.0

        if distance_km > 0.01:
            wh_per_km = energy_used_wh / distance_km
            pub(client, "energy_eff", "Wh/km", wh_per_km)

            if soc is not None and wh_per_km > 0.1:
                remaining_wh = TOTAL_PACK_WH * soc / 100.0
                remaining_km = remaining_wh / wh_per_km
                pub(client, "est_range", "Est. Range (km)", remaining_km)
                pub(client, "est_laps", "Est. Laps Left", remaining_km / LAP_DISTANCE_KM)

                # Remaining time estimate based on average speed
                if distance_m > 50 and energy_used_wh > 0.1:
                    elapsed = now - (last_time - distance_m / max(last_speed, 0.1)) if last_speed > 0.1 else 0
                    avg_speed_kmh = distance_km / max((now - (energy_start / energy_now * 0.001)), 0.01) * 3600
                    if speed and speed > 0.5:
                        avg_speed = distance_m / max(now - last_time + distance_m / speed, 1)
                        remaining_time_min = remaining_km / (speed * 3.6) * 60
                        pub(client, "est_time_min", "Est. Time Left (min)", remaining_time_min)

    # ── Coulomb Counting (independent SOC) ──
    if coulomb_now is not None:
        if coulomb_start is None:
            coulomb_start = coulomb_now
        used_mah = coulomb_start - coulomb_now
        independent_soc = max(0, (1.0 - used_mah / (TOTAL_PACK_AH * 1000)) * 100)
        pub(client, "coulomb_soc", "Coulomb SOC (%)", independent_soc)

    # ── G-Forces (normalized) ──
    acc_y = get("can/bus0/GPS_ACCELERATION/accY")
    acc_x = get("can/bus0/GPS_ACCELERATION/accX")
    if acc_y is not None:
        pub(client, "lat_g", "Lateral G", acc_y / 9.81)
    if acc_x is not None:
        pub(client, "long_g", "Longitudinal G", acc_x / 9.81)

    # ── Understeer Gradient ──
    steer = get("can/bus0/SAS_DATA/LWS_ANGLE")
    yaw = get("can/bus0/GPS_RATE_OF_TURN/gyrZ")
    if steer is not None and yaw is not None and speed is not None and speed > 2.0:
        road_rad = math.radians(steer / STEERING_RATIO)
        theo_yaw = math.degrees(speed * math.tan(road_rad) / WHEELBASE_M)
        pub(client, "understeer", "Understeer (°/s)", theo_yaw - yaw)

    # ── Regen Efficiency (during braking) ──
    if fb is not None and fb > 50 and dc_i is not None and dc_v is not None:
        if dc_i < -1.0:  # negative current = regen
            regen_kw = abs(dc_v * dc_i) / 1000.0
            pub(client, "regen_power", "Regen Power (kW)", regen_kw)

    # ── Average Power (10s rolling) ──
    if len(power_history) >= 10:
        pub(client, "avg_power", "Avg Power (kW)", sum(power_history) / len(power_history))


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
            latest[msg.topic] = float(payload)
    except Exception:
        pass

def on_connect(client, userdata, flags, rc):
    print(f"Connected to {BROKER}:{PORT}")
    client.subscribe("can/#")
    print("Subscribed — computing math channels at 10Hz...")

client = mqtt.Client()
client.on_connect = on_connect
client.on_message = on_message
client.connect(BROKER, PORT)
client.loop_start()

print(f"UTFR Math Channels v2 — connecting to {BROKER}:{PORT}")
try:
    while True:
        compute(client)
        time.sleep(0.1)
except KeyboardInterrupt:
    print("\nStopped.")
    client.loop_stop()
