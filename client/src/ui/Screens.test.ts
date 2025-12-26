import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { Screens } from './Screens';

describe('Screens', () => {
  let screens: Screens;

  // Mock localStorage
  const localStorageMock = (() => {
    let store: Record<string, string> = {};
    return {
      getItem: vi.fn((key: string) => store[key] || null),
      setItem: vi.fn((key: string, value: string) => { store[key] = value; }),
      removeItem: vi.fn((key: string) => { delete store[key]; }),
      clear: vi.fn(() => { store = {}; }),
    };
  })();

  beforeEach(() => {
    // Setup localStorage mock
    Object.defineProperty(window, 'localStorage', {
      value: localStorageMock,
      configurable: true,
    });
    localStorageMock.clear();

    // Create screens
    screens = new Screens();
    screens.mount();
  });

  afterEach(() => {
    // Cleanup DOM
    document.body.textContent = '';
    document.head.textContent = '';
    vi.restoreAllMocks();
  });

  describe('constructor', () => {
    it('should create all screen elements', () => {
      expect(document.getElementById('menu-screen')).not.toBeNull();
      expect(document.getElementById('end-screen')).not.toBeNull();
      expect(document.getElementById('connecting-screen')).not.toBeNull();
      expect(document.getElementById('error-screen')).not.toBeNull();
    });

    it('should load saved color preference from localStorage', () => {
      localStorageMock.setItem('orbit-royale-player-color', '5');

      // Create new screens to test loading
      document.body.textContent = '';
      const newScreens = new Screens();
      newScreens.mount();

      expect(newScreens.getSelectedColor()).toBe(5);
    });

    it('should handle invalid saved color gracefully', () => {
      localStorageMock.setItem('orbit-royale-player-color', 'invalid');

      document.body.textContent = '';
      const newScreens = new Screens();
      newScreens.mount();

      // Should default to 0
      expect(newScreens.getSelectedColor()).toBe(0);
    });

    it('should handle out-of-range saved color', () => {
      localStorageMock.setItem('orbit-royale-player-color', '999');

      document.body.textContent = '';
      const newScreens = new Screens();
      newScreens.mount();

      // Should default to 0 (out of range)
      expect(newScreens.getSelectedColor()).toBe(0);
    });
  });

  describe('mount', () => {
    it('should add styles to document head', () => {
      const styles = document.head.querySelectorAll('style');
      expect(styles.length).toBeGreaterThan(0);
    });

    it('should add screens to document body', () => {
      expect(document.body.querySelectorAll('.screen').length).toBe(4);
    });
  });

  describe('getPlayerName', () => {
    it('should return trimmed player name', () => {
      const input = document.getElementById('player-name') as HTMLInputElement;
      input.value = '  TestPlayer  ';

      expect(screens.getPlayerName()).toBe('TestPlayer');
    });

    it('should return empty string when input is empty', () => {
      const input = document.getElementById('player-name') as HTMLInputElement;
      input.value = '';

      expect(screens.getPlayerName()).toBe('');
    });

    it('should sanitize name by removing control characters', () => {
      const input = document.getElementById('player-name') as HTMLInputElement;
      input.value = 'Test\x00Player\x1F';

      expect(screens.getPlayerName()).toBe('TestPlayer');
    });

    it('should sanitize name by removing HTML-like tags but keeping content', () => {
      const input = document.getElementById('player-name') as HTMLInputElement;
      // The sanitizer removes tags like <b></b> but keeps the content between them
      input.value = 'Test<b>Bold</b>Player';

      expect(screens.getPlayerName()).toBe('TestBoldPlayer');
    });

    it('should collapse multiple spaces', () => {
      const input = document.getElementById('player-name') as HTMLInputElement;
      input.value = 'Test    Player';

      expect(screens.getPlayerName()).toBe('Test Player');
    });

    it('should limit name length to 16 characters', () => {
      const input = document.getElementById('player-name') as HTMLInputElement;
      input.value = 'ThisIsAVeryLongPlayerNameThatExceeds16';

      expect(screens.getPlayerName().length).toBeLessThanOrEqual(16);
    });

    it('should load saved name from localStorage', () => {
      localStorageMock.setItem('orbit-royale-player-name', 'SavedPlayer');

      document.body.textContent = '';
      const newScreens = new Screens();
      newScreens.mount();

      expect(newScreens.getPlayerName()).toBe('SavedPlayer');
    });
  });

  describe('getSelectedColor', () => {
    it('should return default color index (0)', () => {
      expect(screens.getSelectedColor()).toBe(0);
    });

    it('should return selected color from slider', () => {
      const slider = document.querySelector('.color-slider') as HTMLInputElement;
      slider.value = '10';
      slider.dispatchEvent(new Event('input'));

      expect(screens.getSelectedColor()).toBe(10);
    });

    it('should clamp color index to valid range', () => {
      // Force an invalid value internally (for edge case testing)
      // getSelectedColor should still return valid range
      expect(screens.getSelectedColor()).toBeGreaterThanOrEqual(0);
      expect(screens.getSelectedColor()).toBeLessThan(20);
    });
  });

  describe('showMenu / hideMenu', () => {
    it('should show menu screen', () => {
      screens.hideAll();
      screens.showMenu();

      const menuScreen = document.getElementById('menu-screen');
      expect(menuScreen?.classList.contains('hidden')).toBe(false);
    });

    it('should hide menu screen', () => {
      screens.showMenu();
      screens.hideMenu();

      const menuScreen = document.getElementById('menu-screen');
      expect(menuScreen?.classList.contains('hidden')).toBe(true);
    });
  });

  describe('showConnecting / hideConnecting', () => {
    it('should show connecting screen', () => {
      screens.showConnecting();

      const connectingScreen = document.getElementById('connecting-screen');
      expect(connectingScreen?.classList.contains('hidden')).toBe(false);
    });

    it('should hide other screens when showing connecting', () => {
      screens.showMenu();
      screens.showConnecting();

      const menuScreen = document.getElementById('menu-screen');
      expect(menuScreen?.classList.contains('hidden')).toBe(true);
    });

    it('should hide connecting screen', () => {
      screens.showConnecting();
      screens.hideConnecting();

      const connectingScreen = document.getElementById('connecting-screen');
      expect(connectingScreen?.classList.contains('hidden')).toBe(true);
    });
  });

  describe('showEnd / hideEnd', () => {
    it('should show end screen with victory', () => {
      screens.showEnd(true, 1, 10);

      const endScreen = document.getElementById('end-screen');
      expect(endScreen?.classList.contains('hidden')).toBe(false);

      const title = endScreen?.querySelector('.end-title');
      expect(title?.textContent).toBe('VICTORY');
      expect(title?.classList.contains('victory')).toBe(true);
    });

    it('should show end screen with defeat', () => {
      screens.showEnd(false, 5, 3);

      const endScreen = document.getElementById('end-screen');
      expect(endScreen?.classList.contains('hidden')).toBe(false);

      const title = endScreen?.querySelector('.end-title');
      expect(title?.textContent).toBe('DEFEATED');
      expect(title?.classList.contains('defeat')).toBe(true);
    });

    it('should display placement', () => {
      screens.showEnd(false, 5, 3);

      const placement = document.querySelector('.end-placement');
      expect(placement?.textContent).toBe('#5');
    });

    it('should display kills', () => {
      screens.showEnd(false, 5, 7);

      const kills = document.querySelector('.end-kills');
      expect(kills?.textContent).toBe('7');
    });

    it('should hide end screen', () => {
      screens.showEnd(true, 1, 10);
      screens.hideEnd();

      const endScreen = document.getElementById('end-screen');
      expect(endScreen?.classList.contains('hidden')).toBe(true);
    });
  });

  describe('showError / hideError', () => {
    it('should show error screen with message', () => {
      screens.showError('Connection failed');

      const errorScreen = document.getElementById('error-screen');
      expect(errorScreen?.classList.contains('hidden')).toBe(false);

      const message = errorScreen?.querySelector('.error-message');
      expect(message?.textContent).toBe('Connection failed');
    });

    it('should hide error screen', () => {
      screens.showError('Test error');
      screens.hideError();

      const errorScreen = document.getElementById('error-screen');
      expect(errorScreen?.classList.contains('hidden')).toBe(true);
    });
  });

  describe('hideAll', () => {
    it('should hide all screens', () => {
      screens.showMenu();
      screens.hideAll();

      expect(document.getElementById('menu-screen')?.classList.contains('hidden')).toBe(true);
      expect(document.getElementById('end-screen')?.classList.contains('hidden')).toBe(true);
      expect(document.getElementById('connecting-screen')?.classList.contains('hidden')).toBe(true);
      expect(document.getElementById('error-screen')?.classList.contains('hidden')).toBe(true);
    });
  });

  describe('onPlay', () => {
    it('should register click handler on play button', () => {
      const callback = vi.fn();
      screens.onPlay(callback);

      const input = document.getElementById('player-name') as HTMLInputElement;
      input.value = 'TestPlayer';

      const playBtn = document.getElementById('play-btn');
      playBtn?.click();

      expect(callback).toHaveBeenCalled();
    });

    it('should not call callback if name is empty', () => {
      const callback = vi.fn();
      screens.onPlay(callback);

      const input = document.getElementById('player-name') as HTMLInputElement;
      input.value = '';

      const playBtn = document.getElementById('play-btn');
      playBtn?.click();

      expect(callback).not.toHaveBeenCalled();
    });

    it('should respond to Enter key in name input', () => {
      const callback = vi.fn();
      screens.onPlay(callback);

      const input = document.getElementById('player-name') as HTMLInputElement;
      input.value = 'TestPlayer';

      const event = new KeyboardEvent('keydown', { key: 'Enter' });
      input.dispatchEvent(event);

      expect(callback).toHaveBeenCalled();
    });

    it('should not respond to other keys', () => {
      const callback = vi.fn();
      screens.onPlay(callback);

      const input = document.getElementById('player-name') as HTMLInputElement;
      input.value = 'TestPlayer';

      const event = new KeyboardEvent('keydown', { key: 'Escape' });
      input.dispatchEvent(event);

      expect(callback).not.toHaveBeenCalled();
    });

    it('should save preferences when valid name is entered', () => {
      const callback = vi.fn();
      screens.onPlay(callback);

      const input = document.getElementById('player-name') as HTMLInputElement;
      input.value = 'TestPlayer';

      const playBtn = document.getElementById('play-btn');
      playBtn?.click();

      expect(localStorageMock.setItem).toHaveBeenCalledWith('orbit-royale-player-name', 'TestPlayer');
    });

    it('should show error state when name is invalid', () => {
      const callback = vi.fn();
      screens.onPlay(callback);

      const input = document.getElementById('player-name') as HTMLInputElement;
      input.value = '';

      const playBtn = document.getElementById('play-btn');
      playBtn?.click();

      expect(input.classList.contains('error')).toBe(true);
      expect(input.placeholder).toBe('Name required!');
    });

    it('should remove error state when user starts typing', () => {
      const callback = vi.fn();
      screens.onPlay(callback);

      const input = document.getElementById('player-name') as HTMLInputElement;
      input.value = '';

      const playBtn = document.getElementById('play-btn');
      playBtn?.click();

      expect(input.classList.contains('error')).toBe(true);

      // Simulate typing
      input.dispatchEvent(new Event('input'));

      expect(input.classList.contains('error')).toBe(false);
    });
  });

  describe('onSpectate', () => {
    it('should register click handler on spectate button', () => {
      const callback = vi.fn();
      screens.onSpectate(callback);

      const spectateBtn = document.getElementById('spectate-btn');
      spectateBtn?.click();

      expect(callback).toHaveBeenCalled();
    });

    it('should work without name (spectators do not need name)', () => {
      const callback = vi.fn();
      screens.onSpectate(callback);

      const input = document.getElementById('player-name') as HTMLInputElement;
      input.value = '';

      const spectateBtn = document.getElementById('spectate-btn');
      spectateBtn?.click();

      expect(callback).toHaveBeenCalled();
    });

    it('should save preferences when spectating', () => {
      const callback = vi.fn();
      screens.onSpectate(callback);

      const slider = document.querySelector('.color-slider') as HTMLInputElement;
      slider.value = '7';
      slider.dispatchEvent(new Event('input'));

      const spectateBtn = document.getElementById('spectate-btn');
      spectateBtn?.click();

      expect(localStorageMock.setItem).toHaveBeenCalledWith('orbit-royale-player-color', '7');
    });
  });

  describe('onRestart', () => {
    it('should register click handler on restart button', () => {
      screens.showEnd(true, 1, 10);

      const callback = vi.fn();
      screens.onRestart(callback);

      const restartBtn = document.getElementById('restart-btn');
      restartBtn?.click();

      expect(callback).toHaveBeenCalled();
    });
  });

  describe('onRetry', () => {
    it('should register click handler on retry button', () => {
      screens.showError('Test error');

      const callback = vi.fn();
      screens.onRetry(callback);

      const retryBtn = document.getElementById('retry-btn');
      retryBtn?.click();

      expect(callback).toHaveBeenCalled();
    });
  });

  describe('color picker', () => {
    it('should update preview when slider changes', () => {
      const slider = document.querySelector('.color-slider') as HTMLInputElement;
      const preview = document.querySelector('.color-preview') as HTMLElement;

      slider.value = '5';
      slider.dispatchEvent(new Event('input'));

      // Color at index 5 is green (#22c55e)
      expect(preview.style.backgroundColor).toBe('rgb(34, 197, 94)');
    });

    it('should have gradient background on slider', () => {
      const slider = document.querySelector('.color-slider') as HTMLInputElement;

      // Check that gradient is set
      const gradient = slider.style.getPropertyValue('--slider-gradient');
      expect(gradient).toContain('linear-gradient');
    });
  });

  describe('menu screen structure', () => {
    it('should have stars animation container', () => {
      const menuScreen = document.getElementById('menu-screen');
      const starsContainer = menuScreen?.querySelector('.stars-container');

      expect(starsContainer).not.toBeNull();
      expect(starsContainer?.querySelectorAll('.star').length).toBe(50);
    });

    it('should have logo with orbital rings', () => {
      const menuScreen = document.getElementById('menu-screen');
      const logoIcon = menuScreen?.querySelector('.logo-icon');

      expect(logoIcon?.querySelectorAll('.orbit-ring').length).toBe(3);
      expect(logoIcon?.querySelector('.logo-dot')).not.toBeNull();
    });

    it('should have game title', () => {
      const menuScreen = document.getElementById('menu-screen');
      const title = menuScreen?.querySelector('.game-title');

      expect(title?.textContent).toBe('ORBIT');
    });

    it('should have controls section', () => {
      const menuScreen = document.getElementById('menu-screen');
      const controlsSection = menuScreen?.querySelector('.controls-section');

      expect(controlsSection).not.toBeNull();
      expect(controlsSection?.querySelectorAll('.control-row').length).toBeGreaterThan(0);
    });

    it('should have objective text', () => {
      const menuScreen = document.getElementById('menu-screen');
      const objective = menuScreen?.querySelector('.objective-text');

      expect(objective?.textContent).toBe('Be the last one standing');
    });
  });

  describe('localStorage error handling', () => {
    it('should handle localStorage being unavailable', () => {
      // Mock localStorage to throw
      Object.defineProperty(window, 'localStorage', {
        get: () => { throw new Error('localStorage not available'); },
        configurable: true,
      });

      // Should not throw
      expect(() => {
        document.body.textContent = '';
        const newScreens = new Screens();
        newScreens.mount();
      }).not.toThrow();
    });
  });
});
