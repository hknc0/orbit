// State synchronization with interpolation and client prediction

import { Vec2, vec2Lerp } from '@/utils/Vec2';
import { NETWORK, PHYSICS, BOOST, massToThrustMultiplier } from '@/utils/Constants';

const { ADAPTIVE_INTERPOLATION } = NETWORK;
import type {
  GameSnapshot,
  DeltaUpdate,
  PlayerSnapshot,
  PlayerInput,
  PlayerId,
  MatchPhase,
} from './Protocol';

// Gravity well for rendering
export interface InterpolatedGravityWell {
  id: number;
  position: Vec2;
  mass: number;
  coreRadius: number;
  bornTime: number; // Timestamp when well first appeared (for birth animation)
}

// Interpolated state for rendering
export interface InterpolatedState {
  tick: number;
  matchPhase: MatchPhase;
  matchTime: number;
  countdown: number;
  players: Map<PlayerId, InterpolatedPlayer>;
  projectiles: Map<number, InterpolatedProjectile>;
  debris: Map<number, InterpolatedDebris>; // Collectible particles
  arenaCollapsePhase: number;
  arenaSafeRadius: number;
  arenaScale: number;
  gravityWells: InterpolatedGravityWell[];
  totalPlayers: number;  // Total players before AOI filtering
  totalAlive: number;    // Total alive before AOI filtering
  densityGrid: number[]; // 16x16 grid of player counts for minimap heatmap
  notablePlayers: InterpolatedNotablePlayer[]; // High-mass players for minimap radar
}

// Notable player for minimap radar (high-mass, visible everywhere)
export interface InterpolatedNotablePlayer {
  id: PlayerId;
  position: Vec2;
  mass: number;
  colorIndex: number;
}

export interface InterpolatedPlayer {
  id: PlayerId;
  name: string;
  position: Vec2;
  velocity: Vec2;
  rotation: number;
  mass: number;
  alive: boolean;
  kills: number;
  deaths: number;
  spawnProtection: boolean;
  isBot: boolean;
  colorIndex: number;
  bornTime: number; // Timestamp when player spawned (0 = skip animation, >0 = show birth effect)
}

export interface InterpolatedProjectile {
  id: number;
  ownerId: PlayerId;
  position: Vec2;
  velocity: Vec2;
  mass: number;
}

export interface InterpolatedDebris {
  id: number;
  position: Vec2;
  size: number; // 0=Small, 1=Medium, 2=Large
}

interface SnapshotEntry {
  tick: number;
  timestamp: number;
  snapshot: GameSnapshot;
  // Pre-computed Map for O(1) well lookups during interpolation
  wellMap: Map<number, import('./Protocol').GravityWellSnapshot>;
}

export class StateSync {
  // Snapshot buffer for interpolation
  private snapshots: SnapshotEntry[] = [];
  private readonly maxSnapshots = NETWORK.SNAPSHOT_BUFFER_SIZE;

  // Current authoritative state
  private currentTick: number = 0;

  // Client prediction
  private localPlayerId: PlayerId | null = null;
  private pendingInputs: PlayerInput[] = [];
  private predictedPosition: Vec2 = new Vec2();
  private predictedVelocity: Vec2 = new Vec2();

  // Adaptive interpolation delay based on snapshot arrival rate
  // Spectators at reduced rate (15Hz) need more buffer than players (30Hz)
  private adaptiveDelay: number = NETWORK.INTERPOLATION_DELAY_MS;
  private lastSnapshotTime: number = 0;
  private snapshotIntervalAvg: number = PHYSICS.DT * 1000;  // Start assuming tick rate

  // Destroyed gravity wells (filter from interpolated state until server confirms removal)
  private destroyedWellIds: Set<number> = new Set();

  // Track when gravity wells were first seen (for birth animation)
  // bornTime = 0 means skip animation (pre-existing well)
  // bornTime > 0 means show birth animation
  private wellBornTimes: Map<number, number> = new Map();

  // Track when players spawned (for birth animation)
  // bornTime = 0 means skip animation (entered AOI, already alive)
  // bornTime > 0 means show birth animation (actually spawned/respawned)
  private playerBornTimes: Map<PlayerId, number> = new Map();

  // Track if we've received the first snapshot (entities in first snapshot skip birth animation)
  private hasReceivedFirstSnapshot: boolean = false;

  setLocalPlayerId(id: PlayerId): void {
    this.localPlayerId = id;
  }

