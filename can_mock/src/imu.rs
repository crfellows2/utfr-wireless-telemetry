use crate::key_store::{self, KEY_CALIBRATION};
use bmi270::{
    registers::{
        AccelerometerBandwidth, AccelerometerConfig, GyroscopeBandwidth, GyroscopeConfig,
        OutputDataRate, PerformanceKind,
    },
    units::*,
    Bmi270Sync,
};
use control_loop::brake_control::Vector3;
use control_loop::ekf::Matrix3;
use derive_more::From;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::once_lock::OnceLock;
use embassy_sync::watch::Watch;
use embedded_registers::i2c::I2cDeviceSync;
use esp_idf_svc::{
    hal::{
        delay::Delay,
        i2c::{I2cDriver, I2cError},
    },
    sys::esp_timer_get_time,
    timer::EspTimerService,
};
use log::{error, info, warn};
use std::{cell::RefCell, thread_local, time::Duration};

pub mod calibration;
use calibration::CalibrationData;

/// IMU data with timestamp information
#[allow(unused)]
#[derive(Debug, Clone)]
pub struct ImuData {
    pub acceleration: Vector3<f32>,     // gs
    pub angular_velocity: Vector3<f32>, // dps
    pub timestamp_us: u64,
}

/// Cached IMU data with age information
#[derive(Debug)]
struct CachedImuData {
    data: ImuData,
    timestamp_us: u64,
}

pub type Bmi270 = Bmi270Sync<
    I2cDeviceSync<I2cDriver<'static>, embedded_hal::i2c::SevenBitAddress, bmi270::Bmi270I2cCodec>,
    Delay,
>;

/// Hardware wrapper for the BMI270 sensor
struct ImuHardware {
    bmi: Bmi270,
    rotation_matrix: Option<Matrix3<f32>>, // None = not calibrated
    last_read_time_us: u64,
}

/// Error types for IMU operations
#[derive(Debug, From)]
pub enum ImuError {
    HardwareFault,
    CalibrationRequired,
    CommunicationFailed,
    NotInitialized,

    #[from]
    #[allow(unused)]
    Calibration(calibration::CalibrationError),
}

// False positive on embedded targets - known Clippy issues:
// - https://github.com/rust-lang/rust-clippy/pull/12026 (doesn't work on all targets)
// - https://github.com/rust-lang/rust-clippy/issues/12637 (RefCell false positives)
// - https://github.com/rust-lang/rust-clippy/issues/12404 (MSRV issues)
thread_local! {
    /// We can share references through thread local storage because we are using a single threaded
    /// executor. This means:
    /// 1. No concurrent access from other threads
    /// 2. Only one instance of this will exist
    /// We can't hold a RefCell reference across await points, so it is impossible for another
    /// async task to try and access this data at the same time.
    /// Caveat: this is not safe to use from ISR. check that callbacks using the IMU aren't running
    /// in ISR

    /// Global cache storage
    static IMU_CACHE: RefCell<Option<CachedImuData>> = const { RefCell::new(None) };
    /// Global hardware storage
    static IMU_HARDWARE: RefCell<Option<ImuHardware>> = const { RefCell::new(None) };
}

// Compile-time documentation: Assert single HP core assumption
#[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
compile_error!("IMU service uses thread_local RefCell which assumes single HP core ESP32-C6/ESP32");

// Runtime check: Ensure we're on single HP core ESP32-C6
const _: () = {
    // ESP32-C6 has only one HP core (LP core uses separate memory/programming model)
    const fn assert_single_hp_core() {
        // This will be optimized out but documents the assumption
        #[cfg(target_arch = "riscv32")]
        {
            // ESP32-C6 is RISC-V based with single HP core
        }
    }
    assert_single_hp_core();
};

/// One-time initialization lock - set once when IMU service is fully ready
static IMU_READY: OnceLock<()> = OnceLock::new();

/// Watch for broadcasting "ready" event to all waiting tasks
static IMU_READY_WATCH: Watch<CriticalSectionRawMutex, bool, 2> = Watch::new();

/// Load rotation matrix from calibration data in key store
fn load_rotation_matrix() -> Option<Matrix3<f32>> {
    let mut buf = [0u8; CalibrationData::serial_length()];

    match key_store::get_key(KEY_CALIBRATION, &mut buf) {
        Some(data) => match CalibrationData::from_bytes(data) {
            Ok(calib_data) => {
                info!("Loaded rotation matrix from calibration data");
                Some(calib_data.r_matrix)
            }
            Err(e) => {
                warn!("Failed to deserialize calibration data: {:?}", e);
                None
            }
        },
        None => {
            info!("No calibration data in key store - calibration required");
            None
        }
    }
}

/// Internal function to mark IMU service as ready and signal all waiting tasks
fn mark_ready() {
    // Only signal if we weren't already ready
    if IMU_READY.try_get().is_none() {
        // Set the one-time lock (ignore error if already set)
        let _ = IMU_READY.init(());

        // Broadcast to ALL waiting tasks
        IMU_READY_WATCH.sender().send(true);

        info!("IMU service marked as ready - all waiting tasks notified");
    }
}

