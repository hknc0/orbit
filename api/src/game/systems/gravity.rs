use crate::game::constants::physics::{CENTRAL_MASS, G};
use crate::game::state::{GameState, GravityWell};
use crate::util::vec2::Vec2;

/// Apply gravity from all gravity wells to all entities
pub fn update_central(state: &mut GameState, dt: f32) {
    // Clone wells to avoid borrow issues
    let wells = state.arena.gravity_wells.clone();

    // Apply gravity to players
    for player in &mut state.players {
        if !player.alive {
            continue;
        }

        let gravity = calculate_multi_well_gravity(player.position, &wells);
        player.velocity += gravity * dt;
    }

    // Apply gravity to projectiles
    for projectile in &mut state.projectiles {
        let gravity = calculate_multi_well_gravity(projectile.position, &wells);
        projectile.velocity += gravity * dt;
    }

    // Apply gravity to debris
    for debris in &mut state.debris {
        let gravity = calculate_multi_well_gravity(debris.position, &wells);
        debris.velocity += gravity * dt;
    }
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
    if distance_sq < min_distance_sq {
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

    // Direction toward center (negative of position normalized)
    let direction = -position.normalize();

    // Gravitational acceleration magnitude: G * M / r^2
    let acceleration = G * CENTRAL_MASS / distance_sq;

    // Clamp to prevent extreme accelerations
    let clamped_accel = acceleration.min(500.0);

    direction * clamped_accel
}

/// Apply inter-entity gravity (entities attract each other)
/// This is optional and can be disabled for performance
pub fn update_inter_entity(state: &mut GameState, dt: f32) {
    let player_count = state.players.len();

    // Calculate gravitational forces between all player pairs
    let mut accelerations: Vec<Vec2> = vec![Vec2::ZERO; player_count];

    for i in 0..player_count {
        if !state.players[i].alive {
            continue;
        }

        for j in (i + 1)..player_count {
            if !state.players[j].alive {
                continue;
            }

            let delta = state.players[j].position - state.players[i].position;
            let distance_sq = delta.length_sq();

            // Skip if too close (handled by collision) or too far
            if distance_sq < 100.0 || distance_sq > 1_000_000.0 {
                continue;
            }

            let distance = distance_sq.sqrt();
            let direction = delta * (1.0 / distance);

            // Mutual gravitational force
            // F = G * m1 * m2 / r^2
            // Scale down inter-entity gravity to be subtle
            let force_magnitude =
                G * state.players[i].mass * state.players[j].mass / distance_sq * 0.01;

            // F = ma, so a = F/m
            let accel_i = direction * (force_magnitude / state.players[i].mass);
            let accel_j = -direction * (force_magnitude / state.players[j].mass);

            accelerations[i] += accel_i;
            accelerations[j] += accel_j;
        }
    }

    // Apply accumulated accelerations
    for (i, player) in state.players.iter_mut().enumerate() {
        if player.alive {
            player.velocity += accelerations[i] * dt;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::constants::physics::DT;
    use crate::game::state::Player;

    fn create_test_state() -> GameState {
        let mut state = GameState::new();
        state.players.push(Player {
            id: uuid::Uuid::new_v4(),
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
        });
        state
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
        let mut state = create_test_state();
        let initial_velocity = state.players[0].velocity;

        update_central(&mut state, DT);

        // Velocity should have changed toward center
        assert!(state.players[0].velocity.x < initial_velocity.x);
    }

    #[test]
    fn test_dead_players_no_gravity() {
        let mut state = create_test_state();
        state.players[0].alive = false;
        state.players[0].velocity = Vec2::ZERO;

        update_central(&mut state, DT);

        assert_eq!(state.players[0].velocity, Vec2::ZERO);
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
        state.players.push(Player {
            id: uuid::Uuid::new_v4(),
            name: "A".to_string(),
            position: Vec2::new(100.0, 0.0),
            velocity: Vec2::ZERO,
            mass: 200.0,
            alive: true,
            ..Default::default()
        });

        state.players.push(Player {
            id: uuid::Uuid::new_v4(),
            name: "B".to_string(),
            position: Vec2::new(200.0, 0.0),
            velocity: Vec2::ZERO,
            mass: 200.0,
            alive: true,
            ..Default::default()
        });

        update_inter_entity(&mut state, DT);

        // Players should be attracted toward each other
        assert!(state.players[0].velocity.x > 0.0); // A moves toward B
        assert!(state.players[1].velocity.x < 0.0); // B moves toward A
    }

    #[test]
    fn test_gravity_applies_to_projectiles() {
        let mut state = create_test_state();
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
}
