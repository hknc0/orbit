use rayon::prelude::*;

use crate::game::constants::physics::{DRAG, MAX_VELOCITY};
use crate::game::state::GameState;
use crate::net::protocol::PlayerInput;
use crate::util::vec2::Vec2;

// ============================================================================
// Physics System Constants
// ============================================================================

/// Multiplier for projectile cleanup boundary (1.5x escape radius)
const PROJECTILE_BOUNDARY_MULTIPLIER: f32 = 1.5;

/// Multiplier for debris cleanup boundary (1.2x escape radius)
const DEBRIS_BOUNDARY_MULTIPLIER: f32 = 1.2;

/// Minimum input magnitude squared for thrust to be applied
/// Prevents tiny inputs from causing thrust (deadzone)
const THRUST_INPUT_THRESHOLD_SQ: f32 = 0.01;

/// Minimum input magnitude squared for aim to update rotation
const AIM_INPUT_THRESHOLD_SQ: f32 = 0.01;

/// Kinetic energy formula coefficient (1/2 * m * v²)
const KINETIC_ENERGY_COEFFICIENT: f32 = 0.5;

/// Update physics for all entities
/// CRITICAL: Uses exponential drag (velocity *= 1 - DRAG), NOT linear drag
/// Uses rayon for parallel iteration over players, projectiles, and debris
pub fn update(state: &mut GameState, dt: f32) {
    let drag_factor = 1.0 - DRAG;

    // Update players in parallel
    state.players.par_values_mut().for_each(|player| {
        if !player.alive {
            return;
        }

        // Apply exponential drag
        player.velocity *= drag_factor;

        // Clamp velocity to maximum
        player.velocity = player.velocity.clamp_length(MAX_VELOCITY);

        // Integrate position
        player.position += player.velocity * dt;

        // Update spawn protection timer
        if player.spawn_protection > 0.0 {
            player.spawn_protection = (player.spawn_protection - dt).max(0.0);
        }
    });

    // Update projectiles in parallel
    state.projectiles.par_iter_mut().for_each(|projectile| {
        // Apply drag
        projectile.velocity *= drag_factor;

        // Integrate position
        projectile.position += projectile.velocity * dt;

        // Decrease lifetime
        projectile.lifetime -= dt;
    });

    // Remove expired or out-of-bounds projectiles
    let escape_radius = state.arena.escape_radius;
    state.projectiles.retain(|p| p.lifetime > 0.0 && p.position.length() < escape_radius * PROJECTILE_BOUNDARY_MULTIPLIER);

    // Update debris in parallel (includes lifetime decay)
    state.debris.par_iter_mut().for_each(|debris| {
        debris.velocity *= drag_factor;
        debris.position += debris.velocity * dt;
        debris.lifetime -= dt;
    });

    // Remove expired or out-of-bounds debris
    state.debris.retain(|d| d.lifetime > 0.0 && d.position.length() < escape_radius * DEBRIS_BOUNDARY_MULTIPLIER);
}

/// Apply thrust from player input
pub fn apply_thrust(
    state: &mut GameState,
    player_id: uuid::Uuid,
    input: &PlayerInput,
    dt: f32,
) -> bool {
    use crate::game::constants::{boost, mass};

    let player = match state.get_player_mut(player_id) {
        Some(p) if p.alive => p,
        _ => return false,
    };

    // Apply thrust if player is boosting
    if input.boost && input.thrust.length_sq() > THRUST_INPUT_THRESHOLD_SQ {
        let thrust_dir = input.thrust.normalize();

        // Calculate thrust force (base thrust, could scale with mass)
        let thrust_force = boost::BASE_THRUST;

        // Apply thrust to velocity
        player.velocity += thrust_dir * thrust_force * dt;

        // Calculate mass cost - ONLY drain mass if NOT spawn protected
        // This prevents bots/players from losing mass while invulnerable
        if player.spawn_protection <= 0.0 {
            let mass_cost = boost::BASE_COST + player.mass * boost::MASS_COST_RATIO;
            player.mass = (player.mass - mass_cost * dt).max(mass::MINIMUM);
        }

        // Update rotation to face thrust direction
        player.rotation = thrust_dir.angle();

        return true;
    }

    // Update rotation to face aim direction if not thrusting
    if input.aim.length_sq() > AIM_INPUT_THRESHOLD_SQ {
        player.rotation = input.aim.normalize().angle();
    }

    false
}

