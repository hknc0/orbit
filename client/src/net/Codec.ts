// Bincode 2.0 compatible codec for protocol messages
// Server uses bincode with legacy config (little-endian, fixed-size integers)

import { Vec2 } from '@/utils/Vec2';
import type {
  ClientMessage,
  ServerMessage,
  PlayerInput,
  GameSnapshot,
  DeltaUpdate,
  GameEvent,
  PlayerSnapshot,
  ProjectileSnapshot,
  DebrisSnapshot,
  PlayerDelta,
  ProjectileDelta,
  MatchPhase,
  GravityWellSnapshot,
  NotablePlayer,
} from './Protocol';

// Binary writer for encoding messages
class BinaryWriter {
  private buffer: ArrayBuffer;
  private view: DataView;
  private offset: number = 0;

  constructor(initialSize: number = 256) {
    this.buffer = new ArrayBuffer(initialSize);
    this.view = new DataView(this.buffer);
  }

  private ensureCapacity(bytes: number): void {
    if (this.offset + bytes > this.buffer.byteLength) {
      const newSize = Math.max(this.buffer.byteLength * 2, this.offset + bytes);
      const newBuffer = new ArrayBuffer(newSize);
      new Uint8Array(newBuffer).set(new Uint8Array(this.buffer));
      this.buffer = newBuffer;
      this.view = new DataView(this.buffer);
    }
  }

  writeU8(value: number): void {
    this.ensureCapacity(1);
    this.view.setUint8(this.offset++, value);
  }

  writeU32(value: number): void {
    this.ensureCapacity(4);
    this.view.setUint32(this.offset, value, true);
    this.offset += 4;
  }

  writeU64(value: number): void {
    this.ensureCapacity(8);
    // JavaScript doesn't have native 64-bit integers, use BigInt
    this.view.setBigUint64(this.offset, BigInt(value), true);
    this.offset += 8;
  }

  writeF32(value: number): void {
    this.ensureCapacity(4);
    this.view.setFloat32(this.offset, value, true);
    this.offset += 4;
  }

  writeBool(value: boolean): void {
    this.writeU8(value ? 1 : 0);
  }

  writeString(value: string): void {
    const bytes = new TextEncoder().encode(value);
    this.writeU64(bytes.length);
    this.ensureCapacity(bytes.length);
    new Uint8Array(this.buffer, this.offset).set(bytes);
    this.offset += bytes.length;
  }

  writeVec2(v: Vec2): void {
    this.writeF32(v.x);
    this.writeF32(v.y);
  }

  writeUuid(uuid: string): void {
    // bincode with legacy config serializes UUID with a length prefix (as if it were Vec<u8>)
    this.writeU64(16); // Length prefix
    // Parse UUID string to 16 bytes
    const hex = uuid.replace(/-/g, '');
    for (let i = 0; i < 16; i++) {
      this.writeU8(parseInt(hex.substring(i * 2, i * 2 + 2), 16));
    }
  }

  getBytes(): Uint8Array {
    return new Uint8Array(this.buffer, 0, this.offset);
  }
}

// Binary reader for decoding messages
class BinaryReader {
  private view: DataView;
  private offset: number = 0;

  constructor(buffer: ArrayBuffer) {
    this.view = new DataView(buffer);
  }

  readU8(): number {
    return this.view.getUint8(this.offset++);
  }

  readU32(): number {
    const value = this.view.getUint32(this.offset, true);
    this.offset += 4;
    return value;
  }

  readU64(): number {
    const value = this.view.getBigUint64(this.offset, true);
    this.offset += 8;
    return Number(value);
  }

  readF32(): number {
    const value = this.view.getFloat32(this.offset, true);
    this.offset += 4;
    return value;
  }

  readBool(): boolean {
    return this.readU8() !== 0;
  }

  readString(): string {
    const length = this.readU64();
    const bytes = new Uint8Array(this.view.buffer, this.offset, length);
    this.offset += length;
    return new TextDecoder().decode(bytes);
  }

  readVec2(): Vec2 {
    return new Vec2(this.readF32(), this.readF32());
  }

