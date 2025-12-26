//! Area of Interest (AOI) manager for network optimization
//!
//! Filters game state to only include entities relevant to each player,
//! dramatically reducing bandwidth usage at scale.

use std::cell::RefCell;

use smallvec::SmallVec;

use crate::game::state::PlayerId;
use crate::net::protocol::GameSnapshot;
use crate::util::vec2::Vec2;

// Thread-local reusable buffers to avoid per-filter allocations
thread_local! {
    /// Buffer for sorting players by score (top N calculation)
    static PLAYERS_BY_SCORE_BUFFER: RefCell<Vec<(PlayerId, u32)>> = RefCell::new(Vec::with_capacity(256));
    /// Buffer for storing nearby players with distances (for sorting)
    static NEARBY_WITH_DISTANCE_BUFFER: RefCell<Vec<(usize, f32)>> = RefCell::new(Vec::with_capacity(256));
}

// ============================================================================
// AOI System Constants
// ============================================================================

/// Default full detail radius - entities within this range get full update rate
const DEFAULT_FULL_DETAIL_RADIUS: f32 = 500.0;

/// Default extended radius - entities in this range get reduced detail
const DEFAULT_EXTENDED_RADIUS: f32 = 1000.0;

/// Default maximum entities to include per client (performance cap)
const DEFAULT_MAX_ENTITIES: usize = 100;

/// Default number of top players to always include (for leaderboard visibility)
const DEFAULT_ALWAYS_INCLUDE_TOP_N: usize = 5;

/// Lookahead time in seconds for velocity-based AOI expansion
/// Prevents pop-in when moving fast by looking ahead this many seconds
const VELOCITY_LOOKAHEAD_TIME: f32 = 2.0;

/// Maximum velocity expansion as fraction of extended radius
/// AOI can expand by up to 50% when moving at high speed
const VELOCITY_EXPANSION_MAX_RATIO: f32 = 0.5;

/// Projectile entity cap divisor (projectiles capped at max_entities / this value)
const PROJECTILE_CAP_DIVISOR: usize = 2;

/// AOI configuration
#[derive(Debug, Clone)]
pub struct AOIConfig {
    /// Full detail radius - entities within this range get full update rate
    pub full_detail_radius: f32,
    /// Extended radius - entities in this range get reduced detail
    pub extended_radius: f32,
    /// Maximum entities to include per client (performance cap)
    pub max_entities: usize,
    /// Always include top N players by score (for leaderboard visibility)
    pub always_include_top_n: usize,
}

impl Default for AOIConfig {
    fn default() -> Self {
        Self {
            full_detail_radius: DEFAULT_FULL_DETAIL_RADIUS,   // Full detail for nearby entities
            extended_radius: DEFAULT_EXTENDED_RADIUS,         // Reduced detail for medium range
            max_entities: DEFAULT_MAX_ENTITIES,               // Cap to prevent bandwidth explosion
            always_include_top_n: DEFAULT_ALWAYS_INCLUDE_TOP_N, // Always show top N players
        }
    }
}

/// Manages Area of Interest filtering for network optimization
pub struct AOIManager {
    config: AOIConfig,
}

impl AOIManager {
    pub fn new(config: AOIConfig) -> Self {
        Self { config }
    }

