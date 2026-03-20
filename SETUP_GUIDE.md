# UTFR Wireless Telemetry — Quick Setup Guide

## What You Need
- A computer with **Docker** installed (Raspberry Pi, Mac, Linux, or Windows with Docker Desktop)
- **Python 3** with pip
- A web browser

---

## Step 1: Install Docker

**Raspberry Pi / Linux:**
```
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER
```
Log out and log back in, then verify: `docker --version`

**Mac:** Download Docker Desktop from https://docker.com

**Windows:** Download Docker Desktop from https://docker.com (enable WSL2 when prompted)

---

## Step 2: Unzip the Project

Unzip `utfr-telemetry-v10.zip` wherever you want. Open a terminal and navigate into it:
```
cd utfr-wireless-telemetry-main/base_station
```

---

## Step 3: Start the Services

```
docker compose up -d
```

Wait **2 minutes** for Grafana to download the MQTT plugin (first time only), then:
```
docker compose restart grafana
```

Wait 30 seconds. Verify it's working:
```
docker compose logs grafana 2>&1 | tail -3
```
You should see lines with `dsUid=mqtt` and no errors.

---

## Step 4: Install Python Dependency

```
pip install paho-mqtt
```
If you get an "externally managed" error on Linux, add `--break-system-packages`:
```
pip install paho-mqtt --break-system-packages
```

---

## Step 5: Start the Simulator and Analysis Scripts

You need **3 separate terminal windows**, all in the `base_station/can_bridge` folder.

**Terminal 1 — Fake CAN data simulator:**
```
cd base_station/can_bridge
sed 's/BROKER = "mosquitto"/BROKER = "localhost"/' bridge.py | python3
```
You should see: `Publishing simulated UTFR CAN data (realistic driver)...`

**Terminal 2 — Math channels (derived metrics):**
```
cd base_station/can_bridge
python3 math_channels.py
```
You should see: `Connected to localhost:1883`

**Terminal 3 — Driver performance analysis:**
```
cd base_station/can_bridge
python3 driver_analysis.py
```
You should see: `Analyzing driver performance at 10Hz...`

---

## Step 6: Open the Dashboards

Open a browser and go to:

| What | URL |
|------|-----|
| **Grafana Dashboards** | http://localhost:3000 |
| **Live Track Map** | Open a 4th terminal, then: |

For the track map:
```
cd base_station/can_bridge/static
python3 -m http.server 8888
```
Then open: **http://localhost:8888/track.html**

---

## What You'll See

### Grafana (port 3000)
Click the hamburger menu (☰) → **Dashboards** → **UTFR Telemetry** folder.

There are **7 dashboards:**
1. **Race Engineer Overview** — The main at-a-glance dashboard with system states, driver inputs, torque, wheel speeds, temperatures, and energy strategy
2. **Battery & Accumulator** — Cell temps, voltages, SOC, pack power, and cell health diagnostics
3. **Powertrain & Inverter** — Motor speed, torque delivery, inverter temps, phase currents
4. **Suspension & Brakes** — Wheel speeds, slip ratios, brake bias, steering, understeer gradient
5. **Vehicle Dynamics & GPS** — Speed, G-forces, orientation, traction control debug
6. **LV System & Safety** — 12V battery and insulation monitoring
7. **Driver Performance** — Driver technique scoring (throttle smoothness, steering, pedal discipline, brake consistency) with live engineer recommendations

Every panel has an **ℹ️ icon** — hover over it to read what the signal means.

### Track Map (port 8888)
Shows the Michigan Endurance 2023 track with a red dot representing the car moving around the circuit. The telemetry bar at the bottom shows live speed, RPM, throttle, brake, steering, SOC, cell temp, and lap count.

---

## Accessing from Another Device on the Same Network

If running on a Raspberry Pi (or any other machine), replace `localhost` with the machine's IP address. For example if the Pi's IP is `192.168.2.71`:
- Grafana: `http://192.168.2.71:3000`
- Track Map: `http://192.168.2.71:8888/track.html`

To find the IP: `hostname -I`

---

## Stopping Everything

1. Ctrl+C in each of the 3 Python terminal windows
2. Stop the track map server with Ctrl+C
3. Stop Docker services:
```
cd base_station
docker compose down
```

---

## Troubleshooting

**Grafana shows "No data":**
- Make sure bridge.py is running in Terminal 1
- Make sure the MQTT datasource has `uid: mqtt` in `grafana/provisioning/datasources/mqtt.yaml`
- Try: `docker compose down -v` then start again from Step 3

**"data source not found" error in Grafana logs:**
- The MQTT plugin hasn't finished installing. Wait 2 minutes and run `docker compose restart grafana`

**Port already in use:**
- Another service is using port 3000 or 1883. Stop it first, or change ports in `docker-compose.yml`

**Track map shows "Disconnected":**
- Make sure mosquitto is running: `docker compose ps`
- Port 9001 (WebSocket) must be accessible
