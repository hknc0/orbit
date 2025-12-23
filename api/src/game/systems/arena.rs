use crate::game::constants::arena::*;
use crate::game::constants::spawn::RESPAWN_DELAY;
use crate::game::state::{GameState, MatchPhase};

/// Arena events
#[derive(Debug, Clone)]
pub enum ArenaEvent {
    /// Zone collapse started
    CollapseStarted { phase: u8, new_safe_radius: f32 },
    /// Player entered danger zone (core)
    PlayerEnteredCore { player_id: uuid::Uuid },
    /// Player is outside arena bounds
    PlayerOutsideArena { player_id: uuid::Uuid, mass_lost: f32 },
}

/// Update arena state (zone collapse)
pub fn update(state: &mut GameState, dt: f32) -> Vec<ArenaEvent> {
    let mut events = Vec::new();

    // Only update arena during playing phase
    if state.match_state.phase != MatchPhase::Playing {
        return events;
    }

    // Arena collapse disabled for eternal game mode
    // The arena stays at fixed size forever

    // Check players against arena boundaries
    events.extend(check_player_boundaries(state, dt));

    events
}

/// Update arena radii based on collapse phase
fn update_arena_radii(state: &mut GameState) {
    let phase = state.arena.collapse_phase as f32;
    let max_phases = COLLAPSE_PHASES as f32;
    let progress = phase / max_phases;

    // Shrink all radii toward core
    state.arena.escape_radius = ESCAPE_RADIUS * (1.0 - progress * 0.8);
    state.arena.outer_radius = OUTER_RADIUS * (1.0 - progress * 0.8);
    state.arena.middle_radius = MIDDLE_RADIUS * (1.0 - progress * 0.8);
    state.arena.inner_radius = INNER_RADIUS * (1.0 - progress * 0.6);

    // Core doesn't change
}

/// Check player positions against arena boundaries
fn check_player_boundaries(state: &mut GameState, dt: f32) -> Vec<ArenaEvent> {
    let mut events = Vec::new();
    let safe_radius = state.arena.current_safe_radius();
    let wells = state.arena.gravity_wells.clone();

    for player in &mut state.players {
        if !player.alive {
            continue;
        }

        // Check against all gravity well cores (instant death zones)
        let mut in_core = false;
        for well in &wells {
            let distance_to_well = (player.position - well.position).length();
            if distance_to_well < well.core_radius {
                in_core = true;
                break;
            }
        }

        if in_core {
            player.alive = false;
            player.deaths += 1;
            player.respawn_timer = RESPAWN_DELAY;
            events.push(ArenaEvent::PlayerEnteredCore { player_id: player.id });
            continue;
        }

        // Check outside safe zone (mass drain) - distance from arena center
        let distance_from_center = player.position.length();
        if distance_from_center > safe_radius {
            let excess = distance_from_center - safe_radius;
            let drain_rate = ESCAPE_MASS_DRAIN * (1.0 + excess / 100.0); // Faster drain farther out
            let mass_lost = drain_rate * dt;

            player.mass = (player.mass - mass_lost).max(0.0);

            events.push(ArenaEvent::PlayerOutsideArena {
                player_id: player.id,
                mass_lost,
            });

            // Check for death from mass loss
            if player.mass < crate::game::constants::mass::MINIMUM {
                player.alive = false;
                player.deaths += 1;
                player.respawn_timer = RESPAWN_DELAY;
            }
        }
    }

    events
}

/// Get the zone a position is in
pub fn get_zone(position: crate::util::vec2::Vec2, arena: &crate::game::state::Arena) -> Zone {
    let distance = position.length();

    if distance < arena.core_radius {
        Zone::Core
    } else if distance < arena.inner_radius {
        Zone::Inner
    } else if distance < arena.middle_radius {
        Zone::Middle
    } else if distance < arena.outer_radius {
        Zone::Outer
    } else if distance < arena.escape_radius {
        Zone::Escape
    } else {
        Zone::Outside
    }
}

/// Arena zones
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// Core - instant death
    Core,
    /// Inner safe zone
    Inner,
    /// Middle zone
    Middle,
    /// Outer zone
    Outer,
    /// Escape zone (near boundary)
    Escape,
    /// Outside arena (mass drain)
    Outside,
}

impl Zone {
    /// Get danger level (0.0 = safe, 1.0 = deadly)
    pub fn danger_level(&self) -> f32 {
        match self {
            Zone::Core => 1.0,
            Zone::Inner => 0.0,
            Zone::Middle => 0.1,
            Zone::Outer => 0.3,
            Zone::Escape => 0.6,
            Zone::Outside => 0.9,
        }
    }

    /// Is this zone safe?
    pub fn is_safe(&self) -> bool {
        matches!(self, Zone::Inner | Zone::Middle | Zone::Outer)
    }
}

/// Calculate a random spawn position within the spawn zone
pub fn random_spawn_position() -> crate::util::vec2::Vec2 {
    random_spawn_position_scaled(1.0)
}

