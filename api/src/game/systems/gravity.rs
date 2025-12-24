//! Gravity system for orbital mechanics
//!
//! Applies gravitational forces from gravity wells to all entities.

#![allow(dead_code)] // Physics utilities for orbital calculations

use rayon::prelude::*;

use crate::game::constants::physics::{CENTRAL_MASS, G};
use crate::game::state::{GameState, GravityWell};
use crate::util::vec2::Vec2;

/// Apply gravity from all gravity wells to all entities
/// Uses rayon for parallel iteration over players, projectiles, and debris
pub fn update_central(state: &mut GameState, dt: f32) {
    // Clone wells to avoid borrow issues (shared across all parallel iterations)
    let wells = state.arena.gravity_wells.clone();

    // Apply gravity to players in parallel
    state.players.par_values_mut().for_each(|player| {
        if !player.alive {
            return;
        }

        let gravity = calculate_multi_well_gravity(player.position, &wells);
        player.velocity += gravity * dt;
    });

    // Apply gravity to projectiles in parallel
    state.projectiles.par_iter_mut().for_each(|projectile| {
        let gravity = calculate_multi_well_gravity(projectile.position, &wells);
        projectile.velocity += gravity * dt;
    });

    // Apply gravity to debris in parallel
    state.debris.par_iter_mut().for_each(|debris| {
        let gravity = calculate_multi_well_gravity(debris.position, &wells);
        debris.velocity += gravity * dt;
    });
}

/// Calculate gravitational acceleration from multiple gravity wells
pub fn calculate_multi_well_gravity(position: Vec2, wells: &[GravityWell]) -> Vec2 {
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
    let min_distance_sq = (well.core_radius * 2.0).powi(2); // 2x core radius minimum
    if distance_sq <= min_distance_sq {
        return Vec2::ZERO;
    }

    let distance = distance_sq.sqrt();

    // Direction toward well
    let direction = delta * (1.0 / distance);

    // Gravitational acceleration with 1/r falloff (not 1/r²)
    // This gives a more noticeable pull at gameplay distances
    // Scaled so at 300 units with mass 10000: 0.5 * 10000 / 300 ≈ 16.7 units/tick²
    let gravity_scale = 0.5; // Tuned for gameplay feel
    let acceleration = gravity_scale * well.mass / distance;

    // Clamp to prevent extreme accelerations near core
    let clamped_accel = acceleration.min(100.0);

    direction * clamped_accel
}

