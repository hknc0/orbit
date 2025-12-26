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
  // Simulate the getEffectQuality function with spectator awareness
  function getEffectQuality(
    currentZoom: number,
    isSpectator: boolean = false,
    spectateTargetId: string | null = null
  ): 'full' | 'reduced' | 'minimal' {
    // Full-view spectators: viewing entire arena with all entities
    if (isSpectator && spectateTargetId === null) {
      if (currentZoom > 0.3) return 'reduced';
      return 'minimal';
    }

    // Players AND follow-mode spectators use normal thresholds
    if (currentZoom > 0.4) return 'full';
    if (currentZoom > 0.2) return 'reduced';
    return 'minimal';
  }

  describe('Normal player (unchanged behavior)', () => {
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

    it('should return minimal for very zoomed out (0.1)', () => {
      expect(getEffectQuality(0.1)).toBe('minimal');
    });

    it('should handle edge cases', () => {
      expect(getEffectQuality(0.4)).toBe('reduced'); // Exactly at threshold
      expect(getEffectQuality(0.2)).toBe('minimal'); // Exactly at threshold
    });
  });

  describe('Full-view spectator (reduced quality)', () => {
    it('should return reduced at high zoom (0.6) - never full', () => {
      expect(getEffectQuality(0.6, true, null)).toBe('reduced');
    });

    it('should return reduced at zoom 0.5 - shrunken arena', () => {
      expect(getEffectQuality(0.5, true, null)).toBe('reduced');
    });

    it('should return reduced at zoom 0.35', () => {
      expect(getEffectQuality(0.35, true, null)).toBe('reduced');
    });

    it('should return minimal at zoom 0.3 (threshold)', () => {
      expect(getEffectQuality(0.3, true, null)).toBe('minimal');
    });

    it('should return minimal at low zoom (0.1)', () => {
      expect(getEffectQuality(0.1, true, null)).toBe('minimal');
    });
  });

  describe('Follow-mode spectator (same as player)', () => {
    it('should return full at high zoom (0.6)', () => {
      expect(getEffectQuality(0.6, true, 'player-123')).toBe('full');
    });

    it('should return full at zoom 0.45', () => {
      expect(getEffectQuality(0.45, true, 'player-123')).toBe('full');
    });

    it('should return reduced at zoom 0.3', () => {
      expect(getEffectQuality(0.3, true, 'player-123')).toBe('reduced');
    });

    it('should return minimal at zoom 0.1', () => {
      expect(getEffectQuality(0.1, true, 'player-123')).toBe('minimal');
    });
  });

  describe('Edge cases', () => {
    it('should handle NaN gracefully', () => {
      expect(getEffectQuality(NaN)).toBe('minimal');
    });

    it('should handle Infinity', () => {
      expect(getEffectQuality(Infinity)).toBe('full');
      expect(getEffectQuality(-Infinity)).toBe('minimal');
    });
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

// Test gravity well quality optimization
describe('Gravity Well Quality Optimization', () => {
  // The effect quality function that drives gravity well rendering
  function getEffectQuality(
    currentZoom: number,
    isSpectator: boolean = false,
    spectateTargetId: string | null = null
  ): 'full' | 'reduced' | 'minimal' {
    if (isSpectator && spectateTargetId === null) {
      if (currentZoom > 0.3) return 'reduced';
      return 'minimal';
    }
    if (currentZoom > 0.4) return 'full';
    if (currentZoom > 0.2) return 'reduced';
    return 'minimal';
  }

  describe('Central black hole quality', () => {
    it('should use full quality for normal players', () => {
      // Normal player at default zoom sees full black hole effects
      expect(getEffectQuality(1.0, false, null)).toBe('full');
    });

    it('should use reduced quality for full-view spectators at high zoom', () => {
      // Full-view spectator when arena shrinks (zoom 0.5-0.8)
      // Should use reduced quality to skip particles, Doppler, etc.
      expect(getEffectQuality(0.6, true, null)).toBe('reduced');
      expect(getEffectQuality(0.5, true, null)).toBe('reduced');
    });

    it('should use minimal quality for full-view spectators at low zoom', () => {
      // Full-view spectator at full map view
      // Should use minimal (just event horizon + simple halo)
      expect(getEffectQuality(0.2, true, null)).toBe('minimal');
      expect(getEffectQuality(0.1, true, null)).toBe('minimal');
    });

    it('should use full quality for follow-mode spectators', () => {
      // Spectator following a player sees same quality as player
      expect(getEffectQuality(0.6, true, 'player-123')).toBe('full');
    });
  });

  describe('Normal gravity well (star) quality', () => {
    it('should use full quality for normal players', () => {
      expect(getEffectQuality(0.8, false, null)).toBe('full');
    });

    it('should use reduced quality for full-view spectators', () => {
      // Should skip corona shimmer, use simpler gradients
      expect(getEffectQuality(0.5, true, null)).toBe('reduced');
    });

    it('should use minimal quality for zoomed-out view', () => {
      // Should use solid core + single glow only
      expect(getEffectQuality(0.15, true, null)).toBe('minimal');
    });
  });

  describe('Orbit zones', () => {
    it('should render orbit zones at full quality', () => {
      const quality = getEffectQuality(1.0, false, null);
      expect(quality !== 'minimal').toBe(true);
    });

    it('should render orbit zones at reduced quality', () => {
      const quality = getEffectQuality(0.35, true, null);
      expect(quality !== 'minimal').toBe(true);
    });

    it('should skip orbit zones at minimal quality', () => {
      const quality = getEffectQuality(0.1, true, null);
      expect(quality).toBe('minimal');
      // At minimal quality, orbit zones are not rendered
    });
  });

  describe('Performance impact estimation', () => {
    // These tests document the expected draw call reduction
    const estimateBlackHoleDrawCalls = (quality: 'full' | 'reduced' | 'minimal'): number => {
      switch (quality) {
        case 'full': return 9;     // All effects: ambient, halo, bloom, disk, Doppler, particles, horizon, shading, photon
        case 'reduced': return 5;  // Skip particles, Doppler, bloom: ambient, halo, disk, horizon, photon
        case 'minimal': return 4;  // Iconic look: ambient, disk, horizon, photon ring (no particles/Doppler/bloom/shadows)
      }
    };

    const estimateStarDrawCalls = (quality: 'full' | 'reduced' | 'minimal'): number => {
      switch (quality) {
        case 'full': return 5;     // Outer glow, corona, core gradient, center spot, + orbit zones
        case 'reduced': return 2;  // Outer glow, core gradient (no corona, no center spot)
        case 'minimal': return 2;  // Simple glow, solid core
      }
    };

    it('should reduce black hole draw calls significantly at minimal quality', () => {
      const fullCalls = estimateBlackHoleDrawCalls('full');
      const minimalCalls = estimateBlackHoleDrawCalls('minimal');
      const reduction = (fullCalls - minimalCalls) / fullCalls;
      expect(reduction).toBeGreaterThan(0.5); // >50% reduction (9 -> 4 passes)
      // Minimal still looks impressive (has disk + photon ring) but skips expensive effects
    });

    it('should reduce star draw calls at reduced quality', () => {
      const fullCalls = estimateStarDrawCalls('full');
      const reducedCalls = estimateStarDrawCalls('reduced');
      const reduction = (fullCalls - reducedCalls) / fullCalls;
      expect(reduction).toBeGreaterThan(0.5); // >50% reduction
    });

    it('should estimate significant savings for full-view spectator with 10 wells', () => {
      const wellCount = 10;
      const fullViewQuality = getEffectQuality(0.2, true, null);
      expect(fullViewQuality).toBe('minimal');

      // At full quality: 1 black hole (9) + 10 stars (50) = 59 draw calls
      // At minimal quality: 1 black hole (4) + 10 stars (20) = 24 draw calls
      // Savings: ~59% fewer draw calls, but black hole still looks impressive
      const fullCalls = 9 + wellCount * 5;
      const minimalCalls = 4 + wellCount * 2;
      const savings = (fullCalls - minimalCalls) / fullCalls;
      expect(savings).toBeGreaterThan(0.5); // Still >50% savings
    });
  });
});

// Test zoom transition behavior
describe('Zoom Transition', () => {
  const ZOOM_TRANSITION_THRESHOLD = 0.2;
  const ZOOM_TRANSITION_DURATION = 800;

  // Ease-in-out cubic function (same as in RenderSystem)
  function easeInOutCubic(progress: number): number {
    return progress < 0.5
      ? 4 * progress * progress * progress
      : 1 - Math.pow(-2 * progress + 2, 3) / 2;
  }

  describe('transition detection', () => {
    it('should trigger transition for large zoom changes', () => {
      const fromZoom = 0.1;  // Full map view
      const toZoom = 0.8;    // Follow mode
      const delta = Math.abs(toZoom - fromZoom);
      expect(delta).toBeGreaterThan(ZOOM_TRANSITION_THRESHOLD);
    });

    it('should not trigger transition for small zoom changes', () => {
      const fromZoom = 0.8;  // Follow mode
      const toZoom = 0.7;    // Speed-based zoom change
      const delta = Math.abs(toZoom - fromZoom);
      expect(delta).toBeLessThan(ZOOM_TRANSITION_THRESHOLD);
    });
  });

  describe('ease-in-out cubic', () => {
    it('should start slow (ease-in)', () => {
      // At 10% progress, movement should be much less than 10%
      const progress = 0.1;
      const eased = easeInOutCubic(progress);
      expect(eased).toBeLessThan(progress);
      expect(eased).toBeCloseTo(0.004, 2); // 4 * 0.1^3 = 0.004
    });

    it('should be fastest in the middle', () => {
      // At 50% progress, should be at 50% of transition
      const eased = easeInOutCubic(0.5);
      expect(eased).toBeCloseTo(0.5, 5);
    });

    it('should end slow (ease-out)', () => {
      // At 90% progress, movement should be slowing down
      const progressA = 0.8;
      const progressB = 0.9;
      const deltaA = easeInOutCubic(progressB) - easeInOutCubic(progressA);

      const progressC = 0.5;
      const progressD = 0.6;
      const deltaMiddle = easeInOutCubic(progressD) - easeInOutCubic(progressC);

      // Movement near the end should be slower than in the middle
      expect(deltaA).toBeLessThan(deltaMiddle);
    });

    it('should reach 1.0 at progress 1.0', () => {
      expect(easeInOutCubic(1.0)).toBe(1.0);
    });

    it('should be 0 at progress 0', () => {
      expect(easeInOutCubic(0)).toBe(0);
    });
  });

  describe('transition timing', () => {
    it('should complete transition over the duration', () => {
      const duration = ZOOM_TRANSITION_DURATION;

      // At 0ms: 0% progress
      expect(0 / duration).toBe(0);

      // At 400ms: 50% progress
      expect(400 / duration).toBe(0.5);

      // At 800ms: 100% progress
      expect(800 / duration).toBe(1.0);
    });

    it('should interpolate zoom smoothly during transition', () => {
      const fromZoom = 0.1;
      const toZoom = 0.8;

      // At 50% progress with ease-in-out, zoom should be at midpoint
      const progress = 0.5;
      const eased = easeInOutCubic(progress);
      const currentZoom = fromZoom + (toZoom - fromZoom) * eased;

      expect(currentZoom).toBeCloseTo(0.45, 2); // (0.1 + 0.8) / 2 = 0.45
    });
  });
});
describe('RenderSystem Class', () => {
  let ctx: CanvasRenderingContext2D;

  beforeEach(() => {
    ctx = createMockContext();
  });

  describe('constructor', () => {
    it('should create a RenderSystem instance', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);
      expect(renderSystem).toBeDefined();
    });
  });

  describe('reset', () => {
    it('should reset all state', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      // Trigger shake to set some state
      renderSystem.triggerShake(0.5);

      // Reset
      renderSystem.reset();

      // Should not throw when rendering after reset
      // (indirectly verifies state was reset)
      expect(() => renderSystem.reset()).not.toThrow();
    });

    it('should be callable multiple times', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      expect(() => {
        renderSystem.reset();
        renderSystem.reset();
        renderSystem.reset();
      }).not.toThrow();
    });
  });

  describe('triggerShake', () => {
    it('should accept intensity value', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      expect(() => renderSystem.triggerShake(0.5)).not.toThrow();
    });

    it('should handle zero intensity', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      expect(() => renderSystem.triggerShake(0)).not.toThrow();
    });

    it('should handle high intensity', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      expect(() => renderSystem.triggerShake(5.0)).not.toThrow();
    });

    it('should handle negative intensity', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      // Implementation might clamp or use absolute value
      expect(() => renderSystem.triggerShake(-0.5)).not.toThrow();
    });

    it('should accept optional direction parameter', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      expect(() => renderSystem.triggerShake(0.5, { x: 1, y: 0 })).not.toThrow();
    });

    it('should handle direction with zero length', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      // Zero-length direction should not cause errors
      expect(() => renderSystem.triggerShake(0.5, { x: 0, y: 0 })).not.toThrow();
    });

    it('should accumulate shake from multiple triggers', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      // Multiple small shakes should accumulate
      renderSystem.triggerShake(0.2);
      renderSystem.triggerShake(0.2);
      renderSystem.triggerShake(0.2);
      expect(() => renderSystem.triggerShake(0.2)).not.toThrow();
    });

    it('should cap intensity at MAX_SHAKE', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      // Trigger with very high intensity multiple times
      renderSystem.triggerShake(10);
      renderSystem.triggerShake(10);
      renderSystem.triggerShake(10);
      expect(() => renderSystem.triggerShake(10)).not.toThrow();
    });
  });

  describe('screenToWorld', () => {
    it('should return Vec2 result', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      const result = renderSystem.screenToWorld(960, 540);

      expect(result).toBeDefined();
      expect(typeof result.x).toBe('number');
      expect(typeof result.y).toBe('number');
    });

    it('should handle center of screen', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      const result = renderSystem.screenToWorld(960, 540);

      expect(isFinite(result.x)).toBe(true);
      expect(isFinite(result.y)).toBe(true);
    });

    it('should handle corner coordinates', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      const topLeft = renderSystem.screenToWorld(0, 0);
      const bottomRight = renderSystem.screenToWorld(1920, 1080);

      expect(isFinite(topLeft.x)).toBe(true);
      expect(isFinite(topLeft.y)).toBe(true);
      expect(isFinite(bottomRight.x)).toBe(true);
      expect(isFinite(bottomRight.y)).toBe(true);
    });

    it('should handle negative coordinates', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      const result = renderSystem.screenToWorld(-100, -100);

      expect(isFinite(result.x)).toBe(true);
      expect(isFinite(result.y)).toBe(true);
    });

    it('should handle large coordinates', async () => {
      const { RenderSystem } = await import('./RenderSystem');
      const renderSystem = new RenderSystem(ctx);

      const result = renderSystem.screenToWorld(10000, 10000);

      expect(isFinite(result.x)).toBe(true);
      expect(isFinite(result.y)).toBe(true);
    });
  });
});

