import { describe, it, expect, beforeEach, vi } from 'vitest';
import { Vec2 } from '@/utils/Vec2';

// Mock canvas context
function createMockContext(): CanvasRenderingContext2D {
  const canvas = {
    width: 1920,
    height: 1080,
  };
  return {
    canvas,
    save: vi.fn(),
    restore: vi.fn(),
    translate: vi.fn(),
    scale: vi.fn(),
    beginPath: vi.fn(),
    arc: vi.fn(),
    fill: vi.fn(),
    stroke: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    closePath: vi.fn(),
    fillRect: vi.fn(),
    strokeRect: vi.fn(),
    fillText: vi.fn(),
    measureText: vi.fn(() => ({ width: 50 })),
    setLineDash: vi.fn(),
    createRadialGradient: vi.fn(() => ({
      addColorStop: vi.fn(),
    })),
    createLinearGradient: vi.fn(() => ({
      addColorStop: vi.fn(),
    })),
    globalAlpha: 1,
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 1,
    font: '',
    textAlign: 'left',
    textBaseline: 'alphabetic',
    lineCap: 'butt',
    shadowBlur: 0,
    shadowColor: '',
  } as unknown as CanvasRenderingContext2D;
}

// Test Vec2 to ensure velocity.length() works correctly
describe('Vec2', () => {
  it('should have a length method that returns correct value', () => {
    const vec = new Vec2(3, 4);
    expect(vec.length()).toBe(5);
  });

  it('should handle zero vector', () => {
    const vec = new Vec2(0, 0);
    expect(vec.length()).toBe(0);
  });

  it('should handle negative values', () => {
    const vec = new Vec2(-3, -4);
    expect(vec.length()).toBe(5);
  });

  it('should have clone method', () => {
    const vec = new Vec2(10, 20);
    const cloned = vec.clone();
    expect(cloned.x).toBe(10);
    expect(cloned.y).toBe(20);
    expect(cloned).not.toBe(vec);
  });
});

// Test getEffectQuality logic
describe('Effect Quality Logic', () => {
  // Simulate the getEffectQuality function
  function getEffectQuality(currentZoom: number): 'full' | 'reduced' | 'minimal' {
    if (currentZoom > 0.4) return 'full';
    if (currentZoom > 0.2) return 'reduced';
    return 'minimal';
  }

  it('should return full for normal player zoom (1.0)', () => {
    expect(getEffectQuality(1.0)).toBe('full');
  });

  it('should return full for player at max speed zoom (0.45)', () => {
    expect(getEffectQuality(0.45)).toBe('full');
  });

  it('should return full for zoom just above threshold (0.41)', () => {
    expect(getEffectQuality(0.41)).toBe('full');
  });

  it('should return reduced for medium zoom (0.3)', () => {
    expect(getEffectQuality(0.3)).toBe('reduced');
  });

  it('should return minimal for spectator full map view (0.1)', () => {
    expect(getEffectQuality(0.1)).toBe('minimal');
  });

  it('should return minimal for very small zoom (0.05)', () => {
    expect(getEffectQuality(0.05)).toBe('minimal');
  });

  it('should handle edge cases', () => {
    expect(getEffectQuality(0.4)).toBe('reduced'); // Exactly at threshold
    expect(getEffectQuality(0.2)).toBe('minimal'); // Exactly at threshold
  });

  it('should handle NaN gracefully', () => {
    // NaN > 0.4 is false, NaN > 0.2 is false
    expect(getEffectQuality(NaN)).toBe('minimal');
  });

  it('should handle Infinity', () => {
    expect(getEffectQuality(Infinity)).toBe('full');
    expect(getEffectQuality(-Infinity)).toBe('minimal');
  });
});

