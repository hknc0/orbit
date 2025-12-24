//! Arena management system
//!
//! Handles zone collapse, player boundary checks, and arena events.

#![allow(dead_code)] // Arena utilities and event variants

use crate::game::constants::arena::*;
use crate::game::constants::spawn::RESPAWN_DELAY;
use crate::game::state::{GameState, MatchPhase};

// ============================================================================
// Arena System Constants
// ============================================================================

/// Collapse progress shrink factor for escape/outer/middle radii (80% shrink at max collapse)
const COLLAPSE_SHRINK_FACTOR: f32 = 0.8;

/// Collapse progress shrink factor for inner radius (60% shrink at max collapse)
const INNER_COLLAPSE_SHRINK_FACTOR: f32 = 0.6;

/// Distance divisor for progressive drain rate increase outside safe zone
/// Mass drain increases by 1x for each 100 units beyond safe radius
const DRAIN_RATE_DISTANCE_DIVISOR: f32 = 100.0;

/// Supermassive black hole safe spawn multiplier (3x core radius for safety margin)
const SUPERMASSIVE_SAFE_SPAWN_MULTIPLIER: f32 = 3.0;

/// Minimum spawn radius as fraction of outer radius
const SPAWN_RADIUS_MIN_FRACTION: f32 = 0.2;

/// Maximum spawn radius as fraction of outer radius
const SPAWN_RADIUS_MAX_FRACTION: f32 = 0.8;

/// Distance threshold for filtering orbital wells (wells near center are skipped)
const ORBITAL_WELL_POSITION_THRESHOLD: f32 = 10.0;

/// Core radius multiplier for filtering supermassive well
const ORBITAL_WELL_CORE_FILTER_MULTIPLIER: f32 = 1.5;

/// Multiplier for calculating safe spawn distance from central well
const SAFE_SPAWN_RADIUS_MULTIPLIER: f32 = 4.0;

/// Maximum spawn attempts near well before fallback
const WELL_SPAWN_MAX_ATTEMPTS: u32 = 20;

/// Safety margin multiplier for spawn position distance from well cores
const SPAWN_CORE_SAFETY_MARGIN: f32 = 1.5;

// Zone danger levels
/// Danger level for core zone (instant death)
const ZONE_DANGER_CORE: f32 = 1.0;
/// Danger level for inner zone (safe)
const ZONE_DANGER_INNER: f32 = 0.0;
/// Danger level for middle zone (very low risk)
const ZONE_DANGER_MIDDLE: f32 = 0.1;
/// Danger level for outer zone (low risk)
const ZONE_DANGER_OUTER: f32 = 0.3;
/// Danger level for escape zone (moderate risk)
const ZONE_DANGER_ESCAPE: f32 = 0.6;
/// Danger level for outside zone (high risk)
const ZONE_DANGER_OUTSIDE: f32 = 0.9;

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
    state.arena.escape_radius = ESCAPE_RADIUS * (1.0 - progress * COLLAPSE_SHRINK_FACTOR);
    state.arena.outer_radius = OUTER_RADIUS * (1.0 - progress * COLLAPSE_SHRINK_FACTOR);
    state.arena.middle_radius = MIDDLE_RADIUS * (1.0 - progress * COLLAPSE_SHRINK_FACTOR);
    state.arena.inner_radius = INNER_RADIUS * (1.0 - progress * INNER_COLLAPSE_SHRINK_FACTOR);

    // Core doesn't change
}

