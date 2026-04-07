use core::fmt;

use embassy_futures::select::{self, Either};
use embassy_sync::channel::Receiver;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};

/// SD card status for BLE reporting
#[derive(Debug, Clone, Copy)]
pub struct SdStatus {
    pub used_kb: u32,
    pub total_kb: u32,
}

impl SdStatus {
    /// Binary format: [used_kb:4][total_kb:4] = 8 bytes
    pub fn to_ble_bytes(&self) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0..4].copy_from_slice(&self.used_kb.to_le_bytes());
        buf[4..8].copy_from_slice(&self.total_kb.to_le_bytes());
        buf
    }
}

/// Signal for SD status updates (signaled after each flush)
pub static SD_STATUS: Signal<CriticalSectionRawMutex, SdStatus> = Signal::new();

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

#[allow(unused)]
pub fn test_sd_card(
    spi_peripheral: impl esp_idf_svc::hal::spi::SpiAnyPins,
    sclk: impl esp_idf_svc::hal::gpio::OutputPin,
    mosi: impl esp_idf_svc::hal::gpio::OutputPin,
    miso: impl esp_idf_svc::hal::gpio::InputPin,
    cs: impl esp_idf_svc::hal::gpio::OutputPin,
) {
    use esp_idf_svc::fs::fatfs::Fatfs;
    use esp_idf_svc::hal::gpio::AnyIOPin;
    use esp_idf_svc::hal::sd::{spi::SdSpiHostDriver, SdCardConfiguration, SdCardDriver};
    use esp_idf_svc::hal::spi::{config::DriverConfig, Dma, SpiDriver};
    use esp_idf_svc::io::vfs::MountedFatfs;
    use log::info;
    use std::fs::File;
    use std::io::Read;

    info!("Starting SD card test...");

    let spi_driver = SpiDriver::new(
        spi_peripheral,
        sclk,
        mosi,
        Some(miso),
        &DriverConfig::default().dma(Dma::Auto(4096)),
    )
    .expect("Failed to create SPI driver");

    let sd_card_driver = SdCardDriver::new_spi(
        SdSpiHostDriver::new(
            spi_driver,
            Some(cs),
            AnyIOPin::none(),
            AnyIOPin::none(),
            AnyIOPin::none(),
            None,
        )
        .expect("Failed to create SD SPI host driver"),
        &SdCardConfiguration::new(),
    )
    .expect("Failed to create SD card driver");

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

use chrono::{Datelike, Timelike};
use embassy_time::{Duration, Instant, Timer};
use esp_idf_svc::hal::task::block_on;
use log::info;
use std::fs::File;
use std::io::Write;

use crate::can_interface::CanFrameForSd;

const BUFFER_SIZE: usize = 4096;
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// Get SD card filesystem stats using FatFs f_getfree
fn get_sd_stats() -> Option<SdStatus> {
    use esp_idf_svc::sys::{f_getfree, FATFS, FRESULT_FR_OK};

    let path = std::ffi::CString::new("/sd").ok()?;
    let mut free_clusters: u32 = 0;
    let mut fs_ptr: *mut FATFS = std::ptr::null_mut();

    let res = unsafe { f_getfree(path.as_ptr(), &mut free_clusters, &mut fs_ptr) };

    if res != FRESULT_FR_OK || fs_ptr.is_null() {
        return None;
    }

    let fs = unsafe { &*fs_ptr };

    // Total sectors = (total FAT entries - 2) * cluster size
    // Free sectors = free clusters * cluster size
    let total_sectors = (fs.n_fatent - 2) as u64 * fs.csize as u64;
    let free_sectors = free_clusters as u64 * fs.csize as u64;
    let sector_size = fs.ssize as u64;

    let total_kb = (total_sectors * sector_size / 1024) as u32;
    let free_kb = (free_sectors * sector_size / 1024) as u32;
    let used_kb = total_kb.saturating_sub(free_kb);

    Some(SdStatus { used_kb, total_kb })
}

/// Signal SD status update after a flush
fn signal_sd_status() {
    if let Some(status) = get_sd_stats() {
        SD_STATUS.signal(status);
    }
}

pub fn start_sd_logger_thread<const N: usize>(
    log_rx: Receiver<'static, CriticalSectionRawMutex, CanFrameForSd, N>,
    spi_peripheral: impl esp_idf_svc::hal::spi::SpiAnyPins + Send + 'static,
    sclk: impl esp_idf_svc::hal::gpio::OutputPin + Send + 'static,
    mosi: impl esp_idf_svc::hal::gpio::OutputPin + Send + 'static,
    miso: impl esp_idf_svc::hal::gpio::InputPin + Send + 'static,
    cs: impl esp_idf_svc::hal::gpio::OutputPin + Send + 'static,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("sd_logger".to_string())
        .stack_size(16384)
        .spawn(move || sd_logger_thread_main(log_rx, spi_peripheral, sclk, mosi, miso, cs))?;

    Ok(())
}

fn get_log_filename() -> String {
    let (sec, _usec) = crate::rtc::get_system_timestamp_us();
    let dt = chrono::NaiveDateTime::from_timestamp_opt(sec, 0)
        .unwrap_or_else(|| chrono::NaiveDateTime::from_timestamp_opt(0, 0).unwrap());
    format!(
        "/sd/can_{:04}{:02}{:02}_{:02}{:02}{:02}.txt",
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second()
    )
}

fn sd_logger_thread_main<const N: usize>(
    log_rx: Receiver<'static, CriticalSectionRawMutex, CanFrameForSd, N>,
    spi_peripheral: impl esp_idf_svc::hal::spi::SpiAnyPins,
    sclk: impl esp_idf_svc::hal::gpio::OutputPin,
    mosi: impl esp_idf_svc::hal::gpio::OutputPin,
    miso: impl esp_idf_svc::hal::gpio::InputPin,
    cs: impl esp_idf_svc::hal::gpio::OutputPin,
) {
    use esp_idf_svc::fs::fatfs::Fatfs;
    use esp_idf_svc::hal::gpio::AnyIOPin;
    use esp_idf_svc::hal::sd::{spi::SdSpiHostDriver, SdCardConfiguration, SdCardDriver};
    use esp_idf_svc::hal::spi::{config::DriverConfig, Dma, SpiDriver};
    use esp_idf_svc::io::vfs::MountedFatfs;

    log::info!("SD logger thread: mounting SD card");

    let spi_driver = SpiDriver::new(
        spi_peripheral,
        sclk,
        mosi,
        Some(miso),
        &DriverConfig::default().dma(Dma::Auto(4096)),
    )
    .expect("Failed to create SPI driver");

    let sd_card_driver = SdCardDriver::new_spi(
        SdSpiHostDriver::new(
            spi_driver,
            Some(cs),
            AnyIOPin::none(),
            AnyIOPin::none(),
            AnyIOPin::none(),
            None,
        )
        .expect("Failed to create SD SPI host driver"),
        &SdCardConfiguration::new(),
    )
    .expect("Failed to create SD card driver");

    let _mounted_fatfs = MountedFatfs::mount(
        Fatfs::new_sdcard(0, sd_card_driver).expect("Failed to create FATFS"),
        "/sd",
        4,
    )
    .expect("Failed to mount SD card");

    log::info!("SD card mounted at /sd");

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

    let filename = get_log_filename();
    log::info!("Opening log file: {}", filename);

    let mut file = File::create(&filename).expect("Failed to create log file");

    // Signal initial SD status
    signal_sd_status();

    let mut buffer: heapless::Vec<u8, BUFFER_SIZE> = heapless::Vec::new();
    let mut last_flush = Instant::now();

    loop {
        let remaining = FLUSH_INTERVAL
            .checked_sub(last_flush.elapsed())
            .unwrap_or(Duration::from_ticks(0));

        match block_on(select::select(log_rx.receive(), Timer::after(remaining))) {
            Either::First(frame) => {
                let mut line_buf = [0u8; CANDUMP_MAX_FRAME_SIZE];
                let len = format_candump(
                    &mut line_buf,
                    frame.timestamp_sec,
                    frame.timestamp_usec,
                    frame.bus_id,
                    frame.can_id,
                    &frame.data[..frame.data_len as usize],
                );

                // Flush if this frame won't fit
                if buffer.len() + len > buffer.capacity() {
                    file.write_all(&buffer).expect("SD write failed");
                    file.sync_all().expect("SD sync failed");
                    buffer.clear();
                    last_flush = Instant::now();
                    signal_sd_status();
                }

                buffer.extend_from_slice(&line_buf[..len]).ok();
            }
            Either::Second(()) => {
                if !buffer.is_empty() {
                    let bytes_len = buffer.len();
                    file.write_all(&buffer).expect("SD write failed");
                    file.sync_all().expect("SD sync failed");
                    buffer.clear();
                    info!("{bytes_len} Bytes synced to SD");
                    signal_sd_status();
                }
                last_flush = Instant::now();
            }
        }
    }
}