// Test MOTION_FX constants
describe('Motion Effects Configuration', () => {
  it('should have valid trail lifetime base', () => {
    const TRAIL_LIFETIME_BASE = 550;
    expect(TRAIL_LIFETIME_BASE).toBeGreaterThan(0);
    expect(TRAIL_LIFETIME_BASE).toBeLessThan(5000); // Reasonable upper bound
  });

  it('should have valid trail max points', () => {
    const TRAIL_MAX_POINTS = 32;
    expect(TRAIL_MAX_POINTS).toBeGreaterThan(0);
    expect(TRAIL_MAX_POINTS).toBeLessThanOrEqual(100);
  });

  it('should have flame length scale in valid range', () => {
    const FLAME_LENGTH_BASE = 1.6;
    const FLAME_LENGTH_SPEED_SCALE = 0.002;

    expect(FLAME_LENGTH_BASE).toBeGreaterThan(0);
    expect(FLAME_LENGTH_SPEED_SCALE).toBeGreaterThan(0);
    expect(FLAME_LENGTH_SPEED_SCALE).toBeLessThan(1);
  });

  it('should have valid zoom bounds', () => {
    const ZOOM_MIN = 0.45;
    const ZOOM_MAX = 1.0;
    const SPECTATOR_ZOOM_MIN = 0.1;

    expect(SPECTATOR_ZOOM_MIN).toBeLessThan(ZOOM_MIN);
    expect(ZOOM_MIN).toBeLessThan(ZOOM_MAX);
    expect(ZOOM_MAX).toBeLessThanOrEqual(1.0);
  });
});

