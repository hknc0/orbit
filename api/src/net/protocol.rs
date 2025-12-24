use serde::{Deserialize, Serialize};

use crate::game::state::{GameState, MatchPhase, PlayerId, WellId};
use crate::util::vec2::Vec2;

/// Messages from client to server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Request to join a game
    JoinRequest { player_name: String, color_index: u8 },
    /// Player input for current tick
    Input(PlayerInput),
    /// Request to leave the game
    Leave,
    /// Ping for latency measurement
    Ping { timestamp: u64 },
    /// Acknowledge receiving a snapshot
    SnapshotAck { tick: u64 },
}

/// Messages from server to client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Confirmation of joining with assigned player ID and session token
    JoinAccepted {
        player_id: PlayerId,
        session_token: Vec<u8>,
    },
    /// Join was rejected
    JoinRejected { reason: String },
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
    /// Notable players (high mass) for minimap radar - shown regardless of AOI
    #[serde(default)]
    pub notable_players: Vec<NotablePlayer>,
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

/// Minimum mass to appear on minimap radar
pub const NOTABLE_MASS_THRESHOLD: f32 = 80.0;
/// Maximum notable players to send
pub const MAX_NOTABLE_PLAYERS: usize = 15;

/// Compact player info for minimap radar (high-mass players visible everywhere)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotablePlayer {
    pub id: PlayerId,
    pub position: Vec2,
    pub mass: f32,
    pub color_index: u8,
}

fn default_scale() -> f32 { 1.0 }

impl GameSnapshot {
    pub fn from_game_state(state: &GameState) -> Self {
        let total_players = state.players.len() as u32;
        let total_alive = state.players.values().filter(|p| p.alive).count() as u32;

        // Calculate density grid for minimap
        let density_grid = Self::calculate_density_grid(state);

        // Calculate notable players (high mass) for minimap radar
        let notable_players = Self::calculate_notable_players(state);

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
            notable_players,
            echo_client_time: 0, // Set per-player in broadcast
            ai_status: None, // Set by game_session when AI manager is active
        }
    }

    /// Calculate player density grid (16x16) for minimap heatmap
    fn calculate_density_grid(state: &GameState) -> Vec<u8> {
        let mut grid = vec![0u8; DENSITY_GRID_SIZE * DENSITY_GRID_SIZE];
        // Use current_safe_radius() to match minimap scaling on client
        let arena_radius = state.arena.current_safe_radius();
        let cell_size = (arena_radius * 2.0) / DENSITY_GRID_SIZE as f32;

        for player in state.players.values() {
            if !player.alive {
                continue;
            }

            // Convert position to grid cell (centered at origin)
            let grid_x = ((player.position.x + arena_radius) / cell_size) as usize;
            let grid_y = ((player.position.y + arena_radius) / cell_size) as usize;

            // Clamp to grid bounds
            let grid_x = grid_x.min(DENSITY_GRID_SIZE - 1);
            let grid_y = grid_y.min(DENSITY_GRID_SIZE - 1);

            let idx = grid_y * DENSITY_GRID_SIZE + grid_x;
            // Saturating add to prevent overflow
            grid[idx] = grid[idx].saturating_add(1);
        }

        grid
    }

    /// Calculate notable players (high mass) for minimap radar
    fn calculate_notable_players(state: &GameState) -> Vec<NotablePlayer> {
        let mut notable: Vec<_> = state
            .players
            .values()
            .filter(|p| p.alive && p.mass >= NOTABLE_MASS_THRESHOLD)
            .map(|p| NotablePlayer {
                id: p.id,
                position: p.position,
                mass: p.mass,
                color_index: p.color_index,
            })
            .collect();

        // Sort by mass descending and take top N
        notable.sort_by(|a, b| b.mass.partial_cmp(&a.mass).unwrap_or(std::cmp::Ordering::Equal));
        notable.truncate(MAX_NOTABLE_PLAYERS);
        notable
    }
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
    pub alive: bool,
    pub kills: u32,
    pub deaths: u32,
    pub spawn_protection: bool,
    pub is_bot: bool,
    pub color_index: u8,
}

impl PlayerSnapshot {
    pub fn from_player(player: &crate::game::state::Player) -> Self {
        Self {
            id: player.id,
            name: player.name.clone(),
            position: player.position,
            velocity: player.velocity,
            rotation: player.rotation,
            mass: player.mass,
            alive: player.alive,
            kills: player.kills,
            deaths: player.deaths,
            spawn_protection: player.spawn_protection > 0.0,
            is_bot: player.is_bot,
            color_index: player.color_index,
        }
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
        };
        let encoded = encode(&msg).unwrap();
        let decoded: ClientMessage = decode(&encoded).unwrap();
        match decoded {
            ClientMessage::JoinRequest { player_name, color_index } => {
                assert_eq!(player_name, "TestPlayer");
                assert_eq!(color_index, 3);
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
        };
        let encoded = encode(&msg).unwrap();
        let decoded: ServerMessage = decode(&encoded).unwrap();
        match decoded {
            ServerMessage::JoinAccepted {
                player_id: pid,
                session_token,
            } => {
                assert_eq!(pid, player_id);
                assert_eq!(session_token, vec![1, 2, 3, 4]);
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
                alive: true,
                kills: 3,
                deaths: 1,
                spawn_protection: false,
                is_bot: false,
                color_index: 2,
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
            notable_players: vec![],
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
        };

        let encoded = encode(&delta).unwrap();
        let decoded: DeltaUpdate = decode(&encoded).unwrap();

        assert_eq!(decoded.tick, 500);
        assert_eq!(decoded.removed_projectiles.len(), 3);
    }

    #[test]
    fn test_invalid_decode() {
        let garbage = vec![0xFF, 0xFE, 0xFD];
        let result: Result<ClientMessage, _> = decode(&garbage);
        assert!(result.is_err());
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
            reason: "Test".to_string(),
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
                alive: true,
                kills: 3,
                deaths: 0,
                spawn_protection: false,
                is_bot: false,
                color_index: 2,
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
            notable_players: vec![],
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
