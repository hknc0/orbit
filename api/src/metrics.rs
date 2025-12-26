//! Prometheus-compatible metrics endpoint
//!
//! Exposes game server metrics in Prometheus format for Grafana dashboards.
//! - /metrics: Prometheus format for Grafana scraping
//! - /json: Simple JSON format for direct API access
//! - /health: Health check endpoint

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{info, debug};

/// Metrics registry for the game server
#[derive(Debug)]
pub struct Metrics {
    // Player counts
    pub total_players: AtomicU64,
    pub human_players: AtomicU64,
    pub bot_players: AtomicU64,
    pub alive_players: AtomicU64,

    // Entity counts
    pub projectile_count: AtomicU64,
    pub debris_count: AtomicU64,
    pub gravity_well_count: AtomicU64,

    // Tick timing (microseconds)
    pub tick_time_us: AtomicU64,
    pub tick_time_p95_us: AtomicU64,
    pub tick_time_p99_us: AtomicU64,
    pub tick_time_max_us: AtomicU64,

    // Performance status (0=Excellent, 1=Good, 2=Warning, 3=Critical, 4=Catastrophic)
    pub performance_status: AtomicU64,
    pub budget_usage_percent: AtomicU64,

    // Tick counter
    pub tick_count: AtomicU64,

    // Network stats
    pub connections_active: AtomicU64,
    pub messages_sent: AtomicU64,
    pub messages_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,

    // Game state
    pub match_time_seconds: AtomicU64,
    pub arena_scale: AtomicU64, // Stored as scale * 100 (e.g., 1.5 = 150)
    pub arena_radius: AtomicU64, // Arena escape radius in world units
    pub arena_target_radius: AtomicU64, // Target radius before lerping
    pub arena_gravity_wells: AtomicU64, // Number of gravity wells
    pub arena_wells_lerping: AtomicU64, // Wells currently moving toward target
    pub arena_area_per_player: AtomicU64, // Actual area per player in square units

    // Server uptime
    start_time: Instant,

    // Simulation mode metrics
    pub simulation_enabled: AtomicU64,      // 0 or 1
    pub simulation_target_bots: AtomicU64,  // Current target bot count
    pub simulation_cycle_progress: AtomicU64, // Progress through cycle (0-100)

    // Extended metrics (feature-gated: metrics_extended)
    // AOI (Area of Interest) filtering stats
    pub aoi_original_players: AtomicU64,     // Players before AOI filtering
    pub aoi_filtered_players: AtomicU64,     // Players after AOI filtering
    pub aoi_reduction_percent: AtomicU64,    // Bandwidth reduction percentage (0-100)
    pub aoi_original_projectiles: AtomicU64, // Projectiles before filtering
    pub aoi_filtered_projectiles: AtomicU64, // Projectiles after filtering

    // Anti-cheat metrics (feature-gated)
    pub anticheat_inputs_validated: AtomicU64,   // Total inputs validated
    pub anticheat_inputs_rejected: AtomicU64,    // Inputs rejected (invalid)
    pub anticheat_inputs_sanitized: AtomicU64,   // Inputs sanitized (fixed)
    pub anticheat_sequence_violations: AtomicU64, // Sequence validation failures

    // DoS protection metrics
    pub dos_connections_rejected: AtomicU64,   // Connections rejected by DoS
    pub dos_messages_rate_limited: AtomicU64,  // Messages dropped by rate limit
    pub dos_active_bans: AtomicU64,            // Currently banned IPs

    // AI Manager metrics (feature-gated) - for AI Simulation Manager
    pub ai_enabled: AtomicU64,                 // AI manager enabled (0/1)
    pub ai_decisions_total: AtomicU64,         // Total decisions made
    pub ai_decisions_successful: AtomicU64,    // Successful decisions
    pub ai_last_confidence: AtomicU64,         // Last confidence level (0-100)
    #[allow(dead_code)]
    pub ai_pending_evaluations: AtomicU64,     // Decisions awaiting outcome evaluation

    // Bot AI SoA metrics - for million-scale bot AI system
    pub bot_ai_total: AtomicU64,               // Total bots registered
    pub bot_ai_active: AtomicU64,              // Bots active this tick
    pub bot_ai_full_mode: AtomicU64,           // Bots in full update mode
    pub bot_ai_reduced_mode: AtomicU64,        // Bots in reduced update mode
    pub bot_ai_dormant_mode: AtomicU64,        // Bots in dormant mode
    pub bot_ai_lod_scale: AtomicU64,           // LOD scale factor (x100, e.g., 100 = 1.0x)
    pub bot_ai_health_status: AtomicU64,       // Health status (0=Excellent, 4=Catastrophic)

