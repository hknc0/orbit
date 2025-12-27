use serde::{Deserialize, Serialize};
use std::cell::RefCell;

use crate::game::state::{GameState, MatchPhase, PlayerId, WellId};
use crate::util::vec2::Vec2;

// Thread-local reusable buffers to avoid per-snapshot allocations
thread_local! {
    /// Reusable f32 buffer for density grid calculation (256 elements for 16x16 grid)
    static DENSITY_GRID_BUFFER: RefCell<Vec<f32>> = RefCell::new(Vec::with_capacity(DENSITY_GRID_SIZE * DENSITY_GRID_SIZE));
    /// Reusable u8 buffer for final density grid output
    static DENSITY_GRID_OUTPUT: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(DENSITY_GRID_SIZE * DENSITY_GRID_SIZE));
}

/// Messages from client to server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Request to join a game (as player or spectator)
    JoinRequest {
        player_name: String,
        color_index: u8,
        #[serde(default)]
        is_spectator: bool,
    },
    /// Player input for current tick
    Input(PlayerInput),
    /// Request to leave the game
    Leave,
    /// Ping for latency measurement
    Ping { timestamp: u64 },
    /// Acknowledge receiving a snapshot
    SnapshotAck { tick: u64 },
    /// Spectator: set follow target (None = full view)
    SpectateTarget { target_id: Option<PlayerId> },
    /// Spectator: request to convert to player
    SwitchToPlayer { color_index: u8 },
    /// Viewport info for zoom-aware entity filtering
    /// Allows server to skip sending entities too small to see
    ViewportInfo {
        /// Current zoom level (0.1 = zoomed out, 1.0 = normal)
        zoom: f32,
    },
}

/// Reason for rejecting a join request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RejectionReason {
    /// Server is at player capacity
    ServerFull { current_players: u32 },
    /// Server is at capacity and cannot accept spectators
    SpectatorsFull,
    /// Player name is invalid
    InvalidName,
    /// Too many connection attempts
    RateLimited,
    /// Player is banned/blocked
    Banned,
    /// Server is in maintenance
    Maintenance,
    /// Other reason with custom message
    Other { message: String },
}

/// Messages from server to client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Confirmation of joining with assigned player ID and session token
    JoinAccepted {
        player_id: PlayerId,
        session_token: Vec<u8>,
        #[serde(default)]
        is_spectator: bool,
    },
    /// Join was rejected
    JoinRejected { reason: RejectionReason },
    /// Full game state snapshot
    Snapshot(GameSnapshot),
    /// Delta update (only changed entities)
    Delta(DeltaUpdate),
    /// Game event notification
    Event(GameEvent),
    /// Pong response with server timestamp
    Pong {
        client_timestamp: u64,
        server_timestamp: u64,
    },
    /// Server is kicking the player
    Kicked { reason: String },
    /// Match phase changed
    PhaseChange { phase: MatchPhase, countdown: f32 },
    /// Spectator mode changed (after switch)
    SpectatorModeChanged { is_spectator: bool },
}

/// Player input state for one tick
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerInput {
    /// Input sequence number (for reconciliation)
    pub sequence: u64,
    /// Server tick this input is for
    pub tick: u64,
    /// Client timestamp for RTT measurement (ms since page load)
    #[serde(default)]
    pub client_time: u64,
    /// Thrust direction (normalized, -1 to 1 on each axis)
    pub thrust: Vec2,
    /// Aim direction (normalized)
    pub aim: Vec2,
    /// Boost button pressed
    pub boost: bool,
    /// Fire button pressed
    pub fire: bool,
    /// Fire button just released (for charge release)
    pub fire_released: bool,
}

impl PlayerInput {
    #[allow(dead_code)]
    pub fn new(sequence: u64, tick: u64) -> Self {
        Self {
            sequence,
            tick,
            ..Default::default()
        }
    }
}

/// Gravity well snapshot for network transmission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GravityWellSnapshot {
    /// Unique stable well ID
    pub id: WellId,
    pub position: Vec2,
    pub mass: f32,
    pub core_radius: f32,
}

impl GravityWellSnapshot {
    pub fn from_gravity_well(well: &crate::game::state::GravityWell) -> Self {
        Self {
            id: well.id,
            position: well.position,
            mass: well.mass,
            core_radius: well.core_radius,
        }
    }
}

/// Player density grid for minimap (16x16 = 256 cells for detailed heatmap)
pub const DENSITY_GRID_SIZE: usize = 16;

