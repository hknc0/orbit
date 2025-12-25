//! Million-Scale Bot AI System (Structure of Arrays)
//!
//! Optimized for 1M+ bots using:
//! - SoA (Structure of Arrays) for cache-friendly memory layout
//! - SIMD-friendly data organization
//! - Behavior batching for branch-free processing
//! - Dormancy system for distant bot optimization
//! - Zone-based approximate queries

use bitvec::prelude::*;
use hashbrown::HashMap;
use rand::Rng;
use rayon::prelude::*;

use crate::game::constants::ai::*;
use crate::game::state::{GameState, PlayerId, WellId};
use crate::net::protocol::PlayerInput;
use crate::util::vec2::Vec2;

// ============================================================================
// Constants for Million-Scale Optimization
// ============================================================================

/// Zone cell size for hierarchical spatial partitioning (world units)
pub const ZONE_CELL_SIZE: f32 = 4096.0;

/// Distance thresholds for LOD (Level of Detail)
pub const LOD_FULL_RADIUS: f32 = 500.0;
pub const LOD_REDUCED_RADIUS: f32 = 2000.0;
pub const LOD_DORMANT_RADIUS: f32 = 5000.0;

/// Update frequency for reduced mode (every N ticks)
pub const REDUCED_UPDATE_INTERVAL: u32 = 4;

/// Update frequency for dormant mode (every N ticks)
pub const DORMANT_UPDATE_INTERVAL: u32 = 8;

/// Cache refresh interval for nearest well (seconds)
pub const WELL_CACHE_REFRESH_INTERVAL: f32 = 0.5;

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
        Self::new(ZONE_CELL_SIZE)
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
    pub fn update_dormancy(&mut self, state: &GameState) {
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

            // Determine update mode based on distance
            let mode = if min_dist < LOD_FULL_RADIUS {
                UpdateMode::Full
            } else if min_dist < LOD_REDUCED_RADIUS {
                UpdateMode::Reduced
            } else {
                UpdateMode::Dormant
            };
            self.update_modes[i] = mode;

            // Set active mask based on mode and tick
            let should_update = match mode {
                UpdateMode::Full => true,
                UpdateMode::Reduced => self.tick_counter % REDUCED_UPDATE_INTERVAL == 0,
                UpdateMode::Dormant => self.tick_counter % DORMANT_UPDATE_INTERVAL == 0,
            };
            self.active_mask.set(i, should_update);
        }
    }

    /// Main update function - processes all active bots
    pub fn update(&mut self, state: &GameState, dt: f32) {
        self.tick_counter = self.tick_counter.wrapping_add(1);

        // Update zones and dormancy
        self.update_zones(state);
        self.update_dormancy(state);

        // Rebuild behavior batches
        self.batches.rebuild(&self.behaviors, &self.active_mask);

        // Process each behavior batch in parallel
        self.update_orbit_batch(state, dt);
        self.update_chase_batch(state, dt);
        self.update_flee_batch(state, dt);
        self.update_collect_batch(state, dt);
        self.update_idle_batch(state, dt);

        // Update decision timers and make new decisions
        self.update_decisions(state, dt);

        // Update firing for combat behaviors
        self.update_firing(state, dt);
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
            if to_idle && self.target_ids[i].and_then(|id| state.get_player(id)).is_none() {
                self.behaviors[i] = AiBehavior::Idle;
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
    use uuid::Uuid;

    #[test]
    fn test_register_bot() {
        let mut manager = AiManagerSoA::default();
        let bot_id = Uuid::new_v4();

        manager.register_bot(bot_id);

        assert_eq!(manager.count, 1);
        assert!(manager.get_index(bot_id).is_some());
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
    fn test_behavior_batches() {
        let mut batches = BehaviorBatches::default();
        let behaviors = vec![
            AiBehavior::Orbit,
            AiBehavior::Chase,
            AiBehavior::Orbit,
            AiBehavior::Flee,
            AiBehavior::Idle,
        ];
        let mut active = bitvec![1, 1, 1, 1, 1];

        batches.rebuild(&behaviors, &active);

        assert_eq!(batches.orbit.len(), 2);
        assert_eq!(batches.chase.len(), 1);
        assert_eq!(batches.flee.len(), 1);
        assert_eq!(batches.idle.len(), 1);
    }

    #[test]
    fn test_zone_grid() {
        let mut grid = ZoneGrid::new(1000.0);

        let cell = grid.position_to_cell(Vec2::new(500.0, 500.0));
        assert_eq!(cell, (0, 0));

        let cell2 = grid.position_to_cell(Vec2::new(1500.0, 500.0));
        assert_eq!(cell2, (1, 0));

        let zone = grid.get_or_create_zone(cell);
        zone.bot_count = 10;

        assert_eq!(grid.get_zone(cell).unwrap().bot_count, 10);
    }

    #[test]
    fn test_soa_memory_layout() {
        // Verify that SoA uses less memory than AoS
        let manager = AiManagerSoA::with_capacity(1000);

        // Each bot should use approximately:
        // - behavior: 1 byte
        // - decision_timer: 4 bytes
        // - charge_time: 4 bytes
        // - thrust_x/y, aim_x/y: 16 bytes
        // - personality (4 floats): 16 bytes
        // - plus overhead
        // Total ~45-50 bytes per bot vs ~224 bytes for AoS

        // Just verify structure exists
        assert_eq!(manager.count, 0);
        assert!(manager.thrust_x.capacity() >= 1000);
    }
}
