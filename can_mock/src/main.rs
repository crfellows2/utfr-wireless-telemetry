use embassy_futures::select::select;
use embassy_time::{Duration, Ticker};
use enumset::enum_set;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::{
    can::{config, AsyncCanDriver, Flags, Frame, CAN},
    gpio::{InputPin, OutputPin},
    task::block_on,
};
use log::info;

fn main() {
    // It is necessary to call this function once. Otherwise, some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Hello, world!");

    let peripherals = Peripherals::take().unwrap();
    let pins = peripherals.pins;

    // let can1_tx = pins.gpio9;
    // let can1_rx = pins.gpio14;
    // let _can2_tx = pins.gpio19;
    // let _can2_rx = pins.gpio18;
    // let can_peripheral = peripherals.can;
    // block_on(can_task(can_peripheral, can1_tx, can1_rx));

    block_on(can_task(peripherals.can, pins.gpio18, pins.gpio19));
}

pub async fn can_task(can_peripheral: CAN<'_>, tx_pin: impl OutputPin, rx_pin: impl InputPin) {
    info!("Can task started");

    let timing = config::Timing::Custom {
        baudrate_prescaler: 2,
        timing_segment_1: 15,
        timing_segment_2: 4,
        synchronization_jump_width: 3,
        triple_sampling: false,
    };
    let config = config::Config::new().timing(timing);
    let mut can = AsyncCanDriver::new(can_peripheral, tx_pin, rx_pin, &config).unwrap();
    can.start().unwrap();

    loop {
        select(
            async {
                loop {
                    select(
                        async {
                            match can.receive().await {
                                Ok(frame) => info!("CAN RX: {frame}"),
                                Err(e) => log::error!("Can RX Error: {e:?}"),
                            }
                        },
                        async {
                            match can.read_alerts().await {
                                Ok(alerts) => info!("Alerts: {alerts:?}"),
                                Err(e) => info!("Alerts Err: {e:?}"),
                            }
                        },
                    )
                    .await;
                }
            },
            async {
                let mut ticker = Ticker::every(Duration::from_millis(10));
                let mut count = 0u32;

                loop {
                    let frame =
                        Frame::new(0x55, enum_set!(Flags::SingleShot), &count.to_be_bytes())
                            .unwrap();

                    info!("Send: {}", frame);
                    match can.transmit(&frame).await {
                        Ok(()) => info!("Frame Sent"),
                        Err(e) => log::error!("Can TX Error: {e:?}"),
                    }
                    count += 1;
                    ticker.next().await;
                }
            },
        )
        .await;
    }
}
