//! Delta compression for network optimization
//!
//! This module provides delta compression for game state updates, sending only
//! the differences between snapshots instead of full state. Combined with
//! distance-based rate limiting, this significantly reduces network bandwidth
//! while maintaining visual quality.
//!
//! Key features:
//! - Epsilon-based change detection (avoids sending tiny changes)
//! - Distance-based rate limiting (close entities get more updates)
//! - Reuses existing DeltaUpdate protocol structures
//! - Maintains full f32 precision for pixel-perfect quality

use std::collections::{HashMap, HashSet};

use crate::game::state::PlayerId;
use crate::game::systems::ai_soa::{
    DEFAULT_DORMANT_UPDATE_INTERVAL, DEFAULT_LOD_FULL_RADIUS, DEFAULT_LOD_REDUCED_RADIUS,
    DEFAULT_REDUCED_UPDATE_INTERVAL,
};
use crate::net::protocol::{
    DeltaUpdate, GameSnapshot, PlayerDelta, PlayerSnapshot, ProjectileDelta, ProjectileSnapshot,
};
use crate::util::vec2::Vec2;

// ============================================================================
// Change Detection Thresholds
// ============================================================================

/// Position change threshold (world units)
/// Below this, position is considered unchanged
const POSITION_EPSILON: f32 = 0.1;

/// Velocity change threshold (units/second)
/// Below this, velocity is considered unchanged
const VELOCITY_EPSILON: f32 = 0.5;

/// Rotation change threshold (radians, ~0.5 degrees)
/// Below this, rotation is considered unchanged
const ROTATION_EPSILON: f32 = 0.01;

/// Mass change threshold (mass units)
/// Below this, mass is considered unchanged
const MASS_EPSILON: f32 = 0.1;

// ============================================================================
// Rate Limiting (currently disabled - kept for potential future use)
// ============================================================================

/// Determine the update interval (in ticks) for an entity based on its distance
/// from the viewer. Uses the same LOD thresholds as the bot AI system.
///
/// NOTE: Rate limiting is currently disabled as it caused visual jumping.
/// All entities update at full rate (30Hz) for smooth interpolation.
///
/// Returns:
/// - 1 for close entities (full rate, every tick = 30Hz)
/// - 4 for medium distance (reduced rate = 7.5Hz)
/// - 8 for far entities (dormant rate = 3.75Hz)
#[allow(dead_code)]
pub fn get_update_interval(distance: f32) -> u32 {
    if distance <= DEFAULT_LOD_FULL_RADIUS {
        1 // Every tick (30Hz)
    } else if distance <= DEFAULT_LOD_REDUCED_RADIUS {
        DEFAULT_REDUCED_UPDATE_INTERVAL // Every 4 ticks (7.5Hz)
    } else {
        DEFAULT_DORMANT_UPDATE_INTERVAL // Every 8 ticks (3.75Hz)
    }
}

/// Check if an entity should be updated this tick based on distance-based rate limiting.
///
/// NOTE: Rate limiting is currently disabled as it caused visual jumping.
/// All entities update at full rate (30Hz) for smooth interpolation.
///
/// # Arguments
/// - `entity_id`: The entity's unique ID
/// - `distance`: Distance from the viewer
/// - `current_tick`: Current game tick
/// - `last_updates`: Map of entity ID -> last update tick
///
/// # Returns
/// `true` if the entity should be included in this update
#[allow(dead_code)]
pub fn should_update_entity(
    entity_id: &PlayerId,
    distance: f32,
    current_tick: u64,
    last_updates: &HashMap<PlayerId, u64>,
) -> bool {
    // If no history, always include (first update for this entity)
    let last_tick = match last_updates.get(entity_id) {
        Some(&tick) => tick,
        None => return true, // Always include entities we haven't sent before
    };

    let interval = get_update_interval(distance) as u64;

    // Update if enough ticks have passed since last update
    current_tick >= last_tick + interval
}

