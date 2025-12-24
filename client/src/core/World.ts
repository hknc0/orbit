// World state for multiplayer client
// Stores interpolated server state and local player prediction

import { ARENA, MASS, PLAYER_COLORS } from '@/utils/Constants';
import type { PlayerId, MatchPhase } from '@/net/Protocol';
import type { InterpolatedState, InterpolatedPlayer, InterpolatedProjectile, InterpolatedDebris, InterpolatedGravityWell, InterpolatedNotablePlayer } from '@/net/StateSync';

// Arena state
export interface ArenaState {
  coreRadius: number;
  innerRadius: number;
  middleRadius: number;
  outerRadius: number;
  collapsePhase: number;
  isCollapsing: boolean;
  scale: number;
  gravityWells: InterpolatedGravityWell[];
}

// Leaderboard entry
export interface LeaderboardEntry {
  id: PlayerId;
  name: string;
  mass: number;
  kills: number;
}

// Kill effect duration in ms
const KILL_EFFECT_DURATION = 1500;
// Death effect duration in ms
const DEATH_EFFECT_DURATION = 800;
// Collision effect duration in ms
const COLLISION_EFFECT_DURATION = 300;
// Max collision effects at once
const MAX_COLLISION_EFFECTS = 10;
// Gravity wave effect duration in ms
const WAVE_EFFECT_DURATION = 6000; // Waves expand for 6 seconds
// Max wave effects at once
const MAX_WAVE_EFFECTS = 10;
// Wave charging duration in ms
const WAVE_CHARGE_DURATION = 2000;

// Death effect data
interface DeathEffect {
  position: { x: number; y: number };
  timestamp: number;
  color: string;
}

// Collision effect data
interface CollisionEffect {
  position: { x: number; y: number };
  timestamp: number;
  intensity: number;
  color: string;
}

// Gravity wave effect data (expanding ring from well explosion)
interface GravityWaveEffect {
  position: { x: number; y: number };
  timestamp: number;
  strength: number;
  wellIndex: number;
}

// Charging well data (warning before explosion)
interface ChargingWell {
  position: { x: number; y: number };
  timestamp: number;
  wellIndex: number;
}

export class World {
  // Current interpolated state from server
  private state: InterpolatedState | null = null;

  // Local player info
  localPlayerId: PlayerId | null = null;

  // Arena state
  arena: ArenaState = {
    coreRadius: ARENA.CORE_RADIUS,
    innerRadius: ARENA.INNER_RADIUS,
    middleRadius: ARENA.MIDDLE_RADIUS,
    outerRadius: ARENA.OUTER_RADIUS,
    collapsePhase: 0,
    isCollapsing: false,
    scale: 1.0,
    gravityWells: [],
  };

  // Player names (from join events)
  private playerNames: Map<PlayerId, string> = new Map();

  // Recent kills tracking (player id -> timestamp)
  private recentKills: Map<PlayerId, number> = new Map();

  // Previous kill counts to detect new kills
  private lastKillCounts: Map<PlayerId, number> = new Map();

  // Death effects (explosion at death location)
  private deathEffects: DeathEffect[] = [];

  // Collision effects (flash + ring at collision point)
  private collisionEffects: CollisionEffect[] = [];

  // Gravity wave effects (expanding rings from well explosions)
  private gravityWaveEffects: GravityWaveEffect[] = [];

  // Wells currently charging (warning state before explosion)
  private chargingWells: ChargingWell[] = [];

  // Previous alive states to detect deaths
  private lastAliveStates: Map<PlayerId, { alive: boolean; position: { x: number; y: number }; color: string }> = new Map();

  // Session stats tracking
  private sessionStats = {
    bestMass: 0,
    killStreak: 0,
    bestKillStreak: 0,
    lastSpawnTime: Date.now(),
    totalKills: 0,
    totalDeaths: 0,
    bestTimeAlive: 0,
  };

