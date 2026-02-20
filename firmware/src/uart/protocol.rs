use anyhow::Result;

pub trait FlipperProtocol: Send {
    fn execute_command(&mut self, command: &str) -> Result<String>;

    fn get_device_info(&mut self) -> Result<String>;

    fn list_apps(&mut self) -> Result<String>;

    fn launch_app(&mut self, name: &str) -> Result<String>;
}