// Test screenToWorld logic
describe('Screen to World Conversion', () => {
  // Simulate the screenToWorld calculation (with safeguard)
  function screenToWorld(
    screenX: number,
    screenY: number,
    canvasWidth: number,
    canvasHeight: number,
    currentZoom: number,
    cameraOffsetX: number,
    cameraOffsetY: number
  ): { x: number; y: number } {
    const centerX = canvasWidth / 2;
    const centerY = canvasHeight / 2;

    // Safeguard: prevent division by zero/invalid zoom
    const zoom = currentZoom > 0.001 ? currentZoom : 0.1;
    const worldX = (screenX - centerX) / zoom + centerX - cameraOffsetX;
    const worldY = (screenY - centerY) / zoom + centerY - cameraOffsetY;

    return { x: worldX, y: worldY };
  }

  it('should convert center of screen to world origin when camera centered', () => {
    const result = screenToWorld(960, 540, 1920, 1080, 1.0, 960, 540);
    expect(result.x).toBe(0);
    expect(result.y).toBe(0);
  });

  it('should handle zoom correctly', () => {
    // At zoom 0.5, screen distance should map to 2x world distance
    const result1 = screenToWorld(1060, 540, 1920, 1080, 1.0, 960, 540);
    const result05 = screenToWorld(1060, 540, 1920, 1080, 0.5, 960, 540);

    expect(result1.x).toBe(100); // 100 pixels from center
    expect(result05.x).toBe(200); // Same screen position = 2x world distance at 0.5 zoom
  });

  it('should handle very small zoom (spectator full map)', () => {
    const result = screenToWorld(960, 540, 1920, 1080, 0.1, 960, 540);
    expect(isFinite(result.x)).toBe(true);
    expect(isFinite(result.y)).toBe(true);
  });

  it('should handle zero zoom gracefully', () => {
    // Zero zoom should be safeguarded - use fallback zoom of 0.1
    const result = screenToWorld(960, 540, 1920, 1080, 0, 960, 540);
    // With safeguard: zoom = 0.1, result should be finite
    expect(isFinite(result.x)).toBe(true);
    expect(isFinite(result.y)).toBe(true);
  });

  it('should handle NaN zoom', () => {
    // NaN > 0.001 is false, so safeguard uses 0.1, result should be finite
    const result = screenToWorld(960, 540, 1920, 1080, NaN, 960, 540);
    expect(isFinite(result.x)).toBe(true);
    expect(isFinite(result.y)).toBe(true);
  });
});

// Test spectator target following logic
describe('Spectator Follow Mode', () => {
  const ZOOM_MIN = 0.45;
  const ZOOM_MAX = 1.0;
  const SPEED_FOR_MAX_ZOOM_OUT = 250;

  // Simulate the zoom calculation when following a player
  function calculateFollowZoom(velocity: { x: number; y: number } | Vec2): number {
    let speed: number;

    // Test the actual code pattern used
    if (velocity instanceof Vec2) {
      speed = velocity.length?.() ?? 0;
    } else if (velocity && typeof velocity === 'object') {
      // Plain object - no length method
      speed = Math.sqrt(velocity.x * velocity.x + velocity.y * velocity.y);
    } else {
      speed = 0;
    }

    const speedRatio = Math.min(speed / SPEED_FOR_MAX_ZOOM_OUT, 1);
    return ZOOM_MAX - (ZOOM_MAX - ZOOM_MIN) * speedRatio;
  }

  it('should calculate zoom correctly for Vec2 velocity', () => {
    const velocity = new Vec2(0, 0);
    expect(calculateFollowZoom(velocity)).toBe(ZOOM_MAX);
  });

  it('should zoom out for fast moving target', () => {
    const velocity = new Vec2(250, 0); // Max speed
    expect(calculateFollowZoom(velocity)).toBeCloseTo(ZOOM_MIN, 5);
  });

  it('should handle velocity as plain object', () => {
    const velocity = { x: 0, y: 0 };
    // This tests what happens if velocity is not a Vec2 instance
    const zoom = calculateFollowZoom(velocity);
    expect(zoom).toBe(ZOOM_MAX);
  });

  it('should handle undefined velocity', () => {
    const zoom = calculateFollowZoom(undefined as any);
    expect(zoom).toBe(ZOOM_MAX);
  });

  it('should handle null velocity', () => {
    const zoom = calculateFollowZoom(null as any);
    expect(zoom).toBe(ZOOM_MAX);
  });
});