  readUuid(): string {
    // bincode with legacy config serializes UUID with a length prefix (as if it were Vec<u8>)
    const length = this.readU64();
    if (length !== 16) {
      throw new Error(`Invalid UUID length: expected 16, got ${length}`);
    }

    const bytes: string[] = [];
    for (let i = 0; i < 16; i++) {
      bytes.push(this.readU8().toString(16).padStart(2, '0'));
    }
    // Format as UUID: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
    return `${bytes.slice(0, 4).join('')}-${bytes.slice(4, 6).join('')}-${bytes.slice(6, 8).join('')}-${bytes.slice(8, 10).join('')}-${bytes.slice(10, 16).join('')}`;
  }

  readBytes(length: number): Uint8Array {
    const bytes = new Uint8Array(this.view.buffer, this.offset, length);
    this.offset += length;
    return bytes.slice(); // Return copy
  }

  readByteArray(): Uint8Array {
    const length = this.readU64();
    return this.readBytes(length);
  }

  get remaining(): number {
    return this.view.byteLength - this.offset;
  }
}

// Encode client message to binary
export function encodeClientMessage(msg: ClientMessage): Uint8Array {
  const writer = new BinaryWriter();

  switch (msg.type) {
    case 'JoinRequest':
      writer.writeU32(0); // Enum variant
      writer.writeString(msg.playerName);
      writer.writeU8(msg.colorIndex);
      writer.writeBool(msg.isSpectator);
      break;
    case 'Input':
      writer.writeU32(1);
      writePlayerInput(writer, msg.input);
      break;
    case 'Leave':
      writer.writeU32(2);
      break;
    case 'Ping':
      writer.writeU32(3);
      writer.writeU64(msg.timestamp);
      break;
    case 'SnapshotAck':
      writer.writeU32(4);
      writer.writeU64(msg.tick);
      break;
    case 'SpectateTarget':
      writer.writeU32(5);
      // Option<PlayerId> - write 0 for None, 1+UUID for Some
      if (msg.targetId === null) {
        writer.writeU8(0); // None
      } else {
        writer.writeU8(1); // Some
        writer.writeUuid(msg.targetId);
      }
      break;
    case 'SwitchToPlayer':
      writer.writeU32(6);
      writer.writeU8(msg.colorIndex);
      break;
    case 'ViewportInfo':
      writer.writeU32(7);
      writer.writeF32(msg.zoom);
      break;
  }

  return writer.getBytes();
}

function writePlayerInput(writer: BinaryWriter, input: PlayerInput): void {
  writer.writeU64(input.sequence);
  writer.writeU64(input.tick);
  writer.writeU64(input.clientTime); // For RTT measurement
  writer.writeVec2(input.thrust);
  writer.writeVec2(input.aim);
  writer.writeBool(input.boost);
  writer.writeBool(input.fire);
  writer.writeBool(input.fireReleased);
}

// Decode server message from binary
export function decodeServerMessage(data: ArrayBuffer): ServerMessage {
  const reader = new BinaryReader(data);
  const variant = reader.readU32();

  switch (variant) {
    case 0: // JoinAccepted
      return {
        type: 'JoinAccepted',
        playerId: reader.readUuid(),
        sessionToken: reader.readByteArray(),
        isSpectator: reader.readBool(),
      };
    case 1: // JoinRejected
      return {
        type: 'JoinRejected',
        reason: reader.readString(),
      };
    case 2: // Snapshot
      return {
        type: 'Snapshot',
        snapshot: readGameSnapshot(reader),
      };
    case 3: // Delta
      return {
        type: 'Delta',
        delta: readDeltaUpdate(reader),
      };
    case 4: // Event
      return {
        type: 'Event',
        event: readGameEvent(reader),
      };
    case 5: // Pong
      return {
        type: 'Pong',
        clientTimestamp: reader.readU64(),
        serverTimestamp: reader.readU64(),
      };
    case 6: // Kicked
      return {
        type: 'Kicked',
        reason: reader.readString(),
      };
    case 7: // PhaseChange
      return {
        type: 'PhaseChange',
        phase: readMatchPhase(reader),
        countdown: reader.readF32(),
      };
    case 8: // SpectatorModeChanged
      return {
        type: 'SpectatorModeChanged',
        isSpectator: reader.readBool(),
      };
    default:
      throw new Error(`Unknown server message variant: ${variant}`);
  }
}

function readMatchPhase(reader: BinaryReader): MatchPhase {
  const variant = reader.readU32();
  switch (variant) {
    case 0: return 'waiting';
    case 1: return 'countdown';
    case 2: return 'playing';
    case 3: return 'ended';
    default: return 'waiting';
  }
}

