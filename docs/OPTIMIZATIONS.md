# Optimization Techniques

Comprehensive catalog of all performance optimizations in Orbit Royale.

---

## Server (Rust)

### Memory & Allocation

| Technique | How it works |
|-----------|--------------|
| **Pre-allocation** | `Vec::with_capacity(n)` reserves memory upfront. Without it, vectors double in size when full, causing copies and cache misses. |
| **Structure of Arrays (SoA)** | Instead of `Vec<Bot>` with mixed fields, use separate `Vec<f32>` for each field. CPU loads 64 bytes at a time—SoA means loading 16 consecutive thrust values, not 1 thrust + 15 unrelated fields. |
| **BitVec for booleans** | A `Vec<bool>` uses 1 byte per bool. BitVec packs 8 bools into 1 byte. For 100K bots: 100KB → 12.5KB. |
| **Swap-remove deletion** | To remove index 3 from `[A,B,C,D,E]`: swap D↔E, then pop. Result: `[A,B,E,D]`. O(1) vs O(n) shifting. |
| **Dense index mapping** | Map `PlayerId (UUID)` → `u32 (0,1,2...)`. Access bot data via `array[dense_index]` instead of HashMap lookup. |
| **Thread-local buffers** | Reuse `Vec` via `thread_local!` + `RefCell`. Clear and reuse instead of allocating per-call. Used in: density grid, notable players, AOI filtering, collision pairs. |
| **SmallVec inline storage** | `SmallVec<[T; N]>` stores up to N items on stack, spills to heap only when exceeded. Used for inputs (4 inline), top player IDs (8 inline), and wave hit_players (16 inline). |
| **Buffer pool scaling** | `BufferPool::for_connections(n)` scales pool size (32-512) based on expected connections. Reduces allocation fallback rate. |
| **Arc<Vec<u8>> broadcasting** | Send `Arc<Vec<u8>>` through channels instead of cloning. 30 players × 2.5KB = 75KB saved per broadcast (only Arc pointer copied). |

### Network

| Technique | How it works |
|-----------|--------------|
| **AOI filtering** | Only send entities near the player. 1000 bots across map but only 50 nearby? Send 50. Bandwidth scales with density, not total count. |
| **Max entity cap** | Hard limit (100) prevents worst-case: player at center of 500-bot swarm would otherwise get massive packets. |
| **Velocity lookahead** | If player moves right at 200u/s, expand AOI rightward by 400 units (2s). Entities appear before player reaches them, not after. |
| **Density grid** | Instead of sending 1000 bot positions for minimap, divide arena into 16×16 cells and send count per cell (256 bytes total). Uses thread-local buffers. |
| **Notable players** | High-mass players are visible on radar. Sort by mass, take top 15. Uses thread-local buffer for sorting. |
| **Bincode** | Binary format: `f32` = 4 bytes. JSON `"123.456"` = 7+ bytes plus quotes/commas. 60% smaller overall. |
| **Spectator snapshot caching** | Pre-compute AOI snapshots for bots with spectator followers. Deduplicate via `HashSet`, share via `Arc`. O(M) instead of O(N×M). |
| **Bit-packed player flags** | Pack alive/spawn_protection/is_bot into single `u8`. Saves 2 bytes per player per snapshot (200+ bytes @ 100 players). |

### Concurrency

| Technique | How it works |
|-----------|--------------|
| **Crossbeam channels** | MPSC = Multi-Producer Single-Consumer. Many connection threads send inputs; one game loop receives. No locks—uses atomic operations internally. |
| **Parking lot RwLock** | Std RwLock has syscall overhead. Parking_lot spins briefly before sleeping, avoiding kernel transitions for short waits. |
| **Rayon par_iter** | `players.par_iter_mut()` splits work across CPU cores automatically. 8 cores = ~8x throughput for independent updates. Used for gravity, physics, AI. |
| **Hashbrown** | Google's SwissTable algorithm. Better cache utilization than std HashMap through SIMD-accelerated probing. |
| **FxHashMap** | `rustc_hash::FxHashMap` for faster hashing with small integer keys. Used for gravity well lookups, input buffering, and spatial grids. |
| **Lock-free buffer pool** | `crossbeam_channel` MPMC for buffer pool (no Mutex). Lock-free get/put eliminates contention in hot encoding path. |

