import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { GameTransport, ConnectionState, TransportEvents } from './Transport';

// Note: Full WebTransport mocking is complex. These tests focus on
// testable behaviors without requiring a complete mock implementation.

describe('GameTransport', () => {
  let transport: GameTransport;
  let events: TransportEvents;

  beforeEach(() => {
    events = {
      onStateChange: vi.fn(),
      onMessage: vi.fn(),
      onError: vi.fn(),
    };

    transport = new GameTransport(events);
  });

  afterEach(() => {
    transport.disconnect();
    vi.restoreAllMocks();
  });

  describe('constructor', () => {
    it('should initialize in disconnected state', () => {
      expect(transport.getState()).toBe('disconnected');
    });

    it('should have zero RTT initially', () => {
      expect(transport.getRtt()).toBe(0);
    });
  });

  describe('disconnect', () => {
    it('should handle disconnect when not connected', () => {
      expect(() => transport.disconnect()).not.toThrow();
    });

    it('should remain in disconnected state', () => {
      transport.disconnect();
      expect(transport.getState()).toBe('disconnected');
    });
  });

  describe('sendReliable', () => {
    it('should throw when not connected', async () => {
      await expect(transport.sendReliable({ type: 'Leave' })).rejects.toThrow('Not connected');
    });
  });

  describe('sendUnreliable', () => {
    it('should silently drop when not connected', () => {
      const input = {
        sequence: 1,
        tick: 100,
        clientTime: 1000,
        thrust: { x: 1, y: 0 },
        aim: { x: 1, y: 0 },
        boost: true,
        fire: false,
        fireReleased: false,
      };

      // Should not throw
      expect(() => transport.sendUnreliable(input as any)).not.toThrow();
    });
  });

  describe('sendPing', () => {
    it('should throw when not connected', async () => {
      await expect(transport.sendPing()).rejects.toThrow('Not connected');
    });
  });

  describe('getRtt', () => {
    it('should return 0 initially', () => {
      expect(transport.getRtt()).toBe(0);
    });
  });

  describe('getState', () => {
    it('should return disconnected by default', () => {
      expect(transport.getState()).toBe('disconnected');
    });
  });
});

describe('ConnectionState type', () => {
  it('should accept valid states', () => {
    const states: ConnectionState[] = ['disconnected', 'connecting', 'connected', 'error'];
    expect(states).toHaveLength(4);
  });
});

describe('TransportEvents interface', () => {
  it('should have required callback properties', () => {
    const events: TransportEvents = {
      onStateChange: vi.fn(),
      onMessage: vi.fn(),
      onError: vi.fn(),
    };

    expect(events.onStateChange).toBeDefined();
    expect(events.onMessage).toBeDefined();
    expect(events.onError).toBeDefined();
  });
});
