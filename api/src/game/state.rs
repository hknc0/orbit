//! Game state definitions and structures
//!
//! Contains all entities (players, projectiles, debris) and arena state.

// Allow dead_code for utility methods that are part of the public API
#![allow(dead_code)]

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::ArenaScalingConfig;
use crate::game::constants::{arena, mass, spawn};
use crate::util::vec2::Vec2;

/// Unique player identifier
pub type PlayerId = Uuid;

/// Entity identifier for non-player entities
pub type EntityId = u64;

/// Player state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub position: Vec2,
    pub velocity: Vec2,
    pub rotation: f32,
    pub mass: f32,
    pub alive: bool,
    pub kills: u32,
    pub deaths: u32,
    pub spawn_protection: f32,
    pub is_bot: bool,
    pub color_index: u8,
    /// Timer until respawn (0 = can respawn, >0 = waiting)
    #[serde(default)]
    pub respawn_timer: f32,
}

impl Player {
    pub fn new(id: PlayerId, name: String, is_bot: bool, color_index: u8) -> Self {
        Self {
            id,
            name,
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            rotation: 0.0,
            mass: mass::STARTING,
            alive: true,
            kills: 0,
            deaths: 0,
            spawn_protection: spawn::PROTECTION_DURATION,
            is_bot,
            color_index,
            respawn_timer: 0.0,
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
            mass,
            core_radius,
            explosion_timer: Self::random_explosion_delay(),
            is_charging: false,
        }
    }

    /// Generate a random explosion delay (30-90 seconds)
    pub fn random_explosion_delay() -> f32 {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        rng.gen_range(30.0..90.0)
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
    pub hit_players: Vec<PlayerId>,
}

impl GravityWave {
    pub fn new(position: Vec2, strength: f32) -> Self {
        Self {
            position,
            radius: 0.0,
            strength,
            age: 0.0,
            hit_players: Vec::new(),
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
    /// Shrink delay counter - arena only shrinks after this reaches 0
    /// Prevents sudden shrinking when players leave temporarily
    #[serde(default)]
    pub shrink_delay_ticks: u32,
    /// Next well ID to assign (monotonically increasing)
    #[serde(default = "default_next_well_id")]
    pub next_well_id: WellId,
}

fn default_next_well_id() -> WellId { 1 }

fn default_scale() -> f32 { 1.0 }

impl Default for Arena {
    fn default() -> Self {
        use crate::game::constants::physics::CENTRAL_MASS;
        let central_well = GravityWell::new(CENTRAL_WELL_ID, Vec2::ZERO, CENTRAL_MASS, arena::CORE_RADIUS);
        let mut wells = HashMap::with_capacity(32);
        wells.insert(CENTRAL_WELL_ID, central_well);
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
            shrink_delay_ticks: 0,
            next_well_id: 1, // Central well uses ID 0
        }
    }
}

impl Arena {
    /// Get the current safe radius based on collapse progress
    pub fn current_safe_radius(&self) -> f32 {
        let base = self.escape_radius * self.scale;
        let reduction_per_phase = (base - self.core_radius) / arena::COLLAPSE_PHASES as f32;
        base - (self.collapse_phase as f32 * reduction_per_phase)
    }

