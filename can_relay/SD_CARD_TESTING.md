# SD Card Testing and Performance Analysis

**Date**: 2026-03-16
**Hardware**: ESP32-C6 with SD card via SPI interface
**Project**: UTFR Wireless Telemetry System

## Hardware Configuration

### Pin Assignments
- **SCLK (Clock)**: GPIO22
- **MOSI (Master Out, Slave In)**: GPIO2
- **MISO (Master In, Slave Out)**: GPIO21
- **CS (Chip Select)**: GPIO23
- **SPI Peripheral**: SPI2

### SPI Configuration
- **Mode**: SPI (not SDIO)
- **DMA**: Enabled with 4KB buffer (`Dma::Auto(4096)`)
- **Speed**: Default (limited by HAL configuration options)

## SD Card Formatting Requirements

### Critical Discovery: Partition Table Required

Initial testing revealed that ESP-IDF's FATFS driver **requires a partition table** on the SD card.

**Failure Case**:
- Format: Superfloppy (no partition table, entire device formatted as FAT32)
- Command: `sudo mkfs.vfat -F 32 -I /dev/mmcblk0`
- Result: ❌ Driver creation fails with `ESP_ERR_TIMEOUT`

**Success Case**:
- Format: MBR partition table with FAT32 partition
- Commands:
  ```bash
  sudo fdisk /dev/mmcblk0
  # Create DOS/MBR partition table (o)
  # Create new partition (n, p, 1, defaults)
  # Write changes (w)

  sudo mkfs.vfat -F 32 /dev/mmcblk0p1
  ```
- Result: ✅ Driver initializes successfully

### Filesystem Support
- **Supported**: FAT12, FAT16, FAT32
- **Not Supported**: exFAT, NTFS
- **Note**: Cards >32GB are typically exFAT by default and must be reformatted

**Reference**: [ESP-IDF FATFS Documentation](https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/storage/fatfs.html)

## Write Performance Benchmarks

All benchmarks performed writing to `/sd/` mount point with DMA-enabled SPI.

### Test Methodology
- 100 sequential writes of varying buffer sizes
- Timing via `esp_timer_get_time()` (microsecond precision)
- File flush after all writes
- Release build configuration

### Results

| Buffer Size | Time/Write | Throughput | Write Blocking Time |
|-------------|-----------|------------|---------------------|
| 512 bytes   | 962 μs    | 519 KB/s   | ~1 ms               |
| 4096 bytes  | 3,662 μs  | 1,092 KB/s | ~3.7 ms             |
| 16384 bytes | 12,889 μs | 1,241 KB/s | ~12.9 ms            |

### Key Findings

1. **4KB Buffer is Optimal**
   - 2.1x throughput improvement over 512-byte writes
   - Reasonable blocking time (3.7ms)
   - Matches DMA buffer size for efficiency

2. **Diminishing Returns Above 4KB**
   - 16KB: Only 14% throughput gain vs 4KB
   - 3.5x longer blocking time
   - Increased risk of hardware FIFO overflow

3. **Throughput Ceiling**
   - Maximum observed: ~1.2 MB/s
   - **SPI Mode Limitation**: Theoretical max 10-25 MB/s (vs 100 MB/s SD card spec)
   - SPI uses 4-wire interface vs SDIO's 4-bit/8-bit parallel
   - Sufficient for dual 1Mbit CAN buses (~500-750 KB/s formatted)

4. **File Operations are Blocking**
   - `std::fs::File::write_all()` blocks the executor/task
   - DMA handles SPI byte transfers, but filesystem operations are synchronous
   - CPU waiting on: FATFS updates, cluster allocation, DMA completion, SD card latency

## Implications for CAN Logging

### Data Rate Analysis

**CAN Bus Specifications**:
- Two 1Mbit/s CAN buses
- Theoretical max: 2Mbit/s = 250 KB/s raw data
- With candump formatting (~60 bytes/frame): ~500-750 KB/s

**Write Performance**:
- SD throughput: 1.2 MB/s
- **Safety margin**: ~2x

### Hardware FIFO Considerations

ESP32-C6 CAN controller has 64-frame receive FIFO.

**At 10,000 frames/sec (combined, high load)**:

| Write Size | Blocking Time | Frames Arriving | FIFO Status |
|------------|---------------|----------------|-------------|
| 4KB        | 3.7 ms        | ~37 frames     | ✅ Safe (42% full) |
| 16KB       | 12.9 ms       | ~129 frames    | ❌ Overflow (202% full) |

**Conclusion**: 4KB batch writes provide adequate throughput without risking FIFO overflow.

## Buffering Architecture

### Recommended Design

```
CAN Interrupt → CAN Task → Embassy Channel (256 frames) → Logger Task → 4KB Batch Buffer → SD Card
```

**Benefits**:
1. **Prevents FIFO overflow**: Channel absorbs bursts during SD writes
2. **Multi-bus aggregation**: Two CAN tasks feed one logger
3. **Code separation**: Reception vs persistence logic decoupled
4. **Batching efficiency**: Amortizes filesystem overhead

### Alternative (Direct Batching)

```
CAN Interrupt → CAN Task → 4KB Batch Buffer → SD Card
```

**Trade-offs**:
- Simpler architecture
- Relies on 64-frame hardware FIFO during writes
- Less safety margin for burst traffic
- Harder to extend (e.g., add BLE transmission)

### Async Executor Behavior

**Key Insight**: Both approaches run on same FreeRTOS task via Embassy executor.

- ❌ **Not true preemption**: SD write blocks entire executor
- ✅ **Channel provides buffering**: Decouples reception from persistence
- ⚠️ **Without channel**: Relies solely on hardware FIFO during blocking writes

**True preemption** would require:
- Separate high-priority FreeRTOS task for CAN reception
- Low-priority blocking task for SD writes
- Not necessary given current performance margins

## Log Format Selection

### Linux SocketCAN Format (Chosen)

**Format**: `(timestamp) canX ID#DATA\n`

**Example**:
```
(1678901234.567890) can0 123#1122334455667788
(1678901234.568123) can1 1ABCDEF0#DEADBEEF12345678
```

**Advantages**:
- Industry standard (automotive/embedded Linux)
- Human-readable for debugging
- Compatible with `canutils` (`canplayer`, `log2asc`, etc.)
- Simple to implement without allocation
- Easy to parse for custom analysis tools

**Size**: ~45-60 bytes per frame (formatted)

## Open Questions

1. **RTC Integration**: Need timestamp source (current uses microseconds since boot)
2. **File Rotation**: Strategy for managing log file sizes (daily? size-based?)
3. **Error Handling**: Behavior when SD card fills or fails
4. **Power Loss**: Data loss window = channel size + batch buffer (~260 frames max)

## Test Code Location

See: `utfr-wireless-telemetry/can_relay/src/sd_logger.rs`

Function: `test_sd_card()` - Includes directory listing, write/read verification, and performance benchmarking.