    // Spectator metrics
    pub spectators_total: AtomicU64,              // Active spectator count
    pub spectators_full_view: AtomicU64,          // Watching whole map (no target)
    pub spectators_following: AtomicU64,          // Following a player/bot
    pub spectator_joins_total: AtomicU64,         // Counter: spectators joined
    pub spectator_conversions_total: AtomicU64,   // Counter: became players
    pub spectator_idle_evictions_total: AtomicU64,// Counter: kicked for inactivity
    pub spectator_disconnects_total: AtomicU64,   // Counter: voluntary disconnects

    // Tick phase timing (microseconds) - for bottleneck detection
    pub tick_phase_physics_us: AtomicU64,      // Physics integration time
    pub tick_phase_collision_us: AtomicU64,    // Collision detection time
    pub tick_phase_ai_us: AtomicU64,           // Bot AI update time
    pub tick_phase_broadcast_us: AtomicU64,    // State broadcast time

    // Entity lifecycle metrics
    pub spawn_players_total: AtomicU64,        // Player spawns (including respawns)
    pub spawn_projectiles_total: AtomicU64,    // Projectiles created
    pub kills_total: AtomicU64,                // Total kills
    pub deaths_arena_total: AtomicU64,         // Deaths from arena boundary

    // Network quality metrics
    pub network_write_failures_total: AtomicU64, // Failed network writes
    pub broadcast_latency_us: AtomicU64,         // Broadcast time in microseconds

    // Delta compression metrics
    pub delta_updates_sent: AtomicU64,           // Delta update messages sent
    pub full_updates_sent: AtomicU64,            // Full snapshot messages sent
    pub delta_bytes_saved: AtomicU64,            // Estimated bytes saved by delta compression

    // Distance-based rate limiting metrics
    pub updates_full_rate: AtomicU64,            // Entity updates at full rate (30Hz)
    pub updates_reduced_rate: AtomicU64,         // Entity updates at reduced rate (7.5Hz)
    pub updates_dormant_rate: AtomicU64,         // Entity updates at dormant rate (3.75Hz)
    pub updates_skipped_total: AtomicU64,        // Total updates skipped by rate limiting

    // Compression efficiency
    pub avg_delta_size_bytes: AtomicU64,         // Average delta message size
    pub avg_snapshot_size_bytes: AtomicU64,      // Average full snapshot size
    pub compression_ratio: AtomicU64,            // Delta size / Full size (x100 for percentage)