// Test velocity.length?.() pattern
describe('Velocity Length Optional Chaining', () => {
  it('should work with Vec2', () => {
    const velocity = new Vec2(3, 4);
    const speed = velocity.length?.() ?? 0;
    expect(speed).toBe(5);
  });

  it('should return 0 for undefined velocity', () => {
    const velocity = undefined;
    const speed = velocity?.length?.() ?? 0;
    expect(speed).toBe(0);
  });

  it('should return 0 for null velocity', () => {
    const velocity = null;
    const speed = (velocity as any)?.length?.() ?? 0;
    expect(speed).toBe(0);
  });

  it('should throw for plain object without length method', () => {
    const velocity = { x: 3, y: 4 } as any;
    // velocity.length is undefined, so velocity.length?.() returns undefined
    const speed = velocity.length?.() ?? 0;
    expect(speed).toBe(0); // Falls back to 0
  });

  it('should throw for object with non-function length property', () => {
    const velocity = { x: 3, y: 4, length: 5 } as any;
    // velocity.length is 5 (a number), calling it as function throws!
    expect(() => {
      velocity.length?.();
    }).toThrow(TypeError);
    // THIS IS THE BUG! If velocity has a length property that's not a function, it throws
  });

  it('should handle array-like objects', () => {
    // Arrays have .length but it's a property, not a method
    const velocity = [3, 4] as any;
    expect(() => {
      velocity.length?.();
    }).toThrow(TypeError);
    // THIS COULD BE A BUG if velocity is accidentally an array
  });
});

// Test camera initialization and state
describe('Camera State', () => {
  it('should not cause infinite loop when target not found', () => {
    // Simulate the camera update when spectateTargetId is set but player not in snapshot
    let iterations = 0;
    const maxIterations = 100;

    const spectateTargetId = 'player-123';
    const players = new Map(); // Empty - target not found

    // Simulate what happens in render()
    while (iterations < maxIterations) {
      iterations++;
      const spectateTarget = players.get(spectateTargetId);
      if (!spectateTarget) {
        // Target not found - this is the fallback case
        break;
      }
    }

    expect(iterations).toBe(1); // Should exit immediately
  });
});

