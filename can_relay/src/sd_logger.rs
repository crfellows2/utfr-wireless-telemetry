use core::fmt;

/// Maximum size of a candump-formatted CAN frame in bytes
/// Measured worst case: 61 bytes. Using 128 for safety margin.
pub const CANDUMP_MAX_FRAME_SIZE: usize = 128;

/// Wrapper to allow writing formatted output to a byte buffer
struct ByteBuffer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> ByteBuffer<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn len(&self) -> usize {
        self.pos
    }
}

impl fmt::Write for ByteBuffer<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        if self.pos + bytes.len() > self.buf.len() {
            return Err(fmt::Error);
        }
        self.buf[self.pos..self.pos + bytes.len()].copy_from_slice(bytes);
        self.pos += bytes.len();
        Ok(())
    }
}

/// Format a CAN frame to candump format
/// Returns number of bytes written, or 0 if buffer too small
pub fn format_candump(
    buf: &mut [u8],
    timestamp_sec: u64,
    timestamp_usec: u32,
    bus_id: u8,
    can_id: u32,
    data: &[u8],
) -> usize {
    use fmt::Write;

    let mut writer = ByteBuffer::new(buf);

    // Format: (1234567890.123456) can0 123#1122334455667788\n
    if write!(
        writer,
        "({}.{:06}) can{} {:X}#",
        timestamp_sec, timestamp_usec, bus_id, can_id
    )
    .is_err()
    {
        return 0;
    }

    // Write data bytes as hex
    for byte in data {
        if write!(writer, "{:02X}", byte).is_err() {
            return 0;
        }
    }

    // Newline
    if writeln!(writer).is_err() {
        return 0;
    }

    writer.len()
}

pub fn test_sd_card(
    spi_peripheral: impl esp_idf_svc::hal::spi::SpiAnyPins,
    sclk: impl esp_idf_svc::hal::gpio::OutputPin,
    mosi: impl esp_idf_svc::hal::gpio::OutputPin,
    miso: impl esp_idf_svc::hal::gpio::InputPin,
    cs: impl esp_idf_svc::hal::gpio::OutputPin,
) {
    use std::fs::File;
    use std::io::{Read, Write};

    use esp_idf_svc::fs::fatfs::Fatfs;
    use esp_idf_svc::hal::gpio::AnyIOPin;
    use esp_idf_svc::hal::sd::{spi::SdSpiHostDriver, SdCardConfiguration, SdCardDriver};
    use esp_idf_svc::hal::spi::{config::DriverConfig, Dma, SpiDriver};
    use esp_idf_svc::io::vfs::MountedFatfs;

    use log::info;

    info!("Starting SD card test...");

    // Test candump formatting
    {
        let mut buf = [0u8; 128];
        let len = format_candump(
            &mut buf,
            1678901234,
            567890,
            0,
            0x123,
            &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88],
        );
        let formatted = core::str::from_utf8(&buf[..len]).unwrap();
        info!("Candump format test: {}", formatted.trim());
        info!("Formatted length: {} bytes", len);
    }

    // Determine actual maximum candump frame size
    {
        let mut buf = [0u8; 256];

        // Worst case: max timestamp, extended ID, max data
        let max_size = format_candump(
            &mut buf,
            u64::MAX,   // Max timestamp seconds (20 digits)
            999999,     // Max microseconds (6 digits)
            9,          // Max single-digit bus
            0x1FFFFFFF, // Max extended CAN ID (29-bit)
            &[0xFF; 8], // Max 8 bytes data
        );

        let formatted = core::str::from_utf8(&buf[..max_size]).unwrap();
        info!("Maximum candump frame size test:");
        info!("  Formatted: {}", formatted.trim());
        info!("  Size: {} bytes", max_size);
    }

    let spi_driver = SpiDriver::new(
        spi_peripheral,
        sclk,
        mosi,
        Some(miso),
        &DriverConfig::default().dma(Dma::Auto(4096)),
    )
    .expect("Failed to create SPI driver");

    info!("SPI driver created");

    let sd_card_driver = SdCardDriver::new_spi(
        SdSpiHostDriver::new(
            spi_driver,
            Some(cs),
            AnyIOPin::none(),
            AnyIOPin::none(),
            AnyIOPin::none(),
            None, // For ESP-IDF v5.2+
        )
        .expect("Failed to create SD SPI host driver"),
        &SdCardConfiguration::new(),
    )
    .expect("Failed to create SD card driver");

    info!("SD card driver created");

    // Keep it around or else it will be dropped and unmounted
    let _mounted_fatfs = MountedFatfs::mount(
        Fatfs::new_sdcard(0, sd_card_driver).expect("Failed to create FATFS"),
        "/sd",
        4,
    )
    .expect("Failed to mount SD card");

    info!("SD card mounted at /sd");

    // Check if we can read the directory
    match std::fs::read_dir("/sd") {
        Ok(entries) => {
            info!("SD card directory listing:");
            for entry in entries {
                match entry {
                    Ok(e) => info!("  - {:?}", e.file_name()),
                    Err(e) => info!("  Error reading entry: {:?}", e),
                }
            }
        }
        Err(e) => {
            info!("Failed to read /sd directory: {:?}", e);
        }
    }

    let test_content = b"SD card test successful!";

    {
        let mut file = File::create("/sd/test.txt").expect("Failed to create file");
        file.write_all(test_content)
            .expect("Failed to write to file");
        info!("Test file written");
    }

    // Benchmark write performance
    info!("Starting write benchmark...");
    {
        use std::fs::OpenOptions;

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open("/sd/bench.txt")
            .expect("Failed to create benchmark file");

        const DATA_LEN: usize = 4096;
        let test_data = [b'X'; DATA_LEN]; // 512 bytes

        let start = unsafe { esp_idf_svc::sys::esp_timer_get_time() };
        for _ in 0..100 {
            file.write_all(&test_data).expect("Benchmark write failed");
        }
        file.flush().expect("Flush failed");
        let end = unsafe { esp_idf_svc::sys::esp_timer_get_time() };

        let duration_us = end - start;
        let total_bytes = DATA_LEN * 100;
        info!("Wrote {} bytes in {} μs", total_bytes, duration_us);
        info!("Average per write: {} μs", duration_us / 100);
        info!(
            "Throughput: {} KB/s",
            (total_bytes as u64 * 1_000_000) / (duration_us as u64 * 1024)
        );
    }

    {
        let mut file = File::open("/sd/test.txt").expect("Failed to open file");
        let mut read_content = String::new();
        file.read_to_string(&mut read_content)
            .expect("Failed to read file");

        info!("File content: {}", read_content);

        assert_eq!(read_content.as_bytes(), test_content);
        info!("SD card test PASSED!");
    }
}
