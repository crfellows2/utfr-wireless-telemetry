use std::sync::mpsc;
use std::thread;

use futures::channel::oneshot;
use protocol::Command;
use serialport::SerialPort;
use std::io::{BufRead, BufReader};
use std::time::Duration;
use tracing::warn;

pub enum WriteError {
    NotConnected,
    Serialization(serde_json::Error),
    Io(std::io::Error),
}

enum UsbCommand {
    Write {
        command: Command,
        result: oneshot::Sender<Result<(), WriteError>>,
    },
    SetLogHook(Box<dyn Fn(&str) + Send>),
    Connect {
        path: String,
        baud: u32,
        result: oneshot::Sender<Result<(), serialport::Error>>,
    },
}

pub struct CanRecvManager {
    cmd_tx: mpsc::Sender<UsbCommand>,
    handle: Option<thread::JoinHandle<()>>,
}

struct State {
    port: Option<BufReader<Box<dyn SerialPort>>>,
    log_hook: Box<dyn Fn(&str) + Send>,
}

impl State {
    pub fn new() -> Self {
        let log_hook = Box::new(|log_line: &str| {
            println!("[device] {log_line}");
        });

        Self {
            port: None,
            log_hook,
        }
    }
}

enum Event {
    CanMessage(String),
    SerialPortError(std::io::Error),
}

#[allow(unused)]
fn can_recv_task(cmd_rx: mpsc::Receiver<UsbCommand>, tx: mpsc::Sender<Event>) {
    let mut state = State::new();

    loop {
        let cmd = match cmd_rx.try_recv() {
            Ok(cmd) => Some(cmd),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                warn!("can_recv_task command channel dropped. Disconnecting...");
                todo!()
            }
        };

        if let Some(cmd) = cmd {
            match cmd {
                UsbCommand::Connect { path, baud, result } => {
                    if let Some(prev) = state.port.take() {
                        if let Err(e) = disconnect_device(prev) {
                            tracing::error!("Could not disconnect from previous device: {e:?}");
                        } else {
                            tracing::info!("Disconnected from previous device");
                        }
                    }

                    match serialport::new(path, baud)
                        .timeout(Duration::from_millis(1000))
                        .open()
                    {
                        Ok(p) => {
                            let reader = BufReader::new(p);
                            state.port = Some(reader);
                            let _ = result.send(Ok(()));
                        }
                        Err(e) => {
                            let _ = result.send(Err(e));
                        }
                    }
                }
                UsbCommand::SetLogHook(callback) => {
                    state.log_hook = callback;
                }
                UsbCommand::Write { command, result } => {
                    let r = match state.port.as_mut() {
                        Some(port) => write_command(port, &command),
                        None => Err(WriteError::NotConnected),
                    };
                    let _ = result.send(r);
                }
                _ => todo!(),
            }
        }

        if let Some(ref mut reader) = state.port {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(_) => {
                    let trimmed = line.trim();
                    if let Some(json) = trimmed.strip_prefix("j:") {
                        tx.send(Event::CanMessage(json.to_string()))
                            .expect("can_recv_task event channel dropped. Disconnecting...");
                    } else {
                        (state.log_hook)(trimmed);
                    }
                }
                Err(e) => {
                    let _ = tx.send(Event::SerialPortError(e));
                    state.port = None;
                }
            }
        }
    }
}

fn write_command(
    port: &mut BufReader<Box<dyn SerialPort>>,
    command: &Command,
) -> Result<(), WriteError> {
    let mut json = serde_json::to_vec(command).map_err(WriteError::Serialization)?;
    json.push(b'\n');
    port.get_mut().write_all(&json).map_err(WriteError::Io)?;
    Ok(())
}

pub fn disconnect_device(port: BufReader<Box<dyn SerialPort>>) -> Result<(), serialport::Error> {
    todo!()
}

pub fn find_esp32() -> anyhow::Result<String> {
    let ports = serialport::available_ports()?;
    ports
        .into_iter()
        .find(|port| {
            matches!(
                &port.port_type,
                serialport::SerialPortType::UsbPort(usb_info)
                if usb_info.vid == 0x303a && usb_info.pid == 0x1001
            )
        })
        .map(|port| port.port_name)
        .ok_or_else(|| anyhow::anyhow!("ESP32 not found"))
}

#[cfg(test)]
mod test {
    use super::*;

    // #[test]
    // fn test_recv() {
    //     let device_path = find_esp32().expect("Could not find ble bridge");
    //     let mut device = CanReceiver::new(&device_path, 115200);
    //
    //     device.set_log_callback(|logline| {
    //         println!("[device] {logline}");
    //     });
    //
    //     device.connect().expect("Could not connect to usb device");
    // }
}
