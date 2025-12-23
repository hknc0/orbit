use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::game::constants::{arena, mass, spawn};
use crate::util::vec2::Vec2;

/// Unique player identifier
pub type PlayerId = Uuid;

/// Entity identifier for non-player entities
pub type EntityId = u64;

/// Player state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub position: Vec2,
    pub velocity: Vec2,
    pub rotation: f32,
    pub mass: f32,
    pub alive: bool,
    pub kills: u32,
    pub deaths: u32,
    pub spawn_protection: f32,
    pub is_bot: bool,
    pub color_index: u8,
    /// Timer until respawn (0 = can respawn, >0 = waiting)
    #[serde(default)]
    pub respawn_timer: f32,
}

impl Player {
    pub fn new(id: PlayerId, name: String, is_bot: bool, color_index: u8) -> Self {
        Self {
            id,
            name,
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            rotation: 0.0,
            mass: mass::STARTING,
            alive: true,
            kills: 0,
            deaths: 0,
            spawn_protection: spawn::PROTECTION_DURATION,
            is_bot,
            color_index,
            respawn_timer: 0.0,
        }
    }

    /// Get player's collision radius based on mass
    pub fn radius(&self) -> f32 {
        crate::game::constants::mass_to_radius(self.mass)
    }

    /// Check if player has spawn protection active
    pub fn has_spawn_protection(&self) -> bool {
        self.spawn_protection > 0.0
    }

    /// Check if player is in danger zone (too close to center)
    pub fn in_danger_zone(&self) -> bool {
        self.position.length() < arena::CORE_RADIUS
    }

    /// Check if player is outside escape radius
    pub fn outside_arena(&self) -> bool {
        self.position.length() > arena::ESCAPE_RADIUS
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new(Uuid::new_v4(), "Player".to_string(), false, 0)
    }
}

/// Projectile (ejected mass)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projectile {
    pub id: EntityId,
    pub owner_id: PlayerId,
    pub position: Vec2,
    pub velocity: Vec2,
    pub mass: f32,
    pub lifetime: f32,
}

impl Projectile {
    pub fn new(id: EntityId, owner_id: PlayerId, position: Vec2, velocity: Vec2, mass: f32) -> Self {
        Self {
            id,
            owner_id,
            position,
            velocity,
            mass,
            lifetime: crate::game::constants::eject::LIFETIME,
        }
    }

    pub fn radius(&self) -> f32 {
        crate::game::constants::mass_to_radius(self.mass)
    }

    pub fn is_expired(&self) -> bool {
        self.lifetime <= 0.0
    }
}

/// Debris size categories
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DebrisSize {
    Small,
    Medium,
    Large,
}

impl DebrisSize {
    pub fn mass(&self) -> f32 {
        match self {
            DebrisSize::Small => 5.0,
            DebrisSize::Medium => 15.0,
            DebrisSize::Large => 30.0,
        }
    }

    pub fn radius(&self) -> f32 {
        crate::game::constants::mass_to_radius(self.mass())
    }
}

/// Debris (collectible mass fragments)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Debris {
    pub id: EntityId,
    pub position: Vec2,
    pub velocity: Vec2,
    pub size: DebrisSize,
}

impl Debris {
    pub fn new(id: EntityId, position: Vec2, velocity: Vec2, size: DebrisSize) -> Self {
        Self {
            id,
            position,
            velocity,
            size,
        }
    }

    pub fn mass(&self) -> f32 {
        self.size.mass()
    }

    pub fn radius(&self) -> f32 {
        self.size.radius()
    }
}

/// A gravity well (attractor point)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GravityWell {
    pub position: Vec2,
    pub mass: f32,
    pub core_radius: f32, // Death zone radius
}

impl GravityWell {
    pub fn new(position: Vec2, mass: f32, core_radius: f32) -> Self {
        Self { position, mass, core_radius }
    }
}

/// Arena state (zone collapse)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arena {
    pub core_radius: f32,
    pub inner_radius: f32,
    pub middle_radius: f32,
    pub outer_radius: f32,
    pub escape_radius: f32,
    pub collapse_phase: u8,
    pub is_collapsing: bool,
    pub collapse_progress: f32,
    pub time_until_collapse: f32,
    /// Dynamic scale factor based on player count (1.0 = default)
    #[serde(default = "default_scale")]
    pub scale: f32,
    /// Multiple gravity wells
    #[serde(default)]
    pub gravity_wells: Vec<GravityWell>,
}