  // Getter for current interpolation delay (adapts based on snapshot rate)
  get interpolationDelay(): number {
    return this.adaptiveDelay;
  }

  // Mark a gravity well as destroyed (called when GravityWellDestroyed event received)
  markWellDestroyed(wellId: number): void {
    this.destroyedWellIds.add(wellId);
    this.wellBornTimes.delete(wellId);
  }

  // Apply a full snapshot from server
  applySnapshot(snapshot: GameSnapshot): void {
    const now = performance.now();

    // Track snapshot arrival rate for adaptive interpolation
    if (this.lastSnapshotTime > 0) {
      const interval = now - this.lastSnapshotTime;
      // Only update if interval is reasonable (10ms-500ms range)
      if (interval > 10 && interval < 500) {
        // Exponential moving average for smooth adaptation
        this.snapshotIntervalAvg =
          this.snapshotIntervalAvg * (1 - ADAPTIVE_INTERPOLATION.SMOOTHING_FACTOR) +
          interval * ADAPTIVE_INTERPOLATION.SMOOTHING_FACTOR;

        // Calculate adaptive delay: buffer enough snapshots for smooth playback
        // delay = interval * bufferSnapshots, clamped to min/max
        const targetDelay = this.snapshotIntervalAvg * ADAPTIVE_INTERPOLATION.BUFFER_SNAPSHOTS;
        this.adaptiveDelay = Math.max(
          ADAPTIVE_INTERPOLATION.MIN_DELAY_MS,
          Math.min(ADAPTIVE_INTERPOLATION.MAX_DELAY_MS, targetDelay)
        );
      }
    }
    this.lastSnapshotTime = now;

    // Pre-compute well Map for O(1) lookups during interpolation
    const wellMap = new Map(snapshot.gravityWells.map(w => [w.id, w]));

    // Add to buffer
    this.snapshots.push({
      tick: snapshot.tick,
      timestamp: now,
      snapshot,
      wellMap,
    });

    // Keep buffer size limited
    while (this.snapshots.length > this.maxSnapshots) {
      this.snapshots.shift();
    }

    // Update current tick
    if (snapshot.tick > this.currentTick) {
      this.currentTick = snapshot.tick;
    }

    // Clean up destroyed wells tracking when server confirms removal
    if (this.destroyedWellIds.size > 0) {
      const serverWellIds = new Set(snapshot.gravityWells.map(w => w.id));
      for (const wellId of this.destroyedWellIds) {
        if (!serverWellIds.has(wellId)) {
          this.destroyedWellIds.delete(wellId);
        }
      }
    }

    // Reconcile client prediction
    if (this.localPlayerId) {
      const localPlayer = snapshot.players.find((p) => p.id === this.localPlayerId);
      if (localPlayer) {
        this.reconcilePrediction(localPlayer, snapshot.tick);
      }
    }
  }

