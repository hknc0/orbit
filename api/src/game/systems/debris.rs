//! Debris spawning system
//! Spawns collectible debris particles across the arena zones for players to collect
//! Also spawns debris in orbital rings around gravity wells for concentrated "feeding zones"

use rand::Rng;

use crate::config::DebrisSpawnConfig;
use crate::game::constants::arena::{CORE_RADIUS, INNER_RADIUS, MIDDLE_RADIUS, OUTER_RADIUS};
use crate::game::state::{DebrisSize, GameState, GravityWell};
use crate::util::vec2::Vec2;

// ============================================================================
// Debris System Constants
// ============================================================================

/// Offset from core radius for inner zone minimum spawn distance
const INNER_ZONE_CORE_OFFSET: f32 = 20.0;

/// Minimum orbit radius multiplier for debris spawning near gravity wells
/// Debris spawns at 2.5x to 5.0x the well's core radius
const WELL_SPAWN_MIN_RADIUS_MULTIPLIER: f32 = 2.5;

/// Maximum orbit radius multiplier for debris spawning near gravity wells
const WELL_SPAWN_MAX_RADIUS_MULTIPLIER: f32 = 5.0;

/// Orbital velocity boost factor for debris near gravity wells
/// Higher gravity requires faster orbital velocity
const WELL_ORBITAL_VELOCITY_BOOST: f32 = 1.5;

/// Probability threshold for large debris near gravity wells (15%)
const WELL_DEBRIS_LARGE_PROBABILITY: f32 = 0.15;

/// Probability threshold for medium debris near gravity wells (35%, cumulative 50%)
const WELL_DEBRIS_MEDIUM_PROBABILITY: f32 = 0.5;

/// Zone for debris spawning (subset of arena zones)
#[derive(Debug, Clone, Copy)]
pub enum DebrisZone {
    Inner,
    Middle,
    Outer,
}

impl DebrisZone {
    /// Get the spawn radius range for this zone
    fn radius_range(&self) -> (f32, f32) {
        match self {
            DebrisZone::Inner => (CORE_RADIUS + INNER_ZONE_CORE_OFFSET, INNER_RADIUS),
            DebrisZone::Middle => (INNER_RADIUS, MIDDLE_RADIUS),
            DebrisZone::Outer => (MIDDLE_RADIUS, OUTER_RADIUS),
        }
    }
}

/// State for tracking debris spawn accumulators
/// Uses fractional accumulation to handle sub-1 spawn rates per tick
#[derive(Debug, Clone, Default)]
pub struct DebrisSpawnState {
    // Accumulators for each zone/size combination
    inner_small: f32,
    inner_medium: f32,
    inner_large: f32,
    middle_small: f32,
    middle_medium: f32,
    middle_large: f32,
    outer_small: f32,
    outer_medium: f32,
    outer_large: f32,
    // Accumulator for gravity well spawning
    pub well_accumulator: f32,
}

impl DebrisSpawnState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Spawn initial debris across all zones
/// Called once when the game starts
pub fn spawn_initial(state: &mut GameState, config: &DebrisSpawnConfig) {
    if !config.enabled {
        return;
    }

    // Spawn in inner zone
    for _ in 0..config.initial_inner {
        spawn_in_zone(state, config, DebrisZone::Inner);
    }

    // Spawn in middle zone
    for _ in 0..config.initial_middle {
        spawn_in_zone(state, config, DebrisZone::Middle);
    }

    // Spawn in outer zone
    for _ in 0..config.initial_outer {
        spawn_in_zone(state, config, DebrisZone::Outer);
    }

    tracing::debug!(
        "Spawned initial debris: {} total",
        state.debris.len()
    );
}