function readGameSnapshot(reader: BinaryReader): GameSnapshot {
  const tick = reader.readU64();
  const matchPhase = readMatchPhase(reader);
  const matchTime = reader.readF32();
  const countdown = reader.readF32();

  const playerCount = reader.readU64();
  const players: PlayerSnapshot[] = [];
  for (let i = 0; i < playerCount; i++) {
    players.push(readPlayerSnapshot(reader));
  }

  const projectileCount = reader.readU64();
  const projectiles: ProjectileSnapshot[] = [];
  for (let i = 0; i < projectileCount; i++) {
    projectiles.push(readProjectileSnapshot(reader));
  }

  const debrisCount = reader.readU64();
  const debris: DebrisSnapshot[] = [];
  for (let i = 0; i < debrisCount; i++) {
    debris.push(readDebrisSnapshot(reader));
  }

  const arenaCollapsePhase = reader.readU8();
  const arenaSafeRadius = reader.readF32();
  const arenaScale = reader.readF32();

  const wellCount = reader.readU64();
  const gravityWells: GravityWellSnapshot[] = [];
  for (let i = 0; i < wellCount; i++) {
    gravityWells.push({
      id: reader.readU32(),
      position: reader.readVec2(),
      mass: reader.readF32(),
      coreRadius: reader.readF32(),
    });
  }

  // Read total player counts (for UI display with AOI filtering)
  const totalPlayers = reader.readU32();
  const totalAlive = reader.readU32();

  // Read density grid for minimap heatmap (16x16 = 256 u8 values)
  const densityGridLen = reader.readU64();
  const densityGrid: number[] = [];
  for (let i = 0; i < densityGridLen; i++) {
    densityGrid.push(reader.readU8());
  }

  // Read notable players for minimap radar
  const notableCount = reader.readU64();
  const notablePlayers: NotablePlayer[] = [];
  for (let i = 0; i < notableCount; i++) {
    notablePlayers.push({
      id: reader.readUuid(),
      position: reader.readVec2(),
      mass: reader.readF32(),
      colorIndex: reader.readU8(),
    });
  }

  // Read echo_client_time for RTT measurement
  const echoClientTime = reader.readU64();

  return {
    tick,
    matchPhase,
    matchTime,
    countdown,
    players,
    projectiles,
    debris,
    arenaCollapsePhase,
    arenaSafeRadius,
    arenaScale,
    gravityWells,
    totalPlayers,
    totalAlive,
    densityGrid,
    notablePlayers,
    echoClientTime,
  };
}

// Player flags bit positions (must match server protocol.rs)
const PLAYER_FLAG_ALIVE = 0b0000_0001;
const PLAYER_FLAG_SPAWN_PROTECTION = 0b0000_0010;
const PLAYER_FLAG_IS_BOT = 0b0000_0100;

function readPlayerSnapshot(reader: BinaryReader): PlayerSnapshot {
  const id = reader.readUuid();
  const name = reader.readString();
  const position = reader.readVec2();
  const velocity = reader.readVec2();
  const rotation = reader.readF32();
  const mass = reader.readF32();
  // OPTIMIZATION: Bit-packed flags (saves 2 bytes per player)
  const flags = reader.readU8();
  const alive = (flags & PLAYER_FLAG_ALIVE) !== 0;
  const spawnProtection = (flags & PLAYER_FLAG_SPAWN_PROTECTION) !== 0;
  const isBot = (flags & PLAYER_FLAG_IS_BOT) !== 0;
  const kills = reader.readU32();
  const deaths = reader.readU32();
  const colorIndex = reader.readU8();

  return {
    id,
    name,
    position,
    velocity,
    rotation,
    mass,
    alive,
    kills,
    deaths,
    spawnProtection,
    isBot,
    colorIndex,
  };
}

function readProjectileSnapshot(reader: BinaryReader): ProjectileSnapshot {
  return {
    id: reader.readU64(),
    ownerId: reader.readUuid(),
    position: reader.readVec2(),
    velocity: reader.readVec2(),
    mass: reader.readF32(),
  };
}

function readDebrisSnapshot(reader: BinaryReader): DebrisSnapshot {
  return {
    id: reader.readU64(),
    position: reader.readVec2(),
    size: reader.readU8(),
  };
}