/// Get the rate tier for metrics tracking
/// Returns: 0 = full, 1 = reduced, 2 = dormant
pub fn get_rate_tier(distance: f32) -> u8 {
    if distance <= DEFAULT_LOD_FULL_RADIUS {
        0 // Full rate
    } else if distance <= DEFAULT_LOD_REDUCED_RADIUS {
        1 // Reduced rate
    } else {
        2 // Dormant rate
    }
}

// ============================================================================
// Delta Generation
// ============================================================================

/// Statistics about delta generation for metrics
#[derive(Debug, Default, Clone)]
pub struct DeltaStats {
    pub players_included: usize,
    pub players_skipped: usize,
    pub full_rate_count: usize,
    pub reduced_rate_count: usize,
    pub dormant_rate_count: usize,
}

/// Generate a delta update from a base snapshot to the current snapshot.
///
/// # Arguments
/// - `base`: The previous snapshot (used as reference)
/// - `current`: The current snapshot
/// - `viewer_position`: Position of the viewing player (for distance calculation)
/// - `current_tick`: Current game tick (used for delta metadata)
///
/// # Returns
/// - `Some((DeltaUpdate, DeltaStats))` if there are changes to send
/// - `None` if no changes detected (skip sending entirely)
pub fn generate_delta(
    base: &GameSnapshot,
    current: &GameSnapshot,
    viewer_position: Vec2,
    _current_tick: u64,
) -> Option<(DeltaUpdate, DeltaStats)> {
    let mut player_updates = Vec::with_capacity(current.players.len());
    let mut stats = DeltaStats::default();

    // Build lookup map for base players
    let base_players: HashMap<PlayerId, &PlayerSnapshot> =
        base.players.iter().map(|p| (p.id, p)).collect();

    for player in &current.players {
        // Track distance tier for metrics (no rate limiting - causes visual jumping)
        let distance = (player.position - viewer_position).length();
        match get_rate_tier(distance) {
            0 => stats.full_rate_count += 1,
            1 => stats.reduced_rate_count += 1,
            _ => stats.dormant_rate_count += 1,
        }

        // Generate delta - compare against base snapshot
        // Delta compression only sends changed fields, preserving smooth interpolation
        let delta = if let Some(base_player) = base_players.get(&player.id) {
            generate_player_delta(base_player, player)
        } else {
            // New player - send full state as delta
            Some(PlayerDelta {
                id: player.id,
                position: Some(player.position),
                velocity: Some(player.velocity),
                rotation: Some(player.rotation),
                mass: Some(player.mass),
                alive: Some(player.alive()),
                kills: Some(player.kills),
            })
        };

        if let Some(d) = delta {
            stats.players_included += 1;
            player_updates.push(d);
        }
    }

    // Generate projectile deltas (projectiles change every tick, include all)
    let projectile_updates = generate_projectile_deltas(base, current);

    // Find removed projectiles
    let base_projectile_ids: HashSet<u64> = base.projectiles.iter().map(|p| p.id).collect();
    let current_projectile_ids: HashSet<u64> = current.projectiles.iter().map(|p| p.id).collect();
    let removed_projectiles: Vec<u64> = base_projectile_ids
        .difference(&current_projectile_ids)
        .copied()
        .collect();

    // Only return delta if there's something to send
    if player_updates.is_empty()
        && projectile_updates.is_empty()
        && removed_projectiles.is_empty()
    {
        return None;
    }

    Some((
        DeltaUpdate {
            tick: current.tick,
            base_tick: base.tick,
            player_updates,
            projectile_updates,
            removed_projectiles,
            // Include full debris list (debris moves slowly, full list is efficient)
            debris: current.debris.clone(),
        },
        stats,
    ))
}

