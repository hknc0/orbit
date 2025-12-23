//! Performance monitoring and adaptive scaling
//!
//! Tracks game tick performance and provides signals for:
//! - Admission control (reject new players when degraded)
//! - Bot scaling (reduce bots when struggling)
//! - Gravity well scaling (limit wells based on performance)

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Performance status levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformanceStatus {
    /// Performance is excellent, can add more entities
    Excellent,
    /// Performance is good, normal operation
    Good,
    /// Performance is degraded, should not add entities
    Warning,
    /// Performance is critical, stop adding and don't respawn bots
    Critical,
    /// Performance is catastrophic, must forcibly reduce entities
    Catastrophic,
}

impl PerformanceStatus {
    pub fn can_accept_players(&self) -> bool {
        matches!(self, PerformanceStatus::Excellent | PerformanceStatus::Good)
    }

    pub fn can_add_bots(&self) -> bool {
        matches!(self, PerformanceStatus::Excellent | PerformanceStatus::Good)
    }

    /// Should we allow dead bots to respawn?
    pub fn can_respawn_bots(&self) -> bool {
        matches!(self, PerformanceStatus::Excellent | PerformanceStatus::Good | PerformanceStatus::Warning)
    }

    /// Only forcibly remove bots in catastrophic situations
    pub fn should_force_reduce(&self) -> bool {
        matches!(self, PerformanceStatus::Catastrophic)
    }
}

/// Performance monitor that tracks tick durations
pub struct PerformanceMonitor {
    /// Rolling window of tick durations
    tick_durations: VecDeque<Duration>,
    /// Maximum samples to keep
    max_samples: usize,
    /// Target tick duration (budget)
    target_tick_duration: Duration,
    /// Threshold for excellent performance (fraction of budget)
    excellent_threshold: f32,
    /// Threshold for warning (fraction of budget)
    warning_threshold: f32,
    /// Threshold for critical (fraction of budget)
    critical_threshold: f32,
    /// Threshold for catastrophic (fraction of budget) - only force reduce here
    catastrophic_threshold: f32,
    /// Current performance status
    status: PerformanceStatus,
    /// Last tick start time for measuring duration
    tick_start: Option<Instant>,
    /// Total entity count at last measurement
    last_entity_count: usize,
}

impl PerformanceMonitor {
    pub fn new(tick_rate: u32) -> Self {
        let target_tick_duration = Duration::from_secs_f32(1.0 / tick_rate as f32);

        Self {
            tick_durations: VecDeque::with_capacity(120), // ~2 seconds at 60Hz
            max_samples: 120,
            target_tick_duration,
            excellent_threshold: 0.3,     // < 30% of budget = excellent
            warning_threshold: 0.7,       // > 70% of budget = warning
            critical_threshold: 0.9,      // > 90% of budget = critical
            catastrophic_threshold: 1.5,  // > 150% of budget = catastrophic (sustained overload)
            status: PerformanceStatus::Excellent,
            tick_start: None,
            last_entity_count: 0,
        }
    }

    /// Start timing a tick
    pub fn tick_start(&mut self) {
        self.tick_start = Some(Instant::now());
    }

    /// End timing a tick and record the duration
    pub fn tick_end(&mut self, entity_count: usize) {
        if let Some(start) = self.tick_start.take() {
            let duration = start.elapsed();
            self.record_tick(duration);
            self.last_entity_count = entity_count;
        }
    }

    /// Record a tick duration
    fn record_tick(&mut self, duration: Duration) {
        self.tick_durations.push_back(duration);
        while self.tick_durations.len() > self.max_samples {
            self.tick_durations.pop_front();
        }
        self.update_status();
    }

    /// Update performance status based on recent tick durations
    fn update_status(&mut self) {
        if self.tick_durations.len() < 10 {
            // Not enough data yet
            return;
        }

        let avg = self.average_tick_duration();
        let ratio = avg.as_secs_f32() / self.target_tick_duration.as_secs_f32();

        self.status = if ratio < self.excellent_threshold {
            PerformanceStatus::Excellent
        } else if ratio < self.warning_threshold {
            PerformanceStatus::Good
        } else if ratio < self.critical_threshold {
            PerformanceStatus::Warning
        } else if ratio < self.catastrophic_threshold {
            PerformanceStatus::Critical
        } else {
            PerformanceStatus::Catastrophic
        };
    }

