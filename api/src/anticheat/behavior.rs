//! Behavioral analysis for anti-cheat
//!
//! Detects suspicious patterns like aimbot, speedhacks, and automation.

#![allow(dead_code)] // Behavioral analyzer ready for future integration

use std::collections::VecDeque;
use std::time::Instant;

use crate::game::state::PlayerId;
use crate::util::vec2::Vec2;

/// Behavioral analysis flags
#[derive(Debug, Clone)]
pub enum BehaviorFlag {
    /// Player appears to be using aimbot (perfect aim tracking)
    SuspiciousAim { accuracy: f32 },
    /// Player has inhuman reaction time
    InhumanReaction { reaction_ms: u32 },
    /// Movement patterns suggest automation
    AutomatedMovement { pattern_score: f32 },
    /// Player is idle/AFK
    Idle { duration_secs: u32 },
    /// Perfect input timing suggests macros
    MacroDetected { interval_variance: f32 },
}

/// Configuration for behavioral analysis
#[derive(Debug, Clone)]
pub struct BehaviorConfig {
    /// Minimum samples before analysis
    pub min_samples: usize,
    /// Maximum samples to keep in history
    pub max_samples: usize,
    /// Aim accuracy threshold for suspicion (0.0-1.0)
    pub aim_accuracy_threshold: f32,
    /// Minimum reaction time considered human (ms)
    pub min_human_reaction_ms: u32,
    /// Maximum idle time before flagging (seconds)
    pub max_idle_secs: u32,
    /// Input timing variance threshold for macro detection
    pub macro_variance_threshold: f32,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            min_samples: 30,
            max_samples: 300,
            aim_accuracy_threshold: 0.95, // 95% accuracy is suspicious
            min_human_reaction_ms: 100,   // <100ms is inhuman
            max_idle_secs: 60,
            macro_variance_threshold: 0.01, // Very consistent timing = macro
        }
    }
}

/// Tracks behavioral metrics for a player
#[derive(Debug)]
pub struct PlayerBehavior {
    /// Recent aim directions
    aim_history: VecDeque<Vec2>,
    /// Recent hit/miss outcomes
    hit_history: VecDeque<bool>,
    /// Input timing intervals (ms between inputs)
    input_intervals: VecDeque<u32>,
    /// Last input timestamp
    last_input_time: Option<Instant>,
    /// Last non-zero movement time
    last_movement_time: Instant,
    /// Movement direction history
    movement_history: VecDeque<Vec2>,
    /// Reaction times (time from stimulus to response)
    reaction_times: VecDeque<u32>,
    /// Configuration
    config: BehaviorConfig,
}

impl PlayerBehavior {
    pub fn new(config: BehaviorConfig) -> Self {
        Self {
            aim_history: VecDeque::with_capacity(config.max_samples),
            hit_history: VecDeque::with_capacity(config.max_samples),
            input_intervals: VecDeque::with_capacity(config.max_samples),
            last_input_time: None,
            last_movement_time: Instant::now(),
            movement_history: VecDeque::with_capacity(config.max_samples),
            reaction_times: VecDeque::with_capacity(config.max_samples),
            config,
        }
    }

    /// Record an input
    pub fn record_input(&mut self, thrust: Vec2, aim: Vec2) {
        let now = Instant::now();

        // Record input interval
        if let Some(last) = self.last_input_time {
            let interval = now.duration_since(last).as_millis() as u32;
            Self::push_to_bounded(&mut self.input_intervals, self.config.max_samples, interval);
        }
        self.last_input_time = Some(now);

        // Record aim
        if aim.length_sq() > 0.01 {
            Self::push_to_bounded(&mut self.aim_history, self.config.max_samples, aim);
        }

        // Record movement and update last movement time
        if thrust.length_sq() > 0.01 {
            self.last_movement_time = now;
            Self::push_to_bounded(&mut self.movement_history, self.config.max_samples, thrust);
        }
    }

    /// Record a shot outcome
    pub fn record_shot(&mut self, hit: bool) {
        Self::push_to_bounded(&mut self.hit_history, self.config.max_samples, hit);
    }

    /// Record a reaction to a stimulus
    pub fn record_reaction(&mut self, reaction_ms: u32) {
        Self::push_to_bounded(&mut self.reaction_times, self.config.max_samples, reaction_ms);
    }

