# Live Simulation Management System

## Overview

Add ENV-configurable arena scaling values and enhance the JSON metrics endpoint for live simulation monitoring and tuning, with an AI-powered manager that can autonomously optimize parameters.

## Current State

- **JSON endpoint exists**: `/json` at port 9090, but limited (missing arena scaling, extended metrics)
- **Config pattern established**: `from_env()` pattern used by `GravityWaveConfig` and `DebrisSpawnConfig`
- **Many hardcoded values** in arena scaling need to be configurable

---

## Phase 1: Create ArenaScalingConfig

**File: `src/config.rs`** - Add new config struct following existing pattern:

```rust
pub struct ArenaScalingConfig {
    // Growth/Shrink behavior
    pub grow_lerp: f32,           // ENV: ARENA_GROW_LERP (default: 0.02)
    pub shrink_lerp: f32,         // ENV: ARENA_SHRINK_LERP (default: 0.005)
    pub shrink_delay_ticks: u32,  // ENV: ARENA_SHRINK_DELAY_TICKS (default: 150)

    // Size limits
    pub min_escape_radius: f32,   // ENV: ARENA_MIN_RADIUS (default: 800)
    pub max_escape_multiplier: f32, // ENV: ARENA_MAX_MULTIPLIER (default: 10.0)
    pub growth_per_player: f32,   // ENV: ARENA_GROWTH_PER_PLAYER (default: 10.0)
    pub player_threshold: usize,  // ENV: ARENA_PLAYER_THRESHOLD (default: 10)

    // Well positioning
    pub well_min_ratio: f32,      // ENV: ARENA_WELL_MIN_RATIO (default: 0.20)
    pub well_max_ratio: f32,      // ENV: ARENA_WELL_MAX_RATIO (default: 0.85)
    pub wells_per_50_players: usize, // ENV: ARENA_WELLS_PER_50 (default: 1)
    pub max_wells: usize,         // ENV: ARENA_MAX_WELLS (default: 20)

    // Well ring distribution (percentages of escape_radius)
    pub ring_inner_min: f32,      // ENV: ARENA_RING_INNER_MIN (default: 0.25)
    pub ring_inner_max: f32,      // ENV: ARENA_RING_INNER_MAX (default: 0.40)
    pub ring_middle_min: f32,     // ENV: ARENA_RING_MIDDLE_MIN (default: 0.45)
    pub ring_middle_max: f32,     // ENV: ARENA_RING_MIDDLE_MAX (default: 0.65)
    pub ring_outer_min: f32,      // ENV: ARENA_RING_OUTER_MIN (default: 0.70)
    pub ring_outer_max: f32,      // ENV: ARENA_RING_OUTER_MAX (default: 0.90)

    // Supermassive black hole
    pub supermassive_mass_mult: f32,  // ENV: ARENA_SUPERMASSIVE_MASS (default: 3.0)
    pub supermassive_core_mult: f32,  // ENV: ARENA_SUPERMASSIVE_CORE (default: 2.5)
}
```

**Implement:**
- `Default` trait with values from constants
- `from_env()` method with validation and logging
- Tests for config loading

---

## Phase 2: Integrate Config into Arena Scaling

**File: `src/game/state.rs`** - Modify functions to use config:

1. **`scale_for_simulation()`** - Accept `&ArenaScalingConfig` parameter
   - Replace hardcoded `SHRINK_DELAY_TICKS`, lerp values, etc.

2. **`add_orbital_wells()`** - Accept config for ring distribution
   - Replace hardcoded ring percentages

3. **`update_for_player_count()`** - Use config values

**File: `src/game/game_loop.rs`** - Add config to GameLoopConfig

**File: `src/net/game_session.rs`** - Load config and pass to game loop

---

## Phase 3: Enhanced JSON Metrics Endpoint

**File: `src/metrics.rs`** - Enhance `to_json()` method:

Add comprehensive metrics including:

```json
{
  "players": { "total", "human", "bot", "alive" },
  "entities": { "projectiles", "debris", "gravity_wells" },
  "performance": { "tick_time_us", "p95", "p99", "max", "status", "budget_percent" },
  "network": { "connections", "messages_sent/received", "bytes_sent/received" },
  "game": { "match_time", "uptime" },

  "arena": {
    "escape_radius": 4000.0,
    "outer_radius": 3800.0,
    "scale": 5.0,
    "collapse_phase": 0,
    "gravity_wells": [
      { "position": [x, y], "mass": 10000, "core_radius": 50 }
    ],
    "target_escape_radius": 4500.0,
    "shrink_delay_remaining": 0
  },

  "aoi": { "original_players", "filtered_players", "reduction_percent" },
  "anticheat": { "validated", "rejected", "sanitized", "sequence_violations" },
  "dos": { "connections_rejected", "rate_limited", "active_bans" },

  "config": {
    "arena": { "grow_lerp", "shrink_lerp", "shrink_delay_ticks" },
    "simulation": { "enabled", "min_bots", "max_bots", "cycle_minutes" },
    "gravity_waves": { "enabled", "speed", "impulse" },
    "debris": { "enabled", "max_count", "spawn_rates" }
  }
}
```

