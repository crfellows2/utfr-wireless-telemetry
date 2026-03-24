# RTC Design Documentation

## Overview

This document describes the Real-Time Clock (RTC) implementation for the UTFR wireless telemetry system. The design uses a DS3231MZ+TRL external RTC module combined with the ESP32's internal RTC to provide accurate timestamping for CAN bus data logging.

## Hardware

**DS3231MZ+TRL**
- High-precision RTC with integrated temperature-compensated crystal oscillator (TCXO)
- Accuracy: ±2 ppm (~1 minute/year drift)
- Battery backup via CR2032 (maintains time across power cycles)
- I2C interface
- Connected to ESP32: GPIO 0 (SCL), GPIO 1 (SDA)
- Operating at 400 kHz I2C (fast mode)

## Why Both External and Internal RTC?

The design uses two separate time sources:

**DS3231 (External RTC):**
- Source of truth for absolute time
- High accuracy (±2 ppm)
- Survives power cycles (battery backed)
- Slow to read (~250-500 µs per I2C transaction at 400 kHz)

**ESP32 Internal RTC:**
- Working copy for real-time timestamping
- Fast to read (<1 µs, direct register access)
- Resets on power cycle
- Drifts ~20-50 ppm (uncalibrated)

**Strategy:**
1. At boot: Sync ESP32 RTC ← DS3231 (get accurate absolute time)
2. During operation: Use ESP32 RTC for timestamping (microsecond precision, minimal latency)
3. Note: Periodic resyncing could correct ESP32 drift, but not yet implemented

This approach provides:
- Accurate absolute time (from DS3231)
- Microsecond-precision timestamps (from ESP32)
- Minimal I2C traffic during operation

## Time Source Hierarchy

Time sources are tracked in NVS (Non-Volatile Storage) to maintain knowledge of time quality across reboots.

**Current sources (implemented):**
- `Invalid` (0): RTC never set or battery died (year < 2024)
- `BuildTime` (1): Set from firmware compile timestamp

**Future sources (not yet implemented):**
Notes for potential additions:
- BaseStation sync via BLE when downloading data
- CAN bus time sync from another ECU
- NTP if WiFi added

Higher-value sources are considered more accurate. The system only upgrades time sources, never downgrades (except when detecting battery failure).

## Boot Process

**First boot (fresh firmware flash):**
1. NVS is empty → detects first boot
2. Sets DS3231 to BUILD_TIMESTAMP (captured during compilation)
3. Stores `TimeSource::BuildTime` in NVS
4. Syncs ESP32 system time from DS3231

**Subsequent boots:**
1. Reads time source from NVS
2. Validates DS3231 time matches claimed source (year >= 2024)
3. If valid: keeps existing time
4. If invalid (year < 2024): marks as `Invalid`, does NOT use stale build time
5. Syncs ESP32 system time from DS3231

**Re-flash with new firmware:**
1. NVS gets wiped (default behavior)
2. Counts as first boot with fresh build timestamp

## Design Decisions

### Why not use stale build time after battery failure?

If firmware was built months ago and the battery dies, setting RTC to that old timestamp creates more confusion than knowing the time is invalid. Build time is only used on first boot when it's reasonably fresh.

### Why 400 kHz I2C?

DS3231 supports up to 400 kHz. We chose fast mode to minimize read latency (~250-500 µs vs 1-2 ms at standard 100 kHz). No other devices share the I2C bus.

### Why track time source in NVS instead of RTC registers?

- NVS survives even if RTC battery dies
- Easier to extend with additional metadata (sync quality, last sync timestamp, etc.)
- More reliable than trying to use DS3231's limited user SRAM

### Why not differentiate between "Invalid" and "Reset"?

Kept simple for now. Both states indicate unreliable time. Could be extended later to track battery failure counts or alert users, but not currently needed.

## Implementation Details

**Files:**
- `src/rtc.rs`: RTC manager, time source tracking, system time sync
- `build.rs`: Captures BUILD_TIMESTAMP at compile time
- `src/main.rs`: Initialization sequence

**Key functions:**
- `RtcManager::initialize_time()`: Boot-time initialization and validation
- `RtcManager::sync_system_time()`: Sync ESP32 RTC from DS3231
- `get_system_timestamp_us()`: Fast microsecond timestamps from ESP32 RTC
- `get_build_time()`: Parse BUILD_TIMESTAMP from environment

**NVS storage:**
- Namespace: `"rtc_config"`
- Key: `"time_src"` (u8)
- Stores time source as enum value

## Precision vs Accuracy

**For CAN message timestamping:**
- Absolute accuracy: ±2 ppm from DS3231
- Timestamp precision: Microsecond (from ESP32 RTC)
- Latency: <1 µs to read timestamp

**ESP32 RTC drift:**
The ESP32 internal RTC drifts approximately 20-50 ppm. Without periodic resyncing:
- ~2-4 seconds drift per day
- Acceptable for session-based logging (races last minutes/hours)
- Could add periodic resync for longer logging sessions

## Future Considerations

**Notes for investigation:**

**Base station time sync:**
- Could sync from pit laptop via BLE during data downloads
- Provides NTP-accurate time at the track
- Would become highest-priority time source

**Periodic resync:**
- Could periodically resync ESP32 ← DS3231 to correct drift
- Hourly/daily resync would keep timestamps accurate
- Not implemented yet

**Battery monitoring:**
- Could track battery failure events in NVS
- Alert user when battery needs replacement
- Flag telemetry data as "timestamp uncertain" after battery failure
- Not currently needed

## Testing

The implementation logs both DS3231 and ESP32 system time after sync to verify correctness:
```
I (364) can_relay::rtc: DS3231 time: 2026-03-17 05:10:48
I (364) can_relay::rtc: System time after sync: 1773724248.000042
```

Both timestamps should match (convert to same format to verify).