/// Check player positions against arena boundaries
fn check_player_boundaries(state: &mut GameState, dt: f32) -> Vec<ArenaEvent> {
    let mut events = Vec::new();
    let safe_radius = state.arena.current_safe_radius();
    let wells: Vec<_> = state.arena.gravity_wells.values().cloned().collect();

    for player in state.players.values_mut() {
        if !player.alive {
            continue;
        }

        // Check against all gravity well cores (instant death zones)
        // Use squared distance to avoid sqrt()
        let mut in_core = false;
        for well in &wells {
            let dist_sq = player.position.distance_sq_to(well.position);
            let core_radius_sq = well.core_radius * well.core_radius;
            if dist_sq < core_radius_sq {
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
        // Skip mass drain for spawn-protected players (they're invulnerable)
        if player.spawn_protection > 0.0 {
            continue;
        }

        let distance_from_center = player.position.length();
        if distance_from_center > safe_radius {
            let excess = distance_from_center - safe_radius;
            let drain_rate = ESCAPE_MASS_DRAIN * (1.0 + excess / DRAIN_RATE_DISTANCE_DIVISOR); // Faster drain farther out
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
            Zone::Core => ZONE_DANGER_CORE,
            Zone::Inner => ZONE_DANGER_INNER,
            Zone::Middle => ZONE_DANGER_MIDDLE,
            Zone::Outer => ZONE_DANGER_OUTER,
            Zone::Escape => ZONE_DANGER_ESCAPE,
            Zone::Outside => ZONE_DANGER_OUTSIDE,
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
    use crate::game::constants::arena::{CORE_RADIUS, OUTER_RADIUS};
    use rand::Rng;

    let mut rng = rand::thread_rng();

    // Minimum spawn radius must be outside the supermassive black hole death zone
    // Supermassive core is CORE_RADIUS * 2.5 = 125 units, so use 3.0x for safety margin
    let supermassive_safe_radius = CORE_RADIUS * SUPERMASSIVE_SAFE_SPAWN_MULTIPLIER;

    // Spawn across a wider range (20% to 80% of outer radius, but never inside death zone)
    let min_radius = (OUTER_RADIUS * SPAWN_RADIUS_MIN_FRACTION * scale).max(supermassive_safe_radius);
    let max_radius = OUTER_RADIUS * SPAWN_RADIUS_MAX_FRACTION * scale;

    let angle = rng.gen_range(0.0..std::f32::consts::TAU);
    let radius = rng.gen_range(min_radius..max_radius);

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
/// Optionally bounded by max_radius to keep spawns within arena
pub fn spawn_near_well(wells: &[crate::game::state::GravityWell]) -> crate::util::vec2::Vec2 {
    spawn_near_well_bounded(wells, None)
}

/// Calculate a spawn position near a random gravity well with optional max radius
pub fn spawn_near_well_bounded(
    wells: &[crate::game::state::GravityWell],
    max_radius: Option<f32>,
) -> crate::util::vec2::Vec2 {
    use crate::game::constants::spawn::{ZONE_MAX, ZONE_MIN};
    use crate::game::constants::arena::CORE_RADIUS;
    use rand::Rng;

    // Filter out the central supermassive black hole (at origin with large core)
    // to distribute players across orbital wells instead of clustering at center
    let orbital_wells: Vec<_> = wells
        .iter()
        .filter(|w| w.position.length() > ORBITAL_WELL_POSITION_THRESHOLD || w.core_radius <= CORE_RADIUS * ORBITAL_WELL_CORE_FILTER_MULTIPLIER)
        .collect();

    if orbital_wells.is_empty() {
        // No orbital wells - spawn at safe distance from central well
        let mut rng = rand::thread_rng();
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        // Use the first well (central) core radius for safe distance calculation
        let safe_radius = wells.first()
            .map(|w| w.core_radius * SAFE_SPAWN_RADIUS_MULTIPLIER)
            .unwrap_or(CORE_RADIUS * SAFE_SPAWN_RADIUS_MULTIPLIER);
        return crate::util::vec2::Vec2::from_angle(angle) * safe_radius;
    }

    let mut rng = rand::thread_rng();

    // Try multiple times to find a safe spawn position
    for _ in 0..WELL_SPAWN_MAX_ATTEMPTS {
        // Pick a random orbital well (not the central black hole)
        let well = orbital_wells[rng.gen_range(0..orbital_wells.len())];

        // Spawn in orbit zone around that well
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let radius = rng.gen_range(ZONE_MIN..ZONE_MAX);
        let offset = crate::util::vec2::Vec2::from_angle(angle) * radius;

        let spawn_pos = well.position + offset;

        // CRITICAL: Verify spawn position is NOT inside ANY gravity well's core
        // This prevents spawning inside the supermassive black hole when an orbital
        // well is close to center and the angle points inward
        let is_safe = wells.iter().all(|w| {
            let dist_sq = spawn_pos.distance_sq_to(w.position);
            // Use 1.5x core radius as safety margin
            let safe_radius = w.core_radius * SPAWN_CORE_SAFETY_MARGIN;
            dist_sq > safe_radius * safe_radius
        });

        // Also check max radius constraint if provided
        let within_bounds = max_radius
            .map(|r| spawn_pos.length() <= r)
            .unwrap_or(true);

        if is_safe && within_bounds {
            return spawn_pos;
        }
    }

    // Fallback: spawn near a random orbital well without safety checks
    // This keeps players distributed across wells rather than clustering at center
    if !orbital_wells.is_empty() {
        let well = orbital_wells[rng.gen_range(0..orbital_wells.len())];
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let radius = rng.gen_range(ZONE_MIN..ZONE_MAX);
        return well.position + crate::util::vec2::Vec2::from_angle(angle) * radius;
    }

    // Last resort: spawn at safe distance from center (for single-well arenas)
    let angle = rng.gen_range(0.0..std::f32::consts::TAU);
    let radius = CORE_RADIUS * SAFE_SPAWN_RADIUS_MULTIPLIER; // Safe distance from central well
    crate::util::vec2::Vec2::from_angle(angle) * radius
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
            dist_a.partial_cmp(&dist_b).unwrap_or(std::cmp::Ordering::Equal)
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

/// Calculate spawn positions for multiple players randomly distributed across the arena
pub fn spawn_positions(count: usize) -> Vec<crate::util::vec2::Vec2> {
    use crate::game::constants::arena::{CORE_RADIUS, OUTER_RADIUS};
    use rand::Rng;

    let mut rng = rand::thread_rng();

    // Minimum spawn radius must be outside the supermassive black hole death zone
    // Supermassive core is CORE_RADIUS * 2.5 = 125 units, so use 3.0x for safety margin
    let supermassive_safe_radius = CORE_RADIUS * SUPERMASSIVE_SAFE_SPAWN_MULTIPLIER;

    // Spawn across a wider range of the arena (20% to 80% of outer radius, but never inside death zone)
    let min_radius = (OUTER_RADIUS * SPAWN_RADIUS_MIN_FRACTION).max(supermassive_safe_radius);
    let max_radius = OUTER_RADIUS * SPAWN_RADIUS_MAX_FRACTION;

    (0..count)
        .map(|_| {
            // Random angle
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            // Random radius across the arena
            let radius = rng.gen_range(min_radius..max_radius);
            crate::util::vec2::Vec2::from_angle(angle) * radius
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::state::{Arena, Player};
    use crate::util::vec2::Vec2;

    fn create_test_state() -> (GameState, uuid::Uuid) {
        let mut state = GameState::new();
        state.match_state.phase = MatchPhase::Playing;
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
        let (mut state, player_id) = create_test_state();
        state.get_player_mut(player_id).unwrap().position = Vec2::new(25.0, 0.0); // Inside core

        let events = update(&mut state, 0.1);

        assert!(!state.get_player(player_id).unwrap().alive);
        assert!(events
            .iter()
            .any(|e| matches!(e, ArenaEvent::PlayerEnteredCore { .. })));
    }

    #[test]
    fn test_outside_drains_mass() {
        let (mut state, player_id) = create_test_state();
        state.get_player_mut(player_id).unwrap().position = Vec2::new(1000.0, 0.0); // Outside arena

        let initial_mass = state.get_player(player_id).unwrap().mass;
        let events = update(&mut state, 0.1);

        assert!(state.get_player(player_id).unwrap().mass < initial_mass);
        assert!(events
            .iter()
            .any(|e| matches!(e, ArenaEvent::PlayerOutsideArena { .. })));
    }

    #[test]
    fn test_collapse_disabled_for_eternal_mode() {
        let (mut state, _) = create_test_state();
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
        let (mut state, _) = create_test_state();
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
        use crate::game::constants::arena::{CORE_RADIUS, OUTER_RADIUS};

        let positions = spawn_positions(10);

        // Minimum is whichever is larger: 20% of outer radius or supermassive safe distance
        let supermassive_safe_radius = CORE_RADIUS * 3.0;
        let min_radius = (OUTER_RADIUS * 0.2).max(supermassive_safe_radius);
        let max_radius = OUTER_RADIUS * 0.8;

        for pos in positions {
            let dist = pos.length();
            assert!(dist >= min_radius * 0.9, "Position too close to center: {}", dist);
            assert!(dist <= max_radius * 1.1, "Position too far from center: {}", dist);
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
        use crate::game::constants::arena::{CORE_RADIUS, OUTER_RADIUS};

        // Minimum is whichever is larger: 20% of outer radius or supermassive safe distance
        let supermassive_safe_radius = CORE_RADIUS * 3.0;
        let min_radius = (OUTER_RADIUS * 0.2).max(supermassive_safe_radius);
        let max_radius = OUTER_RADIUS * 0.8;

        for _ in 0..100 {
            let pos = random_spawn_position();
            let dist = pos.length();
            assert!(dist >= min_radius * 0.9, "Position too close: {}", dist);
            assert!(dist <= max_radius * 1.1, "Position too far: {}", dist);
        }
    }

    #[test]
    fn test_spawn_outside_supermassive_death_zone() {
        use crate::game::constants::arena::CORE_RADIUS;

        // Supermassive black hole death zone is CORE_RADIUS * 2.5 = 125 units
        let supermassive_core = CORE_RADIUS * 2.5;

        // Test random_spawn_position
        for _ in 0..100 {
            let pos = random_spawn_position();
            let dist = pos.length();
            assert!(
                dist > supermassive_core,
                "Spawn position {} units from center is inside supermassive death zone ({})",
                dist,
                supermassive_core
            );
        }

        // Test spawn_positions
        let positions = spawn_positions(50);
        for pos in positions {
            let dist = pos.length();
            assert!(
                dist > supermassive_core,
                "Spawn position {} units from center is inside supermassive death zone ({})",
                dist,
                supermassive_core
            );
        }
    }

    #[test]
    fn test_arena_radii_shrink() {
        let (mut state, _) = create_test_state();
        let initial_escape = state.arena.escape_radius;

        state.arena.collapse_phase = 4;
        update_arena_radii(&mut state);

        assert!(state.arena.escape_radius < initial_escape);
    }

    #[test]
    fn test_spawn_protection_prevents_arena_mass_drain() {
        use crate::game::constants::mass::STARTING;
        use crate::game::constants::physics::DT;

        let (mut state, _) = create_test_state();

        // Create a player outside the safe zone but with spawn protection
        let player_id = uuid::Uuid::new_v4();
        let player = crate::game::state::Player {
            id: player_id,
            name: "Test".to_string(),
            position: Vec2::new(10000.0, 0.0), // Way outside safe zone
            velocity: Vec2::ZERO,
            rotation: 0.0,
            mass: STARTING,
            alive: true,
            kills: 0,
            deaths: 0,
            spawn_protection: 3.0, // Has spawn protection
            is_bot: false,
            color_index: 0,
            respawn_timer: 0.0,
        };
        state.add_player(player);

        let initial_mass = state.get_player(player_id).unwrap().mass;

        // Run arena update
        let events = update(&mut state, DT);

        // Mass should NOT have drained due to spawn protection
        let final_mass = state.get_player(player_id).unwrap().mass;
        assert_eq!(
            final_mass, initial_mass,
            "Mass should not drain during spawn protection: {} -> {}",
            initial_mass, final_mass
        );

        // Should not generate OutsideArena event for spawn-protected player
        let outside_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, ArenaEvent::PlayerOutsideArena { .. }))
            .collect();
        assert!(
            outside_events.is_empty(),
            "Should not generate OutsideArena event for spawn-protected player"
        );
    }

    #[test]
    fn test_arena_mass_drain_without_spawn_protection() {
        use crate::game::constants::mass::STARTING;
        use crate::game::constants::physics::DT;

        let (mut state, _) = create_test_state();

        // Create a player outside the safe zone WITHOUT spawn protection
        let player_id = uuid::Uuid::new_v4();
        let player = crate::game::state::Player {
            id: player_id,
            name: "Test".to_string(),
            position: Vec2::new(10000.0, 0.0), // Way outside safe zone
            velocity: Vec2::ZERO,
            rotation: 0.0,
            mass: STARTING,
            alive: true,
            kills: 0,
            deaths: 0,
            spawn_protection: 0.0, // NO spawn protection
            is_bot: false,
            color_index: 0,
            respawn_timer: 0.0,
        };
        state.add_player(player);

        let initial_mass = state.get_player(player_id).unwrap().mass;

        // Run arena update
        let events = update(&mut state, DT);

        // Mass SHOULD have drained
        let final_mass = state.get_player(player_id).unwrap().mass;
        assert!(
            final_mass < initial_mass,
            "Mass should drain when outside safe zone: {} -> {}",
            initial_mass, final_mass
        );

        // Should generate OutsideArena event
        let outside_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, ArenaEvent::PlayerOutsideArena { .. }))
            .collect();
        assert!(
            !outside_events.is_empty(),
            "Should generate OutsideArena event for unprotected player outside safe zone"
        );
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

    #[test]
    fn test_spawn_near_well_avoids_all_death_zones() {
        use crate::game::state::GravityWell;
        use crate::game::constants::arena::CORE_RADIUS;
        use crate::game::constants::physics::CENTRAL_MASS;

        // Create a realistic well setup with supermassive at center
        // and an orbital well close enough that some spawn angles could be dangerous
        let supermassive_core = CORE_RADIUS * 2.5; // 125 units
        let wells = vec![
            // Supermassive at center
            GravityWell::new(0, Vec2::ZERO, CENTRAL_MASS * 3.0, supermassive_core),
            // Orbital well at 300 units from center - close enough that
            // spawning 250-350 units toward center could enter death zone
            GravityWell::new(1, Vec2::new(300.0, 0.0), CENTRAL_MASS, CORE_RADIUS),
            // Another orbital well
            GravityWell::new(2, Vec2::new(-400.0, 200.0), CENTRAL_MASS, CORE_RADIUS),
        ];

        // Run many spawn attempts to verify none land in death zones
        for _ in 0..200 {
            let pos = spawn_near_well(&wells);

            // Verify not inside ANY gravity well's core
            for well in &wells {
                let dist = (pos - well.position).length();
                assert!(
                    dist > well.core_radius,
                    "Spawn position {:?} is inside well at {:?} (dist {} < core {})",
                    pos,
                    well.position,
                    dist,
                    well.core_radius
                );
            }
        }
    }

    #[test]
    fn test_spawn_near_well_with_very_close_orbital() {
        use crate::game::state::GravityWell;
        use crate::game::constants::arena::CORE_RADIUS;
        use crate::game::constants::physics::CENTRAL_MASS;

        // Edge case: orbital well very close to supermassive death zone
        // This simulates the worst case scenario
        let supermassive_core = CORE_RADIUS * 2.5; // 125 units
        let wells = vec![
            GravityWell::new(0, Vec2::ZERO, CENTRAL_MASS * 3.0, supermassive_core),
            // Orbital at 200 units - spawn at 250 toward center would be at -50 from origin!
            GravityWell::new(1, Vec2::new(200.0, 0.0), CENTRAL_MASS, CORE_RADIUS),
        ];

        for _ in 0..100 {
            let pos = spawn_near_well(&wells);

            // Must not be inside supermassive
            let dist_from_center = pos.length();
            assert!(
                dist_from_center > supermassive_core,
                "Spawn at {:?} is inside supermassive death zone (dist {} < {})",
                pos,
                dist_from_center,
                supermassive_core
            );

            // Must not be inside the orbital well either
            let dist_from_orbital = (pos - Vec2::new(200.0, 0.0)).length();
            assert!(
                dist_from_orbital > CORE_RADIUS,
                "Spawn at {:?} is inside orbital death zone",
                pos
            );
        }
    }

    #[test]
    fn test_spawn_positions_within_safe_radius() {
        use crate::game::state::Arena;

        // Simulate game with different player counts
        for player_count in [10, 50, 100, 200, 500] {
            let mut arena = Arena::default();
            arena.update_for_player_count(player_count);

            let safe_radius = arena.current_safe_radius();
            let wells: Vec<_> = arena.gravity_wells.values().cloned().collect();

            // Test many spawn positions using bounded spawn
            for _ in 0..50 {
                let pos = spawn_near_well_bounded(&wells, Some(safe_radius));
                let dist_from_center = pos.length();

                assert!(
                    dist_from_center <= safe_radius,
                    "Player count {}: Spawn at {:?} (dist {:.1}) is OUTSIDE safe radius {:.1}. \
                     Wells: {:?}",
                    player_count,
                    pos,
                    dist_from_center,
                    safe_radius,
                    wells.iter().map(|w| (w.position, w.core_radius)).collect::<Vec<_>>()
                );
            }
        }
    }
}
