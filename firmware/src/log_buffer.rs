/// Circular in-memory log buffer.
///
/// Accumulates up to `MAX_LINES` recent log lines (trimmed to `MAX_LINE_LEN` chars).
/// Pushed to the Flipper FAP over UART so the "View Logs" screen can display
/// diagnostics without requiring a USB serial connection.
use std::sync::Mutex;

const MAX_LINES: usize = 20;
const MAX_LINE_LEN: usize = 80;

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
}
