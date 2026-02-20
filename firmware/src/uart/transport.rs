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

    pub fn clear_rx(&self) -> Result<()> {
        self.driver.clear_rx().context("Failed to clear UART RX buffer")?;
        Ok(())
    }
}