fn default_scale() -> f32 { 1.0 }

impl Default for Arena {
    fn default() -> Self {
        use crate::game::constants::physics::CENTRAL_MASS;
        Self {
            core_radius: arena::CORE_RADIUS,
            inner_radius: arena::INNER_RADIUS,
            middle_radius: arena::MIDDLE_RADIUS,
            outer_radius: arena::OUTER_RADIUS,
            escape_radius: arena::ESCAPE_RADIUS,
            collapse_phase: 0,
            is_collapsing: false,
            collapse_progress: 0.0,
            time_until_collapse: arena::COLLAPSE_INTERVAL,
            scale: 1.0,
            gravity_wells: vec![GravityWell::new(Vec2::ZERO, CENTRAL_MASS, arena::CORE_RADIUS)],
        }
    }
}

impl Arena {
    /// Get the current safe radius based on collapse progress
    pub fn current_safe_radius(&self) -> f32 {
        let base = self.escape_radius * self.scale;
        let reduction_per_phase = (base - self.core_radius) / arena::COLLAPSE_PHASES as f32;
        base - (self.collapse_phase as f32 * reduction_per_phase)
    }

    /// Get scaled escape radius
    pub fn scaled_escape_radius(&self) -> f32 {
        self.escape_radius * self.scale
    }

    /// Update arena scale and gravity wells based on player count
    /// If `performance_limit` is provided, it caps the number of wells
    pub fn update_for_player_count(&mut self, player_count: usize) {
        self.update_for_player_count_with_limit(player_count, None);
    }

    /// Update arena scale and gravity wells with optional performance-based limit
    pub fn update_for_player_count_with_limit(&mut self, player_count: usize, max_wells: Option<usize>) {
        use crate::game::constants::physics::CENTRAL_MASS;
        use std::f32::consts::TAU;

        // Scale arena: base + 0.1 per player beyond 10
        // Min 1.0, grows slowly with more players
        self.scale = 1.0 + ((player_count.saturating_sub(10)) as f32 * 0.05).min(1.0);

        // Determine number of gravity wells
        // GRAVITY_WELLS env var overrides dynamic calculation
        // Dynamic: 1-15 players: 1 well, 16-30: 2 wells, 31-45: 3 wells, etc.
        let mut well_count = std::env::var("GRAVITY_WELLS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or_else(|| ((player_count + 14) / 15).max(1))
            .max(1); // At least 1 well

        // Apply performance-based limit if provided
        if let Some(limit) = max_wells {
            well_count = well_count.min(limit);
        }

        // Each well is a full star with full mass - more wells = more gravity in the universe
        let mass_per_well = CENTRAL_MASS;
        let core_per_well = arena::CORE_RADIUS;

        // Each well needs space for a full "solar system" around it
        // Spacing ensures orbits around different wells don't overlap
        let well_spacing = arena::OUTER_RADIUS * 2.0; // 1200 units between wells

        // Calculate orbit radius for well placement
        // Wells are arranged in a circle, spaced far apart
        let orbit_radius = if well_count > 1 {
            // For N wells in a circle, we need circumference = N * spacing
            // circumference = 2 * PI * radius, so radius = N * spacing / (2 * PI)
            // Simplified: spread wells so each has ~1200 units to neighbors
            (well_count as f32 * well_spacing) / TAU
        } else {
            0.0
        };

        // Update arena boundaries to encompass all wells
        // Each well needs ESCAPE_RADIUS of space around it
        let universe_radius = orbit_radius + arena::ESCAPE_RADIUS;
        self.escape_radius = universe_radius;
        self.outer_radius = orbit_radius + arena::OUTER_RADIUS;
        self.inner_radius = arena::INNER_RADIUS; // Keep per-well zones as reference
        self.middle_radius = arena::MIDDLE_RADIUS;

        self.gravity_wells.clear();

        // Use random distribution with minimum spacing between wells
        use rand::Rng;
        let mut rng = rand::thread_rng();

        // Minimum distance between wells - at least one full orbital zone apart
        let min_well_distance = well_spacing * 0.8;
        const MAX_PLACEMENT_ATTEMPTS: usize = 50;

        for _ in 0..well_count {
            let position = if well_count == 1 {
                Vec2::ZERO
            } else {
                // Try to find a position that's far enough from existing wells
                let mut best_pos = Vec2::ZERO;
                let mut best_min_dist = 0.0f32;

                for attempt in 0..MAX_PLACEMENT_ATTEMPTS {
                    let angle = rng.gen_range(0.0..TAU);
                    let radius = rng.gen_range(orbit_radius * 0.3..orbit_radius);
                    let candidate = Vec2::from_angle(angle) * radius;

                    // Find minimum distance to any existing well
                    let min_dist = self.gravity_wells
                        .iter()
                        .map(|w| (w.position - candidate).length())
                        .min_by(|a, b| a.partial_cmp(b).unwrap())
                        .unwrap_or(f32::MAX);

                    // If far enough, use it immediately
                    if min_dist >= min_well_distance {
                        best_pos = candidate;
                        break;
                    }

                    // Track best attempt in case we can't find ideal position
                    if min_dist > best_min_dist || attempt == 0 {
                        best_min_dist = min_dist;
                        best_pos = candidate;
                    }
                }
                best_pos
            };
            self.gravity_wells.push(GravityWell::new(position, mass_per_well, core_per_well));
        }
    }
}

/// Match phase
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MatchPhase {
    /// Waiting for players
    Waiting,
    /// Countdown before match starts
    Countdown,
    /// Match in progress
    Playing,
    /// Match ended
    Ended,
}

impl Default for MatchPhase {
    fn default() -> Self {
        Self::Waiting
    }
}

/// Match state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchState {
    pub phase: MatchPhase,
    pub match_time: f32,
    pub countdown_time: f32,
    pub winner_id: Option<PlayerId>,
}

