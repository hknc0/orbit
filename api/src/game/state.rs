//! Game state definitions and structures
//!
//! Contains all entities (players, projectiles, debris) and arena state.

// Allow dead_code for utility methods that are part of the public API
#![allow(dead_code)]

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use uuid::Uuid;

use crate::config::ArenaScalingConfig;
use crate::game::constants::{arena, mass, spawn};
use crate::game::spatial::WellSpatialGrid;
use crate::util::vec2::Vec2;

/// Unique player identifier
pub type PlayerId = Uuid;

/// Entity identifier for non-player entities
pub type EntityId = u64;

/// Player state
///
/// OPTIMIZATION: Fields are ordered for cache efficiency during physics updates.
/// Hot fields (accessed every tick) are grouped first to fit in cache line 1.
/// Warm fields (accessed during collisions) are in cache line 2.
/// Cold fields (rarely accessed in hot path) are at the end.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    // === HOT FIELDS (Cache Line 1 - accessed every physics tick) ===
    /// Player position in world space
    pub position: Vec2,
    /// Player velocity vector
    pub velocity: Vec2,
    /// Player mass (affects collision and gravity)
    pub mass: f32,
    /// Whether player is alive
    pub alive: bool,
    /// Remaining spawn protection time (seconds)
    pub spawn_protection: f32,
    /// Player rotation (radians)
    pub rotation: f32,

    // === WARM FIELDS (Cache Line 2 - accessed during collisions/scoring) ===
    /// Number of kills
    pub kills: u32,
    /// Number of deaths
    pub deaths: u32,
    /// Timer until respawn (0 = can respawn, >0 = waiting)
    #[serde(default)]
    pub respawn_timer: f32,
    /// Whether player is a bot
    pub is_bot: bool,
    /// Player color palette index
    pub color_index: u8,
    /// Tick when player spawned/respawned (for birth animation detection)
    #[serde(default)]
    pub spawn_tick: u64,

    // === COLD FIELDS (Cache Line 3 - rarely accessed in hot path) ===
    /// Unique player identifier
    pub id: PlayerId,
    /// Player display name
    pub name: String,
}

impl Player {
    pub fn new(id: PlayerId, name: String, is_bot: bool, color_index: u8) -> Self {
        Self {
            // HOT fields
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            mass: mass::STARTING,
            alive: true,
            spawn_protection: spawn::PROTECTION_DURATION,
            rotation: 0.0,
            // WARM fields
            kills: 0,
            deaths: 0,
            respawn_timer: 0.0,
            is_bot,
            color_index,
            spawn_tick: 0, // Set properly when added to game via add_player
            // COLD fields
            id,
            name,
        }
    }

    /// Get player's collision radius based on mass
    pub fn radius(&self) -> f32 {
        crate::game::constants::mass_to_radius(self.mass)
    }

    /// Check if player has spawn protection active
    pub fn has_spawn_protection(&self) -> bool {
        self.spawn_protection > 0.0
    }

    /// Check if player is in danger zone (too close to center)
    pub fn in_danger_zone(&self) -> bool {
        self.position.length() < arena::CORE_RADIUS
    }

    /// Check if player is outside escape radius
    pub fn outside_arena(&self) -> bool {
        self.position.length() > arena::ESCAPE_RADIUS
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new(Uuid::new_v4(), "Player".to_string(), false, 0)
    }
}

/// Projectile (ejected mass)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projectile {
    pub id: EntityId,
    pub owner_id: PlayerId,
    pub position: Vec2,
    pub velocity: Vec2,
    pub mass: f32,
    pub lifetime: f32,
}

impl Projectile {
    pub fn new(id: EntityId, owner_id: PlayerId, position: Vec2, velocity: Vec2, mass: f32) -> Self {
        Self {
            id,
            owner_id,
            position,
            velocity,
            mass,
            lifetime: crate::game::constants::eject::LIFETIME,
        }
    }

    pub fn radius(&self) -> f32 {
        crate::game::constants::mass_to_radius(self.mass)
    }

    pub fn is_expired(&self) -> bool {
        self.lifetime <= 0.0
    }
}

/// Debris size categories
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DebrisSize {
    Small,
    Medium,
    Large,
}

impl DebrisSize {
    pub fn mass(&self) -> f32 {
        match self {
            DebrisSize::Small => 5.0,
            DebrisSize::Medium => 15.0,
            DebrisSize::Large => 30.0,
        }
    }

    pub fn radius(&self) -> f32 {
        crate::game::constants::mass_to_radius(self.mass())
    }
}

/// Debris (collectible mass fragments)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Debris {
    pub id: EntityId,
    pub position: Vec2,
    pub velocity: Vec2,
    pub size: DebrisSize,
    pub lifetime: f32,
}

impl Debris {
    pub fn new(id: EntityId, position: Vec2, velocity: Vec2, size: DebrisSize) -> Self {
        use crate::game::constants::debris_spawning::LIFETIME;
        Self {
            id,
            position,
            velocity,
            size,
            lifetime: LIFETIME,
        }
    }

    pub fn mass(&self) -> f32 {
        self.size.mass()
    }

    pub fn radius(&self) -> f32 {
        self.size.radius()
    }
}

/// Unique identifier for gravity wells (stable across removals)
pub type WellId = u32;

/// Central well ID (supermassive black hole at origin)
pub const CENTRAL_WELL_ID: WellId = 0;

/// A gravity well (attractor point)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GravityWell {
    /// Unique stable ID (doesn't change when other wells are removed)
    pub id: WellId,
    pub position: Vec2,
    /// Target position for smooth lerping during arena scaling
    /// Wells lerp toward this to avoid jittery movement
    #[serde(default)]
    pub target_position: Vec2,
    pub mass: f32,
    pub core_radius: f32, // Death zone radius
    /// Timer until next explosion (counts down)
    #[serde(default)]
    pub explosion_timer: f32,
    /// Whether the well is currently charging (pre-explosion warning)
    #[serde(default)]
    pub is_charging: bool,
}

impl GravityWell {
    pub fn new(id: WellId, position: Vec2, mass: f32, core_radius: f32) -> Self {
        Self {
            id,
            position,
            target_position: position, // Start at position (no lerping needed)
            mass,
            core_radius,
            explosion_timer: crate::config::GravityWaveConfig::global().random_explosion_delay(),
            is_charging: false,
        }
    }

    /// Lerp position toward target_position for smooth movement
    /// Returns true if position changed
    pub fn lerp_to_target(&mut self, lerp_factor: f32) -> bool {
        let diff = self.target_position - self.position;
        let dist_sq = diff.length_sq();

        // If within 1 unit, snap to target
        if dist_sq < 1.0 {
            if dist_sq > 0.0001 {
                self.position = self.target_position;
                return true;
            }
            return false;
        }

        // Smooth lerp toward target
        self.position = self.position + diff * lerp_factor;
        true
    }

}

/// An expanding gravity wave from a well explosion
#[derive(Debug, Clone)]
pub struct GravityWave {
    /// Center position of the wave
    pub position: Vec2,
    /// Current radius of the expanding wave front
    pub radius: f32,
    /// Wave strength (force multiplier)
    pub strength: f32,
    /// Age of the wave in seconds
    pub age: f32,
    /// Players already hit by this wave (to apply impulse only once)
    /// OPTIMIZATION: SmallVec avoids heap allocation for typical wave hits (4-16 players)
    pub hit_players: SmallVec<[PlayerId; 16]>,
}

impl GravityWave {
    pub fn new(position: Vec2, strength: f32) -> Self {
        Self {
            position,
            radius: 0.0,
            strength,
            age: 0.0,
            hit_players: SmallVec::new(),
        }
    }
}

/// Arena state (zone collapse)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arena {
    pub core_radius: f32,
    pub inner_radius: f32,
    pub middle_radius: f32,
    pub outer_radius: f32,
    pub escape_radius: f32,
    pub collapse_phase: u8,
    pub is_collapsing: bool,
    pub collapse_progress: f32,
    pub time_until_collapse: f32,
    /// Dynamic scale factor based on player count (1.0 = default)
    #[serde(default = "default_scale")]
    pub scale: f32,
    /// Gravity wells stored by ID for O(1) lookup/removal (scales to 1000s of wells)
    #[serde(default)]
    pub gravity_wells: HashMap<WellId, GravityWell>,
    /// Spatial grid for efficient well lookups (used in limited gravity mode)
    #[serde(skip)]
    pub well_grid: WellSpatialGrid,
    /// Shrink delay counter - arena only shrinks after this reaches 0
    /// Prevents sudden shrinking when players leave temporarily
    #[serde(default)]
    pub shrink_delay_ticks: u32,
    /// Next well ID to assign (monotonically increasing)
    #[serde(default = "default_next_well_id")]
    pub next_well_id: WellId,
    /// Base angular offset for golden angle well distribution.
    /// Initialized once at arena creation, reused for all well additions
    /// to maintain consistent golden angle spacing across batches.
    #[serde(default = "default_well_base_offset")]
    pub well_base_offset: f32,
    /// Next index for golden angle distribution (monotonically increasing, NEVER reused)
    /// This ensures new wells always get unique angles, even after other wells are destroyed.
    /// Unlike well count, this counter only goes up - destroyed wells don't free their angle slots.
    #[serde(default)]
    pub next_well_angle_index: u32,
}

fn default_next_well_id() -> WellId { 1 }

fn default_well_base_offset() -> f32 {
    use rand::Rng;
    rand::thread_rng().gen_range(0.0..std::f32::consts::TAU)
}

fn default_scale() -> f32 { 1.0 }

