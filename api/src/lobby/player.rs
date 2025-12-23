use uuid::Uuid;

use crate::game::state::PlayerId;
use crate::net::session::SessionToken;

/// Player connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerConnectionState {
    /// Connected and active
    Connected,
    /// Temporarily disconnected (can reconnect)
    Disconnected,
    /// Left the game
    Left,
}

/// Lobby player (represents a connected player before/during game)
#[derive(Debug, Clone)]
pub struct LobbyPlayer {
    pub id: PlayerId,
    pub name: String,
    pub session_token: SessionToken,
    pub connection_state: PlayerConnectionState,
    pub room_id: Option<Uuid>,
    pub is_ready: bool,
    pub is_spectator: bool,
    pub ping_ms: u32,
}

impl LobbyPlayer {
    pub fn new(id: PlayerId, name: String, session_token: SessionToken) -> Self {
        Self {
            id,
            name,
            session_token,
            connection_state: PlayerConnectionState::Connected,
            room_id: None,
            is_ready: false,
            is_spectator: false,
            ping_ms: 0,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connection_state == PlayerConnectionState::Connected
    }

    pub fn disconnect(&mut self) {
        self.connection_state = PlayerConnectionState::Disconnected;
    }

    pub fn reconnect(&mut self) {
        self.connection_state = PlayerConnectionState::Connected;
    }

    pub fn leave(&mut self) {
        self.connection_state = PlayerConnectionState::Left;
    }

    pub fn set_ready(&mut self, ready: bool) {
        self.is_ready = ready;
    }

    pub fn update_ping(&mut self, ping_ms: u32) {
        self.ping_ms = ping_ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_new() {
        let id = Uuid::new_v4();
        let token = SessionToken::generate();
        let player = LobbyPlayer::new(id, "Test".to_string(), token);

        assert_eq!(player.id, id);
        assert!(player.is_connected());
        assert!(!player.is_ready);
    }

    #[test]
    fn test_player_disconnect() {
        let mut player = LobbyPlayer::new(
            Uuid::new_v4(),
            "Test".to_string(),
            SessionToken::generate(),
        );

        player.disconnect();
        assert!(!player.is_connected());
        assert_eq!(player.connection_state, PlayerConnectionState::Disconnected);
    }

    #[test]
    fn test_player_reconnect() {
        let mut player = LobbyPlayer::new(
            Uuid::new_v4(),
            "Test".to_string(),
            SessionToken::generate(),
        );

        player.disconnect();
        player.reconnect();
        assert!(player.is_connected());
    }

    #[test]
    fn test_player_ready() {
        let mut player = LobbyPlayer::new(
            Uuid::new_v4(),
            "Test".to_string(),
            SessionToken::generate(),
        );

        player.set_ready(true);
        assert!(player.is_ready);

        player.set_ready(false);
        assert!(!player.is_ready);
    }
}