### Spatial

| Technique | How it works |
|-----------|--------------|
| **Spatial hash grid** | Divide world into 64×64 unit cells. To find collisions: only check entities in same cell + 8 neighbors. 1000 bots but 10 per cell = 90 checks, not 500K. |
| **Inverse cell size** | Division: ~20 cycles. Multiplication: ~4 cycles. Pre-compute `inv = 1/64`, then `cell = pos * inv`. 5x faster in hot loop. |
| **Pre-computed offsets** | Neighbor cells: `[(-1,-1), (-1,0), ..., (1,1)]`. Static array—no allocation, no computation per query. |
| **Zone grid** | Larger cells (500u) storing aggregate data: total bots, average velocity, threat level. AI checks zone stats instead of individual bots. |
| **Well spatial grid** | Gravity wells have large influence (1000+ units). Separate grid with bigger cells prevents checking distant wells. |
| **Collision pair iterator** | Thread-local buffer + `for_each_potential_collision()` callback for zero-allocation iteration. |

### Algorithms

| Technique | How it works |
|-----------|--------------|
| **Distance squared** | `sqrt(dx² + dy²) < r` becomes `dx² + dy² < r²`. Square root costs ~20 cycles; squaring costs ~1. Same result. Used in AOI with pre-computed `effective_extended_radius_sq`. |
| **Early exit** | Found 100 entities? Stop searching. First collision found? Stop checking pairs. Don't do work you'll throw away. |
| **Input coalescing** | 3 inputs arrive in one tick. Thrust/aim: use latest (player's current intention). Fire-release: OR together (don't miss the tap). |
| **Behavioral batching** | Process all "orbit" bots together, then all "chase" bots. Same code path = instruction cache stays hot, branch predictor learns pattern. |
| **Sorted descending removal** | Removing indices [2,5,8]: start at 8. If you remove 2 first, indices 5 and 8 shift down and become wrong. |

### Game Loop

| Technique | How it works |
|-----------|--------------|
| **Tick budget monitoring** | Track last 120 tick times. P95 > 25ms? Performance degraded. P99 > 33ms? Dropping frames. Trigger adaptations. |
| **Adaptive dormancy** | EMA (Exponential Moving Average) smooths noisy tick times: `avg = avg*0.9 + current*0.1`. Gradual response, not jitter. |
| **Three-tier LOD** | Bots 2000+ units away: update AI every 8 ticks. Player won't notice—they're dots on screen. Saves 87.5% of AI computation. |
| **Active mask** | BitVec where bit N = "bot N is active this tick". Iterate only set bits. 10K bots, 1K active = iterate 1K, not 10K. |
| **30-tick cooldown** | Without cooldown: tick slow → reduce quality → tick fast → increase quality → tick slow. Oscillation. Cooldown dampens it. |

### Caching

| Technique | How it works |
|-----------|--------------|
| **Well cache** | Bot orbiting well #3. Cache well #3's ID for 0.5s instead of finding nearest well every tick (O(n) search). |
| **Zone data cache** | Aggregate stats (bot count, total mass) computed once at cycle start, reused by all zone queries that cycle. |
| **Cold path separation** | Bot personality (aggression, preferred distance) never changes. Store in separate array—hot loop never touches it, keeping cache clean. |
| **Well position cache** | `WellPositionCache` uses SoA layout (separate Vec for x, y, mass, min_dist_sq). Built once per tick, shared across all gravity calculations. |

### Low-level

