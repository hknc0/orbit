import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { InputSystem } from './InputSystem';
import { EJECT } from '@/utils/Constants';

// Mock World with local player
function createMockWorld(hasLocalPlayer: boolean = true) {
  return {
    getLocalPlayer: vi.fn(() => hasLocalPlayer ? { id: 'test-player' } : null),
  };
}

// Create a mock canvas element
function createMockCanvas(): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.width = 1920;
  canvas.height = 1080;
  return canvas;
}

describe('InputSystem', () => {
  let canvas: HTMLCanvasElement;
  let inputSystem: InputSystem;

  beforeEach(() => {
    canvas = createMockCanvas();
    document.body.appendChild(canvas);
    inputSystem = new InputSystem(canvas);
  });

  afterEach(() => {
    inputSystem.destroy();
    document.body.removeChild(canvas);
  });

  describe('constructor', () => {
    it('should create InputSystem with canvas', () => {
      expect(inputSystem).toBeInstanceOf(InputSystem);
    });

    it('should initialize with default aim direction (1, 0)', () => {
      const aim = inputSystem.getAimDirection();
      expect(aim.x).toBe(1);
      expect(aim.y).toBe(0);
    });

    it('should not be boosting initially', () => {
      expect(inputSystem.isBoosting()).toBe(false);
    });

    it('should not be charging initially', () => {
      expect(inputSystem.isCharging()).toBe(false);
    });

    it('should have zero charge ratio initially', () => {
      expect(inputSystem.getChargeRatio()).toBe(0);
    });
  });

  describe('mouse input', () => {
    it('should track mouse position on mousemove', () => {
      const world = createMockWorld();

      canvas.dispatchEvent(new MouseEvent('mousemove', {
        clientX: 1000,
        clientY: 600,
        movementX: 10,
        movementY: 5,
      }));

      inputSystem.update(world as any, 0.016);

      const aim = inputSystem.getAimDirection();
      // Center is 960, 540 for 1920x1080
      // Direction should be normalized vector from center to mouse
      expect(aim.length()).toBeCloseTo(1);
    });

    it('should start boosting on left mouse button down', () => {
      canvas.dispatchEvent(new MouseEvent('mousedown', { button: 0 }));
      expect(inputSystem.isBoosting()).toBe(true);
    });

    it('should stop boosting on left mouse button up', () => {
      canvas.dispatchEvent(new MouseEvent('mousedown', { button: 0 }));
      expect(inputSystem.isBoosting()).toBe(true);

      canvas.dispatchEvent(new MouseEvent('mouseup', { button: 0 }));
      expect(inputSystem.isBoosting()).toBe(false);
    });

    it('should not boost on right mouse button', () => {
      canvas.dispatchEvent(new MouseEvent('mousedown', { button: 2 }));
      expect(inputSystem.isBoosting()).toBe(false);
    });

    it('should stop boosting on mouse leave', () => {
      canvas.dispatchEvent(new MouseEvent('mousedown', { button: 0 }));
      expect(inputSystem.isBoosting()).toBe(true);

      canvas.dispatchEvent(new MouseEvent('mouseleave'));
      expect(inputSystem.isBoosting()).toBe(false);
    });
  });

  describe('keyboard input', () => {
    it('should start charging on Space keydown', () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));
      expect(inputSystem.isCharging()).toBe(true);
    });

    it('should stop charging on Space keyup', () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));
      window.dispatchEvent(new KeyboardEvent('keyup', { code: 'Space' }));
      expect(inputSystem.isCharging()).toBe(false);
    });

    it('should ignore repeat key events', () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));
      expect(inputSystem.isCharging()).toBe(true);

      // Reset and simulate repeat
      inputSystem.reset();
      expect(inputSystem.isCharging()).toBe(false);

      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space', repeat: true }));
      expect(inputSystem.isCharging()).toBe(false);
    });

    it('should boost on Shift key', () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'ShiftLeft' }));
      expect(inputSystem.isBoosting()).toBe(true);

      window.dispatchEvent(new KeyboardEvent('keyup', { code: 'ShiftLeft' }));
      expect(inputSystem.isBoosting()).toBe(false);
    });

    it('should handle right Shift key', () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'ShiftRight' }));
      expect(inputSystem.isBoosting()).toBe(true);
    });

    describe('WASD keys', () => {
      it('should set direction on W key (up)', () => {
        const world = createMockWorld();
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyW' }));
        inputSystem.update(world as any, 0.016);

        const aim = inputSystem.getAimDirection();
        expect(aim.y).toBeLessThan(0);
      });

      it('should set direction on S key (down)', () => {
        const world = createMockWorld();
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyS' }));
        inputSystem.update(world as any, 0.016);

        const aim = inputSystem.getAimDirection();
        expect(aim.y).toBeGreaterThan(0);
      });

      it('should set direction on A key (left)', () => {
        const world = createMockWorld();
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyA' }));
        inputSystem.update(world as any, 0.016);

        const aim = inputSystem.getAimDirection();
        expect(aim.x).toBeLessThan(0);
      });

      it('should set direction on D key (right)', () => {
        const world = createMockWorld();
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyD' }));
        inputSystem.update(world as any, 0.016);

        const aim = inputSystem.getAimDirection();
        expect(aim.x).toBeGreaterThan(0);
      });

      it('should normalize diagonal movement (W+D)', () => {
        const world = createMockWorld();
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyW' }));
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyD' }));
        inputSystem.update(world as any, 0.016);

        const aim = inputSystem.getAimDirection();
        expect(aim.length()).toBeCloseTo(1);
        expect(aim.x).toBeCloseTo(Math.sqrt(2) / 2);
        expect(aim.y).toBeCloseTo(-Math.sqrt(2) / 2);
      });

      it('should auto-boost when using directional keys', () => {
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyW' }));
        expect(inputSystem.isBoosting()).toBe(true);
      });

      it('should stop boosting when releasing all directional keys', () => {
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyW' }));
        expect(inputSystem.isBoosting()).toBe(true);

        window.dispatchEvent(new KeyboardEvent('keyup', { code: 'KeyW' }));
        expect(inputSystem.isBoosting()).toBe(false);
      });
    });

    describe('Arrow keys', () => {
      it('should handle ArrowUp', () => {
        const world = createMockWorld();
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'ArrowUp' }));
        inputSystem.update(world as any, 0.016);

        const aim = inputSystem.getAimDirection();
        expect(aim.y).toBeLessThan(0);
      });

      it('should handle ArrowDown', () => {
        const world = createMockWorld();
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'ArrowDown' }));
        inputSystem.update(world as any, 0.016);

        const aim = inputSystem.getAimDirection();
        expect(aim.y).toBeGreaterThan(0);
      });

      it('should handle ArrowLeft', () => {
        const world = createMockWorld();
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'ArrowLeft' }));
        inputSystem.update(world as any, 0.016);

        const aim = inputSystem.getAimDirection();
        expect(aim.x).toBeLessThan(0);
      });

      it('should handle ArrowRight', () => {
        const world = createMockWorld();
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'ArrowRight' }));
        inputSystem.update(world as any, 0.016);

        const aim = inputSystem.getAimDirection();
        expect(aim.x).toBeGreaterThan(0);
      });

      it('should handle combined WASD and Arrow keys', () => {
        const world = createMockWorld();
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyW' }));
        window.dispatchEvent(new KeyboardEvent('keydown', { code: 'ArrowRight' }));
        inputSystem.update(world as any, 0.016);

        const aim = inputSystem.getAimDirection();
        expect(aim.length()).toBeCloseTo(1);
      });
    });
  });

  describe('input mode switching', () => {
    it('should switch to mouse aim on significant mouse movement', () => {
      const world = createMockWorld();

      // Use keyboard first
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyW' }));
      inputSystem.update(world as any, 0.016);

      // Then move mouse significantly
      canvas.dispatchEvent(new MouseEvent('mousemove', {
        clientX: 1200,
        clientY: 400,
        movementX: 10, // > 3
        movementY: 10,
      }));

      inputSystem.update(world as any, 0.016);

      // Should now be using mouse aim (direction toward 1200, 400 from center 960, 540)
      const aim = inputSystem.getAimDirection();
      expect(aim.x).toBeGreaterThan(0); // Mouse is to the right
      expect(aim.y).toBeLessThan(0); // Mouse is above center
    });

    it('should not switch on small mouse movement', () => {
      const world = createMockWorld();

      // Use keyboard first
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyW' }));
      inputSystem.update(world as any, 0.016);

      const aimBefore = inputSystem.getAimDirection().clone();

      // Small mouse movement (< 3 pixels)
      canvas.dispatchEvent(new MouseEvent('mousemove', {
        clientX: 962,
        clientY: 542,
        movementX: 2,
        movementY: 2,
      }));

      inputSystem.update(world as any, 0.016);

      const aimAfter = inputSystem.getAimDirection();
      expect(aimAfter.x).toBeCloseTo(aimBefore.x);
      expect(aimAfter.y).toBeCloseTo(aimBefore.y);
    });
  });

  describe('eject charging', () => {
    it('should track charge time while holding Space', () => {
      const world = createMockWorld();

      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));

      // Simulate 0.5 seconds of charging
      inputSystem.update(world as any, 0.5);

      expect(inputSystem.getChargeRatio()).toBeCloseTo(0.5);
    });

    it('should cap charge ratio at 1.0', () => {
      const world = createMockWorld();

      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));

      // Charge for longer than MAX_CHARGE_TIME
      inputSystem.update(world as any, EJECT.MAX_CHARGE_TIME + 1);

      expect(inputSystem.getChargeRatio()).toBe(1);
    });

    it('should return 0 when not charging', () => {
      const world = createMockWorld();

      // Charge then release
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));
      inputSystem.update(world as any, 0.5);
      window.dispatchEvent(new KeyboardEvent('keyup', { code: 'Space' }));

      expect(inputSystem.getChargeRatio()).toBe(0);
    });
  });

  describe('createInput', () => {
    it('should create input with sequence and tick', () => {
      const input = inputSystem.createInput(42, 100);

      expect(input.sequence).toBe(42);
      expect(input.tick).toBe(100);
    });

    it('should include client time for RTT', () => {
      const input = inputSystem.createInput(1, 1);

      expect(input.clientTime).toBeGreaterThan(0);
      expect(input.clientTime).toBeLessThanOrEqual(performance.now());
    });

    it('should include aim direction', () => {
      const world = createMockWorld();

      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyD' }));
      inputSystem.update(world as any, 0.016);

      const input = inputSystem.createInput(1, 1);

      expect(input.aim.x).toBeGreaterThan(0);
    });

    it('should set thrust to aim direction when boosting', () => {
      const world = createMockWorld();

      canvas.dispatchEvent(new MouseEvent('mousedown', { button: 0 }));
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyD' }));
      inputSystem.update(world as any, 0.016);

      const input = inputSystem.createInput(1, 1);

      expect(input.boost).toBe(true);
      expect(input.thrust.x).toBeGreaterThan(0);
    });

    it('should set thrust to zero when not boosting', () => {
      const input = inputSystem.createInput(1, 1);

      expect(input.boost).toBe(false);
      expect(input.thrust.x).toBe(0);
      expect(input.thrust.y).toBe(0);
    });

    it('should set fire to true when charging', () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));

      const input = inputSystem.createInput(1, 1);

      expect(input.fire).toBe(true);
    });

    it('should set fireReleased on Space release', () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));
      window.dispatchEvent(new KeyboardEvent('keyup', { code: 'Space' }));

      const input = inputSystem.createInput(1, 1);

      expect(input.fireReleased).toBe(true);
    });

    it('should consume fireReleased flag after creating input', () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));
      window.dispatchEvent(new KeyboardEvent('keyup', { code: 'Space' }));

      const input1 = inputSystem.createInput(1, 1);
      expect(input1.fireReleased).toBe(true);

      const input2 = inputSystem.createInput(2, 2);
      expect(input2.fireReleased).toBe(false);
    });

    it('should reset charge time after consuming fireReleased', () => {
      const world = createMockWorld();

      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));
      inputSystem.update(world as any, 0.5);
      window.dispatchEvent(new KeyboardEvent('keyup', { code: 'Space' }));

      // Consume the release
      inputSystem.createInput(1, 1);

      // Start charging again
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));
      expect(inputSystem.getChargeRatio()).toBe(0);
    });
  });

  describe('reset', () => {
    it('should reset all input state', () => {
      const world = createMockWorld();

      // Set up various input states
      canvas.dispatchEvent(new MouseEvent('mousedown', { button: 0 }));
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'ShiftLeft' }));
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyW' }));
      inputSystem.update(world as any, 0.5);

      expect(inputSystem.isBoosting()).toBe(true);
      expect(inputSystem.isCharging()).toBe(true);

      inputSystem.reset();

      expect(inputSystem.isBoosting()).toBe(false);
      expect(inputSystem.isCharging()).toBe(false);
      expect(inputSystem.getChargeRatio()).toBe(0);
    });

    it('should keep listeners attached after reset', () => {
      inputSystem.reset();

      // Listeners should still work
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));
      expect(inputSystem.isCharging()).toBe(true);
    });
  });

  describe('destroy', () => {
    it('should remove all event listeners', () => {
      inputSystem.destroy();

      // Listeners should not work anymore
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));
      expect(inputSystem.isCharging()).toBe(false);

      canvas.dispatchEvent(new MouseEvent('mousedown', { button: 0 }));
      expect(inputSystem.isBoosting()).toBe(false);
    });

    it('should reset input state on destroy', () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));
      expect(inputSystem.isCharging()).toBe(true);

      inputSystem.destroy();

      expect(inputSystem.isCharging()).toBe(false);
    });
  });

  describe('update with no local player', () => {
    it('should early return when no local player', () => {
      const world = createMockWorld(false);

      canvas.dispatchEvent(new MouseEvent('mousemove', {
        clientX: 1200,
        clientY: 400,
        movementX: 100,
        movementY: 100,
      }));

      // Should not throw
      expect(() => inputSystem.update(world as any, 0.016)).not.toThrow();
    });
  });

  describe('window blur handling', () => {
    it('should reset input on window blur', () => {
      canvas.dispatchEvent(new MouseEvent('mousedown', { button: 0 }));
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }));

      expect(inputSystem.isBoosting()).toBe(true);
      expect(inputSystem.isCharging()).toBe(true);

      window.dispatchEvent(new Event('blur'));

      expect(inputSystem.isBoosting()).toBe(false);
      expect(inputSystem.isCharging()).toBe(false);
    });

    it('should reset input when document becomes hidden', () => {
      canvas.dispatchEvent(new MouseEvent('mousedown', { button: 0 }));

      expect(inputSystem.isBoosting()).toBe(true);

      // Mock document.hidden
      Object.defineProperty(document, 'hidden', { value: true, configurable: true });
      document.dispatchEvent(new Event('visibilitychange'));

      expect(inputSystem.isBoosting()).toBe(false);

      // Restore
      Object.defineProperty(document, 'hidden', { value: false, configurable: true });
    });
  });

  describe('combined mouse and keyboard boost', () => {
    it('should boost when either mouse or keyboard boost is active', () => {
      canvas.dispatchEvent(new MouseEvent('mousedown', { button: 0 }));
      expect(inputSystem.isBoosting()).toBe(true);

      canvas.dispatchEvent(new MouseEvent('mouseup', { button: 0 }));
      expect(inputSystem.isBoosting()).toBe(false);

      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'ShiftLeft' }));
      expect(inputSystem.isBoosting()).toBe(true);
    });

    it('should stay boosting when one input is released but other is held', () => {
      canvas.dispatchEvent(new MouseEvent('mousedown', { button: 0 }));
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'ShiftLeft' }));

      expect(inputSystem.isBoosting()).toBe(true);

      canvas.dispatchEvent(new MouseEvent('mouseup', { button: 0 }));
      expect(inputSystem.isBoosting()).toBe(true); // Shift still held
    });
  });
});