/// Compressed game state for network transmission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSnapshot {
    pub tick: u64,
    pub match_phase: MatchPhase,
    pub match_time: f32,
    pub countdown: f32,
    pub players: Vec<PlayerSnapshot>,
    pub projectiles: Vec<ProjectileSnapshot>,
    /// Debris (collectible particles) in view
    #[serde(default)]
    pub debris: Vec<DebrisSnapshot>,
    pub arena_collapse_phase: u8,
    pub arena_safe_radius: f32,
    /// Arena scale factor (1.0 = default size)
    #[serde(default = "default_scale")]
    pub arena_scale: f32,
    /// Gravity wells in the arena
    #[serde(default)]
    pub gravity_wells: Vec<GravityWellSnapshot>,
    /// Total players in match (before AOI filtering)
    #[serde(default)]
    pub total_players: u32,
    /// Total alive players in match (before AOI filtering)
    #[serde(default)]
    pub total_alive: u32,
    /// Player density grid for minimap (16x16, each cell = player count)
    #[serde(default)]
    pub density_grid: Vec<u8>,
    /// Echo of client's last input timestamp for RTT measurement
    #[serde(default)]
    pub echo_client_time: u64,
    /// AI Manager status (if enabled)
    #[serde(default)]
    pub ai_status: Option<AIStatusSnapshot>,
}

/// AI Manager status for client display
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AIStatusSnapshot {
    /// Whether AI manager is enabled
    pub enabled: bool,
    /// Last decision summary (what the AI decided)
    pub last_decision: Option<String>,
    /// Confidence of last decision (0-100)
    pub confidence: u8,
    /// Success rate percentage (0-100)
    pub success_rate: u8,
    /// Total decisions made
    pub decisions_total: u32,
    /// Decisions successful
    pub decisions_successful: u32,
}

fn default_scale() -> f32 { 1.0 }

impl GameSnapshot {
    pub fn from_game_state(state: &GameState) -> Self {
        let total_players = state.players.len() as u32;
        let total_alive = state.players.values().filter(|p| p.alive).count() as u32;

        // Calculate density grid for minimap heatmap
        let density_grid = Self::calculate_density_grid(state);

        Self {
            tick: state.tick,
            match_phase: state.match_state.phase,
            match_time: state.match_state.match_time,
            countdown: state.match_state.countdown_time,
            players: state
                .players
                .values()
                .map(PlayerSnapshot::from_player)
                .collect(),
            projectiles: state
                .projectiles
                .iter()
                .map(ProjectileSnapshot::from_projectile)
                .collect(),
            debris: state
                .debris
                .iter()
                .map(DebrisSnapshot::from_debris)
                .collect(),
            arena_collapse_phase: state.arena.collapse_phase,
            arena_safe_radius: state.arena.current_safe_radius(),
            arena_scale: state.arena.scale,
            gravity_wells: state
                .arena
                .gravity_wells
                .values()
                .map(GravityWellSnapshot::from_gravity_well)
                .collect(),
            total_players,
            total_alive,
            density_grid,
            echo_client_time: 0, // Set per-player in broadcast
            ai_status: None, // Set by game_session when AI manager is active
        }
    }