/// Generate a delta for a single player by comparing base and current state.
/// Returns `None` if no changes detected (within epsilon thresholds).
fn generate_player_delta(base: &PlayerSnapshot, current: &PlayerSnapshot) -> Option<PlayerDelta> {
    let mut delta = PlayerDelta {
        id: current.id,
        position: None,
        velocity: None,
        rotation: None,
        mass: None,
        alive: None,
        kills: None,
    };
    let mut has_changes = false;

    // Position change detection (with epsilon)
    let pos_diff = current.position - base.position;
    if pos_diff.length_sq() > POSITION_EPSILON * POSITION_EPSILON {
        delta.position = Some(current.position);
        has_changes = true;
    }

    // Velocity change detection
    let vel_diff = current.velocity - base.velocity;
    if vel_diff.length_sq() > VELOCITY_EPSILON * VELOCITY_EPSILON {
        delta.velocity = Some(current.velocity);
        has_changes = true;
    }

    // Rotation change detection
    if (current.rotation - base.rotation).abs() > ROTATION_EPSILON {
        delta.rotation = Some(current.rotation);
        has_changes = true;
    }

    // Mass change detection
    if (current.mass - base.mass).abs() > MASS_EPSILON {
        delta.mass = Some(current.mass);
        has_changes = true;
    }

    // Discrete state changes (always include if changed)
    if current.alive() != base.alive() {
        delta.alive = Some(current.alive());
        has_changes = true;
    }

    if current.kills != base.kills {
        delta.kills = Some(current.kills);
        has_changes = true;
    }

    if has_changes {
        Some(delta)
    } else {
        None
    }
}

