// Protocol types matching api/src/net/protocol.rs
// Uses bincode 2.0 serialization format

import { Vec2 } from '@/utils/Vec2';

// UUID is 16 bytes, represented as hex string on client
export type PlayerId = string;

// Match phases
export type MatchPhase = 'waiting' | 'countdown' | 'playing' | 'ended';

// Client -> Server messages
export type ClientMessage =
  | { type: 'JoinRequest'; playerName: string }
  | { type: 'Input'; input: PlayerInput }
  | { type: 'Leave' }
  | { type: 'Ping'; timestamp: number }
  | { type: 'SnapshotAck'; tick: number };

// Server -> Client messages
export type ServerMessage =
  | { type: 'JoinAccepted'; playerId: PlayerId; sessionToken: Uint8Array }
  | { type: 'JoinRejected'; reason: string }
  | { type: 'Snapshot'; snapshot: GameSnapshot }
  | { type: 'Delta'; delta: DeltaUpdate }
  | { type: 'Event'; event: GameEvent }
  | { type: 'Pong'; clientTimestamp: number; serverTimestamp: number }
  | { type: 'Kicked'; reason: string }
  | { type: 'PhaseChange'; phase: MatchPhase; countdown: number };

// Player input for one tick
export interface PlayerInput {
  sequence: number;
  tick: number;
  thrust: Vec2;
  aim: Vec2;
  boost: boolean;
  fire: boolean;
  fireReleased: boolean;
}

// Gravity well in the arena
export interface GravityWellSnapshot {
  position: Vec2;
  mass: number;
  coreRadius: number;
}

// Full game state snapshot
export interface GameSnapshot {
  tick: number;
  matchPhase: MatchPhase;
  matchTime: number;
  countdown: number;
  players: PlayerSnapshot[];
  projectiles: ProjectileSnapshot[];
  arenaCollapsePhase: number;
  arenaSafeRadius: number;
  arenaScale: number;
  gravityWells: GravityWellSnapshot[];
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
}

// Projectile state in snapshot
export interface ProjectileSnapshot {
  id: number;
  ownerId: PlayerId;
  position: Vec2;
  velocity: Vec2;
  mass: number;
}

// Delta update (incremental changes)
export interface DeltaUpdate {
  tick: number;
  baseTick: number;
  playerUpdates: PlayerDelta[];
  projectileUpdates: ProjectileDelta[];
  removedProjectiles: number[];
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
  | { type: 'ZoneCollapse'; phase: number; newSafeRadius: number };

// Create a default player input
export function createPlayerInput(sequence: number, tick: number): PlayerInput {
  return {
    sequence,
    tick,
    thrust: new Vec2(0, 0),
    aim: new Vec2(1, 0),
    boost: false,
    fire: false,
    fireReleased: false,
  };
}
