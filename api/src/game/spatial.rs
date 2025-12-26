//! Spatial hash grid for O(n) collision detection
//!
//! Divides the game world into cells and stores entities in each cell.
//! Collision queries only check the current cell and neighbors.

#![allow(dead_code)] // Utility methods for spatial queries

use crate::util::vec2::Vec2;
use hashbrown::HashMap;
use std::cell::RefCell;

// Thread-local reusable buffer for collision pair queries
thread_local! {
    /// Reusable buffer for collision pairs to avoid per-frame allocations
    static COLLISION_PAIRS_BUFFER: RefCell<Vec<(SpatialEntity, SpatialEntity)>> =
        RefCell::new(Vec::with_capacity(1024));
}

// ============================================================================
// Entity Spatial Grid Constants
// ============================================================================

/// Default cell size for entity collision grid (world units)
/// Should be ~2x maximum entity radius for collision detection
pub const ENTITY_GRID_CELL_SIZE: f32 = 64.0;

/// Initial capacity for entity grid cells (number of expected non-empty cells)
const ENTITY_GRID_INITIAL_CAPACITY: usize = 256;

/// Initial capacity for entity vectors within cells
const ENTITY_CELL_INITIAL_CAPACITY: usize = 8;

/// Grid cell key - (x, y) cell coordinates
pub type CellKey = (i32, i32);

/// Entity ID for spatial grid (player or projectile)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpatialEntityId {
    Player(uuid::Uuid),
    Projectile(u64),
    Debris(u64),
}

/// Entity data stored in the spatial grid
#[derive(Debug, Clone, Copy)]
pub struct SpatialEntity {
    pub id: SpatialEntityId,
    pub position: Vec2,
    pub radius: f32,
}

/// Spatial hash grid for efficient collision detection
pub struct SpatialGrid {
    /// Cell size in world units (larger = fewer cells, more entities per cell)
    cell_size: f32,
    /// Inverse cell size for fast position-to-cell conversion
    inv_cell_size: f32,
    /// Map from cell key to entities in that cell
    cells: HashMap<CellKey, Vec<SpatialEntity>>,
    /// Pre-allocated neighbor offsets for 9-cell query
    neighbor_offsets: [(i32, i32); 9],
}

impl SpatialGrid {
    /// Create a new spatial grid with the given cell size
    ///
    /// Cell size should be roughly 2x the maximum entity radius
    /// to ensure collisions are always found in neighboring cells
    pub fn new(cell_size: f32) -> Self {
        Self {
            cell_size,
            inv_cell_size: 1.0 / cell_size,
            cells: HashMap::with_capacity(ENTITY_GRID_INITIAL_CAPACITY),
            neighbor_offsets: [
                (-1, -1), (0, -1), (1, -1),
                (-1,  0), (0,  0), (1,  0),
                (-1,  1), (0,  1), (1,  1),
            ],
        }
    }

    /// Clear all entities from the grid
    #[inline]
    pub fn clear(&mut self) {
        for cell in self.cells.values_mut() {
            cell.clear();
        }
    }

    /// Convert world position to cell key
    #[inline]
    fn position_to_cell(&self, position: Vec2) -> CellKey {
        (
            (position.x * self.inv_cell_size).floor() as i32,
            (position.y * self.inv_cell_size).floor() as i32,
        )
    }

    /// Insert an entity into the grid
    #[inline]
    pub fn insert(&mut self, entity: SpatialEntity) {
        let cell_key = self.position_to_cell(entity.position);
        self.cells
            .entry(cell_key)
            .or_insert_with(|| Vec::with_capacity(ENTITY_CELL_INITIAL_CAPACITY))
            .push(entity);
    }

    /// Query all entities within a radius of a position
    /// Returns an iterator over nearby entities (including the query position's cell and neighbors)
    pub fn query_radius(&self, position: Vec2, _radius: f32) -> impl Iterator<Item = &SpatialEntity> {
        let (cx, cy) = self.position_to_cell(position);

        self.neighbor_offsets.iter().flat_map(move |&(dx, dy)| {
            let cell_key = (cx + dx, cy + dy);
            self.cells.get(&cell_key).into_iter().flat_map(|cell| cell.iter())
        })
    }

