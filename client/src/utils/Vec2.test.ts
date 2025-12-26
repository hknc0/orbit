import { describe, it, expect } from 'vitest';
import { Vec2, vec2Lerp } from './Vec2';

describe('Vec2', () => {
  describe('constructor', () => {
    it('should create vector with default values (0, 0)', () => {
      const v = new Vec2();
      expect(v.x).toBe(0);
      expect(v.y).toBe(0);
    });

    it('should create vector with specified values', () => {
      const v = new Vec2(3, 4);
      expect(v.x).toBe(3);
      expect(v.y).toBe(4);
    });

    it('should handle negative values', () => {
      const v = new Vec2(-5, -10);
      expect(v.x).toBe(-5);
      expect(v.y).toBe(-10);
    });

    it('should handle floating point values', () => {
      const v = new Vec2(1.5, 2.7);
      expect(v.x).toBeCloseTo(1.5);
      expect(v.y).toBeCloseTo(2.7);
    });
  });

  describe('static zero()', () => {
    it('should return a zero vector', () => {
      const v = Vec2.zero();
      expect(v.x).toBe(0);
      expect(v.y).toBe(0);
    });

    it('should return a new instance each time', () => {
      const v1 = Vec2.zero();
      const v2 = Vec2.zero();
      expect(v1).not.toBe(v2);
    });
  });

  describe('static fromAngle()', () => {
    it('should create unit vector from angle 0 (pointing right)', () => {
      const v = Vec2.fromAngle(0);
      expect(v.x).toBeCloseTo(1);
      expect(v.y).toBeCloseTo(0);
    });

    it('should create unit vector from angle PI/2 (pointing up)', () => {
      const v = Vec2.fromAngle(Math.PI / 2);
      expect(v.x).toBeCloseTo(0);
      expect(v.y).toBeCloseTo(1);
    });

    it('should create unit vector from angle PI (pointing left)', () => {
      const v = Vec2.fromAngle(Math.PI);
      expect(v.x).toBeCloseTo(-1);
      expect(v.y).toBeCloseTo(0);
    });

    it('should create unit vector from angle 3*PI/2 (pointing down)', () => {
      const v = Vec2.fromAngle((3 * Math.PI) / 2);
      expect(v.x).toBeCloseTo(0);
      expect(v.y).toBeCloseTo(-1);
    });

    it('should create vector with specified length', () => {
      const v = Vec2.fromAngle(0, 5);
      expect(v.x).toBeCloseTo(5);
      expect(v.y).toBeCloseTo(0);
    });

    it('should handle negative length', () => {
      const v = Vec2.fromAngle(0, -3);
      expect(v.x).toBeCloseTo(-3);
      expect(v.y).toBeCloseTo(0);
    });

    it('should create diagonal vector at PI/4', () => {
      const v = Vec2.fromAngle(Math.PI / 4, Math.sqrt(2));
      expect(v.x).toBeCloseTo(1);
      expect(v.y).toBeCloseTo(1);
    });
  });

  describe('clone()', () => {
    it('should create a copy with same values', () => {
      const v1 = new Vec2(3, 4);
      const v2 = v1.clone();
      expect(v2.x).toBe(3);
      expect(v2.y).toBe(4);
    });

    it('should return a new instance', () => {
      const v1 = new Vec2(3, 4);
      const v2 = v1.clone();
      expect(v2).not.toBe(v1);
    });

    it('should not be affected by changes to original', () => {
      const v1 = new Vec2(3, 4);
      const v2 = v1.clone();
      v1.x = 100;
      expect(v2.x).toBe(3);
    });
  });

  describe('set()', () => {
    it('should set new values', () => {
      const v = new Vec2(1, 2);
      v.set(5, 6);
      expect(v.x).toBe(5);
      expect(v.y).toBe(6);
    });

    it('should return this for chaining', () => {
      const v = new Vec2();
      const result = v.set(3, 4);
      expect(result).toBe(v);
    });
  });

  describe('copy()', () => {
    it('should copy values from another vector', () => {
      const v1 = new Vec2(1, 2);
      const v2 = new Vec2(5, 6);
      v1.copy(v2);
      expect(v1.x).toBe(5);
      expect(v1.y).toBe(6);
    });

    it('should return this for chaining', () => {
      const v1 = new Vec2();
      const v2 = new Vec2(3, 4);
      const result = v1.copy(v2);
      expect(result).toBe(v1);
    });

    it('should not modify the source vector', () => {
      const v1 = new Vec2(1, 2);
      const v2 = new Vec2(5, 6);
      v1.copy(v2);
      expect(v2.x).toBe(5);
      expect(v2.y).toBe(6);
    });
  });

  describe('add()', () => {
    it('should add vectors correctly', () => {
      const v1 = new Vec2(1, 2);
      const v2 = new Vec2(3, 4);
      v1.add(v2);
      expect(v1.x).toBe(4);
      expect(v1.y).toBe(6);
    });

    it('should return this for chaining', () => {
      const v1 = new Vec2(1, 2);
      const v2 = new Vec2(3, 4);
      const result = v1.add(v2);
      expect(result).toBe(v1);
    });

    it('should handle negative values', () => {
      const v1 = new Vec2(5, 5);
      const v2 = new Vec2(-3, -2);
      v1.add(v2);
      expect(v1.x).toBe(2);
      expect(v1.y).toBe(3);
    });

    it('should handle adding zero vector', () => {
      const v1 = new Vec2(3, 4);
      const v2 = Vec2.zero();
      v1.add(v2);
      expect(v1.x).toBe(3);
      expect(v1.y).toBe(4);
    });
  });

  describe('sub()', () => {
    it('should subtract vectors correctly', () => {
      const v1 = new Vec2(5, 7);
      const v2 = new Vec2(2, 3);
      v1.sub(v2);
      expect(v1.x).toBe(3);
      expect(v1.y).toBe(4);
    });

    it('should return this for chaining', () => {
      const v1 = new Vec2(5, 7);
      const v2 = new Vec2(2, 3);
      const result = v1.sub(v2);
      expect(result).toBe(v1);
    });

    it('should handle resulting negative values', () => {
      const v1 = new Vec2(2, 3);
      const v2 = new Vec2(5, 7);
      v1.sub(v2);
      expect(v1.x).toBe(-3);
      expect(v1.y).toBe(-4);
    });
  });

  describe('scale()', () => {
    it('should scale vector by positive scalar', () => {
      const v = new Vec2(3, 4);
      v.scale(2);
      expect(v.x).toBe(6);
      expect(v.y).toBe(8);
    });

    it('should scale by fractional value', () => {
      const v = new Vec2(4, 6);
      v.scale(0.5);
      expect(v.x).toBe(2);
      expect(v.y).toBe(3);
    });

    it('should scale by negative value', () => {
      const v = new Vec2(3, 4);
      v.scale(-1);
      expect(v.x).toBe(-3);
      expect(v.y).toBe(-4);
    });

    it('should scale by zero', () => {
      const v = new Vec2(3, 4);
      v.scale(0);
      expect(v.x).toBe(0);
      expect(v.y).toBe(0);
    });

    it('should return this for chaining', () => {
      const v = new Vec2(3, 4);
      const result = v.scale(2);
      expect(result).toBe(v);
    });
  });

  describe('length()', () => {
    it('should return correct length for 3-4-5 triangle', () => {
      const v = new Vec2(3, 4);
      expect(v.length()).toBe(5);
    });

    it('should return 0 for zero vector', () => {
      const v = Vec2.zero();
      expect(v.length()).toBe(0);
    });

    it('should return 1 for unit vector along x', () => {
      const v = new Vec2(1, 0);
      expect(v.length()).toBe(1);
    });

    it('should return 1 for unit vector along y', () => {
      const v = new Vec2(0, 1);
      expect(v.length()).toBe(1);
    });

    it('should handle negative components', () => {
      const v = new Vec2(-3, -4);
      expect(v.length()).toBe(5);
    });

    it('should return sqrt(2) for (1, 1)', () => {
      const v = new Vec2(1, 1);
      expect(v.length()).toBeCloseTo(Math.sqrt(2));
    });
  });

  describe('lengthSq()', () => {
    it('should return squared length for 3-4-5 triangle', () => {
      const v = new Vec2(3, 4);
      expect(v.lengthSq()).toBe(25);
    });

    it('should return 0 for zero vector', () => {
      const v = Vec2.zero();
      expect(v.lengthSq()).toBe(0);
    });

    it('should return 2 for (1, 1)', () => {
      const v = new Vec2(1, 1);
      expect(v.lengthSq()).toBe(2);
    });

    it('should be more efficient than length() for comparisons', () => {
      const v = new Vec2(3, 4);
      // lengthSq avoids sqrt, so it should equal length squared
      expect(v.lengthSq()).toBe(v.length() * v.length());
    });
  });

  describe('normalize()', () => {
    it('should normalize to unit length', () => {
      const v = new Vec2(3, 4);
      v.normalize();
      expect(v.length()).toBeCloseTo(1);
    });

    it('should maintain direction', () => {
      const v = new Vec2(3, 4);
      const originalAngle = v.angle();
      v.normalize();
      expect(v.angle()).toBeCloseTo(originalAngle);
    });

    it('should not modify zero vector', () => {
      const v = Vec2.zero();
      v.normalize();
      expect(v.x).toBe(0);
      expect(v.y).toBe(0);
    });

    it('should return this for chaining', () => {
      const v = new Vec2(3, 4);
      const result = v.normalize();
      expect(result).toBe(v);
    });

    it('should handle already normalized vector', () => {
      const v = new Vec2(1, 0);
      v.normalize();
      expect(v.x).toBeCloseTo(1);
      expect(v.y).toBeCloseTo(0);
    });

    it('should correctly normalize (3, 4) to (0.6, 0.8)', () => {
      const v = new Vec2(3, 4);
      v.normalize();
      expect(v.x).toBeCloseTo(0.6);
      expect(v.y).toBeCloseTo(0.8);
    });
  });

  describe('dot()', () => {
    it('should return correct dot product', () => {
      const v1 = new Vec2(2, 3);
      const v2 = new Vec2(4, 5);
      expect(v1.dot(v2)).toBe(2 * 4 + 3 * 5); // 8 + 15 = 23
    });

    it('should return 0 for perpendicular vectors', () => {
      const v1 = new Vec2(1, 0);
      const v2 = new Vec2(0, 1);
      expect(v1.dot(v2)).toBe(0);
    });

    it('should return lengthSq for same vector', () => {
      const v = new Vec2(3, 4);
      expect(v.dot(v)).toBe(v.lengthSq());
    });

    it('should be negative for opposite directions', () => {
      const v1 = new Vec2(1, 0);
      const v2 = new Vec2(-1, 0);
      expect(v1.dot(v2)).toBe(-1);
    });

    it('should be commutative', () => {
      const v1 = new Vec2(2, 3);
      const v2 = new Vec2(4, 5);
      expect(v1.dot(v2)).toBe(v2.dot(v1));
    });
  });

  describe('angle()', () => {
    it('should return 0 for vector pointing right', () => {
      const v = new Vec2(1, 0);
      expect(v.angle()).toBe(0);
    });

    it('should return PI/2 for vector pointing up', () => {
      const v = new Vec2(0, 1);
      expect(v.angle()).toBeCloseTo(Math.PI / 2);
    });

    it('should return PI for vector pointing left', () => {
      const v = new Vec2(-1, 0);
      expect(v.angle()).toBeCloseTo(Math.PI);
    });

    it('should return -PI/2 for vector pointing down', () => {
      const v = new Vec2(0, -1);
      expect(v.angle()).toBeCloseTo(-Math.PI / 2);
    });

    it('should return PI/4 for (1, 1)', () => {
      const v = new Vec2(1, 1);
      expect(v.angle()).toBeCloseTo(Math.PI / 4);
    });

    it('should handle zero vector (returns 0)', () => {
      const v = Vec2.zero();
      expect(v.angle()).toBe(0);
    });
  });

  describe('distanceTo()', () => {
    it('should return correct distance between two points', () => {
      const v1 = new Vec2(0, 0);
      const v2 = new Vec2(3, 4);
      expect(v1.distanceTo(v2)).toBe(5);
    });

    it('should return 0 for same point', () => {
      const v1 = new Vec2(3, 4);
      const v2 = new Vec2(3, 4);
      expect(v1.distanceTo(v2)).toBe(0);
    });

    it('should be symmetric', () => {
      const v1 = new Vec2(1, 2);
      const v2 = new Vec2(4, 6);
      expect(v1.distanceTo(v2)).toBe(v2.distanceTo(v1));
    });

    it('should handle negative coordinates', () => {
      const v1 = new Vec2(-1, -2);
      const v2 = new Vec2(2, 2);
      expect(v1.distanceTo(v2)).toBe(5);
    });
  });

  describe('lerp()', () => {
    it('should return start at t=0', () => {
      const v1 = new Vec2(0, 0);
      const v2 = new Vec2(10, 10);
      v1.lerp(v2, 0);
      expect(v1.x).toBe(0);
      expect(v1.y).toBe(0);
    });

    it('should return end at t=1', () => {
      const v1 = new Vec2(0, 0);
      const v2 = new Vec2(10, 10);
      v1.lerp(v2, 1);
      expect(v1.x).toBe(10);
      expect(v1.y).toBe(10);
    });

    it('should return midpoint at t=0.5', () => {
      const v1 = new Vec2(0, 0);
      const v2 = new Vec2(10, 10);
      v1.lerp(v2, 0.5);
      expect(v1.x).toBe(5);
      expect(v1.y).toBe(5);
    });

    it('should return this for chaining', () => {
      const v1 = new Vec2(0, 0);
      const v2 = new Vec2(10, 10);
      const result = v1.lerp(v2, 0.5);
      expect(result).toBe(v1);
    });

    it('should handle t > 1 (extrapolation)', () => {
      const v1 = new Vec2(0, 0);
      const v2 = new Vec2(10, 10);
      v1.lerp(v2, 2);
      expect(v1.x).toBe(20);
      expect(v1.y).toBe(20);
    });

    it('should handle t < 0 (extrapolation)', () => {
      const v1 = new Vec2(10, 10);
      const v2 = new Vec2(20, 20);
      v1.lerp(v2, -1);
      expect(v1.x).toBe(0);
      expect(v1.y).toBe(0);
    });
  });

  describe('clampLength()', () => {
    it('should not modify vector if length is below max', () => {
      const v = new Vec2(3, 4); // length = 5
      v.clampLength(10);
      expect(v.x).toBe(3);
      expect(v.y).toBe(4);
    });

    it('should clamp vector if length exceeds max', () => {
      const v = new Vec2(6, 8); // length = 10
      v.clampLength(5);
      expect(v.length()).toBeCloseTo(5);
    });

    it('should maintain direction when clamping', () => {
      const v = new Vec2(6, 8);
      const originalAngle = v.angle();
      v.clampLength(5);
      expect(v.angle()).toBeCloseTo(originalAngle);
    });

    it('should return this for chaining', () => {
      const v = new Vec2(6, 8);
      const result = v.clampLength(5);
      expect(result).toBe(v);
    });

    it('should handle zero vector', () => {
      const v = Vec2.zero();
      v.clampLength(5);
      expect(v.x).toBe(0);
      expect(v.y).toBe(0);
    });

    it('should handle max = 0', () => {
      const v = new Vec2(3, 4);
      v.clampLength(0);
      expect(v.x).toBe(0);
      expect(v.y).toBe(0);
    });
  });

  describe('chaining operations', () => {
    it('should support chaining multiple operations', () => {
      const v = new Vec2(1, 1);
      v.scale(2).add(new Vec2(1, 1)).normalize();
      expect(v.length()).toBeCloseTo(1);
    });

    it('should correctly chain set -> scale -> add', () => {
      const v = new Vec2();
      v.set(3, 4).scale(2).add(new Vec2(1, 2));
      expect(v.x).toBe(7);
      expect(v.y).toBe(10);
    });
  });

  describe('edge cases', () => {
    it('should handle very large values', () => {
      const v = new Vec2(1e10, 1e10);
      expect(v.length()).toBeCloseTo(Math.sqrt(2) * 1e10);
    });

    it('should handle very small values', () => {
      const v = new Vec2(1e-10, 1e-10);
      expect(v.length()).toBeCloseTo(Math.sqrt(2) * 1e-10);
    });

    it('should handle NaN values gracefully', () => {
      const v = new Vec2(NaN, 5);
      expect(Number.isNaN(v.length())).toBe(true);
    });

    it('should handle Infinity values', () => {
      const v = new Vec2(Infinity, 0);
      expect(v.length()).toBe(Infinity);
    });
  });
});