    /// Get average tick duration
    pub fn average_tick_duration(&self) -> Duration {
        if self.tick_durations.is_empty() {
            return Duration::ZERO;
        }
        let sum: Duration = self.tick_durations.iter().sum();
        sum / self.tick_durations.len() as u32
    }

    /// Get the 95th percentile tick duration
    pub fn p95_tick_duration(&self) -> Duration {
        if self.tick_durations.is_empty() {
            return Duration::ZERO;
        }
        let mut sorted: Vec<_> = self.tick_durations.iter().copied().collect();
        sorted.sort();
        let idx = (sorted.len() as f32 * 0.95) as usize;
        sorted.get(idx.min(sorted.len() - 1)).copied().unwrap_or(Duration::ZERO)
    }

    /// Get current performance status
    pub fn status(&self) -> PerformanceStatus {
        self.status
    }

    /// Get budget usage as percentage (0-100+)
    pub fn budget_usage_percent(&self) -> f32 {
        let avg = self.average_tick_duration();
        (avg.as_secs_f32() / self.target_tick_duration.as_secs_f32()) * 100.0
    }

    /// Check if we can accept new players
    pub fn can_accept_players(&self) -> bool {
        self.status.can_accept_players()
    }

    /// Check if we can add bots
    pub fn can_add_bots(&self) -> bool {
        self.status.can_add_bots()
    }

    /// Check if we should allow dead bots to respawn
    pub fn can_respawn_bots(&self) -> bool {
        self.status.can_respawn_bots()
    }

    /// Check if we need to forcibly reduce entities (catastrophic only)
    pub fn should_force_reduce(&self) -> bool {
        self.status.should_force_reduce()
    }

    /// Get last known entity count
    pub fn last_entity_count(&self) -> usize {
        self.last_entity_count
    }

    /// Get a human-readable status message
    pub fn status_message(&self) -> String {
        format!(
            "{:?} - {:.1}% budget, {} entities",
            self.status,
            self.budget_usage_percent(),
            self.last_entity_count
        )
    }

    /// Get rejection message for new players
    pub fn rejection_message(&self, player_count: usize) -> String {
        format!(
            "Server at capacity ({} players). Please try again later.",
            player_count
        )
    }

