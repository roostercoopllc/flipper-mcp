mod proxy;
mod tunnel;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use clap::Parser;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use tunnel::TunnelState;

/// Relay server for cross-network Flipper MCP access.
///
/// The Flipper WiFi Dev Board connects to this relay via WebSocket tunnel.
/// MCP clients (Claude, etc.) send HTTP requests here and the relay proxies
/// them to the connected Flipper, returning responses.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    /// Address + port to listen on
    #[arg(long, default_value = "0.0.0.0:9090")]
    listen: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("flipper_mcp_relay=info".parse()?),
        )
        .init();

    let cli = Cli::parse();
    let state = Arc::new(TunnelState::new());

    let app = Router::new()
        .merge(tunnel::router(state.clone()))
        .merge(proxy::router(state.clone()))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    info!("Flipper MCP Relay listening on {}", cli.listen);
    info!("  Flipper connects to:  ws://{}/tunnel", cli.listen);
    info!("  MCP clients POST to:  http://{}/mcp", cli.listen);
    info!("  Legacy SSE at:        http://{}/sse", cli.listen);
    info!("  Health check:         http://{}/health", cli.listen);

    let listener = tokio::net::TcpListener::bind(cli.listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
