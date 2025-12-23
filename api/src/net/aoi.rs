//! Area of Interest (AOI) manager for network optimization
//!
//! Filters game state to only include entities relevant to each player,
//! dramatically reducing bandwidth usage at scale.

use crate::game::state::PlayerId;
use crate::net::protocol::{GameSnapshot, PlayerSnapshot, ProjectileSnapshot, GravityWellSnapshot};
use crate::util::vec2::Vec2;

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
            full_detail_radius: 500.0,   // Full detail for nearby entities
            extended_radius: 1000.0,     // Reduced detail for medium range
            max_entities: 100,           // Cap to prevent bandwidth explosion
            always_include_top_n: 5,     // Always show top 5 players
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

    /// Filter a game snapshot for a specific player based on their position
    ///
    /// Returns a personalized snapshot containing:
    /// - The player themselves (always)
    /// - Nearby players within AOI radius
    /// - Top N players by score (for leaderboard)
    /// - Nearby projectiles
    /// - All gravity wells (they're sparse and important)
    pub fn filter_for_player(
        &self,
        player_id: PlayerId,
        player_position: Vec2,
        full_snapshot: &GameSnapshot,
    ) -> GameSnapshot {
        let mut filtered_players = Vec::with_capacity(self.config.max_entities);
        let mut filtered_projectiles = Vec::with_capacity(self.config.max_entities);

        // First, find the player themselves and top players
        let mut player_found = false;
        let mut players_by_score: Vec<&PlayerSnapshot> = full_snapshot.players.iter().collect();
        players_by_score.sort_by(|a, b| b.kills.cmp(&a.kills));

        // Get top N player IDs
        let top_player_ids: Vec<PlayerId> = players_by_score
            .iter()
            .take(self.config.always_include_top_n)
            .map(|p| p.id)
            .collect();

        // Filter players
        for player in &full_snapshot.players {
            // Always include self
            if player.id == player_id {
                filtered_players.push(player.clone());
                player_found = true;
                continue;
            }

            // Always include top players
            if top_player_ids.contains(&player.id) {
                filtered_players.push(player.clone());
                continue;
            }

            // Check distance for other players
            let distance = (player.position - player_position).length();
            if distance <= self.config.extended_radius {
                // Include if within extended radius
                filtered_players.push(player.clone());
            }

            // Cap at max entities
            if filtered_players.len() >= self.config.max_entities {
                break;
            }
        }

        // If player not found (shouldn't happen), use full player list
        if !player_found {
            // Fallback - just return a capped version
            filtered_players = full_snapshot.players
                .iter()
                .take(self.config.max_entities)
                .cloned()
                .collect();
        }

        // Filter projectiles by distance
        for proj in &full_snapshot.projectiles {
            let distance = (proj.position - player_position).length();
            if distance <= self.config.extended_radius {
                filtered_projectiles.push(proj.clone());
            }

            // Cap projectiles too
            if filtered_projectiles.len() >= self.config.max_entities / 2 {
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
    use uuid::Uuid;

    fn create_player_snapshot(id: Uuid, position: Vec2, kills: u32) -> PlayerSnapshot {
        PlayerSnapshot {
            id,
            name: format!("Player_{}", kills),
            position,
            velocity: Vec2::ZERO,
            rotation: 0.0,
            mass: 100.0,
            alive: true,
            kills,
            deaths: 0,
            spawn_protection: false,
            is_bot: false,
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
            arena_collapse_phase: 0,
            arena_safe_radius: 800.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: player_len,
            total_alive: player_len,
            density_grid: vec![],
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

        let filtered = aoi.filter_for_player(player_id, player_pos, &snapshot);

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
            arena_collapse_phase: 0,
            arena_safe_radius: 800.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: 3,
            total_alive: 3,
            density_grid: vec![],
        };

        let filtered = aoi.filter_for_player(player_id, player_pos, &snapshot);

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
            arena_collapse_phase: 0,
            arena_safe_radius: 800.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: 3,
            total_alive: 3,
            density_grid: vec![],
        };

        let filtered = aoi.filter_for_player(player_id, player_pos, &snapshot);

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
            arena_collapse_phase: 0,
            arena_safe_radius: 800.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: 1,
            total_alive: 1,
            density_grid: vec![],
        };

        let filtered = aoi.filter_for_player(player_id, player_pos, &snapshot);

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

        let filtered = aoi.filter_for_player(player_id, player_pos, &snapshot);

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
                position: Vec2::new(500.0, 500.0),
                mass: 10000.0,
                core_radius: 50.0,
            },
            GravityWellSnapshot {
                position: Vec2::new(-500.0, -500.0),
                mass: 10000.0,
                core_radius: 50.0,
            },
        ];

        let filtered = aoi.filter_for_player(player_id, player_pos, &snapshot);

        // All gravity wells should be preserved
        assert_eq!(filtered.gravity_wells.len(), 2);
    }
}
