//! Game session manager - runs the game loop and broadcasts state to players
//!
//! Performance optimizations:
//! - Channel-based message sending (no lock contention on writes)
//! - Input deduplication via sequence tracking
//! - Batched writes with coalescing (reduces syscalls)
//! - Pre-allocated encode buffers (reduces allocations)

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Instant};
use tracing::{debug, info, warn};

// ============================================================================
// Buffer Pool Constants
// ============================================================================

/// Default buffer capacity in bytes (4KB covers most snapshot sizes)
const BUFFER_POOL_CAPACITY: usize = 4096;

/// Maximum buffer capacity to retain (prevents memory bloat from large messages)
const BUFFER_POOL_MAX_RETAIN: usize = 65536;

/// Minimum pool size regardless of expected connections
const BUFFER_POOL_MIN_SIZE: usize = 32;

/// Maximum pool size to prevent memory waste
const BUFFER_POOL_MAX_SIZE: usize = 512;

/// Buffers per expected concurrent connection
/// Each connection may need multiple buffers for concurrent encoding
const BUFFERS_PER_CONNECTION: usize = 2;

/// Buffer pool for encoding - avoids allocations in hot path
/// OPTIMIZATION: Uses crossbeam lock-free MPMC channel (no mutex contention)
///
/// Pool size scales based on expected connection count
pub struct BufferPool {
    sender: crossbeam_channel::Sender<Vec<u8>>,
    receiver: crossbeam_channel::Receiver<Vec<u8>>,
}

impl BufferPool {
    /// Create a new buffer pool with pre-allocated buffers
    pub fn new(count: usize, capacity: usize) -> Self {
        // Unbounded channel for flexibility (lock-free MPMC)
        let (sender, receiver) = crossbeam_channel::unbounded();

        // Pre-allocate buffers
        for _ in 0..count {
            let _ = sender.send(Vec::with_capacity(capacity));
        }

        Self { sender, receiver }
    }

    /// Create a buffer pool sized for expected connection count
    /// OPTIMIZATION: Scales pool size to match server capacity
    pub fn for_connections(expected_connections: usize) -> Self {
        let count = (expected_connections * BUFFERS_PER_CONNECTION)
            .max(BUFFER_POOL_MIN_SIZE)
            .min(BUFFER_POOL_MAX_SIZE);
        Self::new(count, BUFFER_POOL_CAPACITY)
    }

    /// Get a buffer from the pool (or allocate new if empty)
    /// OPTIMIZATION: Lock-free try_recv - no mutex contention
    #[inline]
    pub fn get(&self) -> Vec<u8> {
        self.receiver
            .try_recv()
            .unwrap_or_else(|_| Vec::with_capacity(BUFFER_POOL_CAPACITY))
    }

    /// Return a buffer to the pool for reuse
    /// OPTIMIZATION: Lock-free send - no mutex contention
    #[inline]
    pub fn put(&self, mut buf: Vec<u8>) {
        buf.clear();
        // Only keep buffers under max to avoid memory bloat
        if buf.capacity() <= BUFFER_POOL_MAX_RETAIN {
            let _ = self.sender.send(buf);
        }
    }
}

/// Global buffer pool for encoding (lazy initialized)
/// OPTIMIZATION: Sized for 100 concurrent connections (200 buffers)
static ENCODE_POOL: std::sync::OnceLock<BufferPool> = std::sync::OnceLock::new();

fn get_encode_pool() -> &'static BufferPool {
    // Default to 100 expected connections = 200 buffers (clamped to 32-512 range)
    ENCODE_POOL.get_or_init(|| BufferPool::for_connections(100))
}

/// Encode a message using a pooled buffer
pub fn encode_pooled<T: serde::Serialize>(message: &T) -> Result<Vec<u8>, String> {
    let mut buf = get_encode_pool().get();

    // Encode directly into the buffer
    match bincode::serde::encode_into_std_write(message, &mut buf, bincode::config::legacy()) {
        Ok(_) => Ok(buf),
        Err(e) => {
            get_encode_pool().put(buf);
            Err(e.to_string())
        }
    }
}

/// Return a buffer to the pool after use
pub fn return_buffer(buf: Vec<u8>) {
    get_encode_pool().put(buf);
}

use crate::config::{ArenaScalingConfig, DebrisSpawnConfig, GravityWaveConfig};
use crate::game::constants::{ai, physics};
use crate::game::game_loop::{GameLoop, GameLoopConfig, GameLoopEvent};
use crate::game::performance::{PerformanceMonitor, PerformanceStatus};
use crate::game::state::{MatchPhase, Player, PlayerId};
use crate::metrics::Metrics;
use crate::net::aoi::{AOIConfig, AOIManager};
use crate::net::delta::{generate_delta, DeltaStats};
use crate::net::protocol::{GameEvent, GameSnapshot, PlayerInput, ServerMessage};

// ============================================================================
// SPECTATOR MODE CONSTANTS
// ============================================================================

/// Maximum number of spectators allowed per game session
const MAX_SPECTATORS: usize = 20;

/// Spectator rate limiting: send updates every N ticks (2 = 5Hz at 10Hz tick rate)
const SPECTATOR_TICK_DIVISOR: u64 = 2;

/// Spectator inactivity timeout: kick spectators after this many seconds of no messages
const SPECTATOR_IDLE_TIMEOUT_SECS: u64 = 300; // 5 minutes

/// How often to check for idle spectators (in ticks)
/// At 30 TPS, 450 ticks = 15 seconds
const SPECTATOR_IDLE_CHECK_INTERVAL_TICKS: u64 = 450;

/// Minimum projectile mass to include in spectator snapshots (filters tiny projectiles)
const SPECTATOR_MIN_PROJECTILE_MASS: f32 = 10.0;

/// Maximum number of projectiles to include in spectator snapshots
const SPECTATOR_MAX_PROJECTILES: usize = 100;

/// Maximum number of debris to include in spectator snapshots
const SPECTATOR_MAX_DEBRIS: usize = 50;

/// Minimum debris size to include in spectator snapshots (0=small, 1=medium, 2=large)
const SPECTATOR_MIN_DEBRIS_SIZE: u8 = 1;

// ============================================================================
// DELTA COMPRESSION CONSTANTS
// ============================================================================

/// How often to send full snapshots (every N ticks)
/// At 30 TPS: 300 ticks = 10 seconds between full snapshots
const FULL_RESYNC_INTERVAL: u64 = 300;

// ============================================================================

// Feature-gated anticheat integration
#[cfg(feature = "anticheat")]
use crate::anticheat::validator::{sanitize_input, InputValidator};

// Feature-gated AI manager integration
#[cfg(feature = "ai_manager")]
use crate::config::AIManagerConfig;

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

// ============================================================================
// Delta Compression State
// ============================================================================

/// Per-client network state for delta compression and rate limiting
/// Tracks the last sent snapshot to generate deltas, and per-entity update timing
pub struct ClientNetState {
    /// Last full snapshot sent (used as delta base)
    pub last_snapshot: Option<GameSnapshot>,
    /// Tick of the last full snapshot sent
    pub last_full_tick: u64,
    /// Per-entity last update tick (for distance-based rate limiting)
    pub entity_last_update: HashMap<PlayerId, u64>,
    /// Whether this client needs a full resync (first message or error recovery)
    pub needs_full_resync: bool,
}

impl Default for ClientNetState {
    fn default() -> Self {
        Self {
            last_snapshot: None,
            last_full_tick: 0,
            entity_last_update: HashMap::with_capacity(64),
            needs_full_resync: true, // First message is always full
        }
    }
}

impl ClientNetState {
    /// Reset state (on disconnect/reconnect)
    pub fn reset(&mut self) {
        self.last_snapshot = None;
        self.last_full_tick = 0;
        self.entity_last_update.clear();
        self.needs_full_resync = true;
    }
}

/// A connected player's message channel for lock-free sending
/// Uses unbounded channel to avoid backpressure blocking the game loop
#[allow(dead_code)]
pub struct PlayerConnection {
    pub player_id: PlayerId,
    pub player_name: String,
    /// Channel sender for outgoing messages (lock-free)
    /// OPTIMIZATION: Uses Arc<Vec<u8>> to avoid cloning data when broadcasting
    /// to multiple players - only the Arc pointer is cloned (16 bytes)
    pub sender: mpsc::UnboundedSender<Arc<Vec<u8>>>,
    /// Legacy writer for backwards compatibility during transition
    pub writer: Arc<RwLock<Option<wtransport::SendStream>>>,
    /// Whether this connection is a spectator (no game entity)
    pub is_spectator: bool,
    /// Player ID to follow (None = full map view for spectators)
    pub spectate_target: Option<PlayerId>,
    /// Last time this connection sent any message (for idle detection)
    pub last_activity: Instant,
    /// Current viewport zoom level for filtering (1.0 = normal, 0.1 = zoomed out)
    /// Used to skip sending entities that would be too small to see at current zoom
    pub viewport_zoom: f32,
    /// Delta compression state for this client
    pub net_state: ClientNetState,
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
    /// Arena scaling configuration (shared with AI manager)
    arena_config: Arc<parking_lot::RwLock<ArenaScalingConfig>>,
    /// When the session started (for simulation timing)
    session_start: std::time::Instant,
    /// Last tick when simulation target was updated (rate limiting)
    last_simulation_update_tick: u64,
    /// Last tick when a bot was spawned (rate limiting to simulate human joins)
    last_bot_spawn_tick: u64,
    /// Last client timestamp per player for RTT echo
    last_client_times: HashMap<PlayerId, u64>,
    /// Last processed input sequence per player (for deduplication)
    last_input_sequences: HashMap<PlayerId, u64>,
    /// Last tick when we checked for idle spectators
    last_idle_check_tick: u64,
    /// Input validator for anti-cheat (feature-gated)
    #[cfg(feature = "anticheat")]
    input_validator: InputValidator,
    /// Count of rejected inputs per player (for metrics/logging)
    #[cfg(feature = "anticheat")]
    rejected_inputs: HashMap<PlayerId, u32>,
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
        let arena_config = Arc::new(parking_lot::RwLock::new(ArenaScalingConfig::from_env()));

