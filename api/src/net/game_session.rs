//! Game session manager - runs the game loop and broadcasts state to players

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tokio::time::{interval, Instant};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::game::constants::{ai, physics};
use crate::game::game_loop::{GameLoop, GameLoopConfig, GameLoopEvent};
use crate::game::performance::{PerformanceMonitor, PerformanceStatus};
use crate::game::state::{MatchPhase, Player, PlayerId};
use crate::net::protocol::{encode, GameSnapshot, PlayerInput, ServerMessage};

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
    last_snapshot_tick: u64,
    bot_count: usize,
}

impl GameSession {
    pub fn new() -> Self {
        // Read bot count from environment, default to ai::COUNT
        let bot_count = std::env::var("BOT_COUNT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(ai::COUNT);

        info!("Bot count set to {}", bot_count);

        let mut game_loop = GameLoop::new(GameLoopConfig::default());

        // Start in Playing phase immediately (no waiting/countdown)
        game_loop.state_mut().match_state.phase = MatchPhase::Playing;
        game_loop.state_mut().match_state.countdown_time = 0.0;

        // Fill with bots initially
        game_loop.fill_with_bots(bot_count);

        Self {
            game_loop,
            players: HashMap::new(),
            performance: PerformanceMonitor::new(physics::TICK_RATE),
            last_snapshot_tick: 0,
            bot_count,
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
        writer: Arc<RwLock<Option<wtransport::SendStream>>>,
    ) -> PlayerId {
        info!("Adding player {} ({}) to game session", player_name, player_id);

        // Create player entity
        let color_index = self.players.len() as u8;
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

        // Ensure we have enough bots
        self.maintain_player_count();

        // Update arena scaling based on new player count
        self.update_arena_scale();
    }

    /// Update arena scale and gravity wells based on player count and performance
    fn update_arena_scale(&mut self) {
        let player_count = self.game_loop.state().players.len();

        // Dynamic well limit based on performance headroom
        // If using 80% of budget, only allow 20% more wells than current
        // This scales smoothly with actual measured performance
        let max_wells = self.performance.calculate_entity_budget(
            self.game_loop.state().arena.gravity_wells.len()
        );

        self.game_loop
            .state_mut()
            .arena
            .update_for_player_count_with_limit(player_count, max_wells);
    }

    /// Queue input for a player
    pub fn queue_input(&mut self, player_id: PlayerId, input: PlayerInput) {
        self.game_loop.queue_input(player_id, input);
    }

    /// Run a game tick and return events
    pub fn tick(&mut self) -> Vec<GameLoopEvent> {
        // Start performance timing
        self.performance.tick_start();

        let events = self.game_loop.tick();

        // Respawn dead players (humans always, bots only if performance allows)
        self.respawn_dead_players();

        // Performance-based bot management
        // Only forcibly remove bots in catastrophic situations (>150% budget)
        // Otherwise, let natural attrition handle it by not respawning dead bots
        if self.performance.should_force_reduce() {
            // Catastrophic: remove one bot per tick to reduce load
            self.remove_one_bot();
        } else if self.performance.can_add_bots() {
            // Excellent/Good: maintain target bot count
            self.maintain_player_count();
        }
        // Warning/Critical: do nothing - bots that die won't respawn, natural reduction

        // Keep game running forever - reset phase to Playing if it ended
        // This is an eternal game mode with no match end
        if self.game_loop.state().match_state.phase == MatchPhase::Ended {
            self.game_loop.state_mut().match_state.phase = MatchPhase::Playing;
        }

        // End performance timing
        let entity_count = self.game_loop.state().players.len()
            + self.game_loop.state().projectiles.len();
        self.performance.tick_end(entity_count);

        events
    }

    /// Remove one bot to reduce server load
    fn remove_one_bot(&mut self) {
        // Find a bot to remove (prefer dead bots, then any bot)
        let bot_to_remove = self
            .game_loop
            .state()
            .players
            .iter()
            .filter(|p| p.is_bot)
            .min_by_key(|p| if p.alive { 1 } else { 0 })
            .map(|p| p.id);

        if let Some(bot_id) = bot_to_remove {
            self.game_loop.remove_player(bot_id);
            debug!("Removed bot {} due to performance pressure", bot_id);
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

    /// Get current game snapshot
    pub fn get_snapshot(&self) -> GameSnapshot {
        GameSnapshot::from_game_state(self.game_loop.state())
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
        for player in &mut self.game_loop.state_mut().players {
            if !player.alive && player.respawn_timer > 0.0 {
                player.respawn_timer -= dt;
            }
        }

        // Get dead players whose respawn timer has expired, prioritizing humans
        let mut dead_players: Vec<(PlayerId, bool)> = self
            .game_loop
            .state()
            .players
            .iter()
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
                    .iter()
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
    /// Spawns bots gradually (a few per tick) rather than all at once
    fn maintain_player_count(&mut self) {
        let current_count = self.game_loop.state().players.len();
        let target = self.bot_count;

        // Only add bots if we're below target
        if current_count < target {
            // Add bots gradually - max 5 per tick for smoother distribution
            let bots_needed = (target - current_count).min(5);
            let new_target = current_count + bots_needed;
            self.game_loop.fill_with_bots(new_target);
        }
    }
}

impl Default for GameSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Broadcast a message to all connected players
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

/// Sanitize player state to prevent NaN/Infinity corruption
fn sanitize_game_state(session: &mut GameSession) {
    for player in &mut session.game_loop.state_mut().players {
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

            // Broadcast snapshot if needed (with timeout to prevent blocking)
            if let Some(snapshot) = snapshot {
                let session_clone = session.clone();
                tokio::spawn(async move {
                    let session_guard = session_clone.read().await;
                    let message = ServerMessage::Snapshot(snapshot);
                    broadcast_message(&session_guard, &message).await;
                });
            }

            // Log stats periodically (every 30 seconds)
            if tick_count % (physics::TICK_RATE as u64 * 30) == 0 {
                let session_guard = session.read().await;
                let elapsed = start.elapsed().as_secs();
                let human_count = session_guard.players.len();
                let bot_count = session_guard.game_loop.state().players.iter().filter(|p| p.is_bot).count();
                let well_count = session_guard.game_loop.state().arena.gravity_wells.len();
                let perf_status = session_guard.performance.status();
                let perf_budget = session_guard.performance.budget_usage_percent();
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
    });
}
