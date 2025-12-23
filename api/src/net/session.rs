use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::game::state::PlayerId;

/// Session token for authenticated connections
/// Uses CSPRNG for cryptographic security
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionToken([u8; 32]);

impl SessionToken {
    /// Generate a new cryptographically secure session token
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Create from bytes (for deserialization)
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Convert to Vec<u8> for network transmission
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    /// Try to create from a slice
    pub fn try_from_slice(slice: &[u8]) -> Option<Self> {
        if slice.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(slice);
        Some(Self(bytes))
    }
}

impl Default for SessionToken {
    fn default() -> Self {
        Self::generate()
    }
}

/// Session data for an active player connection
#[derive(Debug, Clone)]
pub struct Session {
    pub player_id: PlayerId,
    pub token: SessionToken,
    pub created_at: Instant,
    pub last_activity: Instant,
    pub room_id: Option<Uuid>,
    pub player_name: String,
}

impl Session {
    pub fn new(player_id: PlayerId, player_name: String) -> Self {
        let now = Instant::now();
        Self {
            player_id,
            token: SessionToken::generate(),
            created_at: now,
            last_activity: now,
            room_id: None,
            player_name,
        }
    }

    /// Update last activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Check if session has timed out
    pub fn is_expired(&self, timeout: Duration) -> bool {
        self.last_activity.elapsed() > timeout
    }

    /// Get session age
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Get time since last activity
    pub fn idle_time(&self) -> Duration {
        self.last_activity.elapsed()
    }
}

/// Session manager for tracking active player sessions
pub struct SessionManager {
    /// Sessions indexed by player ID
    sessions: HashMap<PlayerId, Session>,
    /// Token to player ID mapping for O(1) token lookup
    token_index: HashMap<SessionToken, PlayerId>,
    /// Session timeout duration
    timeout: Duration,
    /// Maximum number of sessions
    max_sessions: usize,
}

