//! Gravity system for orbital mechanics
//!
//! Applies gravitational forces from gravity wells to all entities.
//! Supports two modes:
//! - Limited: Uses spatial grid for O(nearby) well lookups (default)
//! - Unlimited: All wells affect all entities with cache-optimized processing

#![allow(dead_code)] // Physics utilities for orbital calculations

use rayon::prelude::*;
use rustc_hash::FxHashMap;

use crate::config::{GravityConfig, GravityRangeMode};
use crate::game::constants::physics::{CENTRAL_MASS, G};
use crate::game::state::{GameState, GravityWell, WellId};
use crate::util::vec2::Vec2;

// ============================================================================
// Gravity Well Constants
// ============================================================================

/// Multiplier for core radius to determine minimum safe distance for gravity calculations
/// Prevents extreme forces when entities are very close to well center
const WELL_MIN_DISTANCE_MULTIPLIER: f32 = 2.0;

/// Scale factor for 1/r gravity falloff (tuned for gameplay feel)
/// At 300 units with mass 10000: 0.5 * 10000 / 300 ≈ 16.7 units/s²
const GRAVITY_SCALE_FACTOR: f32 = 0.5;

/// Maximum gravitational acceleration from a single well (units/s²)
/// Prevents extreme accelerations when very close to well core
const GRAVITY_MAX_ACCELERATION: f32 = 100.0;

/// Minimum acceleration threshold below which gravity effect is ignored (units/s²)
/// At typical mass 10000: accel = 0.5 * 10000 / dist
/// At 50000 dist: accel = 0.1 (threshold)
/// This is ~50 AU in game units, negligible effect
const GRAVITY_MIN_ACCELERATION: f32 = 0.1;

/// Pre-computed max distance squared for insignificance culling
/// max_dist = GRAVITY_SCALE_FACTOR * typical_mass / GRAVITY_MIN_ACCELERATION
/// For mass 10000: 0.5 * 10000 / 0.1 = 50000 → squared = 2.5B
/// But we check per-well based on actual mass
const GRAVITY_INSIGNIFICANCE_FACTOR: f32 = GRAVITY_SCALE_FACTOR / GRAVITY_MIN_ACCELERATION;

// ============================================================================
// Legacy Central Gravity Constants
// ============================================================================

/// Minimum squared distance for legacy central gravity (prevents division by zero)
const LEGACY_GRAVITY_MIN_DISTANCE_SQ: f32 = 100.0;

/// Maximum acceleration for legacy central gravity (units/s²)
const LEGACY_GRAVITY_MAX_ACCELERATION: f32 = 500.0;

// ============================================================================
// Inter-Entity Gravity Constants
// ============================================================================

/// Minimum squared distance for inter-entity gravity (below this, collision handles it)
const INTER_ENTITY_MIN_DISTANCE_SQ: f32 = 100.0;

/// Maximum squared distance for inter-entity gravity (beyond this, force is negligible)
const INTER_ENTITY_MAX_DISTANCE_SQ: f32 = 1_000_000.0;

/// Scale factor to make inter-entity gravity subtle relative to well gravity
const INTER_ENTITY_GRAVITY_SCALE: f32 = 0.01;

// ============================================================================
// Orbital Mechanics Constants
// ============================================================================

/// Minimum radius for orbital velocity calculations (prevents division by zero)
const ORBITAL_MIN_RADIUS: f32 = 10.0;

// ============================================================================
// Gravity Wave Constants
// ============================================================================

/// Base surface gravity for wave strength normalization
/// Formula: g = M / R² where M=10000, R=20 → g = 25
/// This is the reference value for 1.0 strength
const BASE_SURFACE_GRAVITY: f32 = 25.0;

/// Minimum strength for gravity wave explosions (small/less dense wells)
const WAVE_STRENGTH_MIN: f32 = 0.4;

/// Maximum strength for gravity wave explosions (large/dense wells)
const WAVE_STRENGTH_MAX: f32 = 1.8;

/// Minimum distance from wave center to apply impulse (prevents zero direction)
const WAVE_MIN_APPLY_DISTANCE: f32 = 1.0;

/// Force multiplier for projectiles hit by gravity waves (lighter than players)
const WAVE_PROJECTILE_FORCE_RATIO: f32 = 0.5;

/// Force multiplier for debris hit by gravity waves (scatter effect)
const WAVE_DEBRIS_FORCE_RATIO: f32 = 0.7;

// ============================================================================
// Gravity Mode Dispatch
// ============================================================================

/// Apply gravity from gravity wells to all entities
/// Dispatches to limited or unlimited mode based on config
pub fn update_central_with_config(state: &mut GameState, config: &GravityConfig, dt: f32) {
    match config.range_mode {
        GravityRangeMode::Limited => update_central_limited(state, config, dt),
        GravityRangeMode::Unlimited => update_central_unlimited(state, dt),
    }
}

/// Calculate gravity acceleration from nearby wells using spatial grid + cache
/// Returns (gx, gy) acceleration components
/// OPTIMIZATION: Uses FxHashMap for faster well ID lookups
#[inline(always)]
fn calculate_gravity_limited(
    px: f32,
    py: f32,
    position: Vec2,
    well_grid: &crate::game::spatial::WellSpatialGrid,
    cache: &WellPositionCache,
    id_to_index: &FxHashMap<WellId, usize>,
    influence_radius_sq: f32,
) -> (f32, f32) {
    let mut gx = 0.0f32;
    let mut gy = 0.0f32;

    for well_id in well_grid.query_nearby(position) {
        if let Some(&idx) = id_to_index.get(&well_id) {
            let dx = cache.positions_x[idx] - px;
            let dy = cache.positions_y[idx] - py;
            let dist_sq = dx * dx + dy * dy;

            // Skip wells outside effective range (min of config radius and per-well max)
            // Per-well max is mass-dependent: insignificance culling
            let effective_max_sq = cache.max_distance_sq[idx].min(influence_radius_sq);
            if dist_sq > cache.min_distance_sq[idx] && dist_sq < effective_max_sq {
                let dist = dist_sq.sqrt();
                let inv_dist = 1.0 / dist;
                let accel = (GRAVITY_SCALE_FACTOR * cache.masses[idx] * inv_dist)
                    .min(GRAVITY_MAX_ACCELERATION);
                gx += dx * inv_dist * accel;
                gy += dy * inv_dist * accel;
            }
        }
    }

    (gx, gy)
}

/// Limited mode: Use spatial grid to only consider nearby wells
/// O(nearby_wells) per entity - efficient when wells are sparse relative to entities
///
/// PERFORMANCE: Uses WellPositionCache built once per tick instead of cloning HashMap.
/// The cache provides O(1) indexed lookup for well data queried from the spatial grid.
fn update_central_limited(state: &mut GameState, config: &GravityConfig, dt: f32) {
    let influence_radius_sq = config.influence_radius * config.influence_radius;

    // Build cache once per tick - struct-of-arrays for cache locality
    // Also build ID-to-index map for O(1) lookups from spatial grid queries
    // OPTIMIZATION: Use FxHashMap for faster lookup with small integer keys
    let cache = WellPositionCache::from_wells(state.arena.gravity_wells.values());
    let id_to_index: FxHashMap<WellId, usize> = cache
        .ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    // Apply gravity to players in parallel
    state.players.par_values_mut().for_each(|player| {
        if !player.alive {
            return;
        }
        let (gx, gy) = calculate_gravity_limited(
            player.position.x,
            player.position.y,
            player.position,
            &state.arena.well_grid,
            &cache,
            &id_to_index,
            influence_radius_sq,
        );
        player.velocity.x += gx * dt;
        player.velocity.y += gy * dt;
    });

    // Apply gravity to projectiles in parallel
    state.projectiles.par_iter_mut().for_each(|projectile| {
        let (gx, gy) = calculate_gravity_limited(
            projectile.position.x,
            projectile.position.y,
            projectile.position,
            &state.arena.well_grid,
            &cache,
            &id_to_index,
            influence_radius_sq,
        );
        projectile.velocity.x += gx * dt;
        projectile.velocity.y += gy * dt;
    });

    // Apply gravity to debris in parallel
    state.debris.par_iter_mut().for_each(|debris| {
        let (gx, gy) = calculate_gravity_limited(
            debris.position.x,
            debris.position.y,
            debris.position,
            &state.arena.well_grid,
            &cache,
            &id_to_index,
            influence_radius_sq,
        );
        debris.velocity.x += gx * dt;
        debris.velocity.y += gy * dt;
    });
}

