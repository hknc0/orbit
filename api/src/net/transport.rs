//! WebTransport server implementation
//!
//! This module provides the WebTransport server using wtransport.
//! Integrates with GameSession for real-time multiplayer gameplay.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::anticheat::sanctions::BanList;
use crate::config::ServerConfig;
use crate::game::state::PlayerId;
use crate::lobby::manager::LobbyManager;
use crate::metrics::Metrics;
use crate::net::dos_protection::DoSProtection;
use crate::net::game_session::{start_game_loop, send_to_player, GameSession};
use crate::net::protocol::{decode, encode, ClientMessage, ServerMessage};
use crate::net::tls::TlsConfig;

/// WebTransport server
pub struct WebTransportServer {
    config: ServerConfig,
    tls_config: TlsConfig,
    lobby_manager: Arc<RwLock<LobbyManager>>,
    ban_list: Arc<RwLock<BanList>>,
    dos_protection: Arc<RwLock<DoSProtection>>,
    game_session: Arc<RwLock<GameSession>>,
    metrics: Arc<Metrics>,
}

impl WebTransportServer {
    /// Create a new WebTransport server
    pub async fn new(
        config: ServerConfig,
        lobby_manager: Arc<RwLock<LobbyManager>>,
        ban_list: Arc<RwLock<BanList>>,
        metrics: Arc<Metrics>,
    ) -> anyhow::Result<Self> {
        let tls_config = TlsConfig::generate_self_signed().await?;
        let dos_protection = Arc::new(RwLock::new(DoSProtection::default()));
        let game_session = Arc::new(RwLock::new(GameSession::new_with_metrics(metrics.clone())));

        Ok(Self {
            config,
            tls_config,
            lobby_manager,
            ban_list,
            dos_protection,
            game_session,
            metrics,
        })
    }

    /// Get the certificate hash for client configuration
    pub fn cert_hash(&self) -> &str {
        self.tls_config.get_cert_hash()
    }

    /// Get the bind address
    pub fn bind_addr(&self) -> SocketAddr {
        SocketAddr::new(self.config.bind_address, self.config.port)
    }

    /// Run the server
    pub async fn run(self) -> anyhow::Result<()> {
        use wtransport::Endpoint;
        use wtransport::ServerConfig;

        // Use with_bind_default for dual-stack (IPv4 + IPv6) support
        // This allows both localhost (::1 or 127.0.0.1) and LAN connections
        let server_config = ServerConfig::builder()
            .with_bind_default(self.config.port)
            .with_identity(self.tls_config.identity)
            .build();

        let server = Endpoint::server(server_config)?;

        tracing::info!(
            "WebTransport server listening on port {}",
            self.config.port
        );
        tracing::info!("Certificate hash: {}", self.tls_config.cert_hash);

        // Start the game loop background task
        start_game_loop(self.game_session.clone());

        // Accept connections
        loop {
            let incoming = server.accept().await;

            let lobby = self.lobby_manager.clone();
            let bans = self.ban_list.clone();
            let dos = self.dos_protection.clone();
            let game_session = self.game_session.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_connection(incoming, lobby, bans, dos, game_session).await {
                    tracing::warn!("Connection error: {}", e);
                }
            });
        }
    }
}

