# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Orbit Royale is a real-time multiplayer browser game with gravity well physics. Architecture:
- **Backend**: Rust WebTransport server (QUIC/UDP) at port 4433
- **Frontend**: TypeScript/Vite client at port 5173
- **Monitoring**: Prometheus (9091) + Grafana (3000) dashboards

## Common Commands

### Development
```bash
make setup          # Generate TLS certificates (run once)
make dev            # Start both API and client servers
make api            # Start only Rust API server
make client         # Start only Vite client
make chrome         # Open Chrome with cert bypass for WebTransport
```

### Testing
```bash
# Rust (from root or api/)
cargo test                          # Run all tests
cargo test test_name                # Run single test
cargo test -- --nocapture           # With stdout visible
RUST_BACKTRACE=1 cargo test         # With backtrace

# TypeScript (from client/)
npm test                            # Run all tests
npm run test:watch                  # Watch mode
```

### Building & Quality
```bash
cargo build --release               # Optimized Rust build
npm run build                       # Build client
cargo fmt && cargo clippy           # Format and lint Rust
npm run typecheck                   # Type check TypeScript
```

### Docker
```bash
docker-compose up -d                # Start all services
docker-compose up -d api            # Start only API
docker-compose logs -f api          # Watch API logs
docker-compose down                 # Stop all
```

## Architecture

### Backend (`api/src/`)

**Game Loop** (30 Hz tick rate):
- `game/game_loop.rs` - Main tick loop orchestration
- `game/state.rs` - Entity storage (players, projectiles, debris, gravity wells)
- `game/systems/` - Per-tick systems: physics, gravity, collision, AI, arena scaling

**Networking**:
- `net/transport.rs` - WebTransport server (wtransport crate)
- `net/game_session.rs` - Per-player session, snapshot broadcasting
- `net/aoi.rs` - Area of Interest filtering (only send nearby entities)
- `net/delta.rs` - Delta compression (send only changes)
- `net/protocol.rs` - Binary protocol (bincode serialization)

**Key Patterns**:
- Structure-of-Arrays (SoA) for bot AI performance (`systems/ai_soa.rs`)
- Spatial grid for O(1) entity lookups (`game/spatial.rs`)
- Feature flags for optional systems: `anticheat`, `lobby`, `ai_manager`, `metrics_extended`

### Frontend (`client/src/`)

- `core/Game.ts` - Main game loop, connection lifecycle
- `core/World.ts` - Client-side entity state
- `net/StateSync.ts` - Snapshot interpolation, delta reconstruction
- `net/Transport.ts` - WebTransport client
- `net/Codec.ts` - Binary message encoding/decoding
- `systems/RenderSystem.ts` - Canvas rendering
- `ui/Screens.ts` - Menu/HUD UI

**Client-Server Flow**:
1. Client sends inputs via WebTransport
2. Server validates, runs physics at 30 Hz
3. Server broadcasts AOI-filtered delta snapshots
4. Client interpolates between snapshots for smooth rendering

### Configuration

All game parameters are environment variables (see `.env.example`):
- `BOT_COUNT` - Number of AI bots
- `GRAVITY_WAVE_*` - Wave mechanics
- `DEBRIS_*` - Collectible spawning
- `ARENA_*` - Dynamic arena scaling
- `AI_*` - Claude API integration for parameter tuning

## Key Concepts

**AOI (Area of Interest)**: Server only sends entities within player's viewport. Radius scales with zoom level. Critical for bandwidth optimization.

**Delta Compression**: Server sends full snapshots every 30 ticks (~1 second), deltas in between. Client reconstructs state from base + deltas.

**Adaptive Dormancy**: Bots far from human players run reduced AI updates. LOD scale adjusts based on server performance health.

**World Preview Mode**: On join/respawn, client shows world for 300ms before spawning local player (prevents "pop-in" of other players).

## Metrics & Monitoring

- Prometheus endpoint: `http://localhost:9090/metrics`
- Grafana dashboard: `http://localhost:3000` (admin/orbit123)
- Key metrics: `orbit_royale_tick_time_microseconds`, `orbit_royale_players_human`, `orbit_royale_aoi_reduction_percent`