// ============================================================================
// Cache-Optimized Unlimited Mode
// ============================================================================

/// Cache-friendly well position data for batch gravity calculations
/// Uses struct-of-arrays layout for better CPU cache utilization
#[derive(Clone)]
pub struct WellPositionCache {
    /// Well IDs (for reference only)
    pub ids: Vec<WellId>,
    /// X coordinates of well positions
    pub positions_x: Vec<f32>,
    /// Y coordinates of well positions
    pub positions_y: Vec<f32>,
    /// Well masses
    pub masses: Vec<f32>,
    /// Pre-computed minimum distance squared (core_radius * MULTIPLIER)^2
    pub min_distance_sq: Vec<f32>,
    /// Pre-computed maximum distance squared for insignificance culling
    /// Wells beyond this distance produce acceleration < GRAVITY_MIN_ACCELERATION
    pub max_distance_sq: Vec<f32>,
}

impl WellPositionCache {
    /// Build cache from gravity wells
    pub fn from_wells<'a>(wells: impl Iterator<Item = &'a GravityWell>) -> Self {
        let mut cache = Self {
            ids: Vec::new(),
            positions_x: Vec::new(),
            positions_y: Vec::new(),
            masses: Vec::new(),
            min_distance_sq: Vec::new(),
            max_distance_sq: Vec::new(),
        };

        for well in wells {
            cache.ids.push(well.id);
            cache.positions_x.push(well.position.x);
            cache.positions_y.push(well.position.y);
            cache.masses.push(well.mass);

            // Minimum distance: too close causes extreme forces
            let min_dist = well.core_radius * WELL_MIN_DISTANCE_MULTIPLIER;
            cache.min_distance_sq.push(min_dist * min_dist);

            // Maximum distance: beyond this, acceleration < GRAVITY_MIN_ACCELERATION
            // accel = GRAVITY_SCALE_FACTOR * mass / dist
            // GRAVITY_MIN_ACCELERATION = GRAVITY_SCALE_FACTOR * mass / max_dist
            // max_dist = GRAVITY_SCALE_FACTOR * mass / GRAVITY_MIN_ACCELERATION
            //          = mass * GRAVITY_INSIGNIFICANCE_FACTOR
            let max_dist = well.mass * GRAVITY_INSIGNIFICANCE_FACTOR;
            cache.max_distance_sq.push(max_dist * max_dist);
        }

        cache
    }

    /// Calculate total gravity at a position from all wells (cache-friendly)
    /// Uses insignificance culling to skip wells too far away to matter
    #[inline]
    pub fn calculate_gravity(&self, px: f32, py: f32) -> Vec2 {
        let mut gx = 0.0f32;
        let mut gy = 0.0f32;
        let len = self.ids.len();

        // Branchless inner loop for auto-vectorization
        for i in 0..len {
            let dx = self.positions_x[i] - px;
            let dy = self.positions_y[i] - py;
            let dist_sq = dx * dx + dy * dy;

            // Skip if too close or too far (insignificance culling)
            let min_sq = self.min_distance_sq[i];
            let max_sq = self.max_distance_sq[i];
            if dist_sq > min_sq && dist_sq < max_sq {
                let dist = dist_sq.sqrt();
                let inv_dist = 1.0 / dist;

                // Normalized direction
                let dir_x = dx * inv_dist;
                let dir_y = dy * inv_dist;

                // 1/r gravity with clamping
                let accel = (GRAVITY_SCALE_FACTOR * self.masses[i] * inv_dist)
                    .min(GRAVITY_MAX_ACCELERATION);

                gx += dir_x * accel;
                gy += dir_y * accel;
            }
        }

        Vec2::new(gx, gy)
    }

    /// Number of wells in cache
    #[inline]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Check if cache is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// Unlimited mode: All wells affect all entities
/// Uses cache-optimized batch processing for O(W) per entity
fn update_central_unlimited(state: &mut GameState, dt: f32) {
    // Build cache once per tick (struct-of-arrays for cache locality)
    let cache = WellPositionCache::from_wells(state.arena.gravity_wells.values());

    // Apply gravity to players in parallel
    state.players.par_values_mut().for_each(|player| {
        if !player.alive {
            return;
        }

        let gravity = cache.calculate_gravity(player.position.x, player.position.y);
        player.velocity += gravity * dt;
    });

    // Apply gravity to projectiles in parallel
    state.projectiles.par_iter_mut().for_each(|projectile| {
        let gravity = cache.calculate_gravity(projectile.position.x, projectile.position.y);
        projectile.velocity += gravity * dt;
    });

    // Apply gravity to debris in parallel
    state.debris.par_iter_mut().for_each(|debris| {
        let gravity = cache.calculate_gravity(debris.position.x, debris.position.y);
        debris.velocity += gravity * dt;
    });
}

// ============================================================================
// Legacy API (for backward compatibility)
// ============================================================================

/// Apply gravity from all gravity wells to all entities (legacy API)
/// Uses unlimited mode for backward compatibility
/// Prefer update_central_with_config for new code
pub fn update_central(state: &mut GameState, dt: f32) {
    // Collect wells into Vec for parallel iteration (HashMap doesn't support parallel access)
    let wells: Vec<GravityWell> = state.arena.gravity_wells.values().cloned().collect();

    // Apply gravity to players in parallel
    state.players.par_values_mut().for_each(|player| {
        if !player.alive {
            return;
        }

        let gravity = calculate_multi_well_gravity(player.position, wells.iter());
        player.velocity += gravity * dt;
    });

    // Apply gravity to projectiles in parallel
    state.projectiles.par_iter_mut().for_each(|projectile| {
        let gravity = calculate_multi_well_gravity(projectile.position, wells.iter());
        projectile.velocity += gravity * dt;
    });

    // Apply gravity to debris in parallel
    state.debris.par_iter_mut().for_each(|debris| {
        let gravity = calculate_multi_well_gravity(debris.position, wells.iter());
        debris.velocity += gravity * dt;
    });
}

/// Calculate gravitational acceleration from multiple gravity wells
/// Accepts any iterator over well references for flexibility with different storage types
pub fn calculate_multi_well_gravity<'a>(position: Vec2, wells: impl Iterator<Item = &'a GravityWell>) -> Vec2 {
    let mut total_gravity = Vec2::ZERO;

    for well in wells {
        let gravity = calculate_gravity_from_well(position, well);
        total_gravity += gravity;
    }

    total_gravity
}

