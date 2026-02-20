use anyhow::Result;
use log::info;

use super::protocol::FlipperProtocol;
use super::transport::UartTransport;

const DEFAULT_TIMEOUT_MS: u32 = 500;

pub struct CliProtocol {
    transport: UartTransport,
}

impl CliProtocol {
    pub fn new(transport: UartTransport) -> Self {
        Self { transport }
    }

    fn send_and_receive(&mut self, command: &str) -> Result<String> {
        self.transport.clear_rx()?;
        self.transport.send(command)?;
        let response = self.transport.read_response(DEFAULT_TIMEOUT_MS)?;
        // Strip the echoed command from the beginning of the response
        let response = strip_echo(&response, command);
        Ok(response)
    }
}

impl FlipperProtocol for CliProtocol {
    fn execute_command(&mut self, command: &str) -> Result<String> {
        info!("Executing CLI command: {}", command);
        self.send_and_receive(command)
    }

    fn get_device_info(&mut self) -> Result<String> {
        self.execute_command("device_info")
    }

}

fn strip_echo(response: &str, command: &str) -> String {
    let trimmed = response.trim_start();
    if trimmed.starts_with(command) {
        trimmed[command.len()..].trim_start_matches('\r').trim_start_matches('\n').to_string()
    } else {
        response.to_string()
    }
}