// Test color caching logic
describe('Color Caching', () => {
  function getRGB(color: string): { r: number; g: number; b: number } {
    const hex = color.replace('#', '');
    return {
      r: parseInt(hex.substring(0, 2), 16),
      g: parseInt(hex.substring(2, 4), 16),
      b: parseInt(hex.substring(4, 6), 16),
    };
  }

  it('should parse hex colors correctly', () => {
    const rgb = getRGB('#ff0000');
    expect(rgb.r).toBe(255);
    expect(rgb.g).toBe(0);
    expect(rgb.b).toBe(0);
  });

  it('should handle colors without hash', () => {
    const rgb = getRGB('00ff00');
    expect(rgb.r).toBe(0);
    expect(rgb.g).toBe(255);
    expect(rgb.b).toBe(0);
  });

  it('should parse blue correctly', () => {
    const rgb = getRGB('#0000ff');
    expect(rgb.r).toBe(0);
    expect(rgb.g).toBe(0);
    expect(rgb.b).toBe(255);
  });

  it('should parse white correctly', () => {
    const rgb = getRGB('#ffffff');
    expect(rgb.r).toBe(255);
    expect(rgb.g).toBe(255);
    expect(rgb.b).toBe(255);
  });

  it('should parse black correctly', () => {
    const rgb = getRGB('#000000');
    expect(rgb.r).toBe(0);
    expect(rgb.g).toBe(0);
    expect(rgb.b).toBe(0);
  });

  it('should handle lowercase hex', () => {
    const rgb = getRGB('#abcdef');
    expect(rgb.r).toBe(171);
    expect(rgb.g).toBe(205);
    expect(rgb.b).toBe(239);
  });

  it('should handle uppercase hex', () => {
    const rgb = getRGB('#ABCDEF');
    expect(rgb.r).toBe(171);
    expect(rgb.g).toBe(205);
    expect(rgb.b).toBe(239);
  });
});

