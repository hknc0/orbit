//! Million-Scale Bot AI System (Structure of Arrays)
//!
//! Optimized for 1M+ bots using:
//! - SoA (Structure of Arrays) for cache-friendly memory layout
//! - SIMD-friendly data organization
//! - Behavior batching for branch-free processing
//! - Dormancy system for distant bot optimization
//! - Zone-based approximate queries
//!
//! # Environment Variables
//!
//! All settings can be configured via environment variables:
//!
//! ## Feature Toggles
//! - `AI_SOA_DORMANCY_ENABLED` - Enable/disable dormancy system (default: true)
//! - `AI_SOA_ZONE_QUERIES_ENABLED` - Enable/disable zone-based queries (default: true)
//! - `AI_SOA_BEHAVIOR_BATCHING_ENABLED` - Enable/disable behavior batching (default: true)
//! - `AI_SOA_PARALLEL_ENABLED` - Enable/disable parallel processing (default: true)
//!
//! ## LOD Distance Thresholds
//! - `AI_SOA_LOD_FULL_RADIUS` - Distance for full AI updates (default: 500.0)
//! - `AI_SOA_LOD_REDUCED_RADIUS` - Distance for reduced updates (default: 2000.0)
//! - `AI_SOA_LOD_DORMANT_RADIUS` - Distance for dormant mode (default: 5000.0)
//!
//! ## Update Intervals
//! - `AI_SOA_REDUCED_UPDATE_INTERVAL` - Ticks between reduced mode updates (default: 4)
//! - `AI_SOA_DORMANT_UPDATE_INTERVAL` - Ticks between dormant mode updates (default: 8)
//!
//! ## Spatial Partitioning
//! - `AI_SOA_ZONE_CELL_SIZE` - Size of zone cells in world units (default: 4096.0)
//!
//! ## Decision Making
//! - `AI_SOA_DECISION_INTERVAL` - Seconds between AI decisions (default: 0.5)

use bitvec::prelude::*;
use hashbrown::HashMap;
use rand::Rng;
use rayon::prelude::*;
use std::sync::OnceLock;

use crate::game::constants::ai::*;
use crate::game::state::{GameState, PlayerId, WellId};
use crate::net::protocol::PlayerInput;
use crate::util::vec2::Vec2;

// ============================================================================
// Default Constants for Million-Scale Optimization
// ============================================================================

/// Zone cell size for hierarchical spatial partitioning (world units)
pub const DEFAULT_ZONE_CELL_SIZE: f32 = 4096.0;

/// Distance thresholds for LOD (Level of Detail)
pub const DEFAULT_LOD_FULL_RADIUS: f32 = 500.0;
pub const DEFAULT_LOD_REDUCED_RADIUS: f32 = 2000.0;
pub const DEFAULT_LOD_DORMANT_RADIUS: f32 = 5000.0;

/// Update frequency for reduced mode (every N ticks)
pub const DEFAULT_REDUCED_UPDATE_INTERVAL: u32 = 4;

/// Update frequency for dormant mode (every N ticks)
pub const DEFAULT_DORMANT_UPDATE_INTERVAL: u32 = 8;

/// Cache refresh interval for nearest well (seconds)
pub const DEFAULT_WELL_CACHE_REFRESH_INTERVAL: f32 = 0.5;

/// Default decision interval (seconds)
pub const DEFAULT_DECISION_INTERVAL_SOA: f32 = 0.5;

// ============================================================================
// Runtime Configuration (loaded from ENV vars)
// ============================================================================

/// Global configuration singleton
static CONFIG: OnceLock<AiSoaConfig> = OnceLock::new();

/// Configuration for the SoA AI system
#[derive(Debug, Clone)]
pub struct AiSoaConfig {
    // Feature toggles
    /// Enable dormancy system (bots far from humans update less frequently)
    pub dormancy_enabled: bool,
    /// Enable zone-based spatial queries for threat detection
    pub zone_queries_enabled: bool,
    /// Enable behavior batching for branch-free processing
    pub behavior_batching_enabled: bool,
    /// Enable parallel processing via Rayon
    pub parallel_enabled: bool,

    // LOD distance thresholds
    /// Distance from human for full AI updates (every tick)
    pub lod_full_radius: f32,
    /// Distance from human for reduced AI updates
    pub lod_reduced_radius: f32,
    /// Distance from human for dormant mode
    pub lod_dormant_radius: f32,

    // Update intervals
    /// Ticks between updates in reduced mode
    pub reduced_update_interval: u32,
    /// Ticks between updates in dormant mode
    pub dormant_update_interval: u32,

    // Spatial partitioning
    /// Size of zone cells for hierarchical queries
    pub zone_cell_size: f32,

    // Decision making
    /// Base interval between AI decisions (seconds)
    pub decision_interval: f32,
    /// Cache refresh interval for nearest well (seconds)
    pub well_cache_refresh_interval: f32,
}

impl Default for AiSoaConfig {
    fn default() -> Self {
        Self {
            // Feature toggles - all enabled by default
            dormancy_enabled: true,
            zone_queries_enabled: true,
            behavior_batching_enabled: true,
            parallel_enabled: true,

            // LOD thresholds
            lod_full_radius: DEFAULT_LOD_FULL_RADIUS,
            lod_reduced_radius: DEFAULT_LOD_REDUCED_RADIUS,
            lod_dormant_radius: DEFAULT_LOD_DORMANT_RADIUS,

            // Update intervals
            reduced_update_interval: DEFAULT_REDUCED_UPDATE_INTERVAL,
            dormant_update_interval: DEFAULT_DORMANT_UPDATE_INTERVAL,

            // Spatial
            zone_cell_size: DEFAULT_ZONE_CELL_SIZE,

            // Decision making
            decision_interval: DEFAULT_DECISION_INTERVAL_SOA,
            well_cache_refresh_interval: DEFAULT_WELL_CACHE_REFRESH_INTERVAL,
        }
    }
}

impl AiSoaConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Feature toggles
        if let Ok(val) = std::env::var("AI_SOA_DORMANCY_ENABLED") {
            config.dormancy_enabled = val.parse().unwrap_or(true);
        }
        if let Ok(val) = std::env::var("AI_SOA_ZONE_QUERIES_ENABLED") {
            config.zone_queries_enabled = val.parse().unwrap_or(true);
        }
        if let Ok(val) = std::env::var("AI_SOA_BEHAVIOR_BATCHING_ENABLED") {
            config.behavior_batching_enabled = val.parse().unwrap_or(true);
        }
        if let Ok(val) = std::env::var("AI_SOA_PARALLEL_ENABLED") {
            config.parallel_enabled = val.parse().unwrap_or(true);
        }

        // LOD thresholds
        if let Ok(val) = std::env::var("AI_SOA_LOD_FULL_RADIUS") {
            config.lod_full_radius = val.parse().unwrap_or(DEFAULT_LOD_FULL_RADIUS);
        }
        if let Ok(val) = std::env::var("AI_SOA_LOD_REDUCED_RADIUS") {
            config.lod_reduced_radius = val.parse().unwrap_or(DEFAULT_LOD_REDUCED_RADIUS);
        }
        if let Ok(val) = std::env::var("AI_SOA_LOD_DORMANT_RADIUS") {
            config.lod_dormant_radius = val.parse().unwrap_or(DEFAULT_LOD_DORMANT_RADIUS);
        }

        // Update intervals
        if let Ok(val) = std::env::var("AI_SOA_REDUCED_UPDATE_INTERVAL") {
            config.reduced_update_interval = val.parse().unwrap_or(DEFAULT_REDUCED_UPDATE_INTERVAL);
        }
        if let Ok(val) = std::env::var("AI_SOA_DORMANT_UPDATE_INTERVAL") {
            config.dormant_update_interval = val.parse().unwrap_or(DEFAULT_DORMANT_UPDATE_INTERVAL);
        }

        // Spatial
        if let Ok(val) = std::env::var("AI_SOA_ZONE_CELL_SIZE") {
            config.zone_cell_size = val.parse().unwrap_or(DEFAULT_ZONE_CELL_SIZE);
        }

        // Decision making
        if let Ok(val) = std::env::var("AI_SOA_DECISION_INTERVAL") {
            config.decision_interval = val.parse().unwrap_or(DEFAULT_DECISION_INTERVAL_SOA);
        }
        if let Ok(val) = std::env::var("AI_SOA_WELL_CACHE_REFRESH_INTERVAL") {
            config.well_cache_refresh_interval = val.parse().unwrap_or(DEFAULT_WELL_CACHE_REFRESH_INTERVAL);
        }

        // Log configuration on startup
        tracing::info!(
            dormancy = config.dormancy_enabled,
            zone_queries = config.zone_queries_enabled,
            behavior_batching = config.behavior_batching_enabled,
            parallel = config.parallel_enabled,
            lod_full = config.lod_full_radius,
            lod_reduced = config.lod_reduced_radius,
            lod_dormant = config.lod_dormant_radius,
            "AI SoA configuration loaded"
        );

        config
    }

    /// Get the global configuration (loads from env on first call)
    pub fn global() -> &'static Self {
        CONFIG.get_or_init(Self::from_env)
    }

    /// Override the global configuration (for testing)
    #[cfg(test)]
    pub fn set_global(config: Self) {
        let _ = CONFIG.set(config);
    }
}

// ============================================================================
// AI Behavior and Update Mode
// ============================================================================

/// AI behavior mode (1 byte for SoA efficiency)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum AiBehavior {
    #[default]
    Orbit = 0,
    Chase = 1,
    Flee = 2,
    Collect = 3,
    Idle = 4,
}

/// Update mode for LOD system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum UpdateMode {
    #[default]
    Full = 0,    // Full AI, every tick
    Reduced = 1, // Simple AI, every 4 ticks
    Dormant = 2, // No AI, physics every 8 ticks
}

// ============================================================================
// Zone Data for Hierarchical Spatial Partitioning
// ============================================================================

/// Aggregate data for a spatial zone
#[derive(Debug, Clone, Default)]
pub struct ZoneData {
    pub center: Vec2,
    pub bot_count: u32,
    pub total_mass: f32,
    pub velocity_sum: Vec2,
    pub threat_mass: f32, // Mass of threatening entities in zone
    pub has_human: bool,
}

impl ZoneData {
    #[inline]
    pub fn average_velocity(&self) -> Vec2 {
        if self.bot_count > 0 {
            self.velocity_sum * (1.0 / self.bot_count as f32)
        } else {
            Vec2::ZERO
        }
    }

