use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use nalgebra::{Matrix2, Vector2, Vector3};
use embassy_time::{Duration, Ticker};
use crate::can_interface::CanFrameForSd;

pub static ACCEL_SIGNAL: Signal<CriticalSectionRawMutex, CanFrameForSd> = Signal::new();
pub static GYRO_SIGNAL: Signal<CriticalSectionRawMutex, CanFrameForSd> = Signal::new();

// Encoding matches mock_car_data_transmit.ino: big-endian i16, high byte first.
// Accel:  raw = value_g * 2048  →  value_g = raw / 2048
// Gyro:   raw = value_degs * 16.384  →  value_degs = raw / 16.384  →  value_rads = raw / 16.384 * π/180
const ACCEL_SCALE: f32 = 1.0 / 2048.0;
const GYRO_SCALE: f32 = 1.0 / 16.384 * (core::f32::consts::PI / 180.0);

fn parse_accel(frame: &CanFrameForSd) -> (f32, f32, f32) {
    let ax = i16::from_be_bytes([frame.data[0], frame.data[1]]) as f32 * ACCEL_SCALE;
    let ay = i16::from_be_bytes([frame.data[2], frame.data[3]]) as f32 * ACCEL_SCALE;
    let az = i16::from_be_bytes([frame.data[4], frame.data[5]]) as f32 * ACCEL_SCALE;
    (ax, ay, az)
}

fn parse_gyro(frame: &CanFrameForSd) -> (f32, f32, f32) {
    let gx = i16::from_be_bytes([frame.data[0], frame.data[1]]) as f32 * GYRO_SCALE;
    let gy = i16::from_be_bytes([frame.data[2], frame.data[3]]) as f32 * GYRO_SCALE;
    let gz = i16::from_be_bytes([frame.data[4], frame.data[5]]) as f32 * GYRO_SCALE;
    (gx, gy, gz)
}

pub struct EKF {
    pub roll: f32,              // radians
    pub pitch: f32,             // radians
    state: Vector2<f32>,        // [roll, pitch]
    gyro_bias: Vector3<f32>,    // [gx, gy, gz] in rad/s
    p: Matrix2<f32>,            // error covariance
    q: Matrix2<f32>,            // process noise
    r: Matrix2<f32>,            // measurement noise
    h: Matrix2<f32>,            // observation matrix (identity)
}

impl EKF {
    /// gyro_bias: [gx, gy, gz] in rad/s
    /// accel_var: (var_x, var_y) — roll/pitch measurement noise from accelerometer
    /// gyro_var:  (var_x, var_y) — roll/pitch process noise from gyro
    pub fn new(
        gyro_bias: Vector3<f32>,
        accel_var: (f32, f32),
        gyro_var: (f32, f32),
    ) -> Self {
        let mut ekf = Self {
            roll: 0.0,
            pitch: 0.0,
            state: Vector2::zeros(),
            gyro_bias,
            p: Matrix2::zeros(),
            q: Matrix2::zeros(),
            r: Matrix2::zeros(),
            h: Matrix2::identity(),
        };

        ekf.p[(0, 0)] = gyro_var.0;  // roll
        ekf.p[(1, 1)] = gyro_var.1;  // pitch

        ekf.q[(0, 0)] = gyro_var.0;
        ekf.q[(1, 1)] = gyro_var.1;

        ekf.r[(0, 0)] = accel_var.0; // roll from accel
        ekf.r[(1, 1)] = accel_var.1; // pitch from accel

        ekf
    }

    pub fn predict(&mut self, gyro: &CanFrameForSd, dt: f32) {
        let (gx, gy, _) = parse_gyro(gyro);

        self.state[0] += (gx - self.gyro_bias[0]) * dt; // roll
        self.state[1] += (gy - self.gyro_bias[1]) * dt; // pitch

        self.p += self.q;
    }

    pub fn update(&mut self, accel: &CanFrameForSd) {
        let (ax, ay, az) = parse_accel(accel);

        let a_mag = (ax * ax + ay * ay + az * az).sqrt();
        let roll_accel = ay.atan2(az);
        let pitch_accel = (-ax).atan2((ay * ay + az * az).sqrt());

        // Only correct with accel when not under significant dynamic acceleration
        let mut measurement = self.state;
        let is_dynamic = (a_mag - 1.0).abs() > 0.15;
        if !is_dynamic {
            measurement[0] = roll_accel;
            measurement[1] = pitch_accel;
        }

        let y = measurement - self.h * self.state;
        let s = self.h * self.p * self.h.transpose() + self.r;

        if let Some(s_inv) = s.try_inverse() {
            let k = self.p * self.h.transpose() * s_inv;
            self.state += k * y;
            self.p = (Matrix2::identity() - k * self.h) * self.p;
        } else {
            log::warn!("EKF: S matrix not invertible, skipping update");
        }

        self.roll = self.state[0];
        self.pitch = self.state[1];
    }
}

pub fn start_pose_estimation_thread() -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("pose_estimation".to_string())
        .stack_size(8192)
        .spawn(pose_estimation_thread_main)?;

    Ok(())
}

fn pose_estimation_thread_main() {
    use esp_idf_svc::hal::task::block_on;

    // TODO: replace with values computed from a calibration pass at startup
    let gyro_bias = Vector3::new(0.0f32, 0.0, 0.0);
    let mut ekf = EKF::new(gyro_bias, (0.001, 0.001), (0.001, 0.001));
    let mut last_ts: Option<u64> = None;
    let mut ticker = Ticker::every(Duration::from_hz(10));

    block_on(async {
        loop {
            let accel = ACCEL_SIGNAL.wait().await;
            let gyro = GYRO_SIGNAL.wait().await;

            let ts_us = accel.timestamp_sec * 1_000_000 + accel.timestamp_usec as u64;
            let dt = if let Some(last) = last_ts {
                ts_us.saturating_sub(last) as f32 / 1_000_000.0
            } else {
                0.01 // fallback for first iteration
            };
            last_ts = Some(ts_us);

            ekf.predict(&gyro, dt);
            ekf.update(&accel);
            
            log::info!("roll: {:.3} deg, pitch: {:.3} deg", ekf.roll * 180.0 / std::f32::consts::PI, ekf.pitch * 180.0 / std::f32::consts::PI);
            ticker.next().await;
        }
    });
}
