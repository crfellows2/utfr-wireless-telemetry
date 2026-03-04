# UTFR Wireless Telemetry — Data System Documentation

## Overview

This document describes the architecture, design decisions, and usage of the
UTFR wireless telemetry data system. The system transports CAN bus data from
the car to a ground-side base station, decodes it, and distributes it over a
local WiFi network so team members can monitor signals and compute metrics from
their own laptops in real time.

---

## Architecture

```
Car ESP32 (2x CAN) ──BLE──► Receiver ESP32 ──USB──► Pi 4B Base Station
                                                           │
                                              ┌────────────┼────────────┐
                                         Mosquitto     Axum server    Grafana
                                         :1883/:9001    :8080          :3000
                                              │              │              │
                                    ──────────┴──────────────┴──────────────┴──────
                                                   LAN WiFi
                                         ┌──────────────────────────┐
                                         │  Any device on the LAN   │
                                         │  - MQTT scripts/tools    │
                                         │  - Config UI (browser)   │
                                         │  - Grafana (browser)     │
                                         └──────────────────────────┘
```

### Services (Docker Compose)

| Service | Description |
|---|---|
| `mosquitto` | MQTT broker, TCP port 1883, WebSocket port 9001 |
| `mdns` | Advertises `telemetry.local` via mDNS so consumers don't need to know the Pi's IP |
| `can_bridge` | Receives decoded signals from USB, resolves topic paths via DBC, publishes to MQTT. Serves config web UI on port 8080 |
| `grafana` | Live signal dashboard, port 3000 |

---

## Wireless Link

The car ESP32 has a copy of the DBC and decodes CAN frames to individual signal
values onboard. Decoded signals are transmitted over BLE to a receiver ESP32 at
the base station. BLE is the only available wireless option. Practical sustained
throughput is around 30 KBps.

The BLE stream carries `(signal_id, timestamp, value)` tuples batched
contiguously up to the BLE MTU (506 bytes). Transmitting at the signal level
rather than the message level allows the scheduling algorithm to operate cleanly
per signal with no side effects from signals sharing a CAN message.

The base station bridge receives decoded signal values and uses the DBC to
resolve each signal ID to a topic path (`can/<bus>/<message>/<signal>`) before
publishing to MQTT. The DBC must be kept in sync between the car ESP and the
base station — when the DBC changes, update it in both places.

---

## Signal Filtering and Prioritization

Because BLE throughput is limited, the car ESP32 filters and decimates signals
before transmitting. Filtering is defined by **profiles** configured on the
base station and pushed to the car over a BLE RPC channel.

### Profile Format

Profiles are defined in a TOML config file uploaded via the base station web
UI. Each profile contains ordered **buckets** of signals. Bucket order
determines priority — bucket 0 is highest priority.

```toml
[profiles.acceleration]
description = "FSAE Acceleration Event"

[profiles.acceleration.buckets.0]
signals = [
    { name = "engine_rpm", frequency_hz = 10.0, min_frequency_hz = 10.0 },
    { name = "gps_speed",  frequency_hz = 10.0, min_frequency_hz = 1.0  },
]

[profiles.acceleration.buckets.1]
signals = [
    { name = "fl_wheel_speed", frequency_hz = 100.0, min_frequency_hz = 10.0 },
    { name = "steering_angle", frequency_hz = 10.0,  min_frequency_hz = 5.0  },
]
```

Signals can be referenced by name (resolved to CAN ID via the DBC on the base
station before being sent to the ESP) or directly by CAN ID:

```toml
{ id = 0x1A3, frequency_hz = 100.0, min_frequency_hz = 10.0 }
```

### Scheduling Algorithm

Given a current maximum link throughput, the ESP computes a target frequency
for each signal using a bucket interpolation algorithm:

1. Each bucket has a scalar `s ∈ [0.0, 1.0]` where 0 = all signals at
   `min_frequency_hz` and 1 = all signals at `frequency_hz`
2. Starting from the highest priority bucket, increase `s` until either it
   reaches 1.0 or the bandwidth budget is exhausted
3. Move to the next bucket and repeat
4. Any bucket whose scalar remains 0.0 is dropped entirely

