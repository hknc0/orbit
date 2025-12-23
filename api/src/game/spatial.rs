//! Spatial hash grid for O(n) collision detection
//!
//! Divides the game world into cells and stores entities in each cell.
//! Collision queries only check the current cell and neighbors.

use crate::util::vec2::Vec2;
use hashbrown::HashMap;

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
            cells: HashMap::with_capacity(256),
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
            .or_insert_with(|| Vec::with_capacity(8))
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
    pub fn get_potential_collisions(&self) -> Vec<(SpatialEntity, SpatialEntity)> {
        let mut pairs = Vec::new();

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

        pairs
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
        // 64 units cell size - good for typical player radii of 10-30
        Self::new(64.0)
    }
}

/// Statistics about the spatial grid
#[derive(Debug, Clone)]
pub struct SpatialGridStats {
    pub non_empty_cells: usize,
    pub total_entities: usize,
    pub max_per_cell: usize,
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
}