  // Update from interpolated server state
  updateFromState(state: InterpolatedState): void {
    const now = Date.now();

    // Detect new kills and deaths before updating state
    for (const [playerId, player] of state.players) {
      // Detect kills
      const lastKills = this.lastKillCounts.get(playerId) ?? 0;
      if (player.kills > lastKills) {
        // Player got a new kill - record timestamp
        this.recentKills.set(playerId, now);

        // Track local player kill streak
        if (playerId === this.localPlayerId) {
          const killsGained = player.kills - lastKills;
          this.sessionStats.killStreak += killsGained;
          this.sessionStats.totalKills += killsGained;
          if (this.sessionStats.killStreak > this.sessionStats.bestKillStreak) {
            this.sessionStats.bestKillStreak = this.sessionStats.killStreak;
          }
        }
      }
      this.lastKillCounts.set(playerId, player.kills);

      // Detect deaths - player was alive, now dead
      const lastState = this.lastAliveStates.get(playerId);
      if (lastState && lastState.alive && !player.alive) {
        // Player just died - create death effect at their last position
        this.deathEffects.push({
          position: { x: lastState.position.x, y: lastState.position.y },
          timestamp: now,
          color: lastState.color,
        });

        // Reset local player kill streak on death and track best time alive
        if (playerId === this.localPlayerId) {
          const timeAlive = now - this.sessionStats.lastSpawnTime;
          if (timeAlive > this.sessionStats.bestTimeAlive) {
            this.sessionStats.bestTimeAlive = timeAlive;
          }
          this.sessionStats.killStreak = 0;
          this.sessionStats.totalDeaths++;
        }
      }

      // Detect respawn - player was dead, now alive
      if (lastState && !lastState.alive && player.alive && playerId === this.localPlayerId) {
        this.sessionStats.lastSpawnTime = now;
      }

      // Update last alive state for next comparison
      this.lastAliveStates.set(playerId, {
        alive: player.alive,
        position: { x: player.position.x, y: player.position.y },
        color: this.getPlayerColor(player.colorIndex),
      });

      // Track best mass for local player
      if (playerId === this.localPlayerId && player.alive) {
        if (player.mass > this.sessionStats.bestMass) {
          this.sessionStats.bestMass = player.mass;
        }
      }
    }

    // Clean up old kill effects
    for (const [playerId, timestamp] of this.recentKills) {
      if (now - timestamp > KILL_EFFECT_DURATION) {
        this.recentKills.delete(playerId);
      }
    }

    // Clean up old death effects
    this.deathEffects = this.deathEffects.filter(
      (effect) => now - effect.timestamp < DEATH_EFFECT_DURATION
    );

    // Clean up old collision effects
    this.collisionEffects = this.collisionEffects.filter(
      (effect) => now - effect.timestamp < COLLISION_EFFECT_DURATION
    );

    // Clean up tracking for players no longer in state (prevents stale data accumulation)
    const currentPlayerIds = new Set(state.players.keys());
    for (const playerId of this.lastAliveStates.keys()) {
      if (!currentPlayerIds.has(playerId)) {
        this.lastAliveStates.delete(playerId);
        this.lastKillCounts.delete(playerId);
        this.recentKills.delete(playerId);
      }
    }

    this.state = state;

    // Update arena
    this.arena.collapsePhase = state.arenaCollapsePhase;
    this.arena.scale = state.arenaScale;
    this.arena.gravityWells = state.gravityWells;
    // Calculate radii based on collapse phase
    const collapseRatio = state.arenaCollapsePhase / ARENA.COLLAPSE_PHASES;
    this.arena.coreRadius = ARENA.CORE_RADIUS + (ARENA.OUTER_RADIUS - ARENA.CORE_RADIUS) * collapseRatio * 0.5;
  }

  // Set player name (from events)
  setPlayerName(playerId: PlayerId, name: string): void {
    this.playerNames.set(playerId, name);
  }

  // Apply client-side prediction for local player (reduces perceived latency)
  applyLocalPrediction(position: { x: number; y: number }, velocity: { x: number; y: number }): void {
    if (!this.localPlayerId || !this.state) return;

    const localPlayer = this.state.players.get(this.localPlayerId);
    if (localPlayer && localPlayer.alive) {
      localPlayer.position.x = position.x;
      localPlayer.position.y = position.y;
      localPlayer.velocity.x = velocity.x;
      localPlayer.velocity.y = velocity.y;
    }
  }

  // Get all players
  getPlayers(): Map<PlayerId, InterpolatedPlayer> {
    return this.state?.players ?? new Map();
  }

  // Get a specific player
  getPlayer(id: PlayerId): InterpolatedPlayer | undefined {
    return this.state?.players.get(id);
  }

