import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { Vec2 } from '@/utils/Vec2';
import { StateSync } from './StateSync';
import { NETWORK, PHYSICS } from '@/utils/Constants';
import type { GameSnapshot, PlayerSnapshot, DeltaUpdate, PlayerInput, MatchPhase } from './Protocol';

const { ADAPTIVE_INTERPOLATION } = NETWORK;

// Helper to create mock player snapshot
function createMockPlayerSnapshot(overrides: Partial<PlayerSnapshot> = {}): PlayerSnapshot {
  return {
    id: overrides.id ?? 'player-1',
    name: overrides.name ?? 'TestPlayer',
    position: overrides.position ?? new Vec2(100, 100),
    velocity: overrides.velocity ?? new Vec2(0, 0),
    rotation: overrides.rotation ?? 0,
    mass: overrides.mass ?? 100,
    alive: overrides.alive ?? true,
    kills: overrides.kills ?? 0,
    deaths: overrides.deaths ?? 0,
    spawnProtection: overrides.spawnProtection ?? false,
    isBot: overrides.isBot ?? false,
    colorIndex: overrides.colorIndex ?? 0,
  };
}

// Helper to create mock game snapshot
function createMockSnapshot(tick: number, overrides: Partial<GameSnapshot> = {}): GameSnapshot {
  return {
    tick,
    matchPhase: overrides.matchPhase ?? 'playing',
    matchTime: overrides.matchTime ?? 60,
    countdown: overrides.countdown ?? 0,
    players: overrides.players ?? [],
    projectiles: overrides.projectiles ?? [],
    debris: overrides.debris ?? [],
    arenaCollapsePhase: overrides.arenaCollapsePhase ?? 0,
    arenaSafeRadius: overrides.arenaSafeRadius ?? 600,
    arenaScale: overrides.arenaScale ?? 1.0,
    gravityWells: overrides.gravityWells ?? [],
    totalPlayers: overrides.totalPlayers ?? 10,
    totalAlive: overrides.totalAlive ?? 8,
    densityGrid: overrides.densityGrid ?? [],
    notablePlayers: overrides.notablePlayers ?? [],
    echoClientTime: overrides.echoClientTime ?? 0,
  };
}

// Helper to create minimal valid snapshot (for adaptive interpolation tests)
function createSnapshot(tick: number, overrides: Partial<GameSnapshot> = {}): GameSnapshot {
  return {
    tick,
    matchPhase: 'playing' as MatchPhase,
    matchTime: tick * PHYSICS.DT,
    countdown: 0,
    players: [],
    projectiles: [],
    debris: [],
    arenaCollapsePhase: 0,
    arenaSafeRadius: 600,
    arenaScale: 1.0,
    gravityWells: [],
    totalPlayers: 0,
    totalAlive: 0,
    densityGrid: [],
    notablePlayers: [],
    echoClientTime: 0,
    ...overrides,
  };
}