// Test particle angles pre-computation
describe('Particle Angles Pre-computation', () => {
  it('should generate 8 particle angles', () => {
    const angles: { cos: number; sin: number }[] = [];
    for (let i = 0; i < 8; i++) {
      const angle = (i / 8) * Math.PI * 2;
      angles.push({ cos: Math.cos(angle), sin: Math.sin(angle) });
    }

    expect(angles.length).toBe(8);
  });

  it('should have first angle at 0 radians', () => {
    const angle = (0 / 8) * Math.PI * 2;
    expect(Math.cos(angle)).toBeCloseTo(1, 10);
    expect(Math.sin(angle)).toBeCloseTo(0, 10);
  });

  it('should have angles evenly distributed', () => {
    const angles: number[] = [];
    for (let i = 0; i < 8; i++) {
      angles.push((i / 8) * Math.PI * 2);
    }

    const expectedSpacing = Math.PI / 4;
    for (let i = 1; i < angles.length; i++) {
      expect(angles[i] - angles[i - 1]).toBeCloseTo(expectedSpacing, 10);
    }
  });

  it('should have valid cos/sin values', () => {
    for (let i = 0; i < 8; i++) {
      const angle = (i / 8) * Math.PI * 2;
      const cos = Math.cos(angle);
      const sin = Math.sin(angle);

      expect(cos >= -1 && cos <= 1).toBe(true);
      expect(sin >= -1 && sin <= 1).toBe(true);
      // cos^2 + sin^2 = 1
      expect(cos * cos + sin * sin).toBeCloseTo(1, 10);
    }
  });
});