impl Default for Arena {
    fn default() -> Self {
        use crate::game::constants::physics::CENTRAL_MASS;
        let central_well = GravityWell::new(CENTRAL_WELL_ID, Vec2::ZERO, CENTRAL_MASS, arena::CORE_RADIUS);
        let mut wells = HashMap::with_capacity(32);
        wells.insert(CENTRAL_WELL_ID, central_well.clone());

        // Initialize well grid with the central well
        let mut well_grid = WellSpatialGrid::default();
        well_grid.insert(CENTRAL_WELL_ID, central_well.position);

        Self {
            core_radius: arena::CORE_RADIUS,
            inner_radius: arena::INNER_RADIUS,
            middle_radius: arena::MIDDLE_RADIUS,
            outer_radius: arena::OUTER_RADIUS,
            escape_radius: arena::ESCAPE_RADIUS,
            collapse_phase: 0,
            is_collapsing: false,
            collapse_progress: 0.0,
            time_until_collapse: arena::COLLAPSE_INTERVAL,
            scale: 1.0,
            gravity_wells: wells,
            well_grid,
            shrink_delay_ticks: 0,
            next_well_id: 1, // Central well uses ID 0
            well_base_offset: default_well_base_offset(),
            next_well_angle_index: 0, // Start at 0, increment for each well added
        }
    }
}

impl Arena {
    /// Get the current safe radius based on collapse progress
    /// Uses escape_radius directly since it's already lerped to the correct size
    /// (scale is derived from escape_radius, so multiplying them would be quadratic)
    pub fn current_safe_radius(&self) -> f32 {
        let base = self.escape_radius;
        let reduction_per_phase = (base - self.core_radius) / arena::COLLAPSE_PHASES as f32;
        base - (self.collapse_phase as f32 * reduction_per_phase)
    }

    /// Get scaled escape radius (same as escape_radius since scale is derived from it)
    pub fn scaled_escape_radius(&self) -> f32 {
        self.escape_radius
    }

    /// Allocate a new unique well ID
    pub fn alloc_well_id(&mut self) -> WellId {
        let id = self.next_well_id;
        self.next_well_id += 1;
        id
    }

    /// Find a well by ID - O(1) with HashMap
    pub fn get_well(&self, id: WellId) -> Option<&GravityWell> {
        self.gravity_wells.get(&id)
    }

    /// Find a well by ID (mutable) - O(1) with HashMap
    pub fn get_well_mut(&mut self, id: WellId) -> Option<&mut GravityWell> {
        self.gravity_wells.get_mut(&id)
    }

    /// Remove a well by ID - O(1) with HashMap. Returns the removed well if found.
    /// Also removes from spatial grid for gravity calculations.
    pub fn remove_well(&mut self, id: WellId) -> Option<GravityWell> {
        if let Some(well) = self.gravity_wells.remove(&id) {
            self.well_grid.remove(id, well.position);
            Some(well)
        } else {
            None
        }
    }

    /// Insert a well into the arena
    /// Also inserts into spatial grid for gravity calculations.
    pub fn insert_well(&mut self, well: GravityWell) {
        self.well_grid.insert(well.id, well.position);
        self.gravity_wells.insert(well.id, well);
    }

    /// Count wells that are currently lerping toward their target position
    /// (i.e., position != target_position)
    pub fn wells_lerping_count(&self) -> usize {
        self.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .filter(|w| (w.position - w.target_position).length_sq() > 1.0)
            .count()
    }

    /// Rebuild the well spatial grid from current gravity_wells
    /// Call this when wells have been modified directly without using insert_well
    pub fn rebuild_well_grid(&mut self) {
        self.well_grid.rebuild(
            self.gravity_wells.iter().map(|(&id, w)| (id, w.position))
        );
    }

    /// Get all wells as a slice-like iterator (for gravity calculations)
    pub fn wells_iter(&self) -> impl Iterator<Item = &GravityWell> {
        self.gravity_wells.values()
    }

    /// Get number of wells
    pub fn well_count(&self) -> usize {
        self.gravity_wells.len()
    }

    /// Get number of orbital wells (excluding central)
    pub fn orbital_well_count(&self) -> usize {
        self.gravity_wells.len().saturating_sub(1)
    }

    /// Calculate minimum well spacing dynamically based on arena size and well count.
    /// Uses formula: sqrt(arena_area / total_wells) * factor
    /// This ensures spacing scales naturally as arena grows/shrinks.
    fn calculate_min_well_spacing(&self, escape_radius: f32, total_wells: usize) -> f32 {
        let arena_area = std::f32::consts::PI * escape_radius * escape_radius;
        let area_per_well = arena_area / (total_wells.max(1) as f32);
        let ideal_spacing = area_per_well.sqrt();
        // Scale factor 0.4: balances between too tight (0.3) and too spread (0.5)
        ideal_spacing * 0.4
    }

    /// Find optimal radial positions for new wells that fill gaps in existing distribution.
    /// Returns a Vec of radii where new wells should be placed.
    fn find_gap_radii(&self, count: usize, min_radius: f32, max_radius: f32) -> Vec<f32> {
        if count == 0 {
            return Vec::new();
        }

        // Get sorted list of existing well radii (excluding central well at origin)
        let mut existing_radii: Vec<f32> = self.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .map(|w| w.position.length())
            .collect();
        existing_radii.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // If no existing wells, use Fermat spiral distribution for even spacing
        if existing_radii.is_empty() {
            let radial_range = max_radius - min_radius;
            return (0..count)
                .map(|i| {
                    let t = (i as f32 + 1.0) / (count as f32);
                    min_radius + radial_range * t.sqrt()
                })
                .collect();
        }

        // Add boundaries as implicit "walls" to ensure edge gaps are considered
        let mut boundaries = vec![min_radius];
        boundaries.extend(existing_radii.iter().cloned());
        boundaries.push(max_radius);

        // Find gaps and their sizes
        let mut gaps: Vec<(f32, f32)> = Vec::new(); // (gap_center, gap_size)
        for i in 0..boundaries.len() - 1 {
            let gap_start = boundaries[i];
            let gap_end = boundaries[i + 1];
            let gap_size = gap_end - gap_start;
            if gap_size > 0.0 {
                let gap_center = (gap_start + gap_end) / 2.0;
                gaps.push((gap_center, gap_size));
            }
        }

        // Sort gaps by size (largest first)
        gaps.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Return centers of the largest gaps as positions for new wells
        let mut result = Vec::with_capacity(count);
        for (gap_center, _gap_size) in gaps.iter().take(count) {
            result.push(*gap_center);
        }

        // If we still need more positions, use fallback
        let radial_range = max_radius - min_radius;
        while result.len() < count {
            let fallback_t = (result.len() as f32 + 1.0) / (count as f32 + 1.0);
            let fallback_radius = min_radius + radial_range * fallback_t;
            result.push(fallback_radius);
        }

        result
    }

    /// Check if a position is valid for placing a new well.
    /// Returns true if position has sufficient distance from all existing wells.
    fn is_valid_well_position(&self, position: Vec2, min_spacing: f32) -> bool {
        for well in self.gravity_wells.values() {
            if well.id == CENTRAL_WELL_ID {
                continue; // Central well handled via center_exclusion_ratio
            }
            let distance = (well.position - position).length();
            if distance < min_spacing {
                return false;
            }
        }
        true
    }

    /// Try to find a valid position near the target, adjusting if needed.
    /// Returns Some(position) if valid position found, None if impossible.
    /// Uses expanding spiral search: first try near target, then expand outward.
    fn find_valid_well_position(
        &self,
        target_radius: f32,
        target_angle: f32,
        min_spacing: f32,
        min_radius: f32,
        max_radius: f32,
    ) -> Option<Vec2> {
        // Try original position first
        let original = Vec2::from_angle(target_angle) * target_radius;
        if self.is_valid_well_position(original, min_spacing) {
            return Some(original);
        }

        let radial_range = max_radius - min_radius;

        // Spiral search: combine angular and radial adjustments
        // This covers more of the search space efficiently
        const ANGLE_STEPS: usize = 12; // Cover full circle in 30° increments
        const RADIAL_STEPS: usize = 8; // Cover more radial range

        // First pass: angular adjustments at target radius
        for step in 1..=ANGLE_STEPS {
            let offset = (step as f32 / ANGLE_STEPS as f32) * std::f32::consts::TAU;

            let pos = Vec2::from_angle(target_angle + offset) * target_radius;
            if self.is_valid_well_position(pos, min_spacing) {
                return Some(pos);
            }
        }

        // Second pass: combined radial + angular search (spiral outward)
        for radial_step in 1..=RADIAL_STEPS {
            // Try both smaller and larger radii, up to 30% of radial range
            let radial_offset = (radial_step as f32 / RADIAL_STEPS as f32) * (radial_range * 0.3);

            for &radius in &[
                target_radius + radial_offset,
                target_radius - radial_offset,
            ] {
                if radius < min_radius || radius > max_radius {
                    continue;
                }

                // At each radius, try multiple angles
                for angle_step in 0..ANGLE_STEPS {
                    let angle_offset = (angle_step as f32 / ANGLE_STEPS as f32) * std::f32::consts::TAU;
                    let pos = Vec2::from_angle(target_angle + angle_offset) * radius;
                    if self.is_valid_well_position(pos, min_spacing) {
                        return Some(pos);
                    }
                }
            }
        }

        // Could not find valid position
        None
    }

    /// DEPRECATED: Use `scale_for_simulation()` instead for area-based well scaling.
    /// This legacy method uses player-count-based well calculation.
    #[deprecated(since = "0.3.0", note = "Use scale_for_simulation() for area-based well scaling")]
    #[allow(deprecated)]
    pub fn update_for_player_count(&mut self, player_count: usize) {
        self.update_for_player_count_with_limit(player_count, None);
    }