/// Update debris spawning - accumulate spawn rates and spawn when ready
pub fn update(
    state: &mut GameState,
    config: &DebrisSpawnConfig,
    spawn_state: &mut DebrisSpawnState,
    dt: f32,
) {
    if !config.enabled {
        return;
    }

    // Check if we're at max count
    if state.debris.len() >= config.max_count {
        return;
    }

    // Accumulate spawn times
    spawn_state.inner_small += config.spawn_rate_inner_small * dt;
    spawn_state.inner_medium += config.spawn_rate_inner_medium * dt;
    spawn_state.inner_large += config.spawn_rate_inner_large * dt;
    spawn_state.middle_small += config.spawn_rate_middle_small * dt;
    spawn_state.middle_medium += config.spawn_rate_middle_medium * dt;
    spawn_state.middle_large += config.spawn_rate_middle_large * dt;
    spawn_state.outer_small += config.spawn_rate_outer_small * dt;
    spawn_state.outer_medium += config.spawn_rate_outer_medium * dt;
    spawn_state.outer_large += config.spawn_rate_outer_large * dt;

    // Spawn when accumulators exceed 1, respecting max count
    while spawn_state.inner_small >= 1.0 && state.debris.len() < config.max_count {
        spawn_debris(state, config, DebrisZone::Inner, DebrisSize::Small);
        spawn_state.inner_small -= 1.0;
    }

    while spawn_state.inner_medium >= 1.0 && state.debris.len() < config.max_count {
        spawn_debris(state, config, DebrisZone::Inner, DebrisSize::Medium);
        spawn_state.inner_medium -= 1.0;
    }

    while spawn_state.inner_large >= 1.0 && state.debris.len() < config.max_count {
        spawn_debris(state, config, DebrisZone::Inner, DebrisSize::Large);
        spawn_state.inner_large -= 1.0;
    }

    while spawn_state.middle_small >= 1.0 && state.debris.len() < config.max_count {
        spawn_debris(state, config, DebrisZone::Middle, DebrisSize::Small);
        spawn_state.middle_small -= 1.0;
    }

    while spawn_state.middle_medium >= 1.0 && state.debris.len() < config.max_count {
        spawn_debris(state, config, DebrisZone::Middle, DebrisSize::Medium);
        spawn_state.middle_medium -= 1.0;
    }

    while spawn_state.middle_large >= 1.0 && state.debris.len() < config.max_count {
        spawn_debris(state, config, DebrisZone::Middle, DebrisSize::Large);
        spawn_state.middle_large -= 1.0;
    }

    while spawn_state.outer_small >= 1.0 && state.debris.len() < config.max_count {
        spawn_debris(state, config, DebrisZone::Outer, DebrisSize::Small);
        spawn_state.outer_small -= 1.0;
    }

    while spawn_state.outer_medium >= 1.0 && state.debris.len() < config.max_count {
        spawn_debris(state, config, DebrisZone::Outer, DebrisSize::Medium);
        spawn_state.outer_medium -= 1.0;
    }

    while spawn_state.outer_large >= 1.0 && state.debris.len() < config.max_count {
        spawn_debris(state, config, DebrisZone::Outer, DebrisSize::Large);
        spawn_state.outer_large -= 1.0;
    }
}

/// Spawn debris in a zone with random size based on zone weights
fn spawn_in_zone(state: &mut GameState, config: &DebrisSpawnConfig, zone: DebrisZone) {
    if state.debris.len() >= config.max_count {
        return;
    }

    // Determine size based on zone-specific rates
    let (small_rate, medium_rate, large_rate) = match zone {
        DebrisZone::Inner => (
            config.spawn_rate_inner_small,
            config.spawn_rate_inner_medium,
            config.spawn_rate_inner_large,
        ),
        DebrisZone::Middle => (
            config.spawn_rate_middle_small,
            config.spawn_rate_middle_medium,
            config.spawn_rate_middle_large,
        ),
        DebrisZone::Outer => (
            config.spawn_rate_outer_small,
            config.spawn_rate_outer_medium,
            config.spawn_rate_outer_large,
        ),
    };

    let total_rate = small_rate + medium_rate + large_rate;
    if total_rate <= 0.0 {
        return;
    }

    let mut rng = rand::thread_rng();
    let roll = rng.gen::<f32>() * total_rate;

    let size = if roll < large_rate {
        DebrisSize::Large
    } else if roll < large_rate + medium_rate {
        DebrisSize::Medium
    } else {
        DebrisSize::Small
    };

    spawn_debris(state, config, zone, size);
}

