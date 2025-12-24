mod config;
mod game;
mod metrics;
mod net;
mod util;

#[cfg(feature = "anticheat")]
mod anticheat;
#[cfg(feature = "lobby")]
mod lobby;

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, Level};

use crate::config::ServerConfig;
use crate::metrics::Metrics;
use crate::net::transport::WebTransportServer;

#[cfg(feature = "anticheat")]
use crate::anticheat::sanctions::BanList;
#[cfg(feature = "lobby")]
use crate::lobby::manager::LobbyManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present
    dotenvy::dotenv().ok();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .init();

    info!(
        "Orbit Royale Server v{}",
        env!("CARGO_PKG_VERSION")
    );

    // Load configuration
    let config = ServerConfig::load_or_default();

    // Validate configuration
    if let Err(e) = config.validate() {
        error!("Invalid configuration: {}", e);
        return Err(anyhow::anyhow!("Configuration validation failed: {}", e));
    }

    info!(
        "Configuration loaded: {}:{}, max_rooms={}",
        config.bind_address, config.port, config.max_rooms
    );

    // Initialize metrics
    let metrics = Arc::new(Metrics::new());

    // Start metrics server on port 9090 (configurable via METRICS_PORT)
    let metrics_port: u16 = std::env::var("METRICS_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9090);

    let metrics_clone = metrics.clone();
    tokio::spawn(async move {
        if let Err(e) = metrics::start_metrics_server(metrics_clone, metrics_port).await {
            error!("Metrics server error: {}", e);
        }
    });

    // Initialize shared state (feature-gated)
    #[cfg(feature = "lobby")]
    let lobby_manager = Arc::new(RwLock::new(LobbyManager::new(config.max_rooms)));
    #[cfg(not(feature = "lobby"))]
    let lobby_manager = Arc::new(RwLock::new(()));

    #[cfg(feature = "anticheat")]
    let ban_list = Arc::new(RwLock::new(BanList::new()));
    #[cfg(not(feature = "anticheat"))]
    let ban_list = Arc::new(RwLock::new(()));

    // Create WebTransport server
    let server = WebTransportServer::new(
        config.clone(),
        lobby_manager.clone(),
        ban_list.clone(),
        metrics.clone(),
    )
    .await?;

    info!(
        "Server ready on https://{}:{}",
        config.bind_address, config.port
    );
    info!("Certificate hash: {}", server.cert_hash());
    info!(
        "Chrome flag: --ignore-certificate-errors-spki-list={}",
        server.cert_hash()
    );

    // Shutdown signal handler
    let shutdown = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
        info!("Shutdown signal received");
    };

    // Run server with graceful shutdown
    tokio::select! {
        result = server.run() => {
            if let Err(e) = result {
                error!("Server error: {}", e);
            }
        }
        _ = shutdown => {
            info!("Shutting down...");
        }
    }

    // Cleanup
    #[cfg(feature = "lobby")]
    lobby_manager.write().await.shutdown_all_rooms().await;
    info!("Server stopped");

    Ok(())
}
