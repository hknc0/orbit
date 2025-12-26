# Orbit Royale Technical Reference

Complete technical reference for the Orbit Royale multiplayer game server.

## Table of Contents

1. [Connection](#connection)
2. [Message Protocol](#message-protocol)
3. [Client Messages](#client-messages)
4. [Server Messages](#server-messages)
5. [Game Events](#game-events)
6. [Data Structures](#data-structures)
7. [Entity Types](#entity-types)
8. [Physics Constants](#physics-constants)
9. [HTTP Endpoints](#http-endpoints)
10. [Configuration](#configuration)
11. [Security](#security)
12. [Performance](#performance)

---

## Connection

### WebTransport Endpoint

```
wt://localhost:4433/
```

| Property | Value |
|----------|-------|
| Protocol | WebTransport (QUIC-based) |
| TLS | Required (self-signed in dev) |
| Max Message | 64 KB |
| Max Datagram | 1,200 bytes |

### Connection Flow

```
1. Client connects via WebTransport
2. Client sends: JoinRequest { player_name, color_index }
3. Server validates and sanitizes input
4. Server responds: JoinAccepted { player_id, session_token }
   OR JoinRejected { reason }
5. Client sends continuous Input messages
6. Server broadcasts Snapshot every ~50ms
7. Server sends Event for significant changes
8. RTT measured via Ping/Pong exchanges
```

---

## Message Protocol

### Serialization

All messages use **bincode** with legacy configuration for fixed-size integer compatibility.

### Framing

- **Length-prefixed:** 4-byte little-endian length prefix
- **Max size:** 65,536 bytes (64 KB)

---

## Client Messages

### JoinRequest

```rust
JoinRequest {
    player_name: String,  // Max 16 chars, sanitized
    color_index: u8,      // Player color selection (0-based)
}
```

### Input

```rust
Input {
    sequence: u64,       // Client sequence number (deduplication)
    tick: u64,           // Server tick this input targets
    client_time: u64,    // Client ms timestamp (RTT echo)

    thrust: Vec2,        // Movement direction (-1 to 1 each axis)
    aim: Vec2,           // Aiming direction (normalized)
    boost: bool,         // Thrust amplifier (costs mass)
    fire: bool,          // Charging eject/fire
    fire_released: bool, // Fire button released (triggers eject)
}
```

### Leave

```rust
Leave  // Graceful disconnect
```

### Ping

```rust
Ping {
    timestamp: u64,  // Client millisecond timestamp
}
```

### SnapshotAck

```rust
SnapshotAck {
    tick: u64,  // Tick number acknowledged
}
```

---

## Server Messages

### JoinAccepted

```rust
JoinAccepted {
    player_id: PlayerId,       // UUID unique to player
    session_token: Vec<u8>,    // 32-byte session token
}
```

### JoinRejected

```rust
JoinRejected {
    reason: String,  // Human-readable rejection reason
}
```

### Snapshot

Full game state broadcast (sent every ~50ms at 20Hz).

```rust
Snapshot {
    tick: u64,
    match_phase: MatchPhase,
    match_time: f32,
    countdown: f32,

    players: Vec<PlayerSnapshot>,
    projectiles: Vec<ProjectileSnapshot>,
    debris: Vec<DebrisSnapshot>,

    arena_collapse_phase: u8,
    arena_safe_radius: f32,
    arena_scale: f32,
    gravity_wells: Vec<GravityWellSnapshot>,

    total_players: u32,
    total_alive: u32,
    density_grid: Vec<u8>,           // 16x16 minimap heatmap
    notable_players: Vec<NotablePlayer>,
    echo_client_time: u64,
    ai_status: Option<AIStatusSnapshot>,
}
```

### Delta

```rust
Delta {
    tick: u64,
    base_tick: u64,
    player_updates: Vec<PlayerDelta>,
    projectile_updates: Vec<ProjectileDelta>,
    removed_projectiles: Vec<u64>,
}
```

### Event

```rust
Event {
    event: GameEvent,
}
```

### Pong

```rust
Pong {
    client_timestamp: u64,
    server_timestamp: u64,
}
```

### Kicked

```rust
Kicked {
    reason: String,
}
```

### PhaseChange

```rust
PhaseChange {
    phase: MatchPhase,
    countdown: f32,
}
```

---

## Game Events

### PlayerKilled

```rust
PlayerKilled {
    killer_id: PlayerId,
    victim_id: PlayerId,
    killer_name: String,
    victim_name: String,
}
```

### PlayerJoined / PlayerLeft

```rust
PlayerJoined { player_id: PlayerId, name: String }
PlayerLeft { player_id: PlayerId, name: String }
```

### MatchStarted / MatchEnded

```rust
MatchStarted
MatchEnded { winner_id: Option<PlayerId>, winner_name: Option<String> }
```

### ZoneCollapse

```rust
ZoneCollapse {
    phase: u8,
    new_safe_radius: f32,
}
```

### PlayerDeflection

```rust
PlayerDeflection {
    player_a: PlayerId,
    player_b: PlayerId,
    position: Vec2,
    intensity: f32,  // 0-1 for visual scaling
}
```

### GravityWellCharging / GravityWaveExplosion

```rust
GravityWellCharging { well_index: u8, position: Vec2 }
GravityWaveExplosion { well_index: u8, position: Vec2, strength: f32 }
```

---

## Data Structures

### PlayerSnapshot

```rust
PlayerSnapshot {
    id: PlayerId,
    name: String,
    position: Vec2,
    velocity: Vec2,
    rotation: f32,           // Radians
    mass: f32,
    alive: bool,
    kills: u32,
    deaths: u32,
    spawn_protection: bool,
    is_bot: bool,
    color_index: u8,
}
```

### ProjectileSnapshot

```rust
ProjectileSnapshot {
    id: u64,
    owner_id: PlayerId,
    position: Vec2,
    velocity: Vec2,
    mass: f32,
}
```

### DebrisSnapshot

```rust
DebrisSnapshot {
    id: u64,
    position: Vec2,
    size: u8,  // 0=Small(5), 1=Medium(15), 2=Large(30)
}
```

### GravityWellSnapshot

```rust
GravityWellSnapshot {
    position: Vec2,
    mass: f32,
    core_radius: f32,
}
```

### MatchPhase

```rust
enum MatchPhase {
    Waiting,    // 0 - Waiting for players
    Countdown,  // 1 - Pre-match countdown
    Playing,    // 2 - Active gameplay
    Ended,      // 3 - Match finished
}
```

---

## Entity Types

### Player

| Property | Type | Description |
|----------|------|-------------|
| id | UUID | Unique identifier |
| name | String | Display name (16 chars max) |
| position | Vec2 | World coordinates |
| velocity | Vec2 | Movement vector |
| rotation | f32 | Facing angle (radians) |
| mass | f32 | Health/size (10-1000+) |
| radius | f32 | `sqrt(mass) * 2.0` |
| alive | bool | In-game status |
| spawn_protection | f32 | Invulnerability timer (3s) |

### Projectile

| Property | Type | Description |
|----------|------|-------------|
| id | u64 | Unique entity ID |
| owner_id | PlayerId | Firing player |
| position | Vec2 | Current position |
| velocity | Vec2 | Direction and speed |
| mass | f32 | Damage value (10-50) |
| lifetime | f32 | Time remaining (8s max) |

### Debris

| Size | Mass | Radius |
|------|------|--------|
| Small | 5 | 3.16 |
| Medium | 15 | 5.48 |
| Large | 30 | 7.75 |

### Gravity Well

| Property | Description |
|----------|-------------|
| position | Well center |
| mass | Gravitational strength |
| core_radius | Instant death radius |

**Well Types:**
- **Central:** mass = 30,000, core = 125 units
- **Orbital:** mass = 6,000-14,000, core = 30-70 units

---

## Physics Constants

### Simulation

| Constant | Value | Description |
|----------|-------|-------------|
| TICK_RATE | 30 Hz | Server simulation frequency |
| DT | 0.0333s | Delta time per tick |
| MAX_VELOCITY | 500 | Speed cap |
| DRAG | 0.002 | Velocity decay per tick |

### Movement

| Constant | Value | Description |
|----------|-------|-------------|
| BASE_THRUST | 200 | Thrust force units |
| BOOST_MULTIPLIER | 2.0 | Boost thrust multiplier |
| BOOST_MASS_COST | 0.5/tick | Mass consumed while boosting |

### Mass

| Constant | Value | Description |
|----------|-------|-------------|
| STARTING_MASS | 100 | New player mass |
| MINIMUM_MASS | 10 | Death threshold |
| ABSORPTION_CAP | 200 | Max mass per collision |
| ABSORPTION_RATE | 70% | Victim mass absorbed |
| RADIUS_SCALE | 2.0 | `radius = sqrt(mass) * RADIUS_SCALE` |

### Collision

| Constant | Value | Description |
|----------|-------|-------------|
| OVERWHELM_THRESHOLD | 2.0 | Momentum ratio for instant kill |
| DECISIVE_THRESHOLD | 1.5 | Momentum ratio for decisive win |
| RESTITUTION | 0.8 | Bounce coefficient |

### Ejection

| Constant | Value | Description |
|----------|-------|-------------|
| MIN_CHARGE_TIME | 0.2s | Minimum charge duration |
| MAX_CHARGE_TIME | 1.0s | Maximum charge duration |
| MIN_MASS | 10 | Minimum eject mass |
| MAX_MASS_RATIO | 50% | Max eject = 50% current mass |
| MIN_VELOCITY | 100 u/s | Minimum projectile speed |
| MAX_VELOCITY | 300 u/s | Maximum projectile speed |
| LIFETIME | 8s | Projectile lifetime |

### Spawn

| Constant | Value | Description |
|----------|-------|-------------|
| PROTECTION_DURATION | 3s | Invulnerability period |
| SPAWN_ZONE_MIN | 250 | Minimum spawn distance |
| SPAWN_ZONE_MAX | 350 | Maximum spawn distance |
| SAFE_DISTANCE | 80 | Minimum from other players |
| RESPAWN_DELAY | 2s | Time until respawn allowed |

### Arena

| Constant | Value | Description |
|----------|-------|-------------|
| CORE_RADIUS | 50 | Instant death zone |
| ESCAPE_RADIUS | 800 | Safe zone limit |
| COLLAPSE_PHASES | 8 | Total collapse stages |
| COLLAPSE_INTERVAL | 30s | Time between phases |
| ESCAPE_MASS_DRAIN | 10/s | Mass loss outside arena |

---

## HTTP Endpoints

### Metrics Server (Port 9090)

#### Prometheus Metrics

```
GET /metrics
```

```
game_total_players{} 45
game_human_players{} 10
game_bot_players{} 35
game_alive_players{} 42
game_projectiles{} 128
game_debris{} 312
game_gravity_wells{} 5
game_tick_rate_hz{} 30
game_tick_time_us{} 15200
game_tick_time_p95_us{} 18500
game_tick_time_p99_us{} 21000
game_performance_status{} 1
```

#### JSON Metrics

```
GET /json
```

```json
{
  "players": { "total": 45, "human": 10, "bot": 35, "alive": 42 },
  "entities": { "projectiles": 128, "debris": 312, "gravity_wells": 5 },
  "performance": {
    "tick_time_us": 15200,
    "p95": 18500,
    "p99": 21000,
    "max": 25000,
    "status": "healthy",
    "budget_percent": 45
  },
  "arena": {
    "escape_radius": 4000.0,
    "scale": 5.0,
    "collapse_phase": 0,
    "gravity_wells": [
      { "position": [100, 200], "mass": 10000, "core_radius": 50 }
    ]
  },
  "ai_manager": {
    "enabled": true,
    "status": "active",
    "last_evaluation": "2024-01-15T10:30:00Z",
    "decisions_made": 42,
    "success_rate": 0.786
  },
  "config": {
    "arena": { "grow_lerp": 0.02, "shrink_lerp": 0.005 },
    "simulation": { "enabled": true, "min_bots": 50, "max_bots": 500 }
  }
}
```

#### Health Check

```
GET /health
```

Returns `200 OK` if server is running.

---

## Configuration

### Core Server

| Variable | Default | Description |
|----------|---------|-------------|
| `BIND_ADDRESS` | `0.0.0.0` | Server bind address |
| `PORT` | `4433` | WebTransport port |
| `MAX_ROOMS` | `100` | Maximum game rooms |
| `MAX_PLAYERS_PER_ROOM` | `10` | Players per room |
| `TLS_CERT_PATH` | - | TLS certificate path |
| `TLS_KEY_PATH` | - | TLS private key path |

### Arena Scaling

| Variable | Default | Range | Description |
|----------|---------|-------|-------------|
| `ARENA_GROW_LERP` | `0.02` | 0.01-0.1 | Growth speed |
| `ARENA_SHRINK_LERP` | `0.005` | 0.001-0.05 | Shrink speed |
| `ARENA_SHRINK_DELAY_TICKS` | `150` | 0-300 | Delay before shrink |
| `ARENA_MIN_RADIUS` | `800` | 500-2000 | Minimum arena size |
| `ARENA_MAX_MULTIPLIER` | `10.0` | 5-20 | Max scale multiplier |
| `ARENA_GROWTH_PER_PLAYER` | `25.0` | 5-50 | Radius per player |
| `ARENA_PLAYER_THRESHOLD` | `10` | 1-50 | Players before growth |
| `ARENA_MAX_WELLS` | `20` | 5-50 | Maximum gravity wells |

### Well Ring Distribution

| Variable | Default | Description |
|----------|---------|-------------|
| `ARENA_RING_INNER_MIN` | `0.25` | Inner ring start |
| `ARENA_RING_INNER_MAX` | `0.40` | Inner ring end |
| `ARENA_RING_MIDDLE_MIN` | `0.45` | Middle ring start |
| `ARENA_RING_MIDDLE_MAX` | `0.65` | Middle ring end |
| `ARENA_RING_OUTER_MIN` | `0.70` | Outer ring start |
| `ARENA_RING_OUTER_MAX` | `0.90` | Outer ring end |

### Gravity Waves

| Variable | Default | Description |
|----------|---------|-------------|
| `GRAVITY_WAVE_ENABLED` | `true` | Enable wave explosions |
| `GRAVITY_WAVE_SPEED` | `300.0` | Wave expansion speed |
| `GRAVITY_WAVE_FRONT_THICKNESS` | `80.0` | Wave front width |
| `GRAVITY_WAVE_BASE_IMPULSE` | `180.0` | Force multiplier |
| `GRAVITY_WAVE_MAX_RADIUS` | `2000.0` | Max wave radius |
| `GRAVITY_WAVE_CHARGE_DURATION` | `2.0` | Pre-explosion warning |
| `GRAVITY_WAVE_MIN_DELAY` | `30.0` | Min time between waves |
| `GRAVITY_WAVE_MAX_DELAY` | `90.0` | Max time between waves |

### Debris

| Variable | Default | Description |
|----------|---------|-------------|
| `DEBRIS_SPAWN_ENABLED` | `true` | Enable debris spawning |
| `DEBRIS_MAX_COUNT` | `500` | Maximum debris entities |
| `DEBRIS_LIFETIME` | `90.0` | Debris lifetime seconds |

### Simulation Mode

| Variable | Default | Description |
|----------|---------|-------------|
| `SIMULATION_MODE` | `false` | Enable load testing |
| `SIMULATION_MIN_BOTS` | `5` | Minimum bot count |
| `SIMULATION_MAX_BOTS` | `100` | Maximum bot count |
| `SIMULATION_CYCLE_MINUTES` | `10` | Population cycle duration |

### AI Manager

| Variable | Default | Description |
|----------|---------|-------------|
| `AI_ENABLED` | `false` | Enable AI tuning |
| `ORBIT_API_KEY` | - | API key for AI service |
| `AI_EVAL_INTERVAL_MINUTES` | `2` | Evaluation interval |
| `AI_CONFIDENCE_THRESHOLD` | `0.7` | Confidence threshold |
| `AI_MAX_HISTORY` | `100` | Max decisions to keep |
| `AI_MODEL` | `claude-sonnet-4-5` | Model to use |

---

## Security

### Input Validation

- **Player names:** Sanitized (16 chars max, no control chars/HTML)
- **Message size:** 64 KB limit enforced
- **Sequence deduplication:** Prevents replay attacks
- **Rate limiting:** Per-connection (feature-gated)

### Anti-Cheat (Feature-Gated)

- Input range validation
- Sequence tracking (detects regression/gaps)
- Timing validation (alignment with server ticks)
- Violation logging and thresholds
- Automatic input rejection for malicious patterns

### DoS Protection (Feature-Gated)

- Connection rate limiting per IP
- Message rate limiting per connection
- Violation thresholds with auto-disconnect
- Temporary IP bans for repeat offenders

### Session Management

- 32-byte cryptographically secure session tokens
- Session tracking with timeouts
- Connection state machine:
  `Connecting -> Connected -> Disconnecting -> Disconnected`

---

## Performance

### Area of Interest (AOI)

| Setting | Value | Description |
|---------|-------|-------------|
| Full Detail Radius | 3000 units | Nearby entities full detail |
| Extended Radius | 6000 units | Medium-distance entities |
| Max Entities | 150 | Hard cap per client |
| Always Include | Top 10 players | Leaderboard visibility |
| Notable Threshold | 80 mass | Players always visible |

### Optimizations

- **Buffer pooling:** Pre-allocated 4 KB buffers
- **Async I/O:** Tokio-based non-blocking handlers
- **Lock-free messaging:** Channel-based client communication
- **Spatial hashing:** O(n) collision detection
- **Delta updates:** 50-80% bandwidth reduction
- **Parallel physics:** Rayon-based parallel processing
