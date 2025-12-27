//! Area of Interest (AOI) manager for network optimization
//!
//! Filters game state to only include entities relevant to each player,
//! dramatically reducing bandwidth usage at scale.
//!
//! ## Fully Dynamic Filtering
//!
//! The AOI radius is **dynamically calculated** from viewport zoom:
//! - Zoomed in (zoom=1.0): ~1560 unit radius
//! - Zoomed out (zoom=0.45): ~3467 unit radius
//!
//! **No hardcoded entity caps.** If an entity is within the dynamic radius,
//! it's included. The viewport zoom IS the filter.
//!
//! ## Performance Characteristics
//!
//! Worst case (150 players clustered, max zoom out):
//! - All 150 players within radius → all sent
//! - ~6KB per tick with delta compression
//! - ~180KB/s bandwidth
//!
//! Typical case (normal gameplay):
//! - 20-50 players within view
//! - ~2KB per tick
//! - ~60KB/s bandwidth

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
// Dynamic AOI Constants (Performance-Tuned)
// ============================================================================

/// Base visible radius at zoom=1.0 (half screen diagonal in world units)
/// Calculated from typical 1920x1080 screen: sqrt(1920² + 1080²) / 2 ≈ 1100
/// Rounded up slightly for buffer
const BASE_VISIBLE_RADIUS: f32 = 1200.0;

/// Buffer multiplier beyond visible screen edge
/// Prevents pop-in during movement and scroll. 1.3 = 30% extra radius.
const AOI_BUFFER_MULTIPLIER: f32 = 1.3;

/// Minimum zoom clamp to prevent extreme radius values
/// Even at 0.1 zoom: 1200/0.1*1.3 = 15,600 (reasonable for spectators)
const MIN_ZOOM_CLAMP: f32 = 0.1;

/// Lookahead time in seconds for velocity-based AOI expansion
/// Prevents pop-in when moving fast by looking ahead this many seconds
const VELOCITY_LOOKAHEAD_TIME: f32 = 1.5;

/// Maximum velocity expansion as fraction of calculated radius
/// AOI can expand by up to 40% when moving at high speed
const VELOCITY_EXPANSION_MAX_RATIO: f32 = 0.4;

/// Default number of top players to always include (for leaderboard visibility)
/// These players are included regardless of distance for gameplay purposes
const DEFAULT_ALWAYS_INCLUDE_TOP_N: usize = 5;

// ============================================================================
// Pre-computed Constants (Avoid Runtime Calculation)
// ============================================================================

/// Pre-computed: BASE_VISIBLE_RADIUS * AOI_BUFFER_MULTIPLIER
/// Used to avoid multiplication in hot path
const BASE_RADIUS_WITH_BUFFER: f32 = BASE_VISIBLE_RADIUS * AOI_BUFFER_MULTIPLIER;

// ============================================================================
// Inline Calculation Functions
// ============================================================================

/// Calculate effective AOI radius from viewport zoom
///
/// # Performance
/// - Single division
/// - Inlined at all call sites
/// - Branchless on common path
///
/// # Formula
/// `radius = (BASE_VISIBLE_RADIUS * BUFFER) / max(zoom, MIN_ZOOM)`
///
/// # Examples
/// - zoom=1.0  → 1560 units
/// - zoom=0.7  → 2229 units
/// - zoom=0.45 → 3467 units
/// - zoom=0.1  → 15600 units (spectator edge case)
#[inline(always)]
fn calculate_base_radius(viewport_zoom: f32) -> f32 {
    // Branchless max using conditional move
    // Compiler optimizes this to a single MAXSS instruction on x86
    let clamped_zoom = if viewport_zoom > MIN_ZOOM_CLAMP { viewport_zoom } else { MIN_ZOOM_CLAMP };
    BASE_RADIUS_WITH_BUFFER / clamped_zoom
}