    /// Analyze behavior and return any flags
    pub fn analyze(&self) -> Vec<BehaviorFlag> {
        let mut flags = Vec::new();

        // Check aim accuracy
        if let Some(accuracy) = self.calculate_aim_accuracy() {
            if accuracy > self.config.aim_accuracy_threshold {
                flags.push(BehaviorFlag::SuspiciousAim { accuracy });
            }
        }

        // Check for inhuman reaction times
        if let Some(avg_reaction) = self.calculate_avg_reaction() {
            if avg_reaction < self.config.min_human_reaction_ms {
                flags.push(BehaviorFlag::InhumanReaction {
                    reaction_ms: avg_reaction,
                });
            }
        }

        // Check for idle
        let idle_secs = self.last_movement_time.elapsed().as_secs() as u32;
        if idle_secs > self.config.max_idle_secs {
            flags.push(BehaviorFlag::Idle {
                duration_secs: idle_secs,
            });
        }

        // Check for macro-like input timing
        if let Some(variance) = self.calculate_input_variance() {
            if variance < self.config.macro_variance_threshold && self.input_intervals.len() > 20 {
                flags.push(BehaviorFlag::MacroDetected {
                    interval_variance: variance,
                });
            }
        }

        // Check for automated movement patterns
        if let Some(pattern_score) = self.detect_movement_patterns() {
            if pattern_score > 0.9 {
                flags.push(BehaviorFlag::AutomatedMovement { pattern_score });
            }
        }

        flags
    }

    /// Calculate hit accuracy (hits / total shots)
    fn calculate_aim_accuracy(&self) -> Option<f32> {
        if self.hit_history.len() < self.config.min_samples {
            return None;
        }

        let hits = self.hit_history.iter().filter(|&&h| h).count();
        Some(hits as f32 / self.hit_history.len() as f32)
    }

    /// Calculate average reaction time
    fn calculate_avg_reaction(&self) -> Option<u32> {
        if self.reaction_times.len() < 10 {
            return None;
        }

        let sum: u32 = self.reaction_times.iter().sum();
        Some(sum / self.reaction_times.len() as u32)
    }

    /// Calculate variance in input timing (low variance = macro)
    fn calculate_input_variance(&self) -> Option<f32> {
        if self.input_intervals.len() < 10 {
            return None;
        }

        let sum: u32 = self.input_intervals.iter().sum();
        let mean = sum as f32 / self.input_intervals.len() as f32;

        let variance: f32 = self
            .input_intervals
            .iter()
            .map(|&x| {
                let diff = x as f32 - mean;
                diff * diff
            })
            .sum::<f32>()
            / self.input_intervals.len() as f32;

        // Normalize by mean to get coefficient of variation
        if mean > 0.0 {
            Some(variance.sqrt() / mean)
        } else {
            None
        }
    }

    /// Detect repetitive movement patterns
    fn detect_movement_patterns(&self) -> Option<f32> {
        if self.movement_history.len() < 20 {
            return None;
        }

        // Look for repeating sequences
        let history: Vec<_> = self.movement_history.iter().collect();

        // Check for perfect oscillation (common bot pattern)
        let mut oscillations = 0;
        for window in history.windows(3) {
            let a = window[0].normalize();
            let c = window[2].normalize();
            // If direction reverses perfectly
            if (a + c).length() < 0.1 {
                oscillations += 1;
            }
        }

        let pattern_score = oscillations as f32 / (self.movement_history.len() - 2) as f32;

        Some(pattern_score)
    }

    /// Helper to push to bounded deque
    fn push_to_bounded<T>(deque: &mut VecDeque<T>, max_samples: usize, value: T) {
        if deque.len() >= max_samples {
            deque.pop_front();
        }
        deque.push_back(value);
    }

    /// Reset behavioral data (for new round)
    pub fn reset(&mut self) {
        self.aim_history.clear();
        self.hit_history.clear();
        self.input_intervals.clear();
        self.movement_history.clear();
        self.reaction_times.clear();
        self.last_input_time = None;
        self.last_movement_time = Instant::now();
    }
}

impl Default for PlayerBehavior {
    fn default() -> Self {
        Self::new(BehaviorConfig::default())
    }
}

