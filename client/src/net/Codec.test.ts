import { describe, it, expect } from 'vitest';
import { encodeClientMessage, decodeServerMessage } from './Codec';
import { Vec2 } from '@/utils/Vec2';
import type { ClientMessage, PlayerInput } from './Protocol';

describe('Codec', () => {
  describe('BinaryWriter / BinaryReader (via encodeClientMessage)', () => {
    describe('JoinRequest encoding', () => {
      it('should encode JoinRequest with normal data', () => {
        const msg: ClientMessage = {
          type: 'JoinRequest',
          playerName: 'TestPlayer',
          colorIndex: 5,
          isSpectator: false,
        };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
        expect(bytes.length).toBeGreaterThan(0);
      });

      it('should encode JoinRequest as spectator', () => {
        const msg: ClientMessage = {
          type: 'JoinRequest',
          playerName: 'Spectator',
          colorIndex: 0,
          isSpectator: true,
        };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
      });

      it('should encode JoinRequest with empty name', () => {
        const msg: ClientMessage = {
          type: 'JoinRequest',
          playerName: '',
          colorIndex: 0,
          isSpectator: false,
        };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
      });

      it('should encode JoinRequest with unicode name', () => {
        const msg: ClientMessage = {
          type: 'JoinRequest',
          playerName: '日本語プレイヤー',
          colorIndex: 10,
          isSpectator: false,
        };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
      });

      it('should encode JoinRequest with emoji name', () => {
        const msg: ClientMessage = {
          type: 'JoinRequest',
          playerName: '🎮Player🚀',
          colorIndex: 15,
          isSpectator: false,
        };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
      });

      it('should handle all valid color indices (0-19)', () => {
        for (let i = 0; i < 20; i++) {
          const msg: ClientMessage = {
            type: 'JoinRequest',
            playerName: 'Test',
            colorIndex: i,
            isSpectator: false,
          };
          const bytes = encodeClientMessage(msg);
          expect(bytes).toBeInstanceOf(Uint8Array);
        }
      });
    });

    describe('Input encoding', () => {
      it('should encode Input with default values', () => {
        const input: PlayerInput = {
          sequence: 1,
          tick: 100,
          clientTime: 12345,
          thrust: new Vec2(0, 0),
          aim: new Vec2(1, 0),
          boost: false,
          fire: false,
          fireReleased: false,
        };
        const msg: ClientMessage = { type: 'Input', input };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
      });

      it('should encode Input with boost and fire', () => {
        const input: PlayerInput = {
          sequence: 42,
          tick: 500,
          clientTime: 99999,
          thrust: new Vec2(1, 0),
          aim: new Vec2(0.707, 0.707),
          boost: true,
          fire: true,
          fireReleased: true,
        };
        const msg: ClientMessage = { type: 'Input', input };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
      });

      it('should encode Input with negative Vec2 values', () => {
        const input: PlayerInput = {
          sequence: 1,
          tick: 1,
          clientTime: 1,
          thrust: new Vec2(-1, -1),
          aim: new Vec2(-0.5, 0.866),
          boost: false,
          fire: false,
          fireReleased: false,
        };
        const msg: ClientMessage = { type: 'Input', input };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
      });

      it('should encode Input with large sequence number', () => {
        const input: PlayerInput = {
          sequence: Number.MAX_SAFE_INTEGER,
          tick: Number.MAX_SAFE_INTEGER,
          clientTime: Number.MAX_SAFE_INTEGER,
          thrust: new Vec2(0, 0),
          aim: new Vec2(1, 0),
          boost: false,
          fire: false,
          fireReleased: false,
        };
        const msg: ClientMessage = { type: 'Input', input };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
      });
    });

    describe('Leave encoding', () => {
      it('should encode Leave message', () => {
        const msg: ClientMessage = { type: 'Leave' };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
        // Leave is just the variant (U32 = 4 bytes)
        expect(bytes.length).toBe(4);
      });
    });

    describe('Ping encoding', () => {
      it('should encode Ping with timestamp', () => {
        const msg: ClientMessage = { type: 'Ping', timestamp: 123456789 };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
        // Variant (4) + U64 (8) = 12 bytes
        expect(bytes.length).toBe(12);
      });

      it('should encode Ping with zero timestamp', () => {
        const msg: ClientMessage = { type: 'Ping', timestamp: 0 };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
      });
    });

    describe('SnapshotAck encoding', () => {
      it('should encode SnapshotAck', () => {
        const msg: ClientMessage = { type: 'SnapshotAck', tick: 500 };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
        // Variant (4) + U64 (8) = 12 bytes
        expect(bytes.length).toBe(12);
      });
    });

    describe('SpectateTarget encoding', () => {
      it('should encode SpectateTarget with null target', () => {
        const msg: ClientMessage = { type: 'SpectateTarget', targetId: null };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
        // Variant (4) + Option tag (1) = 5 bytes for None
        expect(bytes.length).toBe(5);
      });

      it('should encode SpectateTarget with UUID target', () => {
        const msg: ClientMessage = {
          type: 'SpectateTarget',
          targetId: '12345678-1234-1234-1234-123456789abc',
        };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
        // Variant (4) + Option tag (1) + UUID (8 length prefix + 16 bytes) = 29 bytes
        expect(bytes.length).toBe(29);
      });
    });

    describe('SwitchToPlayer encoding', () => {
      it('should encode SwitchToPlayer', () => {
        const msg: ClientMessage = { type: 'SwitchToPlayer', colorIndex: 7 };
        const bytes = encodeClientMessage(msg);
        expect(bytes).toBeInstanceOf(Uint8Array);
        // Variant (4) + U8 (1) = 5 bytes
        expect(bytes.length).toBe(5);
      });
    });
  });

  describe('decodeServerMessage', () => {
    describe('JoinAccepted decoding', () => {
      it('should decode JoinAccepted message', () => {
        // Build a valid JoinAccepted binary:
        // Variant=0 (U32), UUID (length + 16 bytes), SessionToken (length + bytes), isSpectator (bool)
        const writer = new TestBinaryWriter();
        writer.writeU32(0); // JoinAccepted variant
        writer.writeUuid('12345678-1234-5678-1234-567812345678');
        writer.writeByteArray(new Uint8Array([1, 2, 3, 4])); // session token
        writer.writeBool(false);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('JoinAccepted');
        if (result.type === 'JoinAccepted') {
          expect(result.playerId).toBe('12345678-1234-5678-1234-567812345678');
          expect(result.sessionToken).toEqual(new Uint8Array([1, 2, 3, 4]));
          expect(result.isSpectator).toBe(false);
        }
      });

      it('should decode JoinAccepted as spectator', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(0);
        writer.writeUuid('aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee');
        writer.writeByteArray(new Uint8Array([0xff]));
        writer.writeBool(true);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('JoinAccepted');
        if (result.type === 'JoinAccepted') {
          expect(result.isSpectator).toBe(true);
        }
      });
    });

    describe('JoinRejected decoding', () => {
      it('should decode JoinRejected with ServerFull reason', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(1); // JoinRejected variant
        writer.writeU32(0); // ServerFull reason variant
        writer.writeU32(1500); // currentPlayers

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('JoinRejected');
        if (result.type === 'JoinRejected') {
          expect(result.reason.type).toBe('ServerFull');
          if (result.reason.type === 'ServerFull') {
            expect(result.reason.currentPlayers).toBe(1500);
          }
        }
      });

      it('should decode JoinRejected with SpectatorsFull reason', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(1); // JoinRejected variant
        writer.writeU32(1); // SpectatorsFull reason variant

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('JoinRejected');
        if (result.type === 'JoinRejected') {
          expect(result.reason.type).toBe('SpectatorsFull');
        }
      });

      it('should decode JoinRejected with InvalidName reason', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(1); // JoinRejected variant
        writer.writeU32(2); // InvalidName reason variant

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('JoinRejected');
        if (result.type === 'JoinRejected') {
          expect(result.reason.type).toBe('InvalidName');
        }
      });

      it('should decode JoinRejected with Other reason', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(1); // JoinRejected variant
        writer.writeU32(6); // Other reason variant
        writer.writeString('Custom error message');

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('JoinRejected');
        if (result.type === 'JoinRejected') {
          expect(result.reason.type).toBe('Other');
          if (result.reason.type === 'Other') {
            expect(result.reason.message).toBe('Custom error message');
          }
        }
      });
    });

    describe('Pong decoding', () => {
      it('should decode Pong message', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(5); // Pong variant
        writer.writeU64(12345);
        writer.writeU64(67890);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Pong');
        if (result.type === 'Pong') {
          expect(result.clientTimestamp).toBe(12345);
          expect(result.serverTimestamp).toBe(67890);
        }
      });
    });

    describe('Kicked decoding', () => {
      it('should decode Kicked message', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(6); // Kicked variant
        writer.writeString('AFK timeout');

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Kicked');
        if (result.type === 'Kicked') {
          expect(result.reason).toBe('AFK timeout');
        }
      });
    });

    describe('PhaseChange decoding', () => {
      it('should decode PhaseChange to waiting', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(7); // PhaseChange variant
        writer.writeU32(0); // waiting phase
        writer.writeF32(0);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('PhaseChange');
        if (result.type === 'PhaseChange') {
          expect(result.phase).toBe('waiting');
        }
      });

      it('should decode PhaseChange to countdown', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(7);
        writer.writeU32(1); // countdown phase
        writer.writeF32(3.0);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('PhaseChange');
        if (result.type === 'PhaseChange') {
          expect(result.phase).toBe('countdown');
          expect(result.countdown).toBeCloseTo(3.0);
        }
      });

      it('should decode PhaseChange to playing', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(7);
        writer.writeU32(2); // playing phase
        writer.writeF32(0);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('PhaseChange');
        if (result.type === 'PhaseChange') {
          expect(result.phase).toBe('playing');
        }
      });

      it('should decode PhaseChange to ended', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(7);
        writer.writeU32(3); // ended phase
        writer.writeF32(0);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('PhaseChange');
        if (result.type === 'PhaseChange') {
          expect(result.phase).toBe('ended');
        }
      });
    });

    describe('SpectatorModeChanged decoding', () => {
      it('should decode SpectatorModeChanged true', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(8);
        writer.writeBool(true);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('SpectatorModeChanged');
        if (result.type === 'SpectatorModeChanged') {
          expect(result.isSpectator).toBe(true);
        }
      });

      it('should decode SpectatorModeChanged false', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(8);
        writer.writeBool(false);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('SpectatorModeChanged');
        if (result.type === 'SpectatorModeChanged') {
          expect(result.isSpectator).toBe(false);
        }
      });
    });

    describe('Event decoding', () => {
      it('should decode PlayerKilled event', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(4); // Event variant
        writer.writeU32(0); // PlayerKilled event
        writer.writeUuid('11111111-1111-1111-1111-111111111111');
        writer.writeUuid('22222222-2222-2222-2222-222222222222');
        writer.writeString('Killer');
        writer.writeString('Victim');

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Event');
        if (result.type === 'Event') {
          expect(result.event.type).toBe('PlayerKilled');
          if (result.event.type === 'PlayerKilled') {
            expect(result.event.killerName).toBe('Killer');
            expect(result.event.victimName).toBe('Victim');
          }
        }
      });

      it('should decode PlayerJoined event', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(4);
        writer.writeU32(1); // PlayerJoined
        writer.writeUuid('33333333-3333-3333-3333-333333333333');
        writer.writeString('NewPlayer');

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Event');
        if (result.type === 'Event') {
          expect(result.event.type).toBe('PlayerJoined');
        }
      });

      it('should decode PlayerLeft event', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(4);
        writer.writeU32(2); // PlayerLeft
        writer.writeUuid('44444444-4444-4444-4444-444444444444');
        writer.writeString('LeavingPlayer');

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Event');
        if (result.type === 'Event') {
          expect(result.event.type).toBe('PlayerLeft');
        }
      });

      it('should decode MatchStarted event', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(4);
        writer.writeU32(3); // MatchStarted

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Event');
        if (result.type === 'Event') {
          expect(result.event.type).toBe('MatchStarted');
        }
      });

      it('should decode MatchEnded with winner', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(4);
        writer.writeU32(4); // MatchEnded
        writer.writeBool(true); // has winner
        writer.writeUuid('55555555-5555-5555-5555-555555555555');
        writer.writeString('Winner');

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Event');
        if (result.type === 'Event' && result.event.type === 'MatchEnded') {
          expect(result.event.winnerName).toBe('Winner');
        }
      });

      it('should decode MatchEnded without winner', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(4);
        writer.writeU32(4); // MatchEnded
        writer.writeBool(false); // no winner

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Event');
        if (result.type === 'Event' && result.event.type === 'MatchEnded') {
          expect(result.event.winnerId).toBeNull();
          expect(result.event.winnerName).toBeNull();
        }
      });

      it('should decode ZoneCollapse event', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(4);
        writer.writeU32(5); // ZoneCollapse
        writer.writeU8(3);
        writer.writeF32(400.0);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Event');
        if (result.type === 'Event' && result.event.type === 'ZoneCollapse') {
          expect(result.event.phase).toBe(3);
          expect(result.event.newSafeRadius).toBeCloseTo(400.0);
        }
      });

      it('should decode PlayerDeflection event', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(4);
        writer.writeU32(6); // PlayerDeflection
        writer.writeUuid('66666666-6666-6666-6666-666666666666');
        writer.writeUuid('77777777-7777-7777-7777-777777777777');
        writer.writeF32(100.0);
        writer.writeF32(200.0);
        writer.writeF32(0.75);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Event');
        if (result.type === 'Event' && result.event.type === 'PlayerDeflection') {
          expect(result.event.position.x).toBeCloseTo(100.0);
          expect(result.event.position.y).toBeCloseTo(200.0);
          expect(result.event.intensity).toBeCloseTo(0.75);
        }
      });

      it('should decode GravityWellCharging event', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(4);
        writer.writeU32(7); // GravityWellCharging
        writer.writeU32(42);
        writer.writeF32(150.0);
        writer.writeF32(250.0);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Event');
        if (result.type === 'Event' && result.event.type === 'GravityWellCharging') {
          expect(result.event.wellId).toBe(42);
          expect(result.event.position.x).toBeCloseTo(150.0);
          expect(result.event.position.y).toBeCloseTo(250.0);
        }
      });

      it('should decode GravityWaveExplosion event', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(4);
        writer.writeU32(8); // GravityWaveExplosion
        writer.writeU32(99);
        writer.writeF32(300.0);
        writer.writeF32(400.0);
        writer.writeF32(1.5);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Event');
        if (result.type === 'Event' && result.event.type === 'GravityWaveExplosion') {
          expect(result.event.wellId).toBe(99);
          expect(result.event.strength).toBeCloseTo(1.5);
        }
      });

      it('should decode GravityWellDestroyed event', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(4);
        writer.writeU32(9); // GravityWellDestroyed
        writer.writeU32(123);
        writer.writeF32(500.0);
        writer.writeF32(600.0);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Event');
        if (result.type === 'Event' && result.event.type === 'GravityWellDestroyed') {
          expect(result.event.wellId).toBe(123);
        }
      });
    });

    describe('Snapshot decoding', () => {
      it('should decode minimal Snapshot', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(2); // Snapshot variant

        // GameSnapshot fields
        writer.writeU64(100); // tick
        writer.writeU32(2); // playing phase
        writer.writeF32(60.5); // matchTime
        writer.writeF32(0); // countdown

        writer.writeU64(0); // 0 players
        writer.writeU64(0); // 0 projectiles
        writer.writeU64(0); // 0 debris

        writer.writeU8(0); // arenaCollapsePhase
        writer.writeF32(600.0); // arenaSafeRadius
        writer.writeF32(1.0); // arenaScale

        writer.writeU64(0); // 0 gravity wells

        writer.writeU32(0); // totalPlayers
        writer.writeU32(0); // totalAlive

        writer.writeU64(0); // empty density grid

        writer.writeU64(0); // 0 notable players

        writer.writeU64(12345); // echoClientTime

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Snapshot');
        if (result.type === 'Snapshot') {
          expect(result.snapshot.tick).toBe(100);
          expect(result.snapshot.matchPhase).toBe('playing');
          expect(result.snapshot.matchTime).toBeCloseTo(60.5);
          expect(result.snapshot.players).toHaveLength(0);
          expect(result.snapshot.projectiles).toHaveLength(0);
          expect(result.snapshot.debris).toHaveLength(0);
          expect(result.snapshot.gravityWells).toHaveLength(0);
        }
      });

      it('should decode Snapshot with players', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(2); // Snapshot variant

        writer.writeU64(200); // tick
        writer.writeU32(2); // playing
        writer.writeF32(30.0);
        writer.writeF32(0);

        // 1 player
        writer.writeU64(1);
        writePlayerSnapshot(writer, {
          id: 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
          name: 'Player1',
          position: new Vec2(100, 200),
          velocity: new Vec2(5, 10),
          rotation: 1.5,
          mass: 150,
          alive: true,
          kills: 3,
          deaths: 1,
          spawnProtection: false,
          isBot: false,
          colorIndex: 5,
        });

        writer.writeU64(0); // projectiles
        writer.writeU64(0); // debris
        writer.writeU8(0);
        writer.writeF32(600.0);
        writer.writeF32(1.0);
        writer.writeU64(0); // wells
        writer.writeU32(1);
        writer.writeU32(1);
        writer.writeU64(0); // density grid
        writer.writeU64(0); // notable
        writer.writeU64(0);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Snapshot');
        if (result.type === 'Snapshot') {
          expect(result.snapshot.players).toHaveLength(1);
          expect(result.snapshot.players[0].name).toBe('Player1');
          expect(result.snapshot.players[0].mass).toBe(150);
          expect(result.snapshot.players[0].kills).toBe(3);
        }
      });

      it('should decode Snapshot with gravity wells', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(2);

        writer.writeU64(300);
        writer.writeU32(2);
        writer.writeF32(45.0);
        writer.writeF32(0);

        writer.writeU64(0); // players
        writer.writeU64(0); // projectiles
        writer.writeU64(0); // debris

        writer.writeU8(2);
        writer.writeF32(400.0);
        writer.writeF32(0.9);

        // 2 gravity wells
        writer.writeU64(2);
        // Well 1
        writer.writeU32(1);
        writer.writeVec2(new Vec2(100, 100));
        writer.writeF32(5000);
        writer.writeF32(25);
        // Well 2
        writer.writeU32(2);
        writer.writeVec2(new Vec2(-200, 300));
        writer.writeF32(3000);
        writer.writeF32(20);

        writer.writeU32(5);
        writer.writeU32(3);
        writer.writeU64(0);
        writer.writeU64(0);
        writer.writeU64(0);

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Snapshot');
        if (result.type === 'Snapshot') {
          expect(result.snapshot.gravityWells).toHaveLength(2);
          expect(result.snapshot.gravityWells[0].id).toBe(1);
          expect(result.snapshot.gravityWells[1].mass).toBe(3000);
        }
      });
    });

    describe('Delta decoding', () => {
      it('should decode minimal Delta', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(3); // Delta variant

        writer.writeU64(150); // tick
        writer.writeU64(100); // baseTick

        writer.writeU64(0); // 0 player updates
        writer.writeU64(0); // 0 projectile updates
        writer.writeU64(0); // 0 removed projectiles
        writer.writeU64(0); // 0 debris

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Delta');
        if (result.type === 'Delta') {
          expect(result.delta.tick).toBe(150);
          expect(result.delta.baseTick).toBe(100);
          expect(result.delta.playerUpdates).toHaveLength(0);
          expect(result.delta.debris).toHaveLength(0);
        }
      });

      it('should decode Delta with player updates', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(3);

        writer.writeU64(200);
        writer.writeU64(150);

        // 1 player update
        writer.writeU64(1);
        writer.writeUuid('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb');
        // Optional fields
        writer.writeBool(true); // has position
        writer.writeVec2(new Vec2(250, 350));
        writer.writeBool(true); // has velocity
        writer.writeVec2(new Vec2(10, 20));
        writer.writeBool(false); // no rotation
        writer.writeBool(true); // has mass
        writer.writeF32(175);
        writer.writeBool(false); // no alive change
        writer.writeBool(true); // has kills
        writer.writeU32(5);

        writer.writeU64(0); // 0 projectile updates
        writer.writeU64(0); // 0 removed projectiles
        writer.writeU64(0); // 0 debris

        const result = decodeServerMessage(writer.getBuffer());
        expect(result.type).toBe('Delta');
        if (result.type === 'Delta') {
          expect(result.delta.playerUpdates).toHaveLength(1);
          expect(result.delta.playerUpdates[0].position?.x).toBe(250);
          expect(result.delta.playerUpdates[0].mass).toBe(175);
          expect(result.delta.playerUpdates[0].kills).toBe(5);
          expect(result.delta.playerUpdates[0].rotation).toBeUndefined();
        }
      });
    });

    describe('error handling', () => {
      it('should throw on unknown server message variant', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(999); // Invalid variant

        expect(() => decodeServerMessage(writer.getBuffer())).toThrow(
          'Unknown server message variant: 999'
        );
      });

      it('should throw on unknown game event variant', () => {
        const writer = new TestBinaryWriter();
        writer.writeU32(4); // Event variant
        writer.writeU32(999); // Invalid event type

        expect(() => decodeServerMessage(writer.getBuffer())).toThrow(
          'Unknown game event variant: 999'
        );
      });
    });
  });

  describe('UUID encoding/decoding', () => {
    it('should round-trip UUID correctly', () => {
      const uuid = '12345678-abcd-ef01-2345-6789abcdef01';
      const writer = new TestBinaryWriter();
      writer.writeUuid(uuid);

      const reader = new TestBinaryReader(writer.getBuffer());
      const decoded = reader.readUuid();
      expect(decoded).toBe(uuid);
    });

    it('should handle all-zeros UUID', () => {
      const uuid = '00000000-0000-0000-0000-000000000000';
      const writer = new TestBinaryWriter();
      writer.writeUuid(uuid);

      const reader = new TestBinaryReader(writer.getBuffer());
      const decoded = reader.readUuid();
      expect(decoded).toBe(uuid);
    });

    it('should handle all-f UUID', () => {
      const uuid = 'ffffffff-ffff-ffff-ffff-ffffffffffff';
      const writer = new TestBinaryWriter();
      writer.writeUuid(uuid);

      const reader = new TestBinaryReader(writer.getBuffer());
      const decoded = reader.readUuid();
      expect(decoded).toBe(uuid);
    });
  });
});