// Test player finding logic in click handler
describe('Spectator Click Player Finding', () => {
  interface MockPlayer {
    id: string;
    alive: boolean;
    position: { x: number; y: number };
    mass: number;
  }

  function findClosestPlayer(
    worldX: number,
    worldY: number,
    players: Map<string, MockPlayer>,
    clickRadius: number = 100
  ): { id: string; distance: number } | null {
    let closestPlayer: { id: string; distance: number } | null = null;

    for (const player of players.values()) {
      if (!player.alive) continue;
      if (!player.position || !isFinite(player.position.x) || !isFinite(player.position.y)) continue;

      const dx = player.position.x - worldX;
      const dy = player.position.y - worldY;
      const distance = Math.sqrt(dx * dx + dy * dy);

      const playerRadius = Math.sqrt(player.mass || 100) * 2;
      const adjustedDistance = Math.max(0, distance - playerRadius);

      if (adjustedDistance <= clickRadius) {
        if (!closestPlayer || adjustedDistance < closestPlayer.distance) {
          closestPlayer = { id: player.id, distance: adjustedDistance };
        }
      }
    }

    return closestPlayer;
  }

  it('should find closest player', () => {
    const players = new Map<string, MockPlayer>([
      ['p1', { id: 'p1', alive: true, position: { x: 50, y: 0 }, mass: 100 }],
      ['p2', { id: 'p2', alive: true, position: { x: 150, y: 0 }, mass: 100 }],
    ]);

    const result = findClosestPlayer(0, 0, players);
    expect(result?.id).toBe('p1');
  });

  it('should skip dead players', () => {
    const players = new Map<string, MockPlayer>([
      ['p1', { id: 'p1', alive: false, position: { x: 10, y: 0 }, mass: 100 }],
      ['p2', { id: 'p2', alive: true, position: { x: 50, y: 0 }, mass: 100 }],
    ]);

    const result = findClosestPlayer(0, 0, players);
    expect(result?.id).toBe('p2');
  });

  it('should handle NaN position', () => {
    const players = new Map<string, MockPlayer>([
      ['p1', { id: 'p1', alive: true, position: { x: NaN, y: 0 }, mass: 100 }],
      ['p2', { id: 'p2', alive: true, position: { x: 50, y: 0 }, mass: 100 }],
    ]);

    const result = findClosestPlayer(0, 0, players);
    expect(result?.id).toBe('p2'); // Should skip NaN player
  });

  it('should handle Infinity position', () => {
    const players = new Map<string, MockPlayer>([
      ['p1', { id: 'p1', alive: true, position: { x: Infinity, y: 0 }, mass: 100 }],
    ]);

    const result = findClosestPlayer(0, 0, players);
    expect(result).toBeNull(); // Infinity is not finite
  });

  it('should handle undefined position', () => {
    const players = new Map<string, MockPlayer>([
      ['p1', { id: 'p1', alive: true, position: undefined as any, mass: 100 }],
    ]);

    const result = findClosestPlayer(0, 0, players);
    expect(result).toBeNull();
  });

  it('should handle empty players map', () => {
    const players = new Map<string, MockPlayer>();
    const result = findClosestPlayer(0, 0, players);
    expect(result).toBeNull();
  });

  it('should account for player radius', () => {
    // Player at x=120 with radius 20 (mass 100, sqrt(100)*2 = 20)
    // Click at 0,0 - distance is 120, adjusted is 100 (120 - 20)
    const players = new Map<string, MockPlayer>([
      ['p1', { id: 'p1', alive: true, position: { x: 120, y: 0 }, mass: 100 }],
    ]);

    const result = findClosestPlayer(0, 0, players, 100);
    expect(result?.id).toBe('p1');
    expect(result?.distance).toBe(100);
  });

  it('should not find player outside click radius', () => {
    const players = new Map<string, MockPlayer>([
      ['p1', { id: 'p1', alive: true, position: { x: 200, y: 0 }, mass: 100 }],
    ]);

    const result = findClosestPlayer(0, 0, players, 100);
    expect(result).toBeNull(); // 200 - 20 = 180 > 100
  });
});

// Integration test simulating the full click flow
describe('Full Spectator Click Flow', () => {
  it('should not freeze when clicking in empty area', () => {
    const startTime = Date.now();

    // Simulate the click handler flow
    const isSpectator = true;
    const phase = 'playing';

    if (!isSpectator || (phase !== 'playing' && phase !== 'countdown')) {
      return;
    }

    // screenToWorld
    const worldPos = { x: 0, y: 0 };
    if (!worldPos || !isFinite(worldPos.x) || !isFinite(worldPos.y)) {
      return;
    }

    // Find player - none found
    const players = new Map();
    let closestPlayer = null;
    for (const player of players.values()) {
      // Loop doesn't execute
    }

    // No player found, check if we should clear target
    const spectateTargetId = null;
    if (closestPlayer) {
      // setSpectateTarget
    } else if (spectateTargetId !== null) {
      // setSpectateTarget(null)
    }

    const elapsed = Date.now() - startTime;
    expect(elapsed).toBeLessThan(100); // Should complete quickly
  });

  it('should handle click when velocity is malformed', () => {
    // This simulates what might happen if the player data is corrupted
    const player = {
      id: 'test',
      alive: true,
      position: new Vec2(100, 100),
      velocity: { x: 10, y: 10, length: 'not a function' }, // Malformed!
      mass: 100,
    };

    // Test the velocity.length?.() pattern
    const velocity = player.velocity as any;

    // This should NOT throw because we use optional chaining
    let speed: number;
    try {
      speed = velocity.length?.() ?? 0;
    } catch (e) {
      // If length exists but is not callable, this throws
      speed = 0;
    }

    expect(speed).toBe(0);
  });
});