| Technique | How it works |
|-----------|--------------|
| **#[inline]** | Hint to compiler: copy function body into caller. Eliminates call overhead for tiny hot functions like `vec2.length()`. |
| **Inverse multiply** | `x / 64` → `x * 0.015625`. Division is slow. |
| **Epsilon checks** | Two objects at same position: `distance = 0`, `normalize = divide by 0` = NaN/crash. Add tiny epsilon: `if dist > 0.001`. |
| **Exponential drag** | `vel *= 0.998` every tick. Alternative: `vel -= vel * drag * dt` requires multiply AND subtract AND depends on dt. |
| **Velocity clamping** | Physics bugs or cheats could create vel=999999. Clamp to max prevents entities escaping arena or breaking spatial grid. |

---

## Client (TypeScript)

### Rendering

| Technique | How it works |
|-----------|--------------|
| **Quality tiers** | Zoomed out = 200 players on screen. Full effects = GPU death. Detect zoom level, skip expensive effects (gradients, glow, particles). |
| **Pre-computed angles** | Birth effect needs 8 particles at 45° intervals. Calculate `sin(0°), cos(0°), sin(45°)...` once at startup, not 8 trig calls per frame. |
| **Color caching** | `"#FF5733"` → `{r:255, g:87, b:51}` requires parseInt × 3. Cache result in Map. Second access = O(1) lookup. |
| **Reusable objects** | `return {glow: x, core: y}` allocates object. GC must clean it. Instead: reuse single object, mutate properties. Zero allocation. |
| **Debris batching** | 100 debris: naive = 100 beginPath + 100 arc + 100 fill. Batched = 1 beginPath + 100 arc + 1 fill. GPU receives one draw call. |
| **Fill once pattern** | `ctx.fillStyle = "red"` has overhead. Set once per player, then vary only `globalAlpha` for transparency effects. |

### Memory

| Technique | How it works |
|-----------|--------------|
| **Trail capping** | Unlimited trail points = memory leak. Cap at 32. Old points shift out as new ones enter. Fixed memory per player. |
| **Lazy cleanup** | Player leaves: could scan all trails immediately. Instead: cleanup only when `trails.size > players.size + 5`. Amortized cost. |
| **Map-based storage** | Array: find player = O(n) scan. Map: find player = O(1). Delete player from Map = O(1). Array splice = O(n). |
| **Effect filtering** | Death effects last 2s. Each frame: `effects.filter(e => age < 2s)`. Prevents unbounded array growth. |

### Network

| Technique | How it works |
|-----------|--------------|
| **Delta compression** | Full player: 50 bytes. Delta: `[posChanged?][if yes: pos][velChanged?][if yes: vel]...`. Stationary player = 6 bits. |
| **Bincode encoding** | Fixed format: 4-byte length, then payload. No JSON parsing, no string allocation. DataView reads directly from ArrayBuffer. |
| **Adaptive interpolation** | Server sends 20 snapshots/sec but network jitters. Track average interval with EMA. Interpolation delay = 2× average interval. |
| **RTT via echo** | Client sends timestamp in input. Server echoes it in next snapshot. Client calculates: `RTT = now - echoed_timestamp`. No extra messages. |
| **Unreliable datagrams** | TCP retransmits lost packets—adds latency. Input is ephemeral; old input is useless. UDP-style: send and forget, server uses latest. |
| **Viewport reporting** | Zoomed out = larger visible area. Tell server zoom level; server expands AOI accordingly. Prevents entities popping in at edges. |
| **Pre-built Maps** | Interpolating between snapshots: need to find player by ID repeatedly. Build `Map<id, player>` once, then O(1) lookups during interpolation. |

### Data Structures

| Technique | How it works |
|-----------|--------------|
| **Typed arrays** | `DataView` on `ArrayBuffer` reads binary directly. Regular arrays box numbers as objects. TypedArrays = raw bytes, no boxing. |
| **Map<K, V>** | JS object keys are strings (hashed). Map keys can be any type with O(1) average lookup. Better than array.find() for repeated lookups. |
| **Set for tracking** | `destroyedWellIds.has(id)` = O(1). Array `includes(id)` = O(n). Set is purpose-built for membership tests. |

### Animation

