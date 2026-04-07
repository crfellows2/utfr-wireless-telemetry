use embassy_sync::channel::Sender;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::usb_serial::UsbSerialDriver;
use esp_idf_svc::sys::EspError;
use protocol::Command;

pub fn setup_usb_serial() -> Result<UsbSerialDriver<'static>, EspError> {
    log::info!("Setting up USB Serial driver...");

    let peripherals = Peripherals::take()?;

    let usb_serial = UsbSerialDriver::new(
        peripherals.usb_serial,
        peripherals.pins.gpio12, // USB_D-
        peripherals.pins.gpio13, // USB_D+
        &Default::default(),
    )?;

    log::info!("USB Serial driver created");

    Ok(usb_serial)
}

pub fn spawn_stdin_reader(
    mut usb_serial: UsbSerialDriver<'static>,
    tx: Sender<'static, embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, Command, 32>,
) {
    log::info!("Spawning stdin reader thread...");

    std::thread::Builder::new()
        .name("stdin_reader".to_string())
        .stack_size(8192) // Increase stack size for JSON parsing
        .spawn(move || {
            log::info!("stdin_reader thread started, waiting for commands...");

            let mut line_buffer = String::new();
            let mut read_buf = [0u8; 256];

            loop {
                // Blocking read from USB Serial
                match usb_serial.read(&mut read_buf, esp_idf_svc::hal::delay::BLOCK) {
                    Ok(0) => {
                        // No data, shouldn't happen with BLOCK timeout
                        continue;
                    }
                    Ok(n) => {
                        // Process received bytes
                        let received = match std::str::from_utf8(&read_buf[..n]) {
                            Ok(s) => s,
                            Err(e) => {
                                log::warn!("Invalid UTF-8: {}", e);
                                continue;
                            }
                        };

                        // Accumulate into line buffer
                        for ch in received.chars() {
                            if ch == '\n' {
                                // Process complete line
                                let trimmed = line_buffer.trim();
                                if !trimmed.is_empty() {
                                    log::info!("Received line: {}", trimmed);
                                    match serde_json::from_str::<Command>(trimmed) {
                                        Ok(cmd) => {
                                            log::info!("Parsed command successfully, sending to channel...");
                                            if let Err(e) = tx.try_send(cmd) {
                                                log::error!("Command channel full, dropping command: {:?}", e);
                                            } else {
                                                log::info!("Command sent to channel successfully");
                                            }
                                        }
                                        Err(e) => {
                                            log::warn!("Failed to parse JSON command: {} (input: {})", e, trimmed);
                                        }
                                    }
                                }
                                line_buffer.clear();
                            } else {
                                line_buffer.push(ch);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to read from USB Serial: {}", e);
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
        })
        .expect("Failed to spawn stdin reader thread");

    log::info!("stdin reader thread spawned successfully");
}