/// Calculate gravitational acceleration toward a single gravity well
/// Uses modified gravity: 1/r falloff instead of 1/r² for better gameplay feel
/// This makes gravity noticeable at typical orbital distances (300-600 units)
pub fn calculate_gravity_from_well(position: Vec2, well: &GravityWell) -> Vec2 {
    let delta = well.position - position;
    let distance_sq = delta.length_sq();

    // Prevent division by zero and extreme forces near well center
    let min_distance_sq = (well.core_radius * WELL_MIN_DISTANCE_MULTIPLIER).powi(2);
    if distance_sq <= min_distance_sq {
        return Vec2::ZERO;
    }

    let distance = distance_sq.sqrt();

    // Direction toward well
    let direction = delta * (1.0 / distance);

    // Gravitational acceleration with 1/r falloff (not 1/r²)
    // This gives a more noticeable pull at gameplay distances
    let acceleration = GRAVITY_SCALE_FACTOR * well.mass / distance;

    // Clamp to prevent extreme accelerations near core
    let clamped_accel = acceleration.min(GRAVITY_MAX_ACCELERATION);

    direction * clamped_accel
}

/// Legacy function for backward compatibility - calculates gravity toward origin
pub fn calculate_central_gravity(position: Vec2, _mass: f32) -> Vec2 {
    let distance_sq = position.length_sq();

    // Prevent division by zero and extreme forces at center
    if distance_sq < LEGACY_GRAVITY_MIN_DISTANCE_SQ {
        return Vec2::ZERO;
    }

    let distance = distance_sq.sqrt();

    // Direction toward center (use already-computed distance to avoid recomputing)
    let direction = -position * (1.0 / distance);

    // Gravitational acceleration magnitude: G * M / r^2
    let acceleration = G * CENTRAL_MASS / distance_sq;

    // Clamp to prevent extreme accelerations
    let clamped_accel = acceleration.min(LEGACY_GRAVITY_MAX_ACCELERATION);

    direction * clamped_accel
}

/// Apply inter-entity gravity (entities attract each other)
/// This is optional and can be disabled for performance
/// Uses rayon to parallelize gravity calculation per player
pub fn update_inter_entity(state: &mut GameState, dt: f32) {
    use crate::game::state::PlayerId;

    // Collect alive player data for calculations
    let players_data: Vec<(PlayerId, Vec2, f32)> = state
        .players
        .values()
        .filter(|p| p.alive)
        .map(|p| (p.id, p.position, p.mass))
        .collect();

    // Calculate gravitational accelerations for each player in parallel
    // Each player calculates its own acceleration from all other players
    let accelerations: Vec<(PlayerId, Vec2)> = players_data
        .par_iter()
        .map(|&(id_i, pos_i, mass_i)| {
            let mut accel = Vec2::ZERO;

            for &(id_j, pos_j, mass_j) in &players_data {
                if id_i == id_j {
                    continue;
                }

                let delta = pos_j - pos_i;
                let distance_sq = delta.length_sq();

                // Skip if too close (handled by collision) or too far
                if distance_sq < INTER_ENTITY_MIN_DISTANCE_SQ || distance_sq > INTER_ENTITY_MAX_DISTANCE_SQ {
                    continue;
                }

                let distance = distance_sq.sqrt();
                let direction = delta * (1.0 / distance);

                // Gravitational force: F = G * m1 * m2 / r^2
                // Scale down inter-entity gravity to be subtle
                let force_magnitude = G * mass_i * mass_j / distance_sq * INTER_ENTITY_GRAVITY_SCALE;

                // F = ma, so a = F/m
                accel += direction * (force_magnitude / mass_i);
            }

            (id_i, accel)
        })
        .collect();

    // Apply accumulated accelerations (sequential - requires mutable access)
    for (player_id, accel) in accelerations {
        if let Some(player) = state.get_player_mut(player_id) {
            if player.alive {
                player.velocity += accel * dt;
            }
        }
    }
}

/// Calculate orbital velocity for a circular orbit at given radius
pub fn orbital_velocity(radius: f32) -> f32 {
    // Prevent division by zero
    let safe_radius = radius.max(ORBITAL_MIN_RADIUS);
    // v = sqrt(G * M / r)
    (G * CENTRAL_MASS / safe_radius).sqrt()
}

/// Calculate escape velocity at given radius
pub fn escape_velocity(radius: f32) -> f32 {
    // v_escape = sqrt(2 * G * M / r) = sqrt(2) * v_orbital
    orbital_velocity(radius) * std::f32::consts::SQRT_2
}

/// Check if an entity is in a stable orbit (roughly)
pub fn is_in_orbit(position: Vec2, velocity: Vec2, tolerance: f32) -> bool {
    let radius = position.length();
    if radius < ORBITAL_MIN_RADIUS {
        return false;
    }

    // Check if velocity is roughly perpendicular to position (circular orbit)
    let radial_dir = position.normalize();
    let radial_component = velocity.dot(radial_dir).abs();
    let tangential_component = velocity.cross(radial_dir).abs();

    // Velocity should be mostly tangential
    if tangential_component < radial_component {
        return false;
    }

    // Check if speed is close to orbital velocity
    let speed = velocity.length();
    let orbital = orbital_velocity(radius);

    (speed - orbital).abs() / orbital < tolerance
}

// === GRAVITY WAVE EXPLOSION SYSTEM ===

use crate::config::GravityWaveConfig;

/// Events generated by the gravity wave system
#[derive(Debug)]
pub enum GravityWaveEvent {
    /// Well started charging (warning)
    WellCharging { well_id: WellId, position: Vec2 },
    /// Well exploded, wave created
    WellExploded { well_id: WellId, position: Vec2, strength: f32 },
    /// Well was destroyed (removed from arena after explosion)
    WellDestroyed { well_id: WellId, position: Vec2 },
}

/// Update explosion timers and create waves when wells explode
/// Returns events for charging and explosions
/// Config controls all timing and force parameters
///
/// Wells are ALWAYS destroyed when their timer expires. This keeps the arena dynamic:
/// - Old wells explode and are removed
/// - `scale_for_simulation()` adds new wells at DIFFERENT positions (golden angle distribution)
/// - Wells naturally cycle through new locations over time
///
/// Note: `_target_wells` and `_escape_radius` are kept for API compatibility but no longer used
pub fn update_explosions(
    state: &mut GameState,
    config: &GravityWaveConfig,
    dt: f32,
    _target_wells: usize,
    _escape_radius: f32,
) -> Vec<GravityWaveEvent> {
    use crate::game::state::GravityWave;

    let mut events = Vec::new();
    let mut new_waves = Vec::new();
    let mut wells_to_remove: Vec<WellId> = Vec::new();

    // Skip central well (ID 0 - supermassive black hole, too stable to explode)
    for well in state.arena.gravity_wells.values_mut() {
        if well.id == crate::game::state::CENTRAL_WELL_ID {
            continue; // Central well never explodes
        }

        well.explosion_timer -= dt;

        // Check if entering charge phase (warning)
        if !well.is_charging && well.explosion_timer <= config.charge_duration && well.explosion_timer > 0.0 {
            well.is_charging = true;
            events.push(GravityWaveEvent::WellCharging {
                well_id: well.id,
                position: well.position,
            });
        }

        // Check for explosion
        if well.explosion_timer <= 0.0 {
            // Calculate strength using surface gravity (most realistic)
            // Formula: g = M / R² (surface gravity proportional to density)
            // Dense, compact wells produce stronger waves than diffuse ones
            let surface_gravity = well.mass / (well.core_radius * well.core_radius);
            let normalized = surface_gravity / BASE_SURFACE_GRAVITY;
            let strength = normalized.sqrt().clamp(WAVE_STRENGTH_MIN, WAVE_STRENGTH_MAX);

            events.push(GravityWaveEvent::WellExploded {
                well_id: well.id,
                position: well.position,
                strength,
            });

            // Create the gravity wave
            new_waves.push(GravityWave::new(well.position, strength));

            // ALWAYS destroy the well - scale_for_simulation will add new ones at DIFFERENT positions
            // This keeps the arena dynamic: wells cycle through new locations over time
            // The golden angle distribution ensures new wells appear at different angles
            wells_to_remove.push(well.id);
            events.push(GravityWaveEvent::WellDestroyed {
                well_id: well.id,
                position: well.position,
            });
        }
    }

    // Remove wells by ID (O(n) per removal, but removals are rare)
    for well_id in wells_to_remove {
        state.arena.remove_well(well_id);
    }

    // Add new waves to state
    state.gravity_waves.extend(new_waves);

    events
}

