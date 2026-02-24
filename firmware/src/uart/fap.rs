use std::time::{Duration, Instant};

use anyhow::Result;
use log::{debug, info, warn};

use super::protocol::FlipperProtocol;
use super::transport::UartTransport;

/// Default timeout for CLI relay commands (10 seconds).
const CLI_DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

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
/// Implements `FlipperProtocol` by relaying commands to the FAP, which
/// executes them using native Flipper SDK calls and returns the result.
pub struct FapProtocol {
    transport: UartTransport,
    /// Lines received during execute_command that aren't CLI responses.
    /// Drained first by poll_messages() on the next call.
    pending: Vec<String>,
}

impl FapProtocol {
    pub fn new(transport: UartTransport) -> Self {
        Self {
            transport,
            pending: Vec::new(),
        }
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
    /// First drains messages buffered during execute_command(), then reads UART.
    pub fn poll_messages(&mut self) -> Vec<FapMessage> {
        let mut messages = Vec::new();

        // First drain any messages that arrived during a CLI relay exchange
        for line in self.pending.drain(..) {
            debug!("FAP RX (buffered): {}", line);
            if let Some(msg) = Self::parse_line(&line) {
                messages.push(msg);
            }
        }

        // Then read fresh lines from UART
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

    // ── CLI relay internals ─────────────────────────────────────────────

    /// Send a CLI command and wait for CLI_OK or CLI_ERR response.
    /// Non-CLI messages received during the wait are buffered in `self.pending`.
    fn relay_command(&mut self, command: &str, timeout: Duration) -> Result<String> {
        info!("CLI relay: {}", command);
        self.transport
            .write_raw(format!("CLI|{}\n", command).as_bytes())?;

        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                warn!("CLI relay timeout for: {}", command);
                anyhow::bail!(
                    "CLI relay timeout ({}s) for: {}",
                    timeout.as_secs(),
                    command
                );
            }

            let read_timeout_ms = remaining.as_millis().min(500) as u32;
            match self.transport.read_line(read_timeout_ms) {
                Some(line) => {
                    if let Some(result) = line.strip_prefix("CLI_OK|") {
                        let unescaped = result.replace("\\n", "\n");
                        debug!("CLI relay OK: {} bytes", unescaped.len());
                        return Ok(unescaped);
                    } else if let Some(error) = line.strip_prefix("CLI_ERR|") {
                        let unescaped = error.replace("\\n", "\n");
                        anyhow::bail!("{}", unescaped);
                    } else {
                        // Non-CLI message — buffer for later poll_messages()
                        debug!("CLI relay: buffering non-CLI line: {}", line);
                        self.pending.push(line);
                    }
                }
                None => continue,
            }
        }
    }
}

impl FlipperProtocol for FapProtocol {
    fn execute_command(&mut self, command: &str) -> Result<String> {
        self.relay_command(command, CLI_DEFAULT_TIMEOUT)
    }

    fn execute_command_with_timeout(&mut self, command: &str, timeout_ms: u32) -> Result<String> {
        self.relay_command(command, Duration::from_millis(timeout_ms as u64))
    }

    fn write_file(&mut self, path: &str, content: &str) -> Result<()> {
        let escaped = content.replace('\n', "\\n");
        info!("WRITE_FILE relay: {}", path);
        self.transport
            .write_raw(format!("WRITE_FILE|{}|{}\n", path, escaped).as_bytes())?;

        let deadline = Instant::now() + CLI_DEFAULT_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                anyhow::bail!("WRITE_FILE relay timeout for: {}", path);
            }

            let read_timeout_ms = remaining.as_millis().min(500) as u32;
            match self.transport.read_line(read_timeout_ms) {
                Some(line) => {
                    if line.starts_with("CLI_OK|") {
                        return Ok(());
                    } else if let Some(error) = line.strip_prefix("CLI_ERR|") {
                        anyhow::bail!("WRITE_FILE failed: {}", error.replace("\\n", "\n"));
                    } else {
                        self.pending.push(line);
                    }
                }
                None => continue,
            }
        }
    }
}