  // Get local player
  getLocalPlayer(): InterpolatedPlayer | undefined {
    if (!this.localPlayerId) return undefined;
    return this.state?.players.get(this.localPlayerId);
  }

  // Get all projectiles
  getProjectiles(): Map<number, InterpolatedProjectile> {
    return this.state?.projectiles ?? new Map();
  }

  // Get all debris
  getDebris(): Map<number, InterpolatedDebris> {
    return this.state?.debris ?? new Map();
  }

  // Get match phase
  getMatchPhase(): MatchPhase {
    return this.state?.matchPhase ?? 'waiting';
  }

  // Get match time
  getMatchTime(): number {
    return this.state?.matchTime ?? 0;
  }

  // Get current tick
  getTick(): number {
    return this.state?.tick ?? 0;
  }

  // Get arena safe radius
  getArenaSafeRadius(): number {
    return this.state?.arenaSafeRadius ?? ARENA.OUTER_RADIUS;
  }

  // Calculate radius from mass
  massToRadius(mass: number): number {
    return Math.sqrt(mass) * MASS.RADIUS_SCALE;
  }

  // Get player color
  getPlayerColor(colorIndex: number): string {
    return PLAYER_COLORS[colorIndex % PLAYER_COLORS.length];
  }

  // Get player name
  getPlayerName(playerId: PlayerId): string {
    // Get name from player snapshot
    const player = this.getPlayer(playerId);
    if (player?.name) return player.name;

    // Fallback to cached names
    const cachedName = this.playerNames.get(playerId);
    if (cachedName) return cachedName;

    return `Player ${playerId.substring(0, 4)}`;
  }

  // Get alive player count (uses server's total, not AOI-filtered count)
  getAlivePlayerCount(): number {
    // Use totalAlive from server state (accurate count before AOI filtering)
    if (this.state?.totalAlive !== undefined && this.state.totalAlive > 0) {
      return this.state.totalAlive;
    }
    // Fallback to counting local players (AOI filtered)
    let count = 0;
    for (const player of this.getPlayers().values()) {
      if (player.alive) count++;
    }
    return count;
  }

  // Get total player count (uses server's total, not AOI-filtered count)
  getTotalPlayerCount(): number {
    if (this.state?.totalPlayers !== undefined && this.state.totalPlayers > 0) {
      return this.state.totalPlayers;
    }
    return this.getPlayers().size;
  }

  // Get density grid for minimap heatmap (16x16 grid of player counts)
  getDensityGrid(): number[] {
    return this.state?.densityGrid ?? [];
  }

  // Get notable players for minimap radar (high-mass players visible everywhere)
  getNotablePlayers(): InterpolatedNotablePlayer[] {
    return this.state?.notablePlayers ?? [];
  }

  // Get player placement (rank by mass)
  getPlayerPlacement(playerId: PlayerId): number {
    const players = Array.from(this.getPlayers().values())
      .filter((p) => p.alive)
      .sort((a, b) => b.mass - a.mass);

    const index = players.findIndex((p) => p.id === playerId);
    return index >= 0 ? index + 1 : players.length + 1;
  }

  // Get leaderboard
  getLeaderboard(): LeaderboardEntry[] {
    return Array.from(this.getPlayers().values())
      .filter((p) => p.alive)
      .map((p) => ({
        id: p.id,
        name: this.getPlayerName(p.id),
        mass: p.mass,
        kills: p.kills,
      }))
      .sort((a, b) => b.mass - a.mass);
  }

  // Check if local player is alive
  isLocalPlayerAlive(): boolean {
    const player = this.getLocalPlayer();
    return player?.alive ?? false;
  }

  // Get kill effect progress (0-1, 1 = just killed, 0 = effect ended)
  getKillEffectProgress(playerId: PlayerId): number {
    const timestamp = this.recentKills.get(playerId);
    if (!timestamp) return 0;
    const elapsed = Date.now() - timestamp;
    if (elapsed >= KILL_EFFECT_DURATION) return 0;
    return 1 - elapsed / KILL_EFFECT_DURATION;
  }

  // Get active death effects for rendering
  getDeathEffects(): Array<{ position: { x: number; y: number }; progress: number; color: string }> {
    const now = Date.now();
    return this.deathEffects.map((effect) => ({
      position: effect.position,
      progress: 1 - (now - effect.timestamp) / DEATH_EFFECT_DURATION,
      color: effect.color,
    }));
  }