    /// Filter a game snapshot for a specific player based on their position and velocity
    ///
    /// Returns a personalized snapshot containing:
    /// - The player themselves (always)
    /// - Nearby players within AOI radius (expanded based on speed)
    /// - Top N players by score (for leaderboard)
    /// - Nearby projectiles
    /// - All gravity wells (they're sparse and important)
    ///
    /// AOI radius is expanded when player is moving fast to prevent pop-in:
    /// - At rest: uses configured extended_radius
    /// - At high speed: expanded by up to 50% in direction of movement
    pub fn filter_for_player(
        &self,
        player_id: PlayerId,
        player_position: Vec2,
        player_velocity: Vec2,
        full_snapshot: &GameSnapshot,
    ) -> GameSnapshot {
        // Expand AOI based on speed to prevent pop-in when moving fast
        // At 250 speed (max zoom out), player covers ~250 units/sec
        // Lookahead ~2 seconds of movement = 500 units extra radius
        let speed = player_velocity.length();
        let velocity_expansion = (speed * VELOCITY_LOOKAHEAD_TIME).min(self.config.extended_radius * VELOCITY_EXPANSION_MAX_RATIO);
        let effective_extended_radius = self.config.extended_radius + velocity_expansion;
        // OPTIMIZATION: Pre-compute squared radius to avoid sqrt in distance checks
        let effective_extended_radius_sq = effective_extended_radius * effective_extended_radius;
        let mut filtered_players = Vec::with_capacity(self.config.max_entities);
        let mut filtered_projectiles = Vec::with_capacity(self.config.max_entities);
        let mut filtered_debris = Vec::with_capacity(self.config.max_entities);

        // CRITICAL: First, find and add the local player BEFORE processing others
        // This ensures they're never excluded by the max_entities cap
        let mut player_found = false;
        for player in &full_snapshot.players {
            if player.id == player_id {
                filtered_players.push(player.clone());
                player_found = true;
                break;
            }
        }

        // Get top N players by score (for leaderboard visibility)
        // OPTIMIZATION: Use thread-local buffer to avoid per-filter allocation
        let top_player_ids: SmallVec<[PlayerId; 8]> = PLAYERS_BY_SCORE_BUFFER.with(|buffer_cell| {
            let mut buffer = buffer_cell.borrow_mut();
            buffer.clear();

            // Collect (id, kills) pairs for sorting
            buffer.extend(full_snapshot.players.iter().map(|p| (p.id, p.kills)));

            // Sort by kills descending
            buffer.sort_by(|a, b| b.1.cmp(&a.1));

            // Take top N player IDs
            buffer
                .iter()
                .take(self.config.always_include_top_n)
                .map(|(id, _)| *id)
                .collect()
        });

        // Add top players (skip self, already added)
        for player in &full_snapshot.players {
            if player.id == player_id {
                continue; // Already added
            }
            if top_player_ids.contains(&player.id) {
                filtered_players.push(player.clone());
            }
        }

        // Collect nearby players with their distances
        // CRITICAL: Sort by distance to ensure closest players are included first
        // This prevents far-away players from taking priority over nearby ones
        // when max_entities cap is reached (HashMap iteration order is arbitrary)
        // OPTIMIZATION: Use thread-local buffer with indices instead of references
        let nearby_indices: Vec<usize> = NEARBY_WITH_DISTANCE_BUFFER.with(|buffer_cell| {
            let mut buffer = buffer_cell.borrow_mut();
            buffer.clear();

            // Collect (index, distance_sq) pairs for players not already included
            for (idx, p) in full_snapshot.players.iter().enumerate() {
                if p.id == player_id || top_player_ids.contains(&p.id) {
                    continue;
                }
                let distance_sq = (p.position - player_position).length_sq();
                if distance_sq <= effective_extended_radius_sq {
                    buffer.push((idx, distance_sq));
                }
            }

            // Sort by squared distance (closest first) - sqrt is monotonic so order is preserved
            buffer.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            // Return sorted indices
            buffer.iter().map(|(idx, _)| *idx).collect()
        });

        // Add nearby players up to max_entities cap (closest first)
        for idx in nearby_indices {
            if filtered_players.len() >= self.config.max_entities {
                break;
            }
            filtered_players.push(full_snapshot.players[idx].clone());
        }

        // Fallback should never happen now, but keep for safety
        if !player_found {
            // Player not in snapshot - include all players (capped)
            filtered_players = full_snapshot.players
                .iter()
                .take(self.config.max_entities)
                .cloned()
                .collect();
        }

        // Filter projectiles by distance (using velocity-expanded radius)
        // OPTIMIZATION: Use length_sq() to avoid sqrt
        for proj in &full_snapshot.projectiles {
            let distance_sq = (proj.position - player_position).length_sq();
            if distance_sq <= effective_extended_radius_sq {
                filtered_projectiles.push(proj.clone());
            }

            // Cap projectiles too
            if filtered_projectiles.len() >= self.config.max_entities / PROJECTILE_CAP_DIVISOR {
                break;
            }
        }

        // Filter debris by distance (using velocity-expanded radius)
        // OPTIMIZATION: Use length_sq() to avoid sqrt
        for debris in &full_snapshot.debris {
            let distance_sq = (debris.position - player_position).length_sq();
            if distance_sq <= effective_extended_radius_sq {
                filtered_debris.push(debris.clone());
            }

            // Cap debris
            if filtered_debris.len() >= self.config.max_entities {
                break;
            }
        }

        GameSnapshot {
            tick: full_snapshot.tick,
            match_phase: full_snapshot.match_phase,
            match_time: full_snapshot.match_time,
            countdown: full_snapshot.countdown,
            players: filtered_players,
            projectiles: filtered_projectiles,
            debris: filtered_debris,
            arena_collapse_phase: full_snapshot.arena_collapse_phase,
            arena_safe_radius: full_snapshot.arena_safe_radius,
            arena_scale: full_snapshot.arena_scale,
            // Always include all gravity wells - they're sparse and important
            gravity_wells: full_snapshot.gravity_wells.clone(),
            // Preserve totals from full snapshot so UI shows correct counts
            total_players: full_snapshot.total_players,
            total_alive: full_snapshot.total_alive,
            // Preserve density grid for minimap heatmap
            density_grid: full_snapshot.density_grid.clone(),
            // Preserve notable players for minimap radar
            notable_players: full_snapshot.notable_players.clone(),
            // Set per-player in broadcast
            echo_client_time: 0,
            // Preserve AI status from full snapshot
            ai_status: full_snapshot.ai_status.clone(),
        }
    }