/// Manager for all player behavioral analysis
pub struct BehaviorAnalyzer {
    players: std::collections::HashMap<PlayerId, PlayerBehavior>,
    config: BehaviorConfig,
}

impl BehaviorAnalyzer {
    pub fn new(config: BehaviorConfig) -> Self {
        Self {
            players: std::collections::HashMap::new(),
            config,
        }
    }

    pub fn register_player(&mut self, player_id: PlayerId) {
        self.players
            .insert(player_id, PlayerBehavior::new(self.config.clone()));
    }

    pub fn unregister_player(&mut self, player_id: PlayerId) {
        self.players.remove(&player_id);
    }

    pub fn get_mut(&mut self, player_id: PlayerId) -> Option<&mut PlayerBehavior> {
        self.players.get_mut(&player_id)
    }

    pub fn analyze_all(&self) -> Vec<(PlayerId, Vec<BehaviorFlag>)> {
        self.players
            .iter()
            .map(|(&id, behavior)| (id, behavior.analyze()))
            .filter(|(_, flags)| !flags.is_empty())
            .collect()
    }
}

impl Default for BehaviorAnalyzer {
    fn default() -> Self {
        Self::new(BehaviorConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_behavior_new() {
        let behavior = PlayerBehavior::default();
        assert!(behavior.aim_history.is_empty());
        assert!(behavior.hit_history.is_empty());
    }

    #[test]
    fn test_record_input() {
        let mut behavior = PlayerBehavior::default();

        behavior.record_input(Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0));

        assert_eq!(behavior.movement_history.len(), 1);
        assert_eq!(behavior.aim_history.len(), 1);
    }

    #[test]
    fn test_record_shot() {
        let mut behavior = PlayerBehavior::default();

        behavior.record_shot(true);
        behavior.record_shot(false);

        assert_eq!(behavior.hit_history.len(), 2);
    }

    #[test]
    fn test_idle_detection() {
        use std::time::Duration;

        let config = BehaviorConfig {
            max_idle_secs: 0, // Any idle time triggers
            ..Default::default()
        };
        let mut behavior = PlayerBehavior::new(config);

        // Simulate movement happening 2 seconds ago by backdating the movement time
        behavior.last_movement_time = Instant::now() - Duration::from_secs(2);

        let flags = behavior.analyze();
        assert!(flags.iter().any(|f| matches!(f, BehaviorFlag::Idle { .. })));
    }

    #[test]
    fn test_no_flags_without_enough_data() {
        let behavior = PlayerBehavior::default();
        let flags = behavior.analyze();

        // Should not have aim accuracy or reaction flags (not enough samples)
        assert!(!flags
            .iter()
            .any(|f| matches!(f, BehaviorFlag::SuspiciousAim { .. })));
        assert!(!flags
            .iter()
            .any(|f| matches!(f, BehaviorFlag::InhumanReaction { .. })));
    }

    #[test]
    fn test_reset() {
        let mut behavior = PlayerBehavior::default();

        behavior.record_input(Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0));
        behavior.record_shot(true);

        behavior.reset();

        assert!(behavior.aim_history.is_empty());
        assert!(behavior.hit_history.is_empty());
    }

    #[test]
    fn test_analyzer_register() {
        let mut analyzer = BehaviorAnalyzer::default();
        let player_id = uuid::Uuid::new_v4();

        analyzer.register_player(player_id);

        assert!(analyzer.get_mut(player_id).is_some());
    }

    #[test]
    fn test_analyzer_unregister() {
        let mut analyzer = BehaviorAnalyzer::default();
        let player_id = uuid::Uuid::new_v4();

        analyzer.register_player(player_id);
        analyzer.unregister_player(player_id);

        assert!(analyzer.get_mut(player_id).is_none());
    }

    #[test]
    fn test_suspicious_aim_detection() {
        let config = BehaviorConfig {
            min_samples: 5,
            aim_accuracy_threshold: 0.8,
            ..Default::default()
        };
        let mut behavior = PlayerBehavior::new(config);

        // Perfect accuracy
        for _ in 0..10 {
            behavior.record_shot(true);
        }

        let flags = behavior.analyze();
        assert!(flags
            .iter()
            .any(|f| matches!(f, BehaviorFlag::SuspiciousAim { .. })));
    }
}
