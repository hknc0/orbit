use rayon::prelude::*;

use crate::game::constants::physics::{DRAG, MAX_VELOCITY};
use crate::game::state::GameState;
use crate::net::protocol::PlayerInput;
use crate::util::vec2::Vec2;

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

    // Remove expired projectiles (sequential - modifies collection)
    state.projectiles.retain(|p| p.lifetime > 0.0);

    // Update debris in parallel
    state.debris.par_iter_mut().for_each(|debris| {
        debris.velocity *= drag_factor;
        debris.position += debris.velocity * dt;
    });
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
    if input.boost && input.thrust.length_sq() > 0.01 {
        let thrust_dir = input.thrust.normalize();

        // Calculate thrust force (base thrust, could scale with mass)
        let thrust_force = boost::BASE_THRUST;

        // Apply thrust to velocity
        player.velocity += thrust_dir * thrust_force * dt;

        // Calculate mass cost
        let mass_cost = boost::BASE_COST + player.mass * boost::MASS_COST_RATIO;
        player.mass = (player.mass - mass_cost * dt).max(mass::MINIMUM);

        // Update rotation to face thrust direction
        player.rotation = thrust_dir.angle();

        return true;
    }

    // Update rotation to face aim direction if not thrusting
    if input.aim.length_sq() > 0.01 {
        player.rotation = input.aim.normalize().angle();
    }

    false
}

/// Calculate kinetic energy for a body
pub fn kinetic_energy(mass: f32, velocity: Vec2) -> f32 {
    0.5 * mass * velocity.length_sq()
}

/// Calculate momentum for a body
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
}