    /// Query entities and return pairs to check for collision
    /// This avoids checking the same pair twice
    ///
    /// OPTIMIZATION: Uses thread-local buffer to avoid per-frame allocations.
    /// The returned Vec is cloned from the reusable buffer.
    pub fn get_potential_collisions(&self) -> Vec<(SpatialEntity, SpatialEntity)> {
        COLLISION_PAIRS_BUFFER.with(|buffer_cell| {
            let mut pairs = buffer_cell.borrow_mut();
            pairs.clear();

            // For each cell, check entities within the cell and with right/bottom neighbors
            // This ensures each pair is only checked once
            for (&(cx, cy), entities) in &self.cells {
                // Check pairs within the same cell
                for i in 0..entities.len() {
                    for j in (i + 1)..entities.len() {
                        pairs.push((entities[i], entities[j]));
                    }
                }

                // Check with right neighbor
                if let Some(right_cell) = self.cells.get(&(cx + 1, cy)) {
                    for entity in entities {
                        for other in right_cell {
                            pairs.push((*entity, *other));
                        }
                    }
                }

                // Check with bottom neighbor
                if let Some(bottom_cell) = self.cells.get(&(cx, cy + 1)) {
                    for entity in entities {
                        for other in bottom_cell {
                            pairs.push((*entity, *other));
                        }
                    }
                }

                // Check with bottom-right neighbor
                if let Some(br_cell) = self.cells.get(&(cx + 1, cy + 1)) {
                    for entity in entities {
                        for other in br_cell {
                            pairs.push((*entity, *other));
                        }
                    }
                }

                // Check with bottom-left neighbor
                if let Some(bl_cell) = self.cells.get(&(cx - 1, cy + 1)) {
                    for entity in entities {
                        for other in bl_cell {
                            pairs.push((*entity, *other));
                        }
                    }
                }
            }

            // Clone is required since we return from thread-local borrow
            pairs.clone()
        })
    }

    /// Process each potential collision pair with a callback function
    /// This is more efficient than get_potential_collisions() when you don't
    /// need to store all pairs, as it avoids cloning the buffer.
    ///
    /// OPTIMIZATION: Zero-allocation iteration over collision pairs
    #[inline]
    pub fn for_each_potential_collision<F>(&self, mut callback: F)
    where
        F: FnMut(SpatialEntity, SpatialEntity),
    {
        // For each cell, check entities within the cell and with right/bottom neighbors
        for (&(cx, cy), entities) in &self.cells {
            // Check pairs within the same cell
            for i in 0..entities.len() {
                for j in (i + 1)..entities.len() {
                    callback(entities[i], entities[j]);
                }
            }

            // Check with right neighbor
            if let Some(right_cell) = self.cells.get(&(cx + 1, cy)) {
                for entity in entities {
                    for other in right_cell {
                        callback(*entity, *other);
                    }
                }
            }

            // Check with bottom neighbor
            if let Some(bottom_cell) = self.cells.get(&(cx, cy + 1)) {
                for entity in entities {
                    for other in bottom_cell {
                        callback(*entity, *other);
                    }
                }
            }

            // Check with bottom-right neighbor
            if let Some(br_cell) = self.cells.get(&(cx + 1, cy + 1)) {
                for entity in entities {
                    for other in br_cell {
                        callback(*entity, *other);
                    }
                }
            }

            // Check with bottom-left neighbor
            if let Some(bl_cell) = self.cells.get(&(cx - 1, cy + 1)) {
                for entity in entities {
                    for other in bl_cell {
                        callback(*entity, *other);
                    }
                }
            }
        }
    }