function readDeltaUpdate(reader: BinaryReader): DeltaUpdate {
  const tick = reader.readU64();
  const baseTick = reader.readU64();

  const playerUpdateCount = reader.readU64();
  const playerUpdates: PlayerDelta[] = [];
  for (let i = 0; i < playerUpdateCount; i++) {
    playerUpdates.push(readPlayerDelta(reader));
  }

  const projectileUpdateCount = reader.readU64();
  const projectileUpdates: ProjectileDelta[] = [];
  for (let i = 0; i < projectileUpdateCount; i++) {
    projectileUpdates.push(readProjectileDelta(reader));
  }

  const removedCount = reader.readU64();
  const removedProjectiles: number[] = [];
  for (let i = 0; i < removedCount; i++) {
    removedProjectiles.push(reader.readU64());
  }

  // Read full debris list (debris moves slowly, sent in full)
  const debrisCount = reader.readU64();
  const debris: DebrisSnapshot[] = [];
  for (let i = 0; i < debrisCount; i++) {
    debris.push(readDebrisSnapshot(reader));
  }

  return {
    tick,
    baseTick,
    playerUpdates,
    projectileUpdates,
    removedProjectiles,
    debris,
  };
}

function readPlayerDelta(reader: BinaryReader): PlayerDelta {
  const id = reader.readUuid();
  const delta: PlayerDelta = { id };

  // Read Option<Vec2> for position
  if (reader.readBool()) {
    delta.position = reader.readVec2();
  }
  if (reader.readBool()) {
    delta.velocity = reader.readVec2();
  }
  if (reader.readBool()) {
    delta.rotation = reader.readF32();
  }
  if (reader.readBool()) {
    delta.mass = reader.readF32();
  }
  if (reader.readBool()) {
    delta.alive = reader.readBool();
  }
  if (reader.readBool()) {
    delta.kills = reader.readU32();
  }

  return delta;
}

function readProjectileDelta(reader: BinaryReader): ProjectileDelta {
  return {
    id: reader.readU64(),
    position: reader.readVec2(),
    velocity: reader.readVec2(),
  };
}

function readGameEvent(reader: BinaryReader): GameEvent {
  const variant = reader.readU32();

  switch (variant) {
    case 0: // PlayerKilled
      return {
        type: 'PlayerKilled',
        killerId: reader.readUuid(),
        victimId: reader.readUuid(),
        killerName: reader.readString(),
        victimName: reader.readString(),
      };
    case 1: // PlayerJoined
      return {
        type: 'PlayerJoined',
        playerId: reader.readUuid(),
        name: reader.readString(),
      };
    case 2: // PlayerLeft
      return {
        type: 'PlayerLeft',
        playerId: reader.readUuid(),
        name: reader.readString(),
      };
    case 3: // MatchStarted
      return { type: 'MatchStarted' };
    case 4: // MatchEnded
      const hasWinner = reader.readBool();
      return {
        type: 'MatchEnded',
        winnerId: hasWinner ? reader.readUuid() : null,
        winnerName: hasWinner ? reader.readString() : null,
      };
    case 5: // ZoneCollapse
      return {
        type: 'ZoneCollapse',
        phase: reader.readU8(),
        newSafeRadius: reader.readF32(),
      };
    case 6: // PlayerDeflection
      return {
        type: 'PlayerDeflection',
        playerA: reader.readUuid(),
        playerB: reader.readUuid(),
        position: { x: reader.readF32(), y: reader.readF32() },
        intensity: reader.readF32(),
      };
    case 7: // GravityWellCharging
      return {
        type: 'GravityWellCharging',
        wellId: reader.readU32(),
        position: { x: reader.readF32(), y: reader.readF32() },
      };
    case 8: // GravityWaveExplosion
      return {
        type: 'GravityWaveExplosion',
        wellId: reader.readU32(),
        position: { x: reader.readF32(), y: reader.readF32() },
        strength: reader.readF32(),
      };
    case 9: // GravityWellDestroyed
      return {
        type: 'GravityWellDestroyed',
        wellId: reader.readU32(),
        position: { x: reader.readF32(), y: reader.readF32() },
      };
    default:
      throw new Error(`Unknown game event variant: ${variant}`);
  }
}
