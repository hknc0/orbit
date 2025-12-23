use std::net::SocketAddr;
use std::time::Instant;

use crate::game::state::PlayerId;
use crate::net::session::SessionToken;

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Initial connection, not yet authenticated
    Connecting,
    /// Authenticated and active
    Connected,
    /// Gracefully disconnecting
    Disconnecting,
    /// Fully disconnected
    Disconnected,
}

/// Client connection information
#[derive(Debug)]
pub struct Connection {
    pub id: u64,
    pub player_id: Option<PlayerId>,
    pub session_token: Option<SessionToken>,
    pub remote_addr: SocketAddr,
    pub state: ConnectionState,
    pub created_at: Instant,
    pub last_activity: Instant,
    pub ping_ms: u32,
    pub rtt_samples: Vec<u32>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
}

impl Connection {
    pub fn new(id: u64, remote_addr: SocketAddr) -> Self {
        let now = Instant::now();
        Self {
            id,
            player_id: None,
            session_token: None,
            remote_addr,
            state: ConnectionState::Connecting,
            created_at: now,
            last_activity: now,
            ping_ms: 0,
            rtt_samples: Vec::with_capacity(10),
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
        }
    }

    /// Authenticate the connection
    pub fn authenticate(&mut self, player_id: PlayerId, token: SessionToken) {
        self.player_id = Some(player_id);
        self.session_token = Some(token);
        self.state = ConnectionState::Connected;
    }

    /// Update last activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Update RTT with new sample
    pub fn update_rtt(&mut self, rtt_ms: u32) {
        if self.rtt_samples.len() >= 10 {
            self.rtt_samples.remove(0);
        }
        self.rtt_samples.push(rtt_ms);

        // Calculate average
        if !self.rtt_samples.is_empty() {
            let sum: u32 = self.rtt_samples.iter().sum();
            self.ping_ms = sum / self.rtt_samples.len() as u32;
        }
    }

    /// Record bytes sent
    pub fn record_sent(&mut self, bytes: usize) {
        self.bytes_sent += bytes as u64;
        self.packets_sent += 1;
    }

    /// Record bytes received
    pub fn record_received(&mut self, bytes: usize) {
        self.bytes_received += bytes as u64;
        self.packets_received += 1;
        self.touch();
    }

    /// Get time since last activity
    pub fn idle_time(&self) -> std::time::Duration {
        self.last_activity.elapsed()
    }

    /// Check if connection is authenticated
    pub fn is_authenticated(&self) -> bool {
        self.player_id.is_some() && self.state == ConnectionState::Connected
    }

    /// Start disconnection
    pub fn disconnect(&mut self) {
        self.state = ConnectionState::Disconnecting;
    }

    /// Mark as fully disconnected
    pub fn mark_disconnected(&mut self) {
        self.state = ConnectionState::Disconnected;
    }
}