| Technique | How it works |
|-----------|--------------|
| **RAF loop** | `requestAnimationFrame` fires before browser paint, synced to monitor refresh. `setInterval(16)` drifts and tears. |
| **Time-based** | `x += speed * dt` instead of `x += speed`. Frame drops don't cause slowdown—distance covered scales with actual elapsed time. |
| **Delta clamping** | Tab hidden for 10s, then visible: `dt = 10`. Entity moves 10 seconds in one frame = teleportation. Clamp `dt` to max 0.1s. |
| **Smooth interpolation** | Camera lerp 0.1 = reach 10% of distance to target per frame. Creates smooth follow. 1.0 = instant snap. 0.01 = sluggish. |

### Math

| Technique | How it works |
|-----------|--------------|
| **Distance squared** | Same as server. `Math.sqrt()` = ~20 cycles. Squared comparison = 2 multiplies. |
| **Length squared** | Check if vector is non-zero: `if (v.lengthSq() > 0)`. Avoids sqrt just to compare against zero. |
| **Direct sqrt** | `Math.sqrt(x)` is native, optimized. `Math.pow(x, 0.5)` goes through generic power function. Sqrt is faster. |
| **Angle wrapping** | Rotation 350° → 10°: naive lerp goes 350→180→10 (wrong way). Wrap difference to [-180°, 180°] first: 350° + 20° = 10°. |
| **Cubic easing** | `t³` starts slow, ends fast (ease-in). `1-(1-t)³` starts fast, ends slow (ease-out). More natural than linear. |

### Input & State

| Technique | How it works |
|-----------|--------------|
| **Listener cleanup** | Store `{element, event, handler}` tuples. On destroy: iterate and `removeEventListener`. Prevents memory leaks from orphaned handlers. |
| **State caching** | On keydown: set `isBoostHeld = true`. On keyup: set false. Reading input = read bool. No need to query keyboard state each frame. |
| **Input latching** | Eject button released between ticks. Without latch: tick reads `released = false` (too late). Latch: set flag, clear after tick reads it. |
| **Focus reset** | Tab away while holding W: keyup never fires. On blur: reset all held keys to false. Prevents stuck movement on return. |
| **Snapshot buffer limit** | Interpolation needs 2+ snapshots. Keep 32 max. Older ones useless—just memory. Ring buffer style. |
| **Destroyed tracking** | Well destroyed, but old snapshot (network delay) still has it. Track destroyed IDs; filter them from interpolation. |
| **Birth time tracking** | New player: play spawn animation. Player enters AOI (was always alive, just far): no animation. Track first-seen time to distinguish. |
| **Kill detection** | Player's kill count: 5 → 6. Difference = new kill. Trigger kill effect. Comparing counts avoids server sending explicit kill events. |

---

## Key Patterns Explained

### Why SoA beats AoS

```
Array of Structures (AoS):
[Bot{x,y,vx,vy,hp}, Bot{x,y,vx,vy,hp}, ...]

Structure of Arrays (SoA):
x:  [x0, x1, x2, ...]
y:  [y0, y1, y2, ...]
vx: [vx0, vx1, vx2, ...]
```

CPU cache line = 64 bytes. Updating all X positions:
- AoS: Load bot, use 4 bytes (x), waste 60 bytes
- SoA: Load 16 consecutive x values, use all 64 bytes

**Result:** 4-16x better cache utilization for bulk operations.

### Why Spatial Hashing is O(n)

Naive collision: every pair = n×(n-1)/2 = O(n²)

Spatial hash:
1. Insert all entities into grid cells: O(n)
2. For each entity, check same cell + 8 neighbors
3. Average entities per cell = n / cells
4. Checks per entity = 9 × (n / cells)
5. Total = n × 9 × (n / cells) = O(n) when cells scale with n

**1000 bots, 100 cells:** 90 checks/bot × 1000 = 90K checks (not 500K)

### Why Lock-Free Channels

Mutex: Thread A locks, Thread B waits (blocks), OS context switch (expensive)

Lock-free channel: Atomic compare-and-swap. No waiting, no context switch. Both threads make progress.

**Throughput:** 10-100x higher for high-contention scenarios.

---

## Impact Summary