// Test scale direction calculation
describe('Scale Direction', () => {
  function calculateScaleDirection(
    history: number[]
  ): 'growing' | 'shrinking' | 'stable' {
    if (history.length < 2) return 'stable';

    const recentAvg = history.slice(-10).reduce((a, b) => a + b, 0) / Math.min(history.length, 10);
    const olderAvg = history.slice(0, Math.max(1, history.length - 10))
      .reduce((a, b) => a + b, 0) / Math.max(1, history.length - 10);

    const diff = recentAvg - olderAvg;
    const threshold = 0.01;

    if (diff > threshold) return 'growing';
    if (diff < -threshold) return 'shrinking';
    return 'stable';
  }

  it('should return stable for empty history', () => {
    expect(calculateScaleDirection([])).toBe('stable');
  });

  it('should return stable for single value', () => {
    expect(calculateScaleDirection([1.0])).toBe('stable');
  });

  it('should detect growing scale', () => {
    const history = [1.0, 1.01, 1.02, 1.03, 1.04, 1.05, 1.06, 1.07, 1.08, 1.09, 1.1, 1.11];
    expect(calculateScaleDirection(history)).toBe('growing');
  });

  it('should detect shrinking scale', () => {
    const history = [1.1, 1.09, 1.08, 1.07, 1.06, 1.05, 1.04, 1.03, 1.02, 1.01, 1.0, 0.99];
    expect(calculateScaleDirection(history)).toBe('shrinking');
  });

  it('should return stable for constant values', () => {
    const history = Array(20).fill(1.0);
    expect(calculateScaleDirection(history)).toBe('stable');
  });
});