    /// Calculate mass density grid (16x16) for minimap heatmap
    /// Includes: player mass + gravity well influence (1/r falloff)
    ///
    /// OPTIMIZATION: Uses thread-local buffers to avoid per-snapshot allocations
    fn calculate_density_grid(state: &GameState) -> Vec<u8> {
        DENSITY_GRID_BUFFER.with(|buffer_cell| {
            DENSITY_GRID_OUTPUT.with(|output_cell| {
                let mut grid = buffer_cell.borrow_mut();
                let mut output = output_cell.borrow_mut();
                let grid_size = DENSITY_GRID_SIZE * DENSITY_GRID_SIZE;

                // Reuse buffer: clear and resize instead of allocating
                grid.clear();
                grid.resize(grid_size, 0.0f32);

                let arena_radius = state.arena.current_safe_radius();
                let cell_size = (arena_radius * 2.0) / DENSITY_GRID_SIZE as f32;
                let inv_cell_size = 1.0 / cell_size; // Multiply instead of divide

                // 1. Add player MASS to cells (O(players) - fast)
                for player in state.players.values() {
                    if !player.alive {
                        continue;
                    }

                    let gx = ((player.position.x + arena_radius) * inv_cell_size) as usize;
                    let gy = ((player.position.y + arena_radius) * inv_cell_size) as usize;

                    if gx < DENSITY_GRID_SIZE && gy < DENSITY_GRID_SIZE {
                        grid[gy * DENSITY_GRID_SIZE + gx] += player.mass;
                    }
                }

                // 2. Add gravity well influence (O(cells × wells) = ~4000 ops)
                for gy in 0..DENSITY_GRID_SIZE {
                    let cell_y = (gy as f32 + 0.5) * cell_size - arena_radius;
                    for gx in 0..DENSITY_GRID_SIZE {
                        let cell_x = (gx as f32 + 0.5) * cell_size - arena_radius;
                        let idx = gy * DENSITY_GRID_SIZE + gx;

                        for well in state.arena.gravity_wells.values() {
                            let dx = well.position.x - cell_x;
                            let dy = well.position.y - cell_y;
                            let dist_sq = dx * dx + dy * dy;
                            let min_dist = well.core_radius * 2.0;
                            let min_dist_sq = min_dist * min_dist;

                            // 1/r falloff matching physics, clamped at core
                            let influence = if dist_sq < min_dist_sq {
                                well.mass / min_dist
                            } else {
                                well.mass / dist_sq.sqrt()
                            };

                            grid[idx] += influence;
                        }
                    }
                }

                // 3. Normalize to u8 (0-255)
                let max_density = grid.iter().cloned().fold(0.0f32, f32::max).max(1.0);
                let scale = 255.0 / max_density;

                // Reuse output buffer: clear and fill
                output.clear();
                output.extend(grid.iter().map(|&d| (d * scale).min(255.0) as u8));

                // Clone the output (required since we return from thread-local borrow)
                output.clone()
            })
        })
    }
}

/// Player flags - bit-packed booleans for bandwidth efficiency
/// OPTIMIZATION: Packs 3 bools into 1 byte (saves 2 bytes per player per snapshot)
pub mod player_flags {
    pub const ALIVE: u8 = 0b0000_0001;
    pub const SPAWN_PROTECTION: u8 = 0b0000_0010;
    pub const IS_BOT: u8 = 0b0000_0100;
}

/// Compressed player state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub id: PlayerId,
    pub name: String,
    pub position: Vec2,
    pub velocity: Vec2,
    pub rotation: f32,
    pub mass: f32,
    /// Bit-packed flags: bit 0 = alive, bit 1 = spawn_protection, bit 2 = is_bot
    pub flags: u8,
    pub kills: u32,
    pub deaths: u32,
    pub color_index: u8,
    /// Tick when player spawned/respawned (for birth animation detection)
    #[serde(default)]
    pub spawn_tick: u64,
}

impl PlayerSnapshot {
    pub fn from_player(player: &crate::game::state::Player) -> Self {
        let mut flags = 0u8;
        if player.alive {
            flags |= player_flags::ALIVE;
        }
        if player.spawn_protection > 0.0 {
            flags |= player_flags::SPAWN_PROTECTION;
        }
        if player.is_bot {
            flags |= player_flags::IS_BOT;
        }

        Self {
            id: player.id,
            name: player.name.clone(),
            position: player.position,
            velocity: player.velocity,
            rotation: player.rotation,
            mass: player.mass,
            flags,
            kills: player.kills,
            deaths: player.deaths,
            color_index: player.color_index,
            spawn_tick: player.spawn_tick,
        }
    }

    /// Check if player is alive
    #[inline]
    pub fn alive(&self) -> bool {
        self.flags & player_flags::ALIVE != 0
    }

    /// Check if player has spawn protection
    #[inline]
    pub fn spawn_protection(&self) -> bool {
        self.flags & player_flags::SPAWN_PROTECTION != 0
    }

    /// Check if player is a bot
    #[inline]
    pub fn is_bot(&self) -> bool {
        self.flags & player_flags::IS_BOT != 0
    }
}

/// Compressed projectile state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectileSnapshot {
    pub id: u64,
    pub owner_id: PlayerId,
    pub position: Vec2,
    pub velocity: Vec2,
    pub mass: f32,
}

impl ProjectileSnapshot {
    pub fn from_projectile(proj: &crate::game::state::Projectile) -> Self {
        Self {
            id: proj.id,
            owner_id: proj.owner_id,
            position: proj.position,
            velocity: proj.velocity,
            mass: proj.mass,
        }
    }
}

/// Debris (collectible particle) snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebrisSnapshot {
    pub id: u64,
    pub position: Vec2,
    pub size: u8, // 0=Small, 1=Medium, 2=Large
}