/// Spawn a single debris particle at a random position in the zone
fn spawn_debris(
    state: &mut GameState,
    config: &DebrisSpawnConfig,
    zone: DebrisZone,
    size: DebrisSize,
) {
    let mut rng = rand::thread_rng();

    // Random position in zone
    let (min_radius, max_radius) = zone.radius_range();
    let angle = rng.gen_range(0.0..std::f32::consts::TAU);
    let radius = rng.gen_range(min_radius..max_radius);
    let position = Vec2::from_angle(angle) * radius;

    // Small orbital velocity (perpendicular to radius)
    let orbital_angle = angle + std::f32::consts::FRAC_PI_2;
    let speed = rng.gen_range(config.orbital_velocity_min..config.orbital_velocity_max);
    let velocity = Vec2::from_angle(orbital_angle) * speed;

    state.add_debris_with_lifetime(position, velocity, size, config.lifetime);
}

// === GRAVITY WELL SPAWNING ===

/// Spawn debris in orbital rings around gravity wells
/// Creates "feeding zones" near each non-central well
pub fn spawn_around_wells(state: &mut GameState, config: &DebrisSpawnConfig) {
    if !config.enabled {
        return;
    }

    // Collect non-central wells (filter by ID, not index - HashMap has no guaranteed order)
    let wells: Vec<GravityWell> = state.arena.gravity_wells
        .values()
        .filter(|w| w.id != crate::game::state::CENTRAL_WELL_ID)
        .cloned()
        .collect();

    for well in wells {
        // Spawn debris per well (capped by max_count)
        let debris_per_well = config.well_debris_count;
        for _ in 0..debris_per_well {
            if state.debris.len() >= config.max_count {
                return;
            }
            spawn_near_well(state, config, &well);
        }
    }
}

/// Spawn a single debris particle in an orbital ring around a gravity well
fn spawn_near_well(state: &mut GameState, config: &DebrisSpawnConfig, well: &GravityWell) {
    let mut rng = rand::thread_rng();

    // Orbital ring: between 2.5x and 5x the well's core radius (death zone)
    // This creates a "safe" feeding zone just outside the danger zone
    let min_radius = well.core_radius * WELL_SPAWN_MIN_RADIUS_MULTIPLIER;
    let max_radius = well.core_radius * WELL_SPAWN_MAX_RADIUS_MULTIPLIER;

    let angle = rng.gen_range(0.0..std::f32::consts::TAU);
    let orbit_radius = rng.gen_range(min_radius..max_radius);

    // Position relative to well center
    let offset = Vec2::from_angle(angle) * orbit_radius;
    let position = well.position + offset;

    // Orbital velocity around the well (perpendicular to radius from well)
    // Slightly faster than zone debris to orbit the well
    let orbital_angle = angle + std::f32::consts::FRAC_PI_2;
    let base_speed = rng.gen_range(config.orbital_velocity_min..config.orbital_velocity_max);
    // Boost speed near wells (more gravity = faster orbit needed)
    let speed = base_speed * WELL_ORBITAL_VELOCITY_BOOST;
    let velocity = Vec2::from_angle(orbital_angle) * speed;

    // Well debris is more likely to be medium/large (richer feeding zone)
    let size = {
        let roll = rng.gen::<f32>();
        if roll < WELL_DEBRIS_LARGE_PROBABILITY {
            DebrisSize::Large
        } else if roll < WELL_DEBRIS_MEDIUM_PROBABILITY {
            DebrisSize::Medium
        } else {
            DebrisSize::Small
        }
    };

    state.add_debris_with_lifetime(position, velocity, size, config.lifetime);
}

