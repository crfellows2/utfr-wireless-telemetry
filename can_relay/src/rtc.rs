use ds323x::{DateTimeAccess, Datelike, Ds323x, NaiveDateTime};
use esp_idf_svc::hal::{
    gpio::{InputPin, OutputPin},
    i2c::{I2cConfig, I2cDriver},
    units::FromValueType,
};
use esp_idf_svc::nvs::{EspNvs, NvsDefault};
use log::info;

const NVS_TIME_SOURCE_KEY: &str = "time_src";

/// Time source priority - higher value = more accurate
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum TimeSource {
    Invalid = 0, // Year < 2024, RTC never set or battery died
    BuildTime = 1, // Set from compile timestamp (±minutes of accuracy)
                 // Future time sources (add as needed):
                 // BaseStation = 2,  // Synced from pit laptop via BLE
                 // CanBus = 3,       // Synced from another ECU on CAN bus
                 // Ntp = 4,          // Network Time Protocol (if WiFi added)
}

impl From<u8> for TimeSource {
    fn from(val: u8) -> Self {
        match val {
            0 => TimeSource::Invalid,
            1 => TimeSource::BuildTime,
            _ => TimeSource::Invalid, // Unknown sources default to Invalid
        }
    }
}

pub struct RtcManager {
    rtc: Ds323x<
        ds323x::interface::I2cInterface<esp_idf_svc::hal::i2c::I2cDriver<'static>>,
        ds323x::ic::DS3231,
    >,
}

impl RtcManager {
    /// Initialize the DS3231 RTC on the I2C bus
    pub fn new(
        i2c_peripheral: impl esp_idf_svc::hal::i2c::I2c + 'static,
        sda: impl InputPin + OutputPin + 'static,
        scl: impl InputPin + OutputPin + 'static,
    ) -> Result<Self, esp_idf_svc::hal::i2c::I2cError> {
        info!("Initializing DS3231 RTC...");

        // Use 400kHz (fast mode) - DS3231 supports up to 400kHz
        let config = I2cConfig::new().baudrate(400_u32.kHz().into());

        let i2c = I2cDriver::new(i2c_peripheral, sda, scl, &config)?;

        let rtc = Ds323x::new_ds3231(i2c);

        info!("DS3231 RTC initialized successfully");

        Ok(RtcManager { rtc })
    }

    /// Set the current date and time
    ///
    /// # Arguments
    /// * `datetime` - NaiveDateTime object (year, month, day, hour, minute, second)
    ///
    /// # Example
    /// ```
    /// use ds323x::NaiveDate;
    /// let dt = NaiveDate::from_ymd_opt(2026, 3, 17)
    ///     .unwrap()
    ///     .and_hms_opt(14, 30, 0)
    ///     .unwrap();
    /// rtc_manager.set_datetime(&dt)?;
    /// ```
    pub fn set_datetime(
        &mut self,
        datetime: &NaiveDateTime,
    ) -> Result<(), ds323x::Error<esp_idf_svc::hal::i2c::I2cError>> {
        self.rtc.set_datetime(datetime)?;
        info!("RTC time set to: {}", datetime);
        Ok(())
    }

    /// Get the current date and time from the RTC
    pub fn get_datetime(
        &mut self,
    ) -> Result<NaiveDateTime, ds323x::Error<esp_idf_svc::hal::i2c::I2cError>> {
        self.rtc.datetime()
    }

    /// Get a formatted timestamp string suitable for logging
    /// Format: "YYYY-MM-DD HH:MM:SS"
    pub fn get_timestamp_string(
        &mut self,
    ) -> Result<String, ds323x::Error<esp_idf_svc::hal::i2c::I2cError>> {
        let dt = self.rtc.datetime()?;
        Ok(format!("{}", dt))
    }

    /// Get a formatted filename-safe timestamp
    /// Format: "YYYY-MM-DD_HH-MM-SS"
    pub fn get_filename_timestamp(
        &mut self,
    ) -> Result<String, ds323x::Error<esp_idf_svc::hal::i2c::I2cError>> {
        let dt = self.rtc.datetime()?;
        // Manually format to be filename-safe (replace spaces and colons)
        let timestamp = format!("{}", dt);
        Ok(timestamp.replace(" ", "_").replace(":", "-"))
    }

    /// Get Unix timestamp (seconds since epoch)
    pub fn get_unix_timestamp(
        &mut self,
    ) -> Result<i64, ds323x::Error<esp_idf_svc::hal::i2c::I2cError>> {
        let dt = self.rtc.datetime()?;
        Ok(dt.and_utc().timestamp())
    }

    /// Get the RTC temperature (DS3231 has built-in temperature sensor)
    pub fn get_temperature(
        &mut self,
    ) -> Result<f32, ds323x::Error<esp_idf_svc::hal::i2c::I2cError>> {
        self.rtc.temperature()
    }

    /// Get the current time source from NVS
    fn get_time_source(
        nvs: &EspNvs<NvsDefault>,
    ) -> Result<Option<TimeSource>, esp_idf_svc::sys::EspError> {
        match nvs.get_u8(NVS_TIME_SOURCE_KEY)? {
            Some(val) => Ok(Some(TimeSource::from(val))),
            None => Ok(None),
        }
    }

