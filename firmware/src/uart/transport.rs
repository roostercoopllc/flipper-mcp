use anyhow::{Context, Result};
use esp_idf_svc::hal::gpio;
use esp_idf_svc::hal::peripheral::Peripheral;
use esp_idf_svc::hal::uart::{self, UartDriver};
use esp_idf_svc::hal::units::Hertz;

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

    /// Send raw bytes — used for all protocol messages.
    pub fn write_raw(&self, data: &[u8]) -> Result<()> {
        self.driver.write(data).context("UART raw write failed")?;
        self.driver
            .wait_tx_done(100)
            .context("UART TX flush timeout")?;
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
