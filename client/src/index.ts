// Orbit Royale Multiplayer Client Entry Point

import { Game, type GamePhase } from './core/Game';
import { Screens } from './ui/Screens';

// Browser compatibility check
function checkBrowserCompatibility(): string | null {
  // Check for WebTransport support (Chrome 97+)
  if (typeof WebTransport === 'undefined') {
    return 'Your browser does not support WebTransport. Please use Chrome 97+ or Edge 97+.';
  }

  // Check for canvas context
  const testCanvas = document.createElement('canvas');
  const ctx = testCanvas.getContext('2d');
  if (!ctx) {
    return 'Your browser does not support HTML5 Canvas.';
  }

  // Check for roundRect (Chrome 99+) - provide polyfill if missing
  if (typeof ctx.roundRect !== 'function') {
    // Polyfill roundRect for older browsers
    CanvasRenderingContext2D.prototype.roundRect = function(
      x: number, y: number, w: number, h: number, radii?: number | number[]
    ) {
      const r = typeof radii === 'number' ? radii : (radii?.[0] ?? 0);
      this.beginPath();
      this.moveTo(x + r, y);
      this.lineTo(x + w - r, y);
      this.quadraticCurveTo(x + w, y, x + w, y + r);
      this.lineTo(x + w, y + h - r);
      this.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
      this.lineTo(x + r, y + h);
      this.quadraticCurveTo(x, y + h, x, y + h - r);
      this.lineTo(x, y + r);
      this.quadraticCurveTo(x, y, x + r, y);
      this.closePath();
    };
  }

  return null; // Compatible
}

// Show compatibility error using safe DOM methods
function showCompatibilityError(message: string): void {
  const container = document.createElement('div');
  container.style.cssText = 'display:flex;align-items:center;justify-content:center;height:100vh;background:#0a0a1a;color:#ef4444;font-family:system-ui;text-align:center;padding:20px;';

  const content = document.createElement('div');

  const title = document.createElement('h1');
  title.style.cssText = 'color:#00ffff;margin-bottom:1rem;';
  title.textContent = 'Browser Not Supported';

  const errorText = document.createElement('p');
  errorText.textContent = message;

  const hint = document.createElement('p');
  hint.style.cssText = 'margin-top:1rem;color:#64748b;';
  hint.textContent = 'Orbit Royale requires a modern browser with WebTransport support.';

  content.appendChild(title);
  content.appendChild(errorText);
  content.appendChild(hint);
  container.appendChild(content);

  document.body.appendChild(container);
}

// Run compatibility check
const compatibilityError = checkBrowserCompatibility();
if (compatibilityError) {
  showCompatibilityError(compatibilityError);
  throw new Error(compatibilityError);
}

// Initialize canvas
const canvas = document.getElementById('game');
if (!(canvas instanceof HTMLCanvasElement)) {
  throw new Error('Canvas element #game not found');
}

canvas.width = window.innerWidth;
canvas.height = window.innerHeight;

// Initialize UI
const screens = new Screens();
screens.mount();

// Kill feed for displaying eliminations
const killFeed: { killer: string; victim: string; time: number }[] = [];

// Initialize game with event handlers
const game = new Game(canvas, {
  onPhaseChange: (phase: GamePhase) => {
    switch (phase) {
      case 'menu':
        screens.showMenu();
        break;
      case 'connecting':
        screens.showConnecting();
        break;
      case 'countdown':
      case 'playing':
        screens.hideAll();
        break;
      case 'ended':
        handleGameEnd();
        break;
      case 'disconnected':
        screens.showError('Connection lost');
        break;
    }
  },
  onKillFeed: (killerName: string, victimName: string) => {
    killFeed.push({ killer: killerName, victim: victimName, time: Date.now() });
    // Keep only last 5 entries
    while (killFeed.length > 5) {
      killFeed.shift();
    }
  },
  onConnectionError: (error: string) => {
    screens.showError(error);
  },
});

// Configure server URL
// Secure by default (localhost). Only use hostname for LAN in development mode.
const isDev = import.meta.env.VITE_IS_DEVELOPMENT === 'true';
const defaultServerUrl = isDev
  ? `https://${window.location.hostname}:4433`  // Dev: allows LAN testing
  : 'https://localhost:4433';                    // Prod: secure default
const serverUrl = import.meta.env.VITE_SERVER_URL || defaultServerUrl;
const certHash = import.meta.env.VITE_CERT_HASH;
game.setServer(serverUrl, certHash);

// Parse URL parameters for spectator mode
const urlParams = new URLSearchParams(window.location.search);
const isSpectatorFromUrl = urlParams.get('spectate') === '1';

// Handle window resize
window.addEventListener('resize', () => {
  canvas.width = window.innerWidth;
  canvas.height = window.innerHeight;
});

// Handle play button click
screens.onPlay(() => {
  const playerName = screens.getPlayerName();
  const colorIndex = screens.getSelectedColor();
  game.start(playerName, colorIndex, false);
});

// Handle spectate button click
screens.onSpectate(() => {
  const playerName = screens.getPlayerName() || 'Spectator';
  const colorIndex = screens.getSelectedColor();
  game.start(playerName, colorIndex, true);
});

// Handle restart button click
screens.onRestart(() => {
  screens.hideEnd();
  const playerName = screens.getPlayerName();
  const colorIndex = screens.getSelectedColor();
  game.start(playerName, colorIndex);
});

// Handle retry button click
screens.onRetry(() => {
  screens.hideError();
  const playerName = screens.getPlayerName();
  const colorIndex = screens.getSelectedColor();
  game.start(playerName, colorIndex);
});

// Handle game end
function handleGameEnd(): void {
  const world = game.getWorld();
  const localPlayer = world.getLocalPlayer();
  const isVictory = localPlayer?.alive && world.getAlivePlayerCount() === 1;
  const placement = world.localPlayerId
    ? world.getPlayerPlacement(world.localPlayerId)
    : world.getAlivePlayerCount() + 1;
  const kills = localPlayer?.kills || 0;

  // Small delay before showing end screen
  setTimeout(() => {
    screens.showEnd(isVictory ?? false, placement, kills);
  }, 1000);
}

// Handle keyboard shortcuts
window.addEventListener('keydown', (e) => {
  // ESC to return to menu
  if (e.code === 'Escape' && game.getPhase() !== 'menu') {
    game.disconnect();
    screens.showMenu();
  }

  // Space to restart when ended
  if (e.code === 'Space' && game.getPhase() === 'ended') {
    screens.hideEnd();
    const playerName = screens.getPlayerName();
    const colorIndex = screens.getSelectedColor();
    game.start(playerName, colorIndex);
  }

  // Enter to retry when disconnected
  if (e.code === 'Enter' && game.getPhase() === 'disconnected') {
    screens.hideError();
    const playerName = screens.getPlayerName();
    const colorIndex = screens.getSelectedColor();
    game.start(playerName, colorIndex);
  }
});

// Start with menu screen visible
screens.showMenu();

// Auto-join as spectator if URL parameter is set
if (isSpectatorFromUrl) {
  const playerName = screens.getPlayerName() || 'Spectator';
  const colorIndex = screens.getSelectedColor();
  game.start(playerName, colorIndex, true);
}