---

## Phase 4: Lazy Arena State Access (Zero Tick Overhead)

**Key Insight**: Don't push arena state every tick. Instead, read on-demand when `/json` is requested.

**Approach:**
1. Pass `Arc<RwLock<GameState>>` reference to metrics server
2. On `/json` request, acquire read lock and extract arena details
3. Zero overhead during normal game ticks

**File: `src/metrics.rs`** - Add extended JSON method:

```rust
pub fn to_json_extended(&self, game_state: Option<&GameState>, configs: &AllConfigs) -> String {
    // Basic metrics (already available via atomics)
    let mut json = self.to_json_base();

    // Arena state - only computed on request (lazy)
    if let Some(state) = game_state {
        json.arena = ArenaSnapshot {
            escape_radius: state.escape_radius,
            outer_radius: state.outer_radius,
            scale: state.arena_scale,
            gravity_wells: state.gravity_wells.iter().map(|w| WellSnapshot {
                position: [w.position.x, w.position.y],
                mass: w.mass,
                core_radius: w.core_radius,
            }).collect(),
        };
    }

    // Current config values (for visibility)
    json.config = configs.to_snapshot();

    serde_json::to_string(&json)
}
```

**Why this is better:**
- Zero overhead per tick (no arena state copying)
- Full details available on demand
- Read lock only held briefly during HTTP response

---

## Phase 5: AI Simulation Manager

See [AI Simulation Manager Specification](./ai-simulation-manager.md) for full details.

---

## ENV Variables Summary

```bash
# Arena Scaling
ARENA_GROW_LERP=0.02              # Growth speed (0.01-0.1)
ARENA_SHRINK_LERP=0.005           # Shrink speed (0.001-0.05)
ARENA_SHRINK_DELAY_TICKS=150      # Delay before shrink (0-300)
ARENA_MIN_RADIUS=800              # Minimum arena size (500-2000)
ARENA_MAX_MULTIPLIER=10.0         # Max size multiplier (5-20)
ARENA_GROWTH_PER_PLAYER=10.0      # Units per player (5-50)
ARENA_PLAYER_THRESHOLD=10         # Players before growth (1-50)
ARENA_WELL_MIN_RATIO=0.20         # Min well distance ratio (0.1-0.4)
ARENA_WELL_MAX_RATIO=0.85         # Max well distance ratio (0.6-0.95)
ARENA_MAX_WELLS=20                # Maximum gravity wells (5-50)

# Ring Distribution
ARENA_RING_INNER_MIN=0.25         # Inner ring start
ARENA_RING_INNER_MAX=0.40         # Inner ring end
ARENA_RING_MIDDLE_MIN=0.45        # Middle ring start
ARENA_RING_MIDDLE_MAX=0.65        # Middle ring end
ARENA_RING_OUTER_MIN=0.70         # Outer ring start
ARENA_RING_OUTER_MAX=0.90         # Outer ring end

# Supermassive
ARENA_SUPERMASSIVE_MASS=3.0       # Mass multiplier
ARENA_SUPERMASSIVE_CORE=2.5       # Core radius multiplier

# AI Manager
AI_ENABLED=false                   # Enable/disable AI manager (default: false)
ORBIT_API_KEY=sk-ant-...          # Claude API key (required if enabled)
AI_EVAL_INTERVAL_MINUTES=2        # Minutes between evaluations (default: 2)
AI_MAX_HISTORY=100                # Max decisions to keep
AI_CONFIDENCE_THRESHOLD=0.7       # Min confidence to act
AI_MODEL=claude-sonnet-4-5        # Model to use
```

---

## Files to Modify

| File | Changes |
|------|---------|
| `src/config.rs` | Add `ArenaScalingConfig`, `AIManagerConfig` |
| `src/game/state.rs` | Use config in scaling functions |
| `src/game/game_loop.rs` | Add configs to `GameLoopConfig` |
| `src/net/game_session.rs` | Load configs, pass state to metrics, spawn AI manager |
| `src/metrics.rs` | Add `to_json_extended()` with lazy arena + AI state |
| `src/lib.rs` | Add `ai_manager` module |
| `Cargo.toml` | Add `reqwest`, `chrono` dependencies |

## Files to Add

| File | Purpose |
|------|---------|
| `src/ai_manager/mod.rs` | Main AI manager module |
| `src/ai_manager/client.rs` | Claude API HTTP client |
| `src/ai_manager/history.rs` | Decision history storage |
| `src/ai_manager/analysis.rs` | Response parsing |

---

## Testing Strategy

1. **Unit tests** for `ArenaScalingConfig::from_env()`
2. **Integration tests** for scaling with different configs
3. **JSON format tests** for new metrics structure
4. **AI Manager tests** with mock Claude API responses
5. **Decision history** persistence tests
6. **Manual testing** with Docker to verify live tuning