  // Apply a delta update
  applyDelta(delta: DeltaUpdate): void {
    // Find base snapshot
    const baseEntry = this.snapshots.find((s) => s.tick === delta.baseTick);
    if (!baseEntry) {
      // Missing base snapshot, request full snapshot
      return;
    }

    // Create new snapshot from delta
    // Note: debris comes from delta (full list), not from base
    const newSnapshot: GameSnapshot = {
      tick: delta.tick,
      matchPhase: baseEntry.snapshot.matchPhase,
      matchTime: baseEntry.snapshot.matchTime,
      countdown: baseEntry.snapshot.countdown,
      players: [...baseEntry.snapshot.players],
      projectiles: [...baseEntry.snapshot.projectiles],
      debris: delta.debris, // Use debris from delta (full list)
      arenaCollapsePhase: baseEntry.snapshot.arenaCollapsePhase,
      arenaSafeRadius: baseEntry.snapshot.arenaSafeRadius,
      arenaScale: baseEntry.snapshot.arenaScale,
      gravityWells: baseEntry.snapshot.gravityWells,
      totalPlayers: baseEntry.snapshot.totalPlayers,
      totalAlive: baseEntry.snapshot.totalAlive,
      densityGrid: baseEntry.snapshot.densityGrid,
      notablePlayers: baseEntry.snapshot.notablePlayers,
      echoClientTime: baseEntry.snapshot.echoClientTime,
    };

    // Apply player deltas
    for (const playerDelta of delta.playerUpdates) {
      const playerIndex = newSnapshot.players.findIndex((p) => p.id === playerDelta.id);
      if (playerIndex >= 0) {
        const player = { ...newSnapshot.players[playerIndex] };
        if (playerDelta.position) player.position = playerDelta.position;
        if (playerDelta.velocity) player.velocity = playerDelta.velocity;
        if (playerDelta.rotation !== undefined) player.rotation = playerDelta.rotation;
        if (playerDelta.mass !== undefined) player.mass = playerDelta.mass;
        if (playerDelta.alive !== undefined) player.alive = playerDelta.alive;
        if (playerDelta.kills !== undefined) player.kills = playerDelta.kills;
        newSnapshot.players[playerIndex] = player;
      }
    }

    // Apply projectile deltas (update existing or add new)
    for (const projDelta of delta.projectileUpdates) {
      const projIndex = newSnapshot.projectiles.findIndex((p) => p.id === projDelta.id);
      if (projIndex >= 0) {
        // Update existing projectile
        newSnapshot.projectiles[projIndex] = {
          ...newSnapshot.projectiles[projIndex],
          position: projDelta.position,
          velocity: projDelta.velocity,
        };
      } else {
        // New projectile - add it (wasn't in base snapshot)
        newSnapshot.projectiles.push({
          id: projDelta.id,
          position: projDelta.position,
          velocity: projDelta.velocity,
          ownerId: '', // Owner unknown from delta, will be updated on next full snapshot
          mass: 1, // Default mass, will be updated on next full snapshot
        });
      }
    }

    // Remove projectiles
    newSnapshot.projectiles = newSnapshot.projectiles.filter(
      (p) => !delta.removedProjectiles.includes(p.id)
    );

    this.applySnapshot(newSnapshot);
  }

  // Record an input for client prediction
  recordInput(input: PlayerInput): void {
    this.pendingInputs.push(input);

    // Limit pending inputs
    while (this.pendingInputs.length > NETWORK.INPUT_BUFFER_SIZE) {
      this.pendingInputs.shift();
    }
  }

  // Get interpolated state for rendering
  getInterpolatedState(): InterpolatedState | null {
    if (this.snapshots.length < 2) {
      // Not enough data for interpolation, return latest
      if (this.snapshots.length === 1) {
        return this.snapshotToInterpolatedState(this.snapshots[0]);
      }
      return null;
    }

    // Calculate render time (current time minus interpolation delay)
    const renderTime = performance.now() - this.interpolationDelay;

    // Find surrounding snapshots
    let before: SnapshotEntry | null = null;
    let after: SnapshotEntry | null = null;

    for (let i = 0; i < this.snapshots.length - 1; i++) {
      if (
        this.snapshots[i].timestamp <= renderTime &&
        this.snapshots[i + 1].timestamp >= renderTime
      ) {
        before = this.snapshots[i];
        after = this.snapshots[i + 1];
        break;
      }
    }

    // If no surrounding snapshots found, use edges
    if (!before || !after) {
      if (renderTime < this.snapshots[0].timestamp) {
        return this.snapshotToInterpolatedState(this.snapshots[0]);
      } else {
        return this.snapshotToInterpolatedState(
          this.snapshots[this.snapshots.length - 1]
        );
      }
    }

    // Calculate interpolation factor
    const duration = after.timestamp - before.timestamp;
    const t = duration > 0 ? (renderTime - before.timestamp) / duration : 0;

    return this.interpolateSnapshots(before, after, t);
  }

  // Get predicted position for local player (for rendering)
  getPredictedLocalPlayer(): { position: Vec2; velocity: Vec2 } | null {
    if (!this.localPlayerId) return null;

    return {
      position: this.predictedPosition.clone(),
      velocity: this.predictedVelocity.clone(),
    };
  }

  getCurrentTick(): number {
    return this.currentTick;
  }

  private reconcilePrediction(serverPlayer: PlayerSnapshot, serverTick: number): void {
    // Remove acknowledged inputs
    this.pendingInputs = this.pendingInputs.filter((input) => input.tick > serverTick);

    // Start from server state
    this.predictedPosition.copy(serverPlayer.position);
    this.predictedVelocity.copy(serverPlayer.velocity);

    // Re-apply pending inputs for client-side prediction
    for (const input of this.pendingInputs) {
      this.simulateInput(input, serverPlayer.mass);
    }
  }

