//! Debris spawning system
//! Spawns collectible debris particles across the arena zones for players to collect

use rand::Rng;

use crate::config::DebrisSpawnConfig;
use crate::game::constants::arena::{CORE_RADIUS, INNER_RADIUS, MIDDLE_RADIUS, OUTER_RADIUS};
use crate::game::state::{DebrisSize, GameState};
use crate::util::vec2::Vec2;

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
            DebrisZone::Inner => (CORE_RADIUS + 20.0, INNER_RADIUS),
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
}
