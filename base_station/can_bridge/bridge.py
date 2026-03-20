# Simulated UTFR CAN signals with REALISTIC driver imperfections.
#
# The simulator models a driver who:
# - Has jerky throttle application on corner exits
# - Occasionally overlaps throttle and brake (trail braking gone wrong)
# - Makes steering corrections mid-corner
# - Coasts between braking and throttle zones
# - Has inconsistent brake pressure between events
# - Occasionally spins up the rear wheels
#
# Topic: can/<bus>/<message>/<signal>
# Payload: JSON with descriptive key → {"Motor RPM": 3200}

import paho.mqtt.client as mqtt
import json, time, math, random

BROKER = "mosquitto"
PORT = 1883

# Michigan FSAE Endurance Track 2023 — 59 waypoints traced from official map
# Positioned on MIS back straight paved area
TRACK = [
    (42.063737, -84.237912), (42.063696, -84.238172), (42.063682, -84.238544),
    (42.063710, -84.238916), (42.063696, -84.239288), (42.063737, -84.239586),
    (42.063641, -84.239772), (42.063751, -84.239958), (42.063627, -84.240181),
    (42.063751, -84.240404), (42.063641, -84.240627), (42.063764, -84.240888),
    (42.063655, -84.241074), (42.063751, -84.241297), (42.063696, -84.241483),
    (42.063751, -84.241743), (42.063819, -84.242004), (42.063901, -84.242264),
    (42.064011, -84.242562), (42.064134, -84.242822), (42.064230, -84.243045),
    (42.064326, -84.243268), (42.064395, -84.243492), (42.064449, -84.243678),
    (42.064518, -84.243826), (42.064586, -84.243938), (42.064655, -84.244050),
    (42.064696, -84.244124), (42.064737, -84.244161), (42.064778, -84.244124),
    (42.064806, -84.244050), (42.064819, -84.243938), (42.064806, -84.243826),
    (42.064792, -84.243640), (42.064778, -84.243380), (42.064765, -84.243008),
    (42.064778, -84.242636), (42.064806, -84.242264), (42.064819, -84.241892),
    (42.064806, -84.241520), (42.064778, -84.241148), (42.064765, -84.240776),
    (42.064778, -84.240404), (42.064806, -84.240032), (42.064819, -84.239660),
    (42.064806, -84.239288), (42.064778, -84.238916), (42.064751, -84.238618),
    (42.064696, -84.238395), (42.064628, -84.238209), (42.064545, -84.238060),
    (42.064449, -84.237967), (42.064354, -84.237912), (42.064258, -84.237893),
    (42.064148, -84.237912), (42.064038, -84.237949), (42.063929, -84.237967),
    (42.063819, -84.237949), (42.063737, -84.237912),
]

# Precompute segment distances
def gps_dist(a, b):
    dlat = (b[0] - a[0]) * 111000
    dlon = (b[1] - a[1]) * 82000
    return math.sqrt(dlat**2 + dlon**2)

SEG_DISTS = [gps_dist(TRACK[i], TRACK[(i+1) % len(TRACK)]) for i in range(len(TRACK))]
TRACK_LEN = sum(SEG_DISTS)

# Car position state
car_dist = 0.0  # distance along track in meters

def get_car_gps(speed_mps, dt):
    """Advance car along track and return (lat, lon)."""
    global car_dist
    car_dist = (car_dist + speed_mps * dt) % TRACK_LEN

    # Find which segment we're on
    d = car_dist
    for i, seg_d in enumerate(SEG_DISTS):
        if d < seg_d:
            frac = d / seg_d
            j = (i + 1) % len(TRACK)
            lat = TRACK[i][0] + frac * (TRACK[j][0] - TRACK[i][0])
            lon = TRACK[i][1] + frac * (TRACK[j][1] - TRACK[i][1])
            return (round(lat, 7), round(lon, 7))
        d -= seg_d
    return TRACK[0]

client = mqtt.Client()
client.connect(BROKER, PORT)
client.loop_start()
print("Publishing simulated UTFR CAN data (realistic driver)...")
print(f"Track: {len(TRACK)} waypoints, {TRACK_LEN:.0f}m lap")

t0 = time.time()

def pub(topic, name, value):
    client.publish(topic, json.dumps({name: value}))

# Driver personality parameters (imperfect driver)
THROTTLE_JERK = 0.0       # accumulates random jitter
STEER_CORRECTION = 0.0
brake_peak_bias = 1.0     # varies per braking event to simulate inconsistency
last_braking = False

