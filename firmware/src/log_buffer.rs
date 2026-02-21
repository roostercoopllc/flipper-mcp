/// Circular in-memory log buffer.
///
/// Accumulates up to `MAX_LINES` recent log lines (trimmed to `MAX_LINE_LEN` chars).
/// Flushed to `/ext/apps_data/flipper_mcp/log.txt` on the Flipper SD card by the
/// main loop so the Flipper FAP "View Logs" screen can display diagnostics without
/// requiring a USB serial connection.
use std::sync::Mutex;

use log::warn;

const MAX_LINES: usize = 20;
const MAX_LINE_LEN: usize = 80;

pub const LOG_FILE_PATH: &str = "/ext/apps_data/flipper_mcp/log.txt";

pub struct LogBuffer {
    lines: Mutex<Vec<String>>,
    boot_secs: std::time::Instant,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self {
            lines: Mutex::new(Vec::with_capacity(MAX_LINES)),
            boot_secs: std::time::Instant::now(),
        }
    }

    /// Append a log line, evicting the oldest if the buffer is full.
    pub fn push(&self, msg: &str) {
        let elapsed = self.boot_secs.elapsed().as_secs();
        let h = elapsed / 3600;
        let m = (elapsed % 3600) / 60;
        let s = elapsed % 60;
        let line = format!("[{:02}:{:02}:{:02}] {}", h, m, s, &msg[..msg.len().min(MAX_LINE_LEN)]);

        let mut buf = self.lines.lock().unwrap();
        if buf.len() >= MAX_LINES {
            buf.remove(0);
        }
        buf.push(line);
    }

    /// Return a snapshot of all buffered lines (does not clear).
    pub fn snapshot(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }

    /// Write the buffer contents to log.txt on the Flipper SD card.
    pub fn flush_to_sd(&self, protocol: &mut dyn crate::uart::FlipperProtocol) {
        let content = {
            let buf = self.lines.lock().unwrap();
            buf.join("\n") + "\n"
        };
        if let Err(e) = protocol.write_file(LOG_FILE_PATH, &content) {
            warn!("Log flush failed (non-fatal): {}", e);
        }
    }
}
