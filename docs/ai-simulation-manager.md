# AI Simulation Manager Specification

## Overview

The AI Simulation Manager is an autonomous system that monitors game server metrics, analyzes performance patterns, makes intelligent parameter adjustments, and learns from the outcomes of its decisions.

## Core Principles

1. **Observability First**: All decisions are logged with full context
2. **Reversible Changes**: Parameters can always be reverted
3. **Gradual Adjustments**: Small incremental changes, not drastic ones
4. **Outcome Evaluation**: Every decision is evaluated after a delay
5. **Learning Loop**: Past outcomes inform future decisions

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    AI Manager System                        │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐  │
│  │  Collector   │───▶│   Analyst    │───▶│   Executor   │  │
│  │ (metrics)    │    │ (Claude API) │    │ (apply cfg)  │  │
│  └──────────────┘    └──────────────┘    └──────────────┘  │
│         ▲                   │                    │          │
│         │                   ▼                    │          │
│         │           ┌──────────────┐             │          │
│         └───────────│   History    │◀────────────┘          │
│                     │  (decisions) │                        │
│                     └──────────────┘                        │
└─────────────────────────────────────────────────────────────┘
```

## System Components

### 1. Metrics Collector

Collects snapshots of simulation state at regular intervals:

| Metric | Source | Purpose |
|--------|--------|---------|
| `tick_time_p95` | Performance | Primary health indicator |
| `total_players` | Game state | Population tracking |
| `alive_players` | Game state | Engagement indicator |
| `arena_scale` | Game state | Space utilization |
| `gravity_wells` | Game state | Complexity metric |
| `debris_count` | Game state | Entity load |
| `messages_sent` | Network | Bandwidth indicator |

### 2. Claude API Analyst

Sends metrics + history to Claude for analysis:

**Request Format:**
```json
{
  "model": "claude-sonnet-4-5",
  "max_tokens": 1024,
  "system": "<system prompt with tunable parameters>",
  "messages": [{
    "role": "user",
    "content": "Current metrics:\n<json>\n\nRecent decisions:\n<history>"
  }]
}
```

**Expected Response:**
```json
{
  "analysis": "Tick time is elevated at 18ms with 500 players...",
  "recommendations": [
    {
      "parameter": "arena.max_wells",
      "current": 20,
      "recommended": 15,
      "reason": "Reducing gravity wells will lower physics computation"
    }
  ],
  "confidence": 0.85,
  "risk_level": "low"
}
```

### 3. Action Executor

Applies parameter changes to live simulation:

```rust
pub trait ParameterApplier {
    fn apply(&self, param: &str, value: f32, game_state: &mut GameState) -> Result<f32>;
}

