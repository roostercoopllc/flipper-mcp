use anyhow::Result;

pub trait FlipperProtocol: Send + Sync {
    fn execute_command(&mut self, command: &str) -> Result<String>;

    fn get_device_info(&mut self) -> Result<String>;

    /// Write `content` to a file on the Flipper SD card.
    /// Uses `storage write_chunk` so no interactive input is required.
    /// The file is atomically replaced (remove → mkdir → write_chunk).
    fn write_file(&mut self, path: &str, content: &str) -> Result<()>;
}