/// Legacy function for backward compatibility - calculates gravity toward origin
pub fn calculate_central_gravity(position: Vec2, _mass: f32) -> Vec2 {
    let distance_sq = position.length_sq();

    // Prevent division by zero and extreme forces at center
    if distance_sq < 100.0 {
        return Vec2::ZERO;
    }

    let distance = distance_sq.sqrt();

    // Direction toward center (use already-computed distance to avoid recomputing)
    let direction = -position * (1.0 / distance);

    // Gravitational acceleration magnitude: G * M / r^2
    let acceleration = G * CENTRAL_MASS / distance_sq;

    // Clamp to prevent extreme accelerations
    let clamped_accel = acceleration.min(500.0);

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
                if distance_sq < 100.0 || distance_sq > 1_000_000.0 {
                    continue;
                }

                let distance = distance_sq.sqrt();
                let direction = delta * (1.0 / distance);

                // Gravitational force: F = G * m1 * m2 / r^2
                // Scale down inter-entity gravity to be subtle
                let force_magnitude = G * mass_i * mass_j / distance_sq * 0.01;

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
    let safe_radius = radius.max(10.0);
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
    if radius < 10.0 {
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
    WellCharging { well_index: u8, position: Vec2 },
    /// Well exploded, wave created
    WellExploded { well_index: u8, position: Vec2, strength: f32 },
    /// Well was destroyed (removed from arena after explosion)
    WellDestroyed { well_index: u8, position: Vec2 },
}

/// Update explosion timers and create waves when wells explode
/// Returns events for charging and explosions
/// Config controls all timing and force parameters
/// `target_wells` is the desired number of orbital wells - excess wells are removed on explosion
pub fn update_explosions(
    state: &mut GameState,
    config: &GravityWaveConfig,
    dt: f32,
    target_wells: usize,
) -> Vec<GravityWaveEvent> {
    use crate::game::state::GravityWave;

    let mut events = Vec::new();
    let mut new_waves = Vec::new();
    let mut wells_to_remove: Vec<usize> = Vec::new();

    // Current orbital well count (excluding central supermassive at index 0)
    let current_orbital_wells = state.arena.gravity_wells.len().saturating_sub(1);

    // Skip index 0 (central supermassive black hole - too stable to explode)
    for (i, well) in state.arena.gravity_wells.iter_mut().enumerate().skip(1) {
        well.explosion_timer -= dt;

        // Check if entering charge phase (warning)
        if !well.is_charging && well.explosion_timer <= config.charge_duration && well.explosion_timer > 0.0 {
            well.is_charging = true;
            events.push(GravityWaveEvent::WellCharging {
                well_index: i as u8,
                position: well.position,
            });
        }

        // Check for explosion
        if well.explosion_timer <= 0.0 {
            // Calculate strength based on well mass (normalized)
            let strength = (well.mass / 10000.0).clamp(0.3, 1.0);

            events.push(GravityWaveEvent::WellExploded {
                well_index: i as u8,
                position: well.position,
                strength,
            });

            // Create the wave
            new_waves.push(GravityWave::new(well.position, strength));

            // Check if we have excess wells - if so, mark for removal instead of reset
            let excess_wells = current_orbital_wells.saturating_sub(wells_to_remove.len());
            if excess_wells > target_wells {
                // Remove this well (arena has too many for current player count)
                wells_to_remove.push(i);
                events.push(GravityWaveEvent::WellDestroyed {
                    well_index: i as u8,
                    position: well.position,
                });
            } else {
                // Reset timer for next explosion using config
                well.explosion_timer = config.random_explosion_delay();
                well.is_charging = false;
            }
        }
    }

    // Remove wells in reverse order to preserve indices
    for &idx in wells_to_remove.iter().rev() {
        state.arena.gravity_wells.remove(idx);
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

            if dist >= wave_inner && dist <= wave_outer && dist > 1.0 {
                let direction = delta.normalize();
                let distance_decay = 1.0 - (wave.radius / config.wave_max_radius).min(1.0);
                let force = config.wave_base_impulse * wave.strength * distance_decay;

                player.velocity += direction * force;
                wave.hit_players.push(player.id);
            }
        }

        // Apply impulse to projectiles in the wave front (50% force - lighter)
        for projectile in state.projectiles.iter_mut() {
            let delta = projectile.position - wave.position;
            let dist = delta.length();

            if dist >= wave_inner && dist <= wave_outer && dist > 1.0 {
                let direction = delta.normalize();
                let distance_decay = 1.0 - (wave.radius / config.wave_max_radius).min(1.0);
                let force = config.wave_base_impulse * wave.strength * distance_decay * 0.5;

                projectile.velocity += direction * force;
            }
        }

        // Apply impulse to debris in the wave front (70% force - scatter nicely)
        for debris in state.debris.iter_mut() {
            let delta = debris.position - wave.position;
            let dist = delta.length();

            if dist >= wave_inner && dist <= wave_outer && dist > 1.0 {
                let direction = delta.normalize();
                let distance_decay = 1.0 - (wave.radius / config.wave_max_radius).min(1.0);
                let force = config.wave_base_impulse * wave.strength * distance_decay * 0.7;

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

        // Set a well to explode soon
        if state.arena.gravity_wells.len() > 1 {
            state.arena.gravity_wells[1].explosion_timer = 10.0;
        }

        let initial_timer = state.arena.gravity_wells.get(1).map(|w| w.explosion_timer).unwrap_or(0.0);
        // Use high target_wells to prevent removal
        update_explosions(&mut state, &config, DT, 100);

        if state.arena.gravity_wells.len() > 1 {
            assert!(state.arena.gravity_wells[1].explosion_timer < initial_timer);
        }
    }

    #[test]
    fn test_wave_explosion_creates_wave() {
        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Set a well to explode immediately
        if state.arena.gravity_wells.len() > 1 {
            state.arena.gravity_wells[1].explosion_timer = 0.0;
            state.arena.gravity_wells[1].is_charging = true;

            // Use high target_wells to prevent removal
            let events = update_explosions(&mut state, &config, DT, 100);

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

        // Set well to enter charging phase
        if state.arena.gravity_wells.len() > 1 {
            state.arena.gravity_wells[1].explosion_timer = config.charge_duration - 0.1;
            state.arena.gravity_wells[1].is_charging = false;

            // Use high target_wells to prevent removal
            let events = update_explosions(&mut state, &config, DT, 100);

            // Should create a charging event
            assert!(events.iter().any(|e| matches!(e, GravityWaveEvent::WellCharging { .. })));
            assert!(state.arena.gravity_wells[1].is_charging);
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

        // Add weak wave to state1
        let mut weak_wave = crate::game::state::GravityWave::new(Vec2::ZERO, 0.3);
        weak_wave.radius = 50.0 - config.wave_front_thickness * 0.3;
        state1.gravity_waves.push(weak_wave);

        // Add strong wave to state2
        let mut strong_wave = crate::game::state::GravityWave::new(Vec2::ZERO, 1.0);
        strong_wave.radius = 50.0 - config.wave_front_thickness * 0.3;
        state2.gravity_waves.push(strong_wave);

        update_waves(&mut state1, &config, DT);
        update_waves(&mut state2, &config, DT);

        let vel1 = state1.get_player(id1).unwrap().velocity.length();
        let vel2 = state2.get_player(id2).unwrap().velocity.length();

        // Strong wave should apply more force
        assert!(vel2 > vel1 * 2.5);
    }

    #[test]
    fn test_central_supermassive_never_explodes() {
        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Set central well (index 0) to explode
        if !state.arena.gravity_wells.is_empty() {
            state.arena.gravity_wells[0].explosion_timer = 0.0;
            state.arena.gravity_wells[0].is_charging = true;
        }

        // Use high target_wells to prevent removal
        let events = update_explosions(&mut state, &config, DT, 100);

        // Central well should NOT explode (skipped at index 0)
        assert!(!events.iter().any(|e| match e {
            GravityWaveEvent::WellExploded { well_index, .. } => *well_index == 0,
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
        let well1 = GravityWell::new(Vec2::new(-100.0, 0.0), 10000.0, 20.0);
        let well2 = GravityWell::new(Vec2::new(100.0, 0.0), 10000.0, 20.0);
        let wells = vec![well1, well2];

        // At origin, forces should cancel out (equal wells on both sides)
        let gravity_at_origin = calculate_multi_well_gravity(Vec2::ZERO, &wells);
        assert!(gravity_at_origin.x.abs() < 1.0, "X force should mostly cancel");
        assert!(gravity_at_origin.y.abs() < 0.001, "Y force should be zero");
    }

    #[test]
    fn test_multi_well_gravity_asymmetric() {
        // Closer well should dominate
        let well1 = GravityWell::new(Vec2::new(-50.0, 0.0), 10000.0, 20.0);
        let well2 = GravityWell::new(Vec2::new(200.0, 0.0), 10000.0, 20.0);
        let wells = vec![well1, well2];

        // At origin, should be pulled more toward well1 (closer)
        let gravity = calculate_multi_well_gravity(Vec2::ZERO, &wells);
        assert!(gravity.x < 0.0, "Should be pulled toward closer well on left");
    }

    #[test]
    fn test_gravity_well_minimum_distance() {
        // Very close to well should return zero (safety)
        let well = GravityWell::new(Vec2::ZERO, 10000.0, 50.0);
        let wells = vec![well];

        // Inside 2x core radius should be zero
        let gravity = calculate_multi_well_gravity(Vec2::new(50.0, 0.0), &wells);
        assert_eq!(gravity, Vec2::ZERO);
    }

    #[test]
    fn test_gravity_deterministic() {
        let well = GravityWell::new(Vec2::new(100.0, 50.0), 8000.0, 30.0);
        let wells = vec![well.clone()];
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
        for i in 1..=5 {
            let mut well = GravityWell::new(
                Vec2::new(500.0 * i as f32, 0.0),
                CENTRAL_MASS,
                CORE_RADIUS,
            );
            well.explosion_timer = if i == 3 { 0.0 } else { 30.0 };  // Well 3 explodes
            well.is_charging = i == 3;
            state.arena.gravity_wells.push(well);
        }

        let initial_count = state.arena.gravity_wells.len();
        assert_eq!(initial_count, 6);  // 1 central + 5 orbital

        // Update with low target (1 well) - should remove the exploding well
        let events = update_explosions(&mut state, &config, DT, 1);

        // Should have explosion and destruction events
        assert!(events.iter().any(|e| matches!(e, GravityWaveEvent::WellExploded { .. })));
        assert!(events.iter().any(|e| matches!(e, GravityWaveEvent::WellDestroyed { .. })));

        // Well count should decrease
        assert_eq!(state.arena.gravity_wells.len(), initial_count - 1);
    }

    #[test]
    fn test_well_not_removed_when_at_target() {
        use crate::game::constants::arena::CORE_RADIUS;
        use crate::game::constants::physics::CENTRAL_MASS;

        let mut state = GameState::new();
        let config = GravityWaveConfig::default();

        // Add 2 orbital wells
        for i in 1..=2 {
            let mut well = GravityWell::new(
                Vec2::new(500.0 * i as f32, 0.0),
                CENTRAL_MASS,
                CORE_RADIUS,
            );
            well.explosion_timer = if i == 1 { 0.0 } else { 30.0 };
            well.is_charging = i == 1;
            state.arena.gravity_wells.push(well);
        }

        let initial_count = state.arena.gravity_wells.len();

        // Update with target = 2 (no excess)
        let events = update_explosions(&mut state, &config, DT, 2);

        // Should explode but NOT destroy (no excess)
        assert!(events.iter().any(|e| matches!(e, GravityWaveEvent::WellExploded { .. })));
        assert!(!events.iter().any(|e| matches!(e, GravityWaveEvent::WellDestroyed { .. })));

        // Well count should stay the same (timer reset instead of removal)
        assert_eq!(state.arena.gravity_wells.len(), initial_count);
    }
}