    // Rolling tick times for percentile calculation (VecDeque for O(1) pop_front)
    tick_history: RwLock<VecDeque<u64>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            total_players: AtomicU64::new(0),
            human_players: AtomicU64::new(0),
            bot_players: AtomicU64::new(0),
            alive_players: AtomicU64::new(0),
            projectile_count: AtomicU64::new(0),
            debris_count: AtomicU64::new(0),
            gravity_well_count: AtomicU64::new(0),
            tick_time_us: AtomicU64::new(0),
            tick_time_p95_us: AtomicU64::new(0),
            tick_time_p99_us: AtomicU64::new(0),
            tick_time_max_us: AtomicU64::new(0),
            performance_status: AtomicU64::new(0),
            budget_usage_percent: AtomicU64::new(0),
            tick_count: AtomicU64::new(0),
            connections_active: AtomicU64::new(0),
            messages_sent: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            match_time_seconds: AtomicU64::new(0),
            arena_scale: AtomicU64::new(100),
            arena_radius: AtomicU64::new(0),
            arena_target_radius: AtomicU64::new(0),
            arena_gravity_wells: AtomicU64::new(0),
            arena_wells_lerping: AtomicU64::new(0),
            arena_area_per_player: AtomicU64::new(0),
            start_time: Instant::now(),
            simulation_enabled: AtomicU64::new(0),
            simulation_target_bots: AtomicU64::new(0),
            simulation_cycle_progress: AtomicU64::new(0),
            // Extended metrics
            aoi_original_players: AtomicU64::new(0),
            aoi_filtered_players: AtomicU64::new(0),
            aoi_reduction_percent: AtomicU64::new(0),
            aoi_original_projectiles: AtomicU64::new(0),
            aoi_filtered_projectiles: AtomicU64::new(0),
            // Anti-cheat metrics
            anticheat_inputs_validated: AtomicU64::new(0),
            anticheat_inputs_rejected: AtomicU64::new(0),
            anticheat_inputs_sanitized: AtomicU64::new(0),
            anticheat_sequence_violations: AtomicU64::new(0),
            // DoS metrics
            dos_connections_rejected: AtomicU64::new(0),
            dos_messages_rate_limited: AtomicU64::new(0),
            dos_active_bans: AtomicU64::new(0),
            // AI Manager metrics
            ai_enabled: AtomicU64::new(0),
            ai_decisions_total: AtomicU64::new(0),
            ai_decisions_successful: AtomicU64::new(0),
            ai_last_confidence: AtomicU64::new(0),
            ai_pending_evaluations: AtomicU64::new(0),
            // Bot AI SoA metrics
            bot_ai_total: AtomicU64::new(0),
            bot_ai_active: AtomicU64::new(0),
            bot_ai_full_mode: AtomicU64::new(0),
            bot_ai_reduced_mode: AtomicU64::new(0),
            bot_ai_dormant_mode: AtomicU64::new(0),
            bot_ai_lod_scale: AtomicU64::new(100), // 1.0x default
            bot_ai_health_status: AtomicU64::new(0),
            // Spectator metrics
            spectators_total: AtomicU64::new(0),
            spectators_full_view: AtomicU64::new(0),
            spectators_following: AtomicU64::new(0),
            spectator_joins_total: AtomicU64::new(0),
            spectator_conversions_total: AtomicU64::new(0),
            spectator_idle_evictions_total: AtomicU64::new(0),
            spectator_disconnects_total: AtomicU64::new(0),
            // Tick phase timing
            tick_phase_physics_us: AtomicU64::new(0),
            tick_phase_collision_us: AtomicU64::new(0),
            tick_phase_ai_us: AtomicU64::new(0),
            tick_phase_broadcast_us: AtomicU64::new(0),
            // Entity lifecycle
            spawn_players_total: AtomicU64::new(0),
            spawn_projectiles_total: AtomicU64::new(0),
            kills_total: AtomicU64::new(0),
            deaths_arena_total: AtomicU64::new(0),
            // Network quality
            network_write_failures_total: AtomicU64::new(0),
            broadcast_latency_us: AtomicU64::new(0),
            // Delta compression
            delta_updates_sent: AtomicU64::new(0),
            full_updates_sent: AtomicU64::new(0),
            delta_bytes_saved: AtomicU64::new(0),
            // Rate limiting
            updates_full_rate: AtomicU64::new(0),
            updates_reduced_rate: AtomicU64::new(0),
            updates_dormant_rate: AtomicU64::new(0),
            updates_skipped_total: AtomicU64::new(0),
            // Compression efficiency
            avg_delta_size_bytes: AtomicU64::new(0),
            avg_snapshot_size_bytes: AtomicU64::new(0),
            compression_ratio: AtomicU64::new(0),
            tick_history: RwLock::new(VecDeque::with_capacity(1000)),
        }
    }

    /// Record a tick time and update percentiles
    pub fn record_tick_time(&self, duration: Duration) {
        let us = duration.as_micros() as u64;
        self.tick_time_us.store(us, Ordering::Relaxed);
        self.tick_count.fetch_add(1, Ordering::Relaxed);

        // Update rolling history for percentiles
        let mut history = self.tick_history.write();
        history.push_back(us);

        // Keep last 1000 samples - O(1) with VecDeque
        while history.len() > 1000 {
            history.pop_front();
        }

        // Calculate percentiles
        if history.len() >= 10 {
            let mut sorted: Vec<u64> = history.iter().copied().collect();
            sorted.sort_unstable();

            let p95_idx = (sorted.len() as f32 * 0.95) as usize;
            let p99_idx = (sorted.len() as f32 * 0.99) as usize;

            self.tick_time_p95_us.store(sorted[p95_idx.min(sorted.len() - 1)], Ordering::Relaxed);
            self.tick_time_p99_us.store(sorted[p99_idx.min(sorted.len() - 1)], Ordering::Relaxed);
            self.tick_time_max_us.store(sorted.last().copied().unwrap_or(0), Ordering::Relaxed);
        }
    }

    /// Get uptime in seconds
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Generate Prometheus-format metrics output
    pub fn to_prometheus(&self) -> String {
        let mut output = String::with_capacity(4096);

        // Helper macro for metrics
        macro_rules! metric {
            ($name:expr, $help:expr, $type:expr, $value:expr) => {
                output.push_str(&format!(
                    "# HELP {} {}\n# TYPE {} {}\n{} {}\n",
                    $name, $help, $name, $type, $name, $value
                ));
            };
        }

        // Player metrics
        metric!("orbit_royale_players_total", "Total number of players", "gauge",
            self.total_players.load(Ordering::Relaxed));
        metric!("orbit_royale_players_human", "Number of human players", "gauge",
            self.human_players.load(Ordering::Relaxed));
        metric!("orbit_royale_players_bot", "Number of bot players", "gauge",
            self.bot_players.load(Ordering::Relaxed));
        metric!("orbit_royale_players_alive", "Number of alive players", "gauge",
            self.alive_players.load(Ordering::Relaxed));

        // Entity metrics
        metric!("orbit_royale_projectiles", "Number of active projectiles", "gauge",
            self.projectile_count.load(Ordering::Relaxed));
        metric!("orbit_royale_debris", "Number of debris entities", "gauge",
            self.debris_count.load(Ordering::Relaxed));
        metric!("orbit_royale_gravity_wells", "Number of gravity wells", "gauge",
            self.gravity_well_count.load(Ordering::Relaxed));

        // Performance metrics
        metric!("orbit_royale_tick_time_microseconds", "Current tick time in microseconds", "gauge",
            self.tick_time_us.load(Ordering::Relaxed));
        metric!("orbit_royale_tick_time_p95_microseconds", "95th percentile tick time", "gauge",
            self.tick_time_p95_us.load(Ordering::Relaxed));
        metric!("orbit_royale_tick_time_p99_microseconds", "99th percentile tick time", "gauge",
            self.tick_time_p99_us.load(Ordering::Relaxed));
        metric!("orbit_royale_tick_time_max_microseconds", "Maximum tick time", "gauge",
            self.tick_time_max_us.load(Ordering::Relaxed));
        metric!("orbit_royale_tick_count", "Total ticks processed", "counter",
            self.tick_count.load(Ordering::Relaxed));

        // Budget metrics
        metric!("orbit_royale_performance_status", "Performance status (0=Excellent, 4=Catastrophic)", "gauge",
            self.performance_status.load(Ordering::Relaxed));
        metric!("orbit_royale_budget_usage_percent", "Tick budget usage percentage", "gauge",
            self.budget_usage_percent.load(Ordering::Relaxed));

        // Human-readable performance status as a label
        let status_name = match self.performance_status.load(Ordering::Relaxed) {
            0 => "excellent",
            1 => "good",
            2 => "warning",
            3 => "critical",
            _ => "catastrophic",
        };
        output.push_str(&format!(
            "# HELP orbit_royale_performance_state Human-readable performance state\n# TYPE orbit_royale_performance_state gauge\norbit_royale_performance_state{{state=\"{}\"}} 1\n",
            status_name
        ));

        // Network metrics
        metric!("orbit_royale_connections_active", "Active WebTransport connections", "gauge",
            self.connections_active.load(Ordering::Relaxed));
        metric!("orbit_royale_messages_sent_total", "Total messages sent", "counter",
            self.messages_sent.load(Ordering::Relaxed));
        metric!("orbit_royale_messages_received_total", "Total messages received", "counter",
            self.messages_received.load(Ordering::Relaxed));
        metric!("orbit_royale_bytes_sent_total", "Total bytes sent", "counter",
            self.bytes_sent.load(Ordering::Relaxed));
        metric!("orbit_royale_bytes_received_total", "Total bytes received", "counter",
            self.bytes_received.load(Ordering::Relaxed));

        // Game state
        metric!("orbit_royale_match_time_seconds", "Current match time", "gauge",
            self.match_time_seconds.load(Ordering::Relaxed));
        metric!("orbit_royale_arena_scale", "Arena scale factor (x100)", "gauge",
            self.arena_scale.load(Ordering::Relaxed));
        metric!("orbit_royale_arena_radius", "Arena escape radius in world units", "gauge",
            self.arena_radius.load(Ordering::Relaxed));
        metric!("orbit_royale_arena_target_radius", "Target arena radius before lerping", "gauge",
            self.arena_target_radius.load(Ordering::Relaxed));
        metric!("orbit_royale_arena_gravity_wells", "Number of gravity wells", "gauge",
            self.arena_gravity_wells.load(Ordering::Relaxed));
        metric!("orbit_royale_arena_wells_lerping", "Wells currently moving toward target position", "gauge",
            self.arena_wells_lerping.load(Ordering::Relaxed));
        metric!("orbit_royale_arena_area_per_player", "Actual area per player in square units", "gauge",
            self.arena_area_per_player.load(Ordering::Relaxed));
        metric!("orbit_royale_uptime_seconds", "Server uptime in seconds", "counter",
            self.uptime_seconds());

        // Simulation mode metrics
        metric!("orbit_royale_simulation_enabled", "Simulation mode enabled (0/1)", "gauge",
            self.simulation_enabled.load(Ordering::Relaxed));
        metric!("orbit_royale_simulation_target_bots", "Current target bot count in simulation", "gauge",
            self.simulation_target_bots.load(Ordering::Relaxed));
        metric!("orbit_royale_simulation_cycle_progress", "Simulation cycle progress (0-100%)", "gauge",
            self.simulation_cycle_progress.load(Ordering::Relaxed));

        // Extended metrics (feature-gated)
        #[cfg(feature = "metrics_extended")]
        {
            // AOI filtering metrics
            metric!("orbit_royale_aoi_original_players", "Players before AOI filtering", "gauge",
                self.aoi_original_players.load(Ordering::Relaxed));
            metric!("orbit_royale_aoi_filtered_players", "Players after AOI filtering", "gauge",
                self.aoi_filtered_players.load(Ordering::Relaxed));
            metric!("orbit_royale_aoi_reduction_percent", "AOI bandwidth reduction percentage", "gauge",
                self.aoi_reduction_percent.load(Ordering::Relaxed));
            metric!("orbit_royale_aoi_original_projectiles", "Projectiles before filtering", "gauge",
                self.aoi_original_projectiles.load(Ordering::Relaxed));
            metric!("orbit_royale_aoi_filtered_projectiles", "Projectiles after filtering", "gauge",
                self.aoi_filtered_projectiles.load(Ordering::Relaxed));

            // Anti-cheat metrics
            metric!("orbit_royale_anticheat_inputs_validated", "Total inputs validated", "counter",
                self.anticheat_inputs_validated.load(Ordering::Relaxed));
            metric!("orbit_royale_anticheat_inputs_rejected", "Inputs rejected as invalid", "counter",
                self.anticheat_inputs_rejected.load(Ordering::Relaxed));
            metric!("orbit_royale_anticheat_inputs_sanitized", "Inputs sanitized", "counter",
                self.anticheat_inputs_sanitized.load(Ordering::Relaxed));
            metric!("orbit_royale_anticheat_sequence_violations", "Sequence validation failures", "counter",
                self.anticheat_sequence_violations.load(Ordering::Relaxed));

            // DoS protection metrics
            metric!("orbit_royale_dos_connections_rejected", "Connections rejected by DoS protection", "counter",
                self.dos_connections_rejected.load(Ordering::Relaxed));
            metric!("orbit_royale_dos_messages_rate_limited", "Messages dropped by rate limit", "counter",
                self.dos_messages_rate_limited.load(Ordering::Relaxed));
            metric!("orbit_royale_dos_active_bans", "Currently active IP bans", "gauge",
                self.dos_active_bans.load(Ordering::Relaxed));
        }

        // AI Manager metrics
        #[cfg(feature = "ai_manager")]
        {
            metric!("orbit_royale_ai_enabled", "AI manager enabled (0/1)", "gauge",
                self.ai_enabled.load(Ordering::Relaxed));
            metric!("orbit_royale_ai_decisions_total", "Total AI decisions made", "counter",
                self.ai_decisions_total.load(Ordering::Relaxed));
            metric!("orbit_royale_ai_decisions_successful", "Successful AI decisions", "counter",
                self.ai_decisions_successful.load(Ordering::Relaxed));
            metric!("orbit_royale_ai_last_confidence", "Last AI decision confidence (0-100)", "gauge",
                self.ai_last_confidence.load(Ordering::Relaxed));
            metric!("orbit_royale_ai_pending_evaluations", "Decisions awaiting outcome evaluation", "gauge",
                self.ai_pending_evaluations.load(Ordering::Relaxed));

            // Success rate as percentage (calculated)
            let total = self.ai_decisions_total.load(Ordering::Relaxed);
            let successful = self.ai_decisions_successful.load(Ordering::Relaxed);
            let success_rate = if total > 0 { (successful * 100) / total } else { 0 };
            metric!("orbit_royale_ai_success_rate_percent", "AI decision success rate", "gauge", success_rate);
        }

        // Bot AI SoA metrics (always enabled - core system)
        metric!("orbit_royale_bot_ai_total", "Total bots registered in SoA AI system", "gauge",
            self.bot_ai_total.load(Ordering::Relaxed));
        metric!("orbit_royale_bot_ai_active", "Bots actively updating this tick", "gauge",
            self.bot_ai_active.load(Ordering::Relaxed));
        metric!("orbit_royale_bot_ai_full_mode", "Bots in full update mode (near humans)", "gauge",
            self.bot_ai_full_mode.load(Ordering::Relaxed));
        metric!("orbit_royale_bot_ai_reduced_mode", "Bots in reduced update mode", "gauge",
            self.bot_ai_reduced_mode.load(Ordering::Relaxed));
        metric!("orbit_royale_bot_ai_dormant_mode", "Bots in dormant mode (far from humans)", "gauge",
            self.bot_ai_dormant_mode.load(Ordering::Relaxed));

        // LOD scale (stored as x100, display as float)
        let lod_scale = self.bot_ai_lod_scale.load(Ordering::Relaxed);
        output.push_str(&format!(
            "# HELP orbit_royale_bot_ai_lod_scale Adaptive LOD scale factor (1.0 = normal, <1.0 = aggressive dormancy)\n# TYPE orbit_royale_bot_ai_lod_scale gauge\norbit_royale_bot_ai_lod_scale {:.2}\n",
            lod_scale as f32 / 100.0
        ));

        metric!("orbit_royale_bot_ai_health_status", "Bot AI health status (0=Excellent, 4=Catastrophic)", "gauge",
            self.bot_ai_health_status.load(Ordering::Relaxed));

        // Human-readable health status label
        let health_name = match self.bot_ai_health_status.load(Ordering::Relaxed) {
            0 => "excellent",
            1 => "good",
            2 => "warning",
            3 => "critical",
            _ => "catastrophic",
        };
        output.push_str(&format!(
            "# HELP orbit_royale_bot_ai_health_state Human-readable bot AI health state\n# TYPE orbit_royale_bot_ai_health_state gauge\norbit_royale_bot_ai_health_state{{state=\"{}\"}} 1\n",
            health_name
        ));

        // Spectator metrics
        metric!("orbit_royale_spectators_total", "Active spectator count", "gauge",
            self.spectators_total.load(Ordering::Relaxed));
        metric!("orbit_royale_spectators_full_view", "Spectators watching whole map", "gauge",
            self.spectators_full_view.load(Ordering::Relaxed));
        metric!("orbit_royale_spectators_following", "Spectators following a player", "gauge",
            self.spectators_following.load(Ordering::Relaxed));
        metric!("orbit_royale_spectator_joins_total", "Total spectator joins", "counter",
            self.spectator_joins_total.load(Ordering::Relaxed));
        metric!("orbit_royale_spectator_conversions_total", "Spectators converted to players", "counter",
            self.spectator_conversions_total.load(Ordering::Relaxed));
        metric!("orbit_royale_spectator_idle_evictions_total", "Spectators kicked for inactivity", "counter",
            self.spectator_idle_evictions_total.load(Ordering::Relaxed));
        metric!("orbit_royale_spectator_disconnects_total", "Spectators voluntarily disconnected", "counter",
            self.spectator_disconnects_total.load(Ordering::Relaxed));

        // Tick phase timing metrics (for bottleneck detection)
        metric!("orbit_royale_tick_phase_physics_microseconds", "Physics integration time", "gauge",
            self.tick_phase_physics_us.load(Ordering::Relaxed));
        metric!("orbit_royale_tick_phase_collision_microseconds", "Collision detection time", "gauge",
            self.tick_phase_collision_us.load(Ordering::Relaxed));
        metric!("orbit_royale_tick_phase_ai_microseconds", "Bot AI update time", "gauge",
            self.tick_phase_ai_us.load(Ordering::Relaxed));
        metric!("orbit_royale_tick_phase_broadcast_microseconds", "State broadcast time", "gauge",
            self.tick_phase_broadcast_us.load(Ordering::Relaxed));

        // Entity lifecycle metrics
        metric!("orbit_royale_spawn_players_total", "Total player spawns", "counter",
            self.spawn_players_total.load(Ordering::Relaxed));
        metric!("orbit_royale_spawn_projectiles_total", "Total projectiles created", "counter",
            self.spawn_projectiles_total.load(Ordering::Relaxed));
        metric!("orbit_royale_kills_total", "Total kills", "counter",
            self.kills_total.load(Ordering::Relaxed));
        metric!("orbit_royale_deaths_arena_total", "Deaths from arena boundary", "counter",
            self.deaths_arena_total.load(Ordering::Relaxed));

        // Network quality metrics
        metric!("orbit_royale_network_write_failures_total", "Failed network writes", "counter",
            self.network_write_failures_total.load(Ordering::Relaxed));
        metric!("orbit_royale_broadcast_latency_microseconds", "Broadcast time", "gauge",
            self.broadcast_latency_us.load(Ordering::Relaxed));

        // Delta compression metrics
        metric!("orbit_royale_delta_updates_sent", "Delta update messages sent", "counter",
            self.delta_updates_sent.load(Ordering::Relaxed));
        metric!("orbit_royale_full_updates_sent", "Full snapshot messages sent", "counter",
            self.full_updates_sent.load(Ordering::Relaxed));
        metric!("orbit_royale_delta_bytes_saved", "Bytes saved by delta compression", "counter",
            self.delta_bytes_saved.load(Ordering::Relaxed));

        // Rate limiting metrics
        metric!("orbit_royale_updates_full_rate", "Entity updates at full rate (30Hz)", "counter",
            self.updates_full_rate.load(Ordering::Relaxed));
        metric!("orbit_royale_updates_reduced_rate", "Entity updates at reduced rate (7.5Hz)", "counter",
            self.updates_reduced_rate.load(Ordering::Relaxed));
        metric!("orbit_royale_updates_dormant_rate", "Entity updates at dormant rate (3.75Hz)", "counter",
            self.updates_dormant_rate.load(Ordering::Relaxed));
        metric!("orbit_royale_updates_skipped_total", "Updates skipped by rate limiting", "counter",
            self.updates_skipped_total.load(Ordering::Relaxed));

        // Compression efficiency
        metric!("orbit_royale_avg_delta_size_bytes", "Average delta message size", "gauge",
            self.avg_delta_size_bytes.load(Ordering::Relaxed));
        metric!("orbit_royale_avg_snapshot_size_bytes", "Average full snapshot size", "gauge",
            self.avg_snapshot_size_bytes.load(Ordering::Relaxed));
        metric!("orbit_royale_compression_ratio", "Delta/Full size ratio (x100)", "gauge",
            self.compression_ratio.load(Ordering::Relaxed));

        output
    }

    /// Generate JSON format metrics (alternative for direct API access)
    pub fn to_json(&self) -> String {
        format!(r#"{{
  "players": {{
    "total": {},
    "human": {},
    "bot": {},
    "alive": {}
  }},
  "entities": {{
    "projectiles": {},
    "debris": {},
    "gravity_wells": {}
  }},
  "performance": {{
    "tick_time_us": {},
    "tick_time_p95_us": {},
    "tick_time_p99_us": {},
    "tick_time_max_us": {},
    "tick_count": {},
    "status": {},
    "status_name": "{}",
    "budget_percent": {}
  }},
  "network": {{
    "connections": {},
    "messages_sent": {},
    "messages_received": {},
    "bytes_sent": {},
    "bytes_received": {}
  }},
  "game": {{
    "match_time_seconds": {},
    "arena_scale": {},
    "uptime_seconds": {}
  }},
  "spectators": {{
    "total": {},
    "full_view": {},
    "following": {},
    "joins_total": {},
    "conversions_total": {}
  }},
  "tick_phases": {{
    "physics_us": {},
    "collision_us": {},
    "ai_us": {},
    "broadcast_us": {}
  }}
}}"#,
            self.total_players.load(Ordering::Relaxed),
            self.human_players.load(Ordering::Relaxed),
            self.bot_players.load(Ordering::Relaxed),
            self.alive_players.load(Ordering::Relaxed),
            self.projectile_count.load(Ordering::Relaxed),
            self.debris_count.load(Ordering::Relaxed),
            self.gravity_well_count.load(Ordering::Relaxed),
            self.tick_time_us.load(Ordering::Relaxed),
            self.tick_time_p95_us.load(Ordering::Relaxed),
            self.tick_time_p99_us.load(Ordering::Relaxed),
            self.tick_time_max_us.load(Ordering::Relaxed),
            self.tick_count.load(Ordering::Relaxed),
            self.performance_status.load(Ordering::Relaxed),
            match self.performance_status.load(Ordering::Relaxed) {
                0 => "excellent",
                1 => "good",
                2 => "warning",
                3 => "critical",
                _ => "catastrophic",
            },
            self.budget_usage_percent.load(Ordering::Relaxed),
            self.connections_active.load(Ordering::Relaxed),
            self.messages_sent.load(Ordering::Relaxed),
            self.messages_received.load(Ordering::Relaxed),
            self.bytes_sent.load(Ordering::Relaxed),
            self.bytes_received.load(Ordering::Relaxed),
            self.match_time_seconds.load(Ordering::Relaxed),
            self.arena_scale.load(Ordering::Relaxed) as f32 / 100.0,
            self.uptime_seconds(),
            // Spectator metrics
            self.spectators_total.load(Ordering::Relaxed),
            self.spectators_full_view.load(Ordering::Relaxed),
            self.spectators_following.load(Ordering::Relaxed),
            self.spectator_joins_total.load(Ordering::Relaxed),
            self.spectator_conversions_total.load(Ordering::Relaxed),
            // Tick phase timing
            self.tick_phase_physics_us.load(Ordering::Relaxed),
            self.tick_phase_collision_us.load(Ordering::Relaxed),
            self.tick_phase_ai_us.load(Ordering::Relaxed),
            self.tick_phase_broadcast_us.load(Ordering::Relaxed),
        )
    }

}

