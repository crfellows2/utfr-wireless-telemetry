# UTFR Wireless Telemetry - TODO

## Base Station (Rust / can_bridge)

- [ ] DBC file upload endpoint (`POST /dbc`)
- [ ] DBC file storage in `CONFIG_DIR`
- [ ] USB serial reader for raw CAN frames from receiver ESP
- [ ] CAN frame parser (binary protocol from ESP)
- [ ] DBC decoding of parsed frames using uploaded DBC
- [ ] Publish decoded signals to MQTT (`can/bus0/<signal_name>`)
- [ ] Publish unknown frames to MQTT (`can/unknown/0x<id>`)
- [ ] Active profile selection endpoint (`POST /profile/{name}`)
- [ ] Push active profile to receiver ESP over RPC channel
- [ ] Mosquitto WebSocket listener (port 9001) for browser dashboard

## Car-Side ESP32

- [ ] CAN frame reception on both buses
- [ ] Hardware CAN controller filter configuration via RPC
- [ ] Clock sync from CAN bus timestamp signal
- [ ] Scheduling algorithm
  - [ ] Bucket interpolation (fill highest priority first)
  - [ ] Token bucket sampler per signal
  - [ ] React to link quality changes (update max_throughput)
- [ ] BLE stream channel: transmit filtered/decimated CAN frames
- [ ] BLE RPC channel: receive filter profile updates from base station
- [ ] Profile storage on ESP (survive reboots)

## Receiver ESP32

- [ ] BLE connection to car ESP32
- [ ] Demultiplex BLE stream and RPC channels
- [ ] Forward raw CAN frames over USB serial to Pi
- [ ] Forward RPC messages from Pi to car ESP32

## Infrastructure

- [ ] Mosquitto WebSocket config (port 9001) for Grafana / browser clients
- [ ] Grafana dashboard provisioning (saved dashboards in compose)
- [ ] Svelte dashboard for live signal monitoring

## Nice to Have

- [ ] Web editor for filter profiles (replace plain HTML textarea)
- [ ] Live signal list in web UI (show what is currently publishing)
- [ ] Connection status indicator in web UI (BLE link quality)
- [ ] `notify` watcher for config hot-reload over SSH
- [ ] Write CAN frames (reverse path, low priority)