  private simulateInput(input: PlayerInput, mass: number): void {
    // Apply thrust with mass-based scaling (smaller = faster, larger = slower)
    if (input.boost && input.thrust.lengthSq() > 0) {
      const thrustMultiplier = massToThrustMultiplier(mass);
      const thrustMagnitude = BOOST.BASE_THRUST * thrustMultiplier;
      const thrust = input.thrust.clone().normalize().scale(thrustMagnitude * PHYSICS.DT);
      this.predictedVelocity.add(thrust);
    }

    // Apply drag
    const dragFactor = Math.pow(1 - PHYSICS.DRAG, 1);
    this.predictedVelocity.scale(dragFactor);

    // Clamp velocity
    this.predictedVelocity.clampLength(PHYSICS.MAX_VELOCITY);

    // Update position
    this.predictedPosition.x += this.predictedVelocity.x * PHYSICS.DT;
    this.predictedPosition.y += this.predictedVelocity.y * PHYSICS.DT;
  }

  private snapshotToInterpolatedState(entry: SnapshotEntry): InterpolatedState {
    const { snapshot, wellMap } = entry;
    const now = performance.now();

    // Track player birth times before building player map
    // Players appearing for the first time = 0 (no animation - they're entering AOI, not spawning)
    // Only animate actual respawns (detected via alive state change in interpolateSnapshots)
    for (const player of snapshot.players) {
      if (!this.playerBornTimes.has(player.id)) {
        // First time seeing this player - no animation (they're entering our AOI, not spawning)
        this.playerBornTimes.set(player.id, 0);
      }
      // Note: don't delete on death - need to track to distinguish respawn from AOI entry
    }

    // Cleanup: remove tracking for players who left our AOI (prevents memory leak)
    const currentPlayerIds = new Set(snapshot.players.map(p => p.id));
    for (const trackedId of this.playerBornTimes.keys()) {
      if (!currentPlayerIds.has(trackedId)) {
        this.playerBornTimes.delete(trackedId);
      }
    }

    const players = new Map<PlayerId, InterpolatedPlayer>();
    for (const player of snapshot.players) {
      players.set(player.id, {
        ...player,
        position: player.position.clone(),
        velocity: player.velocity.clone(),
        bornTime: this.playerBornTimes.get(player.id) ?? 0,
      });
    }

    const projectiles = new Map<number, InterpolatedProjectile>();
    for (const proj of snapshot.projectiles) {
      projectiles.set(proj.id, {
        ...proj,
        position: proj.position.clone(),
        velocity: proj.velocity.clone(),
      });
    }

    const debris = new Map<number, InterpolatedDebris>();
    for (const d of snapshot.debris) {
      debris.set(d.id, {
        id: d.id,
        position: d.position.clone(),
        size: d.size,
      });
    }

    // Track new wells and assign born times
    // First snapshot: bornTime = 0 (skip animation for pre-existing wells)
    // Subsequent: bornTime = now (show birth animation for newly spawned wells)
    for (const w of snapshot.gravityWells) {
      if (!this.wellBornTimes.has(w.id)) {
        // Use 0 for first snapshot (skip animation), now for subsequent (show animation)
        this.wellBornTimes.set(w.id, this.hasReceivedFirstSnapshot ? now : 0);
      }
    }
    this.hasReceivedFirstSnapshot = true;

    // Build gravity wells array, filtering destroyed wells using pre-computed Map
    const gravityWells: InterpolatedGravityWell[] = [];
    for (const [id, w] of wellMap) {
      if (this.destroyedWellIds.size === 0 || !this.destroyedWellIds.has(id)) {
        gravityWells.push({
          id: w.id,
          position: w.position.clone(),
          mass: w.mass,
          coreRadius: w.coreRadius,
          bornTime: this.wellBornTimes.get(w.id) ?? 0,
        });
      }
    }

    return {
      tick: snapshot.tick,
      matchPhase: snapshot.matchPhase,
      matchTime: snapshot.matchTime,
      countdown: snapshot.countdown,
      players,
      projectiles,
      debris,
      arenaCollapsePhase: snapshot.arenaCollapsePhase,
      arenaSafeRadius: snapshot.arenaSafeRadius,
      arenaScale: snapshot.arenaScale,
      gravityWells,
      totalPlayers: snapshot.totalPlayers,
      totalAlive: snapshot.totalAlive,
      densityGrid: snapshot.densityGrid,
      notablePlayers: snapshot.notablePlayers.map((p) => ({
        id: p.id,
        position: p.position.clone(),
        mass: p.mass,
        colorIndex: p.colorIndex,
      })),
    };
  }

