use std::sync::Arc;

use anyhow::Result;
use esp_idf_svc::http::server::EspHttpServer;
use log::info;

use crate::mcp::server::McpServer;

use super::streamable::start_http_server;

pub struct HttpServerManager {
    server: Option<EspHttpServer<'static>>,
    mcp_server: Arc<McpServer>,
}

impl HttpServerManager {
    pub fn new(mcp_server: Arc<McpServer>) -> Self {
        Self {
            server: None,
            mcp_server,
        }
    }

    pub fn start(&mut self) -> Result<()> {
        if self.server.is_some() {
            info!("HTTP server already running");
            return Ok(());
        }
        self.server = Some(start_http_server(self.mcp_server.clone())?);
        info!("HTTP server started");
        Ok(())
    }

    pub fn stop(&mut self) {
        if self.server.take().is_some() {
            info!("HTTP server stopped");
        } else {
            info!("HTTP server was not running");
        }
    }

    pub fn restart(&mut self) -> Result<()> {
        self.stop();
        self.start()
    }

    pub fn is_running(&self) -> bool {
        self.server.is_some()
    }
}