impl DebrisSnapshot {
    pub fn from_debris(debris: &crate::game::state::Debris) -> Self {
        use crate::game::state::DebrisSize;
        Self {
            id: debris.id,
            position: debris.position,
            size: match debris.size {
                DebrisSize::Small => 0,
                DebrisSize::Medium => 1,
                DebrisSize::Large => 2,
            },
        }
    }
}

/// Delta update containing only changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaUpdate {
    pub tick: u64,
    pub base_tick: u64,
    pub player_updates: Vec<PlayerDelta>,
    pub projectile_updates: Vec<ProjectileDelta>,
    pub removed_projectiles: Vec<u64>,
    /// Full debris list (debris moves slowly, sending full list is efficient)
    pub debris: Vec<DebrisSnapshot>,
}

/// Delta for a single player
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerDelta {
    pub id: PlayerId,
    pub position: Option<Vec2>,
    pub velocity: Option<Vec2>,
    pub rotation: Option<f32>,
    pub mass: Option<f32>,
    pub alive: Option<bool>,
    pub kills: Option<u32>,
}

/// Delta for a projectile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectileDelta {
    pub id: u64,
    pub position: Vec2,
    pub velocity: Vec2,
}

/// Game events that clients should be notified about
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameEvent {
    /// A player was killed
    PlayerKilled {
        killer_id: PlayerId,
        victim_id: PlayerId,
        killer_name: String,
        victim_name: String,
    },
    /// A player joined
    PlayerJoined { player_id: PlayerId, name: String },
    /// A player left
    PlayerLeft { player_id: PlayerId, name: String },
    /// Match started
    MatchStarted,
    /// Match ended with a winner
    MatchEnded { winner_id: Option<PlayerId>, winner_name: Option<String> },
    /// Zone is collapsing
    ZoneCollapse { phase: u8, new_safe_radius: f32 },
    /// Two players collided and deflected (both survived)
    PlayerDeflection {
        player_a: PlayerId,
        player_b: PlayerId,
        /// Collision midpoint position
        position: Vec2,
        /// Intensity 0-1 for visual effect scaling
        intensity: f32,
    },
    /// A gravity well is charging up (warning before explosion)
    GravityWellCharging {
        /// Unique well ID (stable across removals)
        well_id: WellId,
        /// Well position
        position: Vec2,
    },
    /// A gravity well exploded, creating an expanding wave
    GravityWaveExplosion {
        /// Unique well ID that exploded
        well_id: WellId,
        /// Center position of the explosion
        position: Vec2,
        /// Wave strength (0-1, based on well mass)
        strength: f32,
    },
    /// A gravity well was destroyed (removed from arena)
    GravityWellDestroyed {
        /// Unique well ID that was destroyed
        well_id: WellId,
        /// Well position before removal
        position: Vec2,
    },
}

/// Encode a message using bincode (used in tests, production uses encode_pooled)
/// Uses legacy config for fixed-size integers (compatible with TypeScript client)
#[allow(dead_code)] // Used in tests, production uses encode_pooled
pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, EncodeError> {
    bincode::serde::encode_to_vec(message, bincode::config::legacy())
        .map_err(|e| EncodeError(e.to_string()))
}

/// Decode a message using bincode
/// Uses legacy config for fixed-size integers (compatible with TypeScript client)
pub fn decode<T: for<'de> Deserialize<'de>>(data: &[u8]) -> Result<T, DecodeError> {
    bincode::serde::decode_from_slice(data, bincode::config::legacy())
        .map(|(msg, _)| msg)
        .map_err(|e| DecodeError(e.to_string()))
}

#[allow(dead_code)] // Used by encode() for tests
#[derive(Debug, thiserror::Error)]
#[error("Encode error: {0}")]
pub struct EncodeError(String);

