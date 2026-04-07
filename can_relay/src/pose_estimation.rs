use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
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

// EKF with H = identity and diagonal P, Q, R — roll and pitch decouple completely,
// so all matrix ops reduce to scalar arithmetic.
pub struct EKF {
    pub roll: f32,   // radians
    pub pitch: f32,  // radians

    // diagonal entries of error covariance P
    p_roll: f32,
    p_pitch: f32,

    // diagonal entries of process noise Q
    q_roll: f32,
    q_pitch: f32,

    // diagonal entries of measurement noise R
    r_roll: f32,
    r_pitch: f32,
}

impl EKF {
    /// accel_var: (var_roll, var_pitch) — measurement noise from accelerometer
    /// gyro_var:  (var_roll, var_pitch) — process noise from gyro
    pub fn new(
        accel_var: (f32, f32),
        gyro_var: (f32, f32),
    ) -> Self {
        Self {
            roll: 0.0,
            pitch: 0.0,
            p_roll: gyro_var.0,
            p_pitch: gyro_var.1,
            q_roll: gyro_var.0,
            q_pitch: gyro_var.1,
            r_roll: accel_var.0,
            r_pitch: accel_var.1,
        }
    }

    pub fn predict(&mut self, gyro: &CanFrameForSd, dt: f32) {
        let (gx, gy, _) = parse_gyro(gyro);
        self.roll  += gx * dt;
        self.pitch += gy * dt;
        self.p_roll  += self.q_roll;
        self.p_pitch += self.q_pitch;
    }

    pub fn update(&mut self, accel: &CanFrameForSd) {
        let (ax, ay, az) = parse_accel(accel);

        let a_mag = (ax * ax + ay * ay + az * az).sqrt();
        let roll_accel  = ay.atan2(az);
        let pitch_accel = (-ax).atan2((ay * ay + az * az).sqrt());

        // Only correct with accel when not under significant dynamic acceleration
        let is_dynamic = (a_mag - 1.0).abs() > 0.15;
        let meas_roll  = if is_dynamic { self.roll }  else { roll_accel };
        let meas_pitch = if is_dynamic { self.pitch } else { pitch_accel };

        // With H = I: S = P + R, K = P / S, state += K * (meas - state), P = (1 - K) * P
        let s_roll  = self.p_roll  + self.r_roll;
        let s_pitch = self.p_pitch + self.r_pitch;

        // S is always positive (sum of variances), so no invertibility check needed
        let k_roll  = self.p_roll  / s_roll;
        let k_pitch = self.p_pitch / s_pitch;

        self.roll  += k_roll  * (meas_roll  - self.roll);
        self.pitch += k_pitch * (meas_pitch - self.pitch);

        self.p_roll  = (1.0 - k_roll)  * self.p_roll;
        self.p_pitch = (1.0 - k_pitch) * self.p_pitch;
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
    let mut ekf = EKF::new((0.001, 0.001), (0.001, 0.001));
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

            log::info!(
                "roll: {:.3} deg, pitch: {:.3} deg",
                ekf.roll  * 180.0 / core::f32::consts::PI,
                ekf.pitch * 180.0 / core::f32::consts::PI,
            );
            ticker.next().await;
        }
    });
}
