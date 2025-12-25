//! AI Simulation Manager
//!
//! An autonomous AI agent that monitors game server metrics, analyzes performance patterns,
//! makes intelligent parameter adjustments via Claude API, and learns from outcomes.
//!
//! # Features
//!
//! - Real-time metrics monitoring and analysis
//! - Claude API integration for intelligent decision making
//! - Decision history with outcome tracking
//! - Configurable evaluation intervals and confidence thresholds
//! - Full decision logging with explanations
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    AI Manager System                        │
//! ├─────────────────────────────────────────────────────────────┤
//! │  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐  │
//! │  │  Collector   │───▶│   Analyst    │───▶│   Executor   │  │
//! │  │ (metrics)    │    │ (Claude API) │    │ (apply cfg)  │  │
//! │  └──────────────┘    └──────────────┘    └──────────────┘  │
//! │         ▲                   │                    │          │
//! │         │                   ▼                    │          │
//! │         │           ┌──────────────┐             │          │
//! │         └───────────│   History    │◀────────────┘          │
//! │                     │  (decisions) │                        │
//! │                     └──────────────┘                        │
//! └─────────────────────────────────────────────────────────────┘
//! ```

mod client;
mod history;
mod analysis;

pub use client::ClaudeClient;
pub use history::{Decision, DecisionHistory, Action, Outcome};
pub use analysis::{Analysis, Recommendation};

use std::sync::Arc;
use std::time::Duration;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use tracing::{info, warn, error, debug};

use crate::config::{AIManagerConfig, ArenaScalingConfig};
use crate::metrics::{Metrics, AIManagerMetrics, AIDecisionSummary, AIActionSummary, AIOutcomeSummary};

/// Snapshot of game metrics for AI analysis
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MetricsSnapshot {
    pub timestamp: DateTime<Utc>,
    pub tick_time_p95_us: u64,
    pub tick_time_max_us: u64,
    pub total_players: u64,
    pub human_players: u64,
    pub bot_players: u64,
    pub alive_players: u64,
    pub projectile_count: u64,
    pub debris_count: u64,
    pub gravity_well_count: u64,
    pub arena_scale: f32,
    pub arena_radius: f32,
    pub performance_status: String,
    pub budget_percent: u64,
}

impl MetricsSnapshot {
    /// Create a snapshot from the metrics registry
    pub fn from_metrics(metrics: &Metrics) -> Self {
        use std::sync::atomic::Ordering;

        Self {
            timestamp: Utc::now(),
            tick_time_p95_us: metrics.tick_time_p95_us.load(Ordering::Relaxed),
            tick_time_max_us: metrics.tick_time_max_us.load(Ordering::Relaxed),
            total_players: metrics.total_players.load(Ordering::Relaxed),
            human_players: metrics.human_players.load(Ordering::Relaxed),
            bot_players: metrics.bot_players.load(Ordering::Relaxed),
            alive_players: metrics.alive_players.load(Ordering::Relaxed),
            projectile_count: metrics.projectile_count.load(Ordering::Relaxed),
            debris_count: metrics.debris_count.load(Ordering::Relaxed),
            gravity_well_count: metrics.gravity_well_count.load(Ordering::Relaxed),
            arena_scale: metrics.arena_scale.load(Ordering::Relaxed) as f32 / 100.0,
            arena_radius: metrics.arena_radius.load(Ordering::Relaxed) as f32,
            performance_status: match metrics.performance_status.load(Ordering::Relaxed) {
                0 => "excellent".to_string(),
                1 => "good".to_string(),
                2 => "warning".to_string(),
                3 => "critical".to_string(),
                _ => "catastrophic".to_string(),
            },
            budget_percent: metrics.budget_usage_percent.load(Ordering::Relaxed),
        }
    }
}

/// AI Simulation Manager
///
/// Autonomously monitors and tunes game server parameters using Claude API.
/// Keeps full decision history with outcome tracking for learning.
pub struct AIManager {
    config: AIManagerConfig,
    client: ClaudeClient,
    history: DecisionHistory,
    last_evaluation: Option<DateTime<Utc>>,
    pending_evaluations: Vec<usize>, // Indices of decisions awaiting outcome evaluation
    disabled_due_to_error: bool, // Set to true on fatal errors (e.g., invalid API key)
    consecutive_errors: u32, // Track consecutive errors for auto-disable
}