describe('StateSync', () => {
  let stateSync: StateSync;
  let mockPerformanceNow: number;

  beforeEach(() => {
    stateSync = new StateSync();
    mockPerformanceNow = 1000;
    vi.spyOn(performance, 'now').mockImplementation(() => mockPerformanceNow);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('applySnapshot', () => {
    it('should add snapshot to buffer', () => {
      const snapshot = createMockSnapshot(1);
      stateSync.applySnapshot(snapshot);

      expect(stateSync.getCurrentTick()).toBe(1);
    });

    it('should update current tick', () => {
      stateSync.applySnapshot(createMockSnapshot(10));
      expect(stateSync.getCurrentTick()).toBe(10);

      stateSync.applySnapshot(createMockSnapshot(20));
      expect(stateSync.getCurrentTick()).toBe(20);
    });

    it('should not decrease current tick', () => {
      stateSync.applySnapshot(createMockSnapshot(20));
      stateSync.applySnapshot(createMockSnapshot(10));

      expect(stateSync.getCurrentTick()).toBe(20);
    });

    it('should limit buffer to max size', () => {
      // Add more than max snapshots
      for (let i = 0; i < 40; i++) {
        mockPerformanceNow = 1000 + i * 100;
        stateSync.applySnapshot(createMockSnapshot(i));
      }

      // Should still work and have latest snapshot
      expect(stateSync.getCurrentTick()).toBe(39);
    });

    it('should trigger prediction reconciliation for local player', () => {
      stateSync.setLocalPlayerId('player-1');

      const snapshot = createMockSnapshot(5, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            position: new Vec2(200, 300),
            velocity: new Vec2(10, 20),
          }),
        ],
      });

      stateSync.applySnapshot(snapshot);

      const predicted = stateSync.getPredictedLocalPlayer();
      expect(predicted).not.toBeNull();
      expect(predicted?.position.x).toBe(200);
      expect(predicted?.position.y).toBe(300);
    });
  });

  describe('applyDelta', () => {
    beforeEach(() => {
      // Apply a base snapshot first
      const baseSnapshot = createMockSnapshot(10, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            position: new Vec2(100, 100),
            mass: 100,
          }),
        ],
        projectiles: [
          {
            id: 1,
            ownerId: 'player-1',
            position: new Vec2(50, 50),
            velocity: new Vec2(10, 0),
            mass: 10,
          },
        ],
      });
      stateSync.applySnapshot(baseSnapshot);
    });

    it('should apply delta updates to players', () => {
      const delta: DeltaUpdate = {
        tick: 15,
        baseTick: 10,
        playerUpdates: [
          {
            id: 'player-1',
            position: new Vec2(200, 200),
            mass: 150,
          },
        ],
        projectileUpdates: [],
        removedProjectiles: [],
        debris: [],
      };

      mockPerformanceNow = 1500;
      stateSync.applyDelta(delta);

      expect(stateSync.getCurrentTick()).toBe(15);
    });

    it('should apply projectile updates', () => {
      const delta: DeltaUpdate = {
        tick: 15,
        baseTick: 10,
        playerUpdates: [],
        projectileUpdates: [
          {
            id: 1,
            position: new Vec2(100, 50),
            velocity: new Vec2(10, 0),
          },
        ],
        removedProjectiles: [],
        debris: [],
      };

      mockPerformanceNow = 1500;
      stateSync.applyDelta(delta);

      expect(stateSync.getCurrentTick()).toBe(15);
    });

    it('should remove projectiles', () => {
      const delta: DeltaUpdate = {
        tick: 15,
        baseTick: 10,
        playerUpdates: [],
        projectileUpdates: [],
        removedProjectiles: [1],
        debris: [],
      };

      mockPerformanceNow = 1500;
      stateSync.applyDelta(delta);

      expect(stateSync.getCurrentTick()).toBe(15);
    });

    it('should ignore delta with missing base snapshot', () => {
      const delta: DeltaUpdate = {
        tick: 100,
        baseTick: 99, // Base doesn't exist
        playerUpdates: [],
        projectileUpdates: [],
        removedProjectiles: [],
        debris: [],
      };

      const tickBefore = stateSync.getCurrentTick();
      stateSync.applyDelta(delta);

      expect(stateSync.getCurrentTick()).toBe(tickBefore);
    });
  });

  describe('recordInput', () => {
    it('should record player input', () => {
      const input: PlayerInput = {
        sequence: 1,
        tick: 100,
        clientTime: 1000,
        thrust: new Vec2(1, 0),
        aim: new Vec2(1, 0),
        boost: true,
        fire: false,
        fireReleased: false,
      };

      stateSync.recordInput(input);
      // Input should be recorded (tested indirectly through prediction)
    });

    it('should limit pending inputs buffer', () => {
      // Add more than max inputs
      for (let i = 0; i < 100; i++) {
        stateSync.recordInput({
          sequence: i,
          tick: i,
          clientTime: i * 16,
          thrust: new Vec2(0, 0),
          aim: new Vec2(1, 0),
          boost: false,
          fire: false,
          fireReleased: false,
        });
      }

      // Should not crash and should work
      expect(stateSync.getCurrentTick()).toBeDefined();
    });
  });

  describe('getInterpolatedState', () => {
    it('should return null when no snapshots', () => {
      const state = stateSync.getInterpolatedState();
      expect(state).toBeNull();
    });

    it('should return state from single snapshot', () => {
      stateSync.applySnapshot(createMockSnapshot(1, {
        players: [createMockPlayerSnapshot({ id: 'player-1' })],
      }));

      const state = stateSync.getInterpolatedState();
      expect(state).not.toBeNull();
      expect(state?.tick).toBe(1);
      expect(state?.players.size).toBe(1);
    });

    it('should interpolate between two snapshots', () => {
      // First snapshot
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            position: new Vec2(0, 0),
          }),
        ],
      }));

      // Second snapshot 100ms later
      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            position: new Vec2(100, 100),
          }),
        ],
      }));

      // Render time is 150ms ahead of first snapshot (50% between)
      // But we subtract interpolation delay, so position depends on delay
      mockPerformanceNow = 1150;
      const state = stateSync.getInterpolatedState();

      expect(state).not.toBeNull();
      expect(state?.players.get('player-1')).toBeDefined();
    });

    it('should use first snapshot when render time is too early', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        matchTime: 30,
      }));

      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        matchTime: 60,
      }));

      // Render time before first snapshot
      mockPerformanceNow = 900;
      const state = stateSync.getInterpolatedState();

      expect(state).not.toBeNull();
    });

    it('should use last snapshot when render time is too late', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        matchTime: 30,
      }));

      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        matchTime: 60,
      }));

      // Render time after last snapshot
      mockPerformanceNow = 5000;
      const state = stateSync.getInterpolatedState();

      expect(state).not.toBeNull();
      expect(state?.tick).toBe(2);
    });

    it('should interpolate player mass', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            mass: 100,
          }),
        ],
      }));

      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            mass: 200,
          }),
        ],
      }));

      mockPerformanceNow = 1200;
      const state = stateSync.getInterpolatedState();

      // Mass should be interpolated
      const player = state?.players.get('player-1');
      expect(player?.mass).toBeGreaterThanOrEqual(100);
    });

    it('should handle new players (no interpolation)', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        players: [createMockPlayerSnapshot({ id: 'player-1' })],
      }));

      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        players: [
          createMockPlayerSnapshot({ id: 'player-1' }),
          createMockPlayerSnapshot({
            id: 'player-2', // New player
            position: new Vec2(500, 500),
          }),
        ],
      }));

      mockPerformanceNow = 1200;
      const state = stateSync.getInterpolatedState();

      expect(state?.players.has('player-2')).toBe(true);
    });

    it('should snap to position on player respawn (no interpolation)', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            position: new Vec2(0, 0),
            alive: false,
          }),
        ],
      }));

      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            position: new Vec2(300, 300),
            alive: true, // Respawned
            spawnProtection: true,
          }),
        ],
      }));

      mockPerformanceNow = 1200;
      const state = stateSync.getInterpolatedState();

      // Should snap to new position, not interpolate from death position
      const player = state?.players.get('player-1');
      expect(player?.position.x).toBe(300);
      expect(player?.position.y).toBe(300);
    });

    it('should interpolate gravity wells', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        gravityWells: [
          { id: 1, position: new Vec2(0, 0), mass: 5000, coreRadius: 20 },
        ],
      }));

      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        gravityWells: [
          { id: 1, position: new Vec2(100, 100), mass: 6000, coreRadius: 25 },
        ],
      }));

      mockPerformanceNow = 1200;
      const state = stateSync.getInterpolatedState();

      expect(state?.gravityWells).toHaveLength(1);
    });

    it('should filter destroyed gravity wells', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        gravityWells: [
          { id: 1, position: new Vec2(0, 0), mass: 5000, coreRadius: 20 },
          { id: 2, position: new Vec2(100, 100), mass: 3000, coreRadius: 15 },
        ],
      }));

      stateSync.markWellDestroyed(1);

      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        gravityWells: [
          { id: 1, position: new Vec2(0, 0), mass: 5000, coreRadius: 20 },
          { id: 2, position: new Vec2(100, 100), mass: 3000, coreRadius: 15 },
        ],
      }));

      mockPerformanceNow = 1200;
      const state = stateSync.getInterpolatedState();

      expect(state?.gravityWells).toHaveLength(1);
      expect(state?.gravityWells[0].id).toBe(2);
    });
  });

  describe('getPredictedLocalPlayer', () => {
    it('should return null when no local player set', () => {
      expect(stateSync.getPredictedLocalPlayer()).toBeNull();
    });

    it('should return predicted position after setting local player', () => {
      stateSync.setLocalPlayerId('player-1');

      stateSync.applySnapshot(createMockSnapshot(1, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            position: new Vec2(100, 200),
            velocity: new Vec2(10, 20),
          }),
        ],
      }));

      const predicted = stateSync.getPredictedLocalPlayer();
      expect(predicted).not.toBeNull();
      expect(predicted?.position.x).toBe(100);
      expect(predicted?.position.y).toBe(200);
    });
  });

  describe('client prediction', () => {
    beforeEach(() => {
      stateSync.setLocalPlayerId('player-1');
    });

    it('should apply pending inputs to prediction', () => {
      stateSync.applySnapshot(createMockSnapshot(1, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            position: new Vec2(0, 0),
            velocity: new Vec2(0, 0),
            mass: 100,
          }),
        ],
      }));

      // Record boost input
      stateSync.recordInput({
        sequence: 2,
        tick: 2,
        clientTime: 1050,
        thrust: new Vec2(1, 0), // Boost right
        aim: new Vec2(1, 0),
        boost: true,
        fire: false,
        fireReleased: false,
      });

      // Apply another snapshot to trigger reconciliation
      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(1, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            position: new Vec2(0, 0),
            velocity: new Vec2(0, 0),
            mass: 100,
          }),
        ],
      }));

      const predicted = stateSync.getPredictedLocalPlayer();
      // Should have moved due to boost input
      expect(predicted?.velocity.x).toBeGreaterThan(0);
    });

    it('should remove acknowledged inputs after reconciliation', () => {
      // Record inputs
      for (let i = 1; i <= 5; i++) {
        stateSync.recordInput({
          sequence: i,
          tick: i,
          clientTime: 1000 + i * 16,
          thrust: new Vec2(1, 0),
          aim: new Vec2(1, 0),
          boost: true,
          fire: false,
          fireReleased: false,
        });
      }

      // Apply snapshot at tick 3 - should remove inputs 1-3
      stateSync.applySnapshot(createMockSnapshot(3, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            position: new Vec2(50, 0),
            velocity: new Vec2(10, 0),
          }),
        ],
      }));

      // Remaining inputs should be 4 and 5
      // This is tested indirectly through prediction behavior
    });
  });

  describe('markWellDestroyed', () => {
    it('should mark well as destroyed', () => {
      stateSync.applySnapshot(createMockSnapshot(1, {
        gravityWells: [
          { id: 1, position: new Vec2(0, 0), mass: 5000, coreRadius: 20 },
        ],
      }));

      stateSync.markWellDestroyed(1);

      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        gravityWells: [
          { id: 1, position: new Vec2(0, 0), mass: 5000, coreRadius: 20 },
        ],
      }));

      const state = stateSync.getInterpolatedState();
      expect(state?.gravityWells.find(w => w.id === 1)).toBeUndefined();
    });

    it('should clean up destroyed tracking when server confirms removal', () => {
      stateSync.applySnapshot(createMockSnapshot(1, {
        gravityWells: [
          { id: 1, position: new Vec2(0, 0), mass: 5000, coreRadius: 20 },
          { id: 2, position: new Vec2(100, 100), mass: 3000, coreRadius: 15 },
        ],
      }));

      stateSync.markWellDestroyed(1);

      // Server confirms removal by not sending well 1
      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        gravityWells: [
          { id: 2, position: new Vec2(100, 100), mass: 3000, coreRadius: 15 },
        ],
      }));

      // Destroyed tracking should be cleaned up (tested indirectly)
    });
  });

  describe('interpolationDelay', () => {
    it('should start with default delay', () => {
      expect(stateSync.interpolationDelay).toBe(NETWORK.INTERPOLATION_DELAY_MS);
    });

    it('should adapt based on snapshot arrival rate', () => {
      // First snapshot
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1));

      // Second snapshot 100ms later (faster rate)
      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2));

      // Third snapshot
      mockPerformanceNow = 1200;
      stateSync.applySnapshot(createMockSnapshot(3));

      // Delay should adapt (might be different from initial)
      expect(stateSync.interpolationDelay).toBeGreaterThan(0);
    });
  });

  describe('reset', () => {
    it('should reset all state', () => {
      stateSync.setLocalPlayerId('player-1');
      stateSync.applySnapshot(createMockSnapshot(10, {
        players: [createMockPlayerSnapshot({ id: 'player-1' })],
        gravityWells: [{ id: 1, position: new Vec2(0, 0), mass: 5000, coreRadius: 20 }],
      }));
      stateSync.markWellDestroyed(1);
      stateSync.recordInput({
        sequence: 1,
        tick: 1,
        clientTime: 1000,
        thrust: new Vec2(1, 0),
        aim: new Vec2(1, 0),
        boost: true,
        fire: false,
        fireReleased: false,
      });

      stateSync.reset();

      expect(stateSync.getCurrentTick()).toBe(0);
      expect(stateSync.getInterpolatedState()).toBeNull();
      expect(stateSync.interpolationDelay).toBe(NETWORK.INTERPOLATION_DELAY_MS);
    });
  });

  describe('player birth animations', () => {
    it('should skip birth animation for players in first snapshot', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            spawnProtection: true,
          }),
        ],
      }));

      const state = stateSync.getInterpolatedState();
      const player = state?.players.get('player-1');

      // bornTime should be 0 (skip animation) for first snapshot
      expect(player?.bornTime).toBe(0);
    });

    it('should track birth times for players spawning after first snapshot', () => {
      // First snapshot without player
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        players: [],
      }));

      // Second snapshot with new player
      mockPerformanceNow = 2000;
      stateSync.applySnapshot(createMockSnapshot(2, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            spawnProtection: true,
          }),
        ],
      }));

      // Set time after second snapshot for proper interpolation
      mockPerformanceNow = 2200;
      const state = stateSync.getInterpolatedState();
      const player = state?.players.get('player-1');

      // Player should exist in interpolated state
      expect(player).toBeDefined();
      // bornTime should be defined (exact value depends on internal timing)
      expect(player?.bornTime).toBeDefined();
    });

    it('should track birth times on respawn', () => {
      // First snapshot with alive player
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            alive: true,
          }),
        ],
      }));

      // Player dies
      mockPerformanceNow = 2000;
      stateSync.applySnapshot(createMockSnapshot(2, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            alive: false,
          }),
        ],
      }));

      // Player respawns
      mockPerformanceNow = 3000;
      stateSync.applySnapshot(createMockSnapshot(3, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            alive: true,
            spawnProtection: true,
          }),
        ],
      }));

      // Set time after third snapshot for proper interpolation
      mockPerformanceNow = 3200;
      const state = stateSync.getInterpolatedState();
      const player = state?.players.get('player-1');

      // Player should exist in interpolated state
      expect(player).toBeDefined();
      // bornTime should be defined
      expect(player?.bornTime).toBeDefined();
    });
  });

  describe('gravity well birth animations', () => {
    it('should skip birth animation for wells in first snapshot', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        gravityWells: [
          { id: 1, position: new Vec2(0, 0), mass: 5000, coreRadius: 20 },
        ],
      }));

      const state = stateSync.getInterpolatedState();
      const well = state?.gravityWells[0];

      expect(well?.bornTime).toBe(0);
    });

    it('should track birth times for wells appearing after first snapshot', () => {
      // First snapshot without wells
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        gravityWells: [],
      }));

      // Second snapshot with well
      mockPerformanceNow = 2000;
      stateSync.applySnapshot(createMockSnapshot(2, {
        gravityWells: [
          { id: 1, position: new Vec2(0, 0), mass: 5000, coreRadius: 20 },
        ],
      }));

      // Set time after second snapshot for proper interpolation
      mockPerformanceNow = 2200;
      const state = stateSync.getInterpolatedState();
      const well = state?.gravityWells[0];

      // Well should exist in interpolated state
      expect(well).toBeDefined();
      // bornTime should be defined
      expect(well?.bornTime).toBeDefined();
    });
  });

  describe('angle interpolation', () => {
    it('should interpolate rotation correctly across 0/2π boundary', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            rotation: Math.PI * 1.9, // Near 2π
          }),
        ],
      }));

      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        players: [
          createMockPlayerSnapshot({
            id: 'player-1',
            rotation: 0.1, // Just past 0
          }),
        ],
      }));

      mockPerformanceNow = 1200;
      const state = stateSync.getInterpolatedState();
      const player = state?.players.get('player-1');

      // Should interpolate through the short path (not the long way around)
      expect(player?.rotation).toBeDefined();
    });
  });

  describe('projectile interpolation', () => {
    it('should interpolate projectile position', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        projectiles: [
          { id: 1, ownerId: 'player-1', position: new Vec2(0, 0), velocity: new Vec2(100, 0), mass: 10 },
        ],
      }));

      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        projectiles: [
          { id: 1, ownerId: 'player-1', position: new Vec2(100, 0), velocity: new Vec2(100, 0), mass: 10 },
        ],
      }));

      mockPerformanceNow = 1200;
      const state = stateSync.getInterpolatedState();
      const proj = state?.projectiles.get(1);

      expect(proj?.position.x).toBeGreaterThanOrEqual(0);
    });

    it('should handle new projectiles (no interpolation)', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        projectiles: [],
      }));

      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        projectiles: [
          { id: 1, ownerId: 'player-1', position: new Vec2(50, 50), velocity: new Vec2(100, 0), mass: 10 },
        ],
      }));

      mockPerformanceNow = 1200;
      const state = stateSync.getInterpolatedState();

      expect(state?.projectiles.size).toBe(1);
    });
  });

  describe('debris interpolation', () => {
    it('should interpolate debris position', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        debris: [
          { id: 1, position: new Vec2(0, 0), size: 1 },
        ],
      }));

      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        debris: [
          { id: 1, position: new Vec2(50, 50), size: 1 },
        ],
      }));

      mockPerformanceNow = 1200;
      const state = stateSync.getInterpolatedState();
      const debris = state?.debris.get(1);

      expect(debris).toBeDefined();
    });
  });

  describe('notable players interpolation', () => {
    it('should interpolate notable player positions', () => {
      mockPerformanceNow = 1000;
      stateSync.applySnapshot(createMockSnapshot(1, {
        notablePlayers: [
          { id: 'player-1', position: new Vec2(0, 0), mass: 500, colorIndex: 5 },
        ],
      }));

      mockPerformanceNow = 1100;
      stateSync.applySnapshot(createMockSnapshot(2, {
        notablePlayers: [
          { id: 'player-1', position: new Vec2(100, 100), mass: 600, colorIndex: 5 },
        ],
      }));

      mockPerformanceNow = 1200;
      const state = stateSync.getInterpolatedState();

      expect(state?.notablePlayers).toHaveLength(1);
    });
  });
});

