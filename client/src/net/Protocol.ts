// Protocol types matching api/src/net/protocol.rs
// Uses bincode 2.0 serialization format

import { Vec2 } from '@/utils/Vec2';

// UUID is 16 bytes, represented as hex string on client
export type PlayerId = string;

// Match phases
export type MatchPhase = 'waiting' | 'countdown' | 'playing' | 'ended';

// Client -> Server messages
export type ClientMessage =
  | { type: 'JoinRequest'; playerName: string; colorIndex: number; isSpectator: boolean }
  | { type: 'Input'; input: PlayerInput }
  | { type: 'Leave' }
  | { type: 'Ping'; timestamp: number }
  | { type: 'SnapshotAck'; tick: number }
  | { type: 'SpectateTarget'; targetId: PlayerId | null }
  | { type: 'SwitchToPlayer'; colorIndex: number }
  | { type: 'ViewportInfo'; zoom: number };

// Server -> Client messages
export type ServerMessage =
  | { type: 'JoinAccepted'; playerId: PlayerId; sessionToken: Uint8Array; isSpectator: boolean }
  | { type: 'JoinRejected'; reason: string }
  | { type: 'Snapshot'; snapshot: GameSnapshot }
  | { type: 'Delta'; delta: DeltaUpdate }
  | { type: 'Event'; event: GameEvent }
  | { type: 'Pong'; clientTimestamp: number; serverTimestamp: number }
  | { type: 'Kicked'; reason: string }
  | { type: 'PhaseChange'; phase: MatchPhase; countdown: number }
  | { type: 'SpectatorModeChanged'; isSpectator: boolean };

// Player input for one tick
export interface PlayerInput {
  sequence: number;
  tick: number;
  clientTime: number; // Client timestamp for RTT measurement
  thrust: Vec2;
  aim: Vec2;
  boost: boolean;
  fire: boolean;
  fireReleased: boolean;
}

// Gravity well in the arena
export interface GravityWellSnapshot {
  id: number; // Unique stable well ID
  position: Vec2;
  mass: number;
  coreRadius: number;
}

// Density grid size (must match server DENSITY_GRID_SIZE)
export const DENSITY_GRID_SIZE = 16;

// Full game state snapshot
export interface GameSnapshot {
  tick: number;
  matchPhase: MatchPhase;
  matchTime: number;
  countdown: number;
  players: PlayerSnapshot[];
  projectiles: ProjectileSnapshot[];
  debris: DebrisSnapshot[]; // Collectible particles
  arenaCollapsePhase: number;
  arenaSafeRadius: number;
  arenaScale: number;
  gravityWells: GravityWellSnapshot[];
  totalPlayers: number;  // Total players in match (before AOI filtering)
  totalAlive: number;    // Total alive players (before AOI filtering)
  densityGrid: number[]; // 16x16 grid of player counts for minimap heatmap
  notablePlayers: NotablePlayer[]; // High-mass players for minimap radar
  echoClientTime: number; // Echo of client's last input timestamp for RTT
  aiStatus?: AIStatusSnapshot; // AI Manager status (if enabled)
}

// AI Manager status for in-game display
export interface AIStatusSnapshot {
  enabled: boolean;
  lastDecision?: string;
  confidence: number; // 0-100
  successRate: number; // 0-100
  decisionsTotal: number;
  decisionsSuccessful: number;
}

// Notable player for minimap radar (high-mass players visible everywhere)
export interface NotablePlayer {
  id: PlayerId;
  position: Vec2;
  mass: number;
  colorIndex: number;
}

// Player state in snapshot
export interface PlayerSnapshot {
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
  /** Tick when player spawned/respawned (for birth animation detection) */
  spawnTick: number;
}

// Projectile state in snapshot
export interface ProjectileSnapshot {
  id: number;
  ownerId: PlayerId;
  position: Vec2;
  velocity: Vec2;
  mass: number;
}

// Debris (collectible particle) state in snapshot
export interface DebrisSnapshot {
  id: number;
  position: Vec2;
  size: number; // 0=Small, 1=Medium, 2=Large
}

// Delta update (incremental changes)
export interface DeltaUpdate {
  tick: number;
  baseTick: number;
  playerUpdates: PlayerDelta[];
  projectileUpdates: ProjectileDelta[];
  removedProjectiles: number[];
  debris: DebrisSnapshot[]; // Full debris list (debris moves slowly)
}

// Delta for a single player
export interface PlayerDelta {
  id: PlayerId;
  position?: Vec2;
  velocity?: Vec2;
  rotation?: number;
  mass?: number;
  alive?: boolean;
  kills?: number;
}

// Delta for a projectile
export interface ProjectileDelta {
  id: number;
  position: Vec2;
  velocity: Vec2;
}

// Game events
export type GameEvent =
  | {
      type: 'PlayerKilled';
      killerId: PlayerId;
      victimId: PlayerId;
      killerName: string;
      victimName: string;
    }
  | { type: 'PlayerJoined'; playerId: PlayerId; name: string }
  | { type: 'PlayerLeft'; playerId: PlayerId; name: string }
  | { type: 'MatchStarted' }
  | { type: 'MatchEnded'; winnerId: PlayerId | null; winnerName: string | null }
  | { type: 'ZoneCollapse'; phase: number; newSafeRadius: number }
  | {
      type: 'PlayerDeflection';
      playerA: PlayerId;
      playerB: PlayerId;
      position: { x: number; y: number };
      intensity: number;
    }
  | {
      type: 'GravityWellCharging';
      wellId: number;
      position: { x: number; y: number };
    }
  | {
      type: 'GravityWaveExplosion';
      wellId: number;
      position: { x: number; y: number };
      strength: number;
    }
  | {
      type: 'GravityWellDestroyed';
      wellId: number;
      position: { x: number; y: number };
    };

// Create a default player input
export function createPlayerInput(sequence: number, tick: number): PlayerInput {
  return {
    sequence,
    tick,
    clientTime: Math.floor(performance.now()), // For RTT measurement
    thrust: new Vec2(0, 0),
    aim: new Vec2(1, 0),
    boost: false,
    fire: false,
    fireReleased: false,
  };
}