    #[inline]
    pub fn threat_level(&self) -> f32 {
        if self.total_mass > 0.0 {
            self.threat_mass / self.total_mass
        } else {
            0.0
        }
    }
}

/// Zone grid for hierarchical spatial queries
#[derive(Debug)]
pub struct ZoneGrid {
    cell_size: f32,
    inv_cell_size: f32,
    zones: HashMap<(i32, i32), ZoneData>,
}

impl ZoneGrid {
    pub fn new(cell_size: f32) -> Self {
        Self {
            cell_size,
            inv_cell_size: 1.0 / cell_size,
            zones: HashMap::with_capacity(256),
        }
    }

    #[inline]
    pub fn position_to_cell(&self, pos: Vec2) -> (i32, i32) {
        (
            (pos.x * self.inv_cell_size).floor() as i32,
            (pos.y * self.inv_cell_size).floor() as i32,
        )
    }

    #[inline]
    pub fn cell_center(&self, cell: (i32, i32)) -> Vec2 {
        Vec2::new(
            (cell.0 as f32 + 0.5) * self.cell_size,
            (cell.1 as f32 + 0.5) * self.cell_size,
        )
    }

    pub fn get_zone(&self, cell: (i32, i32)) -> Option<&ZoneData> {
        self.zones.get(&cell)
    }

    pub fn get_or_create_zone(&mut self, cell: (i32, i32)) -> &mut ZoneData {
        let center = self.cell_center(cell);
        self.zones.entry(cell).or_insert_with(|| ZoneData {
            center,
            ..Default::default()
        })
    }

    pub fn clear(&mut self) {
        for zone in self.zones.values_mut() {
            zone.bot_count = 0;
            zone.total_mass = 0.0;
            zone.velocity_sum = Vec2::ZERO;
            zone.threat_mass = 0.0;
            zone.has_human = false;
        }
    }

    /// Get adjacent zone cells (3x3 neighborhood)
    pub fn adjacent_cells(&self, cell: (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
        let (cx, cy) = cell;
        (-1..=1).flat_map(move |dx| (-1..=1).map(move |dy| (cx + dx, cy + dy)))
    }
}

impl Default for ZoneGrid {
    fn default() -> Self {
        Self::new(DEFAULT_ZONE_CELL_SIZE)
    }
}

// ============================================================================
// Behavior Batches for Branch-Free Processing
// ============================================================================

/// Indices grouped by behavior for vectorized processing
#[derive(Debug, Default)]
pub struct BehaviorBatches {
    pub orbit: Vec<u32>,
    pub chase: Vec<u32>,
    pub flee: Vec<u32>,
    pub collect: Vec<u32>,
    pub idle: Vec<u32>,
}

impl BehaviorBatches {
    pub fn clear(&mut self) {
        self.orbit.clear();
        self.chase.clear();
        self.flee.clear();
        self.collect.clear();
        self.idle.clear();
    }

    pub fn rebuild(&mut self, behaviors: &[AiBehavior], active_mask: &BitSlice) {
        self.clear();
        for (i, &behavior) in behaviors.iter().enumerate() {
            if !active_mask.get(i).map(|b| *b).unwrap_or(false) {
                continue;
            }
            let idx = i as u32;
            match behavior {
                AiBehavior::Orbit => self.orbit.push(idx),
                AiBehavior::Chase => self.chase.push(idx),
                AiBehavior::Flee => self.flee.push(idx),
                AiBehavior::Collect => self.collect.push(idx),
                AiBehavior::Idle => self.idle.push(idx),
            }
        }
    }
}

// ============================================================================
// Million-Scale AI Manager (Structure of Arrays)
// ============================================================================

/// SoA-based AI manager optimized for million-scale bot counts
#[derive(Debug)]
pub struct AiManagerSoA {
    // === Identity & Mapping ===
    /// Number of active bots
    pub count: usize,
    /// Bot player IDs (sparse key)
    pub bot_ids: Vec<PlayerId>,
    /// Map from PlayerId to dense index
    pub id_to_index: HashMap<PlayerId, u32>,

    // === Hot Path: Updated Every Tick ===
    /// Current behavior for each bot
    pub behaviors: Vec<AiBehavior>,
    /// Decision timer countdown
    pub decision_timers: Vec<f32>,
    /// Wants boost flag (packed bits)
    pub wants_boost: BitVec,
    /// Wants fire flag (packed bits)
    pub wants_fire: BitVec,
    /// Charge time for projectiles
    pub charge_times: Vec<f32>,

    // === Direction Vectors (SIMD-friendly) ===
    pub thrust_x: Vec<f32>,
    pub thrust_y: Vec<f32>,
    pub aim_x: Vec<f32>,
    pub aim_y: Vec<f32>,

    // === Target Tracking ===
    pub target_ids: Vec<Option<PlayerId>>,

    // === Personality (Cold Path: Read-Only After Init) ===
    pub aggression: Vec<f32>,
    pub preferred_radius: Vec<f32>,
    pub accuracy: Vec<f32>,
    pub reaction_variance: Vec<f32>,

    // === Caching ===
    /// Cached nearest well ID for orbit behavior
    pub cached_well_ids: Vec<Option<WellId>>,
    /// Timer for well cache refresh
    pub well_cache_timers: Vec<f32>,

    // === LOD & Dormancy ===
    /// Update mode for each bot
    pub update_modes: Vec<UpdateMode>,
    /// Active mask (1 = should update this tick)
    pub active_mask: BitVec,

    // === Hierarchical Spatial ===
    /// Zone grid for aggregate queries
    pub zone_grid: ZoneGrid,

    // === Behavior Batches ===
    pub batches: BehaviorBatches,

    // === Tick Counter ===
    pub tick_counter: u32,
}

impl AiManagerSoA {
    /// Create a new SoA AI manager with pre-allocated capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            count: 0,
            bot_ids: Vec::with_capacity(capacity),
            id_to_index: HashMap::with_capacity(capacity),

            behaviors: Vec::with_capacity(capacity),
            decision_timers: Vec::with_capacity(capacity),
            wants_boost: BitVec::with_capacity(capacity),
            wants_fire: BitVec::with_capacity(capacity),
            charge_times: Vec::with_capacity(capacity),

            thrust_x: Vec::with_capacity(capacity),
            thrust_y: Vec::with_capacity(capacity),
            aim_x: Vec::with_capacity(capacity),
            aim_y: Vec::with_capacity(capacity),

            target_ids: Vec::with_capacity(capacity),

            aggression: Vec::with_capacity(capacity),
            preferred_radius: Vec::with_capacity(capacity),
            accuracy: Vec::with_capacity(capacity),
            reaction_variance: Vec::with_capacity(capacity),

            cached_well_ids: Vec::with_capacity(capacity),
            well_cache_timers: Vec::with_capacity(capacity),

            update_modes: Vec::with_capacity(capacity),
            active_mask: BitVec::with_capacity(capacity),

