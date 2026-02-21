use anyhow::{Context, Result};
use esp_idf_svc::hal::gpio;
use esp_idf_svc::hal::peripheral::Peripheral;
use esp_idf_svc::hal::uart::{self, UartDriver};
use esp_idf_svc::hal::units::Hertz;
use log::debug;

const RX_BUF_SIZE: usize = 1024;
const PROMPT: &[u8] = b">: ";

pub struct UartTransport {
    driver: UartDriver<'static>,
}

impl UartTransport {
    pub fn new(
        uart: impl Peripheral<P = impl uart::Uart> + 'static,
        tx: impl Peripheral<P = impl gpio::OutputPin> + 'static,
        rx: impl Peripheral<P = impl gpio::InputPin> + 'static,
        baud_rate: u32,
    ) -> Result<Self> {
        let config = uart::config::Config::default().baudrate(Hertz(baud_rate));

        let driver = UartDriver::new(
            uart,
            tx,
            rx,
            Option::<gpio::AnyIOPin>::None,
            Option::<gpio::AnyIOPin>::None,
            &config,
        )
        .context("Failed to initialize UART driver")?;

        Ok(Self { driver })
    }

    pub fn send(&self, command: &str) -> Result<()> {
        debug!("UART TX: {}", command);

        let data = format!("{}\r\n", command);
        self.driver
            .write(data.as_bytes())
            .context("UART write failed")?;
        self.driver
            .wait_tx_done(100)
            .context("UART TX flush timeout")?;

        Ok(())
    }

    pub fn read_response(&self, timeout_ms: u32) -> Result<String> {
        let mut response = Vec::with_capacity(RX_BUF_SIZE);
        let mut buf = [0u8; 256];

        loop {
            match self.driver.read(&mut buf, timeout_ms) {
                Ok(0) => break,
                Ok(n) => {
                    response.extend_from_slice(&buf[..n]);
                    if response.len() >= PROMPT.len()
                        && response[response.len() - PROMPT.len()..] == *PROMPT
                    {
                        response.truncate(response.len() - PROMPT.len());
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        let text = String::from_utf8_lossy(&response).to_string();
        debug!("UART RX ({} bytes): {}", response.len(), text);
        Ok(text)
    }

    /// Send raw bytes without appending \r\n — used for binary payloads (e.g. write_chunk data).
    pub fn write_raw(&self, data: &[u8]) -> Result<()> {
        self.driver.write(data).context("UART raw write failed")?;
        self.driver.wait_tx_done(100).context("UART TX flush timeout")?;
        Ok(())
    }

    pub fn clear_rx(&self) -> Result<()> {
        self.driver.clear_rx().context("Failed to clear UART RX buffer")?;
        Ok(())
    }

    /// Read a single `\n`-terminated line from UART.
    /// Returns `None` if nothing received within `timeout_ms`.
    /// Strips `\r` and limits line length to 1024 bytes.
    pub fn read_line(&self, timeout_ms: u32) -> Option<String> {
        let mut line = Vec::with_capacity(256);
        let mut buf = [0u8; 1];

        loop {
            match self.driver.read(&mut buf, timeout_ms) {
                Ok(1) => match buf[0] {
                    b'\n' => break,
                    b'\r' => continue,
                    b => {
                        line.push(b);
                        if line.len() >= 1024 {
                            break;
                        }
                    }
                },
                _ => {
                    if line.is_empty() {
                        return None;
                    }
                    break;
                }
            }
        }

        if line.is_empty() {
            return None;
        }

        Some(String::from_utf8_lossy(&line).to_string())
    }
}