/// Handle a single WebTransport connection
async fn handle_connection(
    incoming: wtransport::endpoint::IncomingSession,
    _lobby_manager: Arc<RwLock<LobbyManager>>,
    _ban_list: Arc<RwLock<BanList>>,
    _dos_protection: Arc<RwLock<DoSProtection>>,
    game_session: Arc<RwLock<GameSession>>,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let session_request = incoming.await?;

    tracing::info!(
        "New connection from: {:?}, path: {}",
        session_request.authority(),
        session_request.path()
    );

    let connection = session_request.accept().await?;

    tracing::info!("Connection accepted");

    // Track this connection's player ID (set after JoinRequest)
    let player_id: Arc<RwLock<Option<PlayerId>>> = Arc::new(RwLock::new(None));

    // Main connection loop
    loop {
        let player_id_clone = player_id.clone();
        let game_session_clone = game_session.clone();

        tokio::select! {
            // Handle bidirectional streams
            stream = connection.accept_bi() => {
                match stream {
                    Ok((send, mut recv)) => {
                        tracing::info!("Accepted bidirectional stream");

                        // Wrap send stream in Arc<RwLock> for sharing
                        let writer = Arc::new(RwLock::new(Some(send)));

                        let player_id = player_id_clone.clone();
                        let game_session = game_session_clone.clone();

                        // Spawn task to handle this stream
                        tokio::spawn(async move {
                            const MAX_MESSAGE_SIZE: usize = 65536; // 64KB max
                            let mut buffer = vec![0u8; 4096];

                            loop {
                                // Read length-prefixed message
                                let mut len_buf = [0u8; 4];
                                match recv.read_exact(&mut len_buf).await {
                                    Ok(_) => {}
                                    Err(e) => {
                                        tracing::debug!("Stream read error: {}", e);
                                        break;
                                    }
                                }

                                let msg_len = u32::from_le_bytes(len_buf) as usize;

                                // Security: Reject oversized messages
                                if msg_len > MAX_MESSAGE_SIZE {
                                    tracing::warn!("Rejected oversized message: {} bytes", msg_len);
                                    break;
                                }

                                if msg_len > buffer.len() {
                                    buffer.resize(msg_len, 0);
                                }

                                match recv.read_exact(&mut buffer[..msg_len]).await {
                                    Ok(_) => {}
                                    Err(e) => {
                                        tracing::debug!("Stream read error: {}", e);
                                        break;
                                    }
                                }

                                // Decode the client message
                                let client_msg: ClientMessage = match decode(&buffer[..msg_len]) {
                                    Ok(msg) => msg,
                                    Err(e) => {
                                        tracing::warn!("Failed to decode client message: {}", e);
                                        continue;
                                    }
                                };

                                match client_msg {
                                    ClientMessage::JoinRequest { player_name } => {
                                        tracing::info!("Received JoinRequest from '{}'", player_name);

                                        // Check if server can accept new players (performance-based)
                                        let can_accept = {
                                            let session = game_session.read().await;
                                            session.can_accept_player()
                                        };

                                        if !can_accept {
                                            // Reject due to performance/capacity
                                            let rejection_msg = {
                                                let session = game_session.read().await;
                                                session.rejection_message()
                                            };
                                            tracing::warn!("Rejecting player '{}': {}", player_name, rejection_msg);

                                            let response_msg = ServerMessage::JoinRejected {
                                                reason: rejection_msg,
                                            };
                                            if let Err(e) = send_to_player(&writer, &response_msg).await {
                                                tracing::warn!("Failed to send JoinRejected: {}", e);
                                            }
                                            continue;
                                        }

                                        // Generate player ID
                                        let new_player_id = uuid::Uuid::new_v4();

                                        // Add player to game session
                                        {
                                            let mut session = game_session.write().await;
                                            session.add_player(
                                                new_player_id,
                                                player_name.clone(),
                                                writer.clone(),
                                            );
                                        }

                                        // Store player ID for this connection
                                        *player_id.write().await = Some(new_player_id);

                                        // Send JoinAccepted
                                        let session_token = vec![0u8; 32]; // TODO: generate real token
                                        let response_msg = ServerMessage::JoinAccepted {
                                            player_id: new_player_id,
                                            session_token,
                                        };

                                        if let Err(e) = send_to_player(&writer, &response_msg).await {
                                            tracing::warn!("Failed to send JoinAccepted: {}", e);
                                            break;
                                        }
                                        tracing::info!("Sent JoinAccepted (player_id: {})", new_player_id);

                                        // Send initial snapshot
                                        let snapshot = {
                                            let session = game_session.read().await;
                                            session.get_snapshot()
                                        };
                                        let snapshot_msg = ServerMessage::Snapshot(snapshot);
                                        if let Err(e) = send_to_player(&writer, &snapshot_msg).await {
                                            tracing::warn!("Failed to send initial snapshot: {}", e);
                                        } else {
                                            tracing::info!("Sent initial snapshot to player {}", new_player_id);
                                        }

                                        // Send PhaseChange to let client know game is playing
                                        let phase_msg = ServerMessage::PhaseChange {
                                            phase: crate::game::state::MatchPhase::Playing,
                                            countdown: 0.0,
                                        };
                                        if let Err(e) = send_to_player(&writer, &phase_msg).await {
                                            tracing::warn!("Failed to send PhaseChange: {}", e);
                                        }
                                    }

                                    ClientMessage::Input(input) => {
                                        // Queue input for this player
                                        if let Some(pid) = *player_id.read().await {
                                            let mut session = game_session.write().await;
                                            session.queue_input(pid, input);
                                        }
                                    }

                                    ClientMessage::Leave => {
                                        tracing::info!("Player requested to leave");
                                        if let Some(pid) = *player_id.read().await {
                                            let mut session = game_session.write().await;
                                            session.remove_player(pid);
                                        }
                                        break;
                                    }

                                    ClientMessage::Ping { timestamp } => {
                                        let response_msg = ServerMessage::Pong {
                                            client_timestamp: timestamp,
                                            server_timestamp: std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_millis() as u64,
                                        };

                                        if let Err(e) = send_to_player(&writer, &response_msg).await {
                                            tracing::debug!("Failed to send Pong: {}", e);
                                        }
                                    }

                                    ClientMessage::SnapshotAck { tick: _ } => {
                                        // Acknowledge received, could be used for delta compression
                                    }
                                }
                            }

                            // Clean up: remove player from session when stream closes
                            if let Some(pid) = *player_id.read().await {
                                tracing::info!("Player {} stream closed, removing from game", pid);
                                let mut session = game_session.write().await;
                                session.remove_player(pid);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::debug!("Stream accept error: {}", e);
                        break;
                    }
                }
            }

            // Handle datagrams (unreliable) - used for high-frequency input
            datagram = connection.receive_datagram() => {
                match datagram {
                    Ok(data) => {
                        // Try to decode as PlayerInput
                        match decode::<ClientMessage>(&data) {
                            Ok(ClientMessage::Input(input)) => {
                                if let Some(pid) = *player_id_clone.read().await {
                                    let mut session = game_session_clone.write().await;
                                    session.queue_input(pid, input);
                                }
                            }
                            Ok(_) => {}
                            Err(e) => {
                                tracing::debug!("Failed to decode datagram: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!("Datagram receive error: {}", e);
                        break;
                    }
                }
            }
        }
    }

    // Clean up on disconnect
    if let Some(pid) = *player_id.read().await {
        tracing::info!("Connection closed, removing player {}", pid);
        let mut session = game_session.write().await;
        session.remove_player(pid);
    }

    tracing::info!("Connection closed");
    Ok(())
}

#[cfg(test)]
mod tests {
    // WebTransport tests require a more complex setup with actual
    // network connections, so we test the simpler components here.

    use super::*;
    use crate::config::ServerConfig;

    #[tokio::test]
    async fn test_server_creation() {
        let config = ServerConfig::default();
        let lobby = Arc::new(RwLock::new(LobbyManager::new(10)));
        let bans = Arc::new(RwLock::new(BanList::new()));
        let metrics = Arc::new(Metrics::new());

        let result = WebTransportServer::new(config, lobby, bans, metrics).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_server_cert_hash() {
        let config = ServerConfig::default();
        let lobby = Arc::new(RwLock::new(LobbyManager::new(10)));
        let bans = Arc::new(RwLock::new(BanList::new()));
        let metrics = Arc::new(Metrics::new());

        let server = WebTransportServer::new(config, lobby, bans, metrics).await.unwrap();
        let hash = server.cert_hash();

        assert!(!hash.is_empty());
    }
}