/// Calculate a random spawn position with arena scale
pub fn random_spawn_position_scaled(scale: f32) -> crate::util::vec2::Vec2 {
    use crate::game::constants::spawn::{ZONE_MAX, ZONE_MIN};
    use rand::Rng;

    let mut rng = rand::thread_rng();
    let angle = rng.gen_range(0.0..std::f32::consts::TAU);
    let radius = rng.gen_range(ZONE_MIN * scale..ZONE_MAX * scale);

    crate::util::vec2::Vec2::from_angle(angle) * radius
}

/// Calculate a safe spawn position avoiding other players
pub fn safe_spawn_position(existing_positions: &[crate::util::vec2::Vec2]) -> crate::util::vec2::Vec2 {
    use crate::game::constants::spawn::{MAX_SPAWN_ATTEMPTS, SAFE_DISTANCE};

    for _ in 0..MAX_SPAWN_ATTEMPTS {
        let pos = random_spawn_position();

        // Check if far enough from all existing players
        let is_safe = existing_positions.iter().all(|other| {
            (pos - *other).length() >= SAFE_DISTANCE
        });

        if is_safe {
            return pos;
        }
    }

    // Fallback to random if can't find safe spot
    random_spawn_position()
}

/// Calculate a spawn position near a random gravity well
pub fn spawn_near_well(wells: &[crate::game::state::GravityWell]) -> crate::util::vec2::Vec2 {
    use crate::game::constants::spawn::{ZONE_MAX, ZONE_MIN};
    use rand::Rng;

    if wells.is_empty() {
        return random_spawn_position();
    }

    let mut rng = rand::thread_rng();

    // Pick a random well
    let well = &wells[rng.gen_range(0..wells.len())];

    // Spawn in orbit zone around that well
    let angle = rng.gen_range(0.0..std::f32::consts::TAU);
    let radius = rng.gen_range(ZONE_MIN..ZONE_MAX);
    let offset = crate::util::vec2::Vec2::from_angle(angle) * radius;

    well.position + offset
}

/// Calculate a safe spawn position near gravity wells, avoiding other players
pub fn safe_spawn_near_well(
    wells: &[crate::game::state::GravityWell],
    existing_positions: &[crate::util::vec2::Vec2],
) -> crate::util::vec2::Vec2 {
    use crate::game::constants::spawn::{MAX_SPAWN_ATTEMPTS, SAFE_DISTANCE};

    for _ in 0..MAX_SPAWN_ATTEMPTS {
        let pos = spawn_near_well(wells);

        // Check if far enough from all existing players
        let is_safe = existing_positions.iter().all(|other| {
            (pos - *other).length() >= SAFE_DISTANCE
        });

        if is_safe {
            return pos;
        }
    }

    // Fallback to random well spawn
    spawn_near_well(wells)
}

/// Calculate spawn velocity relative to nearest gravity well (tangent to orbit)
pub fn spawn_velocity_for_well(
    position: crate::util::vec2::Vec2,
    wells: &[crate::game::state::GravityWell],
) -> crate::util::vec2::Vec2 {
    use crate::game::constants::spawn::INITIAL_VELOCITY;

    // Find nearest well
    let nearest_well = wells
        .iter()
        .min_by(|a, b| {
            let dist_a = (a.position - position).length_sq();
            let dist_b = (b.position - position).length_sq();
            dist_a.partial_cmp(&dist_b).unwrap()
        });

    let center = nearest_well.map(|w| w.position).unwrap_or(crate::util::vec2::Vec2::ZERO);

    // Perpendicular to direction from well (tangent to orbit) - counter-clockwise
    let to_center = center - position;
    let tangent = to_center.perpendicular().normalize();
    tangent * INITIAL_VELOCITY
}

/// Calculate spawn velocity (tangent to orbit, fixed speed like orbit-poc)
pub fn spawn_velocity(position: crate::util::vec2::Vec2) -> crate::util::vec2::Vec2 {
    use crate::game::constants::spawn::INITIAL_VELOCITY;

    // Perpendicular to position (tangent to orbit) - counter-clockwise
    let tangent = position.perpendicular().normalize();
    tangent * INITIAL_VELOCITY
}

