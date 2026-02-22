use anyhow::Result;
use log::{debug, warn};

use super::protocol::FlipperProtocol;
use super::transport::UartTransport;

/// Messages received from the Flipper FAP over UART.
pub enum FapMessage {
    /// Server command: start, stop, restart, reboot, status, refresh_modules
    Cmd(String),
    /// Config update: pipe-delimited key=value pairs
    Config(String),
    /// Keepalive ping
    Ping,
}

/// Protocol for communicating with the Flipper FAP over UART.
///
/// Replaces `CliProtocol` — instead of sending Flipper CLI commands, this uses
/// a simple line-based text protocol where the FAP handles all Flipper-side
/// operations directly via the Flipper SDK.
pub struct FapProtocol {
    transport: UartTransport,
}

impl FapProtocol {
    pub fn new(transport: UartTransport) -> Self {
        Self { transport }
    }

    // ── Push methods (ESP32 → FAP) ──────────────────────────────────────

    /// Send a STATUS message with pipe-delimited key=value pairs.
    pub fn push_status(&self, pairs: &str) {
        let line = format!("STATUS|{}\n", pairs);
        if let Err(e) = self.transport.write_raw(line.as_bytes()) {
            debug!("push_status failed (non-fatal): {}", e);
        }
    }

    /// Send a LOG message.
    pub fn push_log(&self, message: &str) {
        let line = format!("LOG|{}\n", message);
        if let Err(e) = self.transport.write_raw(line.as_bytes()) {
            debug!("push_log failed (non-fatal): {}", e);
        }
    }

    /// Send a TOOLS list (comma-separated names).
    pub fn push_tools(&self, names: &[String]) {
        let line = format!("TOOLS|{}\n", names.join(","));
        if let Err(e) = self.transport.write_raw(line.as_bytes()) {
            debug!("push_tools failed (non-fatal): {}", e);
        }
    }

    /// Send an ACK for a command.
    pub fn push_ack(&self, cmd: &str, result: &str) {
        let line = format!("ACK|cmd={}|result={}\n", cmd, result);
        if let Err(e) = self.transport.write_raw(line.as_bytes()) {
            debug!("push_ack failed (non-fatal): {}", e);
        }
    }

    /// Send a PONG keepalive response.
    pub fn push_pong(&self) {
        if let Err(e) = self.transport.write_raw(b"PONG\n") {
            debug!("push_pong failed (non-fatal): {}", e);
        }
    }

    // ── Poll methods (FAP → ESP32) ──────────────────────────────────────

    /// Drain all pending UART lines and return parsed messages.
    /// Uses a short timeout so it doesn't block the main loop.
    pub fn poll_messages(&self) -> Vec<FapMessage> {
        let mut messages = Vec::new();

        loop {
            match self.transport.read_line(100) {
                Some(line) => {
                    debug!("FAP RX: {}", line);
                    if let Some(msg) = Self::parse_line(&line) {
                        messages.push(msg);
                    }
                }
                None => break,
            }
        }

        messages
    }

    fn parse_line(line: &str) -> Option<FapMessage> {
        if let Some(payload) = line.strip_prefix("CMD|") {
            Some(FapMessage::Cmd(payload.to_string()))
        } else if let Some(payload) = line.strip_prefix("CONFIG|") {
            Some(FapMessage::Config(payload.to_string()))
        } else if line.starts_with("PING") {
            Some(FapMessage::Ping)
        } else {
            warn!("Unknown FAP message: {}", line);
            None
        }
    }
}

impl FlipperProtocol for FapProtocol {
    fn execute_command(&mut self, command: &str) -> Result<String> {
        // Phase 1: Flipper CLI commands cannot be relayed through the FAP yet.
        // Module tool calls will fail gracefully.
        warn!(
            "execute_command unavailable (FAP protocol, no CLI relay): {}",
            command
        );
        anyhow::bail!("Flipper CLI not available — FAP bridge mode (Phase 2)")
    }

    fn execute_command_with_timeout(&mut self, command: &str, _timeout_ms: u32) -> Result<String> {
        self.execute_command(command)
    }

    fn write_file(&mut self, _path: &str, _content: &str) -> Result<()> {
        warn!("write_file unavailable (FAP protocol, no CLI relay)");
        anyhow::bail!("File write not available — FAP bridge mode (Phase 2)")
    }
}