/// Internal function to check if we should be ready and mark if so
fn check_and_mark_ready() {
    if is_initialized() && is_calibrated() {
        mark_ready();
    }
}

/// Initialize the IMU service with hardware (tries to load calibration automatically)
/// TODO: provide the imu during setup (blocking) so that any async task can initialize it
pub async fn init_imu_service(mut bmi: Bmi270, max_tries: usize) -> Result<(), ImuError> {
    info!("Initializing IMU...");
    let timer_service = EspTimerService::new().unwrap();
    let mut timer = timer_service
        .timer_async()
        .expect("Should be able to get a new timer");

    for attempt in 1..=max_tries {
        match bmi.init(
            Some(
                AccelerometerConfig::new_checked::<bmi270::types::Error<I2cError>>(
                    OutputDataRate::Hundred,
                    AccelerometerBandwidth::Osr4Avg1,
                    PerformanceKind::Power,
                )
                .unwrap(),
            ),
            Some(
                GyroscopeConfig::new_checked::<bmi270::types::Error<I2cError>>(
                    OutputDataRate::Hundred,
                    GyroscopeBandwidth::Norm,
                    PerformanceKind::Power,
                    PerformanceKind::Power,
                )
                .unwrap(),
            ),
        ) {
            Ok(_) => {
                info!("Initialized IMU on attempt #{attempt}");
                break;
            }
            Err(e) if attempt == max_tries => {
                error!("Failed to Initialize IMU: {e:?}");
                return Err(ImuError::CommunicationFailed);
            }
            Err(e) => {
                warn!("Failed to Initialize IMU (attempt #{attempt}): {e:?}. Trying Again...");
                let _ = timer.after(core::time::Duration::from_millis(250)).await;
            }
        }
    }

    // Try to load calibration from key store
    let rotation_matrix = load_rotation_matrix();

    let hardware = ImuHardware {
        bmi,
        rotation_matrix,
        last_read_time_us: 0,
    };

    IMU_HARDWARE.with(|hw| *hw.borrow_mut() = Some(hardware));

    // Check if we're ready after initialization
    check_and_mark_ready();

    //let _ = timer.after(core::time::Duration::from_millis(250)).await;

    Ok(())
}

/// Primary API: Get IMU data (returns owned data)
// TODO: returns all 0 immediatly after init
/// SPI IMU read takes 225us
pub fn get_imu_data(max_age: Duration) -> Result<ImuData, ImuError> {
    let current_time_us = unsafe { esp_timer_get_time() as u64 };
    let max_age_us = max_age.as_micros() as u64;

    // Check cache first
    if let Some(cached_data) = IMU_CACHE.with(|cache| {
        cache.borrow().as_ref().and_then(|cached| {
            // Handle clock wraparound/reset: if cached timestamp is in future, invalidate cache
            let age_us = current_time_us
                .checked_sub(cached.timestamp_us)
                .unwrap_or(u64::MAX);
            if age_us <= max_age_us {
                Some(cached.data.clone())
            } else {
                None
            }
        })
    }) {
        return Ok(cached_data);
    }

    // Cache miss - read fresh data
    let fresh_data = read_fresh_imu_data(current_time_us)?;

    // Update cache
    IMU_CACHE.with(|cache| {
        *cache.borrow_mut() = Some(CachedImuData {
            data: fresh_data.clone(),
            timestamp_us: current_time_us,
        });
    });

    Ok(fresh_data)
}

#[allow(unused)]
/// Get raw IMU data without rotation transformation (for calibration use)
pub fn get_raw_imu_data() -> Result<ImuData, ImuError> {
    let current_time_us = unsafe { esp_timer_get_time() as u64 };
    read_raw_imu_data(current_time_us)
}

/// Internal function to read fresh data from hardware
fn read_fresh_imu_data(timestamp_us: u64) -> Result<ImuData, ImuError> {
    IMU_HARDWARE.with(|hardware| {
        let mut hardware_guard = hardware.borrow_mut();
        let hw = hardware_guard.as_mut().ok_or(ImuError::NotInitialized)?;

        // Check if calibrated
        let rotation_matrix = hw.rotation_matrix.ok_or(ImuError::CalibrationRequired)?;

        // Single read attempt - no retry here
        let accel_result = hw.bmi.read_accelerometer_data(AccelerationUnit::Gs);
        let gyro_result = hw.bmi.read_gyroscope_data(AngularVelocityUnit::Radps);

        match (accel_result, gyro_result) {
            (Ok(accel), Ok(gyro)) => {
                // Apply rotation matrix transformation
                let accel_vec = Vector3::new(accel.ax, accel.ay, accel.az);
                let gyro_vec = Vector3::new(gyro.gx, gyro.gy, gyro.gz);

                let transformed_accel = rotation_matrix * accel_vec;
                let transformed_gyro = rotation_matrix * gyro_vec;

                let imu_data = ImuData {
                    acceleration: transformed_accel,
                    angular_velocity: transformed_gyro,
                    timestamp_us,
                };

                hw.last_read_time_us = timestamp_us;
                Ok(imu_data)
            }
            (Err(e), _) | (_, Err(e)) => {
                error!("IMU read failed: {:?}", e);
                Err(ImuError::HardwareFault)
            }
        }
    })
}