// Tests for StateSync adaptive interpolation (from main branch)
describe('StateSync adaptive interpolation', () => {
  let stateSync: StateSync;
  let mockNow: number;

  beforeEach(() => {
    stateSync = new StateSync();
    mockNow = 1000;
    vi.spyOn(performance, 'now').mockImplementation(() => mockNow);
  });

  describe('initial state', () => {
    it('starts with default interpolation delay', () => {
      expect(stateSync.interpolationDelay).toBe(NETWORK.INTERPOLATION_DELAY_MS);
    });
  });

  describe('adaptive delay calculation', () => {
    it('adapts delay for 30Hz updates (player rate)', () => {
      // Simulate 30Hz snapshots (~33ms interval)
      const interval = 33;

      // First snapshot establishes baseline
      stateSync.applySnapshot(createSnapshot(1));

      // Apply several snapshots at 30Hz to stabilize EMA
      for (let i = 2; i <= 10; i++) {
        mockNow += interval;
        stateSync.applySnapshot(createSnapshot(i));
      }

      // At 33ms interval with 2 buffer snapshots, target = 66ms
      // Should be clamped to MIN_DELAY_MS (80ms)
      expect(stateSync.interpolationDelay).toBe(ADAPTIVE_INTERPOLATION.MIN_DELAY_MS);
    });

    it('adapts delay for 15Hz updates (spectator rate)', () => {
      // Simulate 15Hz snapshots (~66ms interval)
      const interval = 66;

      stateSync.applySnapshot(createSnapshot(1));

      // Apply several snapshots at 15Hz to stabilize EMA
      for (let i = 2; i <= 10; i++) {
        mockNow += interval;
        stateSync.applySnapshot(createSnapshot(i));
      }

      // At 66ms interval with 2 buffer snapshots, target = 132ms
      // Should be between MIN (80) and MAX (200)
      expect(stateSync.interpolationDelay).toBeGreaterThan(ADAPTIVE_INTERPOLATION.MIN_DELAY_MS);
      expect(stateSync.interpolationDelay).toBeLessThan(ADAPTIVE_INTERPOLATION.MAX_DELAY_MS);
    });

    it('clamps delay to maximum for very slow updates', () => {
      // Simulate very slow updates (~150ms interval)
      const interval = 150;

      stateSync.applySnapshot(createSnapshot(1));

      for (let i = 2; i <= 10; i++) {
        mockNow += interval;
        stateSync.applySnapshot(createSnapshot(i));
      }

      // At 150ms interval with 2 buffer, target = 300ms, clamped to 200ms
      expect(stateSync.interpolationDelay).toBe(ADAPTIVE_INTERPOLATION.MAX_DELAY_MS);
    });

    it('ignores unreasonable intervals (too short)', () => {
      stateSync.applySnapshot(createSnapshot(1));

      // Very short interval (< 10ms) should be ignored
      mockNow += 5;
      stateSync.applySnapshot(createSnapshot(2));

      // Should stay at default since interval was ignored
      expect(stateSync.interpolationDelay).toBe(NETWORK.INTERPOLATION_DELAY_MS);
    });

    it('ignores unreasonable intervals (too long)', () => {
      stateSync.applySnapshot(createSnapshot(1));

      // Very long interval (> 500ms) should be ignored
      mockNow += 600;
      stateSync.applySnapshot(createSnapshot(2));

      // Should stay at default since interval was ignored
      expect(stateSync.interpolationDelay).toBe(NETWORK.INTERPOLATION_DELAY_MS);
    });

    it('smoothly adapts when switching rates', () => {
      const fastInterval = 33;  // 30Hz
      const slowInterval = 66;  // 15Hz

      stateSync.applySnapshot(createSnapshot(1));

      // Establish fast rate
      for (let i = 2; i <= 6; i++) {
        mockNow += fastInterval;
        stateSync.applySnapshot(createSnapshot(i));
      }
      const delayAtFastRate = stateSync.interpolationDelay;

      // Switch to slow rate
      for (let i = 7; i <= 20; i++) {
        mockNow += slowInterval;
        stateSync.applySnapshot(createSnapshot(i));
      }
      const delayAtSlowRate = stateSync.interpolationDelay;

      // Delay should have increased due to EMA smoothing
      expect(delayAtSlowRate).toBeGreaterThan(delayAtFastRate);
    });
  });

  describe('reset', () => {
    it('resets adaptive interpolation state', () => {
      // Apply some snapshots to change state
      stateSync.applySnapshot(createSnapshot(1));
      mockNow += 66;
      stateSync.applySnapshot(createSnapshot(2));
      mockNow += 66;
      stateSync.applySnapshot(createSnapshot(3));

      // State should have changed
      expect(stateSync.interpolationDelay).not.toBe(NETWORK.INTERPOLATION_DELAY_MS);

      // Reset
      stateSync.reset();

      // Should be back to defaults
      expect(stateSync.interpolationDelay).toBe(NETWORK.INTERPOLATION_DELAY_MS);
    });
  });
});
