//! Decision History Storage
//!
//! Stores and persists AI decision history for learning and auditing.
//! Decisions are stored in a JSON file for persistence across restarts.

use std::fs;
use std::path::Path;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::MetricsSnapshot;

/// A recorded AI decision with full context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    /// Unique identifier for this decision
    pub id: String,
    /// When the decision was made
    pub timestamp: DateTime<Utc>,
    /// Metrics snapshot at time of decision
    pub metrics_before: MetricsSnapshot,
    /// AI's summary analysis
    pub analysis: String,
    /// AI's detailed reasoning
    pub reasoning: String,
    /// Actions taken
    pub actions: Vec<Action>,
    /// Confidence level (0.0-1.0)
    pub confidence: f32,
    /// Outcome evaluation (filled in later)
    pub outcome: Option<Outcome>,
}

/// A parameter change action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    /// Parameter path (e.g., "arena.max_wells")
    pub parameter: String,
    /// Value before change
    pub old_value: f32,
    /// Value after change
    pub new_value: f32,
    /// Reason for the change
    pub reason: String,
}

/// Outcome evaluation of a decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    /// When the outcome was evaluated
    pub evaluated_at: DateTime<Utc>,
    /// Change in tick time (negative = improvement)
    pub performance_delta_us: i64,
    /// Change in player count
    pub player_delta: i32,
    /// Whether the decision was successful
    pub success: bool,
}

/// Container for decision history with persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionHistory {
    /// Version for format compatibility
    version: u32,
    /// All recorded decisions
    decisions: Vec<Decision>,
    /// Aggregate statistics
    statistics: Statistics,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Statistics {
    total_decisions: u64,
    successful: u64,
    failed: u64,
}

impl DecisionHistory {
    /// Create a new empty history
    pub fn new() -> Self {
        Self {
            version: 1,
            decisions: Vec::new(),
            statistics: Statistics::default(),
        }
    }

    /// Load history from a JSON file
    ///
    /// Security: Limits file size to 10MB to prevent memory exhaustion
    pub fn load(path: &str) -> Result<Self, String> {
        const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB limit

        let path = Path::new(path);

        if !path.exists() {
            debug!("No existing history file at {}", path.display());
            return Ok(Self::new());
        }

        // Security: Check file size before loading
        let metadata = fs::metadata(path)
            .map_err(|e| format!("Failed to read file metadata: {}", e))?;

        if metadata.len() > MAX_FILE_SIZE {
            return Err(format!(
                "History file too large ({} bytes > {} limit). Consider reducing AI_MAX_HISTORY.",
                metadata.len(),
                MAX_FILE_SIZE
            ));
        }

        let contents = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read history file: {}", e))?;

        let history: DecisionHistory = serde_json::from_str(&contents)
            .map_err(|e| format!("Failed to parse history file: {}", e))?;

        info!("Loaded {} decisions from history", history.decisions.len());
        Ok(history)
    }