impl SessionManager {
    pub fn new(timeout: Duration, max_sessions: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            token_index: HashMap::new(),
            timeout,
            max_sessions,
        }
    }

    /// Create a new session for a player
    pub fn create_session(&mut self, player_id: PlayerId, player_name: String) -> Option<&Session> {
        // Check if at capacity
        if self.sessions.len() >= self.max_sessions {
            // Try to clean up expired sessions first
            self.cleanup_expired();
            if self.sessions.len() >= self.max_sessions {
                return None;
            }
        }

        // Remove any existing session for this player
        self.remove_session(player_id);

        let session = Session::new(player_id, player_name);
        self.token_index.insert(session.token.clone(), player_id);
        self.sessions.insert(player_id, session);
        self.sessions.get(&player_id)
    }

    /// Get session by player ID
    pub fn get_session(&self, player_id: PlayerId) -> Option<&Session> {
        self.sessions.get(&player_id)
    }

    /// Get mutable session by player ID
    pub fn get_session_mut(&mut self, player_id: PlayerId) -> Option<&mut Session> {
        self.sessions.get_mut(&player_id)
    }

    /// Validate a token and return the associated player ID
    pub fn validate_token(&self, token: &SessionToken) -> Option<PlayerId> {
        self.token_index.get(token).copied()
    }

    /// Validate token from bytes
    pub fn validate_token_bytes(&self, bytes: &[u8]) -> Option<PlayerId> {
        SessionToken::try_from_slice(bytes).and_then(|token| self.validate_token(&token))
    }

    /// Remove a session
    pub fn remove_session(&mut self, player_id: PlayerId) -> Option<Session> {
        if let Some(session) = self.sessions.remove(&player_id) {
            self.token_index.remove(&session.token);
            Some(session)
        } else {
            None
        }
    }

    /// Touch a session (update last activity)
    pub fn touch_session(&mut self, player_id: PlayerId) -> bool {
        if let Some(session) = self.sessions.get_mut(&player_id) {
            session.touch();
            true
        } else {
            false
        }
    }

    /// Clean up expired sessions
    pub fn cleanup_expired(&mut self) -> usize {
        let expired: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.is_expired(self.timeout))
            .map(|(id, _)| *id)
            .collect();

        let count = expired.len();
        for player_id in expired {
            self.remove_session(player_id);
        }
        count
    }

    /// Get number of active sessions
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Check if a player has an active session
    pub fn has_session(&self, player_id: PlayerId) -> bool {
        self.sessions.contains_key(&player_id)
    }

    /// Get all sessions in a room
    pub fn sessions_in_room(&self, room_id: Uuid) -> Vec<&Session> {
        self.sessions
            .values()
            .filter(|s| s.room_id == Some(room_id))
            .collect()
    }

    /// Set room for a session
    pub fn set_room(&mut self, player_id: PlayerId, room_id: Option<Uuid>) -> bool {
        if let Some(session) = self.sessions.get_mut(&player_id) {
            session.room_id = room_id;
            true
        } else {
            false
        }
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new(Duration::from_secs(300), 10000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_token_generate() {
        let t1 = SessionToken::generate();
        let t2 = SessionToken::generate();
        assert_ne!(t1, t2);
    }

    #[test]
    fn test_session_token_from_bytes() {
        let original = SessionToken::generate();
        let bytes = *original.as_bytes();
        let restored = SessionToken::from_bytes(bytes);
        assert_eq!(original, restored);
    }

    #[test]
    fn test_session_token_try_from_slice() {
        let original = SessionToken::generate();
        let vec = original.to_vec();
        let restored = SessionToken::try_from_slice(&vec);
        assert_eq!(Some(original), restored);

        // Wrong size
        assert!(SessionToken::try_from_slice(&[1, 2, 3]).is_none());
    }

    #[test]
    fn test_session_new() {
        let player_id = Uuid::new_v4();
        let session = Session::new(player_id, "TestPlayer".to_string());
        assert_eq!(session.player_id, player_id);
        assert_eq!(session.player_name, "TestPlayer");
        assert!(session.room_id.is_none());
    }

    #[test]
    fn test_session_touch() {
        let mut session = Session::new(Uuid::new_v4(), "Test".to_string());
        let old_activity = session.last_activity;
        std::thread::sleep(std::time::Duration::from_millis(10));
        session.touch();
        assert!(session.last_activity > old_activity);
    }

    #[test]
    fn test_session_expired() {
        let session = Session::new(Uuid::new_v4(), "Test".to_string());
        assert!(!session.is_expired(Duration::from_secs(60)));

        // Would need to wait or mock time for actual expiry test
    }

    #[test]
    fn test_session_manager_create() {
        let mut manager = SessionManager::new(Duration::from_secs(60), 100);
        let player_id = Uuid::new_v4();

        let session = manager.create_session(player_id, "Test".to_string());
        assert!(session.is_some());
        assert_eq!(manager.session_count(), 1);
    }

    #[test]
    fn test_session_manager_validate_token() {
        let mut manager = SessionManager::new(Duration::from_secs(60), 100);
        let player_id = Uuid::new_v4();

        let session = manager.create_session(player_id, "Test".to_string()).unwrap();
        let token = session.token.clone();

        let validated = manager.validate_token(&token);
        assert_eq!(validated, Some(player_id));
    }

    #[test]
    fn test_session_manager_validate_token_bytes() {
        let mut manager = SessionManager::new(Duration::from_secs(60), 100);
        let player_id = Uuid::new_v4();

        let session = manager.create_session(player_id, "Test".to_string()).unwrap();
        let bytes = session.token.to_vec();

        let validated = manager.validate_token_bytes(&bytes);
        assert_eq!(validated, Some(player_id));
    }

    #[test]
    fn test_session_manager_remove() {
        let mut manager = SessionManager::new(Duration::from_secs(60), 100);
        let player_id = Uuid::new_v4();

        let session = manager.create_session(player_id, "Test".to_string()).unwrap();
        let token = session.token.clone();

        let removed = manager.remove_session(player_id);
        assert!(removed.is_some());
        assert_eq!(manager.session_count(), 0);
        assert!(manager.validate_token(&token).is_none());
    }

    #[test]
    fn test_session_manager_max_sessions() {
        let mut manager = SessionManager::new(Duration::from_secs(60), 2);

        manager.create_session(Uuid::new_v4(), "P1".to_string());
        manager.create_session(Uuid::new_v4(), "P2".to_string());

        // Third should fail (at capacity)
        let result = manager.create_session(Uuid::new_v4(), "P3".to_string());
        assert!(result.is_none());
    }

    #[test]
    fn test_session_manager_duplicate_player() {
        let mut manager = SessionManager::new(Duration::from_secs(60), 100);
        let player_id = Uuid::new_v4();

        let s1 = manager.create_session(player_id, "First".to_string()).unwrap();
        let token1 = s1.token.clone();

        let s2 = manager.create_session(player_id, "Second".to_string()).unwrap();
        let token2 = s2.token.clone();

        // Old token should be invalid
        assert!(manager.validate_token(&token1).is_none());
        // New token should work
        assert_eq!(manager.validate_token(&token2), Some(player_id));
        // Still only one session
        assert_eq!(manager.session_count(), 1);
    }

    #[test]
    fn test_session_manager_touch() {
        let mut manager = SessionManager::new(Duration::from_secs(60), 100);
        let player_id = Uuid::new_v4();

        manager.create_session(player_id, "Test".to_string());

        assert!(manager.touch_session(player_id));
        assert!(!manager.touch_session(Uuid::new_v4())); // Non-existent
    }

    #[test]
    fn test_session_manager_room() {
        let mut manager = SessionManager::new(Duration::from_secs(60), 100);
        let player_id = Uuid::new_v4();
        let room_id = Uuid::new_v4();

        manager.create_session(player_id, "Test".to_string());
        manager.set_room(player_id, Some(room_id));

        let sessions = manager.sessions_in_room(room_id);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].player_id, player_id);
    }

    #[test]
    fn test_session_manager_has_session() {
        let mut manager = SessionManager::new(Duration::from_secs(60), 100);
        let player_id = Uuid::new_v4();

        assert!(!manager.has_session(player_id));
        manager.create_session(player_id, "Test".to_string());
        assert!(manager.has_session(player_id));
    }
}