| Optimization | Before | After | Improvement |
|--------------|--------|-------|-------------|
| SoA layout | Random cache access | Sequential access | 3-5x faster |
| Spatial hash | O(n²) collision | O(n) collision | 100x+ at scale |
| AOI filtering | Send all entities | Send nearby only | 50-80% bandwidth |
| Bincode | JSON strings | Binary | 60% smaller |
| Delta compression | Full snapshots | Changed fields only | 70-90% smaller |
| BitVec | 1 byte/bool | 1 bit/bool | 8x memory |
| Quality tiers | Full effects always | Adaptive quality | 3x fewer draws |
| Unreliable input | TCP retransmit | Fire-and-forget | Lower latency |
| Thread-local buffers | Alloc per call | Reuse buffer | 15-25% tick time |
| FxHashMap | std HashMap | Faster hash | 5-10% lookup |
| Arc<Vec<u8>> channels | Clone 2.5KB/player | Clone 16-byte Arc | ~250KB/sec saved |
| Bit-packed flags | 3 bytes/player | 1 byte/player | 200+ bytes/snapshot |
| Lock-free buffer pool | Mutex contention | Lock-free | Reduced latency |

---

## Future Optimizations

Opportunities not yet implemented. Profile before implementing.

### Server - SIMD

| Technique | How it works | Expected Impact |
|-----------|--------------|-----------------|
| **Batch position updates** | Use `std::simd` to update 4 positions simultaneously. | 2-4x physics throughput |
| **SIMD velocity normalization** | Normalize batches of velocity vectors using SIMD. | 4x faster bulk normalize |
| **Gravity force SIMD** | Calculate gravity from 4 wells simultaneously per entity. | 2-4x gravity computation |

### Server - Data Structures

| Technique | How it works | Expected Impact |
|-----------|--------------|-----------------|
| **Index-based GameState** | Replace `HashMap<PlayerId, Player>` with `Vec<Option<Player>>` + dense indices. | Better cache locality |
| **Generational arena** | Use generational arena for projectiles/debris for O(1) removal. | O(removed) vs O(total) |

### Client - Rendering

| Technique | How it works | Expected Impact |
|-----------|--------------|-----------------|
| **Well Map caching** | Cache Map when wells unchanged instead of rebuilding every frame. | 1 alloc/frame → 0 |
| **Canvas state batching** | Group entities by color, set fillStyle once per group. | 50-80% fewer state changes |
| **Vec2 object pooling** | Pool and reuse Vec2 objects instead of creating new ones. | 90% fewer allocations |
| **Trail circular buffer** | Replace `.shift()` (O(n)) with ring buffer (O(1)). | O(1) vs O(n) per trail |

### Client - Advanced

| Technique | When to use | Expected Impact |
|-----------|-------------|-----------------|
| **OffscreenCanvas** | When targeting 60+ FPS with complex effects | Parallel rendering |
| **Web Workers** | If interpolation is a bottleneck | Main thread stays responsive |
| **WebGL renderer** | If entity count exceeds 500 visible | 3-10x draw throughput |

### Network Protocol

| Technique | How it works | Expected Impact |
|-----------|--------------|-----------------|
| **Position quantization** | 16-bit fixed-point instead of 32-bit float. | 50% position data reduction |
| **Velocity prediction** | Skip velocity if linear prediction matches. | 30-50% fewer velocity updates |
| **Interest tiers** | Nearby = full, medium = position-only, far = existence-only. | Bandwidth scales with distance |
| **Name caching** | Send names only on join, use ID-only in snapshots. | 1-2KB per snapshot saved |

---

## Profiling Tools

| Tool | Platform | Use for |
|------|----------|---------|
| `cargo flamegraph` | Rust | CPU hotspots, call graph |
| `perf` + `hotspot` | Rust/Linux | Low-level CPU analysis |
| `tracy` | Rust | Frame-by-frame profiling |
| Chrome DevTools Performance | TypeScript | Frame timing, GC pauses |
| Chrome DevTools Memory | TypeScript | Allocation tracking |

**Rule:** Measure twice, optimize once.