    /// Set the time source in NVS
    fn set_time_source(
        nvs: &mut EspNvs<NvsDefault>,
        source: TimeSource,
    ) -> Result<(), esp_idf_svc::sys::EspError> {
        nvs.set_u8(NVS_TIME_SOURCE_KEY, source as u8)
    }

    /// Initialize RTC time on first boot or validate existing time
    ///
    /// This function:
    /// - On first boot: Sets RTC to build time (if available)
    /// - On subsequent boots: Validates RTC hasn't reset (battery died)
    /// - Never uses stale build time from old firmware
    pub fn initialize_time(
        &mut self,
        nvs: &mut EspNvs<NvsDefault>,
    ) -> Result<(), esp_idf_svc::sys::EspError> {
        // Check if we've ever set a time source (indicates first boot)
        match Self::get_time_source(nvs)? {
            None => {
                // First boot ever - NVS key doesn't exist
                info!("First boot detected (NVS empty)");
                if let Some(build_time) = get_build_time() {
                    info!("Setting initial time to build time: {}", build_time);
                    match self.set_datetime(&build_time) {
                        Ok(_) => Self::set_time_source(nvs, TimeSource::BuildTime)?,
                        Err(e) => {
                            log::error!("Failed to set RTC time: {:?}", e);
                            Self::set_time_source(nvs, TimeSource::Invalid)?;
                        }
                    }
                } else {
                    log::warn!("No build time available, marking as Invalid");
                    Self::set_time_source(nvs, TimeSource::Invalid)?;
                }
            }

            Some(claimed_source) => {
                // NVS exists - validate RTC matches what we claim
                match self.get_datetime() {
                    Ok(rtc_time) => {
                        if rtc_time.year() < 2024 && claimed_source != TimeSource::Invalid {
                            log::warn!(
                                "Battery died - RTC shows {} but source was {:?}",
                                rtc_time,
                                claimed_source
                            );
                            log::warn!("Downgrading to Invalid (NOT using stale build time)");
                            Self::set_time_source(nvs, TimeSource::Invalid)?;
                        } else if rtc_time.year() < 2024 {
                            log::warn!("RTC time invalid: {}", rtc_time);
                        } else {
                            info!("RTC time valid: {}, source: {:?}", rtc_time, claimed_source);
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to read RTC time: {:?}", e);
                        Self::set_time_source(nvs, TimeSource::Invalid)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Sync ESP32 system time with DS3231 RTC
    ///
    /// This sets the ESP32's internal RTC to match the DS3231, allowing
    /// fast microsecond-precision timestamps using system time APIs.
    ///
    /// Call this:
    /// - Once at boot after initialize_time()
    /// - Periodically (e.g., hourly) to correct ESP32 RTC drift
    pub fn sync_system_time(
        &mut self,
    ) -> Result<(), ds323x::Error<esp_idf_svc::hal::i2c::I2cError>> {
        let dt = self.get_datetime()?;

        // Convert to Unix timestamp
        let timestamp = dt.and_utc().timestamp();

        // Set ESP32 system time
        let timeval = esp_idf_svc::sys::timeval {
            tv_sec: timestamp,
            tv_usec: 0,
        };

        unsafe {
            esp_idf_svc::sys::settimeofday(&timeval as *const _, std::ptr::null());
        }

        // Verify the sync worked by reading back system time
        let (sec_after, usec_after) = get_system_timestamp_us();
        info!("DS3231 time: {}", dt);
        info!("System time after sync: {}.{:06}", sec_after, usec_after);

        Ok(())
    }
}

/// Helper function to create a NaiveDateTime from components
pub fn create_datetime(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Option<NaiveDateTime> {
    use ds323x::NaiveDate;

    NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|date| date.and_hms_opt(hour, minute, second))
}

/// Get build time from the BUILD_TIMESTAMP environment variable set during compilation
pub fn get_build_time() -> Option<NaiveDateTime> {
    let timestamp_str = env!("BUILD_TIMESTAMP");
    let timestamp: i64 = timestamp_str.parse().ok()?;
    Some(NaiveDateTime::from_timestamp(timestamp, 0))
}

/// Get current system timestamp with microsecond precision
///
/// Returns (seconds_since_epoch, microseconds)
///
/// This reads the ESP32's internal RTC (very fast, <1µs) and should be used
/// for timestamping CAN messages and other real-time events.
///
/// Note: ESP32 RTC drifts ~20-50ppm, so periodic resyncing with DS3231
/// is recommended (call sync_system_time() hourly/daily).
pub fn get_system_timestamp_us() -> (i64, i64) {
    let mut tv = esp_idf_svc::sys::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };

    unsafe {
        esp_idf_svc::sys::gettimeofday(&mut tv as *mut _, std::ptr::null_mut());
    }

    (tv.tv_sec, tv.tv_usec as i64)
}

/// Get current system timestamp as a single i64 microsecond value
///
/// Useful for calculating time deltas and high-precision logging.
pub fn get_system_timestamp_us_single() -> i64 {
    let (sec, usec) = get_system_timestamp_us();
    sec * 1_000_000 + usec
}