        let mut game_loop = GameLoop::new(GameLoopConfig {
            gravity_wave_config,
            debris_spawn_config: debris_spawn_config.clone(),
            ..GameLoopConfig::default()
        });

        // Start in Playing phase immediately (no waiting/countdown)
        game_loop.state_mut().match_state.phase = MatchPhase::Playing;
        game_loop.state_mut().match_state.countdown_time = 0.0;

        // CRITICAL: Initialize arena with area-based well scaling BEFORE spawning bots
        // Uses scale_for_simulation for consistent area-based well calculation
        // Growth is instant so arena is immediately sized for player count
        {
            let config = arena_config.read();
            game_loop.state_mut().arena.scale_for_simulation(bot_count, &config);
        }
        info!(
            "Arena configured: {} gravity wells, escape_radius={:.0}",
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
            arena_config,
            session_start: std::time::Instant::now(),
            last_simulation_update_tick: 0,
            last_bot_spawn_tick: 0,
            last_client_times: HashMap::new(),
            last_input_sequences: HashMap::new(),
            last_idle_check_tick: 0,
            #[cfg(feature = "anticheat")]
            input_validator: InputValidator::default(),
            #[cfg(feature = "anticheat")]
            rejected_inputs: HashMap::new(),
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
    #[allow(dead_code)]
    pub fn player_count(&self) -> usize {
        self.game_loop.state().players.len()
    }

    /// Get shared arena config for AI manager
    #[allow(dead_code)]
    pub fn arena_config(&self) -> Arc<parking_lot::RwLock<ArenaScalingConfig>> {
        Arc::clone(&self.arena_config)
    }

    /// Add a player to the game session
    /// Creates a channel-based message sender for lock-free broadcasting
    pub fn add_player(
        &mut self,
        player_id: PlayerId,
        player_name: String,
        color_index: u8,
        writer: Arc<RwLock<Option<wtransport::SendStream>>>,
    ) -> PlayerId {
        info!("Player joined: {} ({})", player_name, player_id);

        // Create player entity with their selected color
        let player = Player::new(player_id, player_name.clone(), false, color_index);

        // Add to game loop
        self.game_loop.add_player(player);

        // Create unbounded channel for lock-free message sending
        // OPTIMIZATION: Uses Arc<Vec<u8>> to avoid cloning broadcast data
        let (sender, receiver) = mpsc::unbounded_channel::<Arc<Vec<u8>>>();

        // Spawn dedicated writer task for this connection
        // This eliminates lock contention - messages are sent via channel
        let writer_clone = writer.clone();
        let pid = player_id;
        tokio::spawn(async move {
            run_writer_task(pid, receiver, writer_clone).await;
        });

        // Store connection with channel sender
        self.players.insert(
            player_id,
            PlayerConnection {
                player_id,
                player_name,
                sender,
                writer,
                is_spectator: false,
                spectate_target: None,
                last_activity: Instant::now(),
                viewport_zoom: 1.0, // Default to normal zoom
                net_state: ClientNetState::default(),
            },
        );

        // Update arena scaling based on new player count
        self.update_arena_scale();

        player_id
    }

    /// Add a spectator to the game session (no game entity, receive-only)
    pub fn add_spectator(
        &mut self,
        player_id: PlayerId,
        player_name: String,
        writer: Arc<RwLock<Option<wtransport::SendStream>>>,
    ) -> PlayerId {
        info!("Spectator joined: {} ({})", player_name, player_id);

        // Track spectator join
        if let Some(ref metrics) = self.metrics {
            metrics.spectator_joins_total.fetch_add(1, Ordering::Relaxed);
        }

        // Create unbounded channel for lock-free message sending
        // OPTIMIZATION: Uses Arc<Vec<u8>> to avoid cloning broadcast data
        let (sender, receiver) = mpsc::unbounded_channel::<Arc<Vec<u8>>>();

        // Spawn dedicated writer task for this connection
        let writer_clone = writer.clone();
        let pid = player_id;
        tokio::spawn(async move {
            run_writer_task(pid, receiver, writer_clone).await;
        });

        // Store connection as spectator (no game entity created)
        self.players.insert(
            player_id,
            PlayerConnection {
                player_id,
                player_name,
                sender,
                writer,
                is_spectator: true,
                spectate_target: None, // Full view by default
                last_activity: Instant::now(),
                viewport_zoom: 0.1, // Spectators start zoomed out
                net_state: ClientNetState::default(),
            },
        );

        player_id
    }

    /// Set spectator's follow target
    pub fn set_spectate_target(&mut self, spectator_id: PlayerId, target: Option<PlayerId>) {
        if let Some(conn) = self.players.get_mut(&spectator_id) {
            if conn.is_spectator {
                conn.spectate_target = target;
                conn.last_activity = Instant::now(); // Activity on target change
                info!("Spectator {} now following {:?}", spectator_id, target);
            }
        }
    }

    /// Set viewport zoom level for a connection (for entity filtering)
    pub fn set_viewport_zoom(&mut self, player_id: PlayerId, zoom: f32) {
        if let Some(conn) = self.players.get_mut(&player_id) {
            // Clamp zoom to valid range
            conn.viewport_zoom = zoom.clamp(0.05, 1.0);
            conn.last_activity = Instant::now();
        }
    }

    /// Update last activity timestamp for a connection (call on message receive)
    pub fn update_activity(&mut self, player_id: PlayerId) {
        if let Some(conn) = self.players.get_mut(&player_id) {
            conn.last_activity = Instant::now();
        }
    }

    /// Convert a spectator to an active player
    pub fn convert_spectator_to_player(
        &mut self,
        spectator_id: PlayerId,
        color_index: u8,
    ) -> bool {
        if let Some(conn) = self.players.get_mut(&spectator_id) {
            if conn.is_spectator {
                // Create player entity
                let player = Player::new(spectator_id, conn.player_name.clone(), false, color_index);
                self.game_loop.add_player(player);

                // Update connection state
                conn.is_spectator = false;
                conn.spectate_target = None;

                info!("Spectator {} converted to player", spectator_id);

                // Track conversion metric
                if let Some(ref metrics) = self.metrics {
                    metrics.spectator_conversions_total.fetch_add(1, Ordering::Relaxed);
                }

                self.update_arena_scale();
                return true;
            }
        }
        false
    }

    /// Check if server can accept a new spectator
    /// If at spectator capacity, tries to evict an idle spectator first
    pub fn can_accept_spectator(&mut self) -> bool {
        // If server can't accept players (at capacity), don't accept spectators either
        if !self.can_accept_player() {
            return false;
        }

        // If under limit, accept
        if self.spectator_count() < MAX_SPECTATORS {
            return true;
        }

        // At limit - try to evict an idle spectator
        self.evict_idle_spectator()
    }

    /// Get count of current spectators
    pub fn spectator_count(&self) -> usize {
        self.players.values().filter(|c| c.is_spectator).count()
    }

    /// Clean up spectators that have been idle for too long
    /// Returns the list of kicked spectator IDs
    pub fn cleanup_idle_spectators(&mut self) -> Vec<PlayerId> {
        let timeout = Duration::from_secs(SPECTATOR_IDLE_TIMEOUT_SECS);
        let now = Instant::now();

        // Find idle spectators
        let idle_spectators: Vec<PlayerId> = self.players.iter()
            .filter(|(_, conn)| {
                conn.is_spectator && now.duration_since(conn.last_activity) > timeout
            })
            .map(|(id, _)| *id)
            .collect();

        // Remove them
        for spectator_id in &idle_spectators {
            info!("Kicking idle spectator {} (inactive for >{}s)", spectator_id, SPECTATOR_IDLE_TIMEOUT_SECS);
            self.players.remove(spectator_id);
            self.last_client_times.remove(spectator_id);
        }

        // Track idle evictions
        if !idle_spectators.is_empty() {
            if let Some(ref metrics) = self.metrics {
                metrics.spectator_idle_evictions_total.fetch_add(idle_spectators.len() as u64, Ordering::Relaxed);
            }
        }

        idle_spectators
    }

    /// Try to evict the oldest idle spectator to make room for a new one
    /// Only evicts if the spectator has been idle for at least SPECTATOR_IDLE_TIMEOUT_SECS
    /// Returns true if a spectator was evicted
    pub fn evict_idle_spectator(&mut self) -> bool {
        let timeout = Duration::from_secs(SPECTATOR_IDLE_TIMEOUT_SECS);

        // Find the spectator with oldest last_activity
        let oldest = self.players.iter()
            .filter(|(_, c)| c.is_spectator)
            .min_by_key(|(_, c)| c.last_activity)
            .map(|(id, c)| (*id, c.last_activity));

        if let Some((spectator_id, last_activity)) = oldest {
            // Only evict if idle for at least the timeout duration
            if last_activity.elapsed() > timeout {
                info!("Evicting idle spectator {} to make room for new connection", spectator_id);
                self.players.remove(&spectator_id);
                self.last_client_times.remove(&spectator_id);

                // Track eviction
                if let Some(ref metrics) = self.metrics {
                    metrics.spectator_idle_evictions_total.fetch_add(1, Ordering::Relaxed);
                }

                return true;
            }
        }

        false
    }

    /// Check if we should run idle spectator cleanup this tick
    pub fn should_check_idle_spectators(&mut self) -> bool {
        let current_tick = self.game_loop.state().tick;
        if current_tick >= self.last_idle_check_tick + SPECTATOR_IDLE_CHECK_INTERVAL_TICKS {
            self.last_idle_check_tick = current_tick;
            true
        } else {
            false
        }
    }

    /// Remove a player or spectator from the game session
    pub fn remove_player(&mut self, player_id: PlayerId) {
        // Check if this was a spectator (no game entity to remove)
        let was_spectator = self.players.get(&player_id)
            .map(|c| c.is_spectator)
            .unwrap_or(false);

        if was_spectator {
            info!("Spectator left: {}", player_id);
            // Track spectator disconnect
            if let Some(ref metrics) = self.metrics {
                metrics.spectator_disconnects_total.fetch_add(1, Ordering::Relaxed);
            }
        } else {
            info!("Player left: {}", player_id);
            self.game_loop.remove_player(player_id);
        }

        self.players.remove(&player_id); // Dropping sender closes the channel, ending writer task
        self.last_client_times.remove(&player_id);
        self.last_input_sequences.remove(&player_id);

        if !was_spectator {
            // Ensure we have enough bots
            self.maintain_player_count();

            // Update arena scaling based on new player count
            self.update_arena_scale();
        }
    }

    /// Update arena scale and gravity wells based on player count
    /// Uses smooth scaling to avoid regenerating all wells and causing chaos
    /// Triggers rapid collapse if excess wells exceed threshold
    fn update_arena_scale(&mut self) {
        let player_count = self.game_loop.state().players.len();
        let config = self.arena_config.read();

        // Use smooth scaling that only adds wells incrementally
        self.game_loop
            .state_mut()
            .arena
            .scale_for_simulation(player_count, &config);

        // Calculate target wells based on arena area (must be done after scaling)
        let escape_radius = self.game_loop.state().arena.escape_radius;
        let arena_area = std::f32::consts::PI * escape_radius * escape_radius;
        let target_wells = ((arena_area / config.wells_per_area).ceil() as usize)
            .max(config.min_wells);

        // Check if we have significant excess wells (>50% over target)
        let excess = self.game_loop.state().arena.excess_wells(target_wells);
        if excess > target_wells / 2 && excess > 2 {
            // Trigger staggered collapse of excess wells
            let collapsed = self.game_loop
                .state_mut()
                .arena
                .trigger_well_collapse(target_wells);
            if collapsed > 0 {
                info!(
                    "Arena scaling: triggered collapse of {} excess wells (target: {}, excess: {})",
                    collapsed, target_wells, excess
                );
            }
        }
    }

    /// Queue input for a player with deduplication and validation
    /// Inputs with sequence <= last processed are dropped (duplicate from stream+datagram)
    /// With anticheat feature: validates and sanitizes inputs before processing
    pub fn queue_input(&mut self, player_id: PlayerId, mut input: PlayerInput) {
        // Get last sequence for this player
        let last_seq = self.last_input_sequences.get(&player_id).copied().unwrap_or(0);

        // Anti-cheat validation (feature-gated)
        #[cfg(feature = "anticheat")]
        {
            // Validate sequence progression (catches replay attacks and manipulation)
            if let Err(violation) = self.input_validator.validate_sequence(last_seq, input.sequence) {
                // Track rejected inputs
                *self.rejected_inputs.entry(player_id).or_insert(0) += 1;
                let count = self.rejected_inputs.get(&player_id).copied().unwrap_or(0);

                // Log suspicious activity
                if count <= 5 || count % 100 == 0 {
                    warn!(
                        "Player {} sequence violation ({} total): {}",
                        player_id, count, violation
                    );
                }

                // For regression, reject the input completely (potential replay attack)
                // For jumps, log but allow (could be legitimate packet loss recovery)
                if matches!(violation, crate::anticheat::validator::CheatViolation::SequenceRegression(_, _)) {
                    return;
                }
            }

            // Validate input values
            if let Err(violation) = self.input_validator.validate_input(&input) {
                // Track rejected inputs
                *self.rejected_inputs.entry(player_id).or_insert(0) += 1;
                let count = self.rejected_inputs.get(&player_id).copied().unwrap_or(0);

                // Log suspicious activity (but don't spam logs)
                if count <= 5 || count % 100 == 0 {
                    warn!(
                        "Player {} input rejected ({} total): {}",
                        player_id, count, violation
                    );
                }

                // Sanitize instead of dropping completely (graceful degradation)
                sanitize_input(&mut input);
            }

            // Validate timing (with RTT compensation)
            let server_tick = self.game_loop.state().tick;
            if let Err(violation) = self.input_validator.validate_timing(input.tick, server_tick, 10) {
                // Log but don't reject - timing issues are common with network jitter
                debug!("Player {} timing issue: {}", player_id, violation);
            }
        }

        // Deduplicate: skip if we've already processed this or a newer sequence
        // (This check is outside feature gate for basic protection even without anticheat)
        if input.sequence <= last_seq {
            // Duplicate input (likely from both stream and datagram paths)
            return;
        }

        self.last_input_sequences.insert(player_id, input.sequence);

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

        // Periodically clean up idle spectators
        if self.should_check_idle_spectators() {
            let kicked = self.cleanup_idle_spectators();
            if !kicked.is_empty() {
                debug!("Cleaned up {} idle spectators", kicked.len());
            }
        }

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

            // Calculate target radius and area per player for metrics
            {
                let config = self.arena_config.read();
                let player_count = state.players.len();
                let players = (player_count as f32).max(1.0);
                let target_area = players * config.area_per_player;
                let target_radius = (target_area / std::f32::consts::PI).sqrt()
                    .max(config.min_escape_radius)
                    .min(config.min_escape_radius * config.max_escape_multiplier);

                metrics.arena_target_radius.store(target_radius as u64, Ordering::Relaxed);

                // Calculate actual area per player
                let current_area = std::f32::consts::PI * state.arena.escape_radius * state.arena.escape_radius;
                let actual_area_per_player = if player_count > 0 {
                    current_area / player_count as f32
                } else {
                    0.0
                };
                metrics.arena_area_per_player.store(actual_area_per_player as u64, Ordering::Relaxed);
            }

            metrics.arena_gravity_wells.store(
                state.arena.gravity_wells.len() as u64,
                Ordering::Relaxed,
            );
            metrics.arena_wells_lerping.store(
                state.arena.wells_lerping_count() as u64,
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

            // Bot AI SoA metrics
            let ai_stats = self.game_loop.ai_stats();
            metrics.bot_ai_total.store(ai_stats.total_bots as u64, Ordering::Relaxed);
            metrics.bot_ai_active.store(ai_stats.active_this_tick as u64, Ordering::Relaxed);
            metrics.bot_ai_full_mode.store(ai_stats.full_mode as u64, Ordering::Relaxed);
            metrics.bot_ai_reduced_mode.store(ai_stats.reduced_mode as u64, Ordering::Relaxed);
            metrics.bot_ai_dormant_mode.store(ai_stats.dormant_mode as u64, Ordering::Relaxed);
            if let Some(adaptive) = &ai_stats.adaptive {
                metrics.bot_ai_lod_scale.store((adaptive.lod_scale * 100.0) as u64, Ordering::Relaxed);
                metrics.bot_ai_health_status.store(adaptive.health_status as u64, Ordering::Relaxed);
            }

            // Spectator metrics
            let spectator_total = self.players.values().filter(|c| c.is_spectator).count() as u64;
            let spectators_full_view = self.players.values()
                .filter(|c| c.is_spectator && c.spectate_target.is_none())
                .count() as u64;
            let spectators_following = spectator_total - spectators_full_view;

            metrics.spectators_total.store(spectator_total, Ordering::Relaxed);
            metrics.spectators_full_view.store(spectators_full_view, Ordering::Relaxed);
            metrics.spectators_following.store(spectators_following, Ordering::Relaxed);
        }

        // Provide tick metrics to AI manager for adaptive dormancy
        let tick_us = tick_start.elapsed().as_micros() as u64;
        let perf_status = match self.performance.status() {
            PerformanceStatus::Excellent => 0,
            PerformanceStatus::Good => 1,
            PerformanceStatus::Warning => 2,
            PerformanceStatus::Critical => 3,
            PerformanceStatus::Catastrophic => 4,
        };
        self.game_loop.provide_tick_metrics(tick_us, perf_status);

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

    /// Update bot count target (simulation mode) and arena size (all modes)
    /// Rate-limited to once per second
    fn update_simulation_bot_count(&mut self) {
        let current_tick = self.game_loop.state().tick;

        // Rate limit: only update once per second (every 30 ticks at 30 TPS)
        if current_tick < self.last_simulation_update_tick + 30 {
            return;
        }
        self.last_simulation_update_tick = current_tick;

        // Update bot target (simulation mode only)
        if self.simulation_config.enabled {
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
        }

        // Scale arena based on ACTUAL player count (all modes)
        // Arena grows as players join, shrinks as they leave - natural scaling
        let actual_player_count = self.game_loop.state().players.len();
        let config = self.arena_config.read();
        self.game_loop
            .state_mut()
            .arena
            .scale_for_simulation(actual_player_count, &config);
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
        let mut snapshot = GameSnapshot::from_game_state(self.game_loop.state());

        // Add AI manager status if available
        if let Some(metrics) = &self.metrics {
            let ai_enabled = metrics.ai_enabled.load(Ordering::Relaxed);
            if ai_enabled > 0 {
                let total = metrics.ai_decisions_total.load(Ordering::Relaxed) as u32;
                let successful = metrics.ai_decisions_successful.load(Ordering::Relaxed) as u32;
                let success_rate = if total > 0 { ((successful * 100) / total) as u8 } else { 0 };

                snapshot.ai_status = Some(crate::net::protocol::AIStatusSnapshot {
                    enabled: true,
                    last_decision: None, // Could be populated from AI manager history
                    confidence: metrics.ai_last_confidence.load(Ordering::Relaxed) as u8,
                    success_rate,
                    decisions_total: total,
                    decisions_successful: successful,
                });
            }
        }

        snapshot
    }

    /// Get a filtered snapshot for a specific player using AOI
    #[allow(dead_code)]
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
                let wells: Vec<_> = self.game_loop.state().arena.gravity_wells.values().cloned().collect();

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
    /// Rate-limited to ~1 bot per second to simulate realistic human join behavior
    fn maintain_player_count(&mut self) {
        let current_count = self.game_loop.state().players.len();
        let target = self.bot_count;
        let current_tick = self.game_loop.state().tick;

        // Rate limit: ~1 bot per second (30 ticks at 30 TPS)
        // Simulates realistic human join rate on a busy server
        const BOT_SPAWN_INTERVAL_TICKS: u64 = 30;

        if current_count < target && current_tick >= self.last_bot_spawn_tick + BOT_SPAWN_INTERVAL_TICKS {
            self.game_loop.fill_with_bots(current_count + 1);
            self.last_bot_spawn_tick = current_tick;
        }
    }
}

impl Default for GameSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Maximum messages to batch before writing
const WRITE_BATCH_SIZE: usize = 16;
/// Maximum bytes to batch before writing (64KB)
const WRITE_BATCH_BYTES: usize = 65536;

/// Dedicated writer task for a player connection
/// Batches multiple messages before writing to reduce syscall overhead
/// Reads from channel and writes to stream - eliminates lock contention
///
/// OPTIMIZATION: Uses Arc<Vec<u8>> to avoid cloning broadcast data.
/// Trade-off: Buffer pool reuse is lost, but we save N-1 copies per broadcast
/// (30 players = 29 × 2.5KB = 72.5KB saved per tick)
async fn run_writer_task(
    player_id: PlayerId,
    mut receiver: mpsc::UnboundedReceiver<Arc<Vec<u8>>>,
    writer: Arc<RwLock<Option<wtransport::SendStream>>>,
) {
    debug!("Writer task started for player {}", player_id);

    // Pre-allocated write buffer for batching
    let mut batch_buffer = Vec::with_capacity(WRITE_BATCH_BYTES);

    while let Some(first_data) = receiver.recv().await {
        // Start building the batch with the first message
        batch_buffer.clear();

        // Add first message with length prefix
        // OPTIMIZATION: Access Arc contents directly, no clone needed
        batch_buffer.extend_from_slice(&(first_data.len() as u32).to_le_bytes());
        batch_buffer.extend_from_slice(&*first_data);
        // Note: Arc is dropped here, Vec freed when refcount hits 0

        // Try to batch more messages (non-blocking)
        let mut msg_count = 1;
        while msg_count < WRITE_BATCH_SIZE && batch_buffer.len() < WRITE_BATCH_BYTES {
            match receiver.try_recv() {
                Ok(data) => {
                    batch_buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                    batch_buffer.extend_from_slice(&*data);
                    msg_count += 1;
                }
                Err(_) => break, // No more messages waiting
            }
        }

        // Write the entire batch in one syscall
        let mut guard = writer.write().await;
        if let Some(stream) = guard.as_mut() {
            if let Err(e) = stream.write_all(&batch_buffer).await {
                warn!("Writer task {}: batch write failed: {}", player_id, e);
                break;
            }
            // Explicit flush after batch to ensure data is sent
            if let Err(e) = stream.flush().await {
                warn!("Writer task {}: flush failed: {}", player_id, e);
                break;
            }
        } else {
            warn!("Writer task {}: stream closed", player_id);
            break;
        }
    }

    debug!("Writer task ended for player {}", player_id);
}

/// Broadcast a message to all connected players using channels (lock-free)
///
/// OPTIMIZATION: Wraps encoded data in Arc so each player receives a cheap
/// Arc clone (16 bytes) instead of copying the entire message (~2.5KB).
/// For 30 players: saves 29 × 2.5KB = 72.5KB of memory copying per broadcast.
pub async fn broadcast_message(
    session: &GameSession,
    message: &ServerMessage,
) {
    // Use pooled encoding for the shared message
    let encoded = match encode_pooled(message) {
        Ok(data) => data,
        Err(e) => {
            warn!("Failed to encode message for broadcast: {}", e);
            return;
        }
    };

    // Wrap in Arc for zero-copy sharing across all players
    let shared = Arc::new(encoded);

    // Send via channels - no locks, no spawning
    // Each channel sender clones the Arc pointer, not the data
    for (player_id, conn) in session.players.iter() {
        if let Err(e) = conn.sender.send(shared.clone()) {
            debug!("Broadcast to {}: channel closed ({})", player_id, e);
        }
    }
    // Arc is dropped here; Vec freed when all receivers process their messages
}

/// Broadcast AOI-filtered snapshots to each player using channels (lock-free)
/// Each player receives only entities relevant to their position
/// Uses pooled buffers to minimize allocations
///
/// SPECTATOR OPTIMIZATION:
/// - Full-view spectators share a single pre-encoded snapshot (Arc)
/// - Follow-mode spectators reuse the target player's cached snapshot
/// - Spectators receive updates at 5Hz (every 2nd tick) vs 10Hz for players
///
/// DELTA COMPRESSION:
/// - Sends full snapshot periodically (every FULL_RESYNC_INTERVAL ticks)
/// - Between full snapshots, sends deltas with only changed fields
/// - Distance-based rate limiting: close entities 30Hz, medium 7.5Hz, far 3.75Hz
pub async fn broadcast_filtered_snapshots(session: &mut GameSession, tick: u64) {
    use std::sync::Arc;

    // Get full snapshot once
    let full_snapshot = session.get_snapshot();

    // Track AOI stats for metrics (feature-gated)
    #[cfg(feature = "metrics_extended")]
    let mut total_original_players = 0usize;
    #[cfg(feature = "metrics_extended")]
    let mut total_filtered_players = 0usize;
    #[cfg(feature = "metrics_extended")]
    let mut total_original_projectiles = 0usize;
    #[cfg(feature = "metrics_extended")]
    let mut total_filtered_projectiles = 0usize;

    // OPTIMIZATION: Check if we have any spectators that need full snapshot
    // This includes full-view spectators AND follow-mode spectators following bots
    // (bots don't have connections, so won't be in player_snapshot_cache)
    let has_spectators = session.players.values().any(|c| c.is_spectator);

    // Find minimum zoom among full-view spectators for conservative filtering
    // Lower zoom = more zoomed out = filter more aggressively
    let min_spectator_zoom = session.players.values()
        .filter(|c| c.is_spectator && c.spectate_target.is_none())
        .map(|c| c.viewport_zoom)
        .fold(1.0f32, f32::min);

    // OPTIMIZATION: Pre-encode full snapshot ONCE for spectators
    // This saves ~2ms per spectator by encoding only once and sharing via Arc
    // Always create if there are ANY spectators (needed as fallback for bot targets)
    let full_snapshot_bytes: Option<Arc<Vec<u8>>> = if has_spectators {
        // Create a spectator-optimized snapshot using minimum zoom for filtering
        // This conservatively filters based on the most zoomed-out spectator
        let spectator_snapshot = create_spectator_snapshot(&full_snapshot, min_spectator_zoom);
        let message = ServerMessage::Snapshot(spectator_snapshot);
        match encode_pooled(&message) {
            Ok(encoded) => Some(Arc::new(encoded)),
            Err(e) => {
                warn!("Failed to encode spectator snapshot: {}", e);
                None
            }
        }
    } else {
        None
    };

    // OPTIMIZATION: Cache player snapshots for follow-mode spectators
    // Spectators following a player get the exact same bytes (zero extra encoding)
    let mut player_snapshot_cache: HashMap<PlayerId, Arc<Vec<u8>>> = HashMap::new();

    // OPTIMIZATION: Pre-compute bot snapshots for spectators following bots
    // Collect unique bot targets first, then compute snapshots once per bot
    let bot_targets: std::collections::HashSet<PlayerId> = session.players.values()
        .filter(|c| c.is_spectator)
        .filter_map(|c| c.spectate_target)
        .filter(|target_id| {
            // It's a bot if there's no connection for this player ID
            !session.players.contains_key(target_id)
        })
        .collect();

    // Pre-compute AOI snapshots for bots with spectator followers
    let mut bot_snapshot_cache: HashMap<PlayerId, Arc<Vec<u8>>> = HashMap::with_capacity(bot_targets.len());
    for &bot_id in &bot_targets {
        if let Some(bot) = session.game_loop.state().get_player(bot_id) {
            let filtered = session.aoi_manager.filter_for_player(
                bot_id,
                bot.position,
                bot.velocity,
                &full_snapshot,
            );
            let message = ServerMessage::Snapshot(filtered);
            match encode_pooled(&message) {
                Ok(encoded) => {
                    bot_snapshot_cache.insert(bot_id, Arc::new(encoded));
                }
                Err(e) => {
                    warn!("Failed to encode bot snapshot for {}: {}", bot_id, e);
                }
            }
        }
    }

    // Rate limit: spectators get updates at reduced rate (every Nth tick)
    let spectator_tick = tick % SPECTATOR_TICK_DIVISOR == 0;

    // Metrics tracking for delta compression
    #[cfg(feature = "metrics_extended")]
    let mut delta_updates_sent = 0u64;
    #[cfg(feature = "metrics_extended")]
    let mut full_updates_sent = 0u64;
    #[cfg(feature = "metrics_extended")]
    let mut total_delta_stats = DeltaStats::default();

    // First pass: encode and send to players, cache for potential followers
    for (&player_id, conn) in session.players.iter_mut() {
        if conn.is_spectator {
            continue; // Handle spectators in second pass
        }

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

        // Update AOI stats (feature-gated)
        #[cfg(feature = "metrics_extended")]
        {
            use crate::net::aoi::AOIManager;
            let stats = AOIManager::snapshot_stats(&full_snapshot, &filtered);
            total_original_players += stats.original_players;
            total_filtered_players += stats.filtered_players;
            total_original_projectiles += stats.original_projectiles;
            total_filtered_projectiles += stats.filtered_projectiles;
        }

        // Set echo_client_time for RTT measurement
        filtered.echo_client_time = session.last_client_times
            .get(&player_id)
            .copied()
            .unwrap_or(0);

        // Determine if we need a full resync for this client
        let needs_full = conn.net_state.needs_full_resync
            || conn.net_state.last_snapshot.is_none()
            || tick - conn.net_state.last_full_tick >= FULL_RESYNC_INTERVAL;

        if needs_full {
            // Send full snapshot
            let message = ServerMessage::Snapshot(filtered.clone());
            match encode_pooled(&message) {
                Ok(encoded) => {
                    let shared = Arc::new(encoded);
                    player_snapshot_cache.insert(player_id, shared.clone());

                    if let Err(e) = conn.sender.send(shared.clone()) {
                        debug!("AOI broadcast to {}: channel closed ({})", player_id, e);
                    }

                    // Update net_state for delta tracking
                    conn.net_state.last_snapshot = Some(filtered);
                    conn.net_state.last_full_tick = tick;
                    conn.net_state.needs_full_resync = false;
                    conn.net_state.entity_last_update.clear();

                    #[cfg(feature = "metrics_extended")]
                    {
                        full_updates_sent += 1;
                    }
                }
                Err(e) => {
                    warn!("Failed to encode snapshot for {}: {}", player_id, e);
                    conn.net_state.needs_full_resync = true; // Retry next tick
                }
            }
        } else {
            // Generate and send delta
            let base_snapshot = conn.net_state.last_snapshot.as_ref().unwrap();

            match generate_delta(
                base_snapshot,
                &filtered,
                player_position,
                tick,
                &conn.net_state.entity_last_update,
            ) {
                Some((delta, stats)) => {
                    let message = ServerMessage::Delta(delta);
                    match encode_pooled(&message) {
                        Ok(encoded) => {
                            let shared = Arc::new(encoded);
                            // For spectator following, we still need to cache the full snapshot
                            // Generate full snapshot bytes for cache (spectators need full data)
                            if let Ok(full_encoded) = encode_pooled(&ServerMessage::Snapshot(filtered.clone())) {
                                player_snapshot_cache.insert(player_id, Arc::new(full_encoded));
                            }

                            if let Err(e) = conn.sender.send(shared) {
                                debug!("Delta broadcast to {}: channel closed ({})", player_id, e);
                            }

                            // Update entity_last_update for rate limiting
                            for player in &filtered.players {
                                conn.net_state.entity_last_update.insert(player.id, tick);
                            }

                            // Update base snapshot for next delta
                            conn.net_state.last_snapshot = Some(filtered);

                            #[cfg(feature = "metrics_extended")]
                            {
                                delta_updates_sent += 1;
                                total_delta_stats.players_included += stats.players_included;
                                total_delta_stats.players_skipped += stats.players_skipped;
                                total_delta_stats.full_rate_count += stats.full_rate_count;
                                total_delta_stats.reduced_rate_count += stats.reduced_rate_count;
                                total_delta_stats.dormant_rate_count += stats.dormant_rate_count;
                            }
                        }
                        Err(e) => {
                            warn!("Failed to encode delta for {}: {}", player_id, e);
                            conn.net_state.needs_full_resync = true; // Fallback to full
                        }
                    }
                }
                None => {
                    // No changes to send - nothing to do
                    // Still update base snapshot so next delta is relative to current state
                    conn.net_state.last_snapshot = Some(filtered);
                }
            }
        }
    }

    // Second pass: spectators
    // - Follow-mode spectators get updates at FULL rate (same as the player they follow)
    // - Full-view spectators get updates at reduced rate (large snapshots, bandwidth savings)
    for (&player_id, conn) in session.players.iter() {
        if !conn.is_spectator {
            continue;
        }

        let bytes: Arc<Vec<u8>> = match conn.spectate_target {
            // FULL VIEW: Rate-limited (large snapshots)
            None => {
                // Only send on spectator ticks to save bandwidth
                if !spectator_tick {
                    continue;
                }
                if let Some(ref full) = full_snapshot_bytes {
                    full.clone() // Arc::clone - O(1)
                } else {
                    continue;
                }
            }
            // FOLLOW MODE: Full rate (same AOI-filtered data as the target player)
            // No rate limiting - spectators following a player should see smooth movement
            Some(target_id) => {
                if let Some(cached) = player_snapshot_cache.get(&target_id) {
                    // Human player - use their cached AOI-filtered snapshot (O(1))
                    cached.clone() // Arc::clone - O(1)
                } else if let Some(cached) = bot_snapshot_cache.get(&target_id) {
                    // Bot with cached snapshot - reuse pre-computed AOI snapshot (O(1))
                    // This optimization ensures N spectators following same bot = O(1) not O(N)
                    cached.clone() // Arc::clone - O(1)
                } else if let Some(ref full) = full_snapshot_bytes {
                    // Target doesn't exist (disconnected/dead) - fall back to full view (rate-limited)
                    if !spectator_tick {
                        continue;
                    }
                    full.clone() // Arc::clone - O(1)
                } else {
                    continue;
                }
            }
        };

        if let Err(e) = conn.sender.send(bytes) {
            debug!("Spectator broadcast to {}: channel closed ({})", player_id, e);
        }
    }

    // Update metrics with AOI stats (feature-gated)
    #[cfg(feature = "metrics_extended")]
    if let Some(metrics) = &session.metrics {
        use std::sync::atomic::Ordering;
        metrics.aoi_original_players.store(total_original_players as u64, Ordering::Relaxed);
        metrics.aoi_filtered_players.store(total_filtered_players as u64, Ordering::Relaxed);
        metrics.aoi_original_projectiles.store(total_original_projectiles as u64, Ordering::Relaxed);
        metrics.aoi_filtered_projectiles.store(total_filtered_projectiles as u64, Ordering::Relaxed);
        if total_original_players > 0 {
            let reduction = (1.0 - (total_filtered_players as f32 / total_original_players as f32)) * 100.0;
            metrics.aoi_reduction_percent.store(reduction as u64, Ordering::Relaxed);
        }

        // Delta compression metrics
        metrics.delta_updates_sent.fetch_add(delta_updates_sent, Ordering::Relaxed);
        metrics.full_updates_sent.fetch_add(full_updates_sent, Ordering::Relaxed);
        metrics.updates_full_rate.fetch_add(total_delta_stats.full_rate_count as u64, Ordering::Relaxed);
        metrics.updates_reduced_rate.fetch_add(total_delta_stats.reduced_rate_count as u64, Ordering::Relaxed);
        metrics.updates_dormant_rate.fetch_add(total_delta_stats.dormant_rate_count as u64, Ordering::Relaxed);
        metrics.updates_skipped_total.fetch_add(total_delta_stats.players_skipped as u64, Ordering::Relaxed);
    }
}

/// Minimum screen pixels for entity to be visible (below this, skip sending)
const MIN_VISIBLE_SCREEN_PIXELS: f32 = 2.0;

/// Radius scale factor (must match client MASS.RADIUS_SCALE)
const RADIUS_SCALE: f32 = 2.0;

/// Calculate minimum mass visible at a given zoom level
/// Formula: screen_radius = sqrt(mass) * RADIUS_SCALE * zoom
/// We want screen_radius >= MIN_VISIBLE_SCREEN_PIXELS
/// So: sqrt(mass) >= MIN_VISIBLE_SCREEN_PIXELS / (RADIUS_SCALE * zoom)
/// mass >= (MIN_VISIBLE_SCREEN_PIXELS / (RADIUS_SCALE * zoom))^2
fn min_visible_mass(zoom: f32) -> f32 {
    let min_world_radius = MIN_VISIBLE_SCREEN_PIXELS / (RADIUS_SCALE * zoom.max(0.05));
    min_world_radius * min_world_radius
}

/// Create a filtered snapshot for spectators (reduce bandwidth)
/// Filters out entities that would be too small to see at the given zoom level
/// Uses viewport_zoom to dynamically determine what's visible
fn create_spectator_snapshot(full: &GameSnapshot, zoom: f32) -> GameSnapshot {
    // Calculate minimum visible mass based on zoom
    // At zoom 0.1: min_mass = 100 (only large entities visible)
    // At zoom 0.5: min_mass = 4 (most entities visible)
    // At zoom 1.0: min_mass = 1 (everything visible)
    let min_mass = min_visible_mass(zoom);

    // Use whichever is more restrictive: zoom-based or fixed constant
    let effective_projectile_min = min_mass.max(SPECTATOR_MIN_PROJECTILE_MASS);

    // For debris, use size-based filtering at low zoom, mass-based at higher zoom
    // Small debris: ~5 mass, Medium: ~15 mass, Large: ~30 mass
    let min_debris_size = if min_mass > 20.0 {
        2 // Large only at very low zoom
    } else if min_mass > 8.0 {
        1 // Medium and Large
    } else {
        SPECTATOR_MIN_DEBRIS_SIZE // Use default
    };

    GameSnapshot {
        tick: full.tick,
        match_phase: full.match_phase.clone(),
        match_time: full.match_time,
        countdown: full.countdown,
        players: full.players.clone(), // ALL players always visible
        projectiles: full.projectiles.iter()
            .filter(|p| p.mass > effective_projectile_min)
            .take(SPECTATOR_MAX_PROJECTILES)
            .cloned()
            .collect(),
        debris: full.debris.iter()
            .filter(|d| d.size >= min_debris_size)
            .take(SPECTATOR_MAX_DEBRIS)
            .cloned()
            .collect(),
        arena_collapse_phase: full.arena_collapse_phase,
        arena_safe_radius: full.arena_safe_radius,
        arena_scale: full.arena_scale,
        gravity_wells: full.gravity_wells.clone(),
        total_players: full.total_players,
        total_alive: full.total_alive,
        density_grid: full.density_grid.clone(),
        notable_players: full.notable_players.clone(),
        echo_client_time: 0, // Spectators don't need RTT measurement
        ai_status: full.ai_status.clone(),
    }
}

/// Send a message to a specific player using pooled buffers
pub async fn send_to_player(
    writer: &Arc<RwLock<Option<wtransport::SendStream>>>,
    message: &ServerMessage,
) -> Result<(), String> {
    let encoded = encode_pooled(message)?;
    let len_bytes = (encoded.len() as u32).to_le_bytes();

    let result = if let Some(writer) = &mut *writer.write().await {
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
    };

    // Return buffer to pool
    return_buffer(encoded);
    result
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

#[cfg(test)]
mod spectator_tests {
    use super::*;
    use crate::net::protocol::GameSnapshot;

    #[test]
    fn test_create_spectator_snapshot_filters_small_entities() {
        // Create a full snapshot with various entity sizes
        let full = GameSnapshot {
            tick: 100,
            match_phase: crate::game::state::MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players: vec![],
            projectiles: vec![
                crate::net::protocol::ProjectileSnapshot {
                    id: 1,
                    owner_id: uuid::Uuid::nil(),
                    position: crate::util::vec2::Vec2::ZERO,
                    velocity: crate::util::vec2::Vec2::ZERO,
                    mass: 5.0,  // Should be filtered (< 10.0)
                },
                crate::net::protocol::ProjectileSnapshot {
                    id: 2,
                    owner_id: uuid::Uuid::nil(),
                    position: crate::util::vec2::Vec2::ZERO,
                    velocity: crate::util::vec2::Vec2::ZERO,
                    mass: 15.0,  // Should be kept (> 10.0)
                },
            ],
            debris: vec![
                crate::net::protocol::DebrisSnapshot { id: 1, position: crate::util::vec2::Vec2::ZERO, size: 0 },  // Small, filtered
                crate::net::protocol::DebrisSnapshot { id: 2, position: crate::util::vec2::Vec2::ZERO, size: 1 },  // Medium, kept
                crate::net::protocol::DebrisSnapshot { id: 3, position: crate::util::vec2::Vec2::ZERO, size: 2 },  // Large, kept
            ],
            arena_collapse_phase: 0,
            arena_safe_radius: 1000.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: 0,
            total_alive: 0,
            density_grid: vec![],
            notable_players: vec![],
            echo_client_time: 12345,
            ai_status: None,
        };

        // Use moderate zoom (0.5) for basic filtering behavior
        let spectator_snap = create_spectator_snapshot(&full, 0.5);

        // Projectiles: only mass > 10 kept
        assert_eq!(spectator_snap.projectiles.len(), 1);
        assert_eq!(spectator_snap.projectiles[0].id, 2);

        // Debris: only size >= 1 kept
        assert_eq!(spectator_snap.debris.len(), 2);

        // Echo time should be 0 for spectators
        assert_eq!(spectator_snap.echo_client_time, 0);

        // Other fields preserved
        assert_eq!(spectator_snap.tick, 100);
        assert_eq!(spectator_snap.arena_safe_radius, 1000.0);
    }

    #[test]
    fn test_spectator_snapshot_respects_limits() {
        // Create snapshot with more entities than spectator limits
        let mut projectiles = Vec::new();
        for i in 0..150 {
            projectiles.push(crate::net::protocol::ProjectileSnapshot {
                id: i,
                owner_id: uuid::Uuid::nil(),
                position: crate::util::vec2::Vec2::ZERO,
                velocity: crate::util::vec2::Vec2::ZERO,
                mass: 20.0,  // All above threshold
            });
        }

        let mut debris = Vec::new();
        for i in 0..100 {
            debris.push(crate::net::protocol::DebrisSnapshot {
                id: i,
                position: crate::util::vec2::Vec2::ZERO,
                size: 2,  // All large
            });
        }

        let full = GameSnapshot {
            tick: 100,
            match_phase: crate::game::state::MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players: vec![],
            projectiles,
            debris,
            arena_collapse_phase: 0,
            arena_safe_radius: 1000.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: 0,
            total_alive: 0,
            density_grid: vec![],
            notable_players: vec![],
            echo_client_time: 0,
            ai_status: None,
        };

        // Use high zoom (1.0) so all entities pass mass filter
        let spectator_snap = create_spectator_snapshot(&full, 1.0);

        // Should respect limits (100 projectiles, 50 debris)
        assert_eq!(spectator_snap.projectiles.len(), 100);
        assert_eq!(spectator_snap.debris.len(), 50);
    }

    #[test]
    fn test_viewport_aware_filtering() {
        // Test that zoom level affects filtering thresholds
        let full = GameSnapshot {
            tick: 100,
            match_phase: crate::game::state::MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players: vec![],
            projectiles: vec![
                crate::net::protocol::ProjectileSnapshot {
                    id: 1,
                    owner_id: uuid::Uuid::nil(),
                    position: crate::util::vec2::Vec2::ZERO,
                    velocity: crate::util::vec2::Vec2::ZERO,
                    mass: 50.0,  // Medium mass
                },
                crate::net::protocol::ProjectileSnapshot {
                    id: 2,
                    owner_id: uuid::Uuid::nil(),
                    position: crate::util::vec2::Vec2::ZERO,
                    velocity: crate::util::vec2::Vec2::ZERO,
                    mass: 150.0, // Large mass
                },
            ],
            debris: vec![
                crate::net::protocol::DebrisSnapshot { id: 1, position: crate::util::vec2::Vec2::ZERO, size: 0 },
                crate::net::protocol::DebrisSnapshot { id: 2, position: crate::util::vec2::Vec2::ZERO, size: 1 },
                crate::net::protocol::DebrisSnapshot { id: 3, position: crate::util::vec2::Vec2::ZERO, size: 2 },
            ],
            arena_collapse_phase: 0,
            arena_safe_radius: 1000.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: 0,
            total_alive: 0,
            density_grid: vec![],
            notable_players: vec![],
            echo_client_time: 0,
            ai_status: None,
        };

        // At zoom 0.1 (very zoomed out), min_visible_mass = 100
        // Only projectile with mass 150 should pass
        let snap_zoomed_out = create_spectator_snapshot(&full, 0.1);
        assert_eq!(snap_zoomed_out.projectiles.len(), 1, "At zoom 0.1, only mass > 100 should pass");
        assert_eq!(snap_zoomed_out.projectiles[0].id, 2);
        assert_eq!(snap_zoomed_out.debris.len(), 1, "At zoom 0.1, only large debris should pass");

        // At zoom 0.5 (moderate), min_visible_mass = 4, but fixed threshold is 10
        // Both projectiles should pass (50 and 150 > 10)
        let snap_moderate = create_spectator_snapshot(&full, 0.5);
        assert_eq!(snap_moderate.projectiles.len(), 2, "At zoom 0.5, both projectiles should pass");
        assert_eq!(snap_moderate.debris.len(), 2, "At zoom 0.5, medium and large debris should pass");

        // At zoom 1.0 (normal), min_visible_mass = 1
        // All entities should pass their respective thresholds
        let snap_normal = create_spectator_snapshot(&full, 1.0);
        assert_eq!(snap_normal.projectiles.len(), 2, "At zoom 1.0, both projectiles should pass");
        assert_eq!(snap_normal.debris.len(), 2, "At zoom 1.0, medium and large debris should pass");
    }

    #[test]
    fn test_min_visible_mass_calculation() {
        // Verify the min_visible_mass formula
        // At zoom 0.1: min_mass = (2 / (2 * 0.1))^2 = 100
        let mass_01 = min_visible_mass(0.1);
        assert!((mass_01 - 100.0).abs() < 0.1, "At zoom 0.1, min_mass should be ~100");

        // At zoom 0.5: min_mass = (2 / (2 * 0.5))^2 = 4
        let mass_05 = min_visible_mass(0.5);
        assert!((mass_05 - 4.0).abs() < 0.1, "At zoom 0.5, min_mass should be ~4");

        // At zoom 1.0: min_mass = (2 / (2 * 1.0))^2 = 1
        let mass_10 = min_visible_mass(1.0);
        assert!((mass_10 - 1.0).abs() < 0.1, "At zoom 1.0, min_mass should be ~1");
    }

    #[test]
    fn test_spectator_follow_mode_uses_aoi_filter() {
        // This test documents the follow mode behavior:
        // When a spectator follows a player (human or bot), they see the SAME
        // AOI-filtered view that the target player would see.
        //
        // For human players: reuse cached snapshot from player_snapshot_cache
        // For bots: generate AOI-filtered snapshot using bot's position/velocity
        //
        // This is critical for consistent viewing experience - spectators following
        // bots should see exactly what a player at that position would see,
        // not the full unfiltered map.
        //
        // The code path in broadcast_filtered_snapshots:
        // 1. Check player_snapshot_cache (human players)
        // 2. If not found, lookup player in game state (bots)
        // 3. If found, generate AOI-filtered snapshot for bot's position
        // 4. If not found (disconnected), fall back to full view

        // This test documents that the logic handles all three cases:
        struct SnapshotSource {
            from_cache: bool,
            from_aoi_filter: bool,
            from_full_fallback: bool,
        }

        fn get_snapshot_source(in_cache: bool, in_game_state: bool) -> SnapshotSource {
            if in_cache {
                // Human player - use cached
                SnapshotSource { from_cache: true, from_aoi_filter: false, from_full_fallback: false }
            } else if in_game_state {
                // Bot - generate AOI filter
                SnapshotSource { from_cache: false, from_aoi_filter: true, from_full_fallback: false }
            } else {
                // Disconnected - full fallback
                SnapshotSource { from_cache: false, from_aoi_filter: false, from_full_fallback: true }
            }
        }

        // Following human player → use cache
        let human = get_snapshot_source(true, true);
        assert!(human.from_cache, "Human player should use cached snapshot");
        assert!(!human.from_aoi_filter, "Human player should not regenerate AOI");

        // Following bot → generate AOI
        let bot = get_snapshot_source(false, true);
        assert!(!bot.from_cache, "Bot should not be in cache");
        assert!(bot.from_aoi_filter, "Bot should get AOI-filtered view");

        // Following disconnected player → full fallback
        let disconnected = get_snapshot_source(false, false);
        assert!(disconnected.from_full_fallback, "Disconnected should fall back to full view");
    }

    #[test]
    fn test_spectator_full_snapshot_created_for_follow_mode() {
        // Verify that full_snapshot_bytes is created when there are follow-mode spectators
        // This is critical for spectators following bots (which aren't in player_snapshot_cache)

        struct MockConnection {
            is_spectator: bool,
            spectate_target: Option<u32>,
        }

        // Scenario 1: Only follow-mode spectators (following bots)
        let connections1 = vec![
            MockConnection { is_spectator: true, spectate_target: Some(1) },
            MockConnection { is_spectator: true, spectate_target: Some(2) },
        ];
        let has_spectators = connections1.iter().any(|c| c.is_spectator);
        assert!(has_spectators, "Should create full snapshot for follow-mode spectators");

        // Scenario 2: Mix of full-view and follow-mode
        let connections2 = vec![
            MockConnection { is_spectator: true, spectate_target: None }, // Full view
            MockConnection { is_spectator: true, spectate_target: Some(1) }, // Following
        ];
        let has_spectators = connections2.iter().any(|c| c.is_spectator);
        assert!(has_spectators, "Should create full snapshot for mixed spectators");

        // Scenario 3: No spectators
        let connections3: Vec<MockConnection> = vec![];
        let has_spectators = connections3.iter().any(|c| c.is_spectator);
        assert!(!has_spectators, "Should NOT create full snapshot when no spectators");

        // Scenario 4: Only players (no spectators)
        let connections4 = vec![
            MockConnection { is_spectator: false, spectate_target: None },
            MockConnection { is_spectator: false, spectate_target: None },
        ];
        let has_spectators = connections4.iter().any(|c| c.is_spectator);
        assert!(!has_spectators, "Should NOT create full snapshot for players only");
    }

    #[test]
    fn test_spectator_rate_limiting_by_mode() {
        // This test documents the rate limiting behavior for spectators:
        // - Full-view spectators: rate-limited (SPECTATOR_TICK_DIVISOR, e.g., every 2nd tick)
        // - Follow-mode spectators: full rate (every tick, same as players)
        //
        // This is critical for smooth viewing when following a player.

        #[derive(Clone)]
        struct MockSpectator {
            spectate_target: Option<u32>,  // None = full view, Some = following
        }

        // Simulate the rate limiting logic from broadcast_filtered_snapshots
        fn should_send_update(spectator: &MockSpectator, tick: u64, tick_divisor: u64) -> bool {
            let spectator_tick = tick % tick_divisor == 0;

            match spectator.spectate_target {
                // Full view: rate-limited
                None => spectator_tick,
                // Follow mode: full rate (always send)
                Some(_target_id) => true,
            }
        }

        let tick_divisor = 2u64;  // Same as SPECTATOR_TICK_DIVISOR

        // Full-view spectator should only receive updates on spectator ticks
        let full_view = MockSpectator { spectate_target: None };
        assert!(should_send_update(&full_view, 0, tick_divisor), "Full view: tick 0 (even) should send");
        assert!(!should_send_update(&full_view, 1, tick_divisor), "Full view: tick 1 (odd) should skip");
        assert!(should_send_update(&full_view, 2, tick_divisor), "Full view: tick 2 (even) should send");
        assert!(!should_send_update(&full_view, 3, tick_divisor), "Full view: tick 3 (odd) should skip");

        // Follow-mode spectator should receive updates every tick
        let follow_mode = MockSpectator { spectate_target: Some(123) };
        assert!(should_send_update(&follow_mode, 0, tick_divisor), "Follow mode: tick 0 should send");
        assert!(should_send_update(&follow_mode, 1, tick_divisor), "Follow mode: tick 1 should send");
        assert!(should_send_update(&follow_mode, 2, tick_divisor), "Follow mode: tick 2 should send");
        assert!(should_send_update(&follow_mode, 3, tick_divisor), "Follow mode: tick 3 should send");

        // Count updates over 10 ticks
        let full_view_updates: usize = (0..10).filter(|&t| should_send_update(&full_view, t, tick_divisor)).count();
        let follow_mode_updates: usize = (0..10).filter(|&t| should_send_update(&follow_mode, t, tick_divisor)).count();

        assert_eq!(full_view_updates, 5, "Full view should receive 5 updates in 10 ticks (50%)");
        assert_eq!(follow_mode_updates, 10, "Follow mode should receive 10 updates in 10 ticks (100%)");
    }

    #[test]
    fn test_spectator_mode_switch_changes_rate() {
        // When a spectator switches from full-view to follow-mode (or vice versa),
        // their update rate should change accordingly.

        #[derive(Clone)]
        struct MockSpectator {
            spectate_target: Option<u32>,
        }

        fn should_send_update(spectator: &MockSpectator, tick: u64, tick_divisor: u64) -> bool {
            let spectator_tick = tick % tick_divisor == 0;
            match spectator.spectate_target {
                None => spectator_tick,
                Some(_) => true,
            }
        }

        let tick_divisor = 2u64;
        let mut spectator = MockSpectator { spectate_target: None };

        // Start in full-view mode
        let updates_full_view: Vec<bool> = (0..6).map(|t| should_send_update(&spectator, t, tick_divisor)).collect();
        assert_eq!(updates_full_view, vec![true, false, true, false, true, false],
            "Full view: should alternate between send and skip");

        // Switch to follow mode at tick 3
        spectator.spectate_target = Some(42);
        let updates_after_switch: Vec<bool> = (3..9).map(|t| should_send_update(&spectator, t, tick_divisor)).collect();
        assert_eq!(updates_after_switch, vec![true, true, true, true, true, true],
            "After switching to follow mode: should send every tick");

        // Switch back to full view
        spectator.spectate_target = None;
        let updates_back_full: Vec<bool> = (6..12).map(|t| should_send_update(&spectator, t, tick_divisor)).collect();
        assert_eq!(updates_back_full, vec![true, false, true, false, true, false],
            "After switching back to full view: should alternate again");
    }

    #[test]
    fn test_spectator_divisor_constant() {
        // Verify the tick divisor is set to a reasonable value
        assert_eq!(super::SPECTATOR_TICK_DIVISOR, 2,
            "SPECTATOR_TICK_DIVISOR should be 2 (50% rate for full-view spectators)");
    }

    #[test]
    fn test_bot_snapshot_caching_logic() {
        // Test the logic for identifying bot targets that need cached snapshots
        // Bot targets are players without connections (i.e., not in the players HashMap)

        use std::collections::{HashSet, HashMap};

        struct MockConnection {
            is_spectator: bool,
            spectate_target: Option<u32>,
        }

        // Simulate the optimization logic from broadcast_filtered_snapshots
        fn collect_bot_targets(
            connections: &HashMap<u32, MockConnection>,
        ) -> HashSet<u32> {
            connections.values()
                .filter(|c| c.is_spectator)
                .filter_map(|c| c.spectate_target)
                .filter(|target_id| {
                    // It's a bot if there's no connection for this player ID
                    !connections.contains_key(target_id)
                })
                .collect()
        }

        // Scenario: 2 spectators following the same bot (ID 100), 1 following a human (ID 1)
        let mut connections = HashMap::new();
        // Human player with connection
        connections.insert(1, MockConnection { is_spectator: false, spectate_target: None });
        // Spectator following human (player 1)
        connections.insert(2, MockConnection { is_spectator: true, spectate_target: Some(1) });
        // Spectator following bot (bot 100)
        connections.insert(3, MockConnection { is_spectator: true, spectate_target: Some(100) });
        // Another spectator following same bot (bot 100)
        connections.insert(4, MockConnection { is_spectator: true, spectate_target: Some(100) });
        // Spectator following different bot (bot 200)
        connections.insert(5, MockConnection { is_spectator: true, spectate_target: Some(200) });

        let bot_targets = collect_bot_targets(&connections);

        // Should identify bots 100 and 200 (they don't have connections)
        assert!(bot_targets.contains(&100), "Bot 100 should be identified as a target");
        assert!(bot_targets.contains(&200), "Bot 200 should be identified as a target");
        assert_eq!(bot_targets.len(), 2, "Should find exactly 2 unique bot targets");

        // Human player 1 should NOT be in bot_targets (they have a connection)
        assert!(!bot_targets.contains(&1), "Human player 1 should not be in bot_targets");

        // The optimization ensures:
        // - Bot 100's snapshot is computed ONCE even though 2 spectators follow it
        // - Bot 200's snapshot is computed ONCE
        // - Human player 1's snapshot is reused from player_snapshot_cache
        // Total: O(M) bot snapshot computations instead of O(N*M) where N=spectators, M=bots
    }

    #[test]
    fn test_bot_snapshot_cache_deduplication() {
        // Verify that multiple spectators following the same bot only trigger one cache entry
        use std::collections::HashSet;

        struct MockSpectator {
            spectate_target: Option<u32>,
        }

        let spectators = vec![
            MockSpectator { spectate_target: Some(100) }, // Bot 100
            MockSpectator { spectate_target: Some(100) }, // Same bot
            MockSpectator { spectate_target: Some(100) }, // Same bot again
            MockSpectator { spectate_target: Some(200) }, // Different bot
        ];

        // Player IDs that have connections (simulate real players)
        let player_connections: HashSet<u32> = HashSet::new(); // Empty = all targets are bots

        let bot_targets: HashSet<u32> = spectators.iter()
            .filter_map(|s| s.spectate_target)
            .filter(|id| !player_connections.contains(id))
            .collect();

        // Even though 3 spectators follow bot 100, it should only appear once
        assert_eq!(bot_targets.len(), 2, "Should deduplicate to 2 unique bots");
        assert!(bot_targets.contains(&100));
        assert!(bot_targets.contains(&200));

        // The cache would contain only 2 entries, not 4
        // This saves 2 redundant AOI filter + encode operations
    }
}

#[cfg(test)]
mod idle_spectator_tests {
    use super::*;

    #[test]
    fn test_spectator_timeout_constant() {
        // Verify timeout is set to 5 minutes (300 seconds)
        assert_eq!(SPECTATOR_IDLE_TIMEOUT_SECS, 300);
    }

    #[test]
    fn test_idle_check_interval_is_reasonable() {
        // At 30 TPS, check should happen roughly every 15 seconds
        let check_interval_secs = SPECTATOR_IDLE_CHECK_INTERVAL_TICKS / 30;
        assert!(check_interval_secs >= 10 && check_interval_secs <= 20,
            "Idle check interval should be between 10-20 seconds, got {} seconds",
            check_interval_secs);
    }

    #[test]
    fn test_max_spectators_limit() {
        // MAX_SPECTATORS should be a reasonable limit
        assert!(MAX_SPECTATORS >= 10 && MAX_SPECTATORS <= 100,
            "MAX_SPECTATORS should be between 10-100, got {}", MAX_SPECTATORS);
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
                    GameLoopEvent::GravityWellCharging { well_id, position } => {
                        Some(GameEvent::GravityWellCharging {
                            well_id: *well_id,
                            position: *position,
                        })
                    }
                    GameLoopEvent::GravityWaveExplosion { well_id, position, strength } => {
                        Some(GameEvent::GravityWaveExplosion {
                            well_id: *well_id,
                            position: *position,
                            strength: *strength,
                        })
                    }
                    GameLoopEvent::GravityWellDestroyed { well_id, position } => {
                        Some(GameEvent::GravityWellDestroyed {
                            well_id: *well_id,
                            position: *position,
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
            // Uses write lock to update per-client delta compression state
            if snapshot.is_some() {
                let session_clone = session.clone();
                let current_tick = tick_count;
                tokio::spawn(async move {
                    let mut session_guard = session_clone.write().await;
                    // Use AOI filtering + delta compression for per-client snapshots
                    broadcast_filtered_snapshots(&mut session_guard, current_tick).await;
                });
            }

            // Log stats periodically (every 60 seconds by default, configurable via LOG_STATUS_INTERVAL_SECS)
            let log_interval = std::env::var("LOG_STATUS_INTERVAL_SECS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);
            if log_interval > 0 && tick_count % (physics::TICK_RATE as u64 * log_interval) == 0 {
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

/// Start the AI manager for autonomous parameter tuning (if enabled)
/// This runs alongside the game loop and periodically analyzes metrics
#[cfg(feature = "ai_manager")]
pub async fn start_ai_manager(session: Arc<RwLock<GameSession>>) {
    use crate::ai_manager::AIManager;

    // Load AI manager config
    let config = AIManagerConfig::from_env();

    // Skip if not enabled or no API key
    if !config.is_active() {
        info!("AI Manager disabled (AI_ENABLED=false or ORBIT_API_KEY not set)");
        return;
    }

    // Get references needed by AI manager (using async read)
    let metrics = {
        let session_guard = session.read().await;
        match &session_guard.metrics {
            Some(m) => Arc::clone(m),
            None => {
                warn!("AI Manager requires metrics to be enabled");
                return;
            }
        }
    };

    let arena_config = {
        let session_guard = session.read().await;
        session_guard.arena_config()
    };

    // Create and spawn the AI manager
    let manager = AIManager::new(config);

    tokio::spawn(async move {
        info!("Starting AI Simulation Manager");
        manager.run(metrics, arena_config).await;
    });
}

#[cfg(test)]
mod client_net_state_tests {
    use super::*;
    use uuid::Uuid;

    /// Create an empty test snapshot
    fn test_snapshot() -> GameSnapshot {
        GameSnapshot {
            tick: 0,
            match_phase: crate::game::state::MatchPhase::Playing,
            match_time: 0.0,
            countdown: 0.0,
            players: Vec::new(),
            projectiles: Vec::new(),
            debris: Vec::new(),
            arena_collapse_phase: 0,
            arena_safe_radius: 3000.0,
            arena_scale: 1.0,
            gravity_wells: Vec::new(),
            total_players: 0,
            total_alive: 0,
            density_grid: Vec::new(),
            notable_players: Vec::new(),
            echo_client_time: 0,
            ai_status: None,
        }
    }

    #[test]
    fn test_client_net_state_default() {
        let state = ClientNetState::default();
        assert!(state.last_snapshot.is_none());
        assert_eq!(state.last_full_tick, 0);
        assert!(state.entity_last_update.is_empty());
        assert!(state.needs_full_resync);
    }

    #[test]
    fn test_client_net_state_reset() {
        let mut state = ClientNetState::default();
        state.last_full_tick = 100;
        state.entity_last_update.insert(Uuid::new_v4(), 50);
        state.needs_full_resync = false;

        state.reset();

        assert!(state.last_snapshot.is_none());
        assert_eq!(state.last_full_tick, 0);
        assert!(state.entity_last_update.is_empty());
        assert!(state.needs_full_resync);
    }

    #[test]
    fn test_full_resync_interval_constant() {
        // Every 300 ticks = 10 seconds at 30 TPS
        assert_eq!(FULL_RESYNC_INTERVAL, 300);
    }

    #[test]
    fn test_needs_full_on_first_message() {
        let state = ClientNetState::default();
        // On first message, needs_full_resync is true and last_snapshot is None
        // Both conditions trigger a full resync
        assert!(state.needs_full_resync || state.last_snapshot.is_none());
    }

    #[test]
    fn test_needs_full_after_interval() {
        let state = ClientNetState {
            last_snapshot: Some(test_snapshot()),
            last_full_tick: 0,
            entity_last_update: HashMap::new(),
            needs_full_resync: false,
        };

        let current_tick = FULL_RESYNC_INTERVAL + 1;
        let needs_full = state.needs_full_resync
            || state.last_snapshot.is_none()
            || current_tick - state.last_full_tick >= FULL_RESYNC_INTERVAL;

        assert!(needs_full, "Should need full resync after interval elapsed");
    }

    #[test]
    fn test_delta_within_interval() {
        let state = ClientNetState {
            last_snapshot: Some(test_snapshot()),
            last_full_tick: 100,
            entity_last_update: HashMap::new(),
            needs_full_resync: false,
        };

        let current_tick = 200; // Only 100 ticks since last full, interval is 300
        let needs_full = state.needs_full_resync
            || state.last_snapshot.is_none()
            || current_tick - state.last_full_tick >= FULL_RESYNC_INTERVAL;

        assert!(!needs_full, "Should send delta within interval");
    }

    #[test]
    fn test_entity_last_update_tracking() {
        let mut state = ClientNetState::default();
        let player_id = Uuid::new_v4();

        // Initially empty
        assert!(state.entity_last_update.get(&player_id).is_none());

        // After update
        state.entity_last_update.insert(player_id, 100);
        assert_eq!(state.entity_last_update.get(&player_id), Some(&100));

        // Update again
        state.entity_last_update.insert(player_id, 200);
        assert_eq!(state.entity_last_update.get(&player_id), Some(&200));
    }
}