    /// DEPRECATED: Use `scale_for_simulation()` instead for area-based well scaling.
    #[deprecated(since = "0.3.0", note = "Use scale_for_simulation() for area-based well scaling")]
    pub fn update_for_player_count_with_limit(&mut self, player_count: usize, max_wells: Option<usize>) {
        use crate::game::constants::physics::CENTRAL_MASS;
        use std::f32::consts::TAU;

        // Scale arena: base + 0.1 per player beyond 10
        // Min 1.0, grows slowly with more players
        self.scale = 1.0 + ((player_count.saturating_sub(10)) as f32 * 0.05).min(1.0);

        // Determine number of gravity wells (not counting central supermassive)
        // GRAVITY_WELLS env var overrides dynamic calculation
        // Dynamic: 1-15 players: 1 well, 16-30: 2 wells, 31-45: 3 wells, etc.
        let mut well_count = std::env::var("GRAVITY_WELLS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or_else(|| ((player_count + 14) / 15).max(1))
            .max(1); // At least 1 well

        // Apply performance-based limit if provided
        if let Some(limit) = max_wells {
            well_count = well_count.min(limit);
        }

        // Each well needs space for a full "solar system" around it
        // Spacing ensures orbits around different wells don't overlap
        let well_spacing = arena::OUTER_RADIUS * 2.0; // 1200 units between wells

        // Calculate orbit radius for well placement
        // Wells are arranged in a circle around the central supermassive black hole
        let orbit_radius = if well_count > 1 {
            // For N wells in a circle, we need circumference = N * spacing
            (well_count as f32 * well_spacing) / TAU
        } else {
            well_spacing // Single well still orbits the center
        };

        // Update arena boundaries to encompass all wells
        // Each well needs ESCAPE_RADIUS of space around it
        let universe_radius = orbit_radius + arena::ESCAPE_RADIUS;
        self.escape_radius = universe_radius;
        self.outer_radius = orbit_radius + arena::OUTER_RADIUS;
        self.inner_radius = arena::INNER_RADIUS;
        self.middle_radius = arena::MIDDLE_RADIUS;

        self.gravity_wells.clear();
        self.next_well_id = 1; // Reset ID counter (0 is reserved for central well)

        // === SUPERMASSIVE BLACK HOLE AT CENTER ===
        // Much larger and more massive than other wells
        let supermassive_mass = CENTRAL_MASS * 3.0;
        let supermassive_core = arena::CORE_RADIUS * 2.5; // 125 radius death zone
        let central_well = GravityWell::new(CENTRAL_WELL_ID, Vec2::ZERO, supermassive_mass, supermassive_core);
        self.gravity_wells.insert(CENTRAL_WELL_ID, central_well);

        // === ORBITAL GRAVITY WELLS (distributed across rings) ===
        // Use the same multi-ring distribution as scale_for_simulation
        self.add_orbital_wells_default(well_count, self.escape_radius);
    }

    /// Smoothly scale arena based on player count
    /// - GROW: Fast and immediate (players need space), but only if can_grow is true
    /// - SHRINK: Delayed and slow (don't trap players), always allowed
    /// Uses ArenaScalingConfig for all tunable parameters
    ///
    /// # Arguments
    /// * `target_player_count` - Total players (humans + bots)
    /// * `config` - Arena scaling configuration
    /// * `can_grow` - If false, arena will not grow (used for health-based limiting)
    pub fn scale_for_simulation(&mut self, target_player_count: usize, config: &ArenaScalingConfig, can_grow: bool) {
        let min_escape = config.min_escape_radius;
        // Safety cap as emergency brake (default 50x = 40,000 units)
        let max_escape = config.min_escape_radius * config.max_escape_multiplier;

        // SQRT-BASED SCALING for constant player density
        // Area needed = players × area_per_player
        // Area = π × r², so r = √(Area / π)
        // This maintains constant density regardless of player count
        let players = (target_player_count as f32).max(1.0);
        let target_area = players * config.area_per_player;
        let target_escape = (target_area / std::f32::consts::PI).sqrt()
            .max(min_escape)
            .min(max_escape); // Safety cap still applies
        let target_outer = target_escape - 200.0;

        // Calculate target number of wells based on CURRENT arena area (not target)
        // This ensures we only spawn wells that can actually fit in the current arena
        // Wells are added progressively as the arena grows, rather than all at once
        // Area = PI * r^2, wells = area / wells_per_area
        let current_area = std::f32::consts::PI * self.escape_radius * self.escape_radius;
        let target_wells = ((current_area / config.wells_per_area).ceil() as usize)
            .max(config.min_wells)
            .min(config.max_wells); // Hard cap for performance and gameplay
        let current_orbital_wells = self.gravity_wells.len().saturating_sub(1);

        // Smooth lerp toward target (called every tick at 30Hz)
        let diff = target_escape - self.escape_radius;

        if diff > 1.0 && can_grow {
            // GROW: Lerp with cap for smooth expansion
            // Only grow if health allows (can_grow = true)
            // Cap prevents large jumps that cause visual stepping when
            // client linearly interpolates between snapshots (sent at 10Hz)
            self.shrink_delay_ticks = config.shrink_delay_ticks;
            let lerp_delta = diff * config.grow_lerp;
            let delta = lerp_delta.min(config.max_grow_per_tick);
            self.escape_radius = (self.escape_radius + delta).min(target_escape);
            self.outer_radius = (self.outer_radius + delta).min(target_outer);
        } else if diff > 1.0 {
            // Need to grow but can't (health degraded) - just reset shrink delay
            self.shrink_delay_ticks = config.shrink_delay_ticks;
        } else if diff < -1.0 {
            // SHRINK: Only after delay expires, lerp slowly
            if self.shrink_delay_ticks > 0 {
                self.shrink_delay_ticks -= 1;
            } else {
                let delta = (-diff) * config.shrink_lerp;
                self.escape_radius = (self.escape_radius - delta).max(target_escape).max(min_escape);
                self.outer_radius = (self.outer_radius - delta).max(target_outer).max(min_escape - 200.0);
            }
        } else {
            // Within 1 unit of target - stable
            self.shrink_delay_ticks = config.shrink_delay_ticks;
        }

        // Update scale factor based on current escape_radius vs base
        self.scale = self.escape_radius / arena::ESCAPE_RADIUS;

        // Wells stay FIXED - no position scaling during arena resize
        // This prevents chaotic movement that disrupts player orbits
        // Instead, out-of-bounds wells are triggered to explode naturally

        // Trigger explosions for wells outside escape radius after shrinking
        // Uses charge duration for visual warning before death
        if diff < -1.0 && self.shrink_delay_ticks == 0 {
            use crate::game::constants::gravity_waves::CHARGE_DURATION;
            for well in self.gravity_wells.values_mut() {
                if well.id == CENTRAL_WELL_ID {
                    continue;
                }
                // Well outside escape radius and not already charging? Trigger explosion
                let well_dist = well.position.length();
                if well_dist > self.escape_radius && !well.is_charging {
                    well.explosion_timer = CHARGE_DURATION;
                    well.is_charging = true;
                }
            }
        }

        // Add new wells if needed (never remove existing ones during gameplay)
        // Wells are placed based on CURRENT arena size - they're added progressively
        // as the arena grows, ensuring spacing constraints can always be met
        if target_wells > current_orbital_wells {
            let wells_to_add = target_wells - current_orbital_wells;
            self.add_orbital_wells(wells_to_add, self.escape_radius, config);
        }
    }

    /// Legacy version without config for backwards compatibility
    /// Always allows growth (can_grow = true)
    pub fn scale_for_simulation_default(&mut self, target_player_count: usize) {
        self.scale_for_simulation(target_player_count, &ArenaScalingConfig::default(), true);
    }

    /// Trigger rapid collapse of excess wells with staggered timers
    /// Returns the number of wells queued for collapse
    /// Wells explode in sequence (0.5s apart) to avoid performance spikes
    /// The explosion system will remove them when they explode
    pub fn trigger_well_collapse(&mut self, target_wells: usize) -> usize {
        let current_orbital = self.orbital_well_count();
        if current_orbital <= target_wells {
            return 0;
        }

        let excess = current_orbital - target_wells;
        let mut collapsed = 0;

        // Set staggered timers on excess wells (skip central well)
        for well in self.gravity_wells.values_mut() {
            if well.id == CENTRAL_WELL_ID {
                continue;
            }
            if collapsed >= excess {
                break;
            }

            // Only collapse wells that aren't already about to explode
            if well.explosion_timer > 1.0 {
                // Stagger: 0.5s, 1.0s, 1.5s, etc.
                well.explosion_timer = 0.5 + (collapsed as f32 * 0.5);
                well.is_charging = true;
                collapsed += 1;
                tracing::info!(
                    "Well {} queued for collapse in {:.1}s (excess well {})",
                    well.id,
                    well.explosion_timer,
                    collapsed
                );
            }
        }

        collapsed
    }

    /// Get the current number of excess wells compared to target
    pub fn excess_wells(&self, target_wells: usize) -> usize {
        let current_orbital = self.gravity_wells.len().saturating_sub(1);
        current_orbital.saturating_sub(target_wells)
    }

    /// Add orbital wells using golden angle distribution for optimal spacing.
    ///
    /// The golden angle (137.5°) ensures each new well is placed at maximum angular
    /// distance from existing wells. Combined with gap-filling radial distribution,
    /// this creates a natural, visually pleasing galaxy pattern.
    ///
    /// Key features:
    /// - Uses persistent `well_base_offset` for consistent golden angle pattern across batches
    /// - Gap-filling radial distribution places new wells in largest radial gaps
    /// - Dynamic minimum spacing based on arena size and well count
    pub fn add_orbital_wells(&mut self, count: usize, escape_radius: f32, config: &ArenaScalingConfig) {
        use crate::game::constants::physics::CENTRAL_MASS;
        use rand::Rng;

        // Golden angle in radians: 360°/φ² ≈ 137.5077° = 2.399963 radians
        // This is nature's optimal spacing angle (sunflower seeds, pinecones, etc.)
        const GOLDEN_ANGLE: f32 = 2.399963;

        let mut rng = rand::thread_rng();
        let size_multipliers = [0.6, 0.8, 1.0, 1.2, 1.4];

        // Count existing orbital wells (exclude central supermassive at ID 0)
        let existing_orbital = self.gravity_wells.len().saturating_sub(1);

        // Enforce max_wells cap
        let actual_count = count.min(config.max_wells.saturating_sub(existing_orbital));
        if actual_count == 0 {
            return; // Already at max
        }

        // Define radial bounds using center_exclusion_ratio
        let min_radius = escape_radius * config.center_exclusion_ratio;
        let max_radius = escape_radius * config.well_max_ratio;

        // Use persistent base_offset for consistent golden angle pattern across batches
        // (FIX: Previously regenerated each call, breaking incremental distribution)
        let base_offset = self.well_base_offset;

        // Calculate total wells for spacing calculation
        let total_wells = existing_orbital + actual_count;

        // Dynamic minimum spacing based on arena size and well count
        let min_spacing = self.calculate_min_well_spacing(escape_radius, total_wells);

        // Find optimal radii that fill gaps in existing distribution
        // (FIX: Previously used Fermat spiral that didn't account for existing wells)
        let new_radii = self.find_gap_radii(actual_count, min_radius, max_radius);

        for target_radius in new_radii.iter() {
            // === GOLDEN ANGLE DISTRIBUTION ===
            // Use monotonically increasing counter (NEVER reused, even after well destruction)
            // This ensures each new well gets a unique angle, preventing clustering
            let angle_index = self.next_well_angle_index;
            self.next_well_angle_index += 1;

            let target_angle = base_offset + (angle_index as f32) * GOLDEN_ANGLE;

            // Find valid position with minimum spacing check
            let position = match self.find_valid_well_position(
                *target_radius,
                target_angle,
                min_spacing,
                min_radius,
                max_radius,
            ) {
                Some(pos) => pos,
                None => {
                    tracing::warn!(
                        "Could not find valid position for well (angle_idx={}) at target radius {:.0}, skipping",
                        angle_index, target_radius
                    );
                    continue;
                }
            };

            // Random well size for variety
            let size_mult = size_multipliers[rng.gen_range(0..size_multipliers.len())];
            let well_mass = CENTRAL_MASS * size_mult;
            let well_core = arena::CORE_RADIUS * size_mult;

            let well_id = self.alloc_well_id();
            let well = GravityWell::new(well_id, position, well_mass, well_core);
            // Use insert_well to also update the spatial grid
            self.insert_well(well);
        }
    }

    /// Legacy version for update_for_player_count_with_limit
    fn add_orbital_wells_default(&mut self, count: usize, escape_radius: f32) {
        self.add_orbital_wells(count, escape_radius, &ArenaScalingConfig::default());
    }
}

/// Match phase
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MatchPhase {
    /// Waiting for players
    Waiting,
    /// Countdown before match starts
    Countdown,
    /// Match in progress
    Playing,
    /// Match ended
    Ended,
}

impl Default for MatchPhase {
    fn default() -> Self {
        Self::Waiting
    }
}

/// Match state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchState {
    pub phase: MatchPhase,
    pub match_time: f32,
    pub countdown_time: f32,
    pub winner_id: Option<PlayerId>,
}