#[derive(Debug, thiserror::Error)]
#[error("Decode error: {0}")]
pub struct DecodeError(String);

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_client_message_join() {
        let msg = ClientMessage::JoinRequest {
            player_name: "TestPlayer".to_string(),
            color_index: 3,
            is_spectator: false,
        };
        let encoded = encode(&msg).unwrap();
        let decoded: ClientMessage = decode(&encoded).unwrap();
        match decoded {
            ClientMessage::JoinRequest { player_name, color_index, is_spectator } => {
                assert_eq!(player_name, "TestPlayer");
                assert_eq!(color_index, 3);
                assert!(!is_spectator);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_client_message_join_spectator() {
        let msg = ClientMessage::JoinRequest {
            player_name: "Spectator".to_string(),
            color_index: 0,
            is_spectator: true,
        };
        let encoded = encode(&msg).unwrap();
        let decoded: ClientMessage = decode(&encoded).unwrap();
        match decoded {
            ClientMessage::JoinRequest { player_name, is_spectator, .. } => {
                assert_eq!(player_name, "Spectator");
                assert!(is_spectator);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_client_message_viewport_info() {
        let msg = ClientMessage::ViewportInfo { zoom: 0.15 };
        let encoded = encode(&msg).unwrap();
        let decoded: ClientMessage = decode(&encoded).unwrap();
        match decoded {
            ClientMessage::ViewportInfo { zoom } => {
                assert!((zoom - 0.15).abs() < 0.001);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_client_message_input() {
        let input = PlayerInput {
            sequence: 42,
            tick: 100,
            client_time: 0,
            thrust: Vec2::new(0.5, -0.3),
            aim: Vec2::new(1.0, 0.0),
            boost: true,
            fire: false,
            fire_released: false,
        };
        let msg = ClientMessage::Input(input);
        let encoded = encode(&msg).unwrap();
        let decoded: ClientMessage = decode(&encoded).unwrap();
        match decoded {
            ClientMessage::Input(i) => {
                assert_eq!(i.sequence, 42);
                assert_eq!(i.tick, 100);
                assert!(i.boost);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_server_message_join_accepted() {
        let player_id = Uuid::new_v4();
        let msg = ServerMessage::JoinAccepted {
            player_id,
            session_token: vec![1, 2, 3, 4],
            is_spectator: false,
        };
        let encoded = encode(&msg).unwrap();
        let decoded: ServerMessage = decode(&encoded).unwrap();
        match decoded {
            ServerMessage::JoinAccepted {
                player_id: pid,
                session_token,
                is_spectator,
            } => {
                assert_eq!(pid, player_id);
                assert_eq!(session_token, vec![1, 2, 3, 4]);
                assert!(!is_spectator);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_server_message_spectator_mode_changed() {
        let msg = ServerMessage::SpectatorModeChanged { is_spectator: true };
        let encoded = encode(&msg).unwrap();
        let decoded: ServerMessage = decode(&encoded).unwrap();
        match decoded {
            ServerMessage::SpectatorModeChanged { is_spectator } => {
                assert!(is_spectator);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_game_snapshot_serialization() {
        let snapshot = GameSnapshot {
            tick: 1000,
            match_phase: MatchPhase::Playing,
            match_time: 45.5,
            countdown: 0.0,
            players: vec![PlayerSnapshot {
                id: Uuid::new_v4(),
                name: "TestPlayer".to_string(),
                position: Vec2::new(100.0, 200.0),
                velocity: Vec2::new(10.0, -5.0),
                rotation: 1.5,
                mass: 150.0,
                flags: player_flags::ALIVE, // alive=true, spawn_protection=false, is_bot=false
                kills: 3,
                deaths: 1,
                color_index: 2,
                spawn_tick: 0,
            }],
            projectiles: vec![],
            debris: vec![DebrisSnapshot {
                id: 1,
                position: Vec2::new(50.0, 50.0),
                size: 0,
            }],
            arena_collapse_phase: 2,
            arena_safe_radius: 600.0,
            arena_scale: 1.0,
            gravity_wells: vec![GravityWellSnapshot {
                id: 0,
                position: Vec2::ZERO,
                mass: 10000.0,
                core_radius: 50.0,
            }],
            total_players: 1,
            total_alive: 1,
            density_grid: vec![0; 64],
            echo_client_time: 0,
            ai_status: None,
        };

        let encoded = encode(&snapshot).unwrap();
        let decoded: GameSnapshot = decode(&encoded).unwrap();

        assert_eq!(decoded.tick, 1000);
        assert_eq!(decoded.match_phase, MatchPhase::Playing);
        assert_eq!(decoded.players.len(), 1);
        assert_eq!(decoded.players[0].kills, 3);
        assert_eq!(decoded.players[0].deaths, 1);
        assert_eq!(decoded.total_players, 1);
        assert_eq!(decoded.total_alive, 1);
        assert_eq!(decoded.players[0].name, "TestPlayer");
        assert_eq!(decoded.gravity_wells.len(), 1);
        assert_eq!(decoded.density_grid.len(), 64);
        assert_eq!(decoded.debris.len(), 1);
    }

    #[test]
    fn test_game_event_serialization() {
        let event = GameEvent::PlayerKilled {
            killer_id: Uuid::new_v4(),
            victim_id: Uuid::new_v4(),
            killer_name: "Alice".to_string(),
            victim_name: "Bob".to_string(),
        };

        let encoded = encode(&event).unwrap();
        let decoded: GameEvent = decode(&encoded).unwrap();

        match decoded {
            GameEvent::PlayerKilled {
                killer_name,
                victim_name,
                ..
            } => {
                assert_eq!(killer_name, "Alice");
                assert_eq!(victim_name, "Bob");
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_player_input_default() {
        let input = PlayerInput::default();
        assert_eq!(input.sequence, 0);
        assert_eq!(input.tick, 0);
        assert_eq!(input.thrust, Vec2::ZERO);
        assert!(!input.boost);
        assert!(!input.fire);
    }

    #[test]
    fn test_delta_update_serialization() {
        let delta = DeltaUpdate {
            tick: 500,
            base_tick: 490,
            player_updates: vec![PlayerDelta {
                id: Uuid::new_v4(),
                position: Some(Vec2::new(50.0, 60.0)),
                velocity: None,
                rotation: Some(2.0),
                mass: None,
                alive: None,
                kills: Some(1),
            }],
            projectile_updates: vec![],
            removed_projectiles: vec![1, 2, 3],
            debris: vec![DebrisSnapshot {
                id: 1,
                position: Vec2::new(100.0, 200.0),
                size: 1,
            }],
        };

        let encoded = encode(&delta).unwrap();
        let decoded: DeltaUpdate = decode(&encoded).unwrap();

        assert_eq!(decoded.tick, 500);
        assert_eq!(decoded.removed_projectiles.len(), 3);
        assert_eq!(decoded.debris.len(), 1);
    }

    #[test]
    fn test_invalid_decode() {
        let garbage = vec![0xFF, 0xFE, 0xFD];
        let result: Result<ClientMessage, _> = decode(&garbage);
        assert!(result.is_err());
    }

    #[test]
    fn test_density_grid_pooling() {
        // Test that density grid calculation works correctly with pooled buffers
        // and produces consistent results across multiple calls
        use crate::game::state::GameState;

        let state = GameState::new();

        // First calculation
        let grid1 = GameSnapshot::calculate_density_grid(&state);
        assert_eq!(grid1.len(), DENSITY_GRID_SIZE * DENSITY_GRID_SIZE);

        // Second calculation (should reuse buffers and produce same result)
        let grid2 = GameSnapshot::calculate_density_grid(&state);
        assert_eq!(grid2.len(), DENSITY_GRID_SIZE * DENSITY_GRID_SIZE);

        // Results should be identical for the same state
        assert_eq!(grid1, grid2, "Pooled buffers should produce consistent results");

        // Verify non-zero values (gravity wells should produce density)
        let non_zero = grid1.iter().filter(|&&v| v > 0).count();
        assert!(non_zero > 0, "Density grid should have non-zero values from gravity wells");
    }

    #[test]
    fn test_density_grid_pooling_multiple_calls() {
        // Stress test: verify pooling works correctly across many calls
        use crate::game::state::GameState;

        let state = GameState::new();

        // Call many times to ensure buffer reuse works correctly
        for i in 0..100 {
            let grid = GameSnapshot::calculate_density_grid(&state);
            assert_eq!(
                grid.len(),
                DENSITY_GRID_SIZE * DENSITY_GRID_SIZE,
                "Grid size wrong on iteration {}",
                i
            );
        }
    }

}

#[cfg(test)]
mod encoding_tests {
    use super::*;

    #[test]
    fn inspect_server_message_encoding() {
        // Test JoinAccepted
        let msg = ServerMessage::JoinAccepted {
            player_id: uuid::Uuid::nil(),
            session_token: vec![1, 2, 3, 4],
            is_spectator: false,
        };
        let encoded = encode(&msg).unwrap();
        println!("\n=== JoinAccepted ===");
        println!("Encoded bytes: {:?}", encoded);
        println!("Hex: {}", encoded.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
        println!("Length: {}", encoded.len());
        
        if encoded.len() >= 4 {
            let variant = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]);
            println!("Variant discriminant (u32): {}", variant);
        }
        
        // Test JoinRejected
        let msg2 = ServerMessage::JoinRejected {
            reason: RejectionReason::ServerFull { current_players: 1500 },
        };
        let encoded2 = encode(&msg2).unwrap();
        println!("\n=== JoinRejected ===");
        println!("Encoded bytes: {:?}", encoded2);
        println!("Hex: {}", encoded2.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
        
        if encoded2.len() >= 4 {
            let variant = u32::from_le_bytes([encoded2[0], encoded2[1], encoded2[2], encoded2[3]]);
            println!("Variant discriminant (u32): {}", variant);
        }
        
        // Test Kicked
        let msg3 = ServerMessage::Kicked {
            reason: "Test kick".to_string(),
        };
        let encoded3 = encode(&msg3).unwrap();
        println!("\n=== Kicked ===");
        println!("Encoded bytes: {:?}", encoded3);
        println!("Hex: {}", encoded3.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
        
        if encoded3.len() >= 4 {
            let variant = u32::from_le_bytes([encoded3[0], encoded3[1], encoded3[2], encoded3[3]]);
            println!("Variant discriminant (u32): {}", variant);
        }
        
        // Now let's see the string encoding
        println!("\n=== String 'Test' encoding ===");
        let test_str = "Test".to_string();
        let str_encoded = encode(&test_str).unwrap();
        println!("Encoded bytes: {:?}", str_encoded);
        println!("Hex: {}", str_encoded.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
        
        if str_encoded.len() >= 8 {
            let len = u64::from_le_bytes([
                str_encoded[0], str_encoded[1], str_encoded[2], str_encoded[3],
                str_encoded[4], str_encoded[5], str_encoded[6], str_encoded[7],
            ]);
            println!("String length prefix (u64): {}", len);
        }
    }
}

    #[test]
    fn inspect_uuid_serialization() {
        use uuid::Uuid;
        
        // Create a non-nil UUID to see the actual bytes
        let uuid = Uuid::from_bytes([
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10
        ]);
        
        let encoded = encode(&uuid).unwrap();
        println!("\n=== UUID Encoding ===");
        println!("UUID: {}", uuid);
        println!("Encoded bytes: {:?}", encoded);
        println!("Hex: {}", encoded.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
        println!("Length: {}", encoded.len());
        
        if encoded.len() >= 8 {
            let potential_len = u64::from_le_bytes([
                encoded[0], encoded[1], encoded[2], encoded[3],
                encoded[4], encoded[5], encoded[6], encoded[7],
            ]);
            println!("First 8 bytes as u64: {}", potential_len);
        }
        
        // Now encode just the raw bytes
        let raw_bytes = uuid.as_bytes();
        let raw_encoded = encode(&raw_bytes).unwrap();
        println!("\n=== Raw [u8; 16] Encoding ===");
        println!("Encoded bytes: {:?}", raw_encoded);
        println!("Hex: {}", raw_encoded.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
        
        // And as a Vec
        let vec_bytes: Vec<u8> = uuid.as_bytes().to_vec();
        let vec_encoded = encode(&vec_bytes).unwrap();
        println!("\n=== Vec<u8> Encoding ===");
        println!("Encoded bytes: {:?}", vec_encoded);
        println!("Hex: {}", vec_encoded.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
    }

    #[test]
    fn inspect_game_event_encoding() {
        use uuid::Uuid;
        
        let event = GameEvent::PlayerKilled {
            killer_id: Uuid::from_bytes([0x01; 16]),
            victim_id: Uuid::from_bytes([0x02; 16]),
            killer_name: "Alice".to_string(),
            victim_name: "Bob".to_string(),
        };
        
        let encoded = encode(&event).unwrap();
        println!("\n=== PlayerKilled Event ===");
        println!("Encoded bytes: {:?}", &encoded[..]);
        println!("Hex: {}", encoded.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
        println!("Length: {}", encoded.len());
        
        let mut offset = 0;
        if encoded.len() >= 4 {
            let variant = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]);
            println!("Event variant: {}", variant);
            offset = 4;
        }
        
        // First UUID
        if offset + 8 <= encoded.len() {
            let uuid_len = u64::from_le_bytes([
                encoded[offset], encoded[offset+1], encoded[offset+2], encoded[offset+3],
                encoded[offset+4], encoded[offset+5], encoded[offset+6], encoded[offset+7],
            ]);
            println!("Killer UUID length at offset {}: {}", offset, uuid_len);
            offset += 8 + uuid_len as usize;
        }
        
        // Second UUID
        if offset + 8 <= encoded.len() {
            let uuid_len = u64::from_le_bytes([
                encoded[offset], encoded[offset+1], encoded[offset+2], encoded[offset+3],
                encoded[offset+4], encoded[offset+5], encoded[offset+6], encoded[offset+7],
            ]);
            println!("Victim UUID length at offset {}: {}", offset, uuid_len);
            offset += 8 + uuid_len as usize;
        }
        
        // Killer name
        if offset + 8 <= encoded.len() {
            let name_len = u64::from_le_bytes([
                encoded[offset], encoded[offset+1], encoded[offset+2], encoded[offset+3],
                encoded[offset+4], encoded[offset+5], encoded[offset+6], encoded[offset+7],
            ]);
            println!("Killer name length at offset {}: {}", offset, name_len);
            offset += 8;
            let name = String::from_utf8_lossy(&encoded[offset..offset+name_len as usize]);
            println!("Killer name: {}", name);
            offset += name_len as usize;
        }
        
        // Victim name
        if offset + 8 <= encoded.len() {
            let name_len = u64::from_le_bytes([
                encoded[offset], encoded[offset+1], encoded[offset+2], encoded[offset+3],
                encoded[offset+4], encoded[offset+5], encoded[offset+6], encoded[offset+7],
            ]);
            println!("Victim name length at offset {}: {}", offset, name_len);
        }
    }

    #[test]
    fn inspect_match_phase_encoding() {
        use crate::game::state::MatchPhase;
        
        println!("\n=== MatchPhase Encoding ===");
        
        for (name, phase) in [
            ("Waiting", MatchPhase::Waiting),
            ("Countdown", MatchPhase::Countdown),
            ("Playing", MatchPhase::Playing),
            ("Ended", MatchPhase::Ended),
        ] {
            let encoded = encode(&phase).unwrap();
            println!("{}: {:?} (hex: {})", name, encoded, 
                     encoded.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
            if encoded.len() >= 4 {
                let variant = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]);
                println!("  Variant as u32: {}", variant);
            }
        }
    }

    #[test]
    fn inspect_snapshot_encoding() {
        use uuid::Uuid;
        
        let snapshot = GameSnapshot {
            tick: 100,
            match_phase: MatchPhase::Playing,
            match_time: 45.5,
            countdown: 0.0,
            players: vec![PlayerSnapshot {
                id: Uuid::from_bytes([0xAA; 16]),
                name: "TestPlayer".to_string(),
                position: Vec2::new(10.0, 20.0),
                velocity: Vec2::new(1.0, 2.0),
                rotation: 0.5,
                mass: 100.0,
                flags: player_flags::ALIVE, // alive=true, spawn_protection=false, is_bot=false
                kills: 3,
                deaths: 0,
                color_index: 2,
                spawn_tick: 0,
            }],
            projectiles: vec![],
            debris: vec![],
            arena_collapse_phase: 1,
            arena_safe_radius: 500.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: 1,
            total_alive: 1,
            density_grid: vec![],
            echo_client_time: 0,
            ai_status: None,
        };

        let encoded = encode(&snapshot).unwrap();
        println!("\n=== GameSnapshot Encoding ===");
        println!("Total length: {}", encoded.len());
        println!("First 100 bytes hex:");
        println!("{}", encoded.iter().take(100).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
        
        let mut offset = 0;
        let tick = u64::from_le_bytes(encoded[offset..offset+8].try_into().unwrap());
        println!("\nOffset {}: tick = {}", offset, tick);
        offset += 8;
        
        let phase = u32::from_le_bytes(encoded[offset..offset+4].try_into().unwrap());
        println!("Offset {}: match_phase = {}", offset, phase);
        offset += 4;
        
        let match_time = f32::from_le_bytes(encoded[offset..offset+4].try_into().unwrap());
        println!("Offset {}: match_time = {}", offset, match_time);
        offset += 4;
        
        let countdown = f32::from_le_bytes(encoded[offset..offset+4].try_into().unwrap());
        println!("Offset {}: countdown = {}", offset, countdown);
        offset += 4;
        
        let player_count = u64::from_le_bytes(encoded[offset..offset+8].try_into().unwrap());
        println!("Offset {}: player_count = {}", offset, player_count);
        offset += 8;
        
        println!("Offset {}: First player UUID length (should be 16)", offset);
        let uuid_len = u64::from_le_bytes(encoded[offset..offset+8].try_into().unwrap());
        println!("  UUID length = {}", uuid_len);
    }
