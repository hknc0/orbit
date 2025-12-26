// Tests for StateSync adaptive interpolation

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { StateSync } from './StateSync';
import { NETWORK, PHYSICS } from '@/utils/Constants';
import type { GameSnapshot, MatchPhase } from './Protocol';

const { ADAPTIVE_INTERPOLATION } = NETWORK;

// Helper to create minimal valid snapshot
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