/// Calculate spawn positions for multiple players evenly distributed
pub fn spawn_positions(count: usize) -> Vec<crate::util::vec2::Vec2> {
    use crate::game::constants::spawn::{ZONE_MAX, ZONE_MIN};

    let angle_step = std::f32::consts::TAU / count as f32;
    let radius = (ZONE_MIN + ZONE_MAX) / 2.0;

    (0..count)
        .map(|i| {
            let angle = angle_step * i as f32;
            crate::util::vec2::Vec2::from_angle(angle) * radius
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::state::{Arena, Player};
    use crate::util::vec2::Vec2;

    fn create_test_state() -> GameState {
        let mut state = GameState::new();
        state.match_state.phase = MatchPhase::Playing;
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
    fn test_get_zone() {
        let arena = Arena::default();

        // Core
        assert_eq!(get_zone(Vec2::new(25.0, 0.0), &arena), Zone::Core);

        // Inner
        assert_eq!(get_zone(Vec2::new(100.0, 0.0), &arena), Zone::Inner);

        // Middle
        assert_eq!(get_zone(Vec2::new(300.0, 0.0), &arena), Zone::Middle);

        // Outer
        assert_eq!(get_zone(Vec2::new(500.0, 0.0), &arena), Zone::Outer);

        // Escape
        assert_eq!(get_zone(Vec2::new(700.0, 0.0), &arena), Zone::Escape);

        // Outside
        assert_eq!(get_zone(Vec2::new(1000.0, 0.0), &arena), Zone::Outside);
    }

    #[test]
    fn test_zone_danger_levels() {
        assert_eq!(Zone::Core.danger_level(), 1.0);
        assert_eq!(Zone::Inner.danger_level(), 0.0);
        assert!(Zone::Outside.danger_level() > Zone::Inner.danger_level());
    }

    #[test]
    fn test_zone_is_safe() {
        assert!(!Zone::Core.is_safe());
        assert!(Zone::Inner.is_safe());
        assert!(Zone::Middle.is_safe());
        assert!(Zone::Outer.is_safe());
        assert!(!Zone::Escape.is_safe());
        assert!(!Zone::Outside.is_safe());
    }

    #[test]
    fn test_core_kills_player() {
        let mut state = create_test_state();
        state.players[0].position = Vec2::new(25.0, 0.0); // Inside core

        let events = update(&mut state, 0.1);

        assert!(!state.players[0].alive);
        assert!(events
            .iter()
            .any(|e| matches!(e, ArenaEvent::PlayerEnteredCore { .. })));
    }

    #[test]
    fn test_outside_drains_mass() {
        let mut state = create_test_state();
        state.players[0].position = Vec2::new(1000.0, 0.0); // Outside arena

        let initial_mass = state.players[0].mass;
        let events = update(&mut state, 0.1);

        assert!(state.players[0].mass < initial_mass);
        assert!(events
            .iter()
            .any(|e| matches!(e, ArenaEvent::PlayerOutsideArena { .. })));
    }

    #[test]
    fn test_collapse_disabled_for_eternal_mode() {
        let mut state = create_test_state();
        state.arena.time_until_collapse = 0.1; // Would trigger collapse if enabled

        // Update many times
        for _ in 0..100 {
            update(&mut state, 0.1);
        }

        // Collapse should NOT have started (disabled for eternal mode)
        assert!(!state.arena.is_collapsing);
        assert_eq!(state.arena.collapse_phase, 0);
    }

    #[test]
    fn test_not_updating_outside_playing_phase() {
        let mut state = create_test_state();
        state.match_state.phase = MatchPhase::Waiting;
        state.arena.time_until_collapse = 0.1;

        update(&mut state, 1.0);

        // Timer shouldn't have changed
        assert!((state.arena.time_until_collapse - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_spawn_positions_count() {
        let positions = spawn_positions(10);
        assert_eq!(positions.len(), 10);
    }

    #[test]
    fn test_spawn_positions_distributed() {
        let positions = spawn_positions(4);

        // Check that positions are roughly evenly distributed
        for i in 0..4 {
            let next = (i + 1) % 4;
            let angle_diff = (positions[i].angle() - positions[next].angle()).abs();
            // Should be approximately 90 degrees (π/2)
            assert!(angle_diff > 1.0 || angle_diff < 0.1);
        }
    }

    #[test]
    fn test_spawn_positions_in_safe_zone() {
        let positions = spawn_positions(10);

        let arena = Arena::default();
        for pos in positions {
            let zone = get_zone(pos, &arena);
            assert!(zone.is_safe());
        }
    }

    #[test]
    fn test_random_spawn_in_zone() {
        for _ in 0..100 {
            let pos = random_spawn_position();
            let dist = pos.length();
            assert!(dist >= crate::game::constants::spawn::ZONE_MIN);
            assert!(dist <= crate::game::constants::spawn::ZONE_MAX);
        }
    }

    #[test]
    fn test_arena_radii_shrink() {
        let mut state = create_test_state();
        let initial_escape = state.arena.escape_radius;

        state.arena.collapse_phase = 4;
        update_arena_radii(&mut state);

        assert!(state.arena.escape_radius < initial_escape);
    }

    #[test]
    fn test_current_safe_radius() {
        let arena = Arena::default();
        let safe_radius = arena.current_safe_radius();

        assert_eq!(safe_radius, ESCAPE_RADIUS);

        let mut arena2 = Arena::default();
        arena2.collapse_phase = 4;
        let safe_radius2 = arena2.current_safe_radius();

        assert!(safe_radius2 < safe_radius);
    }
}