    /// Get statistics about a filtered snapshot
    pub fn snapshot_stats(original: &GameSnapshot, filtered: &GameSnapshot) -> AOIStats {
        AOIStats {
            original_players: original.players.len(),
            filtered_players: filtered.players.len(),
            original_projectiles: original.projectiles.len(),
            filtered_projectiles: filtered.projectiles.len(),
            reduction_percent: if original.players.len() > 0 {
                (1.0 - (filtered.players.len() as f32 / original.players.len() as f32)) * 100.0
            } else {
                0.0
            },
        }
    }
}

impl Default for AOIManager {
    fn default() -> Self {
        Self::new(AOIConfig::default())
    }
}

/// Statistics about AOI filtering
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AOIStats {
    pub original_players: usize,
    pub filtered_players: usize,
    pub original_projectiles: usize,
    pub filtered_projectiles: usize,
    pub reduction_percent: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::state::MatchPhase;
    use crate::net::protocol::{GravityWellSnapshot, PlayerSnapshot, ProjectileSnapshot};
    use uuid::Uuid;

    fn create_player_snapshot(id: Uuid, position: Vec2, kills: u32) -> PlayerSnapshot {
        use crate::net::protocol::player_flags;
        PlayerSnapshot {
            id,
            name: format!("Player_{}", kills),
            position,
            velocity: Vec2::ZERO,
            rotation: 0.0,
            mass: 100.0,
            flags: player_flags::ALIVE, // alive=true, spawn_protection=false, is_bot=false
            kills,
            deaths: 0,
            color_index: 0,
        }
    }

    fn create_projectile_snapshot(id: u64, position: Vec2) -> ProjectileSnapshot {
        ProjectileSnapshot {
            id,
            owner_id: Uuid::new_v4(),
            position,
            velocity: Vec2::new(100.0, 0.0),
            mass: 10.0,
        }
    }

    fn create_test_snapshot(player_count: usize) -> GameSnapshot {
        let mut players = Vec::with_capacity(player_count);
        for i in 0..player_count {
            let angle = (i as f32 / player_count as f32) * std::f32::consts::TAU;
            let radius = 200.0 + (i as f32 * 50.0);
            let pos = Vec2::new(angle.cos() * radius, angle.sin() * radius);
            players.push(create_player_snapshot(Uuid::new_v4(), pos, i as u32));
        }

        let player_len = players.len() as u32;
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
            total_players: player_len,
            total_alive: player_len,
            density_grid: vec![],
            notable_players: vec![],
            echo_client_time: 0,
            ai_status: None,
        }
    }

    #[test]
    fn test_aoi_filter_self_always_included() {
        let aoi = AOIManager::default();
        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(100.0, 100.0);

        let mut snapshot = create_test_snapshot(10);
        snapshot.players[0].id = player_id;
        snapshot.players[0].position = player_pos;

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, &snapshot);

        // Self should always be included
        assert!(filtered.players.iter().any(|p| p.id == player_id));
    }

    #[test]
    fn test_aoi_filter_distant_players_excluded() {
        let config = AOIConfig {
            full_detail_radius: 100.0,
            extended_radius: 200.0,
            max_entities: 50,
            always_include_top_n: 0,
        };
        let aoi = AOIManager::new(config);

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        let snapshot = GameSnapshot {
            tick: 100,
            match_phase: MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players: vec![
                create_player_snapshot(player_id, player_pos, 0),
                create_player_snapshot(Uuid::new_v4(), Vec2::new(150.0, 0.0), 1),  // Within extended
                create_player_snapshot(Uuid::new_v4(), Vec2::new(500.0, 0.0), 2),  // Far away
            ],
            projectiles: vec![],
            debris: vec![],
            arena_collapse_phase: 0,
            arena_safe_radius: 800.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: 3,
            total_alive: 3,
            density_grid: vec![],
            notable_players: vec![],
            echo_client_time: 0,
            ai_status: None,
        };

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, &snapshot);

        // Should include self and nearby player, but not far player
        assert_eq!(filtered.players.len(), 2);
    }

    #[test]
    fn test_aoi_filter_top_players_included() {
        let config = AOIConfig {
            full_detail_radius: 100.0,
            extended_radius: 200.0,
            max_entities: 50,
            always_include_top_n: 3,
        };
        let aoi = AOIManager::new(config);

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        let snapshot = GameSnapshot {
            tick: 100,
            match_phase: MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players: vec![
                create_player_snapshot(player_id, player_pos, 0),
                create_player_snapshot(Uuid::new_v4(), Vec2::new(5000.0, 0.0), 100),  // Far but top scorer
                create_player_snapshot(Uuid::new_v4(), Vec2::new(5000.0, 5000.0), 50),  // Far, second place
            ],
            projectiles: vec![],
            debris: vec![],
            arena_collapse_phase: 0,
            arena_safe_radius: 800.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: 3,
            total_alive: 3,
            density_grid: vec![],
            notable_players: vec![],
            echo_client_time: 0,
            ai_status: None,
        };

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, &snapshot);

        // Should include all 3 because top N is 3
        assert_eq!(filtered.players.len(), 3);
    }

    #[test]
    fn test_aoi_filter_projectiles() {
        let config = AOIConfig {
            full_detail_radius: 100.0,
            extended_radius: 300.0,
            max_entities: 50,
            always_include_top_n: 0,
        };
        let aoi = AOIManager::new(config);

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        let snapshot = GameSnapshot {
            tick: 100,
            match_phase: MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players: vec![create_player_snapshot(player_id, player_pos, 0)],
            projectiles: vec![
                create_projectile_snapshot(1, Vec2::new(50.0, 0.0)),   // Near
                create_projectile_snapshot(2, Vec2::new(200.0, 0.0)),  // Within extended
                create_projectile_snapshot(3, Vec2::new(1000.0, 0.0)), // Far
            ],
            debris: vec![],
            arena_collapse_phase: 0,
            arena_safe_radius: 800.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: 1,
            total_alive: 1,
            density_grid: vec![],
            notable_players: vec![],
            echo_client_time: 0,
            ai_status: None,
        };

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, &snapshot);

        // Should include 2 nearby projectiles, exclude far one
        assert_eq!(filtered.projectiles.len(), 2);
    }

    #[test]
    fn test_aoi_filter_max_entities_cap() {
        let max_entities = 10;
        let config = AOIConfig {
            full_detail_radius: 10000.0,  // Include everything by distance
            extended_radius: 10000.0,
            max_entities,                 // But cap at 10
            always_include_top_n: 0,
        };
        let aoi = AOIManager::new(config);

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        let mut snapshot = create_test_snapshot(50);  // 50 players
        snapshot.players[0].id = player_id;
        snapshot.players[0].position = player_pos;

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, &snapshot);

        // Should be capped at max_entities
        assert!(filtered.players.len() <= max_entities);
    }

    #[test]
    fn test_aoi_stats() {
        let original = create_test_snapshot(100);
        let mut filtered = create_test_snapshot(25);
        filtered.projectiles = vec![];

        let stats = AOIManager::snapshot_stats(&original, &filtered);

        assert_eq!(stats.original_players, 100);
        assert_eq!(stats.filtered_players, 25);
        assert!((stats.reduction_percent - 75.0).abs() < 0.1);
    }

    #[test]
    fn test_aoi_gravity_wells_preserved() {
        let aoi = AOIManager::default();
        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        let mut snapshot = create_test_snapshot(5);
        snapshot.players[0].id = player_id;
        snapshot.gravity_wells = vec![
            GravityWellSnapshot {
                id: 0,
                position: Vec2::new(500.0, 500.0),
                mass: 10000.0,
                core_radius: 50.0,
            },
            GravityWellSnapshot {
                id: 1,
                position: Vec2::new(-500.0, -500.0),
                mass: 10000.0,
                core_radius: 50.0,
            },
        ];

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, &snapshot);

        // All gravity wells should be preserved
        assert_eq!(filtered.gravity_wells.len(), 2);
    }

    #[test]
    fn test_aoi_self_always_included_even_at_end_of_list() {
        // Test that the local player is ALWAYS included even when they appear
        // late in the players list and max_entities cap is reached early.
        // This was a bug where HashMap iteration order could cause the local
        // player to be missed if they happened to be processed after the cap.
        let max_entities = 5;
        let config = AOIConfig {
            full_detail_radius: 10000.0,  // Include all by distance
            extended_radius: 10000.0,
            max_entities,                  // Tight cap
            always_include_top_n: 0,       // No top player priority
        };
        let aoi = AOIManager::new(config);

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        // Create 50 players, put our player at the END of the list
        let mut snapshot = create_test_snapshot(50);
        // Put the local player as the LAST player in the list
        snapshot.players[49].id = player_id;
        snapshot.players[49].position = player_pos;

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, &snapshot);

        // Local player MUST be included regardless of their position in the list
        assert!(
            filtered.players.iter().any(|p| p.id == player_id),
            "Local player must always be included, even when at end of player list"
        );
    }

    #[test]
    fn test_aoi_self_included_with_many_top_players() {
        // Ensure local player is included even when top players fill most slots
        let max_entities = 10;
        let config = AOIConfig {
            full_detail_radius: 10000.0,
            extended_radius: 10000.0,
            max_entities,
            always_include_top_n: 8,  // Reserve 8 slots for top players
        };
        let aoi = AOIManager::new(config);

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        // Create 20 players with various kill counts
        let mut snapshot = create_test_snapshot(20);
        // Our player has 0 kills (not a top player) and is at end of list
        snapshot.players[19].id = player_id;
        snapshot.players[19].position = player_pos;
        snapshot.players[19].kills = 0;

        // Give other players higher kill counts
        for i in 0..10 {
            snapshot.players[i].kills = 100 - i as u32;
        }

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, &snapshot);

        // Local player MUST be in the filtered list
        assert!(
            filtered.players.iter().any(|p| p.id == player_id),
            "Local player must be included even when not a top scorer"
        );
    }

    #[test]
    fn test_aoi_no_duplicate_players() {
        // Ensure players aren't added multiple times (self, top, nearby)
        let config = AOIConfig {
            full_detail_radius: 10000.0,
            extended_radius: 10000.0,
            max_entities: 100,
            always_include_top_n: 5,
        };
        let aoi = AOIManager::new(config);

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        // Create snapshot where local player is also a top scorer
        let mut snapshot = create_test_snapshot(10);
        snapshot.players[0].id = player_id;
        snapshot.players[0].position = player_pos;
        snapshot.players[0].kills = 1000;  // Make them top scorer

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, &snapshot);

        // Count occurrences of local player
        let self_count = filtered.players.iter().filter(|p| p.id == player_id).count();
        assert_eq!(self_count, 1, "Local player should appear exactly once, not duplicated");
    }

    #[test]
    fn test_aoi_prioritizes_closest_players() {
        // When max_entities is limited, closest players should be included first
        // This prevents invisible collisions from players that are actually nearby
        let max_entities = 5;  // Very limited
        let config = AOIConfig {
            full_detail_radius: 1000.0,
            extended_radius: 1000.0,
            max_entities,
            always_include_top_n: 0,  // No top player priority
        };
        let aoi = AOIManager::new(config);

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        // Create 20 players at various distances
        let mut snapshot = create_test_snapshot(20);
        snapshot.players[0].id = player_id;
        snapshot.players[0].position = player_pos;

        // Set up distances: some far (500-900), some close (50-150)
        // Put the CLOSE ones at the END of the array to test sorting
        for i in 1..15 {
            // Far players (500-900 units)
            snapshot.players[i].position = Vec2::new(500.0 + (i as f32) * 30.0, 0.0);
        }
        for i in 15..20 {
            // Close players (50-150 units) - these should be prioritized
            snapshot.players[i].position = Vec2::new(50.0 + ((i - 15) as f32) * 25.0, 0.0);
        }

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, &snapshot);

        // With max_entities = 5 (1 self + 4 others), we should have the closest 4 others
        // Check that NO far players (500+ units) are included
        for player in &filtered.players {
            if player.id == player_id {
                continue;
            }
            let distance = (player.position - player_pos).length();
            assert!(
                distance < 400.0,
                "Far player at distance {} should NOT be included when closer players exist",
                distance
            );
        }
    }

    #[test]
    fn test_aoi_collision_visibility_guarantee() {
        // Players within collision range (50-100 units) MUST be visible
        // This test ensures no "invisible collision" scenarios
        let config = AOIConfig {
            full_detail_radius: 500.0,
            extended_radius: 1000.0,
            max_entities: 10,  // Limited
            always_include_top_n: 0,
        };
        let aoi = AOIManager::new(config);

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        // Create many players, with some very close (collision range)
        let mut snapshot = create_test_snapshot(50);
        snapshot.players[0].id = player_id;
        snapshot.players[0].position = player_pos;

        // Put 3 players at collision distance (30-50 units)
        let close_ids: Vec<_> = (1..=3).map(|i| {
            snapshot.players[i].position = Vec2::new(30.0 + (i as f32) * 10.0, 0.0);
            snapshot.players[i].id
        }).collect();

        // Fill rest with far players (300-500 units)
        for i in 4..50 {
            snapshot.players[i].position = Vec2::new(300.0 + (i as f32) * 10.0, 0.0);
        }

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, &snapshot);

        // ALL close players (collision range) MUST be included
        for close_id in close_ids {
            assert!(
                filtered.players.iter().any(|p| p.id == close_id),
                "Player at collision range MUST be visible to prevent invisible collisions"
            );
        }
    }

    #[test]
    fn test_aoi_length_sq_optimization_correctness() {
        // Verify that using length_sq() produces the same results as length()
        // for distance-based filtering (since sqrt is monotonic)
        let config = AOIConfig {
            full_detail_radius: 100.0,
            extended_radius: 200.0,
            max_entities: 50,
            always_include_top_n: 0,
        };
        let aoi = AOIManager::new(config);

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        // Create players at various distances around the boundary
        let snapshot = GameSnapshot {
            tick: 100,
            match_phase: MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players: vec![
                create_player_snapshot(player_id, player_pos, 0),
                // At exactly 199 units - should be included
                create_player_snapshot(Uuid::new_v4(), Vec2::new(199.0, 0.0), 1),
                // At exactly 200 units - should be included (boundary)
                create_player_snapshot(Uuid::new_v4(), Vec2::new(200.0, 0.0), 2),
                // At 201 units - should NOT be included
                create_player_snapshot(Uuid::new_v4(), Vec2::new(201.0, 0.0), 3),
                // Diagonal distance: sqrt(141^2 + 141^2) ≈ 199.4 - should be included
                create_player_snapshot(Uuid::new_v4(), Vec2::new(141.0, 141.0), 4),
                // Diagonal distance: sqrt(142^2 + 142^2) ≈ 200.8 - should NOT be included
                create_player_snapshot(Uuid::new_v4(), Vec2::new(142.0, 142.0), 5),
            ],
            projectiles: vec![],
            debris: vec![],
            arena_collapse_phase: 0,
            arena_safe_radius: 800.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: 6,
            total_alive: 6,
            density_grid: vec![],
            notable_players: vec![],
            echo_client_time: 0,
            ai_status: None,
        };

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, &snapshot);

        // Should include: self + 199 + 200 + diagonal(141,141)
        // Should exclude: 201 + diagonal(142,142)
        assert_eq!(filtered.players.len(), 4, "Should include exactly 4 players");

        // Verify boundary cases
        assert!(filtered.players.iter().any(|p| p.kills == 1), "Player at 199 should be included");
        assert!(filtered.players.iter().any(|p| p.kills == 2), "Player at 200 should be included");
        assert!(!filtered.players.iter().any(|p| p.kills == 3), "Player at 201 should be excluded");
        assert!(filtered.players.iter().any(|p| p.kills == 4), "Diagonal player at ~199 should be included");
        assert!(!filtered.players.iter().any(|p| p.kills == 5), "Diagonal player at ~201 should be excluded");
    }

    #[test]
    fn test_aoi_buffer_pooling_multiple_calls() {
        // Stress test: verify buffer pooling works correctly across many calls
        let aoi = AOIManager::default();
        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        let mut snapshot = create_test_snapshot(50);
        snapshot.players[0].id = player_id;
        snapshot.players[0].position = player_pos;

        // Set varied kill counts to test top-N sorting
        for i in 0..50 {
            snapshot.players[i].kills = i as u32;
        }

        // Call many times to ensure buffer reuse works correctly
        for i in 0..100 {
            let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, &snapshot);
            assert!(
                filtered.players.iter().any(|p| p.id == player_id),
                "Local player should be included on iteration {}",
                i
            );
            // Top players by kills should be included
            assert!(
                filtered.players.iter().any(|p| p.kills == 49),
                "Top scorer should be included on iteration {}",
                i
            );
        }
    }
}