impl Default for MatchState {
    fn default() -> Self {
        Self {
            phase: MatchPhase::Waiting,
            match_time: 0.0,
            countdown_time: crate::game::constants::game::COUNTDOWN,
            winner_id: None,
        }
    }
}

/// Complete game state
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameState {
    pub tick: u64,
    pub match_state: MatchState,
    pub arena: Arena,
    pub players: HashMap<PlayerId, Player>,
    pub projectiles: Vec<Projectile>,
    pub debris: Vec<Debris>,
    next_entity_id: EntityId,
}

impl GameState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Generate a new unique entity ID
    pub fn next_entity_id(&mut self) -> EntityId {
        let id = self.next_entity_id;
        self.next_entity_id += 1;
        id
    }

    /// Get player by ID - O(1) with HashMap
    pub fn get_player(&self, id: PlayerId) -> Option<&Player> {
        self.players.get(&id)
    }

    /// Get mutable player by ID - O(1) with HashMap
    pub fn get_player_mut(&mut self, id: PlayerId) -> Option<&mut Player> {
        self.players.get_mut(&id)
    }

    /// Get all alive players
    pub fn alive_players(&self) -> impl Iterator<Item = &Player> {
        self.players.values().filter(|p| p.alive)
    }

    /// Count alive players
    pub fn alive_count(&self) -> usize {
        self.players.values().filter(|p| p.alive).count()
    }

    /// Count alive human players
    pub fn alive_human_count(&self) -> usize {
        self.players.values().filter(|p| p.alive && !p.is_bot).count()
    }

    /// Add a player to the game - O(1) with HashMap
    pub fn add_player(&mut self, player: Player) {
        self.players.insert(player.id, player);
    }

    /// Remove a player from the game - O(1) with HashMap
    pub fn remove_player(&mut self, id: PlayerId) -> Option<Player> {
        self.players.remove(&id)
    }

    /// Add a projectile
    pub fn add_projectile(&mut self, owner_id: PlayerId, position: Vec2, velocity: Vec2, mass: f32) -> EntityId {
        let id = self.next_entity_id();
        self.projectiles
            .push(Projectile::new(id, owner_id, position, velocity, mass));
        id
    }

    /// Add debris
    pub fn add_debris(&mut self, position: Vec2, velocity: Vec2, size: DebrisSize) -> EntityId {
        let id = self.next_entity_id();
        self.debris.push(Debris::new(id, position, velocity, size));
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_new() {
        let id = Uuid::new_v4();
        let player = Player::new(id, "TestPlayer".to_string(), false, 0);
        assert_eq!(player.id, id);
        assert_eq!(player.name, "TestPlayer");
        assert!(!player.is_bot);
        assert!(player.alive);
        assert_eq!(player.mass, mass::STARTING);
        assert!(player.has_spawn_protection());
    }

    #[test]
    fn test_player_radius() {
        let player = Player::default();
        let expected_radius = (mass::STARTING).sqrt() * mass::RADIUS_SCALE;
        assert!((player.radius() - expected_radius).abs() < 0.001);
    }

    #[test]
    fn test_player_danger_zone() {
        let mut player = Player::default();
        player.position = Vec2::new(10.0, 10.0);
        assert!(player.in_danger_zone());

        player.position = Vec2::new(100.0, 100.0);
        assert!(!player.in_danger_zone());
    }

    #[test]
    fn test_player_outside_arena() {
        let mut player = Player::default();
        player.position = Vec2::new(1000.0, 0.0);
        assert!(player.outside_arena());

        player.position = Vec2::new(100.0, 0.0);
        assert!(!player.outside_arena());
    }

    #[test]
    fn test_projectile_expired() {
        let mut proj = Projectile::new(1, Uuid::new_v4(), Vec2::ZERO, Vec2::ZERO, 10.0);
        assert!(!proj.is_expired());
        proj.lifetime = 0.0;
        assert!(proj.is_expired());
    }

    #[test]
    fn test_debris_size_mass() {
        assert!(DebrisSize::Small.mass() < DebrisSize::Medium.mass());
        assert!(DebrisSize::Medium.mass() < DebrisSize::Large.mass());
    }

    #[test]
    fn test_arena_default() {
        let arena = Arena::default();
        assert_eq!(arena.collapse_phase, 0);
        assert!(!arena.is_collapsing);
        assert!(arena.time_until_collapse > 0.0);
    }

    #[test]
    fn test_arena_safe_radius() {
        let arena = Arena::default();
        let safe_radius = arena.current_safe_radius();
        assert_eq!(safe_radius, arena.escape_radius);
    }

    #[test]
    fn test_match_phase_default() {
        let phase = MatchPhase::default();
        assert_eq!(phase, MatchPhase::Waiting);
    }

    #[test]
    fn test_game_state_entity_ids() {
        let mut state = GameState::new();
        let id1 = state.next_entity_id();
        let id2 = state.next_entity_id();
        assert_ne!(id1, id2);
        assert_eq!(id2, id1 + 1);
    }

    #[test]
    fn test_game_state_add_player() {
        let mut state = GameState::new();
        let player = Player::default();
        let id = player.id;
        state.add_player(player);
        assert!(state.get_player(id).is_some());
    }

    #[test]
    fn test_game_state_remove_player() {
        let mut state = GameState::new();
        let player = Player::default();
        let id = player.id;
        state.add_player(player);
        let removed = state.remove_player(id);
        assert!(removed.is_some());
        assert!(state.get_player(id).is_none());
    }

    #[test]
    fn test_game_state_alive_count() {
        let mut state = GameState::new();

        let mut p1 = Player::default();
        p1.alive = true;
        let mut p2 = Player::default();
        p2.alive = false;
        let mut p3 = Player::default();
        p3.alive = true;
        p3.is_bot = true;

        state.add_player(p1);
        state.add_player(p2);
        state.add_player(p3);

        assert_eq!(state.alive_count(), 2);
        assert_eq!(state.alive_human_count(), 1);
    }

    #[test]
    fn test_game_state_add_projectile() {
        let mut state = GameState::new();
        let owner = Uuid::new_v4();
        let id = state.add_projectile(owner, Vec2::ZERO, Vec2::new(100.0, 0.0), 20.0);
        assert_eq!(state.projectiles.len(), 1);
        assert_eq!(state.projectiles[0].id, id);
        assert_eq!(state.projectiles[0].owner_id, owner);
    }

    #[test]
    fn test_game_state_add_debris() {
        let mut state = GameState::new();
        let id = state.add_debris(Vec2::ZERO, Vec2::ZERO, DebrisSize::Medium);
        assert_eq!(state.debris.len(), 1);
        assert_eq!(state.debris[0].id, id);
    }

    #[test]
    fn test_serialization() {
        let state = GameState::new();
        let encoded = bincode::serde::encode_to_vec(&state, bincode::config::standard()).unwrap();
        let (decoded, _): (GameState, usize) =
            bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(decoded.tick, state.tick);
    }
}
