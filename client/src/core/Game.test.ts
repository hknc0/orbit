import { describe, it, expect, beforeEach, afterEach, vi, Mock } from 'vitest';
import { Game, GamePhase, GameEvents } from './Game';
import { Vec2 } from '@/utils/Vec2';

// Mock all dependencies using class syntax for proper constructor behavior
vi.mock('./World', () => {
  return {
    World: class MockWorld {
      reset = vi.fn();
      localPlayerId = null;
      isSpectator = false;
      spectateTargetId = null;
      arena = { collapsePhase: 0, isCollapsing: false };
      aiStatus = null;
      updateFromState = vi.fn();
      setSpectatorMode = vi.fn();
      setPlayerName = vi.fn();
      getPlayers = vi.fn(() => new Map());
      getPlayer = vi.fn();
      getLocalPlayer = vi.fn();
      getPlayerColor = vi.fn(() => '#ffffff');
      addCollisionEffect = vi.fn();
      addChargingWell = vi.fn();
      addGravityWaveEffect = vi.fn();
      removeGravityWell = vi.fn();
      getMatchTime = vi.fn(() => 0);
    },
  };
});

vi.mock('@/net/Transport', () => {
  return {
    GameTransport: class MockGameTransport {
      _events: any;
      connect = vi.fn().mockResolvedValue(undefined);
      disconnect = vi.fn();
      sendReliable = vi.fn().mockResolvedValue(undefined);
      sendUnreliable = vi.fn();
      getRtt = vi.fn(() => 50);
      getState = vi.fn(() => 'connected');

      constructor(events: any) {
        this._events = events;
      }
    },
  };
});

vi.mock('@/net/StateSync', () => {
  return {
    StateSync: class MockStateSync {
      reset = vi.fn();
      setLocalPlayerId = vi.fn();
      applySnapshot = vi.fn();
      applyDelta = vi.fn();
      getInterpolatedState = vi.fn(() => null);
      getCurrentTick = vi.fn(() => 100);
      recordInput = vi.fn();
      markWellDestroyed = vi.fn();
    },
  };
});

vi.mock('@/systems/InputSystem', () => {
  return {
    InputSystem: class MockInputSystem {
      reset = vi.fn();
      destroy = vi.fn();
      update = vi.fn();
      createInput = vi.fn(() => ({
        sequence: 0,
        tick: 100,
        clientTime: Date.now(),
        thrust: { x: 0, y: 0 },
        aim: { x: 1, y: 0 },
        boost: false,
        fire: false,
        fireReleased: false,
      }));
      getAimDirection = vi.fn(() => ({ x: 1, y: 0 }));
      getChargeRatio = vi.fn(() => 0);
      isCharging = vi.fn(() => false);
      isBoosting = vi.fn(() => false);

      constructor(_canvas: any) {}
    },
  };
});

vi.mock('@/systems/RenderSystem', () => {
  return {
    RenderSystem: class MockRenderSystem {
      reset = vi.fn();
      render = vi.fn();
      screenToWorld = vi.fn(() => ({ x: 0, y: 0 }));
      triggerShake = vi.fn();

      constructor(_ctx: any) {}
    },
  };
});

describe('Game', () => {
  let game: Game;
  let canvas: HTMLCanvasElement;
  let events: GameEvents;
  let mockCtx: Record<string, Mock>;

  beforeEach(() => {
    vi.useFakeTimers();

    // Mock requestAnimationFrame
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation(() => 1);
    vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => {});

    // Create mock canvas context
    mockCtx = {
      fillStyle: '',
      fillRect: vi.fn(),
      save: vi.fn(),
      restore: vi.fn(),
      translate: vi.fn(),
      scale: vi.fn(),
    };

    // Create mock canvas
    canvas = {
      getContext: vi.fn(() => mockCtx),
      width: 800,
      height: 600,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    } as unknown as HTMLCanvasElement;

    // Create event handlers
    events = {
      onPhaseChange: vi.fn(),
      onKillFeed: vi.fn(),
      onConnectionError: vi.fn(),
    };

    // Create game instance
    game = new Game(canvas, events);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  describe('constructor', () => {
    it('should initialize with menu phase', () => {
      expect(game.getPhase()).toBe('menu');
    });

    it('should throw if canvas context is unavailable', () => {
      const badCanvas = {
        getContext: vi.fn(() => null),
        addEventListener: vi.fn(),
      } as unknown as HTMLCanvasElement;

      expect(() => new Game(badCanvas, events)).toThrow('Failed to get 2D canvas context');
    });

    it('should set up spectator click listener', () => {
      expect(canvas.addEventListener).toHaveBeenCalledWith('click', expect.any(Function));
    });
  });

  describe('setServer', () => {
    it('should configure server URL and cert hash', () => {
      game.setServer('https://example.com:4433', 'test-cert-hash');
      // Verify by checking phase remains menu
      expect(game.getPhase()).toBe('menu');
    });
  });

  describe('getters', () => {
    it('should return current phase', () => {
      expect(game.getPhase()).toBe('menu');
    });

    it('should return world instance', () => {
      expect(game.getWorld()).toBeDefined();
    });

    it('should return RTT from transport', () => {
      expect(game.getRtt()).toBe(50);
    });
  });

  describe('disconnect', () => {
    it('should reset phase to menu', () => {
      game.disconnect();
      expect(game.getPhase()).toBe('menu');
    });
  });

  describe('destroy', () => {
    it('should not throw', () => {
      expect(() => game.destroy()).not.toThrow();
    });
  });

  describe('setSpectateTarget', () => {
    it('should not throw when setting target', () => {
      expect(() => game.setSpectateTarget('player-123')).not.toThrow();
    });

    it('should not throw when clearing target', () => {
      expect(() => game.setSpectateTarget(null)).not.toThrow();
    });
  });

  describe('switchToPlayer', () => {
    it('should not throw when switching', () => {
      expect(() => game.switchToPlayer(3)).not.toThrow();
    });
  });
});

describe('GamePhase type', () => {
  it('should accept valid phases', () => {
    const phases: GamePhase[] = ['menu', 'connecting', 'countdown', 'playing', 'ended', 'disconnected'];
    expect(phases).toHaveLength(6);
  });
});

describe('GameEvents interface', () => {
  it('should have required callback properties', () => {
    const events: GameEvents = {
      onPhaseChange: vi.fn(),
      onKillFeed: vi.fn(),
      onConnectionError: vi.fn(),
    };

    expect(events.onPhaseChange).toBeDefined();
    expect(events.onKillFeed).toBeDefined();
    expect(events.onConnectionError).toBeDefined();
  });
});