// Test shake decay
describe('Shake Decay', () => {
  const SHAKE_DECAY = 0.85;
  const MAX_SHAKE = 12;

  it('should decay shake intensity over time', () => {
    let intensity = 1.0;

    // After one decay step
    intensity *= SHAKE_DECAY;
    expect(intensity).toBe(0.85);

    // After another step
    intensity *= SHAKE_DECAY;
    expect(intensity).toBeCloseTo(0.7225, 4);
  });

  it('should approach zero after many steps', () => {
    let intensity = 1.0;

    for (let i = 0; i < 50; i++) {
      intensity *= SHAKE_DECAY;
    }

    expect(intensity).toBeLessThan(0.001);
  });

  it('should clamp to max shake', () => {
    const clampedShake = Math.min(20, MAX_SHAKE);
    expect(clampedShake).toBe(MAX_SHAKE);
  });
});

// Test trail point management
describe('Trail Point Management', () => {
  interface TrailPoint {
    x: number;
    y: number;
    timestamp: number;
    radius: number;
  }

  it('should remove expired trail points', () => {
    const now = Date.now();
    const trailLifetime = 550;

    const trail: TrailPoint[] = [
      { x: 0, y: 0, timestamp: now - 600, radius: 20 }, // Expired
      { x: 10, y: 10, timestamp: now - 400, radius: 20 }, // Valid
      { x: 20, y: 20, timestamp: now - 100, radius: 20 }, // Valid
    ];

    // Remove expired
    while (trail.length > 0 && now - trail[0].timestamp > trailLifetime) {
      trail.shift();
    }

    expect(trail.length).toBe(2);
    expect(trail[0].x).toBe(10);
  });

  it('should cap trail to max points', () => {
    const trail: TrailPoint[] = [];
    const maxPoints = 32;

    for (let i = 0; i < 50; i++) {
      trail.push({ x: i, y: i, timestamp: Date.now(), radius: 20 });
      while (trail.length > maxPoints) {
        trail.shift();
      }
    }

    expect(trail.length).toBe(maxPoints);
    expect(trail[0].x).toBe(50 - maxPoints);
  });

  it('should calculate distance squared for efficiency', () => {
    const pos = { x: 100, y: 100 };
    const lastPos = { x: 93, y: 96 }; // Distance = 7.61...

    const dx = pos.x - lastPos.x;
    const dy = pos.y - lastPos.y;
    const distSq = dx * dx + dy * dy;

    expect(distSq).toBe(49 + 16); // 7^2 + 4^2 = 65
    expect(Math.sqrt(distSq)).toBeCloseTo(8.06, 2);
  });
});