/// Internal function to read raw data from hardware (no rotation applied)
/// Does not update cache because it is not rotated
fn read_raw_imu_data(timestamp_us: u64) -> Result<ImuData, ImuError> {
    IMU_HARDWARE.with(|hardware| {
        let mut hardware_guard = hardware.borrow_mut();
        let hw = hardware_guard.as_mut().ok_or(ImuError::NotInitialized)?;

        // Single read attempt - no retry here
        let accel_result = hw.bmi.read_accelerometer_data(AccelerationUnit::Gs);
        let gyro_result = hw.bmi.read_gyroscope_data(AngularVelocityUnit::Dps);

        match (accel_result, gyro_result) {
            (Ok(accel), Ok(gyro)) => {
                // No rotation matrix transformation - raw sensor data
                let accel_vec = Vector3::new(accel.ax, accel.ay, accel.az);
                let gyro_vec = Vector3::new(gyro.gx, gyro.gy, gyro.gz);

                let imu_data = ImuData {
                    acceleration: accel_vec,
                    angular_velocity: gyro_vec,
                    timestamp_us,
                };

                Ok(imu_data)
            }
            (Err(e), _) | (_, Err(e)) => {
                error!("IMU read failed: {:?}", e);
                Err(ImuError::HardwareFault)
            }
        }
    })
}

/// Check if the IMU service is initialized
pub fn is_initialized() -> bool {
    IMU_HARDWARE.with(|hw| hw.borrow().is_some())
}

/// Check if the IMU service is calibrated
pub fn is_calibrated() -> bool {
    IMU_HARDWARE.with(|hardware| {
        hardware
            .borrow()
            .as_ref()
            .map(|hw| hw.rotation_matrix.is_some())
            .unwrap_or(false)
    })
}

/// Check if the IMU service is fully ready (initialized and calibrated)
pub fn is_ready() -> bool {
    IMU_READY.try_get().is_some()
}

/// Wait for IMU service to be fully ready (efficient for multiple tasks)
pub async fn wait_for_ready() {
    // Fast path: already ready
    if is_ready() {
        return;
    }

    info!("Task waiting for IMU service to be ready...");

    // Slow path: wait for ready signal via Watch
    let mut receiver = match IMU_READY_WATCH.receiver() {
        Some(r) => r,
        None => {
            // fallback to polling if all watch slots are full
            warn!("IMU_READY_WATCH watch slots full. Falling back to polling");
            let timer_service = EspTimerService::new().unwrap();
            let mut timer = timer_service.timer_async().unwrap();
            loop {
                let _ = timer.after(std::time::Duration::from_secs(1)).await;
                if is_ready() {
                    return;
                }
            }
        }
    };

    // Wait for the watch to be set to true
    loop {
        let ready = receiver.changed().await;
        if ready {
            break;
        }
    }

    // After signal, we're guaranteed to be ready
    debug_assert!(is_ready());
    info!("IMU service ready - task can proceed");
}

/// Get the current rotation matrix (if calibrated)
#[allow(unused)]
pub fn get_rotation_matrix() -> Option<Matrix3<f32>> {
    IMU_HARDWARE.with(|hardware| hardware.borrow().as_ref().and_then(|hw| hw.rotation_matrix))
}

/// Get the full calibration data from key store
pub fn get_calibration_data() -> Option<CalibrationData> {
    let mut buf = [0u8; CalibrationData::serial_length()];
    // TODO: cache
    key_store::get_key(KEY_CALIBRATION, &mut buf)
        .and_then(|data| CalibrationData::from_bytes(data).ok())
}

/// Run calibration process using the initialized IMU service
/// Can be called at any time to overwrite calibration
pub async fn run_calibration(identity_override: bool) -> Result<(), ImuError> {
    let timer_service = EspTimerService::new().unwrap();
    let mut timer = timer_service
        .timer_async()
        .expect("Should be able to get a new timer");

    // Run calibration
    let calibration_data = match identity_override {
        false => calibration::run_calibration(&mut timer).await?,
        true => calibration::run_identity_calibration(&mut timer).await?,
    };

    IMU_HARDWARE.with(|hardware| {
        let mut hardware_guard = hardware.borrow_mut();
        let hw = hardware_guard.as_mut().ok_or(ImuError::NotInitialized)?;
        hw.rotation_matrix = Some(calibration_data.r_matrix);
        Ok::<(), ImuError>(())
    })?;

    // Save to key store
    let data = calibration_data
        .to_bytes()
        .map_err(|_| ImuError::CalibrationRequired)?;
    key_store::set_key(KEY_CALIBRATION, &data).map_err(|_| ImuError::CalibrationRequired)?;

    // Clear cache since rotation matrix changed
    IMU_CACHE.with(|cache| *cache.borrow_mut() = None);

    info!("IMU calibration completed and saved");

    // Check if we're ready after calibration
    check_and_mark_ready();

    Ok(())
}