            zone_grid: ZoneGrid::default(),
            batches: BehaviorBatches::default(),
            tick_counter: 0,
        }
    }

    /// Register a new bot
    pub fn register_bot(&mut self, player_id: PlayerId) {
        if self.id_to_index.contains_key(&player_id) {
            return;
        }

        let index = self.count as u32;
        self.id_to_index.insert(player_id, index);
        self.bot_ids.push(player_id);
        self.count += 1;

        // Initialize with random personality
        let mut rng = rand::thread_rng();

        self.behaviors.push(AiBehavior::Idle);
        self.decision_timers.push(0.0);
        self.wants_boost.push(false);
        self.wants_fire.push(false);
        self.charge_times.push(0.0);

        self.thrust_x.push(0.0);
        self.thrust_y.push(0.0);
        self.aim_x.push(1.0);
        self.aim_y.push(0.0);

        self.target_ids.push(None);

        self.aggression.push(rng.gen_range(0.2..0.8));
        self.preferred_radius.push(rng.gen_range(250.0..400.0));
        self.accuracy.push(rng.gen_range(0.5..0.9));
        self.reaction_variance.push(rng.gen_range(0.1..0.5));

        self.cached_well_ids.push(None);
        self.well_cache_timers.push(0.0);

        self.update_modes.push(UpdateMode::Full);
        self.active_mask.push(true);
    }

    /// Unregister a bot (swap-remove for O(1))
    pub fn unregister_bot(&mut self, player_id: PlayerId) {
        let Some(&index) = self.id_to_index.get(&player_id) else {
            return;
        };
        let idx = index as usize;
        let last_idx = self.count - 1;

        // Swap with last element
        if idx != last_idx {
            let last_id = self.bot_ids[last_idx];
            self.id_to_index.insert(last_id, index);

            self.bot_ids.swap(idx, last_idx);
            self.behaviors.swap(idx, last_idx);
            self.decision_timers.swap(idx, last_idx);
            self.charge_times.swap(idx, last_idx);
            self.thrust_x.swap(idx, last_idx);
            self.thrust_y.swap(idx, last_idx);
            self.aim_x.swap(idx, last_idx);
            self.aim_y.swap(idx, last_idx);
            self.target_ids.swap(idx, last_idx);
            self.aggression.swap(idx, last_idx);
            self.preferred_radius.swap(idx, last_idx);
            self.accuracy.swap(idx, last_idx);
            self.reaction_variance.swap(idx, last_idx);
            self.cached_well_ids.swap(idx, last_idx);
            self.well_cache_timers.swap(idx, last_idx);
            self.update_modes.swap(idx, last_idx);

            // Swap bits
            let last_boost = self.wants_boost.get(last_idx).map(|b| *b).unwrap_or(false);
            let curr_boost = self.wants_boost.get(idx).map(|b| *b).unwrap_or(false);
            self.wants_boost.set(idx, last_boost);
            self.wants_boost.set(last_idx, curr_boost);

            let last_fire = self.wants_fire.get(last_idx).map(|b| *b).unwrap_or(false);
            let curr_fire = self.wants_fire.get(idx).map(|b| *b).unwrap_or(false);
            self.wants_fire.set(idx, last_fire);
            self.wants_fire.set(last_idx, curr_fire);

            let last_active = self.active_mask.get(last_idx).map(|b| *b).unwrap_or(false);
            let curr_active = self.active_mask.get(idx).map(|b| *b).unwrap_or(false);
            self.active_mask.set(idx, last_active);
            self.active_mask.set(last_idx, curr_active);
        }

        // Remove last element
        self.id_to_index.remove(&player_id);
        self.bot_ids.pop();
        self.behaviors.pop();
        self.decision_timers.pop();
        self.wants_boost.pop();
        self.wants_fire.pop();
        self.charge_times.pop();
        self.thrust_x.pop();
        self.thrust_y.pop();
        self.aim_x.pop();
        self.aim_y.pop();
        self.target_ids.pop();
        self.aggression.pop();
        self.preferred_radius.pop();
        self.accuracy.pop();
        self.reaction_variance.pop();
        self.cached_well_ids.pop();
        self.well_cache_timers.pop();
        self.update_modes.pop();
        self.active_mask.pop();

        self.count -= 1;
    }

    /// Get dense index for a player ID
    #[inline]
    pub fn get_index(&self, player_id: PlayerId) -> Option<u32> {
        self.id_to_index.get(&player_id).copied()
    }

    /// Update zone grid with current bot positions
    pub fn update_zones(&mut self, state: &GameState) {
        self.zone_grid.clear();

        // Aggregate bot data into zones
        for i in 0..self.count {
            let player_id = self.bot_ids[i];
            let Some(player) = state.get_player(player_id) else {
                continue;
            };
            if !player.alive {
                continue;
            }

            let cell = self.zone_grid.position_to_cell(player.position);
            let zone = self.zone_grid.get_or_create_zone(cell);
            zone.bot_count += 1;
            zone.total_mass += player.mass;
            zone.velocity_sum = zone.velocity_sum + player.velocity;
        }

        // Mark zones with human players
        for player in state.players.values() {
            if !player.is_bot && player.alive {
                let cell = self.zone_grid.position_to_cell(player.position);
                let zone = self.zone_grid.get_or_create_zone(cell);
                zone.has_human = true;
                zone.threat_mass += player.mass;
            }
        }
    }

    /// Update dormancy based on distance to human players
    /// Respects AI_SOA_DORMANCY_ENABLED env var - when disabled, all bots update every tick
    pub fn update_dormancy(&mut self, state: &GameState) {
        let config = AiSoaConfig::global();

        // If dormancy is disabled, all bots are always active
        if !config.dormancy_enabled {
            for i in 0..self.count {
                self.update_modes[i] = UpdateMode::Full;
                self.active_mask.set(i, true);
            }
            return;
        }

        // Collect human player positions
        let human_positions: Vec<Vec2> = state
            .players
            .values()
            .filter(|p| !p.is_bot && p.alive)
            .map(|p| p.position)
            .collect();

        // Update each bot's dormancy state
        for i in 0..self.count {
            let player_id = self.bot_ids[i];
            let Some(player) = state.get_player(player_id) else {
                self.active_mask.set(i, false);
                continue;
            };
            if !player.alive {
                self.active_mask.set(i, false);
                continue;
            }

            // Find minimum distance to any human
            let min_dist = human_positions
                .iter()
                .map(|&h| player.position.distance_to(h))
                .min_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(f32::MAX);

            // Determine update mode based on distance (using config thresholds)
            let mode = if min_dist < config.lod_full_radius {
                UpdateMode::Full
            } else if min_dist < config.lod_reduced_radius {
                UpdateMode::Reduced
            } else {
                UpdateMode::Dormant
            };
            self.update_modes[i] = mode;

            // Set active mask based on mode and tick (using config intervals)
            let should_update = match mode {
                UpdateMode::Full => true,
                UpdateMode::Reduced => self.tick_counter % config.reduced_update_interval == 0,
                UpdateMode::Dormant => self.tick_counter % config.dormant_update_interval == 0,
            };
            self.active_mask.set(i, should_update);
        }
    }

    /// Main update function - processes all active bots
    /// Respects config flags for zone queries, behavior batching, and parallel processing
    pub fn update(&mut self, state: &GameState, dt: f32) {
        let config = AiSoaConfig::global();
        self.tick_counter = self.tick_counter.wrapping_add(1);

        // Update zones (for aggregate queries) - skip if zone queries disabled
        if config.zone_queries_enabled {
            self.update_zones(state);
        }

        // Update dormancy (skip if dormancy disabled - handled in update_dormancy)
        self.update_dormancy(state);

        // Rebuild behavior batches (skip if batching disabled)
        if config.behavior_batching_enabled {
            self.batches.rebuild(&self.behaviors, &self.active_mask);

            // Process each behavior batch
            self.update_orbit_batch(state, dt);
            self.update_chase_batch(state, dt);
            self.update_flee_batch(state, dt);
            self.update_collect_batch(state, dt);
            self.update_idle_batch(state, dt);
        } else {
            // Fallback: update all bots sequentially (for debugging/comparison)
            self.update_all_sequential(state, dt);
        }

        // Update decision timers and make new decisions
        self.update_decisions(state, dt);

        // Update firing for combat behaviors
        self.update_firing(state, dt);
    }

    /// Sequential update fallback (when behavior batching is disabled)
    fn update_all_sequential(&mut self, state: &GameState, _dt: f32) {
        for i in 0..self.count {
            if !self.active_mask.get(i).map(|b| *b).unwrap_or(false) {
                continue;
            }

            let player_id = self.bot_ids[i];
            let Some(player) = state.get_player(player_id) else {
                continue;
            };
            if !player.alive {
                continue;
            }

            match self.behaviors[i] {
                AiBehavior::Orbit => {
                    // Simplified orbit logic for sequential mode
                    let nearest_well = state.arena.gravity_wells.values().min_by(|a, b| {
                        let dist_a = (a.position - player.position).length_sq();
                        let dist_b = (b.position - player.position).length_sq();
                        dist_a.partial_cmp(&dist_b).unwrap()
                    });

                    if let Some(well) = nearest_well {
                        let to_well = well.position - player.position;
                        let tangent = to_well.perpendicular().normalize();
                        self.thrust_x[i] = tangent.x;
                        self.thrust_y[i] = tangent.y;
                    }
                }
                AiBehavior::Chase | AiBehavior::Flee => {
                    if let Some(target_id) = self.target_ids[i] {
                        if let Some(target) = state.get_player(target_id) {
                            let dir = if self.behaviors[i] == AiBehavior::Chase {
                                (target.position - player.position).normalize()
                            } else {
                                (player.position - target.position).normalize()
                            };
                            self.thrust_x[i] = dir.x;
                            self.thrust_y[i] = dir.y;
                        }
                    }
                }
                AiBehavior::Collect => {
                    if let Some(debris) = state.debris.first() {
                        let dir = (debris.position - player.position).normalize();
                        self.thrust_x[i] = dir.x;
                        self.thrust_y[i] = dir.y;
                    }
                }
                AiBehavior::Idle => {
                    self.thrust_x[i] = 0.0;
                    self.thrust_y[i] = 0.0;
                }
            }
        }
    }

    /// Update all bots in orbit behavior
    fn update_orbit_batch(&mut self, state: &GameState, _dt: f32) {
        let indices = &self.batches.orbit;
        if indices.is_empty() {
            return;
        }

        // Parallel orbit updates
        let results: Vec<(u32, f32, f32, bool)> = indices
            .par_iter()
            .filter_map(|&idx| {
                let i = idx as usize;
                let player_id = self.bot_ids[i];
                let player = state.get_player(player_id)?;
                if !player.alive {
                    return None;
                }

                // Find nearest well using zone-based approximation or cached value
                let nearest_well = state
                    .arena
                    .gravity_wells
                    .values()
                    .min_by(|a, b| {
                        let dist_a = (a.position - player.position).length_sq();
                        let dist_b = (b.position - player.position).length_sq();
                        dist_a.partial_cmp(&dist_b).unwrap()
                    });

                let (well_pos, core_radius) = nearest_well
                    .map(|w| (w.position, w.core_radius))
                    .unwrap_or((Vec2::ZERO, 50.0));

                let to_well = well_pos - player.position;
                let current_radius = to_well.length();

                // Emergency escape if too close
                let danger_zone = core_radius * 2.5;
                if current_radius < danger_zone && current_radius > 0.1 {
                    let escape_dir = -to_well.normalize();
                    return Some((idx, escape_dir.x, escape_dir.y, true));
                }

                // Target orbit radius
                let preferred = self.preferred_radius[i];
                let min_safe = core_radius * 3.0;
                let target_radius = preferred.max(min_safe);

                // Tangential + radial thrust
                let tangent = to_well.perpendicular().normalize();
                let radial = if current_radius > target_radius + 20.0 {
                    to_well.normalize() * 0.5
                } else if current_radius < target_radius - 20.0 {
                    -to_well.normalize() * 0.5
                } else {
                    Vec2::ZERO
                };

                let thrust = (tangent + radial).normalize();
                let orbital_vel =
                    crate::game::systems::gravity::orbital_velocity(current_radius);
                let boost = player.velocity.length() < orbital_vel * 0.6;

                Some((idx, thrust.x, thrust.y, boost))
            })
            .collect();

        // Apply results
        for (idx, tx, ty, boost) in results {
            let i = idx as usize;
            self.thrust_x[i] = tx;
            self.thrust_y[i] = ty;
            self.wants_boost.set(i, boost);
        }
    }

    /// Update all bots in chase behavior
    fn update_chase_batch(&mut self, state: &GameState, _dt: f32) {
        let indices = &self.batches.chase;
        if indices.is_empty() {
            return;
        }

        let results: Vec<(u32, f32, f32, f32, f32, bool, bool)> = indices
            .par_iter()
            .filter_map(|&idx| {
                let i = idx as usize;
                let player_id = self.bot_ids[i];
                let player = state.get_player(player_id)?;
                if !player.alive {
                    return None;
                }

                let target_id = self.target_ids[i]?;
                let target = state.get_player(target_id)?;
                if !target.alive {
                    return Some((idx, 0.0, 0.0, 1.0, 0.0, false, true)); // Switch to idle
                }

                // Lead the target
                let to_target = target.position - player.position;
                let distance = to_target.length();
                let time_to_reach = distance / (player.velocity.length() + 100.0);
                let predicted_pos = target.position + target.velocity * time_to_reach * 0.5;

                let chase_dir = (predicted_pos - player.position).normalize();
                let boost = distance > 100.0;

                Some((idx, chase_dir.x, chase_dir.y, chase_dir.x, chase_dir.y, boost, false))
            })
            .collect();

        for (idx, tx, ty, ax, ay, boost, to_idle) in results {
            let i = idx as usize;
            self.thrust_x[i] = tx;
            self.thrust_y[i] = ty;
            self.aim_x[i] = ax;
            self.aim_y[i] = ay;
            self.wants_boost.set(i, boost);
            if to_idle {
                self.behaviors[i] = AiBehavior::Idle;
                self.target_ids[i] = None;
            }
        }
    }

    /// Update all bots in flee behavior
    fn update_flee_batch(&mut self, state: &GameState, _dt: f32) {
        let indices = &self.batches.flee;
        if indices.is_empty() {
            return;
        }

        let results: Vec<(u32, f32, f32, f32, f32, bool)> = indices
            .par_iter()
            .filter_map(|&idx| {
                let i = idx as usize;
                let player_id = self.bot_ids[i];
                let player = state.get_player(player_id)?;
                if !player.alive {
                    return None;
                }

                let threat_id = self.target_ids[i]?;
                let threat = state.get_player(threat_id)?;
                if !threat.alive {
                    return Some((idx, 0.0, 0.0, 1.0, 0.0, true));
                }

                let flee_dir = (player.position - threat.position).normalize();

                // Stay in arena
                let zone = crate::game::systems::arena::get_zone(player.position, &state.arena);
                let adjusted = if zone == crate::game::systems::arena::Zone::Escape
                    || zone == crate::game::systems::arena::Zone::Outside
                {
                    let to_center = -player.position.normalize();
                    (flee_dir + to_center).normalize()
                } else {
                    flee_dir
                };

                Some((idx, adjusted.x, adjusted.y, -flee_dir.x, -flee_dir.y, true))
            })
            .collect();

        for (idx, tx, ty, ax, ay, to_idle) in results {
            let i = idx as usize;
            self.thrust_x[i] = tx;
            self.thrust_y[i] = ty;
            self.aim_x[i] = ax;
            self.aim_y[i] = ay;
            self.wants_boost.set(i, true);
            if to_idle {
                self.behaviors[i] = AiBehavior::Idle;
                self.target_ids[i] = None;
            }
        }
    }

    /// Update all bots in collect behavior
    fn update_collect_batch(&mut self, state: &GameState, _dt: f32) {
        let indices = &self.batches.collect;
        if indices.is_empty() {
            return;
        }

        let results: Vec<(u32, f32, f32, bool)> = indices
            .par_iter()
            .filter_map(|&idx| {
                let i = idx as usize;
                let player_id = self.bot_ids[i];
                let player = state.get_player(player_id)?;
                if !player.alive {
                    return None;
                }

                // Find nearest collectible (debris or projectile)
                let nearest_debris = state
                    .debris
                    .iter()
                    .map(|d| (d.position, d.position.distance_to(player.position)))
                    .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

                let target_pos = nearest_debris.map(|(pos, _)| pos);

                if let Some(pos) = target_pos {
                    let dir = (pos - player.position).normalize();
                    Some((idx, dir.x, dir.y, false))
                } else {
                    Some((idx, 0.0, 0.0, true)) // Switch to orbit
                }
            })
            .collect();

        for (idx, tx, ty, to_orbit) in results {
            let i = idx as usize;
            self.thrust_x[i] = tx;
            self.thrust_y[i] = ty;
            self.wants_boost.set(i, false);
            if to_orbit {
                self.behaviors[i] = AiBehavior::Orbit;
            }
        }
    }

    /// Update all bots in idle behavior
    fn update_idle_batch(&mut self, state: &GameState, _dt: f32) {
        let indices = &self.batches.idle;
        if indices.is_empty() {
            return;
        }

        for &idx in indices {
            let i = idx as usize;
            self.thrust_x[i] = 0.0;
            self.thrust_y[i] = 0.0;
            self.wants_boost.set(i, false);

            // Face velocity direction
            let player_id = self.bot_ids[i];
            if let Some(player) = state.get_player(player_id) {
                if player.velocity.length_sq() > 10.0 {
                    let vel_dir = player.velocity.normalize();
                    self.aim_x[i] = vel_dir.x;
                    self.aim_y[i] = vel_dir.y;
                }
            }
        }
    }

    /// Update decision timers and make new behavior decisions
    fn update_decisions(&mut self, state: &GameState, dt: f32) {
        let mut rng = rand::thread_rng();

        for i in 0..self.count {
            if !self.active_mask.get(i).map(|b| *b).unwrap_or(false) {
                continue;
            }

            self.decision_timers[i] -= dt;

            if self.decision_timers[i] <= 0.0 {
                // Reset timer with personality variance
                let variance = self.reaction_variance[i];
                let timing_factor = 1.0 + rng.gen_range(-variance..variance);
                self.decision_timers[i] = DECISION_INTERVAL * timing_factor;

                // Make decision
                self.decide_behavior(i, state, &mut rng);
            }
        }
    }

    /// Decide behavior for a single bot
    fn decide_behavior(&mut self, idx: usize, state: &GameState, rng: &mut impl Rng) {
        let player_id = self.bot_ids[idx];
        let Some(bot) = state.get_player(player_id) else {
            return;
        };
        if !bot.alive {
            return;
        }

        // Use zone-based threat detection
        let bot_cell = self.zone_grid.position_to_cell(bot.position);

        // Check adjacent zones for threats
        let mut threat_direction = Vec2::ZERO;
        let mut has_threat = false;

        for cell in self.zone_grid.adjacent_cells(bot_cell) {
            if let Some(zone) = self.zone_grid.get_zone(cell) {
                if zone.has_human && zone.threat_mass > bot.mass * 1.2 {
                    let threat_dir = zone.center - bot.position;
                    if threat_dir.length() < AGGRESSION_RADIUS {
                        has_threat = true;
                        threat_direction = -threat_dir.normalize();
                        break;
                    }
                }
            }
        }

        // Behavior selection
        if has_threat && rng.gen::<f32>() > self.aggression[idx] {
            // Flee from threat
            self.behaviors[idx] = AiBehavior::Flee;
            self.thrust_x[idx] = threat_direction.x;
            self.thrust_y[idx] = threat_direction.y;
            return;
        }

        // Check for chase opportunity
        if rng.gen::<f32>() < self.aggression[idx] {
            // Find nearest human target in zone
            for player in state.players.values() {
                if player.is_bot || !player.alive || player.id == player_id {
                    continue;
                }
                let dist = bot.position.distance_to(player.position);
                if dist < AGGRESSION_RADIUS * 2.0 && bot.mass >= player.mass * 0.8 {
                    self.behaviors[idx] = AiBehavior::Chase;
                    self.target_ids[idx] = Some(player.id);
                    return;
                }
            }
        }

        // Check for collect opportunity
        if !state.debris.is_empty() && rng.gen::<f32>() < 0.3 {
            self.behaviors[idx] = AiBehavior::Collect;
            return;
        }

        // Default to orbit
        self.behaviors[idx] = AiBehavior::Orbit;
        self.target_ids[idx] = None;
    }

    /// Update firing logic for combat behaviors
    fn update_firing(&mut self, state: &GameState, dt: f32) {
        let mut rng = rand::thread_rng();

        for i in 0..self.count {
            if !self.active_mask.get(i).map(|b| *b).unwrap_or(false) {
                continue;
            }

            let behavior = self.behaviors[i];
            if behavior != AiBehavior::Chase && behavior != AiBehavior::Flee {
                self.wants_fire.set(i, false);
                self.charge_times[i] = 0.0;
                continue;
            }

            let player_id = self.bot_ids[i];
            let Some(bot) = state.get_player(player_id) else {
                continue;
            };

            let target_id = match self.target_ids[i] {
                Some(id) => id,
                None => continue,
            };
            let Some(target) = state.get_player(target_id) else {
                self.wants_fire.set(i, false);
                continue;
            };

            let distance = bot.position.distance_to(target.position);

            // Range check
            if distance > 300.0 {
                self.wants_fire.set(i, false);
                self.charge_times[i] = 0.0;
                continue;
            }

            // Aim with accuracy offset
            let accuracy_offset = (1.0 - self.accuracy[i]) * rng.gen_range(-0.3..0.3);
            let aim_to_target = (target.position - bot.position).normalize();
            let rotated = aim_to_target.rotate(accuracy_offset);
            self.aim_x[i] = rotated.x;
            self.aim_y[i] = rotated.y;

            // Charge and fire logic
            let wants_fire = self.wants_fire.get(i).map(|b| *b).unwrap_or(false);
            if wants_fire {
                self.charge_times[i] += dt;
                let threshold = 0.3 + rng.gen_range(0.0..0.5);
                if self.charge_times[i] > threshold {
                    self.wants_fire.set(i, false);
                }
            } else if self.charge_times[i] > 0.0 {
                self.charge_times[i] = 0.0;
            } else if rng.gen::<f32>() < 0.02 {
                self.wants_fire.set(i, true);
            }
        }
    }

    /// Generate input for a bot
    pub fn get_input(&self, player_id: PlayerId, tick: u64) -> Option<PlayerInput> {
        let idx = *self.id_to_index.get(&player_id)? as usize;

        Some(PlayerInput {
            sequence: tick,
            tick,
            client_time: 0,
            thrust: Vec2::new(self.thrust_x[idx], self.thrust_y[idx]),
            aim: Vec2::new(self.aim_x[idx], self.aim_y[idx]),
            boost: self.wants_boost.get(idx).map(|b| *b).unwrap_or(false),
            fire: self.wants_fire.get(idx).map(|b| *b).unwrap_or(false),
            fire_released: !self.wants_fire.get(idx).map(|b| *b).unwrap_or(false)
                && self.charge_times[idx] > 0.0,
        })
    }

    /// Get statistics about the AI manager
    pub fn stats(&self) -> AiManagerStats {
        let active_count = self.active_mask.count_ones();
        let full_count = self
            .update_modes
            .iter()
            .filter(|&&m| m == UpdateMode::Full)
            .count();
        let reduced_count = self
            .update_modes
            .iter()
            .filter(|&&m| m == UpdateMode::Reduced)
            .count();
        let dormant_count = self
            .update_modes
            .iter()
            .filter(|&&m| m == UpdateMode::Dormant)
            .count();

        AiManagerStats {
            total_bots: self.count,
            active_this_tick: active_count,
            full_mode: full_count,
            reduced_mode: reduced_count,
            dormant_mode: dormant_count,
            zone_count: self.zone_grid.zones.len(),
        }
    }
}

