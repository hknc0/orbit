//! Rate limiting for anti-cheat
//!
//! Tracks input and action rates to detect automation and spam.

#![allow(dead_code)] // Rate limiter ready for future integration

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::game::state::PlayerId;

/// Violations from rate limiting
#[derive(Debug, Clone, thiserror::Error)]
pub enum RateLimitViolation {
    #[error("Input rate exceeded: {0} inputs this second")]
    InputRateExceeded(u32),
    #[error("Fire rate exceeded: cooldown has {0} ticks remaining")]
    FireRateExceeded(u64),
    #[error("Boost spam detected: {0} consecutive boost ticks")]
    BoostSpam(u32),
    #[error("Too many inputs in buffer")]
    InputBufferOverflow,
}

/// Rate limits for a single player
#[derive(Debug)]
pub struct PlayerRateLimits {
    /// Last tick an input was received
    pub last_input_tick: u64,
    /// Number of inputs received this second
    pub inputs_this_second: u32,
    /// When the current second started
    pub second_start: Instant,
    /// Last tick the player fired
    pub last_fire_tick: u64,
    /// Fire cooldown in ticks (can vary based on charge)
    pub fire_cooldown_ticks: u64,
    /// Consecutive boost ticks
    pub consecutive_boost_ticks: u32,
    /// Maximum consecutive boost allowed
    pub max_consecutive_boost: u32,
    /// Buffered inputs waiting to be processed
    pub input_buffer_size: usize,
    /// Maximum input buffer size
    pub max_input_buffer: usize,
}

impl PlayerRateLimits {
    pub fn new() -> Self {
        Self {
            last_input_tick: 0,
            inputs_this_second: 0,
            second_start: Instant::now(),
            last_fire_tick: 0,
            fire_cooldown_ticks: 10, // Default ~0.33 seconds at 30 Hz
            consecutive_boost_ticks: 0,
            max_consecutive_boost: 300, // 10 seconds max continuous boost
            input_buffer_size: 0,
            max_input_buffer: 10,
        }
    }

    /// Check if input rate is within limits
    pub fn check_input_rate(&mut self, tick: u64) -> Result<(), RateLimitViolation> {
        let now = Instant::now();

        // Reset counter if a new second has started
        if now.duration_since(self.second_start) >= Duration::from_secs(1) {
            self.second_start = now;
            self.inputs_this_second = 0;
        }

        // Reject duplicate tick inputs (unless it's a retry)
        if tick == self.last_input_tick {
            // Allow same tick (could be retry), but count it
        }

        self.inputs_this_second += 1;
        self.last_input_tick = tick;

        // Maximum ~60 inputs per second (twice the tick rate for some slack)
        if self.inputs_this_second > 60 {
            return Err(RateLimitViolation::InputRateExceeded(self.inputs_this_second));
        }

        Ok(())
    }

    /// Check if fire rate is within limits
    pub fn check_fire_rate(&mut self, tick: u64) -> Result<(), RateLimitViolation> {
        // First fire is always allowed (last_fire_tick == 0 means never fired)
        if self.last_fire_tick > 0 {
            let ticks_since_fire = tick.saturating_sub(self.last_fire_tick);

            if ticks_since_fire < self.fire_cooldown_ticks {
                let remaining = self.fire_cooldown_ticks - ticks_since_fire;
                return Err(RateLimitViolation::FireRateExceeded(remaining));
            }
        }

        self.last_fire_tick = tick;
        Ok(())
    }

    /// Track boost usage
    pub fn track_boost(&mut self, is_boosting: bool) -> Result<(), RateLimitViolation> {
        if is_boosting {
            self.consecutive_boost_ticks += 1;
            if self.consecutive_boost_ticks > self.max_consecutive_boost {
                return Err(RateLimitViolation::BoostSpam(self.consecutive_boost_ticks));
            }
        } else {
            self.consecutive_boost_ticks = 0;
        }
        Ok(())
    }

    /// Track input buffer size
    pub fn track_input_buffer(&mut self, size: usize) -> Result<(), RateLimitViolation> {
        self.input_buffer_size = size;
        if size > self.max_input_buffer {
            return Err(RateLimitViolation::InputBufferOverflow);
        }
        Ok(())
    }