    /// Get statistics about the grid
    pub fn stats(&self) -> SpatialGridStats {
        let non_empty_cells = self.cells.values().filter(|c| !c.is_empty()).count();
        let total_entities: usize = self.cells.values().map(|c| c.len()).sum();
        let max_per_cell = self.cells.values().map(|c| c.len()).max().unwrap_or(0);

        SpatialGridStats {
            non_empty_cells,
            total_entities,
            max_per_cell,
        }
    }
}

impl Default for SpatialGrid {
    fn default() -> Self {
        Self::new(ENTITY_GRID_CELL_SIZE)
    }
}

/// Statistics about the spatial grid
#[derive(Debug, Clone)]
pub struct SpatialGridStats {
    pub non_empty_cells: usize,
    pub total_entities: usize,
    pub max_per_cell: usize,
}

// ============================================================================
// Well Spatial Grid - Optimized for gravity well lookups
// ============================================================================

use crate::game::state::WellId;

/// Default cell size for well spatial grid (world units)
/// Larger than entity grid since wells have large influence radii
pub const WELL_GRID_CELL_SIZE: f32 = 500.0;

/// Maximum distance a gravity well can meaningfully influence entities
/// At this distance with typical mass (10000), gravity is ~1 unit/s²
/// Formula: acceleration = 0.5 * mass / distance = 0.5 * 10000 / 5000 = 1
pub const WELL_INFLUENCE_RADIUS: f32 = 5000.0;

/// Initial capacity for cell vectors (wells per cell, typically low)
const WELL_CELL_INITIAL_CAPACITY: usize = 4;

/// Initial capacity for cell hashmap (number of cells)
const WELL_GRID_INITIAL_CAPACITY: usize = 64;

/// Spatial hash grid optimized for gravity wells
///
/// Uses larger cell sizes (500+ units) since wells have large influence radii.
/// Supports efficient queries for "all wells that could affect this position".
#[derive(Debug, Clone)]
pub struct WellSpatialGrid {
    /// Cell size in world units (larger than entity grid due to well influence range)
    cell_size: f32,
    /// Inverse cell size for fast position-to-cell conversion
    inv_cell_size: f32,
    /// Map from cell key to well IDs in that cell
    cells: HashMap<CellKey, Vec<WellId>>,
    /// Query radius in cells (how many cells to check around a position)
    query_radius_cells: i32,
}

impl WellSpatialGrid {
    /// Create a new well spatial grid
    ///
    /// # Arguments
    /// * `cell_size` - Size of each cell (recommend 500-1000 for gravity wells)
    /// * `influence_radius` - Maximum distance a well can influence (determines query radius)
    pub fn new(cell_size: f32, influence_radius: f32) -> Self {
        // Calculate how many cells we need to check to cover the influence radius
        let query_radius_cells = (influence_radius / cell_size).ceil() as i32 + 1;

        Self {
            cell_size,
            inv_cell_size: 1.0 / cell_size,
            cells: HashMap::with_capacity(WELL_GRID_INITIAL_CAPACITY),
            query_radius_cells,
        }
    }

    /// Clear all wells from the grid
    #[inline]
    pub fn clear(&mut self) {
        for cell in self.cells.values_mut() {
            cell.clear();
        }
    }

    /// Convert world position to cell key
    #[inline]
    fn position_to_cell(&self, position: Vec2) -> CellKey {
        (
            (position.x * self.inv_cell_size).floor() as i32,
            (position.y * self.inv_cell_size).floor() as i32,
        )
    }

    /// Insert a well into the grid
    #[inline]
    pub fn insert(&mut self, well_id: WellId, position: Vec2) {
        let cell_key = self.position_to_cell(position);
        self.cells
            .entry(cell_key)
            .or_insert_with(|| Vec::with_capacity(WELL_CELL_INITIAL_CAPACITY))
            .push(well_id);
    }

