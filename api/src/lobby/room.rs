use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

use crate::game::game_loop::{GameLoop, GameLoopConfig, GameLoopEvent};
use crate::game::state::{Player, PlayerId};
use crate::lobby::player::LobbyPlayer;
use crate::net::protocol::{GameSnapshot, PlayerInput};

/// Room state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomState {
    /// Waiting for players
    Waiting,
    /// Game in progress
    Playing,
    /// Game ended, showing results
    Ended,
    /// Room is closing
    Closing,
}

/// Game room containing players and game state
pub struct GameRoom {
    pub id: Uuid,
    pub name: String,
    pub state: RoomState,
    pub max_players: usize,
    pub max_humans: usize,
    pub created_at: Instant,
    players: HashMap<PlayerId, LobbyPlayer>,
    game_loop: GameLoop,
    fill_with_bots: bool,
}

impl GameRoom {
    pub fn new(name: String, max_players: usize, max_humans: usize) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            state: RoomState::Waiting,
            max_players,
            max_humans,
            created_at: Instant::now(),
            players: HashMap::new(),
            game_loop: GameLoop::new(GameLoopConfig::default()),
            fill_with_bots: true,
        }
    }

    /// Get room ID
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Get player count
    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    /// Get human player count
    pub fn human_count(&self) -> usize {
        self.players
            .values()
            .filter(|p| p.is_connected() && !p.is_spectator)
            .count()
    }

    /// Check if room is full
    pub fn is_full(&self) -> bool {
        self.human_count() >= self.max_humans
    }

    /// Check if room is empty
    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    /// Get game state
    pub fn game_state(&self) -> &crate::game::state::GameState {
        self.game_loop.state()
    }

    /// Add a player to the room
    pub fn add_player(&mut self, lobby_player: LobbyPlayer) -> Result<(), RoomError> {
        if self.is_full() {
            return Err(RoomError::RoomFull);
        }

        if self.state != RoomState::Waiting {
            return Err(RoomError::GameInProgress);
        }

        let player_id = lobby_player.id;

        // Create game player
        let game_player = Player::new(player_id, lobby_player.name.clone(), false, self.players.len() as u8);

        self.game_loop.add_player(game_player);
        self.players.insert(player_id, lobby_player);

        Ok(())
    }

    /// Remove a player from the room
    pub fn remove_player(&mut self, player_id: PlayerId) -> Option<LobbyPlayer> {
        if let Some(mut player) = self.players.remove(&player_id) {
            player.leave();
            self.game_loop.remove_player(player_id);
            Some(player)
        } else {
            None
        }
    }

    /// Get a player by ID
    pub fn get_player(&self, player_id: PlayerId) -> Option<&LobbyPlayer> {
        self.players.get(&player_id)
    }

    /// Get a mutable player by ID
    pub fn get_player_mut(&mut self, player_id: PlayerId) -> Option<&mut LobbyPlayer> {
        self.players.get_mut(&player_id)
    }

    /// Process player input
    pub fn process_input(&mut self, player_id: PlayerId, input: PlayerInput) {
        self.game_loop.queue_input(player_id, input);
    }

    /// Start the game
    pub fn start_game(&mut self) -> Result<(), RoomError> {
        if self.state != RoomState::Waiting {
            return Err(RoomError::GameInProgress);
        }

        if self.players.is_empty() {
            return Err(RoomError::NotEnoughPlayers);
        }

        // Fill with bots if needed
        if self.fill_with_bots {
            self.game_loop.fill_with_bots(self.max_players);
        }

        self.state = RoomState::Playing;
        Ok(())
    }

    /// Update game state (called each frame/tick)
    pub fn update(&mut self) -> Vec<GameLoopEvent> {
        if self.state != RoomState::Playing {
            return Vec::new();
        }

        let events = self.game_loop.update();

        // Check for game end
        for event in &events {
            if matches!(event, GameLoopEvent::MatchEnded { .. }) {
                self.state = RoomState::Ended;
            }
        }

        events
    }

    /// Run a single tick (for testing or manual control)
    pub fn tick(&mut self) -> Vec<GameLoopEvent> {
        self.game_loop.tick()
    }

    /// Get current game snapshot
    pub fn get_snapshot(&self) -> GameSnapshot {
        GameSnapshot::from_game_state(self.game_loop.state())
    }

    /// Reset the room for a new game
    pub fn reset(&mut self) {
        self.game_loop.reset();
        self.state = RoomState::Waiting;

        // Re-add existing players to new game
        for player in self.players.values() {
            if player.is_connected() && !player.is_spectator {
                let game_player =
                    Player::new(player.id, player.name.clone(), false, 0);
                self.game_loop.add_player(game_player);
            }
        }
    }

    /// Get all player IDs
    pub fn player_ids(&self) -> Vec<PlayerId> {
        self.players.keys().copied().collect()
    }

    /// Get connected player IDs
    pub fn connected_player_ids(&self) -> Vec<PlayerId> {
        self.players
            .iter()
            .filter(|(_, p)| p.is_connected())
            .map(|(id, _)| *id)
            .collect()
    }

    /// Check if all players are ready
    pub fn all_players_ready(&self) -> bool {
        !self.players.is_empty() && self.players.values().all(|p| p.is_ready || !p.is_connected())
    }

    /// Get room age
    pub fn age(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }
}