  private interpolateSnapshots(
    beforeEntry: SnapshotEntry,
    afterEntry: SnapshotEntry,
    t: number
  ): InterpolatedState {
    const before = beforeEntry.snapshot;
    const after = afterEntry.snapshot;
    const now = performance.now();
    const players = new Map<PlayerId, InterpolatedPlayer>();

    // Pre-build Maps for O(1) lookups (avoids O(n²) find() calls)
    const beforePlayerMap = new Map(before.players.map(p => [p.id, p]));
    const beforeProjMap = new Map(before.projectiles.map(p => [p.id, p]));
    const beforeDebrisMap = new Map(before.debris.map(d => [d.id, d]));

    // Track player birth times before building player map
    // Only animate actual respawns, NOT players entering AOI for the first time
    for (const afterPlayer of after.players) {
      const beforePlayer = beforePlayerMap.get(afterPlayer.id);
      const wasTrackedAndDead = this.playerBornTimes.has(afterPlayer.id) && beforePlayer && !beforePlayer.alive;
      const justRespawned = wasTrackedAndDead && afterPlayer.alive;

      if (!this.playerBornTimes.has(afterPlayer.id)) {
        // First time seeing this player - no animation (entering AOI, not spawning)
        this.playerBornTimes.set(afterPlayer.id, 0);
      } else if (justRespawned && afterPlayer.spawnProtection) {
        // Actual respawn detected - animate birth effect
        this.playerBornTimes.set(afterPlayer.id, now);
      }
    }

    // Cleanup: remove tracking for players who left our AOI (prevents memory leak)
    const currentPlayerIds = new Set(after.players.map(p => p.id));
    for (const trackedId of this.playerBornTimes.keys()) {
      if (!currentPlayerIds.has(trackedId)) {
        this.playerBornTimes.delete(trackedId);
      }
    }

    // Interpolate players
    for (const afterPlayer of after.players) {
      const beforePlayer = beforePlayerMap.get(afterPlayer.id);
      const bornTime = this.playerBornTimes.get(afterPlayer.id) ?? 0;

      if (beforePlayer) {
        // Check if player just respawned (was dead, now alive with spawn protection)
        // In this case, DON'T interpolate - snap to new position to avoid "flying" effect
        const justRespawned = !beforePlayer.alive && afterPlayer.alive;
        const justSpawned = !beforePlayer.spawnProtection && afterPlayer.spawnProtection;

        if (justRespawned || justSpawned) {
          // Snap to new position - no interpolation
          players.set(afterPlayer.id, {
            ...afterPlayer,
            position: afterPlayer.position.clone(),
            velocity: afterPlayer.velocity.clone(),
            bornTime,
          });
        } else {
          // Normal interpolation
          players.set(afterPlayer.id, {
            id: afterPlayer.id,
            name: afterPlayer.name,
            position: vec2Lerp(beforePlayer.position, afterPlayer.position, t),
            velocity: vec2Lerp(beforePlayer.velocity, afterPlayer.velocity, t),
            rotation: this.lerpAngle(beforePlayer.rotation, afterPlayer.rotation, t),
            mass: beforePlayer.mass + (afterPlayer.mass - beforePlayer.mass) * t,
            alive: afterPlayer.alive,
            kills: afterPlayer.kills,
            deaths: afterPlayer.deaths,
            spawnProtection: afterPlayer.spawnProtection,
            isBot: afterPlayer.isBot,
            colorIndex: afterPlayer.colorIndex,
            bornTime,
          });
        }
      } else {
        // New player, no interpolation
        players.set(afterPlayer.id, {
          ...afterPlayer,
          position: afterPlayer.position.clone(),
          velocity: afterPlayer.velocity.clone(),
          bornTime,
        });
      }
    }

    // Interpolate projectiles
    const projectiles = new Map<number, InterpolatedProjectile>();
    for (const afterProj of after.projectiles) {
      const beforeProj = beforeProjMap.get(afterProj.id);

      if (beforeProj) {
        projectiles.set(afterProj.id, {
          id: afterProj.id,
          ownerId: afterProj.ownerId,
          position: vec2Lerp(beforeProj.position, afterProj.position, t),
          velocity: vec2Lerp(beforeProj.velocity, afterProj.velocity, t),
          mass: beforeProj.mass + (afterProj.mass - beforeProj.mass) * t,
        });
      } else {
        projectiles.set(afterProj.id, {
          ...afterProj,
          position: afterProj.position.clone(),
          velocity: afterProj.velocity.clone(),
        });
      }
    }

    // Interpolate debris (no velocity, just position)
    const debris = new Map<number, InterpolatedDebris>();
    for (const afterDebris of after.debris) {
      const beforeDebris = beforeDebrisMap.get(afterDebris.id);

      if (beforeDebris) {
        debris.set(afterDebris.id, {
          id: afterDebris.id,
          position: vec2Lerp(beforeDebris.position, afterDebris.position, t),
          size: afterDebris.size,
        });
      } else {
        debris.set(afterDebris.id, {
          id: afterDebris.id,
          position: afterDebris.position.clone(),
          size: afterDebris.size,
        });
      }
    }

    // Track new wells and assign born times
    // Only show birth animation for wells that appear after first snapshot
    for (const [id] of afterEntry.wellMap) {
      if (!this.wellBornTimes.has(id)) {
        this.wellBornTimes.set(id, this.hasReceivedFirstSnapshot ? now : 0);
      }
    }

    // Interpolate gravity wells using pre-computed Maps (O(1) lookups)
    const gravityWells: InterpolatedGravityWell[] = [];
    for (const [id, afterWell] of afterEntry.wellMap) {
      // Skip destroyed wells
      if (this.destroyedWellIds.size > 0 && this.destroyedWellIds.has(id)) {
        continue;
      }
      const bornTime = this.wellBornTimes.get(id) ?? 0;
      const beforeWell = beforeEntry.wellMap.get(id);
      if (beforeWell) {
        gravityWells.push({
          id: afterWell.id,
          position: vec2Lerp(beforeWell.position, afterWell.position, t),
          mass: beforeWell.mass + (afterWell.mass - beforeWell.mass) * t,
          coreRadius: beforeWell.coreRadius + (afterWell.coreRadius - beforeWell.coreRadius) * t,
          bornTime,
        });
      } else {
        gravityWells.push({
          id: afterWell.id,
          position: afterWell.position.clone(),
          mass: afterWell.mass,
          coreRadius: afterWell.coreRadius,
          bornTime,
        });
      }
    }

    return {
      tick: after.tick,
      matchPhase: after.matchPhase,
      matchTime: before.matchTime + (after.matchTime - before.matchTime) * t,
      countdown: before.countdown + (after.countdown - before.countdown) * t,
      players,
      projectiles,
      debris,
      arenaCollapsePhase: after.arenaCollapsePhase,
      arenaSafeRadius:
        before.arenaSafeRadius + (after.arenaSafeRadius - before.arenaSafeRadius) * t,
      arenaScale: before.arenaScale + (after.arenaScale - before.arenaScale) * t,
      gravityWells,
      totalPlayers: after.totalPlayers,
      totalAlive: after.totalAlive,
      densityGrid: after.densityGrid,
      notablePlayers: after.notablePlayers.map((afterPlayer) => {
        const beforePlayer = before.notablePlayers.find((p) => p.id === afterPlayer.id);
        if (beforePlayer) {
          return {
            id: afterPlayer.id,
            position: vec2Lerp(beforePlayer.position, afterPlayer.position, t),
            mass: beforePlayer.mass + (afterPlayer.mass - beforePlayer.mass) * t,
            colorIndex: afterPlayer.colorIndex,
          };
        }
        return {
          id: afterPlayer.id,
          position: afterPlayer.position.clone(),
          mass: afterPlayer.mass,
          colorIndex: afterPlayer.colorIndex,
        };
      }),
    };
  }

  private lerpAngle(a: number, b: number, t: number): number {
    // Handle angle wrapping
    let diff = b - a;
    while (diff > Math.PI) diff -= Math.PI * 2;
    while (diff < -Math.PI) diff += Math.PI * 2;
    return a + diff * t;
  }

  reset(): void {
    this.snapshots = [];
    this.currentTick = 0;
    this.pendingInputs = [];
    this.predictedPosition = new Vec2();
    this.predictedVelocity = new Vec2();
    this.destroyedWellIds.clear();
    this.wellBornTimes.clear();
    // Reset adaptive interpolation state
    this.adaptiveDelay = NETWORK.INTERPOLATION_DELAY_MS;
    this.lastSnapshotTime = 0;
    this.snapshotIntervalAvg = PHYSICS.DT * 1000;
    this.playerBornTimes.clear();
    this.hasReceivedFirstSnapshot = false;
  }
}