// Helper test classes that mirror the internal classes for testing
class TestBinaryWriter {
  private buffer: ArrayBuffer;
  private view: DataView;
  private offset: number = 0;

  constructor(initialSize: number = 1024) {
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
    this.writeU64(16);
    const hex = uuid.replace(/-/g, '');
    for (let i = 0; i < 16; i++) {
      this.writeU8(parseInt(hex.substring(i * 2, i * 2 + 2), 16));
    }
  }

  writeByteArray(data: Uint8Array): void {
    this.writeU64(data.length);
    this.ensureCapacity(data.length);
    new Uint8Array(this.buffer, this.offset).set(data);
    this.offset += data.length;
  }

  getBuffer(): ArrayBuffer {
    return this.buffer.slice(0, this.offset);
  }
}

class TestBinaryReader {
  private view: DataView;
  private offset: number = 0;

  constructor(buffer: ArrayBuffer) {
    this.view = new DataView(buffer);
  }

  readU8(): number {
    return this.view.getUint8(this.offset++);
  }

  readU64(): number {
    const value = this.view.getBigUint64(this.offset, true);
    this.offset += 8;
    return Number(value);
  }

  readUuid(): string {
    const length = this.readU64();
    if (length !== 16) {
      throw new Error(`Invalid UUID length: expected 16, got ${length}`);
    }
    const bytes: string[] = [];
    for (let i = 0; i < 16; i++) {
      bytes.push(this.readU8().toString(16).padStart(2, '0'));
    }
    return `${bytes.slice(0, 4).join('')}-${bytes.slice(4, 6).join('')}-${bytes.slice(6, 8).join('')}-${bytes.slice(8, 10).join('')}-${bytes.slice(10, 16).join('')}`;
  }
}

// Helper to write a full PlayerSnapshot
// Player flag constants (must match Codec.ts)
const PLAYER_FLAG_ALIVE = 0b0000_0001;
const PLAYER_FLAG_SPAWN_PROTECTION = 0b0000_0010;
const PLAYER_FLAG_IS_BOT = 0b0000_0100;

function writePlayerSnapshot(writer: TestBinaryWriter, player: {
  id: string;
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
  spawnTick?: number;
}): void {
  writer.writeUuid(player.id);
  writer.writeString(player.name);
  writer.writeVec2(player.position);
  writer.writeVec2(player.velocity);
  writer.writeF32(player.rotation);
  writer.writeF32(player.mass);
  // Pack flags into single byte (matches Codec.ts readPlayerSnapshot)
  let flags = 0;
  if (player.alive) flags |= PLAYER_FLAG_ALIVE;
  if (player.spawnProtection) flags |= PLAYER_FLAG_SPAWN_PROTECTION;
  if (player.isBot) flags |= PLAYER_FLAG_IS_BOT;
  writer.writeU8(flags);
  writer.writeU32(player.kills);
  writer.writeU32(player.deaths);
  writer.writeU8(player.colorIndex);
  writer.writeU64(player.spawnTick ?? 0);
}