/// Calculate velocity-based radius expansion
///
/// Fast-moving players get expanded AOI to prevent pop-in.
/// Expansion is capped at VELOCITY_EXPANSION_MAX_RATIO of base radius.
#[inline(always)]
fn calculate_velocity_expansion(speed: f32, base_radius: f32) -> f32 {
    let max_expansion = base_radius * VELOCITY_EXPANSION_MAX_RATIO;
    let velocity_expansion = speed * VELOCITY_LOOKAHEAD_TIME;
    // Branchless min
    if velocity_expansion < max_expansion { velocity_expansion } else { max_expansion }
}

// ============================================================================
// AOI Configuration
// ============================================================================

/// AOI configuration (minimal - radius is fully dynamic from viewport zoom)
#[derive(Debug, Clone)]
pub struct AOIConfig {
    /// Always include top N players by score (for leaderboard visibility)
    /// Set to 0 to disable. These are included regardless of distance.
    pub always_include_top_n: usize,
}

impl Default for AOIConfig {
    fn default() -> Self {
        Self {
            always_include_top_n: DEFAULT_ALWAYS_INCLUDE_TOP_N,
        }
    }
}

// ============================================================================
// AOI Manager
// ============================================================================

/// Manages Area of Interest filtering for network optimization
pub struct AOIManager {
    config: AOIConfig,
}

impl AOIManager {
    pub fn new(config: AOIConfig) -> Self {
        Self { config }
    }

