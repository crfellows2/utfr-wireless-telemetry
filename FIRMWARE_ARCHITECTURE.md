# CAN Telemetry System Architecture

This diagram shows the software architecture of the CAN Relay and CAN Receiver systems. Use the exact names shown here to navigate the codebase.

```mermaid
---
  config:
    layout: elk
    elk:
      nodePlacementStrategy: SIMPLE
      direction: DOWN
    theme: base
---
flowchart TB
    subgraph relay_async["LocalExecutor (Async)"]
        can_task["can_task()<br>(can_interface/mod.rs)"]
        ble_task["ble_task()<br>(ble/mod.rs)"]
    end
    subgraph relay_threads["RTOS Threads"]
        sd_logger["sd_logger thread<br>(sd_logger.rs)"]
        pose["pose thread<br>(pose_estimation.rs)"]
    end
    subgraph relay["CAN Relay (can_relay/src/)"]
        relay_async
        relay_threads
        can_bus[("CAN Bus<br>(transceivers)")]
        sd_card[("SD Card")]
        rtc[("RTC<br>(DS3231)")]
    end
    subgraph receiver_async["LocalExecutor (Async)"]
        main_loop["main_loop()<br>(main.rs)"]
    end
    subgraph receiver_threads["RTOS Thread"]
        stdin_reader["stdin_reader thread<br>(stdin.rs)"]
    end
    subgraph receiver["CAN Receiver (can_receiver/src/)"]
        receiver_async
        receiver_threads
        usb[("USB Serial<br>→ Host PC")]
    end
    can_task -- "LOG_CHANNEL<br>CanFrameForSd" --> sd_logger
    sd_logger --> sd_card
    sd_logger -. "SD_STATUS<br>Signal" .-> ble_task
    rtc -.-> sd_logger
    can_task -. "BLE_TX_ONESHOT<br>Signal" .-> ble_task
    can_task -. "ACCEL_SIGNAL<br>Signal" .-> pose
    can_task -. "GYRO_SIGNAL<br>Signal" .-> pose
    ble_task -- "CAN_WRITE_CHANNEL<br>protocol::CanFrame" --> can_task
    can_task <--> can_bus
    usb --> stdin_reader
    stdin_reader -- "STDIN_COMMAND_CHANNEL<br>Command" --> main_loop
    main_loop -- JSON telemetry --> usb
    ble_task <==BLE Wireless<br>COBS + Postcard<br>CanFrame==> main_loop

    can_task:::async
    ble_task:::async
    sd_logger:::thread
    pose:::thread
    can_bus:::hardware
    sd_card:::hardware
    rtc:::hardware
    main_loop:::async
    stdin_reader:::thread
    usb:::hardware
    classDef hardware fill:#e1f5ff,stroke:#01579b,stroke-width:2px
    classDef async fill:#fff3e0,stroke:#e65100,stroke-width:2px
    classDef thread fill:#f3e5f5,stroke:#4a148c,stroke-width:2px
```

## Component Locations

### CAN Relay
- **Async Tasks** (LocalExecutor):
  - `can_task()`: `can_relay/src/can_interface/mod.rs`
  - `ble_task()`: `can_relay/src/ble/mod.rs`
- **RTOS Threads**:
  - `sd_logger`: `can_relay/src/sd_logger.rs`
  - `pose`: `can_relay/src/pose_estimation.rs`
- **Static Channels/Signals**: `can_relay/src/main.rs`
  - `LOG_CHANNEL`: Channel<CanFrameForSd, 256>
  - `CAN_WRITE_CHANNEL`: Channel<protocol::CanFrame, 256>
  - `BLE_TX_ONESHOT`: Signal<BleCanLink>
  - `SD_STATUS`: Signal<SdStatus> (in sd_logger.rs)
  - `ACCEL_SIGNAL`, `GYRO_SIGNAL`: Signals in pose_estimation.rs

### CAN Receiver
- **Async Task** (LocalExecutor):
  - `main_loop()`: `can_receiver/src/main.rs`
- **RTOS Thread**:
  - `stdin_reader`: `can_receiver/src/stdin.rs`
- **Static Channel**: `can_receiver/src/main.rs`
  - `STDIN_COMMAND_CHANNEL`: Channel<Command, 32>

## External Hardware
- **CAN Bus**: Via CAN transceivers (not built into ESP32C6)
- **RTC**: DS3231 I2C real-time clock (not built into ESP32C6)
- **SD Card**: Connected via SPI (card itself is external)
- **USB Serial**: To host PC for telemetry output

## Data Flows
- **CAN → SD Logging**: CAN frames flow through `LOG_CHANNEL` to SD card
- **CAN → BLE → USB**: Telemetry data sent wirelessly, serialized as JSON to USB
- **USB → BLE → CAN**: Commands from host flow back to CAN bus
- **IMU Processing**: Accelerometer and gyroscope signals to pose estimation
