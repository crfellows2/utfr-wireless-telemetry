use protocol::Command;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio_serial::SerialStream;

pub struct UsbTransport {
    writer: WriteHalf<SerialStream>,
    reader: BufReader<ReadHalf<SerialStream>>,
}

impl UsbTransport {
    pub fn open(path: &str, baud: u32) -> anyhow::Result<Self> {
        let port =
            tokio_serial::SerialPortBuilderExt::open_native_async(tokio_serial::new(path, baud))?;
        let (reader, writer) = tokio::io::split(port);
        Ok(Self {
            writer,
            reader: BufReader::new(reader),
        })
    }

    pub async fn send(&mut self, cmd: &Command) -> anyhow::Result<()> {
        let mut json = serde_json::to_vec(cmd)?;
        json.push(b'\n');
        self.writer.write_all(&json).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> anyhow::Result<Option<String>> {
        let mut line = String::new();
        self.reader.read_line(&mut line).await?;
        let trimmed = line.trim();
        if let Some(json) = trimmed.strip_prefix("j:") {
            println!("{json}");
            Ok(Some(json.to_string()))
        } else {
            println!("[device] {trimmed}");
            Ok(None)
        }
    }

    pub async fn close(mut self) {
        let _ = self.writer.shutdown().await;
    }
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

// #[cfg(test)]
// mod test {
//     use protocol::{CanFilter, StandardId};
//     use tracing::info;
//
//     use crate::init_tracing;
//
//     use super::*;
//
//     #[tokio::test]
//     async fn test_recv() {
//         init_tracing().expect("Could not init tracing for test_recv");
//
//         let device_path = find_esp32().expect("Could not find ble bridge");
//         let mut transport = UsbTransport::open(&device_path, 115200).unwrap();
//
//         let cmd = Command::Subscribe(CanFilter::Standard {
//             id: StandardId::new(0x123).unwrap(),
//             mask: StandardId::EXACT_MASK,
//         });
//
//         info!("Sending {cmd:?}");
//         transport.send(&cmd).await.unwrap();
//         if let Some(resp) = transport.recv().await.unwrap() {
//             info!("Got response: {resp:?}")
//         }
//
//         transport.close().await;
//     }
// }
