# AI Simulation Manager

Autonomous system that monitors game metrics, analyzes patterns via Claude API, and tunes parameters in real-time.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    AI Manager System                        │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐  │
│  │  Collector   │───▶│   Analyst    │───▶│   Executor   │  │
│  │  (metrics)   │    │ (Claude API) │    │ (apply cfg)  │  │
│  └──────────────┘    └──────────────┘    └──────────────┘  │
│         ▲                   │                    │          │
│         │                   ▼                    │          │
│         │           ┌──────────────┐             │          │
│         └───────────│   History    │◀────────────┘          │
│                     │  (decisions) │                        │
│                     └──────────────┘                        │
└─────────────────────────────────────────────────────────────┘
```

## Core Principles

1. **Observability First** - All decisions logged with full context
2. **Reversible Changes** - Parameters can always be reverted
3. **Gradual Adjustments** - Small incremental changes only
4. **Outcome Evaluation** - Every decision evaluated after delay
5. **Learning Loop** - Past outcomes inform future decisions

## Metrics Collected

| Metric | Purpose |
|--------|---------|
| `tick_time_p95` | Primary health indicator |
| `total_players` | Population tracking |
| `alive_players` | Engagement indicator |
| `arena_scale` | Space utilization |
| `gravity_wells` | Complexity metric |
| `debris_count` | Entity load |
| `messages_sent` | Bandwidth indicator |

## Claude API Integration

**Request:**
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

**Response:**
```json
{
  "analysis": "Tick time elevated at 18ms with 500 players...",
  "recommendations": [{
    "parameter": "arena.max_wells",
    "current": 20,
    "recommended": 15,
    "reason": "Reduce physics computation"
  }],
  "confidence": 0.85,
  "risk_level": "low"
}
```

## Tunable Parameters

### Arena Scaling

| Parameter | Range | Default |
|-----------|-------|---------|
| `arena.grow_lerp` | 0.01-0.1 | 0.02 |
| `arena.shrink_lerp` | 0.001-0.05 | 0.005 |
| `arena.shrink_delay_ticks` | 0-300 | 150 |
| `arena.min_radius` | 500-2000 | 800 |
| `arena.max_multiplier` | 5-20 | 10.0 |
| `arena.growth_per_player` | 5-50 | 10.0 |
| `arena.max_wells` | 5-50 | 20 |

### Simulation

| Parameter | Range | Default |
|-----------|-------|---------|
| `simulation.min_bots` | 10-500 | 50 |
| `simulation.max_bots` | 50-2000 | 500 |
| `simulation.cycle_minutes` | 1-60 | 10 |

### Physics

| Parameter | Range | Default |
|-----------|-------|---------|
| `physics.gravity_strength` | 0.5-5.0 | 1.0 |
| `physics.debris_friction` | 0.9-1.0 | 0.98 |

## Decision Rules

### Performance Thresholds

| Condition | Action |
|-----------|--------|
| tick_time_p95 > 25ms | Reduce complexity (fewer wells, less debris) |
| tick_time_p95 < 10ms | Can increase complexity |
| Target | <20ms (66% of 33ms budget) |

### Population Scaling

| Population | Strategy |
|------------|----------|
| <100 | Reduce arena size, increase density |
| >500 | Expand arena, distribute wells |
| >800 | Limit entities, prioritize performance |

### Safety Constraints

| Rule | Value |
|------|-------|
| Max change per decision | 20% of current |
| Min time between changes | 2 minutes |
| Revert threshold | Performance drop >30% |
| Confidence threshold | >0.7 to act |

## Decision History

Stored in `data/ai_decisions.json`:

```json
{
  "version": 1,
  "decisions": [{
    "id": "dec_20240115_103000_001",
    "timestamp": "2024-01-15T10:30:00Z",
    "metrics_before": { "tick_time_p95": 18500, "total_players": 523 },
    "actions": [{ "parameter": "arena.max_wells", "old_value": 20, "new_value": 15 }],
    "confidence": 0.85,
    "outcome": {
      "evaluated_at": "2024-01-15T10:32:00Z",
      "metrics_after": { "tick_time_p95": 14200 },
      "performance_delta_us": -4300,
      "success": true
    }
  }],
  "statistics": {
    "total_decisions": 42,
    "successful": 33,
    "success_rate": 0.786
  }
}
```

## Error Handling

| Scenario | Response |
|----------|----------|
| API failure | Retry with exponential backoff (1s→60s max) |
| Invalid recommendation | Reject, log for analysis |
| Rate limit | Buffer requests, respect headers |
| Parameter out of range | Reject recommendation |

## Monitoring

| Metric | Type |
|--------|------|
| `ai_decisions_total` | Counter |
| `ai_decisions_successful` | Counter |
| `ai_api_calls_total` | Counter |
| `ai_api_latency_ms` | Histogram |
| `ai_confidence_avg` | Gauge |

## Configuration

```bash
AI_ENABLED=false                # Enable AI manager
ORBIT_API_KEY=sk-ant-...        # Claude API key (required if enabled)
AI_EVAL_INTERVAL_MINUTES=2      # Minutes between evaluations
AI_MAX_HISTORY=100              # Max decisions to keep
AI_CONFIDENCE_THRESHOLD=0.7     # Min confidence to act
AI_MODEL=claude-sonnet-4-5      # Model to use
```

## Implementation

### Files to Create

| File | Purpose |
|------|---------|
| `src/ai_manager/mod.rs` | Main module |
| `src/ai_manager/client.rs` | Claude API client |
| `src/ai_manager/history.rs` | Decision storage |
| `src/ai_manager/analysis.rs` | Response parsing |

### Files to Modify

| File | Changes |
|------|---------|
| `src/config.rs` | Add `AIManagerConfig` |
| `src/net/game_session.rs` | Load config, spawn AI manager |
| `src/metrics.rs` | Add AI state to `/json` |
| `src/lib.rs` | Add `ai_manager` module |
| `Cargo.toml` | Add `reqwest`, `chrono` |