/// Calculate kinetic energy for a body
/// Used in advanced_physics feature for collision calculations
#[allow(dead_code)]
pub fn kinetic_energy(mass: f32, velocity: Vec2) -> f32 {
    KINETIC_ENERGY_COEFFICIENT * mass * velocity.length_sq()
}

/// Calculate momentum for a body (vector form)
/// Available for advanced collision calculations
#[allow(dead_code)]
pub fn momentum(mass: f32, velocity: Vec2) -> Vec2 {
    velocity * mass
}

/// Calculate momentum magnitude
pub fn momentum_magnitude(mass: f32, velocity: Vec2) -> f32 {
    mass * velocity.length()
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
            position: Vec2::new(100.0, 100.0),
            velocity: Vec2::new(50.0, 0.0),
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
    fn test_exponential_drag() {
        let (mut state, player_id) = create_test_state();
        let initial_velocity = state.get_player(player_id).unwrap().velocity.length();

        update(&mut state, DT);

        let new_velocity = state.get_player(player_id).unwrap().velocity.length();

        // Velocity should decrease by drag factor
        let expected = initial_velocity * (1.0 - DRAG);
        assert!((new_velocity - expected).abs() < 0.001);
    }

    #[test]
    fn test_drag_is_not_linear() {
        // Verify that DRAG = 0.002 causes ~0.2% reduction per tick
        let initial = 100.0;
        let after_one_tick = initial * (1.0 - DRAG);

        assert!((after_one_tick - 99.8).abs() < 0.01);
    }

    #[test]
    fn test_position_integration() {
        let (mut state, player_id) = create_test_state();
        let initial_pos = state.get_player(player_id).unwrap().position;
        let velocity = state.get_player(player_id).unwrap().velocity;

        update(&mut state, DT);

        // Position should change by approximately velocity * dt
        // (slight difference due to drag)
        let expected_delta = velocity * DT;
        let actual_delta = state.get_player(player_id).unwrap().position - initial_pos;

        assert!((actual_delta.x - expected_delta.x).abs() < 1.0);
    }

    #[test]
    fn test_velocity_clamping() {
        let (mut state, player_id) = create_test_state();
        state.get_player_mut(player_id).unwrap().velocity = Vec2::new(1000.0, 0.0);

        update(&mut state, DT);

        let velocity = state.get_player(player_id).unwrap().velocity.length();
        assert!(velocity <= MAX_VELOCITY);
    }

    #[test]
    fn test_dead_players_not_updated() {
        let (mut state, player_id) = create_test_state();
        state.get_player_mut(player_id).unwrap().alive = false;
        let initial_pos = state.get_player(player_id).unwrap().position;

        update(&mut state, DT);

        assert_eq!(state.get_player(player_id).unwrap().position, initial_pos);
    }

    #[test]
    fn test_spawn_protection_decay() {
        let (mut state, player_id) = create_test_state();
        state.get_player_mut(player_id).unwrap().spawn_protection = 3.0;

        update(&mut state, DT);

        assert!(state.get_player(player_id).unwrap().spawn_protection < 3.0);
        assert!(state.get_player(player_id).unwrap().spawn_protection > 0.0);
    }

    #[test]
    fn test_spawn_protection_prevents_boost_mass_drain() {
        use crate::game::constants::mass::STARTING;
        use crate::net::protocol::PlayerInput;

        let (mut state, player_id) = create_test_state();
        let player = state.get_player_mut(player_id).unwrap();
        player.spawn_protection = 3.0;
        player.mass = STARTING;
        let initial_mass = player.mass;

        // Create boost input
        let input = PlayerInput {
            sequence: 0,
            tick: 0,
            client_time: 0,
            thrust: Vec2::new(1.0, 0.0),
            aim: Vec2::new(1.0, 0.0),
            boost: true,
            fire: false,
            fire_released: false,
        };

        // Apply thrust with boost while spawn protected
        apply_thrust(&mut state, player_id, &input, DT);

        // Mass should NOT have decreased (spawn protection prevents drain)
        let final_mass = state.get_player(player_id).unwrap().mass;
        assert_eq!(
            final_mass, initial_mass,
            "Mass should not drain during spawn protection: {} -> {}",
            initial_mass, final_mass
        );

        // Velocity should still increase (thrust still works, just no cost)
        let velocity = state.get_player(player_id).unwrap().velocity.length();
        assert!(velocity > 0.1, "Velocity should increase from thrust");
    }

    #[test]
    fn test_boost_drains_mass_without_spawn_protection() {
        use crate::game::constants::mass::STARTING;
        use crate::net::protocol::PlayerInput;

        let (mut state, player_id) = create_test_state();
        let player = state.get_player_mut(player_id).unwrap();
        player.spawn_protection = 0.0; // No spawn protection
        player.mass = STARTING;
        let initial_mass = player.mass;

        // Create boost input
        let input = PlayerInput {
            sequence: 0,
            tick: 0,
            client_time: 0,
            thrust: Vec2::new(1.0, 0.0),
            aim: Vec2::new(1.0, 0.0),
            boost: true,
            fire: false,
            fire_released: false,
        };

        // Apply thrust with boost without spawn protection
        apply_thrust(&mut state, player_id, &input, DT);

        // Mass SHOULD have decreased
        let final_mass = state.get_player(player_id).unwrap().mass;
        assert!(
            final_mass < initial_mass,
            "Mass should drain when boosting without spawn protection: {} -> {}",
            initial_mass, final_mass
        );
    }

    #[test]
    fn test_projectile_lifetime_decay() {
        let (mut state, _) = create_test_state();
        state.add_projectile(
            uuid::Uuid::new_v4(),
            Vec2::new(100.0, 100.0),
            Vec2::new(100.0, 0.0),
            10.0,
        );

        let initial_lifetime = state.projectiles[0].lifetime;

        update(&mut state, DT);

        assert!(state.projectiles[0].lifetime < initial_lifetime);
    }

    #[test]
    fn test_expired_projectiles_removed() {
        let (mut state, _) = create_test_state();
        state.add_projectile(
            uuid::Uuid::new_v4(),
            Vec2::ZERO,
            Vec2::ZERO,
            10.0,
        );
        state.projectiles[0].lifetime = 0.01;

        update(&mut state, DT);

        assert!(state.projectiles.is_empty());
    }

    #[test]
    fn test_apply_thrust() {
        let (mut state, player_id) = create_test_state();
        let initial_velocity = state.get_player(player_id).unwrap().velocity;

        let input = PlayerInput {
            sequence: 1,
            tick: 1,
            client_time: 0,
            thrust: Vec2::new(1.0, 0.0),
            aim: Vec2::ZERO,
            boost: true,
            fire: false,
            fire_released: false,
        };

        let applied = apply_thrust(&mut state, player_id, &input, DT);

        assert!(applied);
        assert!(state.get_player(player_id).unwrap().velocity.x > initial_velocity.x);
    }

    #[test]
    fn test_thrust_consumes_mass() {
        let (mut state, player_id) = create_test_state();
        let initial_mass = state.get_player(player_id).unwrap().mass;

        let input = PlayerInput {
            sequence: 1,
            tick: 1,
            client_time: 0,
            thrust: Vec2::new(1.0, 0.0),
            aim: Vec2::ZERO,
            boost: true,
            fire: false,
            fire_released: false,
        };

        apply_thrust(&mut state, player_id, &input, DT);

        assert!(state.get_player(player_id).unwrap().mass < initial_mass);
    }

    #[test]
    fn test_thrust_without_boost_flag() {
        let (mut state, player_id) = create_test_state();
        let initial_velocity = state.get_player(player_id).unwrap().velocity;

        let input = PlayerInput {
            sequence: 1,
            tick: 1,
            client_time: 0,
            thrust: Vec2::new(1.0, 0.0),
            aim: Vec2::ZERO,
            boost: false, // Not boosting
            fire: false,
            fire_released: false,
        };

        let applied = apply_thrust(&mut state, player_id, &input, DT);

        assert!(!applied);
        assert_eq!(state.get_player(player_id).unwrap().velocity, initial_velocity);
    }

    #[test]
    fn test_kinetic_energy() {
        let mass = 100.0;
        let velocity = Vec2::new(10.0, 0.0);
        let ke = kinetic_energy(mass, velocity);

        // KE = 0.5 * m * v^2 = 0.5 * 100 * 100 = 5000
        assert!((ke - 5000.0).abs() < 0.001);
    }

    #[test]
    fn test_momentum() {
        let mass = 100.0;
        let velocity = Vec2::new(10.0, 5.0);
        let p = momentum(mass, velocity);

        assert!((p.x - 1000.0).abs() < 0.001);
        assert!((p.y - 500.0).abs() < 0.001);
    }

    #[test]
    fn test_physics_determinism() {
        // Same inputs should produce same outputs
        let (mut state1, player_id1) = create_test_state();
        let (mut state2, player_id2) = create_test_state();

        state1.get_player_mut(player_id1).unwrap().position = Vec2::new(100.0, 100.0);
        state1.get_player_mut(player_id1).unwrap().velocity = Vec2::new(50.0, 25.0);
        state2.get_player_mut(player_id2).unwrap().position = Vec2::new(100.0, 100.0);
        state2.get_player_mut(player_id2).unwrap().velocity = Vec2::new(50.0, 25.0);

        for _ in 0..100 {
            update(&mut state1, DT);
            update(&mut state2, DT);
        }

        assert_eq!(state1.get_player(player_id1).unwrap().position, state2.get_player(player_id2).unwrap().position);
        assert_eq!(state1.get_player(player_id1).unwrap().velocity, state2.get_player(player_id2).unwrap().velocity);
    }

    // === DEBRIS PHYSICS TESTS ===

    #[test]
    fn test_debris_lifetime_decay() {
        use crate::game::state::DebrisSize;

        let (mut state, _) = create_test_state();
        state.add_debris(Vec2::new(100.0, 100.0), Vec2::ZERO, DebrisSize::Medium);

        let initial_lifetime = state.debris[0].lifetime;
        update(&mut state, DT);

        assert!(state.debris[0].lifetime < initial_lifetime);
        assert!((initial_lifetime - state.debris[0].lifetime - DT).abs() < 0.001);
    }

    #[test]
    fn test_expired_debris_removed() {
        use crate::game::state::DebrisSize;

        let (mut state, _) = create_test_state();
        state.add_debris(Vec2::new(100.0, 100.0), Vec2::ZERO, DebrisSize::Small);
        state.debris[0].lifetime = 0.01; // About to expire

        update(&mut state, DT);

        assert!(state.debris.is_empty(), "Expired debris should be removed");
    }

    #[test]
    fn test_debris_drag_application() {
        use crate::game::state::DebrisSize;

        let (mut state, _) = create_test_state();
        let initial_vel = Vec2::new(100.0, 50.0);
        state.add_debris(Vec2::new(200.0, 200.0), initial_vel, DebrisSize::Medium);

        update(&mut state, DT);

        let expected_vel = initial_vel * (1.0 - DRAG);
        let actual_vel = state.debris[0].velocity;
        assert!((actual_vel - expected_vel).length() < 0.01);
    }

    #[test]
    fn test_debris_position_integration() {
        use crate::game::state::DebrisSize;

        let (mut state, _) = create_test_state();
        let initial_pos = Vec2::new(200.0, 200.0);
        let initial_vel = Vec2::new(100.0, 50.0);
        state.add_debris(initial_pos, initial_vel, DebrisSize::Medium);

        update(&mut state, DT);

        // Position should change by approximately velocity * dt (before drag)
        let expected_delta = initial_vel * DT;
        let actual_delta = state.debris[0].position - initial_pos;
        assert!((actual_delta - expected_delta).length() < 1.0);
    }

    #[test]
    fn test_out_of_bounds_debris_removed() {
        use crate::game::state::DebrisSize;

        let (mut state, _) = create_test_state();
        let escape = state.arena.escape_radius;
        // Place debris way outside arena
        state.add_debris(Vec2::new(escape * 2.0, 0.0), Vec2::ZERO, DebrisSize::Small);

        update(&mut state, DT);

        assert!(state.debris.is_empty(), "Out-of-bounds debris should be removed");
    }

    #[test]
    fn test_out_of_bounds_projectile_removed() {
        let (mut state, _) = create_test_state();
        let escape = state.arena.escape_radius;
        // Place projectile way outside arena
        state.add_projectile(
            uuid::Uuid::new_v4(),
            Vec2::new(escape * 2.0, 0.0),
            Vec2::ZERO,
            10.0,
        );

        update(&mut state, DT);

        assert!(state.projectiles.is_empty(), "Out-of-bounds projectile should be removed");
    }

    #[test]
    fn test_debris_at_boundary_kept() {
        use crate::game::state::DebrisSize;

        let (mut state, _) = create_test_state();
        let escape = state.arena.escape_radius;
        // Place debris just inside the cleanup boundary (1.2x escape)
        state.add_debris(Vec2::new(escape * 1.1, 0.0), Vec2::ZERO, DebrisSize::Small);

        update(&mut state, DT);

        assert_eq!(state.debris.len(), 1, "Debris inside boundary should be kept");
    }

    #[test]
    fn test_projectile_at_boundary_kept() {
        let (mut state, _) = create_test_state();
        let escape = state.arena.escape_radius;
        // Place projectile just inside the cleanup boundary (1.5x escape)
        state.add_projectile(
            uuid::Uuid::new_v4(),
            Vec2::new(escape * 1.4, 0.0),
            Vec2::ZERO,
            10.0,
        );

        update(&mut state, DT);

        assert_eq!(state.projectiles.len(), 1, "Projectile inside boundary should be kept");
    }

    #[test]
    fn test_multiple_entities_update() {
        use crate::game::state::DebrisSize;

        let (mut state, player_id) = create_test_state();

        // Add multiple entities
        for i in 0..10 {
            let pos = Vec2::new(100.0 + i as f32 * 50.0, 100.0);
            state.add_debris(pos, Vec2::new(10.0, 0.0), DebrisSize::Small);
            state.add_projectile(uuid::Uuid::new_v4(), pos, Vec2::new(20.0, 0.0), 5.0);
        }

        // All should update
        update(&mut state, DT);

        assert_eq!(state.debris.len(), 10);
        assert_eq!(state.projectiles.len(), 10);

        // All should have moved
        for debris in &state.debris {
            assert!(debris.velocity.x > 9.9);
        }
    }

    #[test]
    fn test_very_high_velocity_debris() {
        use crate::game::state::DebrisSize;

        let (mut state, _) = create_test_state();
        state.add_debris(Vec2::new(100.0, 100.0), Vec2::new(10000.0, 10000.0), DebrisSize::Medium);

        // Should not panic or produce NaN
        update(&mut state, DT);

        assert!(!state.debris[0].velocity.x.is_nan());
        assert!(!state.debris[0].velocity.y.is_nan());
        assert!(!state.debris[0].position.x.is_nan());
        assert!(!state.debris[0].position.y.is_nan());
    }

    #[test]
    fn test_zero_velocity_debris() {
        use crate::game::state::DebrisSize;

        let (mut state, _) = create_test_state();
        state.add_debris(Vec2::new(100.0, 100.0), Vec2::ZERO, DebrisSize::Medium);

        update(&mut state, DT);

        // Should not move, but should still decay lifetime
        assert_eq!(state.debris[0].position, Vec2::new(100.0, 100.0));
        assert!(state.debris[0].lifetime < 90.0);
    }

    // === THRUST EDGE CASES ===

    #[test]
    fn test_thrust_minimum_mass_protection() {
        use crate::game::constants::mass::MINIMUM;

        let (mut state, player_id) = create_test_state();
        state.get_player_mut(player_id).unwrap().mass = MINIMUM + 0.1;

        let input = PlayerInput {
            sequence: 1,
            tick: 1,
            client_time: 0,
            thrust: Vec2::new(1.0, 0.0),
            aim: Vec2::ZERO,
            boost: true,
            fire: false,
            fire_released: false,
        };

        // Apply thrust many times
        for _ in 0..100 {
            apply_thrust(&mut state, player_id, &input, DT);
        }

        // Mass should never go below minimum
        assert!(state.get_player(player_id).unwrap().mass >= MINIMUM);
    }

    #[test]
    fn test_thrust_diagonal_direction() {
        let (mut state, player_id) = create_test_state();
        state.get_player_mut(player_id).unwrap().velocity = Vec2::ZERO;

        let input = PlayerInput {
            sequence: 1,
            tick: 1,
            client_time: 0,
            thrust: Vec2::new(1.0, 1.0), // Diagonal
            aim: Vec2::ZERO,
            boost: true,
            fire: false,
            fire_released: false,
        };

        apply_thrust(&mut state, player_id, &input, DT);

        let vel = state.get_player(player_id).unwrap().velocity;
        // Should move diagonally
        assert!(vel.x > 0.0);
        assert!(vel.y > 0.0);
        // Should be roughly equal in x and y
        assert!((vel.x - vel.y).abs() < 0.1);
    }

    #[test]
    fn test_thrust_very_small_input() {
        let (mut state, player_id) = create_test_state();
        let initial_velocity = state.get_player(player_id).unwrap().velocity;

        let input = PlayerInput {
            sequence: 1,
            tick: 1,
            client_time: 0,
            thrust: Vec2::new(0.001, 0.001), // Very small, below threshold
            aim: Vec2::ZERO,
            boost: true,
            fire: false,
            fire_released: false,
        };

        let applied = apply_thrust(&mut state, player_id, &input, DT);

        // Should not apply thrust (length_sq < 0.01)
        assert!(!applied);
        assert_eq!(state.get_player(player_id).unwrap().velocity, initial_velocity);
    }

    #[test]
    fn test_aim_updates_rotation() {
        let (mut state, player_id) = create_test_state();

        let input = PlayerInput {
            sequence: 1,
            tick: 1,
            client_time: 0,
            thrust: Vec2::ZERO,
            aim: Vec2::new(0.0, 1.0), // Aim up
            boost: false,
            fire: false,
            fire_released: false,
        };

        apply_thrust(&mut state, player_id, &input, DT);

        let rotation = state.get_player(player_id).unwrap().rotation;
        // Should face upward (PI/2)
        assert!((rotation - std::f32::consts::FRAC_PI_2).abs() < 0.1);
    }

    #[test]
    fn test_dead_player_no_thrust() {
        let (mut state, player_id) = create_test_state();
        state.get_player_mut(player_id).unwrap().alive = false;
        state.get_player_mut(player_id).unwrap().velocity = Vec2::ZERO;

        let input = PlayerInput {
            sequence: 1,
            tick: 1,
            client_time: 0,
            thrust: Vec2::new(1.0, 0.0),
            aim: Vec2::ZERO,
            boost: true,
            fire: false,
            fire_released: false,
        };

        let applied = apply_thrust(&mut state, player_id, &input, DT);

        assert!(!applied);
        assert_eq!(state.get_player(player_id).unwrap().velocity, Vec2::ZERO);
    }

    #[test]
    fn test_nonexistent_player_no_thrust() {
        let (mut state, _) = create_test_state();
        let fake_id = uuid::Uuid::new_v4();

        let input = PlayerInput {
            sequence: 1,
            tick: 1,
            client_time: 0,
            thrust: Vec2::new(1.0, 0.0),
            aim: Vec2::ZERO,
            boost: true,
            fire: false,
            fire_released: false,
        };

        let applied = apply_thrust(&mut state, fake_id, &input, DT);
        assert!(!applied);
    }

    // === PARALLEL PROCESSING SANITY ===

    #[test]
    fn test_many_entities_parallel() {
        use crate::game::state::DebrisSize;

        let (mut state, _) = create_test_state();

        // Add many entities to exercise parallel processing
        for _ in 0..100 {
            let pos = Vec2::new(
                rand::random::<f32>() * 400.0 + 100.0,
                rand::random::<f32>() * 400.0 + 100.0,
            );
            state.add_debris(pos, Vec2::new(10.0, 5.0), DebrisSize::Small);
            state.add_projectile(uuid::Uuid::new_v4(), pos, Vec2::new(20.0, 10.0), 5.0);
        }

        // Run many ticks
        for _ in 0..100 {
            update(&mut state, DT);
        }

        // Should complete without panic
        assert!(true);
    }
}
