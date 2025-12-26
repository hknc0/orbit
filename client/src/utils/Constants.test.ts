import { describe, it, expect } from 'vitest';
import {
  PHYSICS,
  MASS,
  BOOST,
  EJECT,
  ARENA,
  SPAWN,
  MATCH,
  NETWORK,
  PLAYER_COLORS,
  massToThrustMultiplier,
} from './Constants';

describe('Constants', () => {
  describe('PHYSICS', () => {
    it('should have correct gravitational constant', () => {
      expect(PHYSICS.G).toBe(6.67);
    });

    it('should have positive central mass', () => {
      expect(PHYSICS.CENTRAL_MASS).toBe(10_000);
      expect(PHYSICS.CENTRAL_MASS).toBeGreaterThan(0);
    });

    it('should have drag coefficient between 0 and 1', () => {
      expect(PHYSICS.DRAG).toBe(0.002);
      expect(PHYSICS.DRAG).toBeGreaterThan(0);
      expect(PHYSICS.DRAG).toBeLessThan(1);
    });

    it('should have positive max velocity', () => {
      expect(PHYSICS.MAX_VELOCITY).toBe(500);
      expect(PHYSICS.MAX_VELOCITY).toBeGreaterThan(0);
    });

    it('should have tick rate of 30', () => {
      expect(PHYSICS.TICK_RATE).toBe(30);
    });

    it('should have DT equal to 1/TICK_RATE', () => {
      expect(PHYSICS.DT).toBeCloseTo(1 / 30);
    });
  });

  describe('MASS', () => {
    it('should have starting mass of 100', () => {
      expect(MASS.STARTING).toBe(100);
    });

    it('should have minimum mass of 10', () => {
      expect(MASS.MINIMUM).toBe(10);
    });

    it('should have minimum less than starting', () => {
      expect(MASS.MINIMUM).toBeLessThan(MASS.STARTING);
    });

    it('should have absorption cap greater than starting', () => {
      expect(MASS.ABSORPTION_CAP).toBe(200);
      expect(MASS.ABSORPTION_CAP).toBeGreaterThan(MASS.STARTING);
    });

    it('should have absorption rate between 0 and 1', () => {
      expect(MASS.ABSORPTION_RATE).toBe(0.7);
      expect(MASS.ABSORPTION_RATE).toBeGreaterThan(0);
      expect(MASS.ABSORPTION_RATE).toBeLessThanOrEqual(1);
    });

    it('should have positive radius scale', () => {
      expect(MASS.RADIUS_SCALE).toBe(2.0);
      expect(MASS.RADIUS_SCALE).toBeGreaterThan(0);
    });
  });

  describe('BOOST', () => {
    it('should have positive base thrust', () => {
      expect(BOOST.BASE_THRUST).toBe(200);
      expect(BOOST.BASE_THRUST).toBeGreaterThan(0);
    });

    it('should have positive base cost', () => {
      expect(BOOST.BASE_COST).toBe(2);
      expect(BOOST.BASE_COST).toBeGreaterThan(0);
    });

    it('should have speed reference mass equal to starting mass', () => {
      expect(BOOST.SPEED_REFERENCE_MASS).toBe(100);
      expect(BOOST.SPEED_REFERENCE_MASS).toBe(MASS.STARTING);
    });

    it('should have speed scaling exponent of 0.5 (sqrt)', () => {
      expect(BOOST.SPEED_SCALING_EXPONENT).toBe(0.5);
    });

    it('should have min multiplier less than max', () => {
      expect(BOOST.SPEED_MIN_MULTIPLIER).toBe(0.25);
      expect(BOOST.SPEED_MAX_MULTIPLIER).toBe(3.5);
      expect(BOOST.SPEED_MIN_MULTIPLIER).toBeLessThan(BOOST.SPEED_MAX_MULTIPLIER);
    });
  });

  describe('EJECT', () => {
    it('should have min charge time less than max', () => {
      expect(EJECT.MIN_CHARGE_TIME).toBe(0.2);
      expect(EJECT.MAX_CHARGE_TIME).toBe(1.0);
      expect(EJECT.MIN_CHARGE_TIME).toBeLessThan(EJECT.MAX_CHARGE_TIME);
    });

    it('should have positive min mass', () => {
      expect(EJECT.MIN_MASS).toBe(10);
      expect(EJECT.MIN_MASS).toBeGreaterThan(0);
    });

    it('should have max mass ratio between 0 and 1', () => {
      expect(EJECT.MAX_MASS_RATIO).toBe(0.5);
      expect(EJECT.MAX_MASS_RATIO).toBeGreaterThan(0);
      expect(EJECT.MAX_MASS_RATIO).toBeLessThanOrEqual(1);
    });

    it('should have min velocity less than max', () => {
      expect(EJECT.MIN_VELOCITY).toBe(100);
      expect(EJECT.MAX_VELOCITY).toBe(300);
      expect(EJECT.MIN_VELOCITY).toBeLessThan(EJECT.MAX_VELOCITY);
    });

    it('should have positive lifetime', () => {
      expect(EJECT.LIFETIME).toBe(8);
      expect(EJECT.LIFETIME).toBeGreaterThan(0);
    });
  });

  describe('ARENA', () => {
    it('should have zones in increasing order', () => {
      expect(ARENA.CORE_RADIUS).toBe(50);
      expect(ARENA.INNER_RADIUS).toBe(200);
      expect(ARENA.MIDDLE_RADIUS).toBe(400);
      expect(ARENA.OUTER_RADIUS).toBe(600);
      expect(ARENA.ESCAPE_RADIUS).toBe(800);

      expect(ARENA.CORE_RADIUS).toBeLessThan(ARENA.INNER_RADIUS);
      expect(ARENA.INNER_RADIUS).toBeLessThan(ARENA.MIDDLE_RADIUS);
      expect(ARENA.MIDDLE_RADIUS).toBeLessThan(ARENA.OUTER_RADIUS);
      expect(ARENA.OUTER_RADIUS).toBeLessThan(ARENA.ESCAPE_RADIUS);
    });

    it('should have positive collapse interval', () => {
      expect(ARENA.COLLAPSE_INTERVAL).toBe(30);
      expect(ARENA.COLLAPSE_INTERVAL).toBeGreaterThan(0);
    });

    it('should have positive collapse phases', () => {
      expect(ARENA.COLLAPSE_PHASES).toBe(8);
      expect(ARENA.COLLAPSE_PHASES).toBeGreaterThan(0);
    });

    it('should have positive collapse duration', () => {
      expect(ARENA.COLLAPSE_DURATION).toBe(3);
      expect(ARENA.COLLAPSE_DURATION).toBeGreaterThan(0);
    });
  });

  describe('SPAWN', () => {
    it('should have positive protection duration', () => {
      expect(SPAWN.PROTECTION_DURATION).toBe(3);
      expect(SPAWN.PROTECTION_DURATION).toBeGreaterThan(0);
    });

    it('should have zone min less than max', () => {
      expect(SPAWN.ZONE_MIN).toBe(250);
      expect(SPAWN.ZONE_MAX).toBe(350);
      expect(SPAWN.ZONE_MIN).toBeLessThan(SPAWN.ZONE_MAX);
    });

    it('should spawn within inner and middle zones', () => {
      expect(SPAWN.ZONE_MIN).toBeGreaterThan(ARENA.INNER_RADIUS);
      expect(SPAWN.ZONE_MAX).toBeLessThan(ARENA.MIDDLE_RADIUS);
    });
  });

  describe('MATCH', () => {
    it('should have positive match duration', () => {
      expect(MATCH.DURATION).toBe(300);
      expect(MATCH.DURATION).toBeGreaterThan(0);
    });

    it('should have positive countdown', () => {
      expect(MATCH.COUNTDOWN).toBe(3);
      expect(MATCH.COUNTDOWN).toBeGreaterThan(0);
    });
  });

  describe('NETWORK', () => {
    it('should have positive interpolation delay', () => {
      expect(NETWORK.INTERPOLATION_DELAY_MS).toBe(100);
      expect(NETWORK.INTERPOLATION_DELAY_MS).toBeGreaterThan(0);
    });

    it('should have positive snapshot buffer size', () => {
      expect(NETWORK.SNAPSHOT_BUFFER_SIZE).toBe(32);
      expect(NETWORK.SNAPSHOT_BUFFER_SIZE).toBeGreaterThan(0);
    });

    it('should have positive input buffer size', () => {
      expect(NETWORK.INPUT_BUFFER_SIZE).toBe(64);
      expect(NETWORK.INPUT_BUFFER_SIZE).toBeGreaterThan(0);
    });

    it('should have positive reconnect attempts', () => {
      expect(NETWORK.RECONNECT_ATTEMPTS).toBe(3);
      expect(NETWORK.RECONNECT_ATTEMPTS).toBeGreaterThan(0);
    });

    it('should have positive ping interval', () => {
      expect(NETWORK.PING_INTERVAL_MS).toBe(1000);
      expect(NETWORK.PING_INTERVAL_MS).toBeGreaterThan(0);
    });
  });

  describe('PLAYER_COLORS', () => {
    it('should have 20 colors', () => {
      expect(PLAYER_COLORS).toHaveLength(20);
    });

    it('should have valid hex color format for all colors', () => {
      const hexColorRegex = /^#[0-9a-fA-F]{6}$/;
      PLAYER_COLORS.forEach((color, index) => {
        expect(color).toMatch(hexColorRegex);
      });
    });

    it('should have unique colors', () => {
      const uniqueColors = new Set(PLAYER_COLORS);
      expect(uniqueColors.size).toBe(PLAYER_COLORS.length);
    });

    it('should start with red', () => {
      expect(PLAYER_COLORS[0]).toBe('#ef4444');
    });

    it('should end with white', () => {
      expect(PLAYER_COLORS[19]).toBe('#ffffff');
    });
  });

  describe('massToThrustMultiplier()', () => {
    it('should return 1.0 at reference mass (100)', () => {
      const multiplier = massToThrustMultiplier(100);
      expect(multiplier).toBeCloseTo(1.0);
    });

    it('should return higher multiplier for smaller mass (faster)', () => {
      const multiplier = massToThrustMultiplier(50);
      expect(multiplier).toBeGreaterThan(1.0);
    });

    it('should return lower multiplier for larger mass (slower)', () => {
      const multiplier = massToThrustMultiplier(200);
      expect(multiplier).toBeLessThan(1.0);
    });

    it('should clamp to minimum multiplier for very large mass', () => {
      const multiplier = massToThrustMultiplier(10000);
      expect(multiplier).toBe(BOOST.SPEED_MIN_MULTIPLIER);
    });

    it('should clamp mass to minimum when very small', () => {
      // mass=1 is clamped to MASS.MINIMUM (10), so result is sqrt(100/10) = sqrt(10) ~ 3.16
      const multiplier = massToThrustMultiplier(1);
      const expected = Math.sqrt(BOOST.SPEED_REFERENCE_MASS / MASS.MINIMUM);
      expect(multiplier).toBeCloseTo(expected, 5);
    });

    it('should use minimum mass when mass is below minimum', () => {
      const multiplier1 = massToThrustMultiplier(5);
      const multiplier2 = massToThrustMultiplier(MASS.MINIMUM);
      // Both should be the same since 5 < MINIMUM gets clamped to MINIMUM
      expect(multiplier1).toBe(multiplier2);
    });

    it('should use minimum mass when mass is zero', () => {
      const multiplier = massToThrustMultiplier(0);
      // sqrt(100 / 10) = sqrt(10) ≈ 3.16
      expect(multiplier).toBeCloseTo(Math.sqrt(100 / MASS.MINIMUM));
    });

    it('should use minimum mass when mass is negative', () => {
      const multiplier = massToThrustMultiplier(-50);
      const expectedMultiplier = massToThrustMultiplier(MASS.MINIMUM);
      expect(multiplier).toBe(expectedMultiplier);
    });

    it('should follow sqrt curve', () => {
      // At mass 25 (4x smaller than 100), multiplier should be 2x (sqrt of 4)
      const multiplier = massToThrustMultiplier(25);
      expect(multiplier).toBeCloseTo(2.0);
    });

    it('should follow sqrt curve for larger masses', () => {
      // At mass 400 (4x larger than 100), multiplier should be 0.5 (sqrt of 0.25)
      const multiplier = massToThrustMultiplier(400);
      expect(multiplier).toBeCloseTo(0.5);
    });

    it('should be monotonically decreasing with mass', () => {
      const masses = [10, 25, 50, 100, 200, 400, 800];
      const multipliers = masses.map(massToThrustMultiplier);

      for (let i = 1; i < multipliers.length; i++) {
        expect(multipliers[i]).toBeLessThanOrEqual(multipliers[i - 1]);
      }
    });

    it('should always be within min and max bounds', () => {
      const testMasses = [1, 5, 10, 50, 100, 500, 1000, 5000, 10000];

      testMasses.forEach((mass) => {
        const multiplier = massToThrustMultiplier(mass);
        expect(multiplier).toBeGreaterThanOrEqual(BOOST.SPEED_MIN_MULTIPLIER);
        expect(multiplier).toBeLessThanOrEqual(BOOST.SPEED_MAX_MULTIPLIER);
      });
    });
  });
});