// Supported parameters:
impl ParameterApplier for ArenaScalingConfig {
    fn apply(&self, param: &str, value: f32, state: &mut GameState) -> Result<f32> {
        let old = match param {
            "arena.grow_lerp" => std::mem::replace(&mut self.grow_lerp, value),
            "arena.shrink_lerp" => std::mem::replace(&mut self.shrink_lerp, value),
            "arena.max_wells" => {
                let old = self.max_wells as f32;
                self.max_wells = value as usize;
                old
            }
            // ... more parameters
            _ => return Err(anyhow!("Unknown parameter: {}", param)),
        };
        Ok(old)
    }
}
```

### 4. Decision History Store

Persisted JSON file with complete decision records:

**File: `data/ai_decisions.json`**

```json
{
  "version": 1,
  "decisions": [
    {
      "id": "dec_20240115_103000_001",
      "timestamp": "2024-01-15T10:30:00Z",
      "metrics_before": {
        "tick_time_p95": 18500,
        "total_players": 523,
        "arena_scale": 4.2
      },
      "analysis": "Performance degrading with high entity count",
      "actions": [
        {
          "parameter": "arena.max_wells",
          "old_value": 20,
          "new_value": 15,
          "reason": "Reduce physics complexity"
        }
      ],
      "confidence": 0.85,
      "outcome": {
        "evaluated_at": "2024-01-15T10:32:00Z",
        "metrics_after": {
          "tick_time_p95": 14200,
          "total_players": 518
        },
        "performance_delta_us": -4300,
        "success": true
      }
    }
  ],
  "statistics": {
    "total_decisions": 42,
    "successful": 33,
    "failed": 9,
    "success_rate": 0.786
  }
}
```

## Tunable Parameters

### Arena Scaling Parameters

| Parameter | Range | Default | Effect |
|-----------|-------|---------|--------|
| `arena.grow_lerp` | 0.01-0.1 | 0.02 | Arena growth speed |
| `arena.shrink_lerp` | 0.001-0.05 | 0.005 | Arena shrink speed |
| `arena.shrink_delay_ticks` | 0-300 | 150 | Delay before shrinking |
| `arena.min_radius` | 500-2000 | 800 | Minimum arena size |
| `arena.max_multiplier` | 5-20 | 10.0 | Maximum scale factor |
| `arena.growth_per_player` | 5-50 | 10.0 | Growth units per player |
| `arena.max_wells` | 5-50 | 20 | Maximum gravity wells |

### Simulation Parameters

| Parameter | Range | Default | Effect |
|-----------|-------|---------|--------|
| `simulation.min_bots` | 10-500 | 50 | Minimum bot count |
| `simulation.max_bots` | 50-2000 | 500 | Maximum bot count |
| `simulation.cycle_minutes` | 1-60 | 10 | Population cycle duration |

### Physics Parameters

| Parameter | Range | Default | Effect |
|-----------|-------|---------|--------|
| `physics.gravity_strength` | 0.5-5.0 | 1.0 | Gravity multiplier |
| `physics.debris_friction` | 0.9-1.0 | 0.98 | Debris slowdown |

## Decision Rules

The AI follows these guidelines when making recommendations:

### Performance Rules

1. **Tick Budget**: Target <20ms (66% of 33ms budget at 30Hz)
2. **If tick_time_p95 > 25ms**: Reduce complexity (fewer wells, less debris)
3. **If tick_time_p95 < 10ms**: Can increase complexity for richer gameplay

### Population Rules

1. **Low population (<100)**: Reduce arena size, increase density
2. **High population (>500)**: Expand arena, distribute gravity wells
3. **Very high (>800)**: Limit entity counts, prioritize performance

### Safety Rules

1. **Max change per decision**: 20% of current value
2. **Min time between changes**: 2 minutes (configurable)
3. **Revert threshold**: If performance drops >30%, revert last change
4. **Confidence threshold**: Only act if confidence > 0.7

## ENV Variables

```bash
# AI Manager
AI_ENABLED=false                   # Enable/disable AI manager (default: false)
AI_API_KEY=sk-ant-...             # Claude API key (required if enabled)
AI_EVAL_INTERVAL_MINUTES=2        # Minutes between evaluations (default: 2)
AI_MAX_HISTORY=100                # Max decisions to keep
AI_CONFIDENCE_THRESHOLD=0.7       # Min confidence to act
AI_MODEL=claude-sonnet-4-5        # Model to use
```

## API Endpoints

### JSON Metrics with AI State

**GET** `/json`

```json
{
  "players": { ... },
  "performance": { ... },
  "arena": { ... },
  "ai_manager": {
    "enabled": true,
    "status": "active",
    "last_evaluation": "2024-01-15T10:30:00Z",
    "next_evaluation": "2024-01-15T10:31:00Z",
    "decisions_made": 42,
    "success_rate": 0.786,
    "current_confidence": 0.85,
    "recent_decisions": [
      {
        "id": "dec_20240115_103000_001",
        "timestamp": "2024-01-15T10:30:00Z",
        "actions": [{"parameter": "arena.max_wells", "old": 20, "new": 15}],
        "outcome": {"success": true, "delta_us": -4300}
      }
    ],
    "pending_evaluations": 1
  },
  "config": {
    "arena": { ... },
    "simulation": { ... },
    "ai": {
      "eval_interval_minutes": 2,
      "confidence_threshold": 0.7,
      "max_history": 100
    }
  }
}
```

## Error Handling

### API Failures

- Retry with exponential backoff (1s, 2s, 4s, max 60s)
- Log failures with full context
- Continue operating with cached last-known-good config

### Invalid Recommendations

- Validate all parameter values against defined ranges
- Reject out-of-range recommendations
- Log validation failures for analysis

### Rate Limiting

- Respect Claude API rate limits (check headers)
- Buffer requests if rate limited
- Default 2 minutes between API calls (configurable via AI_EVAL_INTERVAL_MINUTES)

## Monitoring

### Metrics to Track

| Metric | Type | Description |
|--------|------|-------------|
| `ai_decisions_total` | Counter | Total decisions made |
| `ai_decisions_successful` | Counter | Successful decisions |
| `ai_api_calls_total` | Counter | Total API calls |
| `ai_api_latency_ms` | Histogram | API response time |
| `ai_confidence_avg` | Gauge | Average confidence |

### Alerts

1. **AI Disabled**: When API key invalid or missing
2. **High Failure Rate**: Success rate < 50% over 10 decisions
3. **API Errors**: >3 consecutive API failures
4. **Stale Decisions**: No decisions made in >5 minutes when active

## Implementation Phases

### Phase 1: Core Infrastructure
- [ ] Create `ai_manager` module structure
- [ ] Implement `AIManagerConfig` with ENV loading
- [ ] Add `reqwest` client with retry logic
- [ ] Create `Decision` and `Outcome` structs

### Phase 2: API Integration
- [ ] Implement Claude API client
- [ ] Create system prompt with parameter documentation
- [ ] Parse structured JSON responses
- [ ] Handle errors and rate limiting

### Phase 3: Decision Logic
- [ ] Implement metrics snapshot collection
- [ ] Create parameter applier trait
- [ ] Add validation and safety checks
- [ ] Implement outcome evaluation loop

### Phase 4: Persistence
- [ ] JSON file storage for decision history
- [ ] Load history on startup
- [ ] Periodic auto-save
- [ ] History rotation (keep last N decisions)

### Phase 5: Observability
- [ ] Add to `/json` endpoint
- [ ] Prometheus metrics
- [ ] Structured logging
- [ ] Health check endpoint

## Files to Create

| File | Purpose |
|------|---------|
| `src/ai_manager/mod.rs` | Main AI manager module |
| `src/ai_manager/client.rs` | Claude API HTTP client |
| `src/ai_manager/history.rs` | Decision history storage |
| `src/ai_manager/analysis.rs` | Response parsing |

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