/// Room errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum RoomError {
    #[error("Room is full")]
    RoomFull,
    #[error("Game already in progress")]
    GameInProgress,
    #[error("Not enough players")]
    NotEnoughPlayers,
    #[error("Player not found")]
    PlayerNotFound,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::session::SessionToken;

    fn create_lobby_player(name: &str) -> LobbyPlayer {
        LobbyPlayer::new(Uuid::new_v4(), name.to_string(), SessionToken::generate())
    }

    #[test]
    fn test_room_new() {
        let room = GameRoom::new("Test Room".to_string(), 10, 10);
        assert_eq!(room.state, RoomState::Waiting);
        assert!(room.is_empty());
    }

    #[test]
    fn test_add_player() {
        let mut room = GameRoom::new("Test Room".to_string(), 10, 10);
        let player = create_lobby_player("Player1");
        let id = player.id;

        room.add_player(player).unwrap();

        assert_eq!(room.player_count(), 1);
        assert!(room.get_player(id).is_some());
    }

    #[test]
    fn test_remove_player() {
        let mut room = GameRoom::new("Test Room".to_string(), 10, 10);
        let player = create_lobby_player("Player1");
        let id = player.id;

        room.add_player(player).unwrap();
        let removed = room.remove_player(id);

        assert!(removed.is_some());
        assert!(room.is_empty());
    }

    #[test]
    fn test_room_full() {
        let mut room = GameRoom::new("Test Room".to_string(), 10, 2);

        room.add_player(create_lobby_player("P1")).unwrap();
        room.add_player(create_lobby_player("P2")).unwrap();

        let result = room.add_player(create_lobby_player("P3"));
        assert!(matches!(result, Err(RoomError::RoomFull)));
    }

    #[test]
    fn test_start_game() {
        let mut room = GameRoom::new("Test Room".to_string(), 10, 10);
        room.add_player(create_lobby_player("P1")).unwrap();

        room.start_game().unwrap();

        assert_eq!(room.state, RoomState::Playing);
    }

    #[test]
    fn test_cannot_join_started_game() {
        let mut room = GameRoom::new("Test Room".to_string(), 10, 10);
        room.add_player(create_lobby_player("P1")).unwrap();
        room.start_game().unwrap();

        let result = room.add_player(create_lobby_player("P2"));
        assert!(matches!(result, Err(RoomError::GameInProgress)));
    }

    #[test]
    fn test_get_snapshot() {
        let mut room = GameRoom::new("Test Room".to_string(), 10, 10);
        room.add_player(create_lobby_player("P1")).unwrap();

        let snapshot = room.get_snapshot();
        assert_eq!(snapshot.players.len(), 1);
    }

    #[test]
    fn test_reset() {
        let mut room = GameRoom::new("Test Room".to_string(), 10, 10);
        room.add_player(create_lobby_player("P1")).unwrap();
        room.start_game().unwrap();

        room.reset();

        assert_eq!(room.state, RoomState::Waiting);
    }

    #[test]
    fn test_fill_with_bots() {
        let mut room = GameRoom::new("Test Room".to_string(), 5, 5);
        room.fill_with_bots = true;
        room.add_player(create_lobby_player("Human")).unwrap();
        room.start_game().unwrap();

        // Should have filled with bots
        assert_eq!(room.game_state().players.len(), 5);
    }
}