    /// Calculate dynamic entity budget based on current performance
    /// Returns None (no limit) if plenty of headroom, or Some(max) based on remaining budget
    pub fn calculate_entity_budget(&self, current_count: usize) -> Option<usize> {
        if self.tick_durations.len() < 10 {
            // Not enough data, no limit
            return None;
        }

        let budget_used = self.budget_usage_percent() / 100.0;

        if budget_used < 0.5 {
            // Using less than 50% of budget - no limit, plenty of headroom
            None
        } else if budget_used >= 1.0 {
            // At or over budget - reduce by 25% from current
            Some((current_count as f32 * 0.75).max(1.0) as usize)
        } else {
            // Between 50-100% budget usage
            // Calculate headroom: how much more can we handle?
            // At 50% usage: can potentially double (2x)
            // At 90% usage: can only add ~11% more (1.11x)
            let headroom = 1.0 / budget_used;
            let max_entities = (current_count as f32 * headroom).ceil() as usize;

            // Only apply limit if it would actually constrain growth
            if max_entities <= current_count + 1 {
                Some(max_entities.max(1))
            } else {
                None
            }
        }
    }
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self::new(60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_monitor_new() {
        let monitor = PerformanceMonitor::new(60);
        assert_eq!(monitor.status(), PerformanceStatus::Excellent);
    }

    #[test]
    fn test_excellent_performance() {
        let mut monitor = PerformanceMonitor::new(60);
        // Tick budget is ~16.67ms, excellent is < 30% = ~5ms
        for _ in 0..20 {
            monitor.record_tick(Duration::from_millis(2));
        }
        assert_eq!(monitor.status(), PerformanceStatus::Excellent);
        assert!(monitor.can_accept_players());
        assert!(monitor.can_add_bots());
    }

    #[test]
    fn test_good_performance() {
        let mut monitor = PerformanceMonitor::new(60);
        // Good is 30-70% of budget = ~5-12ms
        for _ in 0..20 {
            monitor.record_tick(Duration::from_millis(8));
        }
        assert_eq!(monitor.status(), PerformanceStatus::Good);
        assert!(monitor.can_accept_players());
        assert!(monitor.can_add_bots());
    }

    #[test]
    fn test_warning_performance() {
        let mut monitor = PerformanceMonitor::new(60);
        // Warning is 70-90% of budget = ~12-15ms
        for _ in 0..20 {
            monitor.record_tick(Duration::from_millis(13));
        }
        assert_eq!(monitor.status(), PerformanceStatus::Warning);
        assert!(!monitor.can_accept_players());
        assert!(!monitor.can_add_bots());
    }

    #[test]
    fn test_critical_performance() {
        let mut monitor = PerformanceMonitor::new(60);
        // Critical is 90-150% of budget = ~15-25ms
        for _ in 0..20 {
            monitor.record_tick(Duration::from_millis(18));
        }
        assert_eq!(monitor.status(), PerformanceStatus::Critical);
        assert!(!monitor.can_accept_players());
        assert!(!monitor.can_respawn_bots()); // Don't respawn bots
        assert!(!monitor.should_force_reduce()); // But don't force-remove either
    }

    #[test]
    fn test_catastrophic_performance() {
        let mut monitor = PerformanceMonitor::new(60);
        // Catastrophic is > 150% of budget = > 25ms
        for _ in 0..20 {
            monitor.record_tick(Duration::from_millis(30));
        }
        assert_eq!(monitor.status(), PerformanceStatus::Catastrophic);
        assert!(!monitor.can_accept_players());
        assert!(!monitor.can_respawn_bots());
        assert!(monitor.should_force_reduce()); // Only now do we force-remove
    }

    #[test]
    fn test_tick_timing() {
        let mut monitor = PerformanceMonitor::new(60);
        monitor.tick_start();
        std::thread::sleep(Duration::from_millis(1));
        monitor.tick_end(10);

        assert!(!monitor.tick_durations.is_empty());
        assert_eq!(monitor.last_entity_count(), 10);
    }

    #[test]
    fn test_entity_budget_no_data() {
        let monitor = PerformanceMonitor::new(60);
        // No data yet, should return None (no limit)
        assert_eq!(monitor.calculate_entity_budget(5), None);
    }

    #[test]
    fn test_entity_budget_low_usage() {
        let mut monitor = PerformanceMonitor::new(60);
        // ~2ms tick = ~12% of 16.67ms budget
        for _ in 0..20 {
            monitor.record_tick(Duration::from_millis(2));
        }
        // Low usage = no limit
        assert_eq!(monitor.calculate_entity_budget(5), None);
    }

    #[test]
    fn test_entity_budget_high_usage() {
        let mut monitor = PerformanceMonitor::new(60);
        // ~16ms tick = ~96% of 16.67ms budget
        for _ in 0..20 {
            monitor.record_tick(Duration::from_micros(16000));
        }
        // At 96% budget with 10 entities, headroom = 1/0.96 = 1.04x
        // Max = 10 * 1.04 = 10.4 -> 11
        // 11 <= 10 + 1, so should return Some
        let budget = monitor.calculate_entity_budget(10);
        assert!(budget.is_some());
        assert!(budget.unwrap() <= 11);
    }

    #[test]
    fn test_entity_budget_over_budget() {
        let mut monitor = PerformanceMonitor::new(60);
        // ~20ms tick = 120% of budget
        for _ in 0..20 {
            monitor.record_tick(Duration::from_millis(20));
        }
        // Over budget = reduce by 25%
        let budget = monitor.calculate_entity_budget(10);
        assert_eq!(budget, Some(7)); // 10 * 0.75 = 7.5 -> 7
    }
}
