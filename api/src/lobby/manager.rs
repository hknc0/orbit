use std::collections::HashMap;
use uuid::Uuid;

use crate::game::state::PlayerId;
use crate::lobby::player::LobbyPlayer;
use crate::lobby::room::{GameRoom, RoomError, RoomState};

/// Lobby manager for managing game rooms
pub struct LobbyManager {
    rooms: HashMap<Uuid, GameRoom>,
    player_rooms: HashMap<PlayerId, Uuid>,
    max_rooms: usize,
    default_room_size: usize,
    default_max_humans: usize,
}

impl LobbyManager {
    pub fn new(max_rooms: usize) -> Self {
        Self {
            rooms: HashMap::new(),
            player_rooms: HashMap::new(),
            max_rooms,
            default_room_size: 10,
            default_max_humans: 10,
        }
    }

    /// Create a new room
    pub fn create_room(&mut self, name: String) -> Result<Uuid, ManagerError> {
        if self.rooms.len() >= self.max_rooms {
            return Err(ManagerError::TooManyRooms);
        }

        let room = GameRoom::new(name, self.default_room_size, self.default_max_humans);
        let id = room.id();
        self.rooms.insert(id, room);

        Ok(id)
    }

    /// Get or create a room for quick play
    pub fn find_or_create_room(&mut self) -> Result<Uuid, ManagerError> {
        // Find a waiting room with space
        for (id, room) in &self.rooms {
            if room.state == RoomState::Waiting && !room.is_full() {
                return Ok(*id);
            }
        }

        // Create a new room
        self.create_room(format!("Game {}", self.rooms.len() + 1))
    }

    /// Get a room by ID
    pub fn get_room(&self, room_id: Uuid) -> Option<&GameRoom> {
        self.rooms.get(&room_id)
    }

    /// Get a mutable room by ID
    pub fn get_room_mut(&mut self, room_id: Uuid) -> Option<&mut GameRoom> {
        self.rooms.get_mut(&room_id)
    }

    /// Remove a room
    pub fn remove_room(&mut self, room_id: Uuid) -> Option<GameRoom> {
        if let Some(room) = self.rooms.remove(&room_id) {
            // Remove player mappings
            for player_id in room.player_ids() {
                self.player_rooms.remove(&player_id);
            }
            Some(room)
        } else {
            None
        }
    }

    /// Join a player to a room
    pub fn join_room(
        &mut self,
        room_id: Uuid,
        player: LobbyPlayer,
    ) -> Result<(), ManagerError> {
        let player_id = player.id;

        // Check if player is already in a room
        if self.player_rooms.contains_key(&player_id) {
            return Err(ManagerError::AlreadyInRoom);
        }

        let room = self
            .rooms
            .get_mut(&room_id)
            .ok_or(ManagerError::RoomNotFound)?;

        room.add_player(player).map_err(ManagerError::RoomError)?;
        self.player_rooms.insert(player_id, room_id);

        Ok(())
    }

    /// Leave current room
    pub fn leave_room(&mut self, player_id: PlayerId) -> Result<(), ManagerError> {
        let room_id = self
            .player_rooms
            .remove(&player_id)
            .ok_or(ManagerError::NotInRoom)?;

        if let Some(room) = self.rooms.get_mut(&room_id) {
            room.remove_player(player_id);

            // Clean up empty rooms
            if room.is_empty() && room.state != RoomState::Playing {
                self.rooms.remove(&room_id);
            }
        }

        Ok(())
    }

    /// Get player's current room
    pub fn get_player_room(&self, player_id: PlayerId) -> Option<Uuid> {
        self.player_rooms.get(&player_id).copied()
    }

    /// Get room count
    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    /// Get total player count across all rooms
    pub fn total_player_count(&self) -> usize {
        self.player_rooms.len()
    }

    /// Get list of available rooms (for room browser)
    pub fn list_rooms(&self) -> Vec<RoomInfo> {
        self.rooms
            .values()
            .map(|room| RoomInfo {
                id: room.id(),
                name: room.name.clone(),
                player_count: room.player_count(),
                max_players: room.max_players,
                state: room.state,
            })
            .collect()
    }

    /// Update all rooms
    pub fn update_all(&mut self) {
        for room in self.rooms.values_mut() {
            room.update();
        }

        // Clean up ended rooms that are empty
        let rooms_to_remove: Vec<Uuid> = self
            .rooms
            .iter()
            .filter(|(_, room)| {
                room.state == RoomState::Ended && room.is_empty()
                    || room.state == RoomState::Closing
            })
            .map(|(id, _)| *id)
            .collect();

        for room_id in rooms_to_remove {
            self.remove_room(room_id);
        }
    }

