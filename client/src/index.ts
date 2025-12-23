// Orbit Royale Multiplayer Client Entry Point

import { Game, type GamePhase } from './core/Game';
import { Screens } from './ui/Screens';

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

// Handle window resize
window.addEventListener('resize', () => {
  canvas.width = window.innerWidth;
  canvas.height = window.innerHeight;
});

// Handle play button click
screens.onPlay(() => {
  const playerName = screens.getPlayerName();
  game.start(playerName);
});

// Handle restart button click
screens.onRestart(() => {
  screens.hideEnd();
  const playerName = screens.getPlayerName();
  game.start(playerName);
});

// Handle retry button click
screens.onRetry(() => {
  screens.hideError();
  const playerName = screens.getPlayerName();
  game.start(playerName);
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
    game.start(playerName);
  }
});

// Start with menu screen visible
screens.showMenu();