while True:
    t = time.time()
    elapsed = t - t0

    # ── Drive cycle: 30s accel, 5s coast, 15s brake, 10s coast ──
    cycle_pos = elapsed % 60
    if cycle_pos < 30:
        # Acceleration phase — throttle ramps up with jitter
        phase = "accel"
        base_throttle = math.sin(cycle_pos / 30 * math.pi) * 200
        # Add realistic jerk — sudden stabs and lifts
        THROTTLE_JERK += random.gauss(0, 8)
        THROTTLE_JERK *= 0.85  # decay
        throttle = max(0, base_throttle + THROTTLE_JERK + random.gauss(0, 5))
        brake_f = 0
        brake_r = 0
        # Occasional throttle-brake overlap (bad habit — ~10% of time during accel)
        if random.random() < 0.08:
            brake_f = random.uniform(20, 80)
            brake_r = random.uniform(15, 60)
    elif cycle_pos < 35:
        # Coast phase — driver lifts off everything briefly
        phase = "coast"
        throttle = random.uniform(0, 3)  # tiny residual
        brake_f = random.uniform(0, 5)
        brake_r = random.uniform(0, 3)
    elif cycle_pos < 50:
        # Braking phase — inconsistent peak pressure each time
        phase = "brake"
        if not last_braking:
            # New braking event — randomize peak pressure (inconsistent driver)
            brake_peak_bias = random.uniform(0.7, 1.3)
            last_braking = True
        brake_progress = (cycle_pos - 35) / 15
        brake_base = math.sin(brake_progress * math.pi)
        brake_f = max(0, brake_base * 1200 * brake_peak_bias + random.gauss(0, 30))
        brake_r = max(0, brake_base * 900 * brake_peak_bias * random.uniform(0.9, 1.1) + random.gauss(0, 20))
        throttle = 0
        # Trail braking gone wrong — occasional throttle overlap during braking
        if brake_progress > 0.6 and random.random() < 0.15:
            throttle = random.uniform(10, 50)
    else:
        # Second coast phase
        phase = "coast"
        last_braking = False
        throttle = random.uniform(0, 5)
        brake_f = random.uniform(0, 8)
        brake_r = random.uniform(0, 5)

    motor_rpm = max(0, throttle * 25 + random.gauss(0, 15))

    # ── Steering with corrections ──
    base_steer = math.sin(elapsed * 0.4) * 55
    # Add mid-corner corrections (sudden direction changes)
    if random.random() < 0.05:
        STEER_CORRECTION = random.gauss(0, 15)
    STEER_CORRECTION *= 0.9
    steer = base_steer + STEER_CORRECTION + random.gauss(0, 2)

    # ── Wheel speeds with slip events ──
    front_base = max(0, motor_rpm)
    # Occasional wheelspin (rear faster than front by 15-25%)
    if phase == "accel" and throttle > 100 and random.random() < 0.12:
        rear_slip_mult = random.uniform(1.12, 1.25)
    else:
        rear_slip_mult = random.uniform(1.0, 1.04)

    fl_ws = max(0, front_base * 1.01 + random.gauss(0, 8))
    fr_ws = max(0, front_base * 0.99 + random.gauss(0, 8))
    rl_ws = max(0, front_base * rear_slip_mult + random.gauss(0, 8))
    rr_ws = max(0, front_base * rear_slip_mult * random.uniform(0.97, 1.03) + random.gauss(0, 8))

    # ── Accelerations with noise ──
    lat_g_raw = math.sin(elapsed * 0.4) * 12 + random.gauss(0, 1.5)
    long_g_raw = 0
    if phase == "accel":
        long_g_raw = throttle * 0.04 + random.gauss(0, 1)
    elif phase == "brake":
        long_g_raw = -brake_f * 0.008 + random.gauss(0, 0.8)

    # ── Publish everything ──
    # Controller states
    pub("can/bus0/RC_STATE/RC_STATE",        "RC State", 3)
    pub("can/bus0/RC_STATE/RC_ERROR",        "RC Error", 0)
    pub("can/bus0/FC_STATE/FC_STATE",        "FC State", 3)
    pub("can/bus0/FC_STATE/FC_ERROR",        "FC Error", 0)
    pub("can/bus0/ACM/ACM_STATE",            "ACM State", 3)
    pub("can/bus0/ACM/ACM_ERROR",            "ACM Error", 0)
    pub("can/bus0/INTSTATES/InvState",       "INV State", 3)
    pub("can/bus0/INTSTATES/InvEnableState", "INV Enable", 1)
    pub("can/bus0/APPS/PedalError",          "Pedal Error", 0)

    # Driver inputs
    pub("can/bus0/APPS/ProcessedThrottle",             "Throttle (Nm)", round(throttle, 1))
    pub("can/bus0/BRAKE_PRESSURE/FrontBrakePressure",  "Front Brake",   round(brake_f, 0))
    pub("can/bus0/BRAKE_PRESSURE/RearBrakePressure",   "Rear Brake",    round(brake_r, 0))
    pub("can/bus0/SAS_DATA/LWS_ANGLE",                "Steering (°)",  round(steer, 1))
    pub("can/bus0/SAS_DATA/LWS_SPEED",                "Steer Rate",    round((steer - base_steer) * 10, 1))

    # Powertrain
    pub("can/bus0/HIGHSPEED/MotorSpeed",       "Motor RPM",      round(motor_rpm, 0))
    pub("can/bus0/HIGHSPEED/TorqueCommanded",  "Commanded (Nm)", round(throttle * 0.8, 1))
    pub("can/bus0/HIGHSPEED/TorqueFeedback",   "Actual (Nm)",    round(throttle * 0.75 + random.gauss(0, 2), 1))
    pub("can/bus0/HIGHSPEED/DcBusVoltage",     "DC Bus (V)",     round(350 + random.gauss(0, 3), 1))
    pub("can/bus0/CURRENTINFO/DcBusCurrent",   "DC Bus (A)",     round(throttle * 0.8, 1))
    pub("can/bus0/CURRENTINFO/PhaseA_Current", "Phase A",        round(throttle * 0.4 * math.sin(elapsed * 50), 1))
    pub("can/bus0/CURRENTINFO/PhaseB_Current", "Phase B",        round(throttle * 0.4 * math.sin(elapsed * 50 + 2.094), 1))
    pub("can/bus0/CURRENTINFO/PhaseC_Current", "Phase C",        round(throttle * 0.4 * math.sin(elapsed * 50 + 4.189), 1))

    # Wheel speeds
    pub("can/bus0/RC_STATE/RR_WHEELSPEED",          "RR", round(rr_ws, 0))
    pub("can/bus0/RC_STATE/RL_WHEELSPEED",          "RL", round(rl_ws, 0))
    pub("can/bus1/FRONT_WHEELSPEEDS/FL_WHEELSPEED", "FL", round(fl_ws, 0))
    pub("can/bus1/FRONT_WHEELSPEEDS/FR_WHEELSPEED", "FR", round(fr_ws, 0))

    # Battery
    pub("can/bus0/BMS_TEMPERATURES/AccuCellHighTemp",  "Cell High",   round(32 + elapsed * 0.008 + random.gauss(0, 0.3), 1))
    pub("can/bus0/BMS_TEMPERATURES/AccuCellAvgTemp",   "Cell Avg",    round(30 + elapsed * 0.006 + random.gauss(0, 0.2), 1))
    pub("can/bus0/BMS_TEMPERATURES/AccuCellLowTemp",   "Cell Low",    round(28 + elapsed * 0.004 + random.gauss(0, 0.2), 1))
    pub("can/bus0/BMS_VOLTAGES/AccuCellHighVolt",      "Cell High V", round(3.65 + random.gauss(0, 0.01), 3))
    pub("can/bus0/BMS_VOLTAGES/AccuCellAvgVolt",       "Cell Avg V",  round(3.60 + random.gauss(0, 0.005), 3))
    pub("can/bus0/BMS_VOLTAGES/AccuCellLowVolt",       "Cell Low V",  round(3.55 + random.gauss(0, 0.01), 3))
    pub("can/bus0/PACK_SOC/PackSOCCoulomb",            "SOC (%)",     round(max(5, 85 - elapsed * 0.03), 1))
    pub("can/bus0/PACK_SOC/PackSOCVoltage",            "SOC V (%)",   round(max(5, 83 - elapsed * 0.025), 1))
    pub("can/bus0/PACK_SOC/PackPower",                 "Power (W)",   round(throttle * 1.5, 1))
    pub("can/bus0/PACK_CURRENT/PackCurrent",           "Current (A)", round(throttle * 0.8, 1))
    pub("can/bus0/PACK_CURRENT/BMSVoltageSummed",      "Pack V",      round(350 - throttle * 0.05 + random.gauss(0, 2), 1))
    pub("can/bus0/PACK_ENERGY/EnergyCount",            "Energy",      round(5000000 - elapsed * 80))
    pub("can/bus0/PACK_ENERGY/CoulombCount",           "Coulombs",    round(15000 - elapsed * 3))
    pub("can/bus0/ACM/ACM_Pack_Voltage",               "ACM Pack V",  round(350 + random.gauss(0, 2), 1))

    # Inverter temps
    tp = max(0, throttle / 200)
    pub("can/bus0/INVTEMPS1/ModuleATemp",    "IGBT A",        round(45 + tp * 15 + random.gauss(0, 0.5), 1))
    pub("can/bus0/INVTEMPS1/ModuleBTemp",    "IGBT B",        round(44 + tp * 15 + random.gauss(0, 0.5), 1))
    pub("can/bus0/INVTEMPS1/ModuleCTemp",    "IGBT C",        round(46 + tp * 15 + random.gauss(0, 0.5), 1))
    pub("can/bus0/INVTEMPS1/GateDriverTemp", "Gate Driver",   round(40 + tp * 8 + random.gauss(0, 0.3), 1))
    pub("can/bus0/INVTEMPS2/ControlTemp",    "Control Board", round(38 + random.gauss(0, 0.3), 1))
    pub("can/bus0/INVTEMPS3/CoolantTemp",    "Coolant",       round(35 + tp * 5, 1))
    pub("can/bus0/INVTEMPS3/HotSpotTemp",    "Inv Hot Spot",  round(50 + tp * 20, 1))
    pub("can/bus0/INVTEMPS3/MotorTemp",      "Motor",         round(55 + tp * 25, 1))

    # LV
    pub("can/bus0/LVBATT/LVBattVoltage",        "LV Batt (V)", round(12.6 - elapsed * 0.0005 + random.gauss(0, 0.03), 2))
    pub("can/bus0/LVBATT/LVHighTemperature",    "High",         round(28 + random.gauss(0, 0.3), 1))
    pub("can/bus0/LVBATT/LVAverageTemperature", "Average",      round(26 + random.gauss(0, 0.2), 1))
    pub("can/bus0/LVBATT/LVLowestTemperature",  "Low",          round(24 + random.gauss(0, 0.2), 1))
    pub("can/bus0/IMD_STATUS/IMD_DUTY_CYCLE",   "IMD Duty (%)", 50)

    # GPS / IMU
    pub("can/bus0/GPS_VELOCITY/velX",      "Longitudinal (m/s)", round(max(0, motor_rpm / 40) / 3.6, 2))
    pub("can/bus0/GPS_VELOCITY/velY",      "Lateral (m/s)",      round(random.gauss(0, 0.3), 2))
    pub("can/bus0/GPS_ACCELERATION/accX",  "Longitudinal G",     round(long_g_raw, 2))
    pub("can/bus0/GPS_ACCELERATION/accY",  "Lateral G",          round(lat_g_raw, 2))
    pub("can/bus0/GPS_RPY/Roll",           "Roll",  round(math.sin(elapsed * 0.3) * 3 + random.gauss(0, 0.5), 1))
    pub("can/bus0/GPS_RPY/Pitch",          "Pitch", round(random.gauss(0, 1), 1))
    pub("can/bus0/GPS_RPY/Yaw",            "Yaw",   round(math.sin(elapsed * 0.1) * 180, 1))
    pub("can/bus0/GPS_RATE_OF_TURN/gyrX",  "Roll Rate",  round(random.gauss(0, 5), 1))
    pub("can/bus0/GPS_RATE_OF_TURN/gyrY",  "Pitch Rate", round(random.gauss(0, 3), 1))
    pub("can/bus0/GPS_RATE_OF_TURN/gyrZ",  "Yaw Rate",   round(math.cos(elapsed * 0.4) * 25 + random.gauss(0, 3), 1))

    # TC Debug
    pub("can/bus0/TC_DEBUG_1/TC_Torque",     "TC Torque",  round(throttle * 0.7 + random.gauss(0, 5), 0))
    pub("can/bus0/TC_DEBUG_1/TC_slip_ratio", "Slip Ratio", round(random.gauss(0.05, 0.03), 3))
    pub("can/bus0/TC_DEBUG_2/TC_fz_fl",      "Fz FL", round(800 + random.gauss(0, 40), 0))
    pub("can/bus0/TC_DEBUG_2/TC_fz_fr",      "Fz FR", round(800 + random.gauss(0, 40), 0))
    pub("can/bus0/TC_DEBUG_2/TC_fz_rl",      "Fz RL", round(700 + random.gauss(0, 40), 0))
    pub("can/bus0/TC_DEBUG_2/TC_fz_rr",      "Fz RR", round(700 + random.gauss(0, 40), 0))

    # GPS Car Position — advance along track based on current speed
    speed_mps = max(0, motor_rpm / 40) / 3.6
    lat, lon = get_car_gps(speed_mps, 0.1)
    pub("can/bus0/GPS_LATLONG/Lat", "Latitude", lat)
    pub("can/bus0/GPS_LATLONG/Long", "Longitude", lon)
    # Also publish as combined payload for Geomap panel
    client.publish("metrics/car_position", json.dumps({"lat": lat, "lon": lon}))

    time.sleep(0.1)