describe('vec2Lerp (standalone function)', () => {
  it('should return new vector at start when t=0', () => {
    const a = new Vec2(0, 0);
    const b = new Vec2(10, 10);
    const result = vec2Lerp(a, b, 0);
    expect(result.x).toBe(0);
    expect(result.y).toBe(0);
  });

  it('should return new vector at end when t=1', () => {
    const a = new Vec2(0, 0);
    const b = new Vec2(10, 10);
    const result = vec2Lerp(a, b, 1);
    expect(result.x).toBe(10);
    expect(result.y).toBe(10);
  });

  it('should return new vector at midpoint when t=0.5', () => {
    const a = new Vec2(0, 0);
    const b = new Vec2(10, 10);
    const result = vec2Lerp(a, b, 0.5);
    expect(result.x).toBe(5);
    expect(result.y).toBe(5);
  });

  it('should not modify input vectors', () => {
    const a = new Vec2(0, 0);
    const b = new Vec2(10, 10);
    vec2Lerp(a, b, 0.5);
    expect(a.x).toBe(0);
    expect(a.y).toBe(0);
    expect(b.x).toBe(10);
    expect(b.y).toBe(10);
  });

  it('should return a new instance', () => {
    const a = new Vec2(0, 0);
    const b = new Vec2(10, 10);
    const result = vec2Lerp(a, b, 0.5);
    expect(result).not.toBe(a);
    expect(result).not.toBe(b);
  });

  it('should handle extrapolation t > 1', () => {
    const a = new Vec2(0, 0);
    const b = new Vec2(10, 10);
    const result = vec2Lerp(a, b, 2);
    expect(result.x).toBe(20);
    expect(result.y).toBe(20);
  });

  it('should handle extrapolation t < 0', () => {
    const a = new Vec2(10, 10);
    const b = new Vec2(20, 20);
    const result = vec2Lerp(a, b, -1);
    expect(result.x).toBe(0);
    expect(result.y).toBe(0);
  });
});
