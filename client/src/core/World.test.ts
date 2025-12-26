import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import { World } from './World';
import { ARENA, MASS, PLAYER_COLORS } from '@/utils/Constants';
import type { InterpolatedState, InterpolatedPlayer, InterpolatedProjectile, InterpolatedDebris, InterpolatedGravityWell, InterpolatedNotablePlayer } from '@/net/StateSync';
import type { MatchPhase } from '@/net/Protocol';

// Helper to create a mock player
function createMockPlayer(overrides: Partial<InterpolatedPlayer> = {}): InterpolatedPlayer {
  return {
    id: overrides.id ?? 'player-1',
    name: overrides.name ?? 'TestPlayer',
    position: overrides.position ?? { x: 100, y: 100 },
    velocity: overrides.velocity ?? { x: 0, y: 0 },
    rotation: overrides.rotation ?? 0,
    mass: overrides.mass ?? 100,
    alive: overrides.alive ?? true,
    kills: overrides.kills ?? 0,
    deaths: overrides.deaths ?? 0,
    spawnProtection: overrides.spawnProtection ?? false,
    isBot: overrides.isBot ?? false,
    colorIndex: overrides.colorIndex ?? 0,
    bornTime: overrides.bornTime ?? 0,
  };
}

// Helper to create a mock state
function createMockState(overrides: Partial<InterpolatedState> = {}): InterpolatedState {
  return {
    tick: overrides.tick ?? 100,
    matchPhase: overrides.matchPhase ?? 'playing',
    matchTime: overrides.matchTime ?? 60,
    countdown: overrides.countdown ?? 0,
    players: overrides.players ?? new Map(),
    projectiles: overrides.projectiles ?? new Map(),
    debris: overrides.debris ?? new Map(),
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

describe('World', () => {
  let world: World;

  beforeEach(() => {
    world = new World();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe('constructor and initial state', () => {
    it('should initialize with null local player', () => {
      expect(world.localPlayerId).toBeNull();
    });

    it('should initialize arena with default values', () => {
      expect(world.arena.coreRadius).toBe(ARENA.CORE_RADIUS);
      expect(world.arena.innerRadius).toBe(ARENA.INNER_RADIUS);
      expect(world.arena.middleRadius).toBe(ARENA.MIDDLE_RADIUS);
      expect(world.arena.outerRadius).toBe(ARENA.OUTER_RADIUS);
      expect(world.arena.collapsePhase).toBe(0);
      expect(world.arena.scale).toBe(1.0);
      expect(world.arena.gravityWells).toEqual([]);
    });

    it('should initialize spectator mode as false', () => {
      expect(world.isSpectator).toBe(false);
      expect(world.spectateTargetId).toBeNull();
    });

    it('should return empty maps for players, projectiles, debris', () => {
      expect(world.getPlayers().size).toBe(0);
      expect(world.getProjectiles().size).toBe(0);
      expect(world.getDebris().size).toBe(0);
    });

    it('should return waiting as default match phase', () => {
      expect(world.getMatchPhase()).toBe('waiting');
    });
  });

  describe('updateFromState', () => {
    it('should update state from interpolated server state', () => {
      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', mass: 150 }));

      const state = createMockState({
        tick: 200,
        matchPhase: 'playing',
        matchTime: 120,
        players,
      });

      world.updateFromState(state);

      expect(world.getTick()).toBe(200);
      expect(world.getMatchPhase()).toBe('playing');
      expect(world.getMatchTime()).toBe(120);
      expect(world.getPlayers().size).toBe(1);
    });

    it('should update arena collapse phase', () => {
      const state = createMockState({
        arenaCollapsePhase: 4,
        arenaScale: 0.8,
      });

      world.updateFromState(state);

      expect(world.arena.collapsePhase).toBe(4);
      expect(world.arena.scale).toBe(0.8);
    });

    it('should update gravity wells', () => {
      const wells: InterpolatedGravityWell[] = [
        { id: 1, position: { x: 100, y: 100 }, mass: 5000, coreRadius: 25, bornTime: 0 },
        { id: 2, position: { x: -200, y: 300 }, mass: 3000, coreRadius: 20, bornTime: 0 },
      ];

      const state = createMockState({ gravityWells: wells });
      world.updateFromState(state);

      expect(world.arena.gravityWells).toHaveLength(2);
    });

    it('should detect new kills and update recent kills', () => {
      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', kills: 0 }));

      world.updateFromState(createMockState({ players }));

      // Player gets a kill
      players.set('player-1', createMockPlayer({ id: 'player-1', kills: 1 }));
      world.updateFromState(createMockState({ players }));

      expect(world.getKillEffectProgress('player-1')).toBeGreaterThan(0);
    });

    it('should track local player kill streak', () => {
      world.localPlayerId = 'player-1';

      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', kills: 0 }));
      world.updateFromState(createMockState({ players }));

      // Player gets 3 kills
      players.set('player-1', createMockPlayer({ id: 'player-1', kills: 3 }));
      world.updateFromState(createMockState({ players }));

      const stats = world.getSessionStats();
      expect(stats.killStreak).toBe(3);
      expect(stats.totalKills).toBe(3);
    });

    it('should detect death and create death effect', () => {
      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({
        id: 'player-1',
        alive: true,
        position: { x: 200, y: 300 },
        mass: 150,
        colorIndex: 5,
      }));

      world.updateFromState(createMockState({ players }));

      // Player dies
      players.set('player-1', createMockPlayer({ id: 'player-1', alive: false }));
      world.updateFromState(createMockState({ players }));

      const effects = world.getDeathEffects();
      expect(effects).toHaveLength(1);
      expect(effects[0].position.x).toBe(200);
      expect(effects[0].position.y).toBe(300);
    });

    it('should reset kill streak on local player death', () => {
      world.localPlayerId = 'player-1';

      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', kills: 5, alive: true }));
      world.updateFromState(createMockState({ players }));

      // Player dies
      players.set('player-1', createMockPlayer({ id: 'player-1', kills: 5, alive: false }));
      world.updateFromState(createMockState({ players }));

      const stats = world.getSessionStats();
      expect(stats.killStreak).toBe(0);
      expect(stats.totalDeaths).toBe(1);
    });

    it('should track best mass for local player', () => {
      world.localPlayerId = 'player-1';

      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', mass: 200, alive: true }));
      world.updateFromState(createMockState({ players }));

      expect(world.getSessionStats().bestMass).toBe(200);

      // Mass increases
      players.set('player-1', createMockPlayer({ id: 'player-1', mass: 350, alive: true }));
      world.updateFromState(createMockState({ players }));

      expect(world.getSessionStats().bestMass).toBe(350);

      // Mass decreases - best should stay
      players.set('player-1', createMockPlayer({ id: 'player-1', mass: 250, alive: true }));
      world.updateFromState(createMockState({ players }));

      expect(world.getSessionStats().bestMass).toBe(350);
    });

    it('should clean up old kill effects', () => {
      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', kills: 0 }));
      world.updateFromState(createMockState({ players }));

      players.set('player-1', createMockPlayer({ id: 'player-1', kills: 1 }));
      world.updateFromState(createMockState({ players }));

      expect(world.getKillEffectProgress('player-1')).toBeGreaterThan(0);

      // Advance time past effect duration (1500ms)
      vi.advanceTimersByTime(2000);
      world.updateFromState(createMockState({ players }));

      expect(world.getKillEffectProgress('player-1')).toBe(0);
    });

    it('should clean up old death effects', () => {
      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', alive: true }));
      world.updateFromState(createMockState({ players }));

      players.set('player-1', createMockPlayer({ id: 'player-1', alive: false }));
      world.updateFromState(createMockState({ players }));

      expect(world.getDeathEffects()).toHaveLength(1);

      // Advance time past death effect duration (800ms)
      vi.advanceTimersByTime(1000);
      world.updateFromState(createMockState({ players }));

      expect(world.getDeathEffects()).toHaveLength(0);
    });

    it('should filter out destroyed wells from snapshots', () => {
      const wells: InterpolatedGravityWell[] = [
        { id: 1, position: { x: 100, y: 100 }, mass: 5000, coreRadius: 25, bornTime: 0 },
        { id: 2, position: { x: -200, y: 300 }, mass: 3000, coreRadius: 20, bornTime: 0 },
      ];

      world.updateFromState(createMockState({ gravityWells: wells }));
      expect(world.arena.gravityWells).toHaveLength(2);

      // Mark well 1 as destroyed
      world.removeGravityWell(1);
      expect(world.arena.gravityWells).toHaveLength(1);

      // Apply same snapshot again - destroyed well should be filtered
      world.updateFromState(createMockState({ gravityWells: wells }));
      expect(world.arena.gravityWells).toHaveLength(1);
      expect(world.arena.gravityWells[0].id).toBe(2);
    });

    it('should clean up tracking for removed players', () => {
      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1' }));
      players.set('player-2', createMockPlayer({ id: 'player-2' }));

      world.updateFromState(createMockState({ players }));
      expect(world.getPlayers().size).toBe(2);

      // Remove player-2
      players.delete('player-2');
      world.updateFromState(createMockState({ players }));

      expect(world.getPlayers().size).toBe(1);
    });
  });

  describe('spectator mode', () => {
    it('should set spectator mode', () => {
      world.setSpectatorMode(true, 'player-1');

      expect(world.isSpectator).toBe(true);
      expect(world.spectateTargetId).toBe('player-1');
    });

    it('should set full map view when target is null', () => {
      world.setSpectatorMode(true, null);

      expect(world.isSpectator).toBe(true);
      expect(world.isFullMapView()).toBe(true);
    });

    it('should get spectate target player', () => {
      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', name: 'Target' }));
      world.updateFromState(createMockState({ players }));

      world.setSpectatorMode(true, 'player-1');

      const target = world.getSpectateTarget();
      expect(target).toBeDefined();
      expect(target?.name).toBe('Target');
    });

    it('should return undefined for non-existent spectate target', () => {
      world.setSpectatorMode(true, 'non-existent');

      expect(world.getSpectateTarget()).toBeUndefined();
    });
  });

  describe('player queries', () => {
    beforeEach(() => {
      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', mass: 200 }));
      players.set('player-2', createMockPlayer({ id: 'player-2', mass: 150 }));
      players.set('player-3', createMockPlayer({ id: 'player-3', mass: 100, alive: false }));
      world.updateFromState(createMockState({ players }));
    });

    it('should get all players', () => {
      expect(world.getPlayers().size).toBe(3);
    });

    it('should get specific player', () => {
      const player = world.getPlayer('player-1');
      expect(player?.mass).toBe(200);
    });

    it('should return undefined for non-existent player', () => {
      expect(world.getPlayer('non-existent')).toBeUndefined();
    });

    it('should get local player when set', () => {
      world.localPlayerId = 'player-2';
      const local = world.getLocalPlayer();
      expect(local?.mass).toBe(150);
    });

    it('should return undefined when no local player', () => {
      expect(world.getLocalPlayer()).toBeUndefined();
    });
  });

  describe('leaderboard', () => {
    beforeEach(() => {
      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', name: 'First', mass: 300, alive: true }));
      players.set('player-2', createMockPlayer({ id: 'player-2', name: 'Second', mass: 200, alive: true }));
      players.set('player-3', createMockPlayer({ id: 'player-3', name: 'Third', mass: 100, alive: true }));
      players.set('player-4', createMockPlayer({ id: 'player-4', name: 'Dead', mass: 500, alive: false }));
      world.updateFromState(createMockState({ players }));
    });

    it('should return leaderboard sorted by mass', () => {
      const leaderboard = world.getLeaderboard();

      expect(leaderboard).toHaveLength(3); // Only alive players
      expect(leaderboard[0].name).toBe('First');
      expect(leaderboard[1].name).toBe('Second');
      expect(leaderboard[2].name).toBe('Third');
    });

    it('should exclude dead players from leaderboard', () => {
      const leaderboard = world.getLeaderboard();
      expect(leaderboard.find(e => e.name === 'Dead')).toBeUndefined();
    });

    it('should get player placement', () => {
      expect(world.getPlayerPlacement('player-1')).toBe(1);
      expect(world.getPlayerPlacement('player-2')).toBe(2);
      expect(world.getPlayerPlacement('player-3')).toBe(3);
    });

    it('should return placement beyond list for dead players', () => {
      expect(world.getPlayerPlacement('player-4')).toBe(4);
    });
  });

  describe('player counts', () => {
    it('should use server total alive count', () => {
      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', alive: true }));

      world.updateFromState(createMockState({
        players,
        totalAlive: 50, // Server says 50 alive
        totalPlayers: 100,
      }));

      expect(world.getAlivePlayerCount()).toBe(50);
      expect(world.getTotalPlayerCount()).toBe(100);
    });

    it('should fallback to local count when server count is 0', () => {
      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', alive: true }));
      players.set('player-2', createMockPlayer({ id: 'player-2', alive: true }));
      players.set('player-3', createMockPlayer({ id: 'player-3', alive: false }));

      world.updateFromState(createMockState({
        players,
        totalAlive: 0,
        totalPlayers: 0,
      }));

      expect(world.getAlivePlayerCount()).toBe(2);
      expect(world.getTotalPlayerCount()).toBe(3);
    });
  });

  describe('massToRadius', () => {
    it('should calculate radius from mass using sqrt', () => {
      // radius = sqrt(mass) * RADIUS_SCALE
      expect(world.massToRadius(100)).toBeCloseTo(10 * MASS.RADIUS_SCALE);
      expect(world.massToRadius(400)).toBeCloseTo(20 * MASS.RADIUS_SCALE);
    });

    it('should handle small mass values', () => {
      expect(world.massToRadius(1)).toBeCloseTo(1 * MASS.RADIUS_SCALE);
    });

    it('should handle zero mass', () => {
      expect(world.massToRadius(0)).toBe(0);
    });
  });

  describe('getPlayerColor', () => {
    it('should return color at index', () => {
      expect(world.getPlayerColor(0)).toBe(PLAYER_COLORS[0]);
      expect(world.getPlayerColor(5)).toBe(PLAYER_COLORS[5]);
    });

    it('should wrap around for indices beyond array length', () => {
      expect(world.getPlayerColor(20)).toBe(PLAYER_COLORS[0]);
      expect(world.getPlayerColor(25)).toBe(PLAYER_COLORS[5]);
    });
  });

  describe('getPlayerName', () => {
    it('should return name from player snapshot', () => {
      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', name: 'SnapshotName' }));
      world.updateFromState(createMockState({ players }));

      expect(world.getPlayerName('player-1')).toBe('SnapshotName');
    });

    it('should fallback to cached name', () => {
      world.setPlayerName('player-2', 'CachedName');
      expect(world.getPlayerName('player-2')).toBe('CachedName');
    });

    it('should return truncated ID as fallback', () => {
      const name = world.getPlayerName('12345678-abcd-efgh-ijkl-mnopqrstuvwx');
      expect(name).toBe('Player 1234');
    });
  });

  describe('collision effects', () => {
    it('should add collision effect', () => {
      world.addCollisionEffect({ x: 100, y: 200 }, 0.75, '#ff0000');

      const effects = world.getCollisionEffects();
      expect(effects).toHaveLength(1);
      expect(effects[0].position.x).toBe(100);
      expect(effects[0].intensity).toBe(0.75);
      expect(effects[0].color).toBe('#ff0000');
    });

    it('should limit to MAX_COLLISION_EFFECTS (10)', () => {
      for (let i = 0; i < 15; i++) {
        world.addCollisionEffect({ x: i, y: i }, 1, '#ffffff');
      }

      expect(world.getCollisionEffects()).toHaveLength(10);
    });

    it('should remove oldest when limit exceeded', () => {
      for (let i = 0; i < 12; i++) {
        world.addCollisionEffect({ x: i * 10, y: 0 }, 1, '#ffffff');
      }

      const effects = world.getCollisionEffects();
      expect(effects[0].position.x).toBe(20); // First two removed
    });

    it('should clean up old collision effects over time', () => {
      world.addCollisionEffect({ x: 100, y: 200 }, 0.5, '#ff0000');
      expect(world.getCollisionEffects()).toHaveLength(1);

      // Advance past collision effect duration (300ms)
      vi.advanceTimersByTime(400);
      world.updateFromState(createMockState());

      expect(world.getCollisionEffects()).toHaveLength(0);
    });
  });

  describe('gravity wave effects', () => {
    it('should add gravity wave effect', () => {
      world.addGravityWaveEffect({ x: 150, y: 250 }, 1.5, 42);

      const effects = world.getGravityWaveEffects();
      expect(effects).toHaveLength(1);
      expect(effects[0].position.x).toBe(150);
      expect(effects[0].strength).toBe(1.5);
    });

    it('should limit to MAX_WAVE_EFFECTS (10)', () => {
      for (let i = 0; i < 15; i++) {
        world.addGravityWaveEffect({ x: i, y: i }, 1, i);
      }

      expect(world.getGravityWaveEffects()).toHaveLength(10);
    });

    it('should remove charging state when explosion happens', () => {
      world.addChargingWell({ x: 100, y: 100 }, 42);
      expect(world.getChargingWells()).toHaveLength(1);

      world.addGravityWaveEffect({ x: 100, y: 100 }, 1.5, 42);
      expect(world.getChargingWells()).toHaveLength(0);
    });

    it('should calculate progress over 6 seconds', () => {
      world.addGravityWaveEffect({ x: 100, y: 100 }, 1, 1);

      let effects = world.getGravityWaveEffects();
      expect(effects[0].progress).toBeCloseTo(0);

      vi.advanceTimersByTime(3000);
      effects = world.getGravityWaveEffects();
      expect(effects[0].progress).toBeCloseTo(0.5);

      vi.advanceTimersByTime(3000);
      effects = world.getGravityWaveEffects();
      expect(effects).toHaveLength(0); // Expired
    });
  });

  describe('charging wells', () => {
    it('should add charging well', () => {
      world.addChargingWell({ x: 200, y: 300 }, 99);

      const wells = world.getChargingWells();
      expect(wells).toHaveLength(1);
      expect(wells[0].wellId).toBe(99);
    });

    it('should replace existing charging for same well', () => {
      world.addChargingWell({ x: 100, y: 100 }, 42);
      world.addChargingWell({ x: 200, y: 200 }, 42);

      const wells = world.getChargingWells();
      expect(wells).toHaveLength(1);
      expect(wells[0].position.x).toBe(200);
    });

    it('should calculate progress over 2 seconds', () => {
      world.addChargingWell({ x: 100, y: 100 }, 1);

      let wells = world.getChargingWells();
      expect(wells[0].progress).toBeCloseTo(0);

      vi.advanceTimersByTime(1000);
      wells = world.getChargingWells();
      expect(wells[0].progress).toBeCloseTo(0.5);

      vi.advanceTimersByTime(1000);
      wells = world.getChargingWells();
      expect(wells).toHaveLength(0); // Expired
    });
  });

  describe('removeGravityWell', () => {
    it('should remove well from arena', () => {
      const wells: InterpolatedGravityWell[] = [
        { id: 1, position: { x: 100, y: 100 }, mass: 5000, coreRadius: 25, bornTime: 0 },
        { id: 2, position: { x: 200, y: 200 }, mass: 3000, coreRadius: 20, bornTime: 0 },
      ];
      world.updateFromState(createMockState({ gravityWells: wells }));

      world.removeGravityWell(1);

      expect(world.arena.gravityWells).toHaveLength(1);
      expect(world.arena.gravityWells[0].id).toBe(2);
    });

    it('should remove charging state for the well', () => {
      world.addChargingWell({ x: 100, y: 100 }, 42);
      expect(world.getChargingWells()).toHaveLength(1);

      world.removeGravityWell(42);
      expect(world.getChargingWells()).toHaveLength(0);
    });
  });

  describe('applyLocalPrediction', () => {
    it('should update local player position and velocity', () => {
      world.localPlayerId = 'player-1';

      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({
        id: 'player-1',
        position: { x: 100, y: 100 },
        velocity: { x: 0, y: 0 },
        alive: true,
      }));
      world.updateFromState(createMockState({ players }));

      world.applyLocalPrediction({ x: 150, y: 200 }, { x: 10, y: 20 });

      const player = world.getLocalPlayer();
      expect(player?.position.x).toBe(150);
      expect(player?.position.y).toBe(200);
      expect(player?.velocity.x).toBe(10);
      expect(player?.velocity.y).toBe(20);
    });

    it('should not update dead player', () => {
      world.localPlayerId = 'player-1';

      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({
        id: 'player-1',
        position: { x: 100, y: 100 },
        alive: false,
      }));
      world.updateFromState(createMockState({ players }));

      world.applyLocalPrediction({ x: 999, y: 999 }, { x: 0, y: 0 });

      const player = world.getLocalPlayer();
      expect(player?.position.x).toBe(100);
    });

    it('should handle no local player', () => {
      expect(() => world.applyLocalPrediction({ x: 0, y: 0 }, { x: 0, y: 0 })).not.toThrow();
    });
  });

  describe('isLocalPlayerAlive', () => {
    it('should return true when local player is alive', () => {
      world.localPlayerId = 'player-1';

      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', alive: true }));
      world.updateFromState(createMockState({ players }));

      expect(world.isLocalPlayerAlive()).toBe(true);
    });

    it('should return false when local player is dead', () => {
      world.localPlayerId = 'player-1';

      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', alive: false }));
      world.updateFromState(createMockState({ players }));

      expect(world.isLocalPlayerAlive()).toBe(false);
    });

    it('should return false when no local player', () => {
      expect(world.isLocalPlayerAlive()).toBe(false);
    });
  });

  describe('getArenaSafeRadius', () => {
    it('should return safe radius from state', () => {
      world.updateFromState(createMockState({ arenaSafeRadius: 450 }));
      expect(world.getArenaSafeRadius()).toBe(450);
    });

    it('should return default when no state', () => {
      expect(world.getArenaSafeRadius()).toBe(ARENA.OUTER_RADIUS);
    });
  });

  describe('getDensityGrid and getNotablePlayers', () => {
    it('should return density grid from state', () => {
      const densityGrid = [1, 2, 3, 4, 5];
      world.updateFromState(createMockState({ densityGrid }));

      expect(world.getDensityGrid()).toEqual(densityGrid);
    });

    it('should return empty array when no state', () => {
      expect(world.getDensityGrid()).toEqual([]);
    });

    it('should return notable players from state', () => {
      const notablePlayers: InterpolatedNotablePlayer[] = [
        { id: 'player-1', position: { x: 100, y: 100 }, mass: 500, colorIndex: 5 },
      ];
      world.updateFromState(createMockState({ notablePlayers }));

      expect(world.getNotablePlayers()).toHaveLength(1);
    });
  });

  describe('reset', () => {
    it('should reset all state to initial values', () => {
      world.localPlayerId = 'player-1';
      world.isSpectator = true;

      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', kills: 5 }));
      world.updateFromState(createMockState({ players }));

      world.addCollisionEffect({ x: 100, y: 100 }, 1, '#fff');
      world.addGravityWaveEffect({ x: 100, y: 100 }, 1, 1);
      world.addChargingWell({ x: 100, y: 100 }, 1);

      world.reset();

      expect(world.localPlayerId).toBeNull();
      expect(world.getPlayers().size).toBe(0);
      expect(world.getCollisionEffects()).toHaveLength(0);
      expect(world.getGravityWaveEffects()).toHaveLength(0);
      expect(world.getChargingWells()).toHaveLength(0);
      expect(world.arena.collapsePhase).toBe(0);
      expect(world.arena.gravityWells).toHaveLength(0);
    });

    it('should reset session stats', () => {
      world.localPlayerId = 'player-1';

      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', kills: 10, mass: 500, alive: true }));
      world.updateFromState(createMockState({ players }));

      world.reset();

      const stats = world.getSessionStats();
      expect(stats.bestMass).toBe(0);
      expect(stats.killStreak).toBe(0);
      expect(stats.totalKills).toBe(0);
      expect(stats.totalDeaths).toBe(0);
    });
  });

  describe('getSessionStats', () => {
    it('should calculate time alive when local player is alive', () => {
      world.localPlayerId = 'player-1';

      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', alive: true }));
      world.updateFromState(createMockState({ players }));

      vi.advanceTimersByTime(5000);

      const stats = world.getSessionStats();
      expect(stats.timeAlive).toBeGreaterThanOrEqual(5000);
    });

    it('should return 0 time alive when player is dead', () => {
      world.localPlayerId = 'player-1';

      const players = new Map<string, InterpolatedPlayer>();
      players.set('player-1', createMockPlayer({ id: 'player-1', alive: false }));
      world.updateFromState(createMockState({ players }));

      const stats = world.getSessionStats();
      expect(stats.timeAlive).toBe(0);
    });
  });
});
