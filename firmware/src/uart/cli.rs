use anyhow::Result;
use log::info;

use super::protocol::FlipperProtocol;
use super::transport::UartTransport;

/// Default UART read timeout. 2 s gives a comfortable margin for most Flipper CLI
/// commands (which typically respond in < 100 ms) while tolerating occasional SD-card
/// or Flipper task scheduling delays without false timeouts.
const DEFAULT_TIMEOUT_MS: u32 = 2000;

pub struct CliProtocol {
    transport: UartTransport,
}

impl CliProtocol {
    pub fn new(transport: UartTransport) -> Self {
        Self { transport }
    }

    fn send_and_receive(&mut self, command: &str, timeout_ms: u32) -> Result<String> {
        self.transport.clear_rx()?;
        self.transport.send(command)?;
        let response = self.transport.read_response(timeout_ms)?;
        // Strip the echoed command from the beginning of the response
        let response = strip_echo(&response, command);
        Ok(response)
    }
}

impl FlipperProtocol for CliProtocol {
    fn execute_command(&mut self, command: &str) -> Result<String> {
        info!("Executing CLI command: {}", command);
        self.send_and_receive(command, DEFAULT_TIMEOUT_MS)
    }

    fn execute_command_with_timeout(&mut self, command: &str, timeout_ms: u32) -> Result<String> {
        info!("Executing CLI command ({}ms): {}", timeout_ms, command);
        self.send_and_receive(command, timeout_ms)
    }

    fn get_device_info(&mut self) -> Result<String> {
        self.execute_command("device_info")
    }

    fn write_file(&mut self, path: &str, content: &str) -> Result<()> {
        // Best-effort: remove existing file so write_chunk starts fresh
        let _ = self.execute_command(&format!("storage remove {}", path));

        // Ensure the parent directory exists
        if let Some(slash) = path.rfind('/') {
            let dir = &path[..slash];
            let _ = self.execute_command(&format!("storage mkdir {}", dir));
        }

        // Flush any stale RX bytes before the two-phase write
        self.transport.clear_rx()?;

        // Phase 1: send "storage write_chunk <path> <len>\r\n"
        let cmd = format!("storage write_chunk {} {}\r\n", path, content.len());
        self.transport.write_raw(cmd.as_bytes())?;

        // Phase 2: immediately send the raw content bytes (no trailing \r\n)
        self.transport.write_raw(content.as_bytes())?;

        // Read the response (Flipper echoes back ">: " after writing)
        let _ = self.transport.read_response(DEFAULT_TIMEOUT_MS);

        Ok(())
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