  // Add a collision effect (called when PlayerDeflection event received)
  addCollisionEffect(
    position: { x: number; y: number },
    intensity: number,
    color: string
  ): void {
    // Enforce max effects limit
    if (this.collisionEffects.length >= MAX_COLLISION_EFFECTS) {
      this.collisionEffects.shift(); // Remove oldest
    }
    this.collisionEffects.push({
      position: { x: position.x, y: position.y },
      timestamp: Date.now(),
      intensity,
      color,
    });
  }

  // Get active collision effects for rendering
  getCollisionEffects(): Array<{
    position: { x: number; y: number };
    progress: number;
    intensity: number;
    color: string;
  }> {
    const now = Date.now();
    return this.collisionEffects.map((effect) => ({
      position: effect.position,
      progress: 1 - (now - effect.timestamp) / COLLISION_EFFECT_DURATION,
      intensity: effect.intensity,
      color: effect.color,
    }));
  }

  // Add a charging well effect (called when GravityWellCharging event received)
  addChargingWell(position: { x: number; y: number }, wellIndex: number): void {
    // Remove any existing charging for this well
    this.chargingWells = this.chargingWells.filter((w) => w.wellIndex !== wellIndex);
    this.chargingWells.push({
      position: { x: position.x, y: position.y },
      timestamp: Date.now(),
      wellIndex,
    });
  }

  // Add a gravity wave effect (called when GravityWaveExplosion event received)
  addGravityWaveEffect(
    position: { x: number; y: number },
    strength: number,
    wellIndex: number
  ): void {
    // Remove charging state for this well
    this.chargingWells = this.chargingWells.filter((w) => w.wellIndex !== wellIndex);

    // Enforce max effects limit
    if (this.gravityWaveEffects.length >= MAX_WAVE_EFFECTS) {
      this.gravityWaveEffects.shift(); // Remove oldest
    }
    this.gravityWaveEffects.push({
      position: { x: position.x, y: position.y },
      timestamp: Date.now(),
      strength,
      wellIndex,
    });
  }

  // Get active gravity wave effects for rendering
  getGravityWaveEffects(): Array<{
    position: { x: number; y: number };
    progress: number; // 0 = just started, 1 = fully expanded
    strength: number;
  }> {
    const now = Date.now();
    return this.gravityWaveEffects
      .filter((effect) => now - effect.timestamp < WAVE_EFFECT_DURATION)
      .map((effect) => ({
        position: effect.position,
        progress: (now - effect.timestamp) / WAVE_EFFECT_DURATION,
        strength: effect.strength,
      }));
  }

  // Get charging wells for rendering warning effect
  getChargingWells(): Array<{
    position: { x: number; y: number };
    progress: number; // 0 = just started charging, 1 = about to explode
    wellIndex: number;
  }> {
    const now = Date.now();
    return this.chargingWells
      .filter((well) => now - well.timestamp < WAVE_CHARGE_DURATION)
      .map((well) => ({
        position: well.position,
        progress: (now - well.timestamp) / WAVE_CHARGE_DURATION,
        wellIndex: well.wellIndex,
      }));
  }

  // Get session stats for HUD
  getSessionStats() {
    return {
      ...this.sessionStats,
      timeAlive: this.isLocalPlayerAlive() ? Date.now() - this.sessionStats.lastSpawnTime : 0,
    };
  }

  // Reset world state
  reset(): void {
    this.state = null;
    this.localPlayerId = null;
    this.playerNames.clear();
    this.recentKills.clear();
    this.lastKillCounts.clear();
    this.deathEffects = [];
    this.collisionEffects = [];
    this.gravityWaveEffects = [];
    this.chargingWells = [];
    this.lastAliveStates.clear();
    this.sessionStats = {
      bestMass: 0,
      killStreak: 0,
      bestKillStreak: 0,
      lastSpawnTime: Date.now(),
      totalKills: 0,
      totalDeaths: 0,
      bestTimeAlive: 0,
    };
    this.arena = {
      coreRadius: ARENA.CORE_RADIUS,
      innerRadius: ARENA.INNER_RADIUS,
      middleRadius: ARENA.MIDDLE_RADIUS,
      outerRadius: ARENA.OUTER_RADIUS,
      collapsePhase: 0,
      isCollapsing: false,
      scale: 1.0,
      gravityWells: [],
    };
  }
}