/// Update active gravity waves - expand them and apply forces to players, projectiles, and debris
/// Config controls wave speed, thickness, and impulse force
pub fn update_waves(state: &mut GameState, config: &GravityWaveConfig, dt: f32) {
    // Expand waves and apply forces
    for wave in state.gravity_waves.iter_mut() {
        let _prev_radius = wave.radius;
        wave.radius += config.wave_speed * dt;
        wave.age += dt;

        // Wave front boundaries
        let wave_inner = (wave.radius - config.wave_front_thickness * 0.5).max(0.0);
        let wave_outer = wave.radius + config.wave_front_thickness * 0.5;

        // Apply impulse to players in the wave front
        for player in state.players.values_mut() {
            if !player.alive || wave.hit_players.contains(&player.id) {
                continue;
            }

            let delta = player.position - wave.position;
            let dist = delta.length();

            if dist >= wave_inner && dist <= wave_outer && dist > WAVE_MIN_APPLY_DISTANCE {
                let direction = delta.normalize();
                let distance_decay = 1.0 - (wave.radius / config.wave_max_radius).min(1.0);
                let force = config.wave_base_impulse * wave.strength * distance_decay;

                player.velocity += direction * force;
                wave.hit_players.push(player.id);
            }
        }

        // Apply impulse to projectiles in the wave front (lighter than players)
        for projectile in state.projectiles.iter_mut() {
            let delta = projectile.position - wave.position;
            let dist = delta.length();

            if dist >= wave_inner && dist <= wave_outer && dist > WAVE_MIN_APPLY_DISTANCE {
                let direction = delta.normalize();
                let distance_decay = 1.0 - (wave.radius / config.wave_max_radius).min(1.0);
                let force = config.wave_base_impulse * wave.strength * distance_decay * WAVE_PROJECTILE_FORCE_RATIO;

                projectile.velocity += direction * force;
            }
        }

        // Apply impulse to debris in the wave front (scatter effect)
        for debris in state.debris.iter_mut() {
            let delta = debris.position - wave.position;
            let dist = delta.length();

            if dist >= wave_inner && dist <= wave_outer && dist > WAVE_MIN_APPLY_DISTANCE {
                let direction = delta.normalize();
                let distance_decay = 1.0 - (wave.radius / config.wave_max_radius).min(1.0);
                let force = config.wave_base_impulse * wave.strength * distance_decay * WAVE_DEBRIS_FORCE_RATIO;

                debris.velocity += direction * force;
            }
        }
    }

    // Remove expired waves
    state.gravity_waves.retain(|w| w.radius < config.wave_max_radius);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::constants::physics::DT;
    use crate::game::state::Player;

    fn create_test_state() -> (GameState, uuid::Uuid) {
        let mut state = GameState::new();
        let player_id = uuid::Uuid::new_v4();
        let player = Player {
            id: player_id,
            name: "Test".to_string(),
            position: Vec2::new(300.0, 0.0),
            velocity: Vec2::ZERO,
            rotation: 0.0,
            mass: 100.0,
            alive: true,
            kills: 0,
            deaths: 0,
            spawn_protection: 0.0,
            is_bot: false,
            color_index: 0,
            respawn_timer: 0.0,
        };
        state.add_player(player);
        (state, player_id)
    }

    #[test]
    fn test_central_gravity_direction() {
        let position = Vec2::new(100.0, 0.0);
        let gravity = calculate_central_gravity(position, 100.0);

        // Should pull toward center (negative x direction)
        assert!(gravity.x < 0.0);
        assert!(gravity.y.abs() < 0.001);
    }

    #[test]
    fn test_central_gravity_diagonal() {
        let position = Vec2::new(100.0, 100.0);
        let gravity = calculate_central_gravity(position, 100.0);

        // Should pull toward center
        assert!(gravity.x < 0.0);
        assert!(gravity.y < 0.0);
    }

    #[test]
    fn test_gravity_inverse_square() {
        // At double distance, gravity should be 1/4
        let pos1 = Vec2::new(100.0, 0.0);
        let pos2 = Vec2::new(200.0, 0.0);

        let g1 = calculate_central_gravity(pos1, 100.0).length();
        let g2 = calculate_central_gravity(pos2, 100.0).length();

        // g2 should be approximately 1/4 of g1
        let ratio = g1 / g2;
        assert!((ratio - 4.0).abs() < 0.1);
    }

    #[test]
    fn test_gravity_zero_at_center() {
        let position = Vec2::new(5.0, 5.0); // Very close to center
        let gravity = calculate_central_gravity(position, 100.0);

        assert_eq!(gravity, Vec2::ZERO);
    }

    #[test]
    fn test_update_central_applies_to_players() {
        let (mut state, player_id) = create_test_state();
        let initial_velocity = state.get_player(player_id).unwrap().velocity;

        update_central(&mut state, DT);

        // Velocity should have changed toward center
        assert!(state.get_player(player_id).unwrap().velocity.x < initial_velocity.x);
    }

    #[test]
    fn test_dead_players_no_gravity() {
        let (mut state, player_id) = create_test_state();
        state.get_player_mut(player_id).unwrap().alive = false;
        state.get_player_mut(player_id).unwrap().velocity = Vec2::ZERO;

        update_central(&mut state, DT);

        assert_eq!(state.get_player(player_id).unwrap().velocity, Vec2::ZERO);
    }

    #[test]
    fn test_orbital_velocity() {
        let radius = 300.0;
        let v_orbital = orbital_velocity(radius);

        // Verify: v^2 = G*M/r
        let v_squared = v_orbital * v_orbital;
        let expected = G * CENTRAL_MASS / radius;

        assert!((v_squared - expected).abs() < 0.01);
    }

    #[test]
    fn test_escape_velocity() {
        let radius = 300.0;
        let v_orbital = orbital_velocity(radius);
        let v_escape = escape_velocity(radius);

        // v_escape = sqrt(2) * v_orbital
        let ratio = v_escape / v_orbital;
        assert!((ratio - std::f32::consts::SQRT_2).abs() < 0.001);
    }

    #[test]
    fn test_is_in_orbit() {
        let radius = 300.0;
        let v = orbital_velocity(radius);

        // Position on x-axis, velocity in y direction (circular orbit)
        let position = Vec2::new(radius, 0.0);
        let velocity = Vec2::new(0.0, v);

        assert!(is_in_orbit(position, velocity, 0.1));
    }

    #[test]
    fn test_not_in_orbit_radial_velocity() {
        let radius = 300.0;
        let position = Vec2::new(radius, 0.0);
        // Velocity pointing away from center (radial, not orbital)
        let velocity = Vec2::new(100.0, 0.0);

        assert!(!is_in_orbit(position, velocity, 0.1));
    }

    #[test]
    fn test_inter_entity_gravity_attracts() {
        let mut state = GameState::new();

        // Two players at different positions
        let player_a_id = uuid::Uuid::new_v4();
        let player_a = Player {
            id: player_a_id,
            name: "A".to_string(),
            position: Vec2::new(100.0, 0.0),
            velocity: Vec2::ZERO,
            mass: 200.0,
            alive: true,
            ..Default::default()
        };
        state.add_player(player_a);

        let player_b_id = uuid::Uuid::new_v4();
        let player_b = Player {
            id: player_b_id,
            name: "B".to_string(),
            position: Vec2::new(200.0, 0.0),
            velocity: Vec2::ZERO,
            mass: 200.0,
            alive: true,
            ..Default::default()
        };
        state.add_player(player_b);

        update_inter_entity(&mut state, DT);

        // Players should be attracted toward each other
        assert!(state.get_player(player_a_id).unwrap().velocity.x > 0.0); // A moves toward B
        assert!(state.get_player(player_b_id).unwrap().velocity.x < 0.0); // B moves toward A
    }

    #[test]
    fn test_gravity_applies_to_projectiles() {
        let (mut state, _) = create_test_state();
        state.add_projectile(
            uuid::Uuid::new_v4(),
            Vec2::new(200.0, 0.0),
            Vec2::new(0.0, 50.0),
            20.0,
        );

        update_central(&mut state, DT);

        // Projectile should be pulled toward center
        assert!(state.projectiles[0].velocity.x < 0.0);
    }

    #[test]
    fn test_gravity_applies_to_debris() {
        use crate::game::state::DebrisSize;

        let (mut state, _) = create_test_state();
        state.add_debris(Vec2::new(200.0, 0.0), Vec2::ZERO, DebrisSize::Medium);

        update_central(&mut state, DT);

        // Debris should be pulled toward center (toward first well)
        // Direction depends on well configuration
        assert!(state.debris[0].velocity.length() > 0.0);
    }

    // === GRAVITY WAVE TESTS ===

    #[test]
    fn test_wave_explosion_timer_decreases() {
        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Find first non-central well and set it to explode soon
        let orbital_well_id = state.arena.gravity_wells.values()
            .find(|w| w.id != crate::game::state::CENTRAL_WELL_ID)
            .map(|w| w.id);

        if let Some(well_id) = orbital_well_id {
            if let Some(well) = state.arena.gravity_wells.get_mut(&well_id) {
                well.explosion_timer = 10.0;
            }
        }

        let initial_timer = orbital_well_id
            .and_then(|id| state.arena.gravity_wells.get(&id))
            .map(|w| w.explosion_timer)
            .unwrap_or(0.0);

        // Use high target_wells and large escape_radius to prevent removal
        update_explosions(&mut state, &config, DT, 100, 10000.0);

        if let Some(well_id) = orbital_well_id {
            if let Some(well) = state.arena.gravity_wells.get(&well_id) {
                assert!(well.explosion_timer < initial_timer);
            }
        }
    }

    #[test]
    fn test_wave_explosion_creates_wave() {
        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Find first non-central well and set it to explode immediately
        let orbital_well_id = state.arena.gravity_wells.values()
            .find(|w| w.id != crate::game::state::CENTRAL_WELL_ID)
            .map(|w| w.id);

        if let Some(well_id) = orbital_well_id {
            if let Some(well) = state.arena.gravity_wells.get_mut(&well_id) {
                well.explosion_timer = 0.0;
                well.is_charging = true;
            }

            // Use high target_wells and large escape_radius to prevent removal
            let events = update_explosions(&mut state, &config, DT, 100, 10000.0);

            // Should create an explosion event
            assert!(events.iter().any(|e| matches!(e, GravityWaveEvent::WellExploded { .. })));
            // Should create a wave
            assert!(!state.gravity_waves.is_empty());
        }
    }

    #[test]
    fn test_wave_charging_event() {
        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Find first non-central well and set it to enter charging phase
        let orbital_well_id = state.arena.gravity_wells.values()
            .find(|w| w.id != crate::game::state::CENTRAL_WELL_ID)
            .map(|w| w.id);

        if let Some(well_id) = orbital_well_id {
            if let Some(well) = state.arena.gravity_wells.get_mut(&well_id) {
                well.explosion_timer = config.charge_duration - 0.1;
                well.is_charging = false;
            }

            // Use high target_wells and large escape_radius to prevent removal
            let events = update_explosions(&mut state, &config, DT, 100, 10000.0);

            // Should create a charging event
            assert!(events.iter().any(|e| matches!(e, GravityWaveEvent::WellCharging { .. })));

            if let Some(well) = state.arena.gravity_wells.get(&well_id) {
                assert!(well.is_charging);
            }
        }
    }

    #[test]
    fn test_wave_expands_over_time() {
        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Add a wave manually
        state.gravity_waves.push(crate::game::state::GravityWave::new(Vec2::ZERO, 1.0));
        let initial_radius = state.gravity_waves[0].radius;

        update_waves(&mut state, &config, DT);

        assert!(state.gravity_waves[0].radius > initial_radius);
    }

    #[test]
    fn test_wave_removes_when_max_radius() {
        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Add a wave at max radius
        let mut wave = crate::game::state::GravityWave::new(Vec2::ZERO, 1.0);
        wave.radius = config.wave_max_radius - 1.0;
        state.gravity_waves.push(wave);

        // Update should expand beyond max and remove
        update_waves(&mut state, &config, 100.0);

        assert!(state.gravity_waves.is_empty());
    }

    #[test]
    fn test_wave_applies_impulse_to_player() {
        let (mut state, player_id) = create_test_state();
        let config = GravityWaveConfig::default();

        // Position player where wave will hit
        state.get_player_mut(player_id).unwrap().position = Vec2::new(50.0, 0.0);
        let initial_velocity = state.get_player(player_id).unwrap().velocity;

        // Add wave that will hit the player
        let mut wave = crate::game::state::GravityWave::new(Vec2::ZERO, 1.0);
        wave.radius = 50.0 - config.wave_front_thickness * 0.3;
        state.gravity_waves.push(wave);

        update_waves(&mut state, &config, DT);

        // Player should have been pushed outward
        let new_velocity = state.get_player(player_id).unwrap().velocity;
        assert!(new_velocity.length() > initial_velocity.length());
    }

    #[test]
    fn test_wave_applies_impulse_to_projectile() {
        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Add projectile in wave path
        state.add_projectile(
            uuid::Uuid::new_v4(),
            Vec2::new(50.0, 0.0),
            Vec2::ZERO,
            10.0,
        );

        // Add wave that will hit the projectile
        let mut wave = crate::game::state::GravityWave::new(Vec2::ZERO, 1.0);
        wave.radius = 50.0 - config.wave_front_thickness * 0.3;
        state.gravity_waves.push(wave);

        update_waves(&mut state, &config, DT);

        // Projectile should have been pushed (50% force)
        assert!(state.projectiles[0].velocity.length() > 0.0);
    }

    #[test]
    fn test_wave_applies_impulse_to_debris() {
        use crate::game::state::DebrisSize;

        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Add debris in wave path
        state.add_debris(Vec2::new(50.0, 0.0), Vec2::ZERO, DebrisSize::Small);

        // Add wave that will hit the debris
        let mut wave = crate::game::state::GravityWave::new(Vec2::ZERO, 1.0);
        wave.radius = 50.0 - config.wave_front_thickness * 0.3;
        state.gravity_waves.push(wave);

        update_waves(&mut state, &config, DT);

        // Debris should have been pushed (70% force)
        assert!(state.debris[0].velocity.length() > 0.0);
    }

    #[test]
    fn test_wave_only_hits_player_once() {
        let (mut state, player_id) = create_test_state();
        let config = GravityWaveConfig::default();

        state.get_player_mut(player_id).unwrap().position = Vec2::new(50.0, 0.0);

        // Add wave that will hit the player
        let mut wave = crate::game::state::GravityWave::new(Vec2::ZERO, 1.0);
        wave.radius = 50.0 - config.wave_front_thickness * 0.3;
        state.gravity_waves.push(wave);

        // First update - player gets hit
        update_waves(&mut state, &config, DT);
        let velocity_after_first = state.get_player(player_id).unwrap().velocity;

        // Reset velocity to track second hit
        state.get_player_mut(player_id).unwrap().velocity = Vec2::ZERO;

        // Second update - wave still passing, but player should NOT be hit again
        update_waves(&mut state, &config, DT);
        let velocity_after_second = state.get_player(player_id).unwrap().velocity;

        // Second hit should not have occurred (hit_players tracking)
        assert!(velocity_after_second.length() < velocity_after_first.length() * 0.1);
    }

    #[test]
    fn test_wave_strength_affects_impulse() {
        let mut state1 = GameState::new();
        let mut state2 = GameState::new();
        let config = GravityWaveConfig::default();

        // Add players at same position
        let player1 = Player {
            id: uuid::Uuid::new_v4(),
            name: "Test".to_string(),
            position: Vec2::new(50.0, 0.0),
            velocity: Vec2::ZERO,
            mass: 100.0,
            alive: true,
            ..Default::default()
        };
        let player2 = Player {
            id: uuid::Uuid::new_v4(),
            name: "Test".to_string(),
            position: Vec2::new(50.0, 0.0),
            velocity: Vec2::ZERO,
            mass: 100.0,
            alive: true,
            ..Default::default()
        };
        let id1 = player1.id;
        let id2 = player2.id;
        state1.add_player(player1);
        state2.add_player(player2);

        // Add weak wave to state1 (min strength = 0.4)
        let mut weak_wave = crate::game::state::GravityWave::new(Vec2::ZERO, WAVE_STRENGTH_MIN);
        weak_wave.radius = 50.0 - config.wave_front_thickness * 0.3;
        state1.gravity_waves.push(weak_wave);

        // Add strong wave to state2 (max strength = 1.8)
        let mut strong_wave = crate::game::state::GravityWave::new(Vec2::ZERO, WAVE_STRENGTH_MAX);
        strong_wave.radius = 50.0 - config.wave_front_thickness * 0.3;
        state2.gravity_waves.push(strong_wave);

        update_waves(&mut state1, &config, DT);
        update_waves(&mut state2, &config, DT);

        let vel1 = state1.get_player(id1).unwrap().velocity.length();
        let vel2 = state2.get_player(id2).unwrap().velocity.length();

        // Strong wave should apply more force
        // With 0.4 min and 1.8 max, ratio is 4.5x, so velocity difference should be > 2.5x
        assert!(vel2 > vel1 * 2.5);
    }

    #[test]
    fn test_wave_strength_surface_gravity_scaling() {
        // Test that surface gravity formula g = M/R² produces correct strength values
        // Reference: mass=10000, radius=20 -> g=25 -> strength=1.0

        // Reference well (should produce 1.0)
        let surface_gravity = 10000.0 / (20.0 * 20.0); // = 25
        let normalized = surface_gravity / BASE_SURFACE_GRAVITY;
        let strength = normalized.sqrt().clamp(WAVE_STRENGTH_MIN, WAVE_STRENGTH_MAX);
        assert!((strength - 1.0).abs() < 0.01, "Reference well should produce 1.0, got {}", strength);

        // Denser well: same mass, smaller radius -> higher gravity -> stronger wave
        let dense_g = 10000.0 / (15.0 * 15.0); // = 44.4
        let dense_normalized = dense_g / BASE_SURFACE_GRAVITY;
        let dense_strength = dense_normalized.sqrt().clamp(WAVE_STRENGTH_MIN, WAVE_STRENGTH_MAX);
        assert!(dense_strength > 1.0, "Dense well should be stronger than reference");

        // Diffuse well: same mass, larger radius -> lower gravity -> weaker wave
        let diffuse_g = 10000.0 / (30.0 * 30.0); // = 11.1
        let diffuse_normalized = diffuse_g / BASE_SURFACE_GRAVITY;
        let diffuse_strength = diffuse_normalized.sqrt().clamp(WAVE_STRENGTH_MIN, WAVE_STRENGTH_MAX);
        assert!(diffuse_strength < 1.0, "Diffuse well should be weaker than reference");
    }

    #[test]
    fn test_wave_strength_clamping() {
        // Test that extreme values get clamped properly

        // Very low surface gravity should clamp to MIN (0.4)
        // Need g/25 < 0.16 (since sqrt(0.16) = 0.4), so g < 4
        let low_g = 2.0;
        let low_strength = (low_g / BASE_SURFACE_GRAVITY).sqrt()
            .clamp(WAVE_STRENGTH_MIN, WAVE_STRENGTH_MAX);
        assert_eq!(low_strength, WAVE_STRENGTH_MIN);

        // Very high surface gravity should clamp to MAX (1.8)
        // Need g/25 > 3.24 (since sqrt(3.24) = 1.8), so g > 81
        let high_g = 100.0;
        let high_strength = (high_g / BASE_SURFACE_GRAVITY).sqrt()
            .clamp(WAVE_STRENGTH_MIN, WAVE_STRENGTH_MAX);
        assert_eq!(high_strength, WAVE_STRENGTH_MAX);
    }

    #[test]
    fn test_wave_strength_density_matters() {
        // Test that two wells with same mass but different radii have different strengths

        let mass = 10000.0;
        let radius_compact = 15.0;  // Compact, dense well
        let radius_diffuse = 30.0;  // Diffuse, spread out well

        let compact_g = mass / (radius_compact * radius_compact);
        let diffuse_g = mass / (radius_diffuse * radius_diffuse);

        let compact_strength = (compact_g / BASE_SURFACE_GRAVITY).sqrt()
            .clamp(WAVE_STRENGTH_MIN, WAVE_STRENGTH_MAX);
        let diffuse_strength = (diffuse_g / BASE_SURFACE_GRAVITY).sqrt()
            .clamp(WAVE_STRENGTH_MIN, WAVE_STRENGTH_MAX);

        // Compact well should produce about 2x stronger wave
        // (radius ratio 2:1 -> area ratio 4:1 -> gravity ratio 4:1 -> sqrt = 2:1)
        assert!(
            compact_strength > diffuse_strength * 1.5,
            "Compact well ({}) should be much stronger than diffuse ({})",
            compact_strength, diffuse_strength
        );
    }

    #[test]
    fn test_central_supermassive_never_explodes() {
        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Set central well to explode (using CENTRAL_WELL_ID)
        if let Some(central_well) = state.arena.gravity_wells.get_mut(&crate::game::state::CENTRAL_WELL_ID) {
            central_well.explosion_timer = 0.0;
            central_well.is_charging = true;
        }

        // Use high target_wells and large escape_radius to prevent removal
        let events = update_explosions(&mut state, &config, DT, 100, 10000.0);

        // Central well should NOT explode (skipped - has CENTRAL_WELL_ID = 0)
        assert!(!events.iter().any(|e| match e {
            GravityWaveEvent::WellExploded { well_id, .. } => *well_id == crate::game::state::CENTRAL_WELL_ID,
            _ => false,
        }));
    }

    #[test]
    fn test_wave_distance_decay() {
        let (mut state, player_id) = create_test_state();
        let config = GravityWaveConfig::default();

        // Position player far from wave center
        state.get_player_mut(player_id).unwrap().position = Vec2::new(1500.0, 0.0);

        // Add wave that's expanded far
        let mut wave = crate::game::state::GravityWave::new(Vec2::ZERO, 1.0);
        wave.radius = 1500.0 - config.wave_front_thickness * 0.3;
        state.gravity_waves.push(wave);

        update_waves(&mut state, &config, DT);

        // Force should be weaker due to distance decay
        let far_velocity = state.get_player(player_id).unwrap().velocity.length();

        // Compare with close position
        let (mut state2, player_id2) = create_test_state();
        state2.get_player_mut(player_id2).unwrap().position = Vec2::new(100.0, 0.0);
        let mut wave2 = crate::game::state::GravityWave::new(Vec2::ZERO, 1.0);
        wave2.radius = 100.0 - config.wave_front_thickness * 0.3;
        state2.gravity_waves.push(wave2);
        update_waves(&mut state2, &config, DT);
        let close_velocity = state2.get_player(player_id2).unwrap().velocity.length();

        // Close should have stronger impulse
        assert!(close_velocity > far_velocity);
    }

    // === MULTI-WELL GRAVITY TESTS ===

    #[test]
    fn test_multi_well_gravity_superposition() {
        // Two wells should have combined effect
        let well1 = GravityWell::new(1, Vec2::new(-100.0, 0.0), 10000.0, 20.0);
        let well2 = GravityWell::new(2, Vec2::new(100.0, 0.0), 10000.0, 20.0);
        let wells = vec![well1, well2];

        // At origin, forces should cancel out (equal wells on both sides)
        let gravity_at_origin = calculate_multi_well_gravity(Vec2::ZERO, wells.iter());
        assert!(gravity_at_origin.x.abs() < 1.0, "X force should mostly cancel");
        assert!(gravity_at_origin.y.abs() < 0.001, "Y force should be zero");
    }

    #[test]
    fn test_multi_well_gravity_asymmetric() {
        // Closer well should dominate
        let well1 = GravityWell::new(1, Vec2::new(-50.0, 0.0), 10000.0, 20.0);
        let well2 = GravityWell::new(2, Vec2::new(200.0, 0.0), 10000.0, 20.0);
        let wells = vec![well1, well2];

        // At origin, should be pulled more toward well1 (closer)
        let gravity = calculate_multi_well_gravity(Vec2::ZERO, wells.iter());
        assert!(gravity.x < 0.0, "Should be pulled toward closer well on left");
    }

    #[test]
    fn test_gravity_well_minimum_distance() {
        // Very close to well should return zero (safety)
        let well = GravityWell::new(1, Vec2::ZERO, 10000.0, 50.0);
        let wells = vec![well];

        // Inside 2x core radius should be zero
        let gravity = calculate_multi_well_gravity(Vec2::new(50.0, 0.0), wells.iter());
        assert_eq!(gravity, Vec2::ZERO);
    }

    #[test]
    fn test_gravity_deterministic() {
        let well = GravityWell::new(1, Vec2::new(100.0, 50.0), 8000.0, 30.0);
        let pos = Vec2::new(300.0, 200.0);

        let g1 = calculate_gravity_from_well(pos, &well);
        let g2 = calculate_gravity_from_well(pos, &well);

        assert_eq!(g1, g2, "Gravity should be deterministic");
    }

    #[test]
    fn test_well_removed_on_explosion_when_excess() {
        use crate::game::constants::arena::CORE_RADIUS;
        use crate::game::constants::physics::CENTRAL_MASS;

        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Add 5 orbital wells
        let mut exploding_well_id = None;
        for i in 1..=5 {
            let well_id = state.arena.alloc_well_id();
            let mut well = GravityWell::new(
                well_id,
                Vec2::new(500.0 * i as f32, 0.0),
                CENTRAL_MASS,
                CORE_RADIUS,
            );
            well.explosion_timer = if i == 3 { 0.0 } else { 30.0 };  // Well 3 explodes
            well.is_charging = i == 3;
            if i == 3 {
                exploding_well_id = Some(well_id);
            }
            state.arena.gravity_wells.insert(well_id, well);
        }
        let _ = exploding_well_id; // Silence unused warning

        let initial_count = state.arena.gravity_wells.len();
        assert_eq!(initial_count, 6);  // 1 central + 5 orbital

        // Update with low target (1 well) - should remove the exploding well
        // Use large escape_radius so wells are in bounds (testing target count logic)
        let events = update_explosions(&mut state, &config, DT, 1, 10000.0);

        // Should have explosion and destruction events
        assert!(events.iter().any(|e| matches!(e, GravityWaveEvent::WellExploded { .. })));
        assert!(events.iter().any(|e| matches!(e, GravityWaveEvent::WellDestroyed { .. })));

        // Well count should decrease
        assert_eq!(state.arena.gravity_wells.len(), initial_count - 1);
    }

    #[test]
    fn test_well_removed_when_at_target() {
        use crate::game::constants::arena::CORE_RADIUS;
        use crate::game::constants::physics::CENTRAL_MASS;

        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Add 2 orbital wells
        for i in 1..=2 {
            let well_id = state.arena.alloc_well_id();
            let mut well = GravityWell::new(
                well_id,
                Vec2::new(500.0 * i as f32, 0.0),
                CENTRAL_MASS,
                CORE_RADIUS,
            );
            well.explosion_timer = if i == 1 { 0.0 } else { 30.0 };
            well.is_charging = i == 1;
            state.arena.gravity_wells.insert(well_id, well);
        }

        let initial_count = state.arena.gravity_wells.len();

        // Update with target = 2 (at target)
        // Use large escape_radius so wells are in bounds (testing target count logic)
        let events = update_explosions(&mut state, &config, DT, 2, 10000.0);

        // Should explode AND destroy (wells are removed at target for respawn elsewhere)
        assert!(events.iter().any(|e| matches!(e, GravityWaveEvent::WellExploded { .. })));
        assert!(events.iter().any(|e| matches!(e, GravityWaveEvent::WellDestroyed { .. })));

        // Well count should decrease (scale_for_simulation will add new one at different location)
        assert_eq!(state.arena.gravity_wells.len(), initial_count - 1);
    }

    #[test]
    fn test_well_always_destroyed_on_explosion() {
        // Wells are ALWAYS destroyed when their timer expires
        // This keeps the arena dynamic - new wells spawn at different positions
        use crate::game::constants::arena::CORE_RADIUS;
        use crate::game::constants::physics::CENTRAL_MASS;

        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Add an orbital well
        let well_id = state.arena.alloc_well_id();
        let mut well = GravityWell::new(
            well_id,
            Vec2::new(500.0, 0.0),
            CENTRAL_MASS,
            CORE_RADIUS,
        );
        well.explosion_timer = 0.0; // Explode immediately
        well.is_charging = true;
        state.arena.gravity_wells.insert(well_id, well);

        let initial_count = state.arena.gravity_wells.len(); // 2 (central + 1 orbital)

        // Update - well should ALWAYS be destroyed regardless of target_wells
        let events = update_explosions(&mut state, &config, DT, 10, 10000.0);

        // Should explode AND destroy
        assert!(events.iter().any(|e| matches!(e, GravityWaveEvent::WellExploded { .. })));
        assert!(events.iter().any(|e| matches!(e, GravityWaveEvent::WellDestroyed { .. })),
            "Wells should always be destroyed on explosion");

        // Well count should decrease
        assert_eq!(state.arena.gravity_wells.len(), initial_count - 1);
    }

    #[test]
    fn test_explosion_creates_gravity_wave() {
        // Verify that explosion creates a gravity wave
        use crate::game::constants::arena::CORE_RADIUS;
        use crate::game::constants::physics::CENTRAL_MASS;

        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Add an orbital well
        let well_id = state.arena.alloc_well_id();
        let mut well = GravityWell::new(
            well_id,
            Vec2::new(500.0, 0.0),
            CENTRAL_MASS,
            CORE_RADIUS,
        );
        well.explosion_timer = 0.0;
        well.is_charging = true;
        state.arena.gravity_wells.insert(well_id, well);

        assert!(state.gravity_waves.is_empty());

        update_explosions(&mut state, &config, DT, 1, 10000.0);

        // Should have created a gravity wave
        assert_eq!(state.gravity_waves.len(), 1);
        assert_eq!(state.gravity_waves[0].position, Vec2::new(500.0, 0.0));
    }

    // === WELL POSITION CACHE TESTS ===

    #[test]
    fn test_well_position_cache_from_wells() {
        let well1 = GravityWell::new(1, Vec2::new(100.0, 200.0), 10000.0, 50.0);
        let well2 = GravityWell::new(2, Vec2::new(300.0, 400.0), 8000.0, 40.0);
        let wells = vec![well1, well2];

        let cache = WellPositionCache::from_wells(wells.iter());

        assert_eq!(cache.len(), 2);
        assert!(!cache.is_empty());
        assert_eq!(cache.positions_x[0], 100.0);
        assert_eq!(cache.positions_y[0], 200.0);
        assert_eq!(cache.masses[0], 10000.0);
    }

    #[test]
    fn test_well_position_cache_gravity() {
        let well1 = GravityWell::new(1, Vec2::new(200.0, 0.0), 10000.0, 20.0);
        let wells = vec![well1.clone()];

        let cache = WellPositionCache::from_wells(wells.iter());

        // Calculate gravity toward the well
        let gravity = cache.calculate_gravity(0.0, 0.0);

        // Gravity from cache should match direct calculation
        let expected = calculate_gravity_from_well(Vec2::ZERO, &well1);

        assert!((gravity.x - expected.x).abs() < 0.001);
        assert!((gravity.y - expected.y).abs() < 0.001);
    }

    #[test]
    fn test_well_position_cache_multiple_wells() {
        // Two wells on opposite sides should mostly cancel
        let well1 = GravityWell::new(1, Vec2::new(-100.0, 0.0), 10000.0, 20.0);
        let well2 = GravityWell::new(2, Vec2::new(100.0, 0.0), 10000.0, 20.0);
        let wells = vec![well1, well2];

        let cache = WellPositionCache::from_wells(wells.iter());
        let gravity = cache.calculate_gravity(0.0, 0.0);

        // Should mostly cancel out
        assert!(gravity.x.abs() < 1.0);
        assert!(gravity.y.abs() < 0.001);
    }

    #[test]
    fn test_well_position_cache_min_distance() {
        // Gravity should be zero when inside minimum distance
        let well = GravityWell::new(1, Vec2::ZERO, 10000.0, 50.0);
        let wells = vec![well];

        let cache = WellPositionCache::from_wells(wells.iter());

        // Inside 2x core radius (100) should be zero
        let gravity = cache.calculate_gravity(50.0, 0.0);
        assert_eq!(gravity.x, 0.0);
        assert_eq!(gravity.y, 0.0);
    }

    #[test]
    fn test_well_position_cache_empty() {
        let wells: Vec<GravityWell> = vec![];
        let cache = WellPositionCache::from_wells(wells.iter());

        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);

        // Should return zero gravity
        let gravity = cache.calculate_gravity(100.0, 100.0);
        assert_eq!(gravity, Vec2::ZERO);
    }

    // === GRAVITY MODE TESTS ===

    #[test]
    fn test_update_central_with_config_unlimited() {
        use crate::config::{GravityConfig, GravityRangeMode};

        let (mut state, player_id) = create_test_state();
        let config = GravityConfig {
            range_mode: GravityRangeMode::Unlimited,
            influence_radius: 5000.0,
        };

        let initial_velocity = state.get_player(player_id).unwrap().velocity;
        update_central_with_config(&mut state, &config, DT);

        // Velocity should have changed toward the wells
        assert!(state.get_player(player_id).unwrap().velocity != initial_velocity);
    }

    #[test]
    fn test_update_central_with_config_limited() {
        use crate::config::{GravityConfig, GravityRangeMode};

        let (mut state, player_id) = create_test_state();
        let config = GravityConfig {
            range_mode: GravityRangeMode::Limited,
            influence_radius: 5000.0,
        };

        let initial_velocity = state.get_player(player_id).unwrap().velocity;
        update_central_with_config(&mut state, &config, DT);

        // Velocity should have changed (player is within influence radius of wells)
        assert!(state.get_player(player_id).unwrap().velocity != initial_velocity);
    }

    // === INSIGNIFICANCE CULLING TESTS ===

    #[test]
    fn test_well_position_cache_max_distance_sq() {
        // Test that max_distance_sq is computed correctly based on mass
        let well = GravityWell::new(1, Vec2::ZERO, 10000.0, 20.0);
        let wells = vec![well];

        let cache = WellPositionCache::from_wells(wells.iter());

        // max_dist = mass * GRAVITY_INSIGNIFICANCE_FACTOR = 10000 * (0.5 / 0.1) = 50000
        // max_dist_sq = 50000^2 = 2.5e9
        let expected_max_dist = 10000.0 * GRAVITY_INSIGNIFICANCE_FACTOR;
        let expected_max_dist_sq = expected_max_dist * expected_max_dist;

        assert!((cache.max_distance_sq[0] - expected_max_dist_sq).abs() < 1.0);
    }

    #[test]
    fn test_insignificance_culling_skips_distant_wells() {
        // Create a well with small mass (small influence range)
        let well = GravityWell::new(1, Vec2::ZERO, 1000.0, 20.0);
        let wells = vec![well];

        let cache = WellPositionCache::from_wells(wells.iter());

        // max_dist = 1000 * 5 = 5000 for mass 1000
        // At distance > 5000, should get zero gravity
        let gravity_far = cache.calculate_gravity(10000.0, 0.0);
        assert_eq!(gravity_far, Vec2::ZERO, "Should cull distant well");

        // At distance < 5000, should get gravity
        let gravity_near = cache.calculate_gravity(1000.0, 0.0);
        assert!(gravity_near.x < 0.0, "Should have gravity toward well");
    }

    #[test]
    fn test_insignificance_culling_mass_dependent() {
        // Large mass well should affect entities farther away
        let well_large = GravityWell::new(1, Vec2::ZERO, 100000.0, 20.0);
        let well_small = GravityWell::new(2, Vec2::ZERO, 1000.0, 20.0);

        let cache_large = WellPositionCache::from_wells(std::iter::once(&well_large));
        let cache_small = WellPositionCache::from_wells(std::iter::once(&well_small));

        // Large mass: max_dist = 100000 * 5 = 500000
        // Small mass: max_dist = 1000 * 5 = 5000

        // At distance 10000, large well should affect, small should not
        let gravity_large = cache_large.calculate_gravity(10000.0, 0.0);
        let gravity_small = cache_small.calculate_gravity(10000.0, 0.0);

        assert!(gravity_large.x < 0.0, "Large mass well should affect at 10000 units");
        assert_eq!(gravity_small, Vec2::ZERO, "Small mass well should be culled at 10000 units");
    }

    #[test]
    fn test_insignificance_threshold_accuracy() {
        // Verify that wells just inside threshold produce > GRAVITY_MIN_ACCELERATION
        // and wells just outside produce < GRAVITY_MIN_ACCELERATION
        let mass = 10000.0;
        let core_radius = 20.0;
        let well = GravityWell::new(1, Vec2::ZERO, mass, core_radius);
        let wells = vec![well];

        let cache = WellPositionCache::from_wells(wells.iter());

        // max_dist where accel = 0.1: 10000 * 0.5 / 0.1 = 50000
        let threshold_dist = mass * GRAVITY_INSIGNIFICANCE_FACTOR;

        // Just inside threshold (should have gravity)
        let dist_inside = threshold_dist * 0.9;
        let gravity_inside = cache.calculate_gravity(dist_inside, 0.0);
        let accel_inside = gravity_inside.length();
        assert!(accel_inside > GRAVITY_MIN_ACCELERATION,
            "Acceleration inside threshold should exceed minimum: {}", accel_inside);

        // Just outside threshold (should be zero due to culling)
        let dist_outside = threshold_dist * 1.1;
        let gravity_outside = cache.calculate_gravity(dist_outside, 0.0);
        assert_eq!(gravity_outside, Vec2::ZERO,
            "Gravity outside threshold should be zero (culled)");
    }
}
