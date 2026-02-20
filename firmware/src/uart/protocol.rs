use anyhow::Result;

pub trait FlipperProtocol: Send + Sync {
    fn execute_command(&mut self, command: &str) -> Result<String>;

    fn get_device_info(&mut self) -> Result<String>;

}