    /// Filter a game snapshot for a specific player based on their viewport and velocity
    ///
    /// # Fully Dynamic Filtering
    ///
    /// **No hardcoded entity caps.** The dynamic radius (calculated from viewport_zoom)
    /// is the ONLY filter. If an entity is within the radius, it's included.
    ///
    /// This ensures:
    /// - No "invisible enemies" within view distance
    /// - Bandwidth scales naturally with what's actually visible
    /// - Zoomed-in players get maximum network savings
    ///
    /// # Arguments
    /// - `player_id`: The player receiving this filtered snapshot
    /// - `player_position`: Player's world position (center of AOI)
    /// - `player_velocity`: Player's velocity (for predictive expansion)
    /// - `viewport_zoom`: Camera zoom level (0.1-1.0, lower = zoomed out = larger AOI)
    /// - `full_snapshot`: Complete game state to filter
    ///
    /// # Returns
    /// A personalized snapshot containing:
    /// - The player themselves (always, first priority)
    /// - Top N players by score (for leaderboard, regardless of distance)
    /// - ALL players within dynamic AOI radius (sorted by distance)
    /// - ALL projectiles within dynamic AOI radius
    /// - ALL debris within dynamic AOI radius
    /// - All gravity wells (sparse and always important)
    ///
    /// # Performance
    /// - O(n log n) for player sorting by distance
    /// - Uses thread-local buffers to avoid allocations
    /// - Pre-computes squared radius to avoid sqrt in distance checks
    /// - Inlined radius calculations
    #[inline]
    pub fn filter_for_player(
        &self,
        player_id: PlayerId,
        player_position: Vec2,
        player_velocity: Vec2,
        viewport_zoom: f32,
        full_snapshot: &GameSnapshot,
    ) -> GameSnapshot {
        // Calculate dynamic AOI radius from viewport zoom
        let base_radius = calculate_base_radius(viewport_zoom);

        // Expand AOI based on speed to prevent pop-in when moving fast
        let speed = player_velocity.length();
        let velocity_expansion = calculate_velocity_expansion(speed, base_radius);
        let effective_radius = base_radius + velocity_expansion;

        // OPTIMIZATION: Pre-compute squared radius to avoid sqrt in distance checks
        let effective_radius_sq = effective_radius * effective_radius;

        // Pre-allocate with reasonable capacity (will grow if needed)
        let mut filtered_players = Vec::with_capacity(64);
        let mut filtered_projectiles = Vec::with_capacity(32);
        let mut filtered_debris = Vec::with_capacity(64);

        // CRITICAL: First, find and add the local player BEFORE processing others
        // This ensures they're always included
        let mut player_found = false;
        for player in &full_snapshot.players {
            if player.id == player_id {
                filtered_players.push(player.clone());
                player_found = true;
                break;
            }
        }

        // Get top N players by score (for leaderboard visibility)
        // These are included regardless of distance - important for gameplay
        let top_player_ids: SmallVec<[PlayerId; 16]> = if self.config.always_include_top_n > 0 {
            PLAYERS_BY_SCORE_BUFFER.with(|buffer_cell| {
                let mut buffer = buffer_cell.borrow_mut();
                buffer.clear();

                // Collect (id, kills) pairs for sorting
                buffer.extend(full_snapshot.players.iter().map(|p| (p.id, p.kills)));

                // Sort by kills descending
                buffer.sort_unstable_by(|a, b| b.1.cmp(&a.1));

                // Take top N player IDs
                buffer
                    .iter()
                    .take(self.config.always_include_top_n)
                    .map(|(id, _)| *id)
                    .collect()
            })
        } else {
            SmallVec::new()
        };

        // Add top players (skip self, already added)
        for player in &full_snapshot.players {
            if player.id == player_id {
                continue; // Already added
            }
            if top_player_ids.contains(&player.id) {
                filtered_players.push(player.clone());
            }
        }

        // Collect ALL nearby players within radius (no cap!)
        // Sort by distance to ensure consistent ordering
        let nearby_indices: Vec<usize> = NEARBY_WITH_DISTANCE_BUFFER.with(|buffer_cell| {
            let mut buffer = buffer_cell.borrow_mut();
            buffer.clear();

            // Collect (index, distance_sq) pairs for players not already included
            for (idx, p) in full_snapshot.players.iter().enumerate() {
                if p.id == player_id || top_player_ids.contains(&p.id) {
                    continue;
                }
                let distance_sq = (p.position - player_position).length_sq();
                if distance_sq <= effective_radius_sq {
                    buffer.push((idx, distance_sq));
                }
            }

            // Sort by squared distance (closest first) - sqrt is monotonic so order is preserved
            buffer.sort_unstable_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            // Return ALL sorted indices (no cap!)
            buffer.iter().map(|(idx, _)| *idx).collect()
        });

        // Add ALL nearby players (no cap - dynamic radius is the only filter)
        for idx in nearby_indices {
            filtered_players.push(full_snapshot.players[idx].clone());
        }

        // Fallback should never happen now, but keep for safety
        if !player_found {
            // Player not in snapshot - include all nearby players
            filtered_players = full_snapshot.players
                .iter()
                .filter(|p| {
                    let distance_sq = (p.position - player_position).length_sq();
                    distance_sq <= effective_radius_sq
                })
                .cloned()
                .collect();
        }

        // Filter ALL projectiles within radius (no cap!)
        for proj in &full_snapshot.projectiles {
            let distance_sq = (proj.position - player_position).length_sq();
            if distance_sq <= effective_radius_sq {
                filtered_projectiles.push(proj.clone());
            }
        }

        // Filter ALL debris within radius (no cap!)
        for debris in &full_snapshot.debris {
            let distance_sq = (debris.position - player_position).length_sq();
            if distance_sq <= effective_radius_sq {
                filtered_debris.push(debris.clone());
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

// ============================================================================
// Tests
// ============================================================================

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
            flags: player_flags::ALIVE,
            kills,
            deaths: 0,
            color_index: 0,
            spawn_tick: 0,
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

    // ========================================================================
    // Dynamic Radius Tests
    // ========================================================================

    #[test]
    fn test_calculate_base_radius_zoom_1() {
        let radius = calculate_base_radius(1.0);
        assert!((radius - 1560.0).abs() < 1.0, "At zoom=1.0, radius should be ~1560, got {}", radius);
    }

    #[test]
    fn test_calculate_base_radius_zoom_half() {
        let radius = calculate_base_radius(0.5);
        assert!((radius - 3120.0).abs() < 1.0, "At zoom=0.5, radius should be ~3120, got {}", radius);
    }

    #[test]
    fn test_calculate_base_radius_zoom_min_clamp() {
        let radius_at_min = calculate_base_radius(MIN_ZOOM_CLAMP);
        let radius_below_min = calculate_base_radius(0.01);
        assert_eq!(radius_at_min, radius_below_min, "Zoom below MIN should be clamped");
    }

    #[test]
    fn test_dynamic_aoi_zoomed_in_filters_more() {
        let aoi = AOIManager::new(AOIConfig {
            always_include_top_n: 0,
        });

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        let snapshot = GameSnapshot {
            tick: 100,
            match_phase: MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players: vec![
                create_player_snapshot(player_id, player_pos, 0),
                create_player_snapshot(Uuid::new_v4(), Vec2::new(1000.0, 0.0), 1),
                create_player_snapshot(Uuid::new_v4(), Vec2::new(2000.0, 0.0), 2),
                create_player_snapshot(Uuid::new_v4(), Vec2::new(3000.0, 0.0), 3),
            ],
            projectiles: vec![],
            debris: vec![],
            arena_collapse_phase: 0,
            arena_safe_radius: 5000.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: 4,
            total_alive: 4,
            density_grid: vec![],
            notable_players: vec![],
            echo_client_time: 0,
            ai_status: None,
        };

        // Zoomed in (zoom=1.0): radius ~1560
        let filtered_zoomed_in = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, 1.0, &snapshot);

        // Zoomed out (zoom=0.45): radius ~3467
        let filtered_zoomed_out = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, 0.45, &snapshot);

        assert!(
            filtered_zoomed_in.players.len() < filtered_zoomed_out.players.len(),
            "Zoomed in should see fewer players. In: {}, Out: {}",
            filtered_zoomed_in.players.len(),
            filtered_zoomed_out.players.len()
        );
    }

    // ========================================================================
    // No Cap Tests - Verify all entities within radius are included
    // ========================================================================

    #[test]
    fn test_no_player_cap_all_nearby_included() {
        let aoi = AOIManager::new(AOIConfig {
            always_include_top_n: 0,
        });

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        // Create 200 players all within a small radius
        let mut snapshot = create_test_snapshot(0);
        snapshot.players.push(create_player_snapshot(player_id, player_pos, 0));
        for i in 1..200 {
            // All within 500 units (well within zoom=1.0 radius of 1560)
            let angle = (i as f32 / 200.0) * std::f32::consts::TAU;
            let pos = Vec2::new(angle.cos() * 400.0, angle.sin() * 400.0);
            snapshot.players.push(create_player_snapshot(Uuid::new_v4(), pos, i as u32));
        }
        snapshot.total_players = 200;
        snapshot.total_alive = 200;

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, 1.0, &snapshot);

        // ALL 200 players should be included (no cap!)
        assert_eq!(
            filtered.players.len(), 200,
            "All players within radius should be included, no cap. Got {}",
            filtered.players.len()
        );
    }

