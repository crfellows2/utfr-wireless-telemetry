# Scripts — User Customization Layer

Example scripts demonstrating how to build custom analysis on top of the core telemetry system. These are starting points for the team to modify and extend.

## Python Scripts

**can_simulator.py** - Generates realistic fake CAN data for testing without the car

**math_channels.py** - Computes derived metrics (slip ratios, power, energy, etc.). Required for full Grafana dashboard functionality.

**driver_analysis.py** - Real-time driver performance scoring. Required for Driver Performance dashboard.

**metrics_demo.py** - Minimal example showing how to subscribe to CAN signals and publish custom metrics

All scripts use `MQTT_BROKER` environment variable (defaults to `telemetry.local`).

## Web Tools

**serve_web.py** - Serves HTML visualization tools from `web/` directory

**web/track.html** - Live track map showing car position and telemetry

Run `python3 serve_web.py`, then open http://localhost:8888

Add your own HTML tools to `web/` - they'll appear in the directory listing automatically.