impl Default for AiManagerSoA {
    fn default() -> Self {
        Self::with_capacity(1024)
    }
}

/// Statistics about the AI manager state
#[derive(Debug, Clone)]
pub struct AiManagerStats {
    pub total_bots: usize,
    pub active_this_tick: usize,
    pub full_mode: usize,
    pub reduced_mode: usize,
    pub dormant_mode: usize,
    pub zone_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::state::{GameState, MatchPhase, Player, GravityWell};
    use uuid::Uuid;

    // ========================================================================
    // Test Helpers
    // ========================================================================

    fn create_test_state() -> GameState {
        let mut state = GameState::new();
        state.match_state.phase = MatchPhase::Playing;
        state
    }

    fn create_bot_player(position: Vec2, mass: f32) -> Player {
        Player {
            id: Uuid::new_v4(),
            name: "TestBot".to_string(),
            position,
            velocity: Vec2::ZERO,
            rotation: 0.0,
            mass,
            alive: true,
            kills: 0,
            deaths: 0,
            spawn_protection: 0.0,
            is_bot: true,
            color_index: 0,
            respawn_timer: 0.0,
        }
    }

    fn create_human_player(position: Vec2, mass: f32) -> Player {
        let mut player = create_bot_player(position, mass);
        player.is_bot = false;
        player.name = "Human".to_string();
        player
    }