    /// Set fire cooldown (based on projectile mass/charge)
    pub fn set_fire_cooldown(&mut self, ticks: u64) {
        self.fire_cooldown_ticks = ticks;
    }

    /// Reset all limits (for respawn)
    pub fn reset(&mut self) {
        self.last_input_tick = 0;
        self.inputs_this_second = 0;
        self.second_start = Instant::now();
        self.last_fire_tick = 0;
        self.consecutive_boost_ticks = 0;
        self.input_buffer_size = 0;
    }
}

impl Default for PlayerRateLimits {
    fn default() -> Self {
        Self::new()
    }
}

/// Rate limiter manager for all players
pub struct RateLimiterManager {
    /// Per-player rate limits
    players: HashMap<PlayerId, PlayerRateLimits>,
    /// Default fire cooldown
    default_fire_cooldown: u64,
}

impl RateLimiterManager {
    pub fn new() -> Self {
        Self {
            players: HashMap::new(),
            default_fire_cooldown: 10,
        }
    }

    /// Register a new player
    pub fn register_player(&mut self, player_id: PlayerId) {
        self.players.insert(player_id, PlayerRateLimits::new());
    }

    /// Unregister a player
    pub fn unregister_player(&mut self, player_id: PlayerId) {
        self.players.remove(&player_id);
    }

    /// Get rate limits for a player
    pub fn get(&self, player_id: PlayerId) -> Option<&PlayerRateLimits> {
        self.players.get(&player_id)
    }

    /// Get mutable rate limits for a player
    pub fn get_mut(&mut self, player_id: PlayerId) -> Option<&mut PlayerRateLimits> {
        self.players.get_mut(&player_id)
    }

    /// Check input rate for a player
    pub fn check_input_rate(
        &mut self,
        player_id: PlayerId,
        tick: u64,
    ) -> Result<(), RateLimitViolation> {
        if let Some(limits) = self.players.get_mut(&player_id) {
            limits.check_input_rate(tick)
        } else {
            Ok(()) // Unknown player, let other systems handle it
        }
    }

    /// Check fire rate for a player
    pub fn check_fire_rate(
        &mut self,
        player_id: PlayerId,
        tick: u64,
    ) -> Result<(), RateLimitViolation> {
        if let Some(limits) = self.players.get_mut(&player_id) {
            limits.check_fire_rate(tick)
        } else {
            Ok(())
        }
    }

    /// Track boost for a player
    pub fn track_boost(
        &mut self,
        player_id: PlayerId,
        is_boosting: bool,
    ) -> Result<(), RateLimitViolation> {
        if let Some(limits) = self.players.get_mut(&player_id) {
            limits.track_boost(is_boosting)
        } else {
            Ok(())
        }
    }

    /// Reset limits for a player (on respawn)
    pub fn reset_player(&mut self, player_id: PlayerId) {
        if let Some(limits) = self.players.get_mut(&player_id) {
            limits.reset();
        }
    }

    /// Get player count
    pub fn player_count(&self) -> usize {
        self.players.len()
    }
}

