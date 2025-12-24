//! Game session manager - runs the game loop and broadcasts state to players

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tokio::time::{interval, Instant};
use tracing::{debug, info, warn};

use crate::config::{DebrisSpawnConfig, GravityWaveConfig};
use crate::game::constants::{ai, physics};
use crate::game::game_loop::{GameLoop, GameLoopConfig, GameLoopEvent};
use crate::game::performance::{PerformanceMonitor, PerformanceStatus};
use crate::game::state::{MatchPhase, Player, PlayerId};
use crate::metrics::Metrics;
use crate::net::aoi::{AOIConfig, AOIManager};
use crate::net::protocol::{encode, GameEvent, GameSnapshot, PlayerInput, ServerMessage};

/// Simulation mode configuration for load testing
/// Scales bots up and down over time in a sinusoidal pattern
#[derive(Debug, Clone)]
pub struct SimulationConfig {
    /// Whether simulation mode is enabled
    pub enabled: bool,
    /// Minimum number of bots
    pub min_bots: usize,
    /// Maximum number of bots
    pub max_bots: usize,
    /// Duration of one full cycle (min → max → min) in seconds
    pub cycle_duration_secs: f32,
}

impl SimulationConfig {
    /// Load simulation config from environment variables
    pub fn from_env() -> Self {
        let enabled = std::env::var("SIMULATION_MODE")
            .map(|s| s.to_lowercase() == "true" || s == "1")
            .unwrap_or(false);

        let min_bots = std::env::var("SIMULATION_MIN_BOTS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);

        let max_bots = std::env::var("SIMULATION_MAX_BOTS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);

        let cycle_minutes: f32 = std::env::var("SIMULATION_CYCLE_MINUTES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5.0);

        Self {
            enabled,
            min_bots,
            max_bots,
            cycle_duration_secs: cycle_minutes * 60.0,
        }
    }

    /// Calculate target bot count based on elapsed time
    /// Uses sinusoidal wave: starts at min, goes to max at half cycle, back to min
    pub fn target_bots(&self, elapsed_secs: f32) -> usize {
        if !self.enabled {
            return self.max_bots;
        }

        // Use cosine for smooth min→max→min cycle
        // cos(0) = 1 (min), cos(π) = -1 (max), cos(2π) = 1 (min)
        let phase = (elapsed_secs / self.cycle_duration_secs) * std::f32::consts::TAU;
        let normalized = (1.0 - phase.cos()) / 2.0; // 0.0 at start, 1.0 at half, 0.0 at end

        let range = self.max_bots - self.min_bots;
        self.min_bots + (normalized * range as f32) as usize
    }
}

/// A connected player's stream writer for sending messages
pub struct PlayerConnection {
    pub player_id: PlayerId,
    pub player_name: String,
    pub writer: Arc<RwLock<Option<wtransport::SendStream>>>,
}

/// Shared game session that manages the game loop and player connections
pub struct GameSession {
    pub game_loop: GameLoop,
    pub players: HashMap<PlayerId, PlayerConnection>,
    pub performance: PerformanceMonitor,
    pub aoi_manager: AOIManager,
    pub metrics: Option<Arc<Metrics>>,
    last_snapshot_tick: u64,
    bot_count: usize,
    /// Simulation mode configuration
    simulation_config: SimulationConfig,
    /// When the session started (for simulation timing)
    session_start: std::time::Instant,
    /// Last tick when simulation target was updated (rate limiting)
    last_simulation_update_tick: u64,
    /// Last client timestamp per player for RTT echo
    last_client_times: HashMap<PlayerId, u64>,
}

impl GameSession {
    /// Create a new game session without metrics
    pub fn new() -> Self {
        Self::new_with_metrics_opt(None)
    }

    /// Create a new game session with metrics collection
    pub fn new_with_metrics(metrics: Arc<Metrics>) -> Self {
        Self::new_with_metrics_opt(Some(metrics))
    }

    fn new_with_metrics_opt(metrics: Option<Arc<Metrics>>) -> Self {
        // Load simulation config from environment
        let simulation_config = SimulationConfig::from_env();

        // Determine initial bot count
        let bot_count = if simulation_config.enabled {
            // In simulation mode, start at minimum bots but configure arena for max
            // This ensures gravity wells are properly distributed for the full scale
            info!(
                "Simulation mode ENABLED: {} → {} bots over {} minutes",
                simulation_config.min_bots,
                simulation_config.max_bots,
                simulation_config.cycle_duration_secs / 60.0
            );
            simulation_config.min_bots
        } else {
            // Normal mode: read from BOT_COUNT env or default
            let count = std::env::var("BOT_COUNT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(ai::COUNT);
            info!("Bot count set to {}", count);
            count
        };

        // Load configs from environment
        let gravity_wave_config = GravityWaveConfig::from_env();
        let debris_spawn_config = DebrisSpawnConfig::from_env();

        let mut game_loop = GameLoop::new(GameLoopConfig {
            gravity_wave_config,
            debris_spawn_config: debris_spawn_config.clone(),
            ..GameLoopConfig::default()
        });

        // Start in Playing phase immediately (no waiting/countdown)
        game_loop.state_mut().match_state.phase = MatchPhase::Playing;
        game_loop.state_mut().match_state.countdown_time = 0.0;

        // CRITICAL: Update arena scale BEFORE spawning bots
        // This creates appropriate gravity wells for the initial player count
        // As players join/leave, scale_for_simulation() handles dynamic scaling
        // and repositions wells that end up too close to center
        game_loop.state_mut().arena.update_for_player_count(bot_count);
        info!(
            "Arena configured: {} gravity wells, escape_radius={}",
            game_loop.state().arena.gravity_wells.len(),
            game_loop.state().arena.escape_radius
        );

        // Fill with bots initially
        game_loop.fill_with_bots(bot_count);

        // Spawn initial debris (since we skip countdown phase)
        if debris_spawn_config.enabled {
            use crate::game::systems::debris;
            debris::spawn_initial(game_loop.state_mut(), &debris_spawn_config);
            // Spawn debris around gravity wells (feeding zones)
            debris::spawn_around_wells(game_loop.state_mut(), &debris_spawn_config);
            info!(
                "Spawned {} initial debris particles (including well zones)",
                game_loop.state().debris.len()
            );
        }

        // Create AOI manager with view-based radii (not arena-based)
        // Camera zoom ranges 0.45x-1.0x, screen ~2000px diagonal = ~4500 world units at max zoom out
        // Using fixed values that work regardless of arena size
        let aoi_config = AOIConfig {
            full_detail_radius: 3000.0,   // Full detail for immediate viewport
            extended_radius: 6000.0,      // Extended for max zoom out (0.45x) + buffer
            max_entities: 150,            // Cap per client for performance
            always_include_top_n: 10,     // Always show top 10 players
        };
        info!(
            "AOI configured: full_detail={:.0}, extended={:.0}, max_entities={}, arena_radius={:.0}",
            aoi_config.full_detail_radius, aoi_config.extended_radius, aoi_config.max_entities,
            game_loop.state().arena.escape_radius
        );

        // Initialize metrics with current state
        if let Some(ref m) = metrics {
            m.total_players.store(bot_count as u64, Ordering::Relaxed);
            m.bot_players.store(bot_count as u64, Ordering::Relaxed);
            m.alive_players.store(bot_count as u64, Ordering::Relaxed);
            m.gravity_well_count.store(
                game_loop.state().arena.gravity_wells.len() as u64,
                Ordering::Relaxed,
            );
        }

        Self {
            game_loop,
            players: HashMap::new(),
            performance: PerformanceMonitor::new(physics::TICK_RATE),
            aoi_manager: AOIManager::new(aoi_config),
            metrics,
            last_snapshot_tick: 0,
            bot_count,
            simulation_config,
            session_start: std::time::Instant::now(),
            last_simulation_update_tick: 0,
            last_client_times: HashMap::new(),
        }
    }

    /// Check if we can accept a new player (performance-based admission control)
    pub fn can_accept_player(&self) -> bool {
        self.performance.can_accept_players()
    }

    /// Get rejection message for when server is at capacity
    pub fn rejection_message(&self) -> String {
        let player_count = self.game_loop.state().players.len();
        format!(
            "Server at capacity ({} players). Please try again later.",
            player_count
        )
    }

    /// Get current player count
    pub fn player_count(&self) -> usize {
        self.game_loop.state().players.len()
    }

    /// Add a player to the game session
    pub fn add_player(
        &mut self,
        player_id: PlayerId,
        player_name: String,
        color_index: u8,
        writer: Arc<RwLock<Option<wtransport::SendStream>>>,
    ) -> PlayerId {
        info!("Adding player {} ({}) to game session", player_name, player_id);

        // Create player entity with their selected color
        let player = Player::new(player_id, player_name.clone(), false, color_index);

        // Add to game loop
        self.game_loop.add_player(player);

        // Store connection
        self.players.insert(
            player_id,
            PlayerConnection {
                player_id,
                player_name,
                writer,
            },
        );

        // Update arena scaling based on new player count
        self.update_arena_scale();

        player_id
    }

    /// Remove a player from the game session
    pub fn remove_player(&mut self, player_id: PlayerId) {
        info!("Removing player {} from game session", player_id);
        self.game_loop.remove_player(player_id);
        self.players.remove(&player_id);
        self.last_client_times.remove(&player_id);

        // Ensure we have enough bots
        self.maintain_player_count();

        // Update arena scaling based on new player count
        self.update_arena_scale();
    }

    /// Update arena scale and gravity wells based on player count
    /// Uses smooth scaling to avoid regenerating all wells and causing chaos
    fn update_arena_scale(&mut self) {
        let player_count = self.game_loop.state().players.len();
        // Use smooth scaling that only adds wells incrementally (never removes/moves existing)
        self.game_loop
            .state_mut()
            .arena
            .scale_for_simulation(player_count);
    }

    /// Queue input for a player
    pub fn queue_input(&mut self, player_id: PlayerId, input: PlayerInput) {
        // Track client timestamp for RTT echo
        if input.client_time > 0 {
            self.last_client_times.insert(player_id, input.client_time);
        }
        self.game_loop.queue_input(player_id, input);
    }

    /// Run a game tick and return events
    pub fn tick(&mut self) -> Vec<GameLoopEvent> {
        // Start performance timing
        let tick_start = std::time::Instant::now();
        self.performance.tick_start();

        let events = self.game_loop.tick();

        // Continuously update arena scale for smooth lerping
        // (scale_for_simulation uses lerp factors that need per-tick updates)
        self.update_arena_scale();

        // Respawn dead players (humans always, bots only if performance allows)
        self.respawn_dead_players();

        // Update simulation bot target if in simulation mode
        self.update_simulation_bot_count();

        // Performance-based bot management
        // Only forcibly remove bots in catastrophic situations (>150% budget)
        // Otherwise, let natural attrition handle it by not respawning dead bots
        if self.performance.should_force_reduce() {
            // Catastrophic: remove one bot per tick to reduce load
            self.remove_one_bot();
        } else if self.performance.can_add_bots() {
            // Excellent/Good: maintain target bot count (spawns bots up to target)
            self.maintain_player_count();
            // In simulation mode, also clean up dead bots if we're over target
            if self.simulation_config.enabled {
                self.scale_down_bots_if_needed();
            }
        } else if self.simulation_config.enabled {
            // In simulation mode during Warning/Critical: still scale down if target is lower
            self.scale_down_bots_if_needed();
        }
        // Warning/Critical (non-simulation): do nothing - bots that die won't respawn, natural reduction

        // Keep game running forever - reset phase to Playing if it ended
        // This is an eternal game mode with no match end
        if self.game_loop.state().match_state.phase == MatchPhase::Ended {
            self.game_loop.state_mut().match_state.phase = MatchPhase::Playing;
        }

        // End performance timing
        let entity_count = self.game_loop.state().players.len()
            + self.game_loop.state().projectiles.len();
        self.performance.tick_end(entity_count);

        // Update metrics
        if let Some(ref metrics) = self.metrics {
            let tick_duration = tick_start.elapsed();
            metrics.record_tick_time(tick_duration);

            let state = self.game_loop.state();

            // Player counts
            let total = state.players.len() as u64;
            let bots = state.players.values().filter(|p| p.is_bot).count() as u64;
            let humans = total - bots;
            let alive = state.players.values().filter(|p| p.alive).count() as u64;

            metrics.total_players.store(total, Ordering::Relaxed);
            metrics.human_players.store(humans, Ordering::Relaxed);
            metrics.bot_players.store(bots, Ordering::Relaxed);
            metrics.alive_players.store(alive, Ordering::Relaxed);

            // Entity counts
            metrics.projectile_count.store(state.projectiles.len() as u64, Ordering::Relaxed);
            metrics.debris_count.store(state.debris.len() as u64, Ordering::Relaxed);
            metrics.gravity_well_count.store(
                state.arena.gravity_wells.len() as u64,
                Ordering::Relaxed,
            );

            // Performance status
            let status = match self.performance.status() {
                PerformanceStatus::Excellent => 0,
                PerformanceStatus::Good => 1,
                PerformanceStatus::Warning => 2,
                PerformanceStatus::Critical => 3,
                PerformanceStatus::Catastrophic => 4,
            };
            metrics.performance_status.store(status, Ordering::Relaxed);
            metrics.budget_usage_percent.store(
                self.performance.budget_usage_percent() as u64,
                Ordering::Relaxed,
            );

            // Game state
            metrics.match_time_seconds.store(
                state.match_state.match_time as u64,
                Ordering::Relaxed,
            );
            metrics.arena_scale.store(
                (state.arena.scale * 100.0) as u64,
                Ordering::Relaxed,
            );
            metrics.arena_radius.store(
                state.arena.escape_radius as u64,
                Ordering::Relaxed,
            );
            metrics.arena_gravity_wells.store(
                state.arena.gravity_wells.len() as u64,
                Ordering::Relaxed,
            );

            // Network (connection count)
            metrics.connections_active.store(self.players.len() as u64, Ordering::Relaxed);

            // Simulation metrics
            metrics.simulation_enabled.store(
                if self.simulation_config.enabled { 1 } else { 0 },
                Ordering::Relaxed,
            );
            if self.simulation_config.enabled {
                metrics.simulation_target_bots.store(self.bot_count as u64, Ordering::Relaxed);
                let elapsed = self.session_start.elapsed().as_secs_f32();
                let cycle_progress = ((elapsed % self.simulation_config.cycle_duration_secs)
                    / self.simulation_config.cycle_duration_secs * 100.0) as u64;
                metrics.simulation_cycle_progress.store(cycle_progress, Ordering::Relaxed);
            }
        }

        events
    }

    /// Remove one bot to reduce server load
    fn remove_one_bot(&mut self) {
        // Find a bot to remove (prefer dead bots, then any bot)
        let bot_to_remove = self
            .game_loop
            .state()
            .players
            .values()
            .filter(|p| p.is_bot)
            .min_by_key(|p| if p.alive { 1 } else { 0 })
            .map(|p| p.id);

        if let Some(bot_id) = bot_to_remove {
            self.game_loop.remove_player(bot_id);
            debug!("Removed bot {} due to performance pressure", bot_id);
        }
    }

    /// Update bot count target based on simulation mode timing
    /// Rate-limited to once per second to prevent chaos
    fn update_simulation_bot_count(&mut self) {
        if !self.simulation_config.enabled {
            return;
        }

        let current_tick = self.game_loop.state().tick;

        // Rate limit: only update target once per second (every 30 ticks at 30 TPS)
        if current_tick < self.last_simulation_update_tick + 30 {
            return;
        }
        self.last_simulation_update_tick = current_tick;

        let elapsed = self.session_start.elapsed().as_secs_f32();
        let target = self.simulation_config.target_bots(elapsed);

        // Only update if target changed significantly (±2 bots to reduce noise)
        if (target as i32 - self.bot_count as i32).abs() >= 2 {
            let old_target = self.bot_count;
            self.bot_count = target;

            info!(
                "Simulation: bot target {} → {} (elapsed: {:.1}s, cycle {:.1}%)",
                old_target,
                target,
                elapsed,
                (elapsed % self.simulation_config.cycle_duration_secs)
                    / self.simulation_config.cycle_duration_secs
                    * 100.0
            );
        }

        // Smoothly scale arena for current target (runs every update for smooth lerping)
        // This adjusts radii smoothly and adds wells incrementally without chaos
        self.game_loop
            .state_mut()
            .arena
            .scale_for_simulation(self.bot_count);
    }

    /// Scale down bots if current count exceeds target (used during simulation scale-down phase)
    /// ONLY removes dead bots - never kills alive bots to prevent flickering
    fn scale_down_bots_if_needed(&mut self) {
        let current_bot_count = self
            .game_loop
            .state()
            .players
            .values()
            .filter(|p| p.is_bot)
            .count();

        // Only scale down if we have excess bots
        if current_bot_count <= self.bot_count {
            return;
        }

        // Find DEAD bots to remove (never remove alive bots during simulation)
        let dead_bots: Vec<_> = self
            .game_loop
            .state()
            .players
            .values()
            .filter(|p| p.is_bot && !p.alive)
            .map(|p| p.id)
            .take(2) // Remove max 2 dead bots per tick
            .collect();

        for bot_id in dead_bots {
            self.game_loop.remove_player(bot_id);
            debug!("Simulation scale-down: removed dead bot {}", bot_id);
        }
    }

    /// Check if we should send a snapshot this tick
    pub fn should_send_snapshot(&self) -> bool {
        let current_tick = self.game_loop.state().tick;
        // Send snapshot every 3 ticks (30 Hz tick rate / 3 = 10 Hz snapshots)
        // Or use SNAPSHOT_RATE from constants for 20 Hz
        let ticks_per_snapshot = physics::TICK_RATE / 10; // 10 Hz for now
        current_tick > self.last_snapshot_tick &&
            (current_tick - self.last_snapshot_tick) >= ticks_per_snapshot as u64
    }

    /// Mark that a snapshot was sent
    pub fn mark_snapshot_sent(&mut self) {
        self.last_snapshot_tick = self.game_loop.state().tick;
    }

    /// Get current game snapshot (full, unfiltered)
    pub fn get_snapshot(&self) -> GameSnapshot {
        GameSnapshot::from_game_state(self.game_loop.state())
    }

    /// Get a filtered snapshot for a specific player using AOI
    pub fn get_filtered_snapshot(&self, player_id: PlayerId) -> GameSnapshot {
        let full_snapshot = self.get_snapshot();

        // Get player position and velocity for AOI filtering
        let (player_position, player_velocity) = self.game_loop.state()
            .get_player(player_id)
            .map(|p| (p.position, p.velocity))
            .unwrap_or((crate::util::vec2::Vec2::ZERO, crate::util::vec2::Vec2::ZERO));

        self.aoi_manager.filter_for_player(player_id, player_position, player_velocity, &full_snapshot)
    }

    /// Respawn dead players after respawn delay
    /// - Humans always respawn when timer expires
    /// - Bots only respawn if alive count is below minimum
    fn respawn_dead_players(&mut self) {
        use crate::game::constants::mass;
        use crate::game::systems::arena;

        let alive_count = self.game_loop.state().alive_count();
        let target = self.bot_count;

        // First, decrement respawn timers for all dead players
        let dt = physics::DT;
        for player in self.game_loop.state_mut().players.values_mut() {
            if !player.alive && player.respawn_timer > 0.0 {
                player.respawn_timer -= dt;
            }
        }

        // Get dead players whose respawn timer has expired, prioritizing humans
        let mut dead_players: Vec<(PlayerId, bool)> = self
            .game_loop
            .state()
            .players
            .values()
            .filter(|p| !p.alive && p.respawn_timer <= 0.0)
            .map(|p| (p.id, p.is_bot))
            .collect();

        // Sort so humans respawn first
        dead_players.sort_by_key(|(_, is_bot)| *is_bot);

        // Check performance status for respawn decisions
        let can_respawn_bots = self.performance.can_respawn_bots();
        let is_catastrophic = self.performance.should_force_reduce();

        let mut respawned = 0;
        for (player_id, is_bot) in dead_players {
            // Respawn logic:
            // - Catastrophic: NO respawns at all (wait for recovery)
            // - Critical: Humans respawn, bots don't
            // - Warning/Good/Excellent: Everyone respawns normally
            let should_respawn = if is_catastrophic {
                false // Server overloaded - wait for recovery
            } else if is_bot {
                can_respawn_bots && (alive_count + respawned) < target
            } else {
                true // Humans respawn unless catastrophic
            };

            if should_respawn {
                // Collect alive player positions and wells for safe spawn
                let alive_positions: Vec<_> = self
                    .game_loop
                    .state()
                    .players
                    .values()
                    .filter(|p| p.alive && p.id != player_id)
                    .map(|p| p.position)
                    .collect();
                let wells = self.game_loop.state().arena.gravity_wells.clone();

                if let Some(player) = self.game_loop.state_mut().get_player_mut(player_id) {
                    // Find safe spawn position near a gravity well, away from other players
                    player.position = arena::safe_spawn_near_well(&wells, &alive_positions);

                    // Use orbital velocity relative to nearest well
                    player.velocity = arena::spawn_velocity_for_well(player.position, &wells);

                    player.alive = true;
                    player.mass = mass::STARTING;
                    player.spawn_protection = crate::game::constants::spawn::PROTECTION_DURATION;
                    player.respawn_timer = 0.0;

                    respawned += 1;
                    debug!("Respawned player {}", player_id);
                }

                // Reset charge state to prevent stale charging state from before death
                self.game_loop.reset_charge(player_id);
            }
        }
    }

    /// Maintain minimum player count with bots
    /// Spawns bots ONE at a time (like humans) so arena scales naturally
    fn maintain_player_count(&mut self) {
        let current_count = self.game_loop.state().players.len();
        let target = self.bot_count;

        // Only add ONE bot per tick to simulate natural human join behavior
        // Arena scaling is handled by update_simulation_bot_count (once per second)
        // to avoid constant small updates that cause visual pulsing
        if current_count < target {
            self.game_loop.fill_with_bots(current_count + 1);
        }
    }
}

impl Default for GameSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Broadcast a message to all connected players (same message to all)
pub async fn broadcast_message(
    session: &GameSession,
    message: &ServerMessage,
) {
    let encoded = match encode(message) {
        Ok(data) => data,
        Err(e) => {
            warn!("Failed to encode message for broadcast: {}", e);
            return;
        }
    };

    let len_bytes = (encoded.len() as u32).to_le_bytes();
    let msg_len = encoded.len();

    for (player_id, conn) in session.players.iter() {
        let writer = conn.writer.clone();
        let len_bytes = len_bytes;
        let encoded = encoded.clone();
        let pid = *player_id;

        tokio::spawn(async move {
            if let Some(writer) = &mut *writer.write().await {
                match writer.write_all(&len_bytes).await {
                    Ok(_) => {}
                    Err(e) => {
                        warn!("Broadcast to {}: failed to write length: {}", pid, e);
                        return;
                    }
                }
                match writer.write_all(&encoded).await {
                    Ok(_) => {
                        debug!("Broadcast to {}: sent {} bytes", pid, msg_len);
                    }
                    Err(e) => {
                        warn!("Broadcast to {}: failed to write data: {}", pid, e);
                    }
                }
            } else {
                warn!("Broadcast to {}: writer is None", pid);
            }
        });
    }
}

/// Broadcast AOI-filtered snapshots to each player (per-client filtering)
/// Each player receives only entities relevant to their position
pub async fn broadcast_filtered_snapshots(session: &GameSession) {
    use rayon::prelude::*;

    // Get full snapshot once
    let full_snapshot = session.get_snapshot();

    // Prepare per-client data in parallel
    let client_data: Vec<(PlayerId, Arc<RwLock<Option<wtransport::SendStream>>>, Vec<u8>)> =
        session.players.iter()
            .filter_map(|(&player_id, conn)| {
                // Get player position and velocity for filtering
                let (player_position, player_velocity) = session.game_loop.state()
                    .get_player(player_id)
                    .map(|p| (p.position, p.velocity))
                    .unwrap_or((crate::util::vec2::Vec2::ZERO, crate::util::vec2::Vec2::ZERO));

                // Filter snapshot for this player (AOI expands based on velocity)
                let mut filtered = session.aoi_manager.filter_for_player(
                    player_id,
                    player_position,
                    player_velocity,
                    &full_snapshot,
                );

                // Set echo_client_time for RTT measurement
                filtered.echo_client_time = session.last_client_times
                    .get(&player_id)
                    .copied()
                    .unwrap_or(0);

                // Encode the message
                let message = ServerMessage::Snapshot(filtered);
                match encode(&message) {
                    Ok(encoded) => Some((player_id, conn.writer.clone(), encoded)),
                    Err(e) => {
                        warn!("Failed to encode snapshot for {}: {}", player_id, e);
                        None
                    }
                }
            })
            .collect();

    // Send to each client in parallel
    for (player_id, writer, encoded) in client_data {
        let len_bytes = (encoded.len() as u32).to_le_bytes();
        let msg_len = encoded.len();

        tokio::spawn(async move {
            if let Some(writer) = &mut *writer.write().await {
                if let Err(e) = writer.write_all(&len_bytes).await {
                    warn!("AOI broadcast to {}: failed to write length: {}", player_id, e);
                    return;
                }
                match writer.write_all(&encoded).await {
                    Ok(_) => {
                        debug!("AOI broadcast to {}: sent {} bytes", player_id, msg_len);
                    }
                    Err(e) => {
                        warn!("AOI broadcast to {}: failed to write data: {}", player_id, e);
                    }
                }
            }
        });
    }
}

/// Send a message to a specific player
pub async fn send_to_player(
    writer: &Arc<RwLock<Option<wtransport::SendStream>>>,
    message: &ServerMessage,
) -> Result<(), String> {
    let encoded = encode(message).map_err(|e| e.to_string())?;
    let len_bytes = (encoded.len() as u32).to_le_bytes();

    if let Some(writer) = &mut *writer.write().await {
        writer
            .write_all(&len_bytes)
            .await
            .map_err(|e| e.to_string())?;
        writer
            .write_all(&encoded)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("Writer not available".to_string())
    }
}

#[cfg(test)]
mod simulation_tests {
    use super::*;

    #[test]
    fn test_simulation_config_default() {
        // Without env vars, should be disabled
        let config = SimulationConfig {
            enabled: false,
            min_bots: 5,
            max_bots: 100,
            cycle_duration_secs: 300.0,
        };
        assert!(!config.enabled);
    }

    #[test]
    fn test_target_bots_disabled() {
        let config = SimulationConfig {
            enabled: false,
            min_bots: 5,
            max_bots: 100,
            cycle_duration_secs: 300.0,
        };
        // When disabled, always returns max
        assert_eq!(config.target_bots(0.0), 100);
        assert_eq!(config.target_bots(150.0), 100);
    }

    #[test]
    fn test_target_bots_at_start() {
        let config = SimulationConfig {
            enabled: true,
            min_bots: 10,
            max_bots: 100,
            cycle_duration_secs: 300.0, // 5 minutes
        };
        // At t=0, should be at minimum (cosine starts at 1, normalized to 0)
        assert_eq!(config.target_bots(0.0), 10);
    }

    #[test]
    fn test_target_bots_at_half_cycle() {
        let config = SimulationConfig {
            enabled: true,
            min_bots: 10,
            max_bots: 100,
            cycle_duration_secs: 300.0,
        };
        // At half cycle (150s), should be at maximum
        let target = config.target_bots(150.0);
        assert_eq!(target, 100);
    }

    #[test]
    fn test_target_bots_at_full_cycle() {
        let config = SimulationConfig {
            enabled: true,
            min_bots: 10,
            max_bots: 100,
            cycle_duration_secs: 300.0,
        };
        // At full cycle (300s), should be back at minimum
        assert_eq!(config.target_bots(300.0), 10);
    }

    #[test]
    fn test_target_bots_quarter_cycle() {
        let config = SimulationConfig {
            enabled: true,
            min_bots: 0,
            max_bots: 100,
            cycle_duration_secs: 300.0,
        };
        // At quarter cycle (75s), should be around 50 (halfway up)
        let target = config.target_bots(75.0);
        assert!(target >= 45 && target <= 55, "Expected ~50, got {}", target);
    }

    #[test]
    fn test_target_bots_wraps_around() {
        let config = SimulationConfig {
            enabled: true,
            min_bots: 10,
            max_bots: 100,
            cycle_duration_secs: 300.0,
        };
        // After one full cycle, pattern should repeat
        let t0 = config.target_bots(0.0);
        let t300 = config.target_bots(300.0);
        let t600 = config.target_bots(600.0);
        assert_eq!(t0, t300);
        assert_eq!(t0, t600);
    }
}

/// Sanitize player state to prevent NaN/Infinity corruption
fn sanitize_game_state(session: &mut GameSession) {
    for player in session.game_loop.state_mut().players.values_mut() {
        // Fix NaN/Infinity positions
        if !player.position.x.is_finite() || !player.position.y.is_finite() {
            warn!("Fixed NaN position for player {}", player.id);
            player.position = crate::game::systems::arena::random_spawn_position();
        }
        // Fix NaN/Infinity velocities
        if !player.velocity.x.is_finite() || !player.velocity.y.is_finite() {
            warn!("Fixed NaN velocity for player {}", player.id);
            player.velocity = crate::util::vec2::Vec2::ZERO;
        }
        // Fix NaN mass
        if !player.mass.is_finite() || player.mass <= 0.0 {
            warn!("Fixed invalid mass for player {}", player.id);
            player.mass = crate::game::constants::mass::STARTING;
        }
    }
}

/// Start the game loop background task
pub fn start_game_loop(session: Arc<RwLock<GameSession>>) {
    tokio::spawn(async move {
        let tick_duration = Duration::from_millis(physics::TICK_DURATION_MS);
        let mut ticker = interval(tick_duration);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        info!("Game loop started at {} Hz", physics::TICK_RATE);
        let start = Instant::now();
        let mut tick_count: u64 = 0;

        loop {
            ticker.tick().await;
            tick_count += 1;

            // Run game tick with error recovery
            let tick_result: Result<(Vec<GameLoopEvent>, Option<GameSnapshot>), String> = {
                let mut session_guard = session.write().await;

                // Sanitize state before tick to prevent NaN propagation
                sanitize_game_state(&mut session_guard);

                let events = session_guard.tick();

                // Sanitize again after tick
                sanitize_game_state(&mut session_guard);

                let snapshot = if session_guard.should_send_snapshot() {
                    session_guard.mark_snapshot_sent();
                    Some(session_guard.get_snapshot())
                } else {
                    None
                };
                Ok((events, snapshot))
            };

            let (events, snapshot) = match tick_result {
                Ok(result) => result,
                Err(e) => {
                    warn!("Game tick error: {}", e);
                    continue;
                }
            };

            // Log kill events only
            for event in &events {
                if let GameLoopEvent::PlayerKilled { killer_id, victim_id } = event {
                    debug!("Player {:?} killed {:?}", killer_id, victim_id);
                }
            }

            // Broadcast game events to all players
            for event in &events {
                let game_event = match event {
                    GameLoopEvent::PlayerDeflection { player_a, player_b, position, intensity } => {
                        Some(GameEvent::PlayerDeflection {
                            player_a: *player_a,
                            player_b: *player_b,
                            position: *position,
                            intensity: *intensity,
                        })
                    }
                    GameLoopEvent::GravityWellCharging { well_index, position } => {
                        Some(GameEvent::GravityWellCharging {
                            well_index: *well_index,
                            position: *position,
                        })
                    }
                    GameLoopEvent::GravityWaveExplosion { well_index, position, strength } => {
                        Some(GameEvent::GravityWaveExplosion {
                            well_index: *well_index,
                            position: *position,
                            strength: *strength,
                        })
                    }
                    // Other events are already reflected in state snapshots
                    _ => None,
                };

                if let Some(game_event) = game_event {
                    let session_clone = session.clone();
                    tokio::spawn(async move {
                        let session_guard = session_clone.read().await;
                        broadcast_message(&session_guard, &ServerMessage::Event(game_event)).await;
                    });
                }
            }

            // Broadcast AOI-filtered snapshots if needed (each player gets their own filtered view)
            if snapshot.is_some() {
                let session_clone = session.clone();
                tokio::spawn(async move {
                    let session_guard = session_clone.read().await;
                    // Use AOI filtering for per-client snapshots
                    broadcast_filtered_snapshots(&session_guard).await;
                });
            }

            // Log stats periodically (every 30 seconds)
            if tick_count % (physics::TICK_RATE as u64 * 30) == 0 {
                let session_guard = session.read().await;
                let elapsed = start.elapsed().as_secs();
                let human_count = session_guard.players.len();
                let bot_count = session_guard.game_loop.state().players.values().filter(|p| p.is_bot).count();
                let well_count = session_guard.game_loop.state().arena.gravity_wells.len();
                let perf_status = session_guard.performance.status();
                let perf_budget = session_guard.performance.budget_usage_percent();

                if session_guard.simulation_config.enabled {
                    let target = session_guard.bot_count;
                    let cycle_progress = (elapsed as f32 % session_guard.simulation_config.cycle_duration_secs)
                        / session_guard.simulation_config.cycle_duration_secs * 100.0;
                    info!(
                        "Game: {}s, tick {}, {} humans + {}/{} bots, {} wells | Perf: {:?} ({:.1}%) | Sim: {:.1}% cycle",
                        elapsed,
                        session_guard.game_loop.state().tick,
                        human_count,
                        bot_count,
                        target,
                        well_count,
                        perf_status,
                        perf_budget,
                        cycle_progress
                    );
                } else {
                    info!(
                        "Game: {}s, tick {}, {} humans + {} bots, {} wells | Perf: {:?} ({:.1}%)",
                        elapsed,
                        session_guard.game_loop.state().tick,
                        human_count,
                        bot_count,
                        well_count,
                        perf_status,
                        perf_budget
                    );
                }
            }
        }
    });
}
