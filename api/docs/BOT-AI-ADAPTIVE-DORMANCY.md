# Bot AI Adaptive Dormancy System

The million-scale bot AI system uses **adaptive dormancy** to automatically balance AI quality against server performance. This document explains how it works and why specific thresholds were chosen.

## Overview

At scale (100K+ bots), not all bots need full AI updates every tick. Bots far from human players can update less frequently without players noticing. The adaptive dormancy system:

1. **Monitors tick performance** in real-time
2. **Adjusts LOD (Level of Detail) thresholds** dynamically
3. **Self-regulates** to maintain target frame rate

## The Tick Budget

The game server runs at **30 Hz** (30 ticks per second):

```
1 second ÷ 30 ticks = 33.33ms per tick
```

Each tick must complete all processing within this budget:

```
|<=============== 33.33ms tick period ===============>|
|                                                      |
|  [Tick Processing]                  [I/O & Margin]   |
|  - Process player inputs             - Send states   |
|  - Bot AI updates (SoA)              - Recv inputs   |
|  - Physics simulation                - OS scheduling |
|  - Collision detection               - Memory ops    |
|  - State synchronization                             |
|                                                      |
|<----------- tick_time ------------>|<--- ~3ms --->|
```

## Why 30ms Target (Not 33.33ms)?

If tick processing consumes the **entire** 33.33ms budget, zero time remains for network I/O. The server falls behind real-time and clients experience lag.

The **30ms target** reserves ~10% headroom:

| Requirement | Description |
|-------------|-------------|
| Network I/O | WebTransport send/receive operations between ticks |
| OS Scheduling | Thread wake-up jitter (~0.5-2ms typical) |
| Memory Pressure | Allocator operations, cache management |
| Frame Pacing | Consistent 30 Hz delivery to clients |

## Health Status Thresholds

The system maps tick time to health status:

| Tick Time | Health Status | Budget Usage | System Response |
|-----------|---------------|--------------|-----------------|
| < 20ms | Excellent | < 60% | Expand LOD radii (more full-mode bots) |
| 20-30ms | Good | 60-90% | Slight expansion, system healthy |
| 30-33ms | Warning | 90-100% | Hold steady, approaching limit |
| 33-50ms | Critical | 100-150% | Shrink LOD radii (more dormant bots) |
| > 50ms | Catastrophic | > 150% | Emergency shrink at 2x rate |

## Why 50ms for Critical Threshold?

50ms represents **150% of the tick budget**. At this point:

- Server is definitively behind real-time
- Each tick creates 16.67ms of additional lag debt
- Clients experience visible stutter and teleporting
- Without intervention, lag compounds exponentially

The system responds with 2x shrink rate to aggressively push bots into dormancy until performance recovers.

## LOD (Level of Detail) Modes

Bots operate in one of three modes based on distance from nearest human player:

| Mode | Distance | Update Frequency | AI Complexity |
|------|----------|------------------|---------------|
| Full | < 500 units | Every tick | Full decision-making, firing, targeting |
| Reduced | 500-2000 units | Every 4 ticks | Simplified decisions, basic movement |
| Dormant | > 2000 units | Every 8 ticks | Minimal updates, orbit only |

These distance thresholds are **scaled** by the adaptive system:

```
Actual Threshold = Base Threshold × LOD Scale

Example at LOD Scale 0.5 (under pressure):
  Full mode:    500 × 0.5 = 250 units
  Reduced mode: 2000 × 0.5 = 1000 units
  Dormant mode: 5000 × 0.5 = 2500 units
```

Lower scale = smaller radii = more bots in dormant mode = less CPU usage.

## The Feedback Loop

```
                    ┌──────────────────────────┐
                    │                          │
                    ▼                          │
              ┌───────────┐                    │
              │ Tick runs │                    │
              └─────┬─────┘                    │
                    │                          │
                    ▼                          │
            ┌───────────────┐                  │
            │ Measure time  │                  │
            └───────┬───────┘                  │
                    │                          │
         ┌──────────┴──────────┐               │
         ▼                     ▼               │
    < 30ms                 > 30ms              │
    ┌─────────┐          ┌─────────┐           │
    │ Expand  │          │ Shrink  │           │
    │ LOD     │          │ LOD     │           │
    └────┬────┘          └────┬────┘           │
         │                    │                │
         ▼                    ▼                │
    More bots             Fewer bots           │
    in full mode          in full mode         │
         │                    │                │
         │    ┌───────────────┘                │
         │    │                                │
         ▼    ▼                                │
    ┌─────────────┐                            │
    │ Next tick   │────────────────────────────┘
    │ performance │
    │ changes     │
    └─────────────┘
```

The system is **self-regulating**: when overloaded, it reduces work. When healthy, it increases quality.

## Asymmetric Adaptation

The system shrinks faster than it expands:

- **Shrink rate**: 2x adaptation rate (react quickly to problems)
- **Expand rate**: 1x adaptation rate (grow cautiously)

This prevents oscillation and ensures stability under load.

## Configuration

All thresholds are configurable via environment variables:

```bash
# Feature toggles
AI_SOA_DORMANCY_ENABLED=true          # Enable/disable dormancy system
AI_SOA_ADAPTIVE_DORMANCY=true         # Enable dynamic threshold adjustment

# Performance thresholds
AI_SOA_TARGET_TICK_MS=30.0            # Target tick duration (ms)
AI_SOA_CRITICAL_TICK_MS=50.0          # Critical threshold triggering emergency mode

# Adaptation behavior
AI_SOA_ADAPTATION_RATE=0.1            # How fast thresholds adjust (0.0-1.0)
AI_SOA_MIN_LOD_SCALE=0.25             # Minimum scale (radii shrink to 25%)
AI_SOA_MAX_LOD_SCALE=2.0              # Maximum scale (radii expand to 200%)

# Base LOD distance thresholds (scaled by adaptive system)
AI_SOA_LOD_FULL_RADIUS=500.0          # Distance for full AI updates
AI_SOA_LOD_REDUCED_RADIUS=2000.0      # Distance for reduced updates
AI_SOA_LOD_DORMANT_RADIUS=5000.0      # Distance for dormant mode

# Update intervals
AI_SOA_REDUCED_UPDATE_INTERVAL=4      # Ticks between reduced mode updates
AI_SOA_DORMANT_UPDATE_INTERVAL=8      # Ticks between dormant mode updates
```

### Example Configurations

**High-performance dedicated server:**
```bash
AI_SOA_TARGET_TICK_MS=20.0      # Tighter margin
AI_SOA_CRITICAL_TICK_MS=35.0    # React faster to issues
```

**Resource-constrained environment:**
```bash
AI_SOA_TARGET_TICK_MS=40.0      # More lenient target
AI_SOA_CRITICAL_TICK_MS=60.0    # Allow more headroom
```

**60 Hz server (if tick rate changed):**
```bash
AI_SOA_TARGET_TICK_MS=14.0      # 16.67ms budget - margin
AI_SOA_CRITICAL_TICK_MS=25.0    # ~150% of budget
```

## Prometheus Metrics

The system exports metrics for monitoring:

| Metric | Description |
|--------|-------------|
| `orbit_royale_bot_ai_total` | Total bots registered in SoA AI system |
| `orbit_royale_bot_ai_active` | Bots actively updating this tick |
| `orbit_royale_bot_ai_full_mode` | Bots in full update mode (near humans) |
| `orbit_royale_bot_ai_reduced_mode` | Bots in reduced update mode |
| `orbit_royale_bot_ai_dormant_mode` | Bots in dormant mode (far from humans) |
| `orbit_royale_bot_ai_lod_scale` | Current LOD scale factor (1.0 = normal) |
| `orbit_royale_bot_ai_health_status` | Health status (0=Excellent, 4=Catastrophic) |
| `orbit_royale_bot_ai_health_state{state="..."}` | Human-readable health label |

### Grafana Dashboard Queries

**Bot distribution by mode:**
```promql
orbit_royale_bot_ai_full_mode
orbit_royale_bot_ai_reduced_mode
orbit_royale_bot_ai_dormant_mode
```

**LOD scale over time:**
```promql
orbit_royale_bot_ai_lod_scale
```

**Health status alerts:**
```promql
orbit_royale_bot_ai_health_status >= 3  # Critical or worse
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        GameSession                          │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                     GameLoop                         │   │
│  │  ┌─────────────────────────────────────────────┐    │   │
│  │  │              AiManagerSoA                    │    │   │
│  │  │  ┌─────────────────────────────────────┐    │    │   │
│  │  │  │        AdaptiveDormancy             │    │    │   │
│  │  │  │  - lod_scale                        │    │    │   │
│  │  │  │  - health_status                    │    │    │   │
│  │  │  │  - tick_time_ema                    │    │    │   │
│  │  │  └─────────────────────────────────────┘    │    │   │
│  │  │                                              │    │   │
│  │  │  ┌─────────────┐  ┌─────────────────────┐   │    │   │
│  │  │  │ SoA Arrays  │  │  BehaviorBatches    │   │    │   │
│  │  │  │ (1M+ bots)  │  │  (branch-free)      │   │    │   │
│  │  │  └─────────────┘  └─────────────────────┘   │    │   │
│  │  └─────────────────────────────────────────────┘    │   │
│  └─────────────────────────────────────────────────────┘   │
│                              │                              │
│                              ▼                              │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                      Metrics                         │   │
│  │  - tick_time_us                                      │   │
│  │  - performance_status ──────► AdaptiveDormancy       │   │
│  │  - bot_ai_* metrics                                  │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## Performance Characteristics

| Bot Count | Tick Time (typical) | Active Bots | Notes |
|-----------|---------------------|-------------|-------|
| 1,000 | < 5ms | ~1,000 | All full mode |
| 10,000 | ~10ms | ~3,000 | Mixed modes |
| 100,000 | ~20ms | ~10,000 | Heavy dormancy |
| 1,000,000 | ~25ms | ~50,000 | Aggressive dormancy |

*With 10 human players scattered across the arena.*

## Related Documentation

- [Technical Architecture](./02-TECHNICAL-ARCHITECTURE.md) - Overall system design
- [Development Roadmap](./06-DEVELOPMENT-ROADMAP.md) - Implementation timeline