/// AI Manager metrics for JSON endpoint
#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct AIManagerMetrics {
    pub enabled: bool,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_evaluation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_evaluation: Option<String>,
    pub decisions_made: u64,
    pub success_rate: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_confidence: Option<f32>,
    pub recent_decisions: Vec<AIDecisionSummary>,
    pub pending_evaluations: u64,
}

/// Summary of an AI decision for metrics display
#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct AIDecisionSummary {
    pub id: String,
    pub timestamp: String,
    pub analysis: String,
    pub actions: Vec<AIActionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<AIOutcomeSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct AIActionSummary {
    pub parameter: String,
    pub old_value: f32,
    pub new_value: f32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct AIOutcomeSummary {
    pub success: bool,
    pub performance_delta_us: i64,
    pub player_delta: i32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Start the metrics HTTP server
pub async fn start_metrics_server(metrics: Arc<Metrics>, port: u16) -> anyhow::Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;

    info!("Metrics server listening on http://{}/metrics", addr);

    loop {
        let (mut socket, peer) = listener.accept().await?;
        let metrics = metrics.clone();

        tokio::spawn(async move {
            let mut buffer = [0u8; 1024];

            match socket.read(&mut buffer).await {
                Ok(n) if n > 0 => {
                    let request = String::from_utf8_lossy(&buffer[..n]);

                    // Parse the request line
                    let response = if request.starts_with("GET /metrics") {
                        let body = metrics.to_prometheus();
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    } else if request.starts_with("GET /metrics/json") || request.starts_with("GET /json") {
                        let body = metrics.to_json();
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    } else if request.starts_with("GET /health") || request.starts_with("GET /") {
                        let body = "OK";
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    } else {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                    };

                    if let Err(e) = socket.write_all(response.as_bytes()).await {
                        debug!("Failed to write metrics response to {}: {}", peer, e);
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    debug!("Failed to read from metrics socket {}: {}", peer, e);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_new() {
        let metrics = Metrics::new();
        assert_eq!(metrics.total_players.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.tick_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_record_tick_time() {
        let metrics = Metrics::new();

        // Record some tick times
        for i in 0..100 {
            metrics.record_tick_time(Duration::from_micros(100 + i * 10));
        }

        assert_eq!(metrics.tick_count.load(Ordering::Relaxed), 100);
        assert!(metrics.tick_time_p95_us.load(Ordering::Relaxed) > 0);
        assert!(metrics.tick_time_p99_us.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn test_prometheus_format() {
        let metrics = Metrics::new();
        metrics.total_players.store(50, Ordering::Relaxed);
        metrics.human_players.store(5, Ordering::Relaxed);
        metrics.bot_players.store(45, Ordering::Relaxed);

        let output = metrics.to_prometheus();

        assert!(output.contains("orbit_royale_players_total 50"));
        assert!(output.contains("orbit_royale_players_human 5"));
        assert!(output.contains("orbit_royale_players_bot 45"));
        assert!(output.contains("# HELP"));
        assert!(output.contains("# TYPE"));
    }

    #[test]
    fn test_json_format() {
        let metrics = Metrics::new();
        metrics.total_players.store(100, Ordering::Relaxed);

        let output = metrics.to_json();

        assert!(output.contains("\"total\": 100"));
        assert!(output.contains("\"players\":"));
        assert!(output.contains("\"performance\":"));
    }

    #[test]
    fn test_uptime() {
        let metrics = Metrics::new();
        std::thread::sleep(Duration::from_millis(10));
        // uptime_seconds() returns u64 so it's always >= 0
        // Just verify it returns a reasonable value (small after short sleep)
        assert!(metrics.uptime_seconds() < 60);
    }

    #[test]
    fn test_ai_manager_metrics() {
        let metrics = Metrics::new();

        // Initially all AI metrics should be zero
        assert_eq!(metrics.ai_enabled.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.ai_decisions_total.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.ai_decisions_successful.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.ai_last_confidence.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.ai_pending_evaluations.load(Ordering::Relaxed), 0);

        // Simulate AI manager enabling
        metrics.ai_enabled.store(1, Ordering::Relaxed);
        assert_eq!(metrics.ai_enabled.load(Ordering::Relaxed), 1);

        // Simulate a decision being made
        metrics.ai_decisions_total.fetch_add(1, Ordering::Relaxed);
        metrics.ai_last_confidence.store(85, Ordering::Relaxed); // 85% confidence
        assert_eq!(metrics.ai_decisions_total.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.ai_last_confidence.load(Ordering::Relaxed), 85);

        // Simulate successful outcome
        metrics.ai_decisions_successful.fetch_add(1, Ordering::Relaxed);
        assert_eq!(metrics.ai_decisions_successful.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_ai_metrics_in_prometheus() {
        let metrics = Metrics::new();
        metrics.ai_enabled.store(1, Ordering::Relaxed);
        metrics.ai_decisions_total.store(10, Ordering::Relaxed);
        metrics.ai_decisions_successful.store(8, Ordering::Relaxed);
        metrics.ai_last_confidence.store(92, Ordering::Relaxed);

        #[allow(unused_variables)]
        let output = metrics.to_prometheus();

        // AI metrics should appear in Prometheus output (when feature enabled)
        #[cfg(feature = "ai_manager")]
        {
            assert!(output.contains("orbit_royale_ai_enabled 1"));
            assert!(output.contains("orbit_royale_ai_decisions_total 10"));
            assert!(output.contains("orbit_royale_ai_decisions_successful 8"));
            assert!(output.contains("orbit_royale_ai_last_confidence 92"));
            assert!(output.contains("orbit_royale_ai_success_rate_percent 80")); // 8/10 = 80%
        }
    }
}