    /// Shutdown all rooms
    pub async fn shutdown_all_rooms(&mut self) {
        for (_, room) in self.rooms.iter_mut() {
            // Could send shutdown messages to players here
            let _ = room; // Suppress warning
        }
        self.rooms.clear();
        self.player_rooms.clear();
    }
}

impl Default for LobbyManager {
    fn default() -> Self {
        Self::new(100)
    }
}

/// Room information for listing
#[derive(Debug, Clone)]
pub struct RoomInfo {
    pub id: Uuid,
    pub name: String,
    pub player_count: usize,
    pub max_players: usize,
    pub state: RoomState,
}

/// Manager errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum ManagerError {
    #[error("Too many rooms")]
    TooManyRooms,
    #[error("Room not found")]
    RoomNotFound,
    #[error("Already in a room")]
    AlreadyInRoom,
    #[error("Not in a room")]
    NotInRoom,
    #[error("Room error: {0}")]
    RoomError(#[from] RoomError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::session::SessionToken;

    fn create_player(name: &str) -> LobbyPlayer {
        LobbyPlayer::new(Uuid::new_v4(), name.to_string(), SessionToken::generate())
    }

    #[test]
    fn test_create_room() {
        let mut manager = LobbyManager::new(10);

        let room_id = manager.create_room("Test Room".to_string()).unwrap();

        assert!(manager.get_room(room_id).is_some());
        assert_eq!(manager.room_count(), 1);
    }

    #[test]
    fn test_max_rooms() {
        let mut manager = LobbyManager::new(2);

        manager.create_room("Room 1".to_string()).unwrap();
        manager.create_room("Room 2".to_string()).unwrap();

        let result = manager.create_room("Room 3".to_string());
        assert!(matches!(result, Err(ManagerError::TooManyRooms)));
    }

    #[test]
    fn test_join_room() {
        let mut manager = LobbyManager::new(10);
        let room_id = manager.create_room("Test".to_string()).unwrap();
        let player = create_player("Player1");
        let player_id = player.id;

        manager.join_room(room_id, player).unwrap();

        assert_eq!(manager.get_player_room(player_id), Some(room_id));
    }

    #[test]
    fn test_leave_room() {
        let mut manager = LobbyManager::new(10);
        let room_id = manager.create_room("Test".to_string()).unwrap();
        let player = create_player("Player1");
        let player_id = player.id;

        manager.join_room(room_id, player).unwrap();
        manager.leave_room(player_id).unwrap();

        assert!(manager.get_player_room(player_id).is_none());
    }

    #[test]
    fn test_cannot_join_twice() {
        let mut manager = LobbyManager::new(10);
        let room_id = manager.create_room("Test".to_string()).unwrap();
        let player1 = create_player("Player1");
        let player2 = LobbyPlayer::new(player1.id, "Player1".to_string(), SessionToken::generate());

        manager.join_room(room_id, player1).unwrap();
        let result = manager.join_room(room_id, player2);

        assert!(matches!(result, Err(ManagerError::AlreadyInRoom)));
    }

    #[test]
    fn test_find_or_create_room() {
        let mut manager = LobbyManager::new(10);

        // Should create first room
        let room_id1 = manager.find_or_create_room().unwrap();
        assert_eq!(manager.room_count(), 1);

        // Should return same room
        let room_id2 = manager.find_or_create_room().unwrap();
        assert_eq!(room_id1, room_id2);
    }

    #[test]
    fn test_list_rooms() {
        let mut manager = LobbyManager::new(10);
        manager.create_room("Room A".to_string()).unwrap();
        manager.create_room("Room B".to_string()).unwrap();

        let rooms = manager.list_rooms();
        assert_eq!(rooms.len(), 2);
    }

    #[test]
    fn test_empty_room_cleanup() {
        let mut manager = LobbyManager::new(10);
        let room_id = manager.create_room("Test".to_string()).unwrap();
        let player = create_player("P1");
        let player_id = player.id;

        manager.join_room(room_id, player).unwrap();
        manager.leave_room(player_id).unwrap();

        // Room should be removed
        assert!(manager.get_room(room_id).is_none());
    }

    #[test]
    fn test_total_player_count() {
        let mut manager = LobbyManager::new(10);
        let room_id = manager.create_room("Test".to_string()).unwrap();

        manager.join_room(room_id, create_player("P1")).unwrap();
        manager.join_room(room_id, create_player("P2")).unwrap();

        assert_eq!(manager.total_player_count(), 2);
    }
}