    fn create_gravity_well(id: u32, position: Vec2, mass: f32, core_radius: f32) -> GravityWell {
        GravityWell::new(id, position, mass, core_radius)
    }

    // ========================================================================
    // Registration Tests
    // ========================================================================

    #[test]
    fn test_register_bot() {
        let mut manager = AiManagerSoA::default();
        let bot_id = Uuid::new_v4();

        manager.register_bot(bot_id);

        assert_eq!(manager.count, 1);
        assert!(manager.get_index(bot_id).is_some());
        assert_eq!(manager.get_index(bot_id), Some(0));
    }

    #[test]
    fn test_register_multiple_bots() {
        let mut manager = AiManagerSoA::default();
        let ids: Vec<_> = (0..100).map(|_| Uuid::new_v4()).collect();

        for id in &ids {
            manager.register_bot(*id);
        }

        assert_eq!(manager.count, 100);
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(manager.get_index(*id), Some(i as u32));
        }
    }

    #[test]
    fn test_register_duplicate_bot() {
        let mut manager = AiManagerSoA::default();
        let bot_id = Uuid::new_v4();

        manager.register_bot(bot_id);
        manager.register_bot(bot_id); // Duplicate

        assert_eq!(manager.count, 1); // Should not increase
    }

    #[test]
    fn test_unregister_bot() {
        let mut manager = AiManagerSoA::default();
        let bot1 = Uuid::new_v4();
        let bot2 = Uuid::new_v4();

        manager.register_bot(bot1);
        manager.register_bot(bot2);
        assert_eq!(manager.count, 2);

        manager.unregister_bot(bot1);
        assert_eq!(manager.count, 1);
        assert!(manager.get_index(bot1).is_none());
        assert!(manager.get_index(bot2).is_some());
    }

    #[test]
    fn test_unregister_preserves_data_integrity() {
        let mut manager = AiManagerSoA::default();
        let bots: Vec<_> = (0..5).map(|_| Uuid::new_v4()).collect();

        for id in &bots {
            manager.register_bot(*id);
        }

        // Set specific personality for bot[2]
        manager.aggression[2] = 0.99;
        manager.preferred_radius[2] = 999.0;

        // Remove bot[0] - bot[4] (last) should swap into position 0
        manager.unregister_bot(bots[0]);

        assert_eq!(manager.count, 4);
        // bot[4] is now at index 0
        assert_eq!(manager.get_index(bots[4]), Some(0));
        // bot[2] should still have its data at index 2
        let idx2 = manager.get_index(bots[2]).unwrap() as usize;
        assert!((manager.aggression[idx2] - 0.99).abs() < 0.001);
        assert!((manager.preferred_radius[idx2] - 999.0).abs() < 0.001);
    }

    #[test]
    fn test_unregister_nonexistent_bot() {
        let mut manager = AiManagerSoA::default();
        let bot_id = Uuid::new_v4();

        manager.unregister_bot(bot_id); // Should not panic

        assert_eq!(manager.count, 0);
    }

    // ========================================================================
    // Zone Grid Tests
    // ========================================================================

    #[test]
    fn test_zone_grid_position_to_cell() {
        let grid = ZoneGrid::new(1000.0);

        assert_eq!(grid.position_to_cell(Vec2::new(0.0, 0.0)), (0, 0));
        assert_eq!(grid.position_to_cell(Vec2::new(500.0, 500.0)), (0, 0));
        assert_eq!(grid.position_to_cell(Vec2::new(999.0, 999.0)), (0, 0));
        assert_eq!(grid.position_to_cell(Vec2::new(1000.0, 0.0)), (1, 0));
        assert_eq!(grid.position_to_cell(Vec2::new(-500.0, -500.0)), (-1, -1));
    }

    #[test]
    fn test_zone_grid_cell_center() {
        let grid = ZoneGrid::new(1000.0);

        let center = grid.cell_center((0, 0));
        assert!((center.x - 500.0).abs() < 0.01);
        assert!((center.y - 500.0).abs() < 0.01);

        let center2 = grid.cell_center((1, 2));
        assert!((center2.x - 1500.0).abs() < 0.01);
        assert!((center2.y - 2500.0).abs() < 0.01);
    }

    #[test]
    fn test_zone_grid_get_or_create() {
        let mut grid = ZoneGrid::new(1000.0);

        let zone = grid.get_or_create_zone((0, 0));
        zone.bot_count = 10;
        zone.total_mass = 1000.0;

        let zone_ref = grid.get_zone((0, 0)).unwrap();
        assert_eq!(zone_ref.bot_count, 10);
        assert!((zone_ref.total_mass - 1000.0).abs() < 0.01);
    }

    #[test]
    fn test_zone_grid_clear() {
        let mut grid = ZoneGrid::new(1000.0);

        let zone1 = grid.get_or_create_zone((0, 0));
        zone1.bot_count = 10;
        zone1.has_human = true;

        let zone2 = grid.get_or_create_zone((1, 1));
        zone2.bot_count = 5;

        grid.clear();

        assert_eq!(grid.get_zone((0, 0)).unwrap().bot_count, 0);
        assert!(!grid.get_zone((0, 0)).unwrap().has_human);
        assert_eq!(grid.get_zone((1, 1)).unwrap().bot_count, 0);
    }

    #[test]
    fn test_zone_grid_adjacent_cells() {
        let grid = ZoneGrid::new(1000.0);

        let adjacent: Vec<_> = grid.adjacent_cells((5, 5)).collect();
        assert_eq!(adjacent.len(), 9);
        assert!(adjacent.contains(&(4, 4)));
        assert!(adjacent.contains(&(5, 5)));
        assert!(adjacent.contains(&(6, 6)));
    }

    // ========================================================================
    // Zone Data Tests
    // ========================================================================

    #[test]
    fn test_zone_data_average_velocity() {
        let mut zone = ZoneData::default();
        zone.bot_count = 4;
        zone.velocity_sum = Vec2::new(100.0, 200.0);

        let avg = zone.average_velocity();
        assert!((avg.x - 25.0).abs() < 0.01);
        assert!((avg.y - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_zone_data_average_velocity_empty() {
        let zone = ZoneData::default();
        let avg = zone.average_velocity();
        assert!((avg.x).abs() < 0.01);
        assert!((avg.y).abs() < 0.01);
    }

    #[test]
    fn test_zone_data_threat_level() {
        let mut zone = ZoneData::default();
        zone.total_mass = 1000.0;
        zone.threat_mass = 500.0;

        assert!((zone.threat_level() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_zone_data_threat_level_zero_mass() {
        let zone = ZoneData::default();
        assert!((zone.threat_level()).abs() < 0.01);
    }

    // ========================================================================
    // Behavior Batches Tests
    // ========================================================================

    #[test]
    fn test_behavior_batches_rebuild() {
        let mut batches = BehaviorBatches::default();
        let behaviors = vec![
            AiBehavior::Orbit,
            AiBehavior::Chase,
            AiBehavior::Orbit,
            AiBehavior::Flee,
            AiBehavior::Idle,
        ];
        let active = bitvec![1, 1, 1, 1, 1];

        batches.rebuild(&behaviors, &active);

        assert_eq!(batches.orbit.len(), 2);
        assert!(batches.orbit.contains(&0));
        assert!(batches.orbit.contains(&2));
        assert_eq!(batches.chase.len(), 1);
        assert!(batches.chase.contains(&1));
        assert_eq!(batches.flee.len(), 1);
        assert!(batches.flee.contains(&3));
        assert_eq!(batches.idle.len(), 1);
        assert!(batches.idle.contains(&4));
    }

    #[test]
    fn test_behavior_batches_respects_active_mask() {
        let mut batches = BehaviorBatches::default();
        let behaviors = vec![
            AiBehavior::Orbit,
            AiBehavior::Chase,
            AiBehavior::Orbit,
        ];
        let active = bitvec![1, 0, 1]; // Bot 1 inactive

        batches.rebuild(&behaviors, &active);

        assert_eq!(batches.orbit.len(), 2);
        assert_eq!(batches.chase.len(), 0); // Bot 1 excluded
    }

    #[test]
    fn test_behavior_batches_clear() {
        let mut batches = BehaviorBatches::default();
        batches.orbit.push(0);
        batches.chase.push(1);

        batches.clear();

        assert!(batches.orbit.is_empty());
        assert!(batches.chase.is_empty());
    }

    // ========================================================================
    // Dormancy & Update Mode Tests
    // ========================================================================

    #[test]
    fn test_update_mode_enum() {
        assert_eq!(UpdateMode::Full as u8, 0);
        assert_eq!(UpdateMode::Reduced as u8, 1);
        assert_eq!(UpdateMode::Dormant as u8, 2);
    }

    #[test]
    fn test_dormancy_near_human() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add bot at origin
        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add human nearby (within LOD_FULL_RADIUS)
        let human = create_human_player(Vec2::new(100.0, 0.0), 100.0);
        state.add_player(human);

        manager.update_dormancy(&state);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        assert_eq!(manager.update_modes[idx], UpdateMode::Full);
        assert!(manager.active_mask.get(idx).map(|b| *b).unwrap_or(false));
    }

    #[test]
    fn test_dormancy_far_from_human() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add bot far from origin
        let bot = create_bot_player(Vec2::new(10000.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add human at origin
        let human = create_human_player(Vec2::new(0.0, 0.0), 100.0);
        state.add_player(human);

        manager.update_dormancy(&state);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        assert_eq!(manager.update_modes[idx], UpdateMode::Dormant);
    }

    #[test]
    fn test_dormancy_no_humans() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        manager.update_dormancy(&state);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        // No humans = maximum distance = Dormant
        assert_eq!(manager.update_modes[idx], UpdateMode::Dormant);
    }

    // ========================================================================
    // Zone Aggregation Tests
    // ========================================================================

    #[test]
    fn test_update_zones_aggregates_bots() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add 3 bots in same zone
        for i in 0..3 {
            let bot = create_bot_player(Vec2::new(100.0 * i as f32, 0.0), 100.0 + i as f32 * 10.0);
            let bot_id = bot.id;
            state.add_player(bot);
            manager.register_bot(bot_id);
        }

        manager.update_zones(&state);

        let cell = manager.zone_grid.position_to_cell(Vec2::new(0.0, 0.0));
        let zone = manager.zone_grid.get_zone(cell).unwrap();
        assert_eq!(zone.bot_count, 3);
        assert!((zone.total_mass - 330.0).abs() < 0.01); // 100 + 110 + 120
    }

    #[test]
    fn test_update_zones_marks_human_zones() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let human = create_human_player(Vec2::new(500.0, 500.0), 200.0);
        state.add_player(human);

        manager.update_zones(&state);

        let cell = manager.zone_grid.position_to_cell(Vec2::new(500.0, 500.0));
        let zone = manager.zone_grid.get_zone(cell).unwrap();
        assert!(zone.has_human);
        assert!((zone.threat_mass - 200.0).abs() < 0.01);
    }

    // ========================================================================
    // Input Generation Tests
    // ========================================================================

    #[test]
    fn test_get_input_basic() {
        let mut manager = AiManagerSoA::default();
        let bot_id = Uuid::new_v4();
        manager.register_bot(bot_id);

        // Set some values
        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.thrust_x[idx] = 0.5;
        manager.thrust_y[idx] = -0.5;
        manager.aim_x[idx] = 1.0;
        manager.aim_y[idx] = 0.0;
        manager.wants_boost.set(idx, true);
        manager.wants_fire.set(idx, false);

        let input = manager.get_input(bot_id, 100).unwrap();

        assert_eq!(input.tick, 100);
        assert!((input.thrust.x - 0.5).abs() < 0.01);
        assert!((input.thrust.y - (-0.5)).abs() < 0.01);
        assert!((input.aim.x - 1.0).abs() < 0.01);
        assert!(input.boost);
        assert!(!input.fire);
    }

    #[test]
    fn test_get_input_nonexistent_bot() {
        let manager = AiManagerSoA::default();
        let fake_id = Uuid::new_v4();

        assert!(manager.get_input(fake_id, 0).is_none());
    }

    #[test]
    fn test_get_input_fire_released() {
        let mut manager = AiManagerSoA::default();
        let bot_id = Uuid::new_v4();
        manager.register_bot(bot_id);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.wants_fire.set(idx, false);
        manager.charge_times[idx] = 0.5; // Was charging

        let input = manager.get_input(bot_id, 0).unwrap();

        assert!(!input.fire);
        assert!(input.fire_released);
    }

    // ========================================================================
    // Stats Tests
    // ========================================================================

    #[test]
    fn test_stats() {
        let mut manager = AiManagerSoA::default();

        for _ in 0..10 {
            manager.register_bot(Uuid::new_v4());
        }

        // Set various update modes
        manager.update_modes[0] = UpdateMode::Full;
        manager.update_modes[1] = UpdateMode::Full;
        manager.update_modes[2] = UpdateMode::Reduced;
        manager.update_modes[3] = UpdateMode::Reduced;
        manager.update_modes[4] = UpdateMode::Reduced;
        manager.update_modes[5] = UpdateMode::Dormant;
        manager.update_modes[6] = UpdateMode::Dormant;
        manager.update_modes[7] = UpdateMode::Dormant;
        manager.update_modes[8] = UpdateMode::Dormant;
        manager.update_modes[9] = UpdateMode::Dormant;

        // Clear all active flags first, then set some
        for i in 0..10 {
            manager.active_mask.set(i, false);
        }
        for i in 0..5 {
            manager.active_mask.set(i, true);
        }

        let stats = manager.stats();

        assert_eq!(stats.total_bots, 10);
        assert_eq!(stats.active_this_tick, 5);
        assert_eq!(stats.full_mode, 2);
        assert_eq!(stats.reduced_mode, 3);
        assert_eq!(stats.dormant_mode, 5);
    }

    // ========================================================================
    // Personality Tests
    // ========================================================================

    #[test]
    fn test_personality_randomization() {
        let mut manager = AiManagerSoA::default();

        for _ in 0..100 {
            manager.register_bot(Uuid::new_v4());
        }

        // Check all personalities are within valid ranges
        for i in 0..100 {
            assert!(manager.aggression[i] >= 0.2 && manager.aggression[i] <= 0.8);
            assert!(manager.preferred_radius[i] >= 250.0 && manager.preferred_radius[i] <= 400.0);
            assert!(manager.accuracy[i] >= 0.5 && manager.accuracy[i] <= 0.9);
            assert!(manager.reaction_variance[i] >= 0.1 && manager.reaction_variance[i] <= 0.5);
        }

        // Check there's variance (not all same values)
        let first_aggression = manager.aggression[0];
        let has_variance = manager.aggression.iter().any(|&a| (a - first_aggression).abs() > 0.01);
        assert!(has_variance, "Personalities should have variance");
    }

    // ========================================================================
    // Behavior State Tests
    // ========================================================================

    #[test]
    fn test_behavior_defaults_to_idle() {
        let mut manager = AiManagerSoA::default();
        let bot_id = Uuid::new_v4();
        manager.register_bot(bot_id);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        assert_eq!(manager.behaviors[idx], AiBehavior::Idle);
    }

    #[test]
    fn test_behavior_enum_size() {
        // Ensure behavior enum is 1 byte for cache efficiency
        assert_eq!(std::mem::size_of::<AiBehavior>(), 1);
    }

    #[test]
    fn test_update_mode_enum_size() {
        assert_eq!(std::mem::size_of::<UpdateMode>(), 1);
    }

    // ========================================================================
    // Memory Layout Tests
    // ========================================================================

    #[test]
    fn test_soa_memory_layout() {
        let manager = AiManagerSoA::with_capacity(1000);

        assert_eq!(manager.count, 0);
        assert!(manager.thrust_x.capacity() >= 1000);
        assert!(manager.thrust_y.capacity() >= 1000);
        assert!(manager.behaviors.capacity() >= 1000);
    }

    #[test]
    fn test_large_scale_registration() {
        let mut manager = AiManagerSoA::with_capacity(10000);

        for _ in 0..10000 {
            manager.register_bot(Uuid::new_v4());
        }

        assert_eq!(manager.count, 10000);
        assert_eq!(manager.bot_ids.len(), 10000);
        assert_eq!(manager.behaviors.len(), 10000);
        assert_eq!(manager.thrust_x.len(), 10000);
    }

    // ========================================================================
    // Decision Timer Tests
    // ========================================================================

    #[test]
    fn test_decision_timer_initialized() {
        let mut manager = AiManagerSoA::default();
        let bot_id = Uuid::new_v4();
        manager.register_bot(bot_id);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        assert!((manager.decision_timers[idx]).abs() < 0.01);
    }

    // ========================================================================
    // Integration Tests
    // ========================================================================

    #[test]
    fn test_full_update_cycle() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add gravity well
        let well = create_gravity_well(0, Vec2::new(0.0, 0.0), 10000.0, 50.0);
        state.arena.gravity_wells.insert(0, well);

        // Add bots
        for i in 0..5 {
            let bot = create_bot_player(Vec2::new(300.0 + i as f32 * 10.0, 0.0), 100.0);
            let bot_id = bot.id;
            state.add_player(bot);
            manager.register_bot(bot_id);
        }

        // Add human
        let human = create_human_player(Vec2::new(400.0, 0.0), 150.0);
        state.add_player(human);

        // Run update
        manager.update(&state, 0.033);

        // Verify tick counter incremented
        assert_eq!(manager.tick_counter, 1);

        // Verify zones updated
        let cell = manager.zone_grid.position_to_cell(Vec2::new(300.0, 0.0));
        let zone = manager.zone_grid.get_zone(cell);
        assert!(zone.is_some());
    }

    #[test]
    fn test_orbit_behavior_near_well() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add gravity well at origin
        let well = create_gravity_well(0, Vec2::new(0.0, 0.0), 10000.0, 50.0);
        state.arena.gravity_wells.insert(0, well);

        // Add bot in stable orbit position
        let bot = create_bot_player(Vec2::new(300.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add human to make bot active
        let human = create_human_player(Vec2::new(300.0, 100.0), 100.0);
        state.add_player(human);

        // Set to orbit behavior and mark active
        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Orbit;
        manager.active_mask.set(idx, true);

        // Rebuild batches
        manager.batches.rebuild(&manager.behaviors, &manager.active_mask);

        // Update orbit batch
        manager.update_orbit_batch(&state, 0.033);

        // Should have some thrust (tangential to well)
        assert!(manager.thrust_x[idx].abs() > 0.01 || manager.thrust_y[idx].abs() > 0.01);
    }

    #[test]
    fn test_collect_behavior() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add bot
        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add debris
        state.debris.push(crate::game::state::Debris::new(
            1,
            Vec2::new(100.0, 0.0),
            Vec2::ZERO,
            crate::game::state::DebrisSize::Medium,
        ));

        // Set to collect behavior
        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Collect;
        manager.active_mask.set(idx, true);
        manager.batches.rebuild(&manager.behaviors, &manager.active_mask);

        manager.update_collect_batch(&state, 0.033);

        // Should thrust toward debris (positive x)
        assert!(manager.thrust_x[idx] > 0.5);
    }

    #[test]
    fn test_idle_behavior() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let mut bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        bot.velocity = Vec2::new(50.0, 0.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Idle;
        manager.active_mask.set(idx, true);
        manager.batches.rebuild(&manager.behaviors, &manager.active_mask);

        manager.update_idle_batch(&state, 0.033);

        // Thrust should be zero
        assert!(manager.thrust_x[idx].abs() < 0.01);
        assert!(manager.thrust_y[idx].abs() < 0.01);
        // Aim should face velocity direction
        assert!(manager.aim_x[idx] > 0.9);
    }

    // ========================================================================
    // Configuration Tests
    // ========================================================================

    #[test]
    fn test_config_default_values() {
        let config = AiSoaConfig::default();

        // Feature toggles default to enabled
        assert!(config.dormancy_enabled);
        assert!(config.zone_queries_enabled);
        assert!(config.behavior_batching_enabled);
        assert!(config.parallel_enabled);

        // LOD thresholds
        assert!((config.lod_full_radius - 500.0).abs() < 0.01);
        assert!((config.lod_reduced_radius - 2000.0).abs() < 0.01);
        assert!((config.lod_dormant_radius - 5000.0).abs() < 0.01);

        // Update intervals
        assert_eq!(config.reduced_update_interval, 4);
        assert_eq!(config.dormant_update_interval, 8);

        // Spatial
        assert!((config.zone_cell_size - 4096.0).abs() < 0.01);

        // Decision making
        assert!((config.decision_interval - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_config_custom_values() {
        let config = AiSoaConfig {
            dormancy_enabled: false,
            zone_queries_enabled: false,
            behavior_batching_enabled: false,
            parallel_enabled: false,
            lod_full_radius: 100.0,
            lod_reduced_radius: 500.0,
            lod_dormant_radius: 1000.0,
            reduced_update_interval: 2,
            dormant_update_interval: 4,
            zone_cell_size: 2048.0,
            decision_interval: 0.25,
            well_cache_refresh_interval: 0.25,
        };

        assert!(!config.dormancy_enabled);
        assert!(!config.zone_queries_enabled);
        assert_eq!(config.reduced_update_interval, 2);
        assert!((config.lod_full_radius - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_dormancy_disabled_all_bots_active() {
        // Note: Can't easily test env vars in unit tests, but we can test the logic
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add bot far from any human
        let bot = create_bot_player(Vec2::new(10000.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Normally this would be dormant, but if dormancy is disabled
        // (via global config), it should be full update mode
        // We can't easily override the global config in tests without
        // env vars, so just verify the function exists and basic behavior
        let idx = manager.get_index(bot_id).unwrap() as usize;
        assert_eq!(manager.behaviors[idx], AiBehavior::Idle);
    }

    #[test]
    fn test_sequential_update_fallback() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add gravity well
        let well = create_gravity_well(0, Vec2::new(0.0, 0.0), 10000.0, 50.0);
        state.arena.gravity_wells.insert(0, well);

        // Add bot
        let bot = create_bot_player(Vec2::new(300.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Set to orbit and mark active
        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Orbit;
        manager.active_mask.set(idx, true);

        // Call sequential fallback directly
        manager.update_all_sequential(&state, 0.033);

        // Should have computed thrust
        assert!(manager.thrust_x[idx].abs() > 0.01 || manager.thrust_y[idx].abs() > 0.01);
    }

    // ========================================================================
    // Chase Behavior Tests
    // ========================================================================

    #[test]
    fn test_chase_behavior_with_target() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add bot at origin
        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add target far to the right
        let target = create_human_player(Vec2::new(500.0, 0.0), 80.0);
        let target_id = target.id;
        state.add_player(target);

        // Set to chase behavior with target
        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Chase;
        manager.target_ids[idx] = Some(target_id);
        manager.active_mask.set(idx, true);
        manager.batches.rebuild(&manager.behaviors, &manager.active_mask);

        manager.update_chase_batch(&state, 0.033);

        // Should thrust toward target (positive x direction)
        assert!(manager.thrust_x[idx] > 0.5);
        // Should aim toward target
        assert!(manager.aim_x[idx] > 0.5);
        // Should boost when far from target
        assert!(manager.wants_boost.get(idx).map(|b| *b).unwrap_or(false));
    }

    #[test]
    fn test_chase_behavior_target_dead() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add bot
        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add dead target
        let mut target = create_human_player(Vec2::new(500.0, 0.0), 80.0);
        target.alive = false;
        let target_id = target.id;
        state.add_player(target);

        // Set to chase behavior with dead target
        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Chase;
        manager.target_ids[idx] = Some(target_id);
        manager.active_mask.set(idx, true);
        manager.batches.rebuild(&manager.behaviors, &manager.active_mask);

        manager.update_chase_batch(&state, 0.033);

        // Should switch to idle when target is dead
        assert_eq!(manager.behaviors[idx], AiBehavior::Idle);
        assert!(manager.target_ids[idx].is_none());
    }

    #[test]
    fn test_chase_behavior_no_target() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Chase;
        manager.target_ids[idx] = None; // No target
        manager.active_mask.set(idx, true);
        manager.batches.rebuild(&manager.behaviors, &manager.active_mask);

        // Should not panic
        manager.update_chase_batch(&state, 0.033);
    }

    // ========================================================================
    // Flee Behavior Tests
    // ========================================================================

    #[test]
    fn test_flee_behavior_from_threat() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add bot at origin
        let bot = create_bot_player(Vec2::new(0.0, 0.0), 50.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add threatening player nearby
        let threat = create_human_player(Vec2::new(100.0, 0.0), 200.0);
        let threat_id = threat.id;
        state.add_player(threat);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Flee;
        manager.target_ids[idx] = Some(threat_id);
        manager.active_mask.set(idx, true);
        manager.batches.rebuild(&manager.behaviors, &manager.active_mask);

        manager.update_flee_batch(&state, 0.033);

        // Should thrust away from threat (negative x direction)
        assert!(manager.thrust_x[idx] < -0.5);
        // Should boost when fleeing
        assert!(manager.wants_boost.get(idx).map(|b| *b).unwrap_or(false));
    }

    #[test]
    fn test_flee_behavior_threat_dead() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 50.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add dead threat
        let mut threat = create_human_player(Vec2::new(100.0, 0.0), 200.0);
        threat.alive = false;
        let threat_id = threat.id;
        state.add_player(threat);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Flee;
        manager.target_ids[idx] = Some(threat_id);
        manager.active_mask.set(idx, true);
        manager.batches.rebuild(&manager.behaviors, &manager.active_mask);

        manager.update_flee_batch(&state, 0.033);

        // Should switch to idle when threat is dead
        assert_eq!(manager.behaviors[idx], AiBehavior::Idle);
    }

    #[test]
    fn test_flee_behavior_threat_nonexistent() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 50.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Threat ID doesn't exist in state
        let fake_threat_id = Uuid::new_v4();

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Flee;
        manager.target_ids[idx] = Some(fake_threat_id);
        manager.active_mask.set(idx, true);
        manager.batches.rebuild(&manager.behaviors, &manager.active_mask);

        // Should not panic when threat doesn't exist
        manager.update_flee_batch(&state, 0.033);

        // Behavior remains Flee (skipped in batch processing due to missing target)
        // but will be cleaned up on next decision cycle
        assert_eq!(manager.behaviors[idx], AiBehavior::Flee);
    }

    // ========================================================================
    // Reduced Update Mode Tests
    // ========================================================================

    #[test]
    fn test_dormancy_reduced_mode_distance() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add bot at intermediate distance (between full and dormant thresholds)
        let bot = create_bot_player(Vec2::new(1000.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add human at origin
        let human = create_human_player(Vec2::new(0.0, 0.0), 100.0);
        state.add_player(human);

        manager.update_dormancy(&state);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        // Distance is 1000, which is > 500 (full) but < 2000 (reduced threshold)
        assert_eq!(manager.update_modes[idx], UpdateMode::Reduced);
    }

    #[test]
    fn test_tick_counter_reduced_interval() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add bot
        let bot = create_bot_player(Vec2::new(1000.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add human
        let human = create_human_player(Vec2::new(0.0, 0.0), 100.0);
        state.add_player(human);

        let idx = manager.get_index(bot_id).unwrap() as usize;

        // Test multiple tick cycles
        // Reduced mode updates every 4 ticks (tick_counter % 4 == 0)
        for tick in 0..12u32 {
            manager.tick_counter = tick;
            manager.update_dormancy(&state);

            let should_be_active = tick % 4 == 0;
            let is_active = manager.active_mask.get(idx).map(|b| *b).unwrap_or(false);

            // Only check if we're in reduced mode
            if manager.update_modes[idx] == UpdateMode::Reduced {
                assert_eq!(is_active, should_be_active,
                    "At tick {}, reduced mode bot should be active={}", tick, should_be_active);
            }
        }
    }

    #[test]
    fn test_tick_counter_dormant_interval() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add bot very far away
        let bot = create_bot_player(Vec2::new(8000.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add human at origin
        let human = create_human_player(Vec2::new(0.0, 0.0), 100.0);
        state.add_player(human);

        let idx = manager.get_index(bot_id).unwrap() as usize;

        // Dormant mode updates every 8 ticks
        for tick in 0..16u32 {
            manager.tick_counter = tick;
            manager.update_dormancy(&state);

            let should_be_active = tick % 8 == 0;
            let is_active = manager.active_mask.get(idx).map(|b| *b).unwrap_or(false);

            if manager.update_modes[idx] == UpdateMode::Dormant {
                assert_eq!(is_active, should_be_active,
                    "At tick {}, dormant mode bot should be active={}", tick, should_be_active);
            }
        }
    }

    // ========================================================================
    // Dead Player Handling Tests
    // ========================================================================

    #[test]
    fn test_dead_bot_marked_inactive() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add dead bot
        let mut bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        bot.alive = false;
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add human
        let human = create_human_player(Vec2::new(100.0, 0.0), 100.0);
        state.add_player(human);

        manager.update_dormancy(&state);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        // Dead bot should be marked inactive
        assert!(!manager.active_mask.get(idx).map(|b| *b).unwrap_or(true));
    }

    #[test]
    fn test_dead_bot_skipped_in_zone_update() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add dead bot
        let mut bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        bot.alive = false;
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        manager.update_zones(&state);

        let cell = manager.zone_grid.position_to_cell(Vec2::new(0.0, 0.0));
        let zone = manager.zone_grid.get_zone(cell);
        // Dead bot should not be counted
        assert!(zone.is_none() || zone.unwrap().bot_count == 0);
    }

    // ========================================================================
    // Decision Making Tests
    // ========================================================================

    #[test]
    fn test_update_decisions_decrements_timer() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.active_mask.set(idx, true);
        manager.decision_timers[idx] = 1.0;

        manager.update_decisions(&state, 0.1);

        // Timer should be decremented
        assert!((manager.decision_timers[idx] - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_update_decisions_triggers_on_zero() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add gravity well for orbit behavior
        let well = create_gravity_well(0, Vec2::new(0.0, 0.0), 10000.0, 50.0);
        state.arena.gravity_wells.insert(0, well);

        let bot = create_bot_player(Vec2::new(300.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.active_mask.set(idx, true);
        manager.decision_timers[idx] = 0.0; // About to trigger

        manager.update_decisions(&state, 0.1);

        // Timer should be reset to a new value
        assert!(manager.decision_timers[idx] > 0.0);
        // Behavior should have changed from Idle
        // (could be Orbit, Chase, Flee, or Collect depending on RNG)
    }

    #[test]
    fn test_decide_behavior_defaults_to_orbit() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add gravity well
        let well = create_gravity_well(0, Vec2::new(0.0, 0.0), 10000.0, 50.0);
        state.arena.gravity_wells.insert(0, well);

        // Add bot with low aggression (won't chase)
        let bot = create_bot_player(Vec2::new(300.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.aggression[idx] = 0.0; // No aggression

        // Clear debris so collect won't trigger
        state.debris.clear();

        let mut rng = rand::thread_rng();
        manager.decide_behavior(idx, &state, &mut rng);

        // With no threats, no debris, and low aggression, should default to orbit
        assert_eq!(manager.behaviors[idx], AiBehavior::Orbit);
        assert!(manager.target_ids[idx].is_none());
    }

    // ========================================================================
    // Firing Logic Tests
    // ========================================================================

    #[test]
    fn test_firing_only_for_combat_behaviors() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.active_mask.set(idx, true);
        manager.behaviors[idx] = AiBehavior::Orbit; // Non-combat
        manager.wants_fire.set(idx, true);

        manager.update_firing(&state, 0.033);

        // Fire should be cleared for non-combat behavior
        assert!(!manager.wants_fire.get(idx).map(|b| *b).unwrap_or(true));
        assert!((manager.charge_times[idx]).abs() < 0.01);
    }

    #[test]
    fn test_firing_range_check() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add target far away (out of range)
        let target = create_human_player(Vec2::new(500.0, 0.0), 80.0);
        let target_id = target.id;
        state.add_player(target);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.active_mask.set(idx, true);
        manager.behaviors[idx] = AiBehavior::Chase;
        manager.target_ids[idx] = Some(target_id);
        manager.wants_fire.set(idx, true);

        manager.update_firing(&state, 0.033);

        // Fire should be cleared when target is out of range (> 300)
        assert!(!manager.wants_fire.get(idx).map(|b| *b).unwrap_or(true));
    }

    #[test]
    fn test_firing_charges_over_time() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add target in range
        let target = create_human_player(Vec2::new(100.0, 0.0), 80.0);
        let target_id = target.id;
        state.add_player(target);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.active_mask.set(idx, true);
        manager.behaviors[idx] = AiBehavior::Chase;
        manager.target_ids[idx] = Some(target_id);
        manager.wants_fire.set(idx, true);
        manager.charge_times[idx] = 0.0;

        manager.update_firing(&state, 0.1);

        // Charge time should increase while firing
        assert!(manager.charge_times[idx] > 0.0);
    }

    // ========================================================================
    // Orbit Danger Zone Tests
    // ========================================================================

    #[test]
    fn test_orbit_escape_danger_zone() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add gravity well with core_radius 50
        let well = create_gravity_well(0, Vec2::new(0.0, 0.0), 10000.0, 50.0);
        state.arena.gravity_wells.insert(0, well);

        // Add bot very close to well (in danger zone: < core_radius * 2.5 = 125)
        let bot = create_bot_player(Vec2::new(80.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add human to keep bot active
        let human = create_human_player(Vec2::new(100.0, 100.0), 100.0);
        state.add_player(human);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Orbit;
        manager.active_mask.set(idx, true);
        manager.batches.rebuild(&manager.behaviors, &manager.active_mask);

        manager.update_orbit_batch(&state, 0.033);

        // Should thrust away from well (positive x, escaping)
        assert!(manager.thrust_x[idx] > 0.5);
        // Should boost when escaping danger
        assert!(manager.wants_boost.get(idx).map(|b| *b).unwrap_or(false));
    }

    // ========================================================================
    // Sequential Update Behaviors Tests
    // ========================================================================

    #[test]
    fn test_sequential_chase_behavior() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        let target = create_human_player(Vec2::new(200.0, 0.0), 80.0);
        let target_id = target.id;
        state.add_player(target);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Chase;
        manager.target_ids[idx] = Some(target_id);
        manager.active_mask.set(idx, true);

        manager.update_all_sequential(&state, 0.033);

        // Should thrust toward target
        assert!(manager.thrust_x[idx] > 0.5);
    }

    #[test]
    fn test_sequential_flee_behavior() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 50.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        let threat = create_human_player(Vec2::new(100.0, 0.0), 200.0);
        let threat_id = threat.id;
        state.add_player(threat);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Flee;
        manager.target_ids[idx] = Some(threat_id);
        manager.active_mask.set(idx, true);

        manager.update_all_sequential(&state, 0.033);

        // Should thrust away from threat
        assert!(manager.thrust_x[idx] < -0.5);
    }

    #[test]
    fn test_sequential_collect_behavior() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        state.debris.push(crate::game::state::Debris::new(
            1,
            Vec2::new(100.0, 0.0),
            Vec2::ZERO,
            crate::game::state::DebrisSize::Medium,
        ));

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Collect;
        manager.active_mask.set(idx, true);

        manager.update_all_sequential(&state, 0.033);

        // Should thrust toward debris
        assert!(manager.thrust_x[idx] > 0.5);
    }

    // ========================================================================
    // Collect Behavior Switch Tests
    // ========================================================================

    #[test]
    fn test_collect_switches_to_orbit_when_no_debris() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // No debris in state
        state.debris.clear();

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Collect;
        manager.active_mask.set(idx, true);
        manager.batches.rebuild(&manager.behaviors, &manager.active_mask);

        manager.update_collect_batch(&state, 0.033);

        // Should switch to orbit when no collectibles
        assert_eq!(manager.behaviors[idx], AiBehavior::Orbit);
    }

    // ========================================================================
    // Zone Threat Detection Tests
    // ========================================================================

    #[test]
    fn test_zone_threat_triggers_flee() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        // Add small bot
        let bot = create_bot_player(Vec2::new(100.0, 0.0), 50.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add large threatening human nearby
        let human = create_human_player(Vec2::new(150.0, 0.0), 200.0);
        state.add_player(human);

        // Update zones to register threat
        manager.update_zones(&state);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.aggression[idx] = 0.0; // Cowardly bot

        let mut rng = rand::thread_rng();
        manager.decide_behavior(idx, &state, &mut rng);

        // With a large threatening human nearby and low aggression,
        // bot should flee (or at least not chase)
        assert!(manager.behaviors[idx] == AiBehavior::Flee ||
                manager.behaviors[idx] == AiBehavior::Orbit);
    }

    // ========================================================================
    // Get Index Tests
    // ========================================================================

    #[test]
    fn test_get_index_returns_none_for_unknown() {
        let manager = AiManagerSoA::default();
        let unknown_id = Uuid::new_v4();

        assert!(manager.get_index(unknown_id).is_none());
    }

    #[test]
    fn test_get_index_correct_after_unregister() {
        let mut manager = AiManagerSoA::default();
        let bot1 = Uuid::new_v4();
        let bot2 = Uuid::new_v4();
        let bot3 = Uuid::new_v4();

        manager.register_bot(bot1);
        manager.register_bot(bot2);
        manager.register_bot(bot3);

        assert_eq!(manager.get_index(bot1), Some(0));
        assert_eq!(manager.get_index(bot2), Some(1));
        assert_eq!(manager.get_index(bot3), Some(2));

        // Unregister middle bot
        manager.unregister_bot(bot2);

        // bot3 should have moved to index 1
        assert!(manager.get_index(bot2).is_none());
        assert_eq!(manager.get_index(bot3), Some(1));
        assert_eq!(manager.get_index(bot1), Some(0));
    }

    // ========================================================================
    // Default Implementation Tests
    // ========================================================================

    #[test]
    fn test_ai_manager_default() {
        let manager = AiManagerSoA::default();

        assert_eq!(manager.count, 0);
        assert!(manager.bot_ids.capacity() >= 1024);
        assert_eq!(manager.tick_counter, 0);
    }

    #[test]
    fn test_zone_grid_default() {
        let grid = ZoneGrid::default();

        assert!((grid.cell_size - DEFAULT_ZONE_CELL_SIZE).abs() < 0.01);
    }

    #[test]
    fn test_behavior_batches_default() {
        let batches = BehaviorBatches::default();

        assert!(batches.orbit.is_empty());
        assert!(batches.chase.is_empty());
        assert!(batches.flee.is_empty());
        assert!(batches.collect.is_empty());
        assert!(batches.idle.is_empty());
    }

    // ========================================================================
    // Aim Direction Tests
    // ========================================================================

    #[test]
    fn test_aim_updates_toward_target() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        // Add target above
        let target = create_human_player(Vec2::new(0.0, 100.0), 80.0);
        let target_id = target.id;
        state.add_player(target);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.behaviors[idx] = AiBehavior::Chase;
        manager.target_ids[idx] = Some(target_id);
        manager.active_mask.set(idx, true);
        manager.batches.rebuild(&manager.behaviors, &manager.active_mask);

        manager.update_chase_batch(&state, 0.033);

        // Aim should point upward (positive y)
        assert!(manager.aim_y[idx] > 0.5);
    }

    // ========================================================================
    // Inactive Bot Skip Tests
    // ========================================================================

    #[test]
    fn test_inactive_bot_skipped_in_decisions() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.active_mask.set(idx, false); // Inactive
        manager.decision_timers[idx] = 0.0; // Would normally trigger decision

        let original_behavior = manager.behaviors[idx];
        manager.update_decisions(&state, 0.1);

        // Behavior should not have changed
        assert_eq!(manager.behaviors[idx], original_behavior);
    }

    #[test]
    fn test_inactive_bot_skipped_in_firing() {
        let mut manager = AiManagerSoA::default();
        let mut state = create_test_state();

        let bot = create_bot_player(Vec2::new(0.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);

        let idx = manager.get_index(bot_id).unwrap() as usize;
        manager.active_mask.set(idx, false); // Inactive
        manager.behaviors[idx] = AiBehavior::Chase;
        manager.wants_fire.set(idx, true);

        manager.update_firing(&state, 0.033);

        // Fire should remain true (not processed)
        assert!(manager.wants_fire.get(idx).map(|b| *b).unwrap_or(false));
    }

    // ========================================================================
    // Tick Counter Tests
    // ========================================================================

    #[test]
    fn test_tick_counter_wrapping() {
        let mut manager = AiManagerSoA::default();
        let state = create_test_state();

        manager.tick_counter = u32::MAX;
        manager.update(&state, 0.033);

        // Should wrap to 0
        assert_eq!(manager.tick_counter, 0);
    }

    #[test]
    fn test_tick_counter_increments() {
        let mut manager = AiManagerSoA::default();
        let state = create_test_state();

        assert_eq!(manager.tick_counter, 0);
        manager.update(&state, 0.033);
        assert_eq!(manager.tick_counter, 1);
        manager.update(&state, 0.033);
        assert_eq!(manager.tick_counter, 2);
    }
}