impl Default for RateLimiterManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_player_rate_limits_new() {
        let limits = PlayerRateLimits::new();
        assert_eq!(limits.last_input_tick, 0);
        assert_eq!(limits.inputs_this_second, 0);
    }

    #[test]
    fn test_input_rate_normal() {
        let mut limits = PlayerRateLimits::new();

        for i in 1..=30 {
            assert!(limits.check_input_rate(i).is_ok());
        }
    }

    #[test]
    fn test_input_rate_exceeded() {
        let mut limits = PlayerRateLimits::new();

        // Exceed the limit
        for i in 1..=61 {
            let result = limits.check_input_rate(i);
            if i <= 60 {
                assert!(result.is_ok());
            } else {
                assert!(matches!(
                    result,
                    Err(RateLimitViolation::InputRateExceeded(_))
                ));
            }
        }
    }

    #[test]
    fn test_fire_rate_normal() {
        let mut limits = PlayerRateLimits::new();
        limits.fire_cooldown_ticks = 5;

        assert!(limits.check_fire_rate(10).is_ok());
        assert!(limits.check_fire_rate(20).is_ok()); // 10 ticks later
    }

    #[test]
    fn test_fire_rate_too_fast() {
        let mut limits = PlayerRateLimits::new();
        limits.fire_cooldown_ticks = 10;

        assert!(limits.check_fire_rate(10).is_ok());

        // Try to fire 5 ticks later (should fail, need 10)
        let result = limits.check_fire_rate(15);
        assert!(matches!(
            result,
            Err(RateLimitViolation::FireRateExceeded(_))
        ));
    }

    #[test]
    fn test_boost_tracking_normal() {
        let mut limits = PlayerRateLimits::new();

        // Some boosting is fine
        for _ in 0..100 {
            assert!(limits.track_boost(true).is_ok());
        }

        // Stopping resets counter
        assert!(limits.track_boost(false).is_ok());
        assert_eq!(limits.consecutive_boost_ticks, 0);
    }

    #[test]
    fn test_boost_spam_detected() {
        let mut limits = PlayerRateLimits::new();
        limits.max_consecutive_boost = 50;

        for i in 1..=51 {
            let result = limits.track_boost(true);
            if i <= 50 {
                assert!(result.is_ok());
            } else {
                assert!(matches!(result, Err(RateLimitViolation::BoostSpam(_))));
            }
        }
    }

    #[test]
    fn test_input_buffer_overflow() {
        let mut limits = PlayerRateLimits::new();
        limits.max_input_buffer = 5;

        assert!(limits.track_input_buffer(3).is_ok());
        assert!(limits.track_input_buffer(5).is_ok());
        assert!(matches!(
            limits.track_input_buffer(6),
            Err(RateLimitViolation::InputBufferOverflow)
        ));
    }

    #[test]
    fn test_reset() {
        let mut limits = PlayerRateLimits::new();
        limits.last_input_tick = 100;
        limits.consecutive_boost_ticks = 50;

        limits.reset();

        assert_eq!(limits.last_input_tick, 0);
        assert_eq!(limits.consecutive_boost_ticks, 0);
    }

    #[test]
    fn test_manager_register_player() {
        let mut manager = RateLimiterManager::new();
        let player_id = Uuid::new_v4();

        manager.register_player(player_id);

        assert!(manager.get(player_id).is_some());
        assert_eq!(manager.player_count(), 1);
    }

    #[test]
    fn test_manager_unregister_player() {
        let mut manager = RateLimiterManager::new();
        let player_id = Uuid::new_v4();

        manager.register_player(player_id);
        manager.unregister_player(player_id);

        assert!(manager.get(player_id).is_none());
        assert_eq!(manager.player_count(), 0);
    }

    #[test]
    fn test_manager_check_input_rate() {
        let mut manager = RateLimiterManager::new();
        let player_id = Uuid::new_v4();

        manager.register_player(player_id);

        assert!(manager.check_input_rate(player_id, 1).is_ok());
    }

    #[test]
    fn test_manager_unknown_player_ok() {
        let mut manager = RateLimiterManager::new();
        let player_id = Uuid::new_v4();

        // Unknown player should pass (handled elsewhere)
        assert!(manager.check_input_rate(player_id, 1).is_ok());
    }

    #[test]
    fn test_manager_reset_player() {
        let mut manager = RateLimiterManager::new();
        let player_id = Uuid::new_v4();

        manager.register_player(player_id);

        // Accumulate some state
        manager.track_boost(player_id, true).unwrap();
        manager.track_boost(player_id, true).unwrap();

        manager.reset_player(player_id);

        let limits = manager.get(player_id).unwrap();
        assert_eq!(limits.consecutive_boost_ticks, 0);
    }

    #[test]
    fn test_fire_cooldown_setting() {
        let mut limits = PlayerRateLimits::new();
        limits.set_fire_cooldown(20);

        assert!(limits.check_fire_rate(10).is_ok());

        // Need 20 ticks now
        assert!(limits.check_fire_rate(25).is_err()); // Only 15 ticks
        assert!(limits.check_fire_rate(30).is_ok()); // 20 ticks
    }
}