/// Generate deltas for projectiles. Since projectiles move every tick,
/// we include all projectiles that exist in the current snapshot.
fn generate_projectile_deltas(base: &GameSnapshot, current: &GameSnapshot) -> Vec<ProjectileDelta> {
    let base_projectiles: HashMap<u64, &ProjectileSnapshot> =
        base.projectiles.iter().map(|p| (p.id, p)).collect();

    current
        .projectiles
        .iter()
        .filter_map(|proj| {
            // Only include if position changed from base (or new projectile)
            let should_include = base_projectiles.get(&proj.id).map_or(true, |base_proj| {
                let pos_diff = proj.position - base_proj.position;
                pos_diff.length_sq() > POSITION_EPSILON * POSITION_EPSILON
            });

            if should_include {
                Some(ProjectileDelta {
                    id: proj.id,
                    position: proj.position,
                    velocity: proj.velocity,
                })
            } else {
                None
            }
        })
        .collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::state::MatchPhase;
    use crate::net::protocol::player_flags;
    use uuid::Uuid;

    fn create_player(id: Uuid, position: Vec2, kills: u32) -> PlayerSnapshot {
        PlayerSnapshot {
            id,
            name: format!("Player_{}", kills),
            position,
            velocity: Vec2::ZERO,
            rotation: 0.0,
            mass: 100.0,
            flags: player_flags::ALIVE,
            kills,
            deaths: 0,
            color_index: 0,
        }
    }

    fn create_projectile(id: u64, position: Vec2) -> ProjectileSnapshot {
        ProjectileSnapshot {
            id,
            owner_id: Uuid::new_v4(),
            position,
            velocity: Vec2::new(100.0, 0.0),
            mass: 10.0,
        }
    }

    fn create_snapshot(players: Vec<PlayerSnapshot>) -> GameSnapshot {
        let player_count = players.len() as u32;
        GameSnapshot {
            tick: 100,
            match_phase: MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players,
            projectiles: vec![],
            debris: vec![],
            arena_collapse_phase: 0,
            arena_safe_radius: 800.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: player_count,
            total_alive: player_count,
            density_grid: vec![],
            notable_players: vec![],
            echo_client_time: 0,
            ai_status: None,
        }
    }

    // ========================================================================
    // Rate Limiting Tests
    // ========================================================================

    #[test]
    fn test_get_update_interval_full_rate() {
        // Close entities should get full rate (every tick)
        assert_eq!(get_update_interval(0.0), 1);
        assert_eq!(get_update_interval(100.0), 1);
        assert_eq!(get_update_interval(500.0), 1);
    }

    #[test]
    fn test_get_update_interval_reduced_rate() {
        // Medium distance should get reduced rate
        assert_eq!(get_update_interval(501.0), DEFAULT_REDUCED_UPDATE_INTERVAL);
        assert_eq!(get_update_interval(1000.0), DEFAULT_REDUCED_UPDATE_INTERVAL);
        assert_eq!(get_update_interval(2000.0), DEFAULT_REDUCED_UPDATE_INTERVAL);
    }

    #[test]
    fn test_get_update_interval_dormant_rate() {
        // Far entities should get dormant rate
        assert_eq!(get_update_interval(2001.0), DEFAULT_DORMANT_UPDATE_INTERVAL);
        assert_eq!(get_update_interval(5000.0), DEFAULT_DORMANT_UPDATE_INTERVAL);
        assert_eq!(get_update_interval(10000.0), DEFAULT_DORMANT_UPDATE_INTERVAL);
    }

    #[test]
    fn test_should_update_entity_no_history() {
        let last_updates = HashMap::new();
        let entity_id = Uuid::new_v4();

        // Without history, always update
        assert!(should_update_entity(&entity_id, 100.0, 1, &last_updates));
        assert!(should_update_entity(&entity_id, 1000.0, 1, &last_updates));
        assert!(should_update_entity(&entity_id, 5000.0, 1, &last_updates));
    }

    #[test]
    fn test_should_update_entity_respects_interval() {
        let entity_id = Uuid::new_v4();
        let mut last_updates = HashMap::new();
        last_updates.insert(entity_id, 100);

        // Full rate (interval 1): Should update on tick 101
        assert!(should_update_entity(&entity_id, 100.0, 101, &last_updates));

        // Reduced rate (interval 4): Should NOT update on tick 101-103
        assert!(!should_update_entity(&entity_id, 1000.0, 101, &last_updates));
        assert!(!should_update_entity(&entity_id, 1000.0, 103, &last_updates));
        // Should update on tick 104
        assert!(should_update_entity(&entity_id, 1000.0, 104, &last_updates));

        // Dormant rate (interval 8): Should update on tick 108
        assert!(!should_update_entity(&entity_id, 5000.0, 104, &last_updates));
        assert!(should_update_entity(&entity_id, 5000.0, 108, &last_updates));
    }

    #[test]
    fn test_get_rate_tier() {
        assert_eq!(get_rate_tier(0.0), 0);    // Full
        assert_eq!(get_rate_tier(500.0), 0);  // Full (boundary)
        assert_eq!(get_rate_tier(501.0), 1);  // Reduced
        assert_eq!(get_rate_tier(2000.0), 1); // Reduced (boundary)
        assert_eq!(get_rate_tier(2001.0), 2); // Dormant
        assert_eq!(get_rate_tier(10000.0), 2); // Dormant
    }

    // ========================================================================
    // Player Delta Generation Tests
    // ========================================================================

    #[test]
    fn test_no_changes_produces_no_delta() {
        let player = create_player(Uuid::new_v4(), Vec2::new(100.0, 100.0), 5);
        let delta = generate_player_delta(&player, &player);
        assert!(delta.is_none());
    }

    #[test]
    fn test_position_change_detected() {
        let id = Uuid::new_v4();
        let base = create_player(id, Vec2::new(100.0, 100.0), 5);
        let mut current = base.clone();
        current.position = Vec2::new(110.0, 100.0); // Moved 10 units

        let delta = generate_player_delta(&base, &current).unwrap();
        assert!(delta.position.is_some());
        assert!(delta.velocity.is_none());
        assert!(delta.rotation.is_none());
        assert!(delta.mass.is_none());
    }

    #[test]
    fn test_position_change_within_epsilon_ignored() {
        let id = Uuid::new_v4();
        let base = create_player(id, Vec2::new(100.0, 100.0), 5);
        let mut current = base.clone();
        current.position = Vec2::new(100.05, 100.05); // Tiny change

        let delta = generate_player_delta(&base, &current);
        assert!(delta.is_none());
    }

    #[test]
    fn test_velocity_change_detected() {
        let id = Uuid::new_v4();
        let base = create_player(id, Vec2::new(100.0, 100.0), 5);
        let mut current = base.clone();
        current.velocity = Vec2::new(50.0, 0.0);

        let delta = generate_player_delta(&base, &current).unwrap();
        assert!(delta.position.is_none());
        assert!(delta.velocity.is_some());
    }

    #[test]
    fn test_rotation_change_detected() {
        let id = Uuid::new_v4();
        let base = create_player(id, Vec2::new(100.0, 100.0), 5);
        let mut current = base.clone();
        current.rotation = 1.5; // Significant rotation change

        let delta = generate_player_delta(&base, &current).unwrap();
        assert!(delta.rotation.is_some());
    }

    #[test]
    fn test_mass_change_detected() {
        let id = Uuid::new_v4();
        let base = create_player(id, Vec2::new(100.0, 100.0), 5);
        let mut current = base.clone();
        current.mass = 150.0;

        let delta = generate_player_delta(&base, &current).unwrap();
        assert!(delta.mass.is_some());
    }

    #[test]
    fn test_alive_change_detected() {
        let id = Uuid::new_v4();
        let base = create_player(id, Vec2::new(100.0, 100.0), 5);
        let mut current = base.clone();
        current.flags = 0; // Dead

        let delta = generate_player_delta(&base, &current).unwrap();
        assert!(delta.alive.is_some());
        assert_eq!(delta.alive, Some(false));
    }

    #[test]
    fn test_kills_change_detected() {
        let id = Uuid::new_v4();
        let base = create_player(id, Vec2::new(100.0, 100.0), 5);
        let mut current = base.clone();
        current.kills = 6;

        let delta = generate_player_delta(&base, &current).unwrap();
        assert!(delta.kills.is_some());
        assert_eq!(delta.kills, Some(6));
    }

    #[test]
    fn test_multiple_changes_combined() {
        let id = Uuid::new_v4();
        let base = create_player(id, Vec2::new(100.0, 100.0), 5);
        let mut current = base.clone();
        current.position = Vec2::new(200.0, 200.0);
        current.velocity = Vec2::new(50.0, 50.0);
        current.kills = 10;

        let delta = generate_player_delta(&base, &current).unwrap();
        assert!(delta.position.is_some());
        assert!(delta.velocity.is_some());
        assert!(delta.kills.is_some());
        assert!(delta.rotation.is_none()); // Unchanged
        assert!(delta.mass.is_none());     // Unchanged
    }

    // ========================================================================
    // Full Delta Generation Tests
    // ========================================================================

    #[test]
    fn test_generate_delta_no_changes() {
        let snapshot = create_snapshot(vec![
            create_player(Uuid::new_v4(), Vec2::new(100.0, 100.0), 5),
        ]);

        let result = generate_delta(&snapshot, &snapshot, Vec2::ZERO, 100);
        assert!(result.is_none());
    }

    #[test]
    fn test_generate_delta_with_changes() {
        let id = Uuid::new_v4();
        let base = create_snapshot(vec![
            create_player(id, Vec2::new(100.0, 100.0), 5),
        ]);
        let mut current = base.clone();
        current.tick = 101;
        current.players[0].position = Vec2::new(200.0, 200.0);

        let (delta, stats) = generate_delta(&base, &current, Vec2::ZERO, 101).unwrap();

        assert_eq!(delta.tick, 101);
        assert_eq!(delta.base_tick, 100);
        assert_eq!(delta.player_updates.len(), 1);
        assert!(delta.player_updates[0].position.is_some());
        assert_eq!(stats.players_included, 1);
    }

    #[test]
    fn test_generate_delta_new_player() {
        let base = create_snapshot(vec![]);
        let new_player = create_player(Uuid::new_v4(), Vec2::new(100.0, 100.0), 0);
        let current = create_snapshot(vec![new_player.clone()]);

        let (delta, _) = generate_delta(&base, &current, Vec2::ZERO, 100).unwrap();

        assert_eq!(delta.player_updates.len(), 1);
        // New player should have all fields set
        let player_delta = &delta.player_updates[0];
        assert!(player_delta.position.is_some());
        assert!(player_delta.velocity.is_some());
        assert!(player_delta.rotation.is_some());
        assert!(player_delta.mass.is_some());
        assert!(player_delta.alive.is_some());
        assert!(player_delta.kills.is_some());
    }

    #[test]
    fn test_generate_delta_includes_all_players_for_smooth_interpolation() {
        // Rate limiting was removed to preserve smooth client-side interpolation
        // All players are now included in every delta (only changed fields sent)
        let id = Uuid::new_v4();
        let base = create_snapshot(vec![
            create_player(id, Vec2::new(5000.0, 0.0), 5), // Far player
        ]);
        let mut current = base.clone();
        current.tick = 101;
        current.players[0].position = Vec2::new(5010.0, 0.0); // Moved

        // Even distant players are included (no rate limiting)
        let (delta, stats) = generate_delta(&base, &current, Vec2::ZERO, 101).unwrap();
        assert_eq!(stats.players_included, 1);
        assert_eq!(stats.dormant_rate_count, 1); // Still tracks distance tier for metrics
        assert_eq!(delta.player_updates.len(), 1);
    }

    #[test]
    fn test_generate_delta_tracks_rate_tiers() {
        let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let base = create_snapshot(vec![
            create_player(ids[0], Vec2::new(100.0, 0.0), 0),   // Close
            create_player(ids[1], Vec2::new(1000.0, 0.0), 0),  // Medium
            create_player(ids[2], Vec2::new(5000.0, 0.0), 0),  // Far
        ]);
        let mut current = base.clone();
        current.tick = 101;
        // Move all players
        for p in &mut current.players {
            p.position.x += 10.0;
        }

        let (_, stats) = generate_delta(&base, &current, Vec2::ZERO, 101).unwrap();

        assert_eq!(stats.full_rate_count, 1);
        assert_eq!(stats.reduced_rate_count, 1);
        assert_eq!(stats.dormant_rate_count, 1);
    }

    // ========================================================================
    // Projectile Delta Tests
    // ========================================================================

    #[test]
    fn test_projectile_deltas() {
        let mut base = create_snapshot(vec![]);
        base.projectiles = vec![
            create_projectile(1, Vec2::new(100.0, 0.0)),
            create_projectile(2, Vec2::new(200.0, 0.0)),
        ];

        let mut current = base.clone();
        current.tick = 101;
        // Move projectile 1, leave projectile 2 unchanged
        current.projectiles[0].position = Vec2::new(110.0, 0.0);

        let deltas = generate_projectile_deltas(&base, &current);
        assert_eq!(deltas.len(), 1); // Only changed projectile
        assert_eq!(deltas[0].id, 1);
    }

    #[test]
    fn test_removed_projectiles_detected() {
        let mut base = create_snapshot(vec![]);
        base.projectiles = vec![
            create_projectile(1, Vec2::new(100.0, 0.0)),
            create_projectile(2, Vec2::new(200.0, 0.0)),
        ];

        let mut current = base.clone();
        current.tick = 101;
        // Remove projectile 1
        current.projectiles = vec![create_projectile(2, Vec2::new(200.0, 0.0))];

        // Add a player to ensure delta is generated
        let id = Uuid::new_v4();
        base.players = vec![create_player(id, Vec2::new(0.0, 0.0), 0)];
        current.players = vec![create_player(id, Vec2::new(10.0, 0.0), 0)];

        let (delta, _) = generate_delta(&base, &current, Vec2::ZERO, 101).unwrap();
        assert!(delta.removed_projectiles.contains(&1));
        assert!(!delta.removed_projectiles.contains(&2));
    }

    #[test]
    fn test_new_projectile_included() {
        let base = create_snapshot(vec![]);
        let mut current = base.clone();
        current.tick = 101;
        current.projectiles = vec![create_projectile(1, Vec2::new(100.0, 0.0))];

        // Add a player to ensure delta is generated
        let id = Uuid::new_v4();
        current.players = vec![create_player(id, Vec2::new(0.0, 0.0), 0)];

        let (delta, _) = generate_delta(&base, &current, Vec2::ZERO, 101).unwrap();

        assert_eq!(delta.projectile_updates.len(), 1);
        assert_eq!(delta.projectile_updates[0].id, 1);
    }
}