    /// Save history to a JSON file
    pub fn save(&self, path: &str) -> Result<(), String> {
        let path = Path::new(path);

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create directory: {}", e))?;
            }
        }

        let contents = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize history: {}", e))?;

        fs::write(path, contents)
            .map_err(|e| format!("Failed to write history file: {}", e))?;

        debug!("Saved {} decisions to {}", self.decisions.len(), path.display());
        Ok(())
    }

    /// Add a new decision to history
    pub fn add(&mut self, decision: Decision) {
        self.decisions.push(decision);
        self.statistics.total_decisions += 1;
    }

    /// Get a decision by index
    pub fn get(&self, index: usize) -> Option<&Decision> {
        self.decisions.get(index)
    }

    /// Get a mutable decision by index
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Decision> {
        self.decisions.get_mut(index)
    }

    /// Get the last decision
    pub fn last(&self) -> Option<&Decision> {
        self.decisions.last()
    }

    /// Get recent decisions (most recent first)
    pub fn recent(&self, count: usize) -> Vec<&Decision> {
        self.decisions.iter().rev().take(count).collect()
    }

    /// Get number of decisions
    pub fn len(&self) -> usize {
        self.decisions.len()
    }

    /// Check if history is empty
    pub fn is_empty(&self) -> bool {
        self.decisions.is_empty()
    }

    /// Remove the oldest decision
    pub fn remove_oldest(&mut self) {
        if !self.decisions.is_empty() {
            self.decisions.remove(0);
        }
    }

    /// Update an outcome and recalculate statistics
    pub fn update_outcome(&mut self, index: usize, outcome: Outcome) {
        if let Some(decision) = self.decisions.get_mut(index) {
            if outcome.success {
                self.statistics.successful += 1;
            } else {
                self.statistics.failed += 1;
            }
            decision.outcome = Some(outcome);
        }
    }

    /// Get success rate (successful, total with outcomes)
    pub fn success_rate(&self) -> (usize, usize) {
        let with_outcomes: Vec<_> = self.decisions.iter()
            .filter(|d| d.outcome.is_some())
            .collect();

        let successful = with_outcomes.iter()
            .filter(|d| d.outcome.as_ref().map(|o| o.success).unwrap_or(false))
            .count();

        (successful, with_outcomes.len())
    }

    /// Get all decisions for a parameter
    pub fn decisions_for_parameter(&self, param: &str) -> Vec<&Decision> {
        self.decisions.iter()
            .filter(|d| d.actions.iter().any(|a| a.parameter == param))
            .collect()
    }
}

impl Default for DecisionHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_decision(id: &str) -> Decision {
        Decision {
            id: id.to_string(),
            timestamp: Utc::now(),
            metrics_before: MetricsSnapshot {
                timestamp: Utc::now(),
                tick_time_p95_us: 15000,
                tick_time_max_us: 20000,
                total_players: 100,
                human_players: 5,
                bot_players: 95,
                alive_players: 80,
                projectile_count: 50,
                debris_count: 200,
                gravity_well_count: 10,
                arena_scale: 2.0,
                arena_radius: 2000.0,
                performance_status: "good".to_string(),
                budget_percent: 50,
            },
            analysis: "Test analysis".to_string(),
            reasoning: "Test reasoning".to_string(),
            actions: vec![Action {
                parameter: "arena.max_wells".to_string(),
                old_value: 20.0,
                new_value: 15.0,
                reason: "Test reason".to_string(),
            }],
            confidence: 0.8,
            outcome: None,
        }
    }

    #[test]
    fn test_new_history() {
        let history = DecisionHistory::new();
        assert!(history.is_empty());
        assert_eq!(history.len(), 0);
    }

    #[test]
    fn test_add_decision() {
        let mut history = DecisionHistory::new();
        let decision = create_test_decision("test_1");

        history.add(decision);

        assert_eq!(history.len(), 1);
        assert!(!history.is_empty());
    }

    #[test]
    fn test_recent_decisions() {
        let mut history = DecisionHistory::new();

        for i in 0..5 {
            history.add(create_test_decision(&format!("test_{}", i)));
        }

        let recent = history.recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].id, "test_4"); // Most recent first
        assert_eq!(recent[2].id, "test_2");
    }

    #[test]
    fn test_success_rate() {
        let mut history = DecisionHistory::new();

        let mut d1 = create_test_decision("test_1");
        d1.outcome = Some(Outcome {
            evaluated_at: Utc::now(),
            performance_delta_us: -1000,
            player_delta: 0,
            success: true,
        });

        let mut d2 = create_test_decision("test_2");
        d2.outcome = Some(Outcome {
            evaluated_at: Utc::now(),
            performance_delta_us: 5000,
            player_delta: -10,
            success: false,
        });

        history.add(d1);
        history.add(d2);
        history.add(create_test_decision("test_3")); // No outcome

        let (successful, total) = history.success_rate();
        assert_eq!(successful, 1);
        assert_eq!(total, 2); // Only counts those with outcomes
    }

    #[test]
    fn test_remove_oldest() {
        let mut history = DecisionHistory::new();

        for i in 0..3 {
            history.add(create_test_decision(&format!("test_{}", i)));
        }

        history.remove_oldest();

        assert_eq!(history.len(), 2);
        assert_eq!(history.get(0).unwrap().id, "test_1");
    }
}