/// Connection manager
pub struct ConnectionManager {
    connections: std::collections::HashMap<u64, Connection>,
    player_connections: std::collections::HashMap<PlayerId, u64>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: std::collections::HashMap::new(),
            player_connections: std::collections::HashMap::new(),
        }
    }

    /// Create a new connection with random ID
    pub fn create(&mut self, remote_addr: SocketAddr) -> u64 {
        // Generate random connection ID (avoid collisions)
        let id = loop {
            let candidate = rand::random::<u64>();
            if !self.connections.contains_key(&candidate) {
                break candidate;
            }
        };

        let conn = Connection::new(id, remote_addr);
        self.connections.insert(id, conn);

        id
    }

    /// Get a connection by ID
    pub fn get(&self, id: u64) -> Option<&Connection> {
        self.connections.get(&id)
    }

    /// Get a mutable connection by ID
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Connection> {
        self.connections.get_mut(&id)
    }

    /// Get connection by player ID
    pub fn get_by_player(&self, player_id: PlayerId) -> Option<&Connection> {
        self.player_connections
            .get(&player_id)
            .and_then(|id| self.connections.get(id))
    }

    /// Get mutable connection by player ID
    pub fn get_by_player_mut(&mut self, player_id: PlayerId) -> Option<&mut Connection> {
        let conn_id = self.player_connections.get(&player_id).copied()?;
        self.connections.get_mut(&conn_id)
    }

    /// Associate player with connection
    pub fn associate_player(&mut self, conn_id: u64, player_id: PlayerId) {
        self.player_connections.insert(player_id, conn_id);
    }

    /// Remove a connection
    pub fn remove(&mut self, id: u64) -> Option<Connection> {
        if let Some(conn) = self.connections.remove(&id) {
            if let Some(player_id) = conn.player_id {
                self.player_connections.remove(&player_id);
            }
            Some(conn)
        } else {
            None
        }
    }

    /// Get connection count
    pub fn count(&self) -> usize {
        self.connections.len()
    }

    /// Get all connection IDs
    pub fn ids(&self) -> Vec<u64> {
        self.connections.keys().copied().collect()
    }

    /// Clean up stale connections
    pub fn cleanup_stale(&mut self, max_idle: std::time::Duration) -> Vec<u64> {
        let stale: Vec<u64> = self
            .connections
            .iter()
            .filter(|(_, conn)| conn.idle_time() > max_idle)
            .map(|(id, _)| *id)
            .collect();

        for id in &stale {
            self.remove(*id);
        }

        stale
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use uuid::Uuid;

    fn test_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)
    }

    #[test]
    fn test_connection_new() {
        let conn = Connection::new(1, test_addr());
        assert_eq!(conn.id, 1);
        assert_eq!(conn.state, ConnectionState::Connecting);
        assert!(conn.player_id.is_none());
    }

    #[test]
    fn test_connection_authenticate() {
        let mut conn = Connection::new(1, test_addr());
        let player_id = Uuid::new_v4();
        let token = SessionToken::generate();

        conn.authenticate(player_id, token);

        assert!(conn.is_authenticated());
        assert_eq!(conn.player_id, Some(player_id));
    }

    #[test]
    fn test_connection_rtt() {
        let mut conn = Connection::new(1, test_addr());

        conn.update_rtt(100);
        conn.update_rtt(110);
        conn.update_rtt(90);

        // Average should be 100
        assert_eq!(conn.ping_ms, 100);
    }

    #[test]
    fn test_connection_stats() {
        let mut conn = Connection::new(1, test_addr());

        conn.record_sent(100);
        conn.record_sent(200);
        conn.record_received(50);

        assert_eq!(conn.bytes_sent, 300);
        assert_eq!(conn.bytes_received, 50);
        assert_eq!(conn.packets_sent, 2);
        assert_eq!(conn.packets_received, 1);
    }

    #[test]
    fn test_manager_create() {
        let mut manager = ConnectionManager::new();

        let id1 = manager.create(test_addr());
        let id2 = manager.create(test_addr());

        assert_ne!(id1, id2);
        assert_eq!(manager.count(), 2);
    }

    #[test]
    fn test_manager_remove() {
        let mut manager = ConnectionManager::new();
        let id = manager.create(test_addr());

        let removed = manager.remove(id);
        assert!(removed.is_some());
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_manager_player_association() {
        let mut manager = ConnectionManager::new();
        let conn_id = manager.create(test_addr());
        let player_id = Uuid::new_v4();

        manager.associate_player(conn_id, player_id);

        assert!(manager.get_by_player(player_id).is_some());
    }

    #[test]
    fn test_manager_cleanup_on_remove() {
        let mut manager = ConnectionManager::new();
        let conn_id = manager.create(test_addr());
        let player_id = Uuid::new_v4();

        if let Some(conn) = manager.get_mut(conn_id) {
            conn.player_id = Some(player_id);
        }
        manager.associate_player(conn_id, player_id);

        manager.remove(conn_id);

        assert!(manager.get_by_player(player_id).is_none());
    }
}
