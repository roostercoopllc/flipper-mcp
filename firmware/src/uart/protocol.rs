use anyhow::Result;

pub trait FlipperProtocol: Send + Sync {
    fn execute_command(&mut self, command: &str) -> Result<String>;

    /// Like `execute_command` but with a caller-specified UART read timeout.
    /// Use for commands that take longer than the default 2 s (e.g. subghz rx, nfc detect).
    fn execute_command_with_timeout(&mut self, command: &str, timeout_ms: u32) -> Result<String> {
        let _ = timeout_ms; // default: ignore the hint and use execute_command
        self.execute_command(command)
    }

    /// Write `content` to a file on the Flipper SD card.
    fn write_file(&mut self, path: &str, content: &str) -> Result<()>;
}