    /// Remove a well from the grid
    /// Returns true if the well was found and removed
    pub fn remove(&mut self, well_id: WellId, position: Vec2) -> bool {
        let cell_key = self.position_to_cell(position);
        if let Some(cell) = self.cells.get_mut(&cell_key) {
            if let Some(idx) = cell.iter().position(|&id| id == well_id) {
                cell.swap_remove(idx);
                return true;
            }
        }
        false
    }

    /// Query all well IDs that could potentially influence a position
    /// Returns well IDs within the configured influence radius
    pub fn query_nearby(&self, position: Vec2) -> impl Iterator<Item = WellId> + '_ {
        let (cx, cy) = self.position_to_cell(position);
        let radius = self.query_radius_cells;

        (-radius..=radius).flat_map(move |dx| {
            (-radius..=radius).flat_map(move |dy| {
                let cell_key = (cx + dx, cy + dy);
                self.cells
                    .get(&cell_key)
                    .into_iter()
                    .flat_map(|cell| cell.iter().copied())
            })
        })
    }

    /// Query well IDs within a specific radius (for custom influence calculations)
    pub fn query_radius(&self, position: Vec2, radius: f32) -> impl Iterator<Item = WellId> + '_ {
        let (cx, cy) = self.position_to_cell(position);
        let cell_radius = (radius * self.inv_cell_size).ceil() as i32 + 1;

        (-cell_radius..=cell_radius).flat_map(move |dx| {
            (-cell_radius..=cell_radius).flat_map(move |dy| {
                let cell_key = (cx + dx, cy + dy);
                self.cells
                    .get(&cell_key)
                    .into_iter()
                    .flat_map(|cell| cell.iter().copied())
            })
        })
    }

    /// Rebuild the grid from a collection of wells
    pub fn rebuild<'a>(&mut self, wells: impl Iterator<Item = (WellId, Vec2)>) {
        self.clear();
        for (well_id, position) in wells {
            self.insert(well_id, position);
        }
    }

    /// Get the number of non-empty cells
    pub fn cell_count(&self) -> usize {
        self.cells.values().filter(|c| !c.is_empty()).count()
    }

    /// Get total wells in the grid
    pub fn well_count(&self) -> usize {
        self.cells.values().map(|c| c.len()).sum()
    }
}