    /// Get scaled escape radius
    pub fn scaled_escape_radius(&self) -> f32 {
        self.escape_radius * self.scale
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
    pub fn remove_well(&mut self, id: WellId) -> Option<GravityWell> {
        self.gravity_wells.remove(&id)
    }

    /// Insert a well into the arena
    pub fn insert_well(&mut self, well: GravityWell) {
        self.gravity_wells.insert(well.id, well);
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

    /// Update arena scale and gravity wells based on player count
    /// If `performance_limit` is provided, it caps the number of wells
    pub fn update_for_player_count(&mut self, player_count: usize) {
        self.update_for_player_count_with_limit(player_count, None);
    }

    /// Update arena scale and gravity wells with optional performance-based limit
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
    /// - GROW: Fast and immediate (players need space)
    /// - SHRINK: Delayed and slow (don't trap players)
    /// Uses ArenaScalingConfig for all tunable parameters
    pub fn scale_for_simulation(&mut self, target_player_count: usize, config: &ArenaScalingConfig) {
        let min_escape = config.min_escape_radius;
        let max_escape = config.min_escape_radius * config.max_escape_multiplier;

        // Calculate target number of wells (not counting central supermassive)
        let players_per_well = 50 / config.wells_per_50_players.max(1);
        let target_wells = ((target_player_count + players_per_well - 1) / players_per_well)
            .max(1)
            .min(config.max_wells);
        let current_orbital_wells = self.gravity_wells.len().saturating_sub(1);

        // Calculate target arena size based on player count
        let additional = (target_player_count.saturating_sub(config.player_threshold) as f32)
            * config.growth_per_player;
        let target_escape = (config.min_escape_radius + additional)
            .min(max_escape)
            .max(min_escape);
        let target_outer = target_escape - 200.0;

        // Track previous radius for proportional well scaling
        let previous_escape = self.escape_radius;

        // Smooth lerp toward target (called every tick at 30Hz)
        let diff = target_escape - self.escape_radius;

        if diff > 1.0 {
            // GROW: Reset shrink delay, lerp quickly
            self.shrink_delay_ticks = config.shrink_delay_ticks;
            let delta = diff * config.grow_lerp;
            self.escape_radius = (self.escape_radius + delta).min(target_escape);
            self.outer_radius = (self.outer_radius + delta).min(target_outer);
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

        // Scale well positions proportionally when arena size changes
        let arena_changed = (self.escape_radius - previous_escape).abs() > 0.1;

        if arena_changed && previous_escape > 1.0 {
            let scale_factor = self.escape_radius / previous_escape;

            for well in self.gravity_wells.values_mut() {
                // Skip central well (ID 0)
                if well.id == CENTRAL_WELL_ID {
                    continue;
                }
                let dist = well.position.length();
                if dist < 1.0 {
                    continue;
                }

                let new_dist = dist * scale_factor;
                let min_dist = self.escape_radius * config.well_min_ratio;
                let max_dist = self.escape_radius * config.well_max_ratio;
                let clamped_dist = new_dist.clamp(min_dist, max_dist);

                let direction = well.position.normalize();
                well.position = direction * clamped_dist;
            }
        }

        // Add new wells if needed (never remove existing ones during gameplay)
        if target_wells > current_orbital_wells {
            let wells_to_add = target_wells - current_orbital_wells;
            self.add_orbital_wells(wells_to_add, self.escape_radius, config);
        }
    }

    /// Legacy version without config for backwards compatibility
    pub fn scale_for_simulation_default(&mut self, target_player_count: usize) {
        self.scale_for_simulation(target_player_count, &ArenaScalingConfig::default());
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

    /// Add orbital wells distributed across multiple rings for better player spread
    /// Ring positions are configurable via ArenaScalingConfig
    pub fn add_orbital_wells(&mut self, count: usize, escape_radius: f32, config: &ArenaScalingConfig) {
        use crate::game::constants::physics::CENTRAL_MASS;
        use rand::Rng;
        use std::f32::consts::TAU;

        let mut rng = rand::thread_rng();
        let size_multipliers = [0.6, 0.8, 1.0, 1.2, 1.4];
        let min_well_distance = arena::OUTER_RADIUS * 1.2;
        const MAX_PLACEMENT_ATTEMPTS: usize = 50;

        // Define orbital rings from config
        let rings: [(f32, f32, f32); 3] = [
            (config.ring_inner_min, config.ring_inner_max, 1.0),
            (config.ring_middle_min, config.ring_middle_max, 2.0),
            (config.ring_outer_min, config.ring_outer_max, 2.0),
        ];
        let total_weight: f32 = rings.iter().map(|(_, _, w)| w).sum();

        for _ in 0..count {
            let size_mult = size_multipliers[rng.gen_range(0..size_multipliers.len())];
            let well_mass = CENTRAL_MASS * size_mult;
            let well_core = arena::CORE_RADIUS * size_mult;

            // Pick a ring based on weights
            let roll: f32 = rng.gen_range(0.0..total_weight);
            let mut cumulative = 0.0;
            let mut selected_ring = (config.ring_middle_min, config.ring_middle_max);
            for (min, max, weight) in &rings {
                cumulative += weight;
                if roll < cumulative {
                    selected_ring = (*min, *max);
                    break;
                }
            }

            let mut best_pos = Vec2::ZERO;
            let mut best_min_dist = 0.0f32;

            for attempt in 0..MAX_PLACEMENT_ATTEMPTS {
                let angle = rng.gen_range(0.0..TAU);
                let radius = rng.gen_range(
                    escape_radius * selected_ring.0..escape_radius * selected_ring.1
                );
                let candidate = Vec2::from_angle(angle) * radius;

                let min_dist = self.gravity_wells
                    .values()
                    .map(|w| (w.position - candidate).length())
                    .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                    .unwrap_or(f32::MAX);

                if min_dist >= min_well_distance {
                    best_pos = candidate;
                    break;
                }

                if min_dist > best_min_dist || attempt == 0 {
                    best_min_dist = min_dist;
                    best_pos = candidate;
                }
            }

            let well_id = self.alloc_well_id();
            let well = GravityWell::new(well_id, best_pos, well_mass, well_core);
            self.gravity_wells.insert(well_id, well);
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
    next_entity_id: EntityId,
}

impl GameState {
    pub fn new() -> Self {
        Self::default()
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
            arena.scale_for_simulation(500, &config);
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
        for _ in 0..50 {
            arena.scale_for_simulation(500, &config);
        }
        let expanded_escape = arena.escape_radius;
        assert!(expanded_escape > 3000.0, "Should have grown significantly");

        // Now request shrink to 10 players
        // First few calls should NOT shrink (delay period)
        arena.scale_for_simulation(10, &config);
        arena.scale_for_simulation(10, &config);
        arena.scale_for_simulation(10, &config);
        assert!(arena.escape_radius >= expanded_escape - 10.0,
            "Should not shrink during delay period");

        // After delay (150+ ticks), should start shrinking slowly
        for _ in 0..200 {
            arena.scale_for_simulation(10, &config);
        }
        assert!(arena.escape_radius < expanded_escape,
            "Should shrink after delay: {} < {}", arena.escape_radius, expanded_escape);

        // But should never go below minimum
        for _ in 0..500 {
            arena.scale_for_simulation(10, &config);
        }
        assert!(arena.escape_radius >= config.min_escape_radius,
            "Should never shrink below minimum: {}", arena.escape_radius);
    }

    #[test]
    fn test_scale_for_simulation_scales_wells_proportionally() {
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();
        let initial_escape = arena.escape_radius;
        assert_eq!(initial_escape, 800.0, "Default arena should be 800");

        // First grow arena and add wells via scale_for_simulation
        for _ in 0..30 {
            arena.scale_for_simulation(100, &config);
        }

        // Record well ratios after initial setup
        let setup_escape = arena.escape_radius;
        let setup_ratios: Vec<f32> = arena.gravity_wells.values()
            .filter(|w| w.id != CENTRAL_WELL_ID)
            .map(|w| w.position.length() / setup_escape)
            .collect();
        let setup_well_count = arena.gravity_wells.len();
        assert!(setup_well_count > 1, "Should have wells after setup");

        // Now scale up significantly
        for _ in 0..100 {
            arena.scale_for_simulation(500, &config);
        }

        let final_escape = arena.escape_radius;

        // Arena should have grown
        assert!(final_escape > setup_escape,
            "Arena should have grown: {} > {}", final_escape, setup_escape);

        // Wells should have scaled proportionally - check that all orbital wells are in valid range
        for well in arena.gravity_wells.values() {
            if well.id != CENTRAL_WELL_ID {
                let current_ratio = well.position.length() / arena.escape_radius;

                // Ratio should be within clamped range
                assert!(current_ratio >= config.well_min_ratio - 0.01 &&
                        current_ratio <= config.well_max_ratio + 0.01,
                    "Well {} ratio {} outside valid range", well.id, current_ratio);
            }
        }

        // Should have same or more wells (never removes)
        assert!(arena.gravity_wells.len() >= setup_well_count,
            "Wells should not decrease: {} >= {}", arena.gravity_wells.len(), setup_well_count);
    }

    #[test]
    fn test_scale_for_simulation_smooth_lerp() {
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();
        let initial_escape = arena.escape_radius;

        // Single call should move toward target with lerp
        arena.scale_for_simulation(1000, &config);
        let after_one = arena.escape_radius;

        // Should have moved but not instantly jumped
        assert!(after_one > initial_escape, "Should start expanding");
        assert!(after_one < initial_escape * 2.0, "Should not jump instantly");

        // Multiple calls should continue converging
        for _ in 0..100 {
            arena.scale_for_simulation(1000, &config);
        }
        let after_many = arena.escape_radius;

        assert!(after_many > after_one, "Should continue expanding");
    }

    #[test]
    fn test_scale_for_simulation_well_cap() {
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();

        // Even with huge player count, wells should be capped
        for _ in 0..100 {
            arena.scale_for_simulation(10000, &config);
        }

        // -1 because central supermassive doesn't count toward the cap
        let orbital_wells = arena.gravity_wells.len() - 1;
        assert!(orbital_wells <= config.max_wells,
            "Orbital wells should be capped at {}, got {}", config.max_wells, orbital_wells);
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
    fn test_scale_grow_resets_shrink_delay() {
        use crate::config::ArenaScalingConfig;
        let config = ArenaScalingConfig::default();
        let mut arena = Arena::default();

        // Grow to large size
        for _ in 0..50 {
            arena.scale_for_simulation(500, &config);
        }

        // Start shrink process
        for _ in 0..3 {
            arena.scale_for_simulation(10, &config);
        }

        // Now request growth again
        arena.scale_for_simulation(500, &config);

        // Shrink delay should be reset
        assert_eq!(arena.shrink_delay_ticks, config.shrink_delay_ticks,
            "Shrink delay should be reset on grow");
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
}