impl Default for MatchState {
    fn default() -> Self {
        Self {
            phase: MatchPhase::Waiting,
            match_time: 0.0,
            countdown_time: crate::game::constants::game::COUNTDOWN,
            winner_id: None,
        }
    }
}

/// Complete game state
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameState {
    pub tick: u64,
    pub match_state: MatchState,
    pub arena: Arena,
    pub players: HashMap<PlayerId, Player>,
    pub projectiles: Vec<Projectile>,
    pub debris: Vec<Debris>,
    /// Active gravity waves from well explosions (not serialized - visual only)
    #[serde(skip)]
    pub gravity_waves: Vec<GravityWave>,
    /// Spatial grid for efficient gravity well lookups (not serialized - rebuilt as needed)
    #[serde(skip)]
    pub well_grid: crate::game::spatial::WellSpatialGrid,
    next_entity_id: EntityId,
}

impl GameState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuild the spatial grid for gravity wells
    /// Call this after wells are added, removed, or moved
    pub fn rebuild_well_grid(&mut self) {
        self.well_grid.rebuild(
            self.arena.gravity_wells.values().map(|w| (w.id, w.position))
        );
    }

    /// Generate a new unique entity ID
    pub fn next_entity_id(&mut self) -> EntityId {
        let id = self.next_entity_id;
        self.next_entity_id += 1;
        id
    }

    /// Get player by ID - O(1) with HashMap
    pub fn get_player(&self, id: PlayerId) -> Option<&Player> {
        self.players.get(&id)
    }

    /// Get mutable player by ID - O(1) with HashMap
    pub fn get_player_mut(&mut self, id: PlayerId) -> Option<&mut Player> {
        self.players.get_mut(&id)
    }

    /// Get all alive players
    pub fn alive_players(&self) -> impl Iterator<Item = &Player> {
        self.players.values().filter(|p| p.alive)
    }

    /// Count alive players
    pub fn alive_count(&self) -> usize {
        self.players.values().filter(|p| p.alive).count()
    }

    /// Count alive human players
    pub fn alive_human_count(&self) -> usize {
        self.players.values().filter(|p| p.alive && !p.is_bot).count()
    }

    /// Add a player to the game - O(1) with HashMap
    pub fn add_player(&mut self, player: Player) {
        self.players.insert(player.id, player);
    }

    /// Remove a player from the game - O(1) with HashMap
    pub fn remove_player(&mut self, id: PlayerId) -> Option<Player> {
        self.players.remove(&id)
    }

    /// Add a projectile
    pub fn add_projectile(&mut self, owner_id: PlayerId, position: Vec2, velocity: Vec2, mass: f32) -> EntityId {
        let id = self.next_entity_id();
        self.projectiles
            .push(Projectile::new(id, owner_id, position, velocity, mass));
        id
    }

    /// Add debris
    pub fn add_debris(&mut self, position: Vec2, velocity: Vec2, size: DebrisSize) -> EntityId {
        let id = self.next_entity_id();
        self.debris.push(Debris::new(id, position, velocity, size));
        id
    }

    pub fn add_debris_with_lifetime(&mut self, position: Vec2, velocity: Vec2, size: DebrisSize, lifetime: f32) -> EntityId {
        let id = self.next_entity_id();
        let mut debris = Debris::new(id, position, velocity, size);
        debris.lifetime = lifetime;
        self.debris.push(debris);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_new() {
        let id = Uuid::new_v4();
        let player = Player::new(id, "TestPlayer".to_string(), false, 0);
        assert_eq!(player.id, id);
        assert_eq!(player.name, "TestPlayer");
        assert!(!player.is_bot);
        assert!(player.alive);
        assert_eq!(player.mass, mass::STARTING);
        assert!(player.has_spawn_protection());
    }

    #[test]
    fn test_player_radius() {
        let player = Player::default();
        let expected_radius = (mass::STARTING).sqrt() * mass::RADIUS_SCALE;
        assert!((player.radius() - expected_radius).abs() < 0.001);
    }

    #[test]
    fn test_player_danger_zone() {
        let mut player = Player::default();
        player.position = Vec2::new(10.0, 10.0);
        assert!(player.in_danger_zone());

        player.position = Vec2::new(100.0, 100.0);
        assert!(!player.in_danger_zone());
    }

    #[test]
    fn test_player_outside_arena() {
        let mut player = Player::default();
        player.position = Vec2::new(1000.0, 0.0);
        assert!(player.outside_arena());

        player.position = Vec2::new(100.0, 0.0);
        assert!(!player.outside_arena());
    }

    #[test]
    fn test_projectile_expired() {
        let mut proj = Projectile::new(1, Uuid::new_v4(), Vec2::ZERO, Vec2::ZERO, 10.0);
        assert!(!proj.is_expired());
        proj.lifetime = 0.0;
        assert!(proj.is_expired());
    }

    #[test]
    fn test_debris_size_mass() {
        assert!(DebrisSize::Small.mass() < DebrisSize::Medium.mass());
        assert!(DebrisSize::Medium.mass() < DebrisSize::Large.mass());
    }

    #[test]
    fn test_arena_default() {
        let arena = Arena::default();
        assert_eq!(arena.collapse_phase, 0);
        assert!(!arena.is_collapsing);
        assert!(arena.time_until_collapse > 0.0);
    }

    #[test]
    fn test_arena_safe_radius() {
        let arena = Arena::default();
        let safe_radius = arena.current_safe_radius();
        assert_eq!(safe_radius, arena.escape_radius);
    }

    #[test]
    fn test_arena_safe_radius_linear_with_escape_radius() {
        // Verifies that current_safe_radius is LINEAR with escape_radius
        // (not quadratic, which was a bug where we multiplied escape_radius * scale)
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();

        // Scale arena to different sizes
        for _ in 0..50 {
            arena.scale_for_simulation(500, &config, true);
        }

        let scaled_escape = arena.escape_radius;
        let scaled_safe = arena.current_safe_radius();

        // With no collapse, safe_radius should equal escape_radius exactly
        assert_eq!(arena.collapse_phase, 0);
        assert_eq!(scaled_safe, scaled_escape,
            "Safe radius should equal escape_radius (linear), got {} vs {}",
            scaled_safe, scaled_escape);

        // Verify it's NOT quadratic: if it were, safe_radius would be
        // escape_radius * (escape_radius / ESCAPE_RADIUS) = escape_radius² / ESCAPE_RADIUS
        let quadratic_value = scaled_escape * scaled_escape / arena::ESCAPE_RADIUS;
        assert_ne!(scaled_safe, quadratic_value,
            "Safe radius should NOT be quadratic with escape_radius");

        // scaled_escape_radius should also just return escape_radius
        assert_eq!(arena.scaled_escape_radius(), scaled_escape);
    }

    #[test]
    fn test_match_phase_default() {
        let phase = MatchPhase::default();
        assert_eq!(phase, MatchPhase::Waiting);
    }

    #[test]
    fn test_game_state_entity_ids() {
        let mut state = GameState::new();
        let id1 = state.next_entity_id();
        let id2 = state.next_entity_id();
        assert_ne!(id1, id2);
        assert_eq!(id2, id1 + 1);
    }

    #[test]
    fn test_game_state_add_player() {
        let mut state = GameState::new();
        let player = Player::default();
        let id = player.id;
        state.add_player(player);
        assert!(state.get_player(id).is_some());
    }

    #[test]
    fn test_game_state_remove_player() {
        let mut state = GameState::new();
        let player = Player::default();
        let id = player.id;
        state.add_player(player);
        let removed = state.remove_player(id);
        assert!(removed.is_some());
        assert!(state.get_player(id).is_none());
    }

    #[test]
    fn test_game_state_alive_count() {
        let mut state = GameState::new();

        let mut p1 = Player::default();
        p1.alive = true;
        let mut p2 = Player::default();
        p2.alive = false;
        let mut p3 = Player::default();
        p3.alive = true;
        p3.is_bot = true;

        state.add_player(p1);
        state.add_player(p2);
        state.add_player(p3);

        assert_eq!(state.alive_count(), 2);
        assert_eq!(state.alive_human_count(), 1);
    }

    #[test]
    fn test_game_state_add_projectile() {
        let mut state = GameState::new();
        let owner = Uuid::new_v4();
        let id = state.add_projectile(owner, Vec2::ZERO, Vec2::new(100.0, 0.0), 20.0);
        assert_eq!(state.projectiles.len(), 1);
        assert_eq!(state.projectiles[0].id, id);
        assert_eq!(state.projectiles[0].owner_id, owner);
    }

    #[test]
    fn test_game_state_add_debris() {
        let mut state = GameState::new();
        let id = state.add_debris(Vec2::ZERO, Vec2::ZERO, DebrisSize::Medium);
        assert_eq!(state.debris.len(), 1);
        assert_eq!(state.debris[0].id, id);
    }

    #[test]
    fn test_serialization() {
        let state = GameState::new();
        let encoded = bincode::serde::encode_to_vec(&state, bincode::config::standard()).unwrap();
        let (decoded, _): (GameState, usize) =
            bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(decoded.tick, state.tick);
    }

    #[test]
    fn test_scale_for_simulation_expands_arena() {
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();
        let initial_escape = arena.escape_radius;
        let initial_outer = arena.outer_radius;
        let initial_wells = arena.gravity_wells.len();

        // Simulate scaling for 500 players (should grow arena)
        for _ in 0..50 {
            arena.scale_for_simulation(500, &config, true);
        }

        // Arena should have expanded
        assert!(arena.escape_radius > initial_escape,
            "escape_radius should grow: {} > {}", arena.escape_radius, initial_escape);
        assert!(arena.outer_radius > initial_outer,
            "outer_radius should grow: {} > {}", arena.outer_radius, initial_outer);
        // Should have added wells (500 players = 10 wells target)
        assert!(arena.gravity_wells.len() > initial_wells,
            "wells should increase: {} > {}", arena.gravity_wells.len(), initial_wells);
    }

    #[test]
    fn test_scale_for_simulation_shrinks_with_delay() {
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();

        // First grow the arena
        // With max_grow_per_tick=30, need more iterations for large changes
        for _ in 0..200 {
            arena.scale_for_simulation(500, &config, true);
        }
        let expanded_escape = arena.escape_radius;
        assert!(expanded_escape > 3000.0, "Should have grown significantly: got {}", expanded_escape);

        // Now request shrink to 10 players
        // First few calls should NOT shrink (delay period)
        arena.scale_for_simulation(10, &config, true);
        arena.scale_for_simulation(10, &config, true);
        arena.scale_for_simulation(10, &config, true);
        assert!(arena.escape_radius >= expanded_escape - 10.0,
            "Should not shrink during delay period");

        // After delay (150+ ticks), should start shrinking slowly
        for _ in 0..200 {
            arena.scale_for_simulation(10, &config, true);
        }
        assert!(arena.escape_radius < expanded_escape,
            "Should shrink after delay: {} < {}", arena.escape_radius, expanded_escape);

        // But should never go below minimum
        for _ in 0..500 {
            arena.scale_for_simulation(10, &config, true);
        }
        assert!(arena.escape_radius >= config.min_escape_radius,
            "Should never shrink below minimum: {}", arena.escape_radius);
    }

    #[test]
    fn test_scale_for_simulation_wells_stay_fixed() {
        // Wells should NOT move when arena scales - they stay at original positions
        // This prevents chaotic movement that disrupts player orbits
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();

        // First grow arena and add wells via scale_for_simulation
        for _ in 0..150 {
            arena.scale_for_simulation(100, &config, true);
        }

        // Record exact well positions after initial setup
        let well_positions: Vec<(WellId, Vec2)> = arena.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .map(|w| (w.id, w.position))
            .collect();
        assert!(!well_positions.is_empty(), "Should have orbital wells after setup");

        let setup_escape = arena.escape_radius;

        // Now scale up significantly
        for _ in 0..300 {
            arena.scale_for_simulation(500, &config, true);
        }

        // Arena should have grown
        assert!(arena.escape_radius > setup_escape,
            "Arena should have grown: {} > {}", arena.escape_radius, setup_escape);

        // Original wells should NOT have moved (positions should be identical)
        for (well_id, original_pos) in &well_positions {
            if let Some(well) = arena.gravity_wells.get(well_id) {
                let distance_moved = (well.position - *original_pos).length();
                assert!(distance_moved < 0.01,
                    "Well {} should not have moved. Original: {:?}, Current: {:?}",
                    well_id, original_pos, well.position);
            }
        }
    }

    #[test]
    fn test_scale_for_simulation_triggers_out_of_bounds_explosions() {
        // When arena shrinks, wells outside escape_radius should start charging
        use crate::config::ArenaScalingConfig;
        use crate::game::constants::arena::CORE_RADIUS;
        use crate::game::constants::physics::CENTRAL_MASS;

        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();

        // Start with large arena
        arena.escape_radius = 1500.0;

        // Add a well far from center (will be outside after shrink)
        let outer_well_id = arena.alloc_well_id();
        let outer_well = GravityWell::new(
            outer_well_id,
            Vec2::new(1200.0, 0.0), // At 1200 units from center
            CENTRAL_MASS,
            CORE_RADIUS,
        );
        arena.gravity_wells.insert(outer_well_id, outer_well);

        // Add a well close to center (will remain inside after shrink)
        let inner_well_id = arena.alloc_well_id();
        let inner_well = GravityWell::new(
            inner_well_id,
            Vec2::new(400.0, 0.0), // At 400 units from center
            CENTRAL_MASS,
            CORE_RADIUS,
        );
        arena.gravity_wells.insert(inner_well_id, inner_well);

        // Verify neither is charging initially
        assert!(!arena.gravity_wells.get(&outer_well_id).unwrap().is_charging);
        assert!(!arena.gravity_wells.get(&inner_well_id).unwrap().is_charging);

        // Shrink arena - exhaust delay and call with low player count
        arena.shrink_delay_ticks = 0;

        // Keep shrinking until we're below 1200 (outer well position)
        for _ in 0..200 {
            arena.scale_for_simulation(1, &config, true);
            arena.shrink_delay_ticks = 0; // Keep delay exhausted
        }

        // Verify arena actually shrunk below outer well
        assert!(arena.escape_radius < 1200.0,
            "Arena should have shrunk below 1200, got {}", arena.escape_radius);

        // Outer well (at 1200) should now be charging (outside escape radius)
        let outer = arena.gravity_wells.get(&outer_well_id).unwrap();
        assert!(outer.is_charging,
            "Outer well at 1200 should be charging when escape_radius is {}",
            arena.escape_radius);

        // Inner well (at 400) should NOT be charging (still inside)
        let inner = arena.gravity_wells.get(&inner_well_id).unwrap();
        assert!(!inner.is_charging,
            "Inner well at 400 should NOT be charging when escape_radius is {}",
            arena.escape_radius);
    }

    #[test]
    fn test_scale_for_simulation_smooth_lerp() {
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();
        let initial_escape = arena.escape_radius;

        // Calculate expected target using sqrt-based formula
        // target_area = players * area_per_player
        // target_radius = sqrt(target_area / PI)
        let target_area = 1000.0 * config.area_per_player;
        let target = (target_area / std::f32::consts::PI).sqrt()
            .max(config.min_escape_radius)
            .min(config.min_escape_radius * config.max_escape_multiplier);

        // Single call should move toward target with lerp (not instant)
        arena.scale_for_simulation(1000, &config, true);
        let after_one = arena.escape_radius;

        // Should have moved but not reached target instantly
        assert!(after_one > initial_escape, "Should start expanding");
        assert!(after_one < target, "Should not reach target in one tick");

        // Multiple calls should converge to target
        // With max_grow_per_tick=30 and lerp=0.05, large changes take longer
        // For 7200 unit diff, linear phase takes ~240 ticks, then exponential
        for _ in 0..400 {
            arena.scale_for_simulation(1000, &config, true);
        }
        let after_many = arena.escape_radius;

        assert!(after_many > after_one, "Should continue expanding");
        // Should be very close to target after enough iterations
        assert!((after_many - target).abs() < 10.0, "Should converge to target");
    }

    #[test]
    fn test_scale_for_simulation_area_based_wells() {
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();

        // Small stable player count to test min_wells
        for _ in 0..100 {
            arena.scale_for_simulation(5, &config, true);
        }

        // -1 because central supermassive doesn't count toward the orbital wells
        let orbital_wells = arena.gravity_wells.len() - 1;

        // Wells should be at least min_wells
        assert!(orbital_wells >= config.min_wells,
            "Should have at least {} wells, got {}", config.min_wells, orbital_wells);

        // Now test with larger player count - wells should grow
        let wells_before = orbital_wells;
        for _ in 0..200 {
            arena.scale_for_simulation(500, &config, true);
        }

        let wells_after = arena.gravity_wells.len() - 1;

        // Wells should increase as arena area grows
        // Note: wells only add, never remove during gameplay (scale_for_simulation)
        assert!(wells_after >= wells_before,
            "Wells should not decrease: before={}, after={}", wells_before, wells_after);

        // Calculate expected wells based on final area
        let arena_area = std::f32::consts::PI * arena.escape_radius * arena.escape_radius;
        let expected_wells = ((arena_area / config.wells_per_area).ceil() as usize)
            .max(config.min_wells);

        // Wells should be reasonable (at least expected, since we only add during growth)
        assert!(wells_after >= expected_wells,
            "Should have at least {} wells for this area, got {}", expected_wells, wells_after);
    }

    #[test]
    fn test_add_orbital_wells_spacing() {
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();
        let orbit_radius = 2000.0;

        // Add several wells
        arena.add_orbital_wells(5, orbit_radius, &config);
        let initial_count = arena.gravity_wells.len();

        // All wells should be reasonably spaced
        let wells: Vec<_> = arena.gravity_wells.values().collect();
        for i in 0..wells.len() {
            for j in (i + 1)..wells.len() {
                let dist = (wells[i].position - wells[j].position).length();
                assert!(dist > 500.0, "Wells {} and {} too close: {}", i, j, dist);
            }
        }

        // Add more wells
        arena.add_orbital_wells(3, orbit_radius, &config);
        assert_eq!(arena.gravity_wells.len(), initial_count + 3);
    }

    #[test]
    fn test_add_orbital_wells_radial_spread() {
        // Verifies that few wells spread across the FULL radial range,
        // not just clustered at the inner edge (regression test for golden angle fix)
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();
        let escape_radius = 800.0;

        // Add only 3 wells - they should spread across the full range
        arena.add_orbital_wells(3, escape_radius, &config);

        let min_radius = escape_radius * config.center_exclusion_ratio;
        let max_radius = escape_radius * config.well_max_ratio;
        let radial_range = max_radius - min_radius;

        // Get orbital well distances from center
        let mut distances: Vec<f32> = arena.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .map(|w| w.position.length())
            .collect();
        distances.sort_by(|a, b| a.partial_cmp(b).unwrap());

        assert_eq!(distances.len(), 3, "Should have exactly 3 orbital wells");

        // Wells should span at least 40% of the radial range
        // (sqrt distribution gives ~42% span for 3 wells: from 58% to 100% of range)
        let actual_span = distances.last().unwrap() - distances.first().unwrap();
        let min_expected_span = radial_range * 0.4;

        assert!(
            actual_span >= min_expected_span,
            "Wells should span at least 50% of radial range. \
             Span: {:.0}, expected >= {:.0}. Distances: {:?}",
            actual_span, min_expected_span, distances
        );

        // Innermost well should be in inner 65%, outermost in outer 15%
        // (sqrt(1/3)=0.58, so first well is at ~58% of range)
        let inner_bound = min_radius + radial_range * 0.65;
        let outer_bound = min_radius + radial_range * 0.85;

        assert!(
            *distances.first().unwrap() <= inner_bound,
            "Innermost well at {:.0} should be <= {:.0} (inner portion)",
            distances.first().unwrap(), inner_bound
        );
        assert!(
            *distances.last().unwrap() >= outer_bound,
            "Outermost well at {:.0} should be >= {:.0} (outer portion)",
            distances.last().unwrap(), outer_bound
        );
    }

    #[test]
    fn test_add_orbital_wells_respects_center_exclusion() {
        // Verifies wells stay outside center_exclusion_ratio boundary
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();

        // Test with various arena sizes
        for escape_radius in [800.0, 1500.0, 3000.0] {
            let mut arena = Arena::default();
            arena.add_orbital_wells(5, escape_radius, &config);

            let min_allowed_radius = escape_radius * config.center_exclusion_ratio;

            for well in arena.gravity_wells.values() {
                if well.id == CENTRAL_WELL_ID {
                    continue; // Skip central supermassive
                }

                let dist = well.position.length();
                assert!(
                    dist >= min_allowed_radius,
                    "Well at {:.0} units violates center exclusion (min: {:.0}) for arena size {:.0}",
                    dist, min_allowed_radius, escape_radius
                );
            }
        }
    }

    #[test]
    fn test_scale_grow_resets_shrink_delay() {
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();

        // Grow to large size
        for _ in 0..50 {
            arena.scale_for_simulation(500, &config, true);
        }

        // Start shrink process
        for _ in 0..3 {
            arena.scale_for_simulation(10, &config, true);
        }

        // Now request growth again
        arena.scale_for_simulation(500, &config, true);

        // Shrink delay should be reset
        assert_eq!(arena.shrink_delay_ticks, config.shrink_delay_ticks,
            "Shrink delay should be reset on grow");
    }

    #[test]
    fn test_health_based_arena_growth_limiting() {
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();

        // Record initial size
        let initial_escape = arena.escape_radius;

        // Try to grow with can_grow=false - should NOT grow
        for _ in 0..50 {
            arena.scale_for_simulation(1000, &config, false);
        }

        // Arena should NOT have grown
        assert_eq!(arena.escape_radius, initial_escape,
            "Arena should not grow when can_grow=false");

        // Now allow growth with can_grow=true
        for _ in 0..100 {
            arena.scale_for_simulation(1000, &config, true);
        }

        // Arena should have grown
        assert!(arena.escape_radius > initial_escape + 100.0,
            "Arena should grow when can_grow=true: {} > {}",
            arena.escape_radius, initial_escape);

        // Record size after growth
        let grown_escape = arena.escape_radius;

        // Shrinking should ALWAYS work regardless of can_grow
        // First exhaust shrink delay
        for _ in 0..160 {
            arena.scale_for_simulation(10, &config, false);
        }

        // Arena should have shrunk even with can_grow=false
        assert!(arena.escape_radius < grown_escape,
            "Arena should shrink even when can_grow=false: {} < {}",
            arena.escape_radius, grown_escape);
    }

    #[test]
    fn test_excess_wells_calculation() {
        let mut arena = Arena::default();

        // Start with 1 well (central supermassive)
        assert_eq!(arena.excess_wells(1), 0);
        assert_eq!(arena.excess_wells(5), 0);

        // Add some orbital wells manually
        use crate::game::constants::arena::CORE_RADIUS;
        use crate::game::constants::physics::CENTRAL_MASS;
        for i in 1..=5 {
            let well_id = arena.alloc_well_id();
            let well = GravityWell::new(
                well_id,
                Vec2::new(500.0 * i as f32, 0.0),
                CENTRAL_MASS,
                CORE_RADIUS,
            );
            arena.gravity_wells.insert(well_id, well);
        }

        // Now we have 6 wells total (1 central + 5 orbital)
        assert_eq!(arena.gravity_wells.len(), 6);
        assert_eq!(arena.excess_wells(5), 0);  // 5 orbital, target 5
        assert_eq!(arena.excess_wells(3), 2);  // 5 orbital, target 3 = 2 excess
        assert_eq!(arena.excess_wells(1), 4);  // 5 orbital, target 1 = 4 excess
    }

    #[test]
    fn test_trigger_well_collapse_sets_timers() {
        use crate::game::constants::arena::CORE_RADIUS;
        use crate::game::constants::physics::CENTRAL_MASS;

        let mut arena = Arena::default();

        // Add 5 orbital wells with long timers
        for i in 1..=5 {
            let well_id = arena.alloc_well_id();
            let mut well = GravityWell::new(
                well_id,
                Vec2::new(500.0 * i as f32, 0.0),
                CENTRAL_MASS,
                CORE_RADIUS,
            );
            well.explosion_timer = 30.0;  // Long timer
            arena.gravity_wells.insert(well_id, well);
        }

        // Trigger collapse to reduce to 2 target wells
        let collapsed = arena.trigger_well_collapse(2);

        // Should collapse 3 wells (5 - 2 = 3 excess)
        assert_eq!(collapsed, 3);

        // Check that 3 wells have short timers and are charging
        let charging_count = arena.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .filter(|w| w.is_charging && w.explosion_timer < 2.0)
            .count();
        assert_eq!(charging_count, 3);

        // Check staggered timers (0.5, 1.0, 1.5)
        let timers: Vec<f32> = arena.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .filter(|w| w.is_charging)
            .map(|w| w.explosion_timer)
            .collect();
        assert!(timers.contains(&0.5) || timers.iter().any(|t| (*t - 0.5).abs() < 0.01));
    }

    #[test]
    fn test_trigger_well_collapse_no_excess() {
        let mut arena = Arena::default();

        // Only central well exists
        let collapsed = arena.trigger_well_collapse(5);
        assert_eq!(collapsed, 0);  // No excess, nothing to collapse
    }

    #[test]
    fn test_trigger_well_collapse_skips_already_exploding() {
        use crate::game::constants::arena::CORE_RADIUS;
        use crate::game::constants::physics::CENTRAL_MASS;

        let mut arena = Arena::default();

        // Add wells - some already about to explode
        for i in 1..=4 {
            let well_id = arena.alloc_well_id();
            let mut well = GravityWell::new(
                well_id,
                Vec2::new(500.0 * i as f32, 0.0),
                CENTRAL_MASS,
                CORE_RADIUS,
            );
            if i <= 2 {
                well.explosion_timer = 0.5;  // Already exploding soon
            } else {
                well.explosion_timer = 30.0;  // Normal timer
            }
            arena.gravity_wells.insert(well_id, well);
        }

        // Try to collapse 3 (target = 1, so 3 excess)
        let collapsed = arena.trigger_well_collapse(1);

        // Should only collapse 2 (the ones not already exploding)
        assert_eq!(collapsed, 2);
    }

    #[test]
    fn test_wells_placed_at_current_radius_for_progressive_growth() {
        use crate::config::ArenaScalingConfig;

        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();

        // Start with small arena
        let initial_escape = arena.escape_radius;

        // Call scale_for_simulation with large player count
        // After ONE call, arena hasn't reached target yet (lerping)
        // Wells should be placed based on CURRENT arena size (progressive growth)
        arena.scale_for_simulation(1000, &config, true);

        let after_one_escape = arena.escape_radius;
        // Calculate target using sqrt-based formula
        let target_area = 1000.0 * config.area_per_player;
        let target_escape = (target_area / std::f32::consts::PI).sqrt()
            .max(config.min_escape_radius)
            .min(config.min_escape_radius * config.max_escape_multiplier);

        // Arena should not have reached target yet (smooth lerping)
        assert!(after_one_escape < target_escape,
            "Arena should still be lerping: {} < {}", after_one_escape, target_escape);
        assert!(after_one_escape > initial_escape,
            "Arena should have grown: {} > {}", after_one_escape, initial_escape);

        // Check orbital wells are placed at CURRENT radius range (progressive growth)
        // This ensures spacing constraints can always be met
        let orbital_wells: Vec<_> = arena.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .collect();

        if !orbital_wells.is_empty() {
            for well in &orbital_wells {
                let dist = well.position.length();
                // Wells should be within current arena bounds, not target
                let min_expected = after_one_escape * config.center_exclusion_ratio * 0.9; // 10% tolerance
                let max_expected = after_one_escape * config.well_max_ratio * 1.1;

                assert!(
                    dist >= min_expected && dist <= max_expected,
                    "Well at {:?} (dist {:.0}) should be in range [{:.0}, {:.0}] based on CURRENT radius {:.0}",
                    well.position, dist, min_expected, max_expected, after_one_escape
                );
            }
        }
    }

    #[test]
    fn test_well_base_offset_consistency() {
        // Verify that well_base_offset stays constant across multiple add_orbital_wells() calls
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();

        let original_offset = arena.well_base_offset;

        // Add wells in multiple batches
        arena.add_orbital_wells(2, 1000.0, &config);
        assert!(
            (arena.well_base_offset - original_offset).abs() < f32::EPSILON,
            "Offset should not change after adding wells"
        );

        arena.add_orbital_wells(2, 1500.0, &config);
        assert!(
            (arena.well_base_offset - original_offset).abs() < f32::EPSILON,
            "Offset should remain constant across batches"
        );

        arena.add_orbital_wells(3, 2000.0, &config);
        assert!(
            (arena.well_base_offset - original_offset).abs() < f32::EPSILON,
            "Offset should persist across all additions"
        );
    }

    #[test]
    fn test_incremental_well_distribution_quality() {
        // Add wells in multiple batches and verify ALL wells maintain minimum spacing
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();
        let escape_radius = 2000.0;

        // Simulate incremental arena growth
        arena.add_orbital_wells(2, escape_radius, &config);
        arena.add_orbital_wells(2, escape_radius, &config);
        arena.add_orbital_wells(3, escape_radius, &config);

        let wells: Vec<_> = arena.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .collect();

        // Calculate expected minimum spacing
        let total_wells = wells.len();
        let arena_area = std::f32::consts::PI * escape_radius * escape_radius;
        let expected_min_spacing = (arena_area / (total_wells as f32)).sqrt() * 0.4 * 0.8; // 80% tolerance

        // Verify all pairs maintain spacing
        for i in 0..wells.len() {
            for j in (i + 1)..wells.len() {
                let dist = (wells[i].position - wells[j].position).length();
                assert!(
                    dist >= expected_min_spacing,
                    "Wells {} and {} too close: {:.0} < {:.0}",
                    wells[i].id, wells[j].id, dist, expected_min_spacing
                );
            }
        }
    }

    #[test]
    fn test_gap_filling_distribution() {
        // Add wells in batches and verify new wells fill radial gaps
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();
        let escape_radius = 2000.0;

        let min_r = escape_radius * config.center_exclusion_ratio;
        let max_r = escape_radius * config.well_max_ratio;

        // Add initial wells
        arena.add_orbital_wells(3, escape_radius, &config);

        // Add more wells
        arena.add_orbital_wells(2, escape_radius, &config);

        // Get all radii sorted
        let mut all_radii: Vec<f32> = arena.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .map(|w| w.position.length())
            .collect();
        all_radii.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // Verify radii are reasonably spread across the range
        let radial_span = all_radii.last().unwrap() - all_radii.first().unwrap();
        let expected_min_span = (max_r - min_r) * 0.4; // At least 40% of range

        assert!(
            radial_span >= expected_min_span,
            "Wells should span at least 40% of radial range. Span: {:.0}, expected >= {:.0}",
            radial_span, expected_min_span
        );

        // Calculate minimum gap between adjacent radii (should be reasonable)
        let min_gap = all_radii.windows(2)
            .map(|w| w[1] - w[0])
            .fold(f32::MAX, f32::min);

        // With 5 wells across ~1200 unit range, min gap should be at least 50 units
        assert!(
            min_gap >= 50.0,
            "Radial gaps should not be too small. Min gap: {:.0}, expected >= 50",
            min_gap
        );
    }

    #[test]
    fn test_dynamic_well_spacing() {
        // Test that spacing scales dynamically with arena size and well count
        let arena = Arena::default();

        // Small arena with few wells
        let small_spacing = arena.calculate_min_well_spacing(800.0, 3);
        // sqrt(PI * 800^2 / 3) * 0.4 ≈ 327

        // Larger arena with more wells
        let large_spacing = arena.calculate_min_well_spacing(3000.0, 10);
        // sqrt(PI * 3000^2 / 10) * 0.4 ≈ 672

        // Spacing should scale with arena size
        assert!(
            large_spacing > small_spacing,
            "Larger arena should have larger spacing: {} > {}",
            large_spacing, small_spacing
        );

        // Verify rough expected values
        assert!(
            small_spacing > 200.0 && small_spacing < 500.0,
            "Small arena spacing should be ~327: got {}",
            small_spacing
        );
        assert!(
            large_spacing > 500.0 && large_spacing < 900.0,
            "Large arena spacing should be ~672: got {}",
            large_spacing
        );
    }

    #[test]
    fn test_incremental_adds_maintain_spacing_across_batches() {
        // Updated version of test_add_orbital_wells_spacing that verifies
        // spacing is maintained ACROSS incremental batches (not just within single batch)
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();
        let orbit_radius = 2000.0;

        // Add wells in separate batches (simulating arena growth over time)
        arena.add_orbital_wells(2, orbit_radius, &config);
        let count_after_first = arena.orbital_well_count();

        arena.add_orbital_wells(3, orbit_radius, &config);
        let count_after_second = arena.orbital_well_count();

        arena.add_orbital_wells(2, orbit_radius, &config);
        let final_count = arena.orbital_well_count();

        // Verify wells were added across batches
        assert_eq!(count_after_first, 2, "First batch should add 2 wells");
        assert_eq!(count_after_second, 5, "Second batch should add 3 more");
        assert_eq!(final_count, 7, "Third batch should add 2 more");

        // All wells across ALL batches should be reasonably spaced
        let wells: Vec<_> = arena.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .collect();

        // Calculate dynamic spacing expectation
        let arena_area = std::f32::consts::PI * orbit_radius * orbit_radius;
        let dynamic_spacing = (arena_area / (wells.len() as f32)).sqrt() * 0.4;
        let min_acceptable = dynamic_spacing * 0.7; // 70% of calculated minimum

        for i in 0..wells.len() {
            for j in (i + 1)..wells.len() {
                let dist = (wells[i].position - wells[j].position).length();
                assert!(
                    dist >= min_acceptable,
                    "Wells {} (batch unknown) and {} too close: {:.0} < {:.0}. \
                     This could indicate broken incremental distribution.",
                    wells[i].id, wells[j].id, dist, min_acceptable
                );
            }
        }
    }

    #[test]
    fn test_wells_after_destruction_get_unique_angles() {
        // Regression test: wells added after destruction must get NEW unique angles,
        // not reuse angle indices from the current well count.
        // This prevents wells from clustering at similar positions.
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();
        let escape_radius = 2000.0;

        // Add initial wells
        arena.add_orbital_wells(5, escape_radius, &config);
        assert_eq!(arena.orbital_well_count(), 5);
        assert_eq!(arena.next_well_angle_index, 5); // Indices 0-4 used

        // Record positions of all orbital wells
        let original_positions: Vec<(WellId, Vec2)> = arena.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .map(|w| (w.id, w.position))
            .collect();

        // Destroy one well (simulate explosion)
        let well_to_destroy = original_positions[2].0;
        let destroyed_position = original_positions[2].1;
        arena.remove_well(well_to_destroy);
        assert_eq!(arena.orbital_well_count(), 4);

        // Add a replacement well
        arena.add_orbital_wells(1, escape_radius, &config);
        assert_eq!(arena.orbital_well_count(), 5);
        assert_eq!(arena.next_well_angle_index, 6); // Index 5 used, NOT 4 (reused)

        // Find the new well (highest ID)
        let new_well = arena.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .max_by_key(|w| w.id)
            .expect("Should have new well");

        // New well should NOT be at the same position as the destroyed well
        let dist_from_destroyed = (new_well.position - destroyed_position).length();

        // Golden angle separation (137.5° × 3 indices = 412.5° ≈ 52.5° actual)
        // At radius ~1000, 52.5° separation = ~900 units minimum
        // Allow some tolerance for gap-filling radius differences
        assert!(
            dist_from_destroyed > 200.0,
            "New well at {:?} is too close to destroyed well position {:?} (dist: {:.0}). \
             This indicates angle index reuse bug.",
            new_well.position, destroyed_position, dist_from_destroyed
        );
    }

    #[test]
    fn test_angle_index_never_reused() {
        // Verify that next_well_angle_index only increases, never decreases
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();

        assert_eq!(arena.next_well_angle_index, 0);

        arena.add_orbital_wells(3, 1500.0, &config);
        assert_eq!(arena.next_well_angle_index, 3);

        // Remove a well
        let well_id = arena.gravity_wells.keys()
            .find(|&&id| id != CENTRAL_WELL_ID)
            .copied()
            .unwrap();
        arena.remove_well(well_id);

        // Index should NOT decrease
        assert_eq!(arena.next_well_angle_index, 3);

        // Add more wells
        arena.add_orbital_wells(2, 1500.0, &config);
        assert_eq!(arena.next_well_angle_index, 5); // 3 + 2 = 5
    }

    // ========== OPTIMIZATION TESTS: Player Struct Field Reordering ==========
    // These tests verify that performance optimizations don't break functionality

    #[test]
    fn test_player_struct_hot_fields_accessible() {
        // Verify all hot fields (position, velocity, mass, alive) are accessible
        // These are the fields accessed every physics tick and should be in cache line 1
        let mut player = Player::default();

        // Hot fields - accessed every tick
        player.position = Vec2::new(100.0, 200.0);
        player.velocity = Vec2::new(10.0, -5.0);
        player.mass = 150.0;
        player.alive = true;
        player.spawn_protection = 2.5;
        player.rotation = 1.57;

        assert_eq!(player.position.x, 100.0);
        assert_eq!(player.position.y, 200.0);
        assert_eq!(player.velocity.x, 10.0);
        assert_eq!(player.velocity.y, -5.0);
        assert_eq!(player.mass, 150.0);
        assert!(player.alive);
        assert_eq!(player.spawn_protection, 2.5);
        assert_eq!(player.rotation, 1.57);
    }

    #[test]
    fn test_player_struct_warm_fields_accessible() {
        // Verify warm fields (kills, deaths, etc.) are accessible
        // These are accessed during collisions/scoring
        let mut player = Player::default();

        player.kills = 10;
        player.deaths = 3;
        player.respawn_timer = 5.0;
        player.is_bot = true;
        player.color_index = 5;
        player.spawn_tick = 12345;

        assert_eq!(player.kills, 10);
        assert_eq!(player.deaths, 3);
        assert_eq!(player.respawn_timer, 5.0);
        assert!(player.is_bot);
        assert_eq!(player.color_index, 5);
        assert_eq!(player.spawn_tick, 12345);
    }

    #[test]
    fn test_player_struct_cold_fields_accessible() {
        // Verify cold fields (id, name) are accessible
        // These are rarely accessed in hot path
        let id = Uuid::new_v4();
        let player = Player::new(id, "TestPlayer".to_string(), false, 0);

        assert_eq!(player.id, id);
        assert_eq!(player.name, "TestPlayer");
    }

    #[test]
    fn test_player_serialization_round_trip() {
        // Verify Player can be serialized and deserialized correctly
        // Field reordering must not break serialization compatibility
        let id = Uuid::new_v4();
        let mut original = Player::new(id, "SerializeTest".to_string(), true, 7);
        original.position = Vec2::new(123.456, 789.012);
        original.velocity = Vec2::new(-50.0, 25.0);
        original.rotation = 3.14159;
        original.mass = 200.5;
        original.alive = false;
        original.kills = 42;
        original.deaths = 17;
        original.spawn_protection = 1.5;
        original.respawn_timer = 3.0;
        original.spawn_tick = 999;

        // Serialize
        let encoded = bincode::serde::encode_to_vec(&original, bincode::config::standard())
            .expect("Failed to serialize Player");

        // Deserialize
        let (decoded, _): (Player, usize) = bincode::serde::decode_from_slice(
            &encoded,
            bincode::config::standard()
        ).expect("Failed to deserialize Player");

        // Verify all fields match
        assert_eq!(decoded.id, original.id);
        assert_eq!(decoded.name, original.name);
        assert!((decoded.position.x - original.position.x).abs() < 0.001);
        assert!((decoded.position.y - original.position.y).abs() < 0.001);
        assert!((decoded.velocity.x - original.velocity.x).abs() < 0.001);
        assert!((decoded.velocity.y - original.velocity.y).abs() < 0.001);
        assert!((decoded.rotation - original.rotation).abs() < 0.001);
        assert!((decoded.mass - original.mass).abs() < 0.001);
        assert_eq!(decoded.alive, original.alive);
        assert_eq!(decoded.kills, original.kills);
        assert_eq!(decoded.deaths, original.deaths);
        assert!((decoded.spawn_protection - original.spawn_protection).abs() < 0.001);
        assert_eq!(decoded.is_bot, original.is_bot);
        assert_eq!(decoded.color_index, original.color_index);
        assert!((decoded.respawn_timer - original.respawn_timer).abs() < 0.001);
        assert_eq!(decoded.spawn_tick, original.spawn_tick);
    }

    #[test]
    fn test_player_all_fields_preserved_after_clone() {
        // Verify Clone implementation preserves all fields
        let id = Uuid::new_v4();
        let mut original = Player::new(id, "CloneTest".to_string(), true, 3);
        original.position = Vec2::new(500.0, 600.0);
        original.velocity = Vec2::new(100.0, -200.0);
        original.rotation = 2.0;
        original.mass = 300.0;
        original.alive = false;
        original.kills = 99;
        original.deaths = 88;
        original.spawn_protection = 0.5;
        original.respawn_timer = 2.5;
        original.spawn_tick = 555;

        let cloned = original.clone();

        assert_eq!(cloned.id, original.id);
        assert_eq!(cloned.name, original.name);
        assert_eq!(cloned.position.x, original.position.x);
        assert_eq!(cloned.position.y, original.position.y);
        assert_eq!(cloned.velocity.x, original.velocity.x);
        assert_eq!(cloned.velocity.y, original.velocity.y);
        assert_eq!(cloned.rotation, original.rotation);
        assert_eq!(cloned.mass, original.mass);
        assert_eq!(cloned.alive, original.alive);
        assert_eq!(cloned.kills, original.kills);
        assert_eq!(cloned.deaths, original.deaths);
        assert_eq!(cloned.spawn_protection, original.spawn_protection);
        assert_eq!(cloned.is_bot, original.is_bot);
        assert_eq!(cloned.color_index, original.color_index);
        assert_eq!(cloned.respawn_timer, original.respawn_timer);
        assert_eq!(cloned.spawn_tick, original.spawn_tick);
    }

    #[test]
    fn test_player_default_impl_all_fields_initialized() {
        // Verify Default impl initializes all fields to expected values
        let player = Player::default();

        // Default should create a valid player
        assert!(!player.id.is_nil()); // UUID should be generated
        assert_eq!(player.name, "Player");
        assert_eq!(player.position, Vec2::ZERO);
        assert_eq!(player.velocity, Vec2::ZERO);
        assert_eq!(player.rotation, 0.0);
        assert_eq!(player.mass, mass::STARTING);
        assert!(player.alive);
        assert_eq!(player.kills, 0);
        assert_eq!(player.deaths, 0);
        assert!(player.spawn_protection > 0.0); // Should have spawn protection
        assert!(!player.is_bot);
        assert_eq!(player.color_index, 0);
        assert_eq!(player.respawn_timer, 0.0);
        assert_eq!(player.spawn_tick, 0);
    }

    // ========== EXPLOSION TIMER RANDOMIZATION TESTS ==========

    #[test]
    fn test_well_explosion_timer_in_valid_range() {
        // Verify explosion timers are within expected range (30-90 seconds)
        use crate::config::GravityWaveConfig;
        let config = GravityWaveConfig::from_env();
        for _ in 0..100 {
            let timer = config.random_explosion_delay();
            assert!(
                timer >= config.min_explosion_delay && timer < config.max_explosion_delay,
                "Explosion timer {} should be in range [{}, {})",
                timer, config.min_explosion_delay, config.max_explosion_delay
            );
        }
    }

    #[test]
    fn test_well_explosion_timer_randomization_quality() {
        // Create 20 wells and verify their explosion timers are diverse
        // If randomization is broken, all wells would have identical or near-identical timers
        use crate::game::constants::arena::CORE_RADIUS;
        use crate::game::constants::physics::CENTRAL_MASS;

        let mut timers: Vec<f32> = Vec::new();
        for i in 0..20 {
            let well = GravityWell::new(
                i + 1, // Well IDs starting from 1
                Vec2::new(500.0 * (i as f32), 0.0),
                CENTRAL_MASS,
                CORE_RADIUS,
            );
            timers.push(well.explosion_timer);
        }

        // All timers should be in valid range
        for timer in &timers {
            assert!(
                *timer >= 30.0 && *timer < 90.0,
                "Timer {} out of range",
                timer
            );
        }

        // Check for diversity - at least 10 unique values (given 60s range, should be easy)
        let mut unique_timers: Vec<f32> = timers.clone();
        unique_timers.sort_by(|a, b| a.partial_cmp(b).unwrap());
        unique_timers.dedup_by(|a, b| (*a - *b).abs() < 0.001);

        assert!(
            unique_timers.len() >= 10,
            "Expected at least 10 unique timer values, got {}. Timers: {:?}",
            unique_timers.len(),
            timers
        );

        // Check spread - min and max should differ by at least 20 seconds
        let min_timer = timers.iter().cloned().fold(f32::MAX, f32::min);
        let max_timer = timers.iter().cloned().fold(f32::MIN, f32::max);
        let spread = max_timer - min_timer;

        assert!(
            spread >= 20.0,
            "Timer spread {} should be at least 20s (min={}, max={}). Timers: {:?}",
            spread,
            min_timer,
            max_timer,
            timers
        );
    }

    #[test]
    fn test_well_explosion_timers_are_independent() {
        // Verify that creating wells in quick succession still produces random timers
        // This tests that the RNG isn't seeded in a way that causes correlation
        use crate::game::constants::arena::CORE_RADIUS;
        use crate::game::constants::physics::CENTRAL_MASS;

        let well1 = GravityWell::new(1, Vec2::new(100.0, 0.0), CENTRAL_MASS, CORE_RADIUS);
        let well2 = GravityWell::new(2, Vec2::new(200.0, 0.0), CENTRAL_MASS, CORE_RADIUS);
        let well3 = GravityWell::new(3, Vec2::new(300.0, 0.0), CENTRAL_MASS, CORE_RADIUS);

        // Wells should not all have identical timers
        let all_same = (well1.explosion_timer - well2.explosion_timer).abs() < 0.001
            && (well2.explosion_timer - well3.explosion_timer).abs() < 0.001;

        assert!(
            !all_same,
            "Wells created in succession should have different timers: {}, {}, {}",
            well1.explosion_timer,
            well2.explosion_timer,
            well3.explosion_timer
        );
    }

    #[test]
    fn test_arena_wells_have_diverse_explosion_timers() {
        // Verify wells added via scale_for_simulation have diverse timers
        use crate::config::ArenaScalingConfig;

        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();

        // Scale arena to add multiple wells
        for _ in 0..100 {
            arena.scale_for_simulation(100, &config, true);
        }

        // Collect explosion timers from orbital wells
        let timers: Vec<f32> = arena.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .map(|w| w.explosion_timer)
            .collect();

        if timers.len() >= 3 {
            // Check that timers are diverse
            let min_timer = timers.iter().cloned().fold(f32::MAX, f32::min);
            let max_timer = timers.iter().cloned().fold(f32::MIN, f32::max);
            let spread = max_timer - min_timer;

            assert!(
                spread >= 10.0,
                "Arena wells should have diverse explosion timers. Spread: {} (min={}, max={}). Timers: {:?}",
                spread,
                min_timer,
                max_timer,
                timers
            );
        }
    }
}