/// Update: spawn debris around wells over time (called each tick)
pub fn update_well_spawning(
    state: &mut GameState,
    config: &DebrisSpawnConfig,
    accumulator: &mut f32,
    dt: f32,
) {
    if !config.enabled || config.well_spawn_rate <= 0.0 {
        return;
    }

    // Skip central well (index 0)
    let well_count = state.arena.gravity_wells.len().saturating_sub(1);
    if well_count == 0 {
        return;
    }

    // Accumulate spawn time
    *accumulator += config.well_spawn_rate * dt;

    // Spawn when accumulator exceeds 1
    while *accumulator >= 1.0 && state.debris.len() < config.max_count {
        // Pick a random non-central well
        let mut rng = rand::thread_rng();
        let orbital_wells: Vec<_> = state.arena.gravity_wells
            .values()
            .filter(|w| w.id != crate::game::state::CENTRAL_WELL_ID)
            .collect();

        if orbital_wells.is_empty() {
            break;
        }

        let well = orbital_wells[rng.gen_range(0..orbital_wells.len())].clone();

        spawn_near_well(state, config, &well);
        *accumulator -= 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> DebrisSpawnConfig {
        DebrisSpawnConfig::default()
    }

    #[test]
    fn test_spawn_initial() {
        let mut state = GameState::new();
        let config = test_config();

        spawn_initial(&mut state, &config);

        // Should spawn initial_inner + initial_middle + initial_outer debris
        let expected = config.initial_inner + config.initial_middle + config.initial_outer;
        assert_eq!(state.debris.len(), expected);
    }

    #[test]
    fn test_spawn_respects_max_count() {
        let mut state = GameState::new();
        let mut config = test_config();
        config.max_count = 50;
        config.initial_inner = 100; // More than max

        spawn_initial(&mut state, &config);

        // Should be capped at max_count
        assert!(state.debris.len() <= config.max_count);
    }

    #[test]
    fn test_spawn_disabled() {
        let mut state = GameState::new();
        let mut config = test_config();
        config.enabled = false;

        spawn_initial(&mut state, &config);

        assert_eq!(state.debris.len(), 0);
    }

    #[test]
    fn test_update_spawns_over_time() {
        let mut state = GameState::new();
        let mut config = test_config();
        config.initial_inner = 0;
        config.initial_middle = 0;
        config.initial_outer = 0;
        config.spawn_rate_inner_small = 10.0; // 10 per second

        let mut spawn_state = DebrisSpawnState::new();

        // Simulate 1 second of updates at 30 FPS
        for _ in 0..30 {
            update(&mut state, &config, &mut spawn_state, 1.0 / 30.0);
        }

        // Should have spawned approximately 10 debris
        assert!(state.debris.len() >= 9 && state.debris.len() <= 11);
    }

    #[test]
    fn test_debris_positions_in_zone() {
        let mut state = GameState::new();
        let config = test_config();

        spawn_initial(&mut state, &config);

        for debris in &state.debris {
            let dist = debris.position.length();
            // Should be in one of the valid zones (not in core, not outside arena)
            assert!(dist > CORE_RADIUS);
            assert!(dist < OUTER_RADIUS);
        }
    }

    #[test]
    fn test_debris_has_orbital_velocity() {
        let mut state = GameState::new();
        let config = test_config();

        spawn_initial(&mut state, &config);

        for debris in &state.debris {
            let speed = debris.velocity.length();
            // Should have some velocity
            assert!(speed >= config.orbital_velocity_min);
            assert!(speed <= config.orbital_velocity_max);
        }
    }

    // === EDGE CASE TESTS ===

    #[test]
    fn test_spawn_state_accumulator_works() {
        let mut state = GameState::new();
        let mut config = test_config();
        config.initial_inner = 0;
        config.initial_middle = 0;
        config.initial_outer = 0;
        config.spawn_rate_inner_small = 1.0; // 1 per second
        config.spawn_rate_inner_medium = 0.0;
        config.spawn_rate_inner_large = 0.0;
        config.spawn_rate_middle_small = 0.0;
        config.spawn_rate_middle_medium = 0.0;
        config.spawn_rate_middle_large = 0.0;
        config.spawn_rate_outer_small = 0.0;
        config.spawn_rate_outer_medium = 0.0;
        config.spawn_rate_outer_large = 0.0;

        let mut spawn_state = DebrisSpawnState::new();

        // Run for exactly 1 second
        for _ in 0..30 {
            update(&mut state, &config, &mut spawn_state, 1.0 / 30.0);
        }

        // Should have spawned exactly 1 debris (1.0 per second)
        assert_eq!(state.debris.len(), 1);
    }

    #[test]
    fn test_fractional_accumulation() {
        let mut state = GameState::new();
        let mut config = test_config();
        config.initial_inner = 0;
        config.initial_middle = 0;
        config.initial_outer = 0;
        // Very slow spawn rate: 0.1 per second = 1 per 10 seconds
        config.spawn_rate_inner_small = 0.1;
        config.spawn_rate_inner_medium = 0.0;
        config.spawn_rate_inner_large = 0.0;
        config.spawn_rate_middle_small = 0.0;
        config.spawn_rate_middle_medium = 0.0;
        config.spawn_rate_middle_large = 0.0;
        config.spawn_rate_outer_small = 0.0;
        config.spawn_rate_outer_medium = 0.0;
        config.spawn_rate_outer_large = 0.0;

        let mut spawn_state = DebrisSpawnState::new();

        // Run for 5 seconds - should NOT spawn anything yet
        for _ in 0..150 {
            update(&mut state, &config, &mut spawn_state, 1.0 / 30.0);
        }
        assert_eq!(state.debris.len(), 0, "Should not spawn before accumulator reaches 1.0");

        // Run for another 5+ seconds - should spawn 1
        for _ in 0..180 {
            update(&mut state, &config, &mut spawn_state, 1.0 / 30.0);
        }
        assert_eq!(state.debris.len(), 1, "Should spawn after accumulator reaches 1.0");
    }

    #[test]
    fn test_spawn_in_correct_zone_inner() {
        let mut state = GameState::new();
        let mut config = test_config();
        config.initial_inner = 10;
        config.initial_middle = 0;
        config.initial_outer = 0;

        spawn_initial(&mut state, &config);

        for debris in &state.debris {
            let dist = debris.position.length();
            let (min_r, max_r) = DebrisZone::Inner.radius_range();
            assert!(dist >= min_r, "Debris at {} should be >= {}", dist, min_r);
            assert!(dist <= max_r, "Debris at {} should be <= {}", dist, max_r);
        }
    }

    #[test]
    fn test_spawn_in_correct_zone_middle() {
        let mut state = GameState::new();
        let mut config = test_config();
        config.initial_inner = 0;
        config.initial_middle = 10;
        config.initial_outer = 0;

        spawn_initial(&mut state, &config);

        for debris in &state.debris {
            let dist = debris.position.length();
            let (min_r, max_r) = DebrisZone::Middle.radius_range();
            assert!(dist >= min_r, "Debris at {} should be >= {}", dist, min_r);
            assert!(dist <= max_r, "Debris at {} should be <= {}", dist, max_r);
        }
    }

    #[test]
    fn test_spawn_in_correct_zone_outer() {
        let mut state = GameState::new();
        let mut config = test_config();
        config.initial_inner = 0;
        config.initial_middle = 0;
        config.initial_outer = 10;

        spawn_initial(&mut state, &config);

        for debris in &state.debris {
            let dist = debris.position.length();
            let (min_r, max_r) = DebrisZone::Outer.radius_range();
            assert!(dist >= min_r, "Debris at {} should be >= {}", dist, min_r);
            assert!(dist <= max_r, "Debris at {} should be <= {}", dist, max_r);
        }
    }

    #[test]
    fn test_debris_lifetime_set_from_config() {
        let mut state = GameState::new();
        let mut config = test_config();
        config.lifetime = 45.0; // Custom lifetime
        config.initial_inner = 5;
        config.initial_middle = 0;
        config.initial_outer = 0;

        spawn_initial(&mut state, &config);

        for debris in &state.debris {
            assert!((debris.lifetime - 45.0).abs() < 0.001);
        }
    }

    #[test]
    fn test_velocity_direction_is_orbital() {
        let mut state = GameState::new();
        let config = test_config();

        spawn_initial(&mut state, &config);

        for debris in &state.debris {
            // Velocity should be roughly perpendicular to position (orbital)
            let radial = debris.position.normalize();
            let tangent = debris.velocity.normalize();

            // Dot product should be close to 0 (perpendicular)
            let dot = radial.dot(tangent).abs();
            assert!(dot < 0.2, "Velocity should be roughly perpendicular to radius, got dot={}", dot);
        }
    }

    #[test]
    fn test_zone_weights_determine_size_distribution() {
        let mut state = GameState::new();
        let mut config = test_config();
        // Set up so only large debris spawns in inner zone
        config.spawn_rate_inner_small = 0.0;
        config.spawn_rate_inner_medium = 0.0;
        config.spawn_rate_inner_large = 10.0;
        config.initial_inner = 20;
        config.initial_middle = 0;
        config.initial_outer = 0;

        spawn_initial(&mut state, &config);

        // All should be large
        for debris in &state.debris {
            assert_eq!(debris.size, DebrisSize::Large);
        }
    }

    #[test]
    fn test_zero_total_rate_no_spawn() {
        let mut state = GameState::new();
        let mut config = test_config();
        // All rates zero
        config.spawn_rate_inner_small = 0.0;
        config.spawn_rate_inner_medium = 0.0;
        config.spawn_rate_inner_large = 0.0;
        config.initial_inner = 10;
        config.initial_middle = 0;
        config.initial_outer = 0;

        spawn_initial(&mut state, &config);

        // Should not spawn anything when total rate is 0
        assert_eq!(state.debris.len(), 0);
    }

    #[test]
    fn test_max_count_enforced_during_update() {
        let mut state = GameState::new();
        let mut config = test_config();
        config.max_count = 5;
        config.initial_inner = 0;
        config.initial_middle = 0;
        config.initial_outer = 0;
        config.spawn_rate_inner_small = 100.0; // Very fast

        let mut spawn_state = DebrisSpawnState::new();

        // Run many updates
        for _ in 0..300 {
            update(&mut state, &config, &mut spawn_state, 1.0 / 30.0);
        }

        assert!(state.debris.len() <= config.max_count);
    }

    #[test]
    fn test_debris_spawn_state_new() {
        let state = DebrisSpawnState::new();
        assert!((state.inner_small - 0.0).abs() < 0.001);
        assert!((state.inner_medium - 0.0).abs() < 0.001);
        assert!((state.inner_large - 0.0).abs() < 0.001);
        assert!((state.middle_small - 0.0).abs() < 0.001);
        assert!((state.middle_medium - 0.0).abs() < 0.001);
        assert!((state.middle_large - 0.0).abs() < 0.001);
        assert!((state.outer_small - 0.0).abs() < 0.001);
        assert!((state.outer_medium - 0.0).abs() < 0.001);
        assert!((state.outer_large - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_all_zones_spawn_simultaneously() {
        let mut state = GameState::new();
        let mut config = test_config();
        config.initial_inner = 0;
        config.initial_middle = 0;
        config.initial_outer = 0;
        config.spawn_rate_inner_small = 30.0; // 1 per tick
        config.spawn_rate_middle_small = 30.0;
        config.spawn_rate_outer_small = 30.0;
        // Zero other rates to ensure only small debris
        config.spawn_rate_inner_medium = 0.0;
        config.spawn_rate_inner_large = 0.0;
        config.spawn_rate_middle_medium = 0.0;
        config.spawn_rate_middle_large = 0.0;
        config.spawn_rate_outer_medium = 0.0;
        config.spawn_rate_outer_large = 0.0;

        let mut spawn_state = DebrisSpawnState::new();
        update(&mut state, &config, &mut spawn_state, 1.0 / 30.0);

        // Should have spawned at least 1 from each zone (3 total)
        assert!(state.debris.len() >= 3);
    }

    #[test]
    fn test_debris_zone_radius_ordering() {
        let inner_range = DebrisZone::Inner.radius_range();
        let middle_range = DebrisZone::Middle.radius_range();
        let outer_range = DebrisZone::Outer.radius_range();

        // Inner zone ends where middle begins
        assert!((inner_range.1 - middle_range.0).abs() < 1.0);
        // Middle zone ends where outer begins
        assert!((middle_range.1 - outer_range.0).abs() < 1.0);
        // Outer extends beyond middle
        assert!(outer_range.1 > middle_range.1);
    }
}