The interpolated frequency for each signal:

```
current_hz = min_frequency_hz + s * (frequency_hz - min_frequency_hz)
```

This gives smooth graceful degradation as link quality changes — signals are
reduced proportionally within a bucket before lower priority buckets are
touched, and signals are never dropped until all options for reduction are
exhausted.

A **token bucket** sampler per signal is used to match incoming CAN frames to
the target rate without per-signal timers.

Setting `min_frequency_hz == frequency_hz` gives fixed-rate behavior (all or
nothing). Setting `min_frequency_hz = 0` allows a signal to be reduced all the
way to zero before being dropped.

---

## MQTT Topic Structure

All decoded signals are published to:

```
can/<bus>/<message>/<signal>
```

Examples:

```
can/bus0/EngineData/engine_rpm
can/bus0/WheelSpeeds/fl_wheel_speed
can/bus1/BatteryData/cell_temp_max
```

CAN frames that cannot be decoded (not in the loaded DBC) are published to:

```
can/<bus>/unknown/<hex_id>
```

Computed metrics published by consumer scripts should use:

```
metrics/<author>/<metric_name>
```

### MQTT Wildcards

| Subscription | Receives |
|---|---|
| `can/#` | All signals, all buses |
| `can/bus0/#` | All signals on bus 0 |
| `can/bus0/EngineData/#` | All signals in one CAN message |
| `can/+/+/engine_rpm` | One signal across all buses and messages |
| `metrics/#` | All computed metrics |

---

## Payload Format

Every topic carries a JSON payload:

```json
{"ts": 1771615974.328, "value": 3200.1}
```

- `ts` — Unix timestamp in seconds (float), sourced from the car-side CAN bus
  clock. All sensors on the car sync to this clock, so timestamps are coherent
  across all signals with no additional synchronization required.
- `value` — decoded signal value (JSON number). DBC decoding always produces a
  numeric value via `decoded = raw * scale + offset`.

JSON is used for broad compatibility — Grafana, Python, MATLAB, JavaScript, and
C/C++ clients can all consume it without custom parsing.

Unknown frame payloads include the raw bytes as an array of integers:

```json
{"ts": 1771615974.328, "id": 419, "data": [26, 59, 12, 0, 0, 0, 0, 0]}
```

---

## Connecting to the Broker

| Protocol | Address | Use case |
|---|---|---|
| MQTT TCP | `telemetry.local:1883` | Python, MATLAB, C/C++ scripts |
| MQTT WebSocket | `ws://telemetry.local:9001` | Browser-based tools |

`telemetry.local` resolves via mDNS — no IP address needed. Any device on the
LAN with mDNS support (macOS, Linux with avahi, Windows 10+) can use it
directly.

---

## Writing Metric Scripts

Scripts subscribe to one or more signal topics, compute a metric, and publish
the result back to the broker. They can run anywhere on the LAN — on the Pi,
on a teammate's laptop, or on a more powerful machine if compute is needed.
There is no registration or configuration required to publish metrics.

See `metrics_demo.py` for a minimal Python example. The same approach works in
any language with an MQTT client library.

### Deployment options

| Option | Broker address | Notes |
|---|---|---|
| Laptop on LAN | `telemetry.local` | Recommended, no setup needed |
| On the Pi | `localhost` | Fine for lightweight scripts |
| In the compose | `mosquitto` | For scripts that should always be running |

---

## Base Station Web UI

The Axum web server at `http://telemetry.local:8080` provides:

- `GET /config` — returns current filter profile config as TOML (or an example if none uploaded)
- `POST /config` — upload and apply a new filter profile config
- `GET /dbc` — returns the current DBC file
- `POST /dbc` — upload a new DBC file

The config is validated on upload and an error is returned if parsing fails.
Valid configs are persisted to disk and survive restarts.

---

## Grafana Dashboard

Grafana is available at `http://telemetry.local:3000`. The MQTT datasource is
pre-configured and connects to the broker automatically. Anonymous access is
enabled — no login required.

To monitor a signal, go to **Explore**, select the MQTT datasource, and
subscribe to a topic such as `can/bus0/EngineData/engine_rpm`.