impl Default for WellSpatialGrid {
    fn default() -> Self {
        Self::new(WELL_GRID_CELL_SIZE, WELL_INFLUENCE_RADIUS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_player_entity(id: uuid::Uuid, x: f32, y: f32, radius: f32) -> SpatialEntity {
        SpatialEntity {
            id: SpatialEntityId::Player(id),
            position: Vec2::new(x, y),
            radius,
        }
    }

    #[test]
    fn test_new_grid() {
        let grid = SpatialGrid::new(64.0);
        assert_eq!(grid.cell_size, 64.0);
    }

    #[test]
    fn test_insert_and_query() {
        let mut grid = SpatialGrid::new(64.0);

        let id = uuid::Uuid::new_v4();
        let entity = create_player_entity(id, 100.0, 100.0, 10.0);
        grid.insert(entity);

        let results: Vec<_> = grid.query_radius(Vec2::new(100.0, 100.0), 20.0).collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, SpatialEntityId::Player(id));
    }

    #[test]
    fn test_query_finds_neighbors() {
        let mut grid = SpatialGrid::new(64.0);

        // Insert entity at cell (1, 1)
        let id1 = uuid::Uuid::new_v4();
        grid.insert(create_player_entity(id1, 80.0, 80.0, 10.0));

        // Insert entity at cell (2, 1) - neighbor
        let id2 = uuid::Uuid::new_v4();
        grid.insert(create_player_entity(id2, 130.0, 80.0, 10.0));

        // Query from first position should find both
        let results: Vec<_> = grid.query_radius(Vec2::new(80.0, 80.0), 100.0).collect();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_clear() {
        let mut grid = SpatialGrid::new(64.0);

        let id = uuid::Uuid::new_v4();
        grid.insert(create_player_entity(id, 100.0, 100.0, 10.0));

        grid.clear();

        let results: Vec<_> = grid.query_radius(Vec2::new(100.0, 100.0), 50.0).collect();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_potential_collisions() {
        let mut grid = SpatialGrid::new(64.0);

        // Two entities in same cell
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        grid.insert(create_player_entity(id1, 100.0, 100.0, 10.0));
        grid.insert(create_player_entity(id2, 110.0, 100.0, 10.0));

        let pairs = grid.get_potential_collisions();
        assert_eq!(pairs.len(), 1);
    }

    #[test]
    fn test_for_each_potential_collision() {
        let mut grid = SpatialGrid::new(64.0);

        // Two entities in same cell
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        grid.insert(create_player_entity(id1, 100.0, 100.0, 10.0));
        grid.insert(create_player_entity(id2, 110.0, 100.0, 10.0));

        // Third entity in neighboring cell
        let id3 = uuid::Uuid::new_v4();
        grid.insert(create_player_entity(id3, 165.0, 100.0, 10.0));

        // Count pairs using callback
        let mut pair_count = 0;
        grid.for_each_potential_collision(|_a, _b| {
            pair_count += 1;
        });

        // Should have: (1,2) in same cell + (1,3) and (2,3) from neighbor
        assert_eq!(pair_count, 3, "Should find 3 pairs");

        // Verify it produces same results as get_potential_collisions
        let pairs = grid.get_potential_collisions();
        assert_eq!(pairs.len(), pair_count, "for_each and get should produce same count");
    }

    #[test]
    fn test_collision_buffer_reuse() {
        // Verify that the thread-local buffer is properly reused
        let mut grid = SpatialGrid::new(64.0);

        // First call
        for i in 0..10 {
            let id = uuid::Uuid::new_v4();
            grid.insert(create_player_entity(id, 100.0 + i as f32 * 5.0, 100.0, 10.0));
        }
        let pairs1 = grid.get_potential_collisions();

        // Clear and add different entities
        grid.clear();
        for i in 0..5 {
            let id = uuid::Uuid::new_v4();
            grid.insert(create_player_entity(id, 100.0 + i as f32 * 5.0, 100.0, 10.0));
        }
        let pairs2 = grid.get_potential_collisions();

        // Verify correct counts (n*(n-1)/2 pairs for n entities in same cell)
        assert_eq!(pairs1.len(), 45, "10 entities = 45 pairs");
        assert_eq!(pairs2.len(), 10, "5 entities = 10 pairs");
    }

    #[test]
    fn test_stats() {
        let mut grid = SpatialGrid::new(64.0);

        // Insert 3 entities in same cell
        for _ in 0..3 {
            let id = uuid::Uuid::new_v4();
            grid.insert(create_player_entity(id, 100.0, 100.0, 10.0));
        }

        // Insert 1 entity in different cell
        let id = uuid::Uuid::new_v4();
        grid.insert(create_player_entity(id, 500.0, 500.0, 10.0));

        let stats = grid.stats();
        assert_eq!(stats.total_entities, 4);
        assert_eq!(stats.non_empty_cells, 2);
        assert_eq!(stats.max_per_cell, 3);
    }

    // === WellSpatialGrid Tests ===

    #[test]
    fn test_well_grid_new() {
        let grid = WellSpatialGrid::new(500.0, 2000.0);
        assert_eq!(grid.cell_size, 500.0);
        assert_eq!(grid.query_radius_cells, 5); // ceil(2000/500) + 1 = 5
    }

    #[test]
    fn test_well_grid_insert_and_query() {
        let mut grid = WellSpatialGrid::new(500.0, 2000.0);

        // Insert a well at origin
        grid.insert(1, Vec2::ZERO);

        // Query should find it
        let results: Vec<_> = grid.query_nearby(Vec2::ZERO).collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], 1);

        // Query from nearby position should also find it
        let results: Vec<_> = grid.query_nearby(Vec2::new(100.0, 100.0)).collect();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_well_grid_remove() {
        let mut grid = WellSpatialGrid::new(500.0, 2000.0);

        grid.insert(1, Vec2::new(100.0, 100.0));
        grid.insert(2, Vec2::new(200.0, 200.0));

        assert_eq!(grid.well_count(), 2);

        // Remove well 1
        let removed = grid.remove(1, Vec2::new(100.0, 100.0));
        assert!(removed);
        assert_eq!(grid.well_count(), 1);

        // Try to remove again (should fail)
        let removed_again = grid.remove(1, Vec2::new(100.0, 100.0));
        assert!(!removed_again);
    }

    #[test]
    fn test_well_grid_query_finds_distant_wells() {
        let mut grid = WellSpatialGrid::new(500.0, 2000.0);

        // Insert well at origin
        grid.insert(1, Vec2::ZERO);

        // Query from 1500 units away (within influence radius)
        let results: Vec<_> = grid.query_nearby(Vec2::new(1500.0, 0.0)).collect();
        assert_eq!(results.len(), 1);

        // Query from 3000 units away (outside default influence radius)
        let results: Vec<_> = grid.query_nearby(Vec2::new(3000.0, 0.0)).collect();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_well_grid_rebuild() {
        let mut grid = WellSpatialGrid::new(500.0, 2000.0);

        // Insert some wells
        grid.insert(1, Vec2::new(0.0, 0.0));
        grid.insert(2, Vec2::new(1000.0, 0.0));
        assert_eq!(grid.well_count(), 2);

        // Rebuild with different wells
        let new_wells = vec![
            (10, Vec2::new(500.0, 500.0)),
            (11, Vec2::new(-500.0, -500.0)),
            (12, Vec2::new(0.0, 1000.0)),
        ];
        grid.rebuild(new_wells.into_iter());

        assert_eq!(grid.well_count(), 3);

        // Old wells should not be found
        let results: Vec<_> = grid.query_nearby(Vec2::ZERO).collect();
        assert!(!results.contains(&1));
        assert!(!results.contains(&2));

        // New wells should be found
        assert!(results.contains(&10));
        assert!(results.contains(&11));
    }

    #[test]
    fn test_well_grid_many_wells() {
        // Use larger spacing to test sparse queries
        let mut grid = WellSpatialGrid::new(500.0, 1500.0);

        // Insert 400 wells in a grid pattern (20x20, spaced 1000 apart)
        for x in 0..20 {
            for y in 0..20 {
                let id = (x * 20 + y) as u32;
                let pos = Vec2::new(x as f32 * 1000.0, y as f32 * 1000.0);
                grid.insert(id, pos);
            }
        }

        assert_eq!(grid.well_count(), 400);

        // Query from center should find only nearby wells, not all 400
        let center = Vec2::new(10000.0, 10000.0);
        let results: Vec<_> = grid.query_nearby(center).collect();

        // Should find wells within 1500 units (not all 400)
        // With 1000-unit spacing, we should find ~9-16 wells in a 3x3 to 4x4 area
        assert!(results.len() > 5, "Should find nearby wells: found {}", results.len());
        assert!(results.len() < 50, "Should not find all wells: found {}", results.len());
    }

    #[test]
    fn test_well_grid_query_radius() {
        let mut grid = WellSpatialGrid::new(500.0, 2000.0);

        // Insert wells at various distances from origin
        grid.insert(1, Vec2::new(0.0, 0.0));
        grid.insert(2, Vec2::new(500.0, 0.0));
        grid.insert(3, Vec2::new(1500.0, 0.0));
        grid.insert(4, Vec2::new(3000.0, 0.0));

        // Query with small radius
        let results: Vec<_> = grid.query_radius(Vec2::ZERO, 600.0).collect();
        assert!(results.contains(&1));
        assert!(results.contains(&2));
        assert!(!results.contains(&4)); // Too far

        // Query with large radius
        let results: Vec<_> = grid.query_radius(Vec2::ZERO, 3500.0).collect();
        assert_eq!(results.len(), 4);
    }
}
