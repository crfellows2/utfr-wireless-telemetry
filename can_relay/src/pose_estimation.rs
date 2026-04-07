use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Ticker};

use crate::can_interface::CanFrameForSd;

pub static ACCEL_SIGNAL: Signal<CriticalSectionRawMutex, CanFrameForSd> = Signal::new();
pub static GYRO_SIGNAL: Signal<CriticalSectionRawMutex, CanFrameForSd> = Signal::new();

pub fn start_pose_estimation_thread() -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("pose_estimation".to_string())
        // .stack_size(16384)
        .spawn(sd_logger_thread_main)?;

    Ok(())
}

fn sd_logger_thread_main() {
    let mut ticker = Ticker::every(Duration::from_hz(10));

    esp_idf_svc::hal::task::block_on(async {
        loop {
            // Note: these are send based on can IDs 0x100 and 0x101 in can_interface/mod.rs . I
            // might have got the ids the wrong way around. Flip them in there if you see gyro
            // values on accel or just ask robert what the ids are
            let accel = ACCEL_SIGNAL.wait().await;
            let gyro = GYRO_SIGNAL.wait().await;
            println!("accel: {accel:?}");
            println!("gyro: {gyro:?}");

            // limit the loop to 10Hz
            ticker.next().await;
        }
    });
}