    #[test]
    fn test_no_projectile_cap() {
        let aoi = AOIManager::default();

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        let mut snapshot = create_test_snapshot(1);
        snapshot.players[0].id = player_id;
        snapshot.players[0].position = player_pos;

        // Create 100 projectiles all within radius
        for i in 0..100 {
            let angle = (i as f32 / 100.0) * std::f32::consts::TAU;
            let pos = Vec2::new(angle.cos() * 500.0, angle.sin() * 500.0);
            snapshot.projectiles.push(create_projectile_snapshot(i, pos));
        }

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, 1.0, &snapshot);

        // ALL projectiles should be included
        assert_eq!(filtered.projectiles.len(), 100, "All projectiles within radius should be included");
    }

    // ========================================================================
    // Core Functionality Tests
    // ========================================================================

    #[test]
    fn test_aoi_filter_self_always_included() {
        let aoi = AOIManager::default();
        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(100.0, 100.0);

        let mut snapshot = create_test_snapshot(10);
        snapshot.players[0].id = player_id;
        snapshot.players[0].position = player_pos;

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, 1.0, &snapshot);

        assert!(filtered.players.iter().any(|p| p.id == player_id));
    }

    #[test]
    fn test_aoi_filter_distant_players_excluded() {
        let aoi = AOIManager::new(AOIConfig {
            always_include_top_n: 0,
        });

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        let snapshot = GameSnapshot {
            tick: 100,
            match_phase: MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players: vec![
                create_player_snapshot(player_id, player_pos, 0),
                create_player_snapshot(Uuid::new_v4(), Vec2::new(500.0, 0.0), 1),   // Within
                create_player_snapshot(Uuid::new_v4(), Vec2::new(5000.0, 0.0), 2),  // Far
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

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, 1.0, &snapshot);

        assert_eq!(filtered.players.len(), 2, "Should include self and nearby player only");
    }

    #[test]
    fn test_aoi_filter_top_players_included() {
        let aoi = AOIManager::new(AOIConfig {
            always_include_top_n: 3,
        });

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        let snapshot = GameSnapshot {
            tick: 100,
            match_phase: MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players: vec![
                create_player_snapshot(player_id, player_pos, 0),
                create_player_snapshot(Uuid::new_v4(), Vec2::new(50000.0, 0.0), 100),
                create_player_snapshot(Uuid::new_v4(), Vec2::new(50000.0, 50000.0), 50),
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

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, 1.0, &snapshot);

        assert_eq!(filtered.players.len(), 3);
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
                position: Vec2::new(50000.0, 50000.0),
                mass: 10000.0,
                core_radius: 50.0,
            },
            GravityWellSnapshot {
                id: 1,
                position: Vec2::new(-50000.0, -50000.0),
                mass: 10000.0,
                core_radius: 50.0,
            },
        ];

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, 1.0, &snapshot);

        assert_eq!(filtered.gravity_wells.len(), 2);
    }

    #[test]
    fn test_aoi_velocity_expansion() {
        let aoi = AOIManager::new(AOIConfig {
            always_include_top_n: 0,
        });

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);
        let edge_player_pos = Vec2::new(1800.0, 0.0);

        let snapshot = GameSnapshot {
            tick: 100,
            match_phase: MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players: vec![
                create_player_snapshot(player_id, player_pos, 0),
                create_player_snapshot(Uuid::new_v4(), edge_player_pos, 1),
            ],
            projectiles: vec![],
            debris: vec![],
            arena_collapse_phase: 0,
            arena_safe_radius: 5000.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: 2,
            total_alive: 2,
            density_grid: vec![],
            notable_players: vec![],
            echo_client_time: 0,
            ai_status: None,
        };

        let filtered_stationary = aoi.filter_for_player(
            player_id, player_pos, Vec2::ZERO, 1.0, &snapshot
        );

        let filtered_moving = aoi.filter_for_player(
            player_id, player_pos, Vec2::new(300.0, 0.0), 1.0, &snapshot
        );

        assert!(
            filtered_moving.players.len() >= filtered_stationary.players.len(),
            "Moving player should see at least as many players as stationary"
        );
    }

    #[test]
    fn test_aoi_no_duplicate_players() {
        let aoi = AOIManager::new(AOIConfig {
            always_include_top_n: 5,
        });

        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        let mut snapshot = create_test_snapshot(10);
        snapshot.players[0].id = player_id;
        snapshot.players[0].position = player_pos;
        snapshot.players[0].kills = 1000;

        let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, 1.0, &snapshot);

        let self_count = filtered.players.iter().filter(|p| p.id == player_id).count();
        assert_eq!(self_count, 1, "Local player should appear exactly once");
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
    fn test_aoi_buffer_pooling_multiple_calls() {
        let aoi = AOIManager::default();
        let player_id = Uuid::new_v4();
        let player_pos = Vec2::new(0.0, 0.0);

        let mut snapshot = create_test_snapshot(50);
        snapshot.players[0].id = player_id;
        snapshot.players[0].position = player_pos;

        for i in 0..50 {
            snapshot.players[i].kills = i as u32;
        }

        for i in 0..100 {
            let filtered = aoi.filter_for_player(player_id, player_pos, Vec2::ZERO, 1.0, &snapshot);
            assert!(
                filtered.players.iter().any(|p| p.id == player_id),
                "Local player should be included on iteration {}",
                i
            );
        }
    }
}
