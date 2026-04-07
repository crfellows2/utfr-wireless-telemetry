use protocol::Command;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio_serial::SerialPortBuilderExt;

// --- types ---

pub enum WriteError {
    NotConnected,
    Serialization(serde_json::Error),
    Io(std::io::Error),
}

enum UsbCommand {
    Connect {
        path: String,
        baud: u32,
        reply: oneshot::Sender<Result<(), tokio_serial::Error>>,
    },
    Write {
        command: Command,
        reply: oneshot::Sender<Result<(), WriteError>>,
    },
    SetLogHook(Box<dyn Fn(&str) + Send>),
    Disconnect,
}

pub enum Event {
    CanMessage(String),
    SerialPortError(std::io::Error),
    Disconnected,
}

type Port = BufReader<tokio_serial::SerialStream>;

// --- public struct ---

pub struct CanRecvManager {
    cmd_tx: mpsc::Sender<UsbCommand>,
    handle: tokio::task::JoinHandle<()>,
}

impl CanRecvManager {
    pub fn new() -> (Self, mpsc::Receiver<Event>) {
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (event_tx, event_rx) = mpsc::channel(32);
        let handle = tokio::spawn(can_recv_task(cmd_rx, event_tx));
        (Self { cmd_tx, handle }, event_rx)
    }

    pub async fn connect(&self, path: &str, baud: u32) -> Result<(), tokio_serial::Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .cmd_tx
            .send(UsbCommand::Connect {
                path: path.to_string(),
                baud,
                reply: tx,
            })
            .await;
        rx.await.unwrap()
    }

    pub async fn write(&self, command: Command) -> Result<(), WriteError> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .cmd_tx
            .send(UsbCommand::Write { command, reply: tx })
            .await;
        rx.await.unwrap()
    }

    pub async fn disconnect(&self) {
        let _ = self.cmd_tx.send(UsbCommand::Disconnect).await;
    }

    pub async fn disconnect(&self) {
        let _ = self.cmd_tx.send(UsbCommand::Disconnect).await;
    }
}

impl Drop for CanRecvManager {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

// --- task ---

async fn can_recv_task(mut cmd_rx: mpsc::Receiver<UsbCommand>, event_tx: mpsc::Sender<Event>) {
    let mut port: Option<Port> = None;
    let mut log_hook: Box<dyn Fn(&str) + Send> = Box::new(|line: &str| println!("[device] {line}"));

    loop {
        if port.is_none() {
            // idle: no spinning, just block until a command arrives
            match cmd_rx.recv().await {
                Some(cmd) => handle_command(cmd, &mut port, &mut log_hook, &event_tx).await,
                None => return,
            }
        } else {
            // connected: select! across commands and serial data simultaneously
            // take/replace is needed because select! can't hold a &mut port
            // borrow in one arm while the other arm also needs &mut port
            let mut taken = port.take().unwrap();
            let mut line = String::new();

            tokio::select! {
                cmd = cmd_rx.recv() => {
                    port = Some(taken);
                    match cmd {
                        Some(cmd) => handle_command(cmd, &mut port, &mut log_hook, &event_tx).await,
                        None => return,
                    }
                }
                result = taken.read_line(&mut line) => {
                    match result {
                        Ok(0) => {
                            let _ = event_tx.send(Event::Disconnected).await;
                            // port stays None
                        }
                        Ok(_) => {
                            port = Some(taken);
                            let trimmed = line.trim();
                            if let Some(json) = trimmed.strip_prefix("j:") {
                                let _ = event_tx.send(Event::CanMessage(json.to_string())).await;
                            } else {
                                log_hook(trimmed);
                            }
                        }
                        Err(e) => {
                            let _ = event_tx.send(Event::SerialPortError(e)).await;
                            // port stays None
                        }
                    }
                }
            }
        }
    }
}

async fn handle_command(
    cmd: UsbCommand,
    port: &mut Option<Port>,
    log_hook: &mut Box<dyn Fn(&str) + Send>,
    event_tx: &mpsc::Sender<Event>,
) {
    match cmd {
        UsbCommand::Connect { path, baud, reply } => {
            *port = None; // drop existing connection first
            match tokio_serial::new(&path, baud).open_native_async() {
                Ok(p) => {
                    *port = Some(BufReader::new(p));
                    let _ = reply.send(Ok(()));
                }
                Err(e) => {
                    let _ = reply.send(Err(e));
                }
            }
        }
        UsbCommand::Write { command, reply } => {
            let r = match port.as_mut() {
                Some(p) => write_command(p, &command).await,
                None => Err(WriteError::NotConnected),
            };
            let _ = reply.send(r);
        }
        UsbCommand::SetLogHook(hook) => *log_hook = hook,
        UsbCommand::Disconnect => {
            *port = None;
            let _ = event_tx.send(Event::Disconnected).await;
        }
    }
}

async fn write_command(port: &mut Port, command: &Command) -> Result<(), WriteError> {
    let mut json = serde_json::to_vec(command).map_err(WriteError::Serialization)?;
    json.push(b'\n');
    port.get_mut()
        .write_all(&json)
        .await
        .map_err(WriteError::Io)?;
    Ok(())
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

    #[tokio::test]
    async fn test_recv() {
        let device_path = find_esp32().expect("Could not find ble bridge");
        let (manager, mut events) = CanRecvManager::new();

        manager
            .set_log_hook(Box::new(|logline: &str| {
                println!("[device] {logline}");
            }))
            .await;

        manager
            .connect(&device_path, 115200)
            .await
            .expect("Could not connect to usb device");

        while let Some(event) = events.recv().await {
            match event {
                Event::CanMessage(json) => println!("[can] {json}"),
                Event::SerialPortError(e) => panic!("serial error: {e}"),
                Event::Disconnected => break,
            }
        }
    }
}