impl AIManager {
    /// Create a new AI Manager with the given configuration
    pub fn new(config: AIManagerConfig) -> Self {
        let client = ClaudeClient::new(
            config.api_key.clone().unwrap_or_default(),
            config.model.clone(),
        );

        // Load existing history from disk
        let history = DecisionHistory::load(&config.history_file)
            .unwrap_or_else(|e| {
                warn!("Failed to load AI decision history: {}. Starting fresh.", e);
                DecisionHistory::new()
            });

        info!(
            "AI Manager initialized: model={}, interval={}m, history={} decisions",
            config.model,
            config.eval_interval_minutes,
            history.len()
        );

        Self {
            config,
            client,
            history,
            last_evaluation: None,
            pending_evaluations: Vec::new(),
            disabled_due_to_error: false,
            consecutive_errors: 0,
        }
    }

    /// Check if an error is fatal (should disable the AI manager)
    fn is_fatal_error(error: &str) -> bool {
        let error_lower = error.to_lowercase();
        // API key issues
        error_lower.contains("api key") ||
        error_lower.contains("invalid_api_key") ||
        error_lower.contains("authentication") ||
        error_lower.contains("unauthorized") ||
        error_lower.contains("401") ||
        error_lower.contains("403") ||
        // Rate limiting - disable to avoid burning money/getting banned
        error_lower.contains("rate limit") ||
        error_lower.contains("rate_limit") ||
        error_lower.contains("too many requests") ||
        error_lower.contains("429") ||
        // Billing/quota issues
        error_lower.contains("insufficient") ||
        error_lower.contains("quota") ||
        error_lower.contains("billing")
    }

    /// Run the AI manager main loop
    pub async fn run(
        mut self,
        metrics: Arc<Metrics>,
        arena_config: Arc<RwLock<ArenaScalingConfig>>,
    ) {
        let interval = Duration::from_secs(self.config.eval_interval_minutes as u64 * 60);
        let mut interval_timer = tokio::time::interval(interval);

        info!("AI Manager starting main loop (interval: {}m)", self.config.eval_interval_minutes);

        // Mark AI as enabled in Prometheus metrics
        metrics.ai_enabled.store(1, std::sync::atomic::Ordering::Relaxed);

        const MAX_CONSECUTIVE_ERRORS: u32 = 5;

        loop {
            interval_timer.tick().await;

            // Skip if disabled due to fatal error
            if self.disabled_due_to_error {
                debug!("AI Manager: disabled due to previous fatal error");
                continue;
            }

            // Skip if not properly configured
            if !self.config.is_active() {
                debug!("AI Manager: not active (missing API key or disabled)");
                metrics.ai_enabled.store(0, std::sync::atomic::Ordering::Relaxed);
                continue;
            }

            // Update pending count in Prometheus
            metrics.ai_pending_evaluations.store(
                self.pending_evaluations.len() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );

            // 1. Collect current metrics snapshot
            let snapshot = MetricsSnapshot::from_metrics(&metrics);

            // 2. Evaluate any pending decisions (made >60s ago)
            self.evaluate_pending_decisions(&snapshot, &metrics);

            // 3. Ask Claude for analysis
            match self.analyze_simulation(&snapshot).await {
                Ok(analysis) => {
                    // Log the analysis
                    info!(
                        "AI Analysis: {} (confidence: {:.2})",
                        analysis.summary,
                        analysis.confidence
                    );

                    if analysis.confidence >= self.config.confidence_threshold {
                        // 4. Apply recommended changes
                        let actions = self.apply_recommendations(
                            &analysis,
                            &arena_config,
                        );

                        if !actions.is_empty() {
                            // 5. Record decision with full explanation
                            let decision = Decision {
                                id: self.generate_decision_id(),
                                timestamp: Utc::now(),
                                metrics_before: snapshot.clone(),
                                analysis: analysis.summary.clone(),
                                reasoning: analysis.reasoning.clone(),
                                actions: actions.clone(),
                                confidence: analysis.confidence,
                                outcome: None,
                            };

                            // Log the decision with full explanation
                            info!("=== AI DECISION ===");
                            info!("ID: {}", decision.id);
                            info!("Analysis: {}", decision.analysis);
                            info!("Reasoning: {}", decision.reasoning);
                            for action in &decision.actions {
                                info!(
                                    "  Action: {} = {} -> {} ({})",
                                    action.parameter,
                                    action.old_value,
                                    action.new_value,
                                    action.reason
                                );
                            }
                            info!("Confidence: {:.2}", decision.confidence);
                            info!("===================");

                            // Track for outcome evaluation
                            let idx = self.history.len();
                            self.history.add(decision);
                            self.pending_evaluations.push(idx);

                            // Update Prometheus metrics
                            metrics.ai_decisions_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            metrics.ai_last_confidence.store(
                                (analysis.confidence * 100.0) as u64,
                                std::sync::atomic::Ordering::Relaxed,
                            );

                            // 6. Persist history
                            if let Err(e) = self.history.save(&self.config.history_file) {
                                error!("Failed to save AI decision history: {}", e);
                            }
                        }
                    } else {
                        debug!(
                            "AI: Low confidence ({:.2} < {:.2}), skipping action",
                            analysis.confidence,
                            self.config.confidence_threshold
                        );
                    }
                }
                Err(e) => {
                    error!("AI analysis failed: {}", e);
                    self.consecutive_errors += 1;

                    // Check if this is a fatal error that should disable AI
                    if Self::is_fatal_error(&e) {
                        error!("AI Manager: Fatal error detected, disabling AI manager: {}", e);
                        self.disabled_due_to_error = true;
                        metrics.ai_enabled.store(0, std::sync::atomic::Ordering::Relaxed);
                    } else if self.consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        error!(
                            "AI Manager: Too many consecutive errors ({}), disabling",
                            self.consecutive_errors
                        );
                        self.disabled_due_to_error = true;
                        metrics.ai_enabled.store(0, std::sync::atomic::Ordering::Relaxed);
                    }

                    continue;
                }
            }

            // Reset consecutive errors on success
            self.consecutive_errors = 0;

            self.last_evaluation = Some(Utc::now());

            // Keep history bounded
            while self.history.len() > self.config.max_history {
                self.history.remove_oldest();
                // Adjust pending indices
                self.pending_evaluations.retain_mut(|idx| {
                    if *idx > 0 {
                        *idx -= 1;
                        true
                    } else {
                        false
                    }
                });
            }
        }
    }

    /// Analyze current simulation state using Claude API
    async fn analyze_simulation(&self, snapshot: &MetricsSnapshot) -> Result<Analysis, String> {
        self.client.analyze(snapshot, &self.history.recent(5)).await
    }

    /// Apply recommended parameter changes
    fn apply_recommendations(
        &self,
        analysis: &Analysis,
        arena_config: &Arc<RwLock<ArenaScalingConfig>>,
    ) -> Vec<Action> {
        let mut actions = Vec::new();

        for rec in &analysis.recommendations {
            // Validate parameter is known and value is in range
            if !self.is_valid_parameter(&rec.parameter) {
                warn!("AI: Unknown parameter '{}', skipping", rec.parameter);
                continue;
            }

            // Apply the change
            let mut config = arena_config.write();
            let old_value = self.get_parameter_value(&config, &rec.parameter);

            if let Some(old) = old_value {
                // Safety: limit change to 20% of current value
                let max_change = old.abs() * 0.2;
                let clamped_new = if (rec.value - old).abs() > max_change {
                    if rec.value > old {
                        old + max_change
                    } else {
                        old - max_change
                    }
                } else {
                    rec.value
                };

                if self.set_parameter_value(&mut config, &rec.parameter, clamped_new) {
                    info!(
                        "AI: Applied {} = {} -> {} (requested: {}, reason: {})",
                        rec.parameter, old, clamped_new, rec.value, rec.reason
                    );

                    actions.push(Action {
                        parameter: rec.parameter.clone(),
                        old_value: old,
                        new_value: clamped_new,
                        reason: rec.reason.clone(),
                    });
                }
            }
        }

        actions
    }

    /// Evaluate pending decisions after outcome delay
    fn evaluate_pending_decisions(&mut self, current: &MetricsSnapshot, metrics: &Metrics) {
        let outcome_delay = Duration::from_secs(60); // Evaluate after 60 seconds
        let now = Utc::now();

        let mut evaluated = Vec::new();
        let mut successful = 0u64;

        for &idx in &self.pending_evaluations {
            if let Some(decision) = self.history.get(idx) {
                // Check if enough time has passed
                let elapsed = now.signed_duration_since(decision.timestamp);
                if elapsed.to_std().unwrap_or(Duration::ZERO) >= outcome_delay {
                    // Evaluate outcome
                    let perf_before = decision.metrics_before.tick_time_p95_us as i64;
                    let perf_after = current.tick_time_p95_us as i64;
                    let player_before = decision.metrics_before.total_players as i32;
                    let player_after = current.total_players as i32;

                    let outcome = Outcome {
                        evaluated_at: now,
                        performance_delta_us: perf_after - perf_before,
                        player_delta: player_after - player_before,
                        success: perf_after <= perf_before, // Lower tick time = better
                    };

                    info!(
                        "AI Outcome: {} - {} (perf: {}us, players: {})",
                        decision.id,
                        if outcome.success { "SUCCESS" } else { "FAILED" },
                        outcome.performance_delta_us,
                        outcome.player_delta
                    );

                    if outcome.success {
                        successful += 1;
                    }

                    if let Some(d) = self.history.get_mut(idx) {
                        d.outcome = Some(outcome);
                    }

                    evaluated.push(idx);
                }
            }
        }

        // Update Prometheus metrics for successful outcomes
        if successful > 0 {
            metrics.ai_decisions_successful.fetch_add(successful, std::sync::atomic::Ordering::Relaxed);
        }

        // Remove evaluated from pending
        self.pending_evaluations.retain(|idx| !evaluated.contains(idx));

        // Save if any were evaluated
        if !evaluated.is_empty() {
            if let Err(e) = self.history.save(&self.config.history_file) {
                error!("Failed to save AI decision history after evaluation: {}", e);
            }
        }
    }

    /// Check if a parameter name is valid
    fn is_valid_parameter(&self, param: &str) -> bool {
        matches!(param,
            "arena.grow_lerp" |
            "arena.shrink_lerp" |
            "arena.shrink_delay_ticks" |
            "arena.max_wells" |
            "arena.base_player_count" |
            "arena.area_per_player"
        )
    }

    /// Get current value of a parameter
    fn get_parameter_value(&self, config: &ArenaScalingConfig, param: &str) -> Option<f32> {
        match param {
            "arena.grow_lerp" => Some(config.grow_lerp),
            "arena.shrink_lerp" => Some(config.shrink_lerp),
            "arena.shrink_delay_ticks" => Some(config.shrink_delay_ticks as f32),
            "arena.wells_per_area" => Some(config.wells_per_area),
            "arena.min_wells" => Some(config.min_wells as f32),
            "arena.base_player_count" => Some(config.base_player_count),
            "arena.area_per_player" => Some(config.area_per_player),
            _ => None,
        }
    }

    /// Set a parameter value
    fn set_parameter_value(&self, config: &mut ArenaScalingConfig, param: &str, value: f32) -> bool {
        match param {
            "arena.grow_lerp" => {
                config.grow_lerp = value.clamp(0.01, 0.1);
                true
            }
            "arena.shrink_lerp" => {
                config.shrink_lerp = value.clamp(0.001, 0.05);
                true
            }
            "arena.shrink_delay_ticks" => {
                config.shrink_delay_ticks = (value as u32).clamp(0, 300);
                true
            }
            "arena.wells_per_area" => {
                // Area per well: lower = more wells, higher = fewer wells
                config.wells_per_area = value.clamp(100_000.0, 5_000_000.0);
                true
            }
            "arena.min_wells" => {
                config.min_wells = (value as usize).clamp(1, 1000);
                true
            }
            "arena.base_player_count" => {
                config.base_player_count = value.clamp(1.0, 100.0);
                true
            }
            "arena.area_per_player" => {
                config.area_per_player = value.clamp(50_000.0, 500_000.0);
                true
            }
            _ => false,
        }
    }

    /// Generate a unique decision ID
    fn generate_decision_id(&self) -> String {
        format!("dec_{}_{:03}",
            Utc::now().format("%Y%m%d_%H%M%S"),
            self.history.len() % 1000
        )
    }

    /// Get metrics for the /json endpoint
    pub fn get_metrics(&self) -> AIManagerMetrics {
        let (successful, total) = self.history.success_rate();
        let success_rate = if total > 0 {
            successful as f32 / total as f32
        } else {
            0.0
        };

        AIManagerMetrics {
            enabled: self.config.is_active(),
            status: if self.config.is_active() { "active".to_string() } else { "disabled".to_string() },
            last_evaluation: self.last_evaluation.map(|t| t.to_rfc3339()),
            next_evaluation: self.last_evaluation.map(|t| {
                (t + chrono::Duration::minutes(self.config.eval_interval_minutes as i64)).to_rfc3339()
            }),
            decisions_made: total as u64,
            success_rate,
            current_confidence: self.history.last().map(|d| d.confidence),
            recent_decisions: self.history.recent(5).iter().map(|d| {
                AIDecisionSummary {
                    id: d.id.clone(),
                    timestamp: d.timestamp.to_rfc3339(),
                    analysis: d.analysis.clone(),
                    actions: d.actions.iter().map(|a| AIActionSummary {
                        parameter: a.parameter.clone(),
                        old_value: a.old_value,
                        new_value: a.new_value,
                        reason: a.reason.clone(),
                    }).collect(),
                    outcome: d.outcome.as_ref().map(|o| AIOutcomeSummary {
                        success: o.success,
                        performance_delta_us: o.performance_delta_us,
                        player_delta: o.player_delta,
                    }),
                }
            }).collect(),
            pending_evaluations: self.pending_evaluations.len() as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_snapshot() {
        let metrics = Metrics::new();
        let snapshot = MetricsSnapshot::from_metrics(&metrics);

        assert_eq!(snapshot.total_players, 0);
        assert_eq!(snapshot.performance_status, "excellent");
    }

    #[test]
    fn test_decision_id_generation() {
        let config = AIManagerConfig::default();
        let manager = AIManager::new(config);

        let id = manager.generate_decision_id();
        assert!(id.starts_with("dec_"));
    }
}
