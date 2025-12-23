// Render system for multiplayer client
// Adapted from orbit-poc to work with server state

import { World } from '@/core/World';
import { Vec2 } from '@/utils/Vec2';
import type { GamePhase } from '@/core/Game';
import type { ConnectionState } from '@/net/Transport';

interface InputState {
  aimDirection: Vec2;
  chargeRatio: number;
  isCharging: boolean;
  isBoosting: boolean;
}

interface RenderState {
  phase: GamePhase;
  matchTime: number;
  countdownTime: number;
  input?: InputState;
  rtt: number;
  connectionState: ConnectionState;
}

export class RenderSystem {
  private ctx: CanvasRenderingContext2D;
  private cameraOffset: Vec2 = new Vec2();
  private targetCameraOffset: Vec2 = new Vec2();
  private readonly CAMERA_SMOOTHING = 0.1;

  constructor(ctx: CanvasRenderingContext2D) {
    this.ctx = ctx;
  }

  render(world: World, state: RenderState): void {
    const canvas = this.ctx.canvas;
    const centerX = canvas.width / 2;
    const centerY = canvas.height / 2;

    // Update camera to follow local player
    const localPlayer = world.getLocalPlayer();
    if (localPlayer) {
      this.targetCameraOffset.set(
        centerX - localPlayer.position.x,
        centerY - localPlayer.position.y
      );
    } else {
      this.targetCameraOffset.set(centerX, centerY);
    }

    // Smooth camera interpolation
    this.cameraOffset.x +=
      (this.targetCameraOffset.x - this.cameraOffset.x) * this.CAMERA_SMOOTHING;
    this.cameraOffset.y +=
      (this.targetCameraOffset.y - this.cameraOffset.y) * this.CAMERA_SMOOTHING;

    this.ctx.save();
    this.ctx.translate(this.cameraOffset.x, this.cameraOffset.y);

    // Render in order (back to front)
    this.renderArena(world);
    this.renderDeathEffects(world);
    this.renderProjectiles(world);
    this.renderPlayers(world, state.input?.isBoosting ?? false);

    // Render aim indicator
    if (state.input && (state.phase === 'playing' || state.phase === 'countdown')) {
      this.renderAimIndicator(world, state.input);
    }

    this.ctx.restore();

    // Render UI overlay
    this.renderHUD(world, state);

    // Render charge indicator
    if (state.input?.isCharging && state.phase === 'playing') {
      this.renderChargeIndicator(state.input.chargeRatio);
    }

    // Render countdown
    if (state.phase === 'countdown') {
      this.renderCountdown(state.countdownTime);
    }

    // Render connection status
    this.renderConnectionStatus(state);
  }

  private renderAimIndicator(world: World, input: InputState): void {
    const localPlayer = world.getLocalPlayer();
    if (!localPlayer || !localPlayer.alive) return;

    const radius = world.massToRadius(localPlayer.mass);
    const aimLength = radius + 30 + (input.isCharging ? input.chargeRatio * 20 : 0);

    const startX = localPlayer.position.x + input.aimDirection.x * radius;
    const startY = localPlayer.position.y + input.aimDirection.y * radius;
    const endX = localPlayer.position.x + input.aimDirection.x * aimLength;
    const endY = localPlayer.position.y + input.aimDirection.y * aimLength;

    // Aim line
    this.ctx.strokeStyle = input.isCharging
      ? `rgba(255, ${255 - input.chargeRatio * 155}, 100, 0.8)`
      : input.isBoosting
        ? 'rgba(100, 200, 255, 0.6)'
        : 'rgba(255, 255, 255, 0.4)';
    this.ctx.lineWidth = input.isCharging ? 3 : 2;
    this.ctx.setLineDash(input.isCharging ? [] : [5, 5]);
    this.ctx.beginPath();
    this.ctx.moveTo(startX, startY);
    this.ctx.lineTo(endX, endY);
    this.ctx.stroke();
    this.ctx.setLineDash([]);

    // Crosshair dot
    this.ctx.fillStyle = input.isCharging
      ? `rgba(255, ${255 - input.chargeRatio * 155}, 100, 0.9)`
      : 'rgba(255, 255, 255, 0.5)';
    this.ctx.beginPath();
    this.ctx.arc(endX, endY, input.isCharging ? 4 : 3, 0, Math.PI * 2);
    this.ctx.fill();
  }

  private renderChargeIndicator(chargeRatio: number): void {
    const canvas = this.ctx.canvas;
    const barWidth = 150;
    const barHeight = 8;
    const x = (canvas.width - barWidth) / 2;
    const y = canvas.height - 60;

    // Background
    this.ctx.fillStyle = 'rgba(0, 0, 0, 0.5)';
    this.ctx.fillRect(x - 2, y - 2, barWidth + 4, barHeight + 4);

    // Charge bar background
    this.ctx.fillStyle = 'rgba(100, 100, 100, 0.5)';
    this.ctx.fillRect(x, y, barWidth, barHeight);

    // Charge fill
    const gradient = this.ctx.createLinearGradient(x, 0, x + barWidth, 0);
    gradient.addColorStop(0, '#fbbf24');
    gradient.addColorStop(0.5, '#f97316');
    gradient.addColorStop(1, '#ef4444');
    this.ctx.fillStyle = gradient;
    this.ctx.fillRect(x, y, barWidth * chargeRatio, barHeight);

    // Border
    this.ctx.strokeStyle = 'rgba(255, 255, 255, 0.3)';
    this.ctx.lineWidth = 1;
    this.ctx.strokeRect(x, y, barWidth, barHeight);

    // Label
    this.ctx.fillStyle = '#ffffff';
    this.ctx.font = '10px Inter, system-ui, sans-serif';
    this.ctx.textAlign = 'center';
    this.ctx.fillText('CHARGING', canvas.width / 2, y - 5);
  }

  private renderArena(world: World): void {
    const safeRadius = world.getArenaSafeRadius();
    const wells = world.arena.gravityWells;

    // Universe boundary (dynamic, contains all wells)
    this.ctx.strokeStyle = 'rgba(100, 100, 150, 0.3)';
    this.ctx.lineWidth = 2;
    this.ctx.beginPath();
    this.ctx.arc(0, 0, safeRadius, 0, Math.PI * 2);
    this.ctx.stroke();

    // Render gravity wells (suns) - each is its own "solar system"
    if (wells.length > 0) {
      for (const well of wells) {
        this.renderGravityWell(well.position.x, well.position.y, well.coreRadius, well.mass);

        // Draw orbit zones around each well (subtle rings)
        this.renderWellZones(well.position.x, well.position.y, well.coreRadius);
      }
    } else {
      // Fallback: single well at center
      this.renderGravityWell(0, 0, world.arena.coreRadius, 1000000);
      this.renderWellZones(0, 0, world.arena.coreRadius);
    }

    // Collapse warning
    if (world.arena.isCollapsing) {
      this.ctx.strokeStyle = `rgba(255, 100, 100, ${0.5 + Math.sin(Date.now() / 100) * 0.3})`;
      this.ctx.lineWidth = 3;
      for (const well of wells.length > 0 ? wells : [{ position: { x: 0, y: 0 }, coreRadius: world.arena.coreRadius }]) {
        this.ctx.beginPath();
        this.ctx.arc(well.position.x, well.position.y, well.coreRadius, 0, Math.PI * 2);
        this.ctx.stroke();
      }
    }
  }

  private renderWellZones(x: number, y: number, coreRadius: number): void {
    // Inner safe zone (spawn area)
    this.ctx.strokeStyle = 'rgba(120, 120, 160, 0.2)';
    this.ctx.lineWidth = 1;
    this.ctx.beginPath();
    this.ctx.arc(x, y, coreRadius * 4, 0, Math.PI * 2); // ~200 units for 50 core
    this.ctx.stroke();

    // Middle zone
    this.ctx.strokeStyle = 'rgba(100, 100, 140, 0.15)';
    this.ctx.beginPath();
    this.ctx.arc(x, y, coreRadius * 8, 0, Math.PI * 2); // ~400 units
    this.ctx.stroke();

    // Outer zone
    this.ctx.strokeStyle = 'rgba(80, 80, 120, 0.1)';
    this.ctx.beginPath();
    this.ctx.arc(x, y, coreRadius * 12, 0, Math.PI * 2); // ~600 units
    this.ctx.stroke();
  }

  private renderGravityWell(x: number, y: number, coreRadius: number, mass: number): void {
    // Outer glow based on mass
    const glowRadius = coreRadius * (1.5 + Math.log10(mass) * 0.1);
    const outerGlow = this.ctx.createRadialGradient(x, y, coreRadius * 0.5, x, y, glowRadius);
    outerGlow.addColorStop(0, 'rgba(255, 150, 50, 0.3)');
    outerGlow.addColorStop(0.5, 'rgba(255, 100, 30, 0.15)');
    outerGlow.addColorStop(1, 'rgba(255, 50, 0, 0)');
    this.ctx.fillStyle = outerGlow;
    this.ctx.beginPath();
    this.ctx.arc(x, y, glowRadius, 0, Math.PI * 2);
    this.ctx.fill();

    // Core death zone gradient
    const gradient = this.ctx.createRadialGradient(x, y, 0, x, y, coreRadius);
    gradient.addColorStop(0, 'rgba(255, 200, 100, 0.9)');
    gradient.addColorStop(0.3, 'rgba(255, 150, 50, 0.8)');
    gradient.addColorStop(0.7, 'rgba(200, 80, 30, 0.6)');
    gradient.addColorStop(1, 'rgba(150, 40, 20, 0.3)');

    this.ctx.fillStyle = gradient;
    this.ctx.beginPath();
    this.ctx.arc(x, y, coreRadius, 0, Math.PI * 2);
    this.ctx.fill();
  }

  private renderProjectiles(world: World): void {
    for (const proj of world.getProjectiles().values()) {
      const ownerPlayer = world.getPlayer(proj.ownerId);
      const color = ownerPlayer
        ? world.getPlayerColor(ownerPlayer.colorIndex)
        : '#ffffff';
      const radius = world.massToRadius(proj.mass);

      // Outer glow
      this.ctx.fillStyle = this.colorWithAlpha(color, 0.3);
      this.ctx.beginPath();
      this.ctx.arc(proj.position.x, proj.position.y, radius * 1.3, 0, Math.PI * 2);
      this.ctx.fill();

      // Core
      this.ctx.fillStyle = color;
      this.ctx.beginPath();
      this.ctx.arc(proj.position.x, proj.position.y, radius, 0, Math.PI * 2);
      this.ctx.fill();
    }
  }

  private renderPlayers(world: World, localPlayerBoosting: boolean): void {
    for (const player of world.getPlayers().values()) {
      if (!player.alive) continue;

      const radius = world.massToRadius(player.mass);
      const color = world.getPlayerColor(player.colorIndex);
      const isLocal = player.id === world.localPlayerId;

      // Render boost flame for local player
      if (isLocal && localPlayerBoosting) {
        this.renderBoostFlame(player.position, player.velocity, radius);
      }

      // Kill effect - golden pulsing glow when player gets a kill
      const killProgress = world.getKillEffectProgress(player.id);
      if (killProgress > 0) {
        this.renderKillEffect(player.position, radius, killProgress);
      }

      // Spawn protection indicator
      if (player.spawnProtection) {
        this.ctx.strokeStyle = 'rgba(255, 255, 255, 0.5)';
        this.ctx.lineWidth = 2;
        this.ctx.setLineDash([5, 5]);
        this.ctx.beginPath();
        this.ctx.arc(player.position.x, player.position.y, radius + 5, 0, Math.PI * 2);
        this.ctx.stroke();
        this.ctx.setLineDash([]);
      }

      // Player body with gradient
      const gradient = this.ctx.createRadialGradient(
        player.position.x - radius * 0.3,
        player.position.y - radius * 0.3,
        0,
        player.position.x,
        player.position.y,
        radius
      );
      gradient.addColorStop(0, this.lightenColor(color, 30));
      gradient.addColorStop(1, color);

      this.ctx.fillStyle = gradient;
      this.ctx.beginPath();
      this.ctx.arc(player.position.x, player.position.y, radius, 0, Math.PI * 2);
      this.ctx.fill();

      // Direction indicator
      const dirX = Math.cos(player.rotation);
      const dirY = Math.sin(player.rotation);
      this.ctx.strokeStyle = 'rgba(255, 255, 255, 0.7)';
      this.ctx.lineWidth = 2;
      this.ctx.beginPath();
      this.ctx.moveTo(player.position.x, player.position.y);
      this.ctx.lineTo(
        player.position.x + dirX * radius * 0.8,
        player.position.y + dirY * radius * 0.8
      );
      this.ctx.stroke();

      // Local player highlight
      if (isLocal) {
        this.ctx.strokeStyle = 'rgba(255, 255, 255, 0.3)';
        this.ctx.lineWidth = 2;
        this.ctx.beginPath();
        this.ctx.arc(player.position.x, player.position.y, radius + 3, 0, Math.PI * 2);
        this.ctx.stroke();
      }

      // Player name
      this.ctx.fillStyle = '#ffffff';
      this.ctx.font = '12px Inter, system-ui, sans-serif';
      this.ctx.textAlign = 'center';
      this.ctx.fillText(
        world.getPlayerName(player.id),
        player.position.x,
        player.position.y - radius - 10
      );
    }
  }

  private renderKillEffect(position: Vec2, radius: number, progress: number): void {
    // Expanding ring effect
    const ringRadius = radius + 10 + (1 - progress) * 30;
    const alpha = progress * 0.8;

    // Outer glow
    const glowRadius = radius + progress * 15;
    const glowGradient = this.ctx.createRadialGradient(
      position.x, position.y, radius,
      position.x, position.y, glowRadius
    );
    glowGradient.addColorStop(0, `rgba(255, 200, 50, ${alpha * 0.5})`);
    glowGradient.addColorStop(1, 'rgba(255, 200, 50, 0)');

    this.ctx.fillStyle = glowGradient;
    this.ctx.beginPath();
    this.ctx.arc(position.x, position.y, glowRadius, 0, Math.PI * 2);
    this.ctx.fill();

    // Expanding ring
    this.ctx.strokeStyle = `rgba(255, 220, 100, ${alpha})`;
    this.ctx.lineWidth = 3 * progress;
    this.ctx.beginPath();
    this.ctx.arc(position.x, position.y, ringRadius, 0, Math.PI * 2);
    this.ctx.stroke();
  }

  private renderDeathEffects(world: World): void {
    const effects = world.getDeathEffects();

    for (const effect of effects) {
      const { position, progress, color } = effect;

      // Multiple expanding rings
      const numRings = 3;
      for (let i = 0; i < numRings; i++) {
        const ringProgress = Math.max(0, progress - i * 0.15);
        if (ringProgress <= 0) continue;

        const ringRadius = 10 + (1 - ringProgress) * (60 + i * 20);
        const alpha = ringProgress * 0.7;

        this.ctx.strokeStyle = this.colorWithAlpha(color, alpha);
        this.ctx.lineWidth = 4 * ringProgress;
        this.ctx.beginPath();
        this.ctx.arc(position.x, position.y, ringRadius, 0, Math.PI * 2);
        this.ctx.stroke();
      }

      // Central flash
      if (progress > 0.5) {
        const flashProgress = (progress - 0.5) * 2;
        const flashRadius = 30 * flashProgress;

        const flashGradient = this.ctx.createRadialGradient(
          position.x, position.y, 0,
          position.x, position.y, flashRadius
        );
        flashGradient.addColorStop(0, `rgba(255, 255, 255, ${flashProgress * 0.8})`);
        flashGradient.addColorStop(0.5, this.colorWithAlpha(color, flashProgress * 0.5));
        flashGradient.addColorStop(1, 'rgba(255, 255, 255, 0)');

        this.ctx.fillStyle = flashGradient;
        this.ctx.beginPath();
        this.ctx.arc(position.x, position.y, flashRadius, 0, Math.PI * 2);
        this.ctx.fill();
      }

      // Particle burst (simple dots flying outward)
      const numParticles = 8;
      for (let i = 0; i < numParticles; i++) {
        const angle = (i / numParticles) * Math.PI * 2;
        const distance = (1 - progress) * 50 + 10;
        const particleX = position.x + Math.cos(angle) * distance;
        const particleY = position.y + Math.sin(angle) * distance;
        const particleSize = 3 * progress;

        this.ctx.fillStyle = this.colorWithAlpha(color, progress * 0.8);
        this.ctx.beginPath();
        this.ctx.arc(particleX, particleY, particleSize, 0, Math.PI * 2);
        this.ctx.fill();
      }
    }
  }

  private renderBoostFlame(position: Vec2, velocity: Vec2, radius: number): void {
    const speed = velocity.length();
    if (speed < 10) return;

    // Flame direction is opposite to velocity
    const dirX = -velocity.x / speed;
    const dirY = -velocity.y / speed;

    const flameX = position.x + dirX * radius;
    const flameY = position.y + dirY * radius;
    const flameLen = Math.min(radius * 1.5, 15 + speed * 0.05);
    const flameWidth = radius * 0.4;

    const perpX = -dirY;
    const perpY = dirX;

    // Outer flame
    this.ctx.fillStyle = 'rgba(255, 150, 50, 0.8)';
    this.ctx.beginPath();
    this.ctx.moveTo(flameX + perpX * flameWidth, flameY + perpY * flameWidth);
    this.ctx.lineTo(flameX - perpX * flameWidth, flameY - perpY * flameWidth);
    this.ctx.lineTo(flameX + dirX * flameLen, flameY + dirY * flameLen);
    this.ctx.closePath();
    this.ctx.fill();

    // Inner flame
    this.ctx.fillStyle = 'rgba(255, 220, 100, 0.9)';
    this.ctx.beginPath();
    this.ctx.moveTo(flameX + perpX * flameWidth * 0.5, flameY + perpY * flameWidth * 0.5);
    this.ctx.lineTo(flameX - perpX * flameWidth * 0.5, flameY - perpY * flameWidth * 0.5);
    this.ctx.lineTo(flameX + dirX * flameLen * 0.6, flameY + dirY * flameLen * 0.6);
    this.ctx.closePath();
    this.ctx.fill();
  }

  private renderHUD(world: World, _state: RenderState): void {
    const canvas = this.ctx.canvas;
    const padding = 16;
    const localPlayer = world.getLocalPlayer();
    const sessionStats = world.getSessionStats();

    // === LEFT PANEL - Player Stats ===
    if (localPlayer) {
      const panelX = padding;
      const panelY = padding;
      const panelW = 180;
      const panelH = 150;

      // Panel background
      this.drawPanel(panelX, panelY, panelW, panelH);

      // Rank badge
      const aliveCount = world.getAlivePlayerCount();
      if (localPlayer.alive) {
        const placement = world.getPlayerPlacement(world.localPlayerId!);
        const rankBadgeColors = ['#FFD700', '#E2E8F0', '#CD853F']; // Gold, Silver, Bronze
        this.ctx.fillStyle = placement <= 3 ? rankBadgeColors[placement - 1] : '#94a3b8';
        this.ctx.font = 'bold 24px Inter, system-ui, sans-serif';
        this.ctx.textAlign = 'left';
        const rankText = `#${placement}`;
        this.ctx.fillText(rankText, panelX + 12, panelY + 30);
        const rankWidth = this.ctx.measureText(rankText).width;
        this.ctx.fillStyle = '#64748b';
        this.ctx.font = '11px Inter, system-ui, sans-serif';
        this.ctx.fillText(`of ${aliveCount}`, panelX + 16 + rankWidth, panelY + 30);
      } else {
        // Dead state
        this.ctx.fillStyle = '#ef4444';
        this.ctx.font = 'bold 18px Inter, system-ui, sans-serif';
        this.ctx.textAlign = 'left';
        const deadText = 'DEAD';
        this.ctx.fillText(deadText, panelX + 12, panelY + 30);
        const deadWidth = this.ctx.measureText(deadText).width;
        this.ctx.fillStyle = '#64748b';
        this.ctx.font = '11px Inter, system-ui, sans-serif';
        this.ctx.fillText(`${aliveCount} alive`, panelX + 16 + deadWidth, panelY + 30);
      }

      // Mass with bar
      const massPercent = Math.min(localPlayer.mass / 500, 1);
      const barStartX = panelX + 55;
      const barWidth = panelW - 67;
      this.ctx.fillStyle = '#94a3b8';
      this.ctx.font = '10px Inter, system-ui, sans-serif';
      this.ctx.fillText('MASS', panelX + 12, panelY + 50);
      this.ctx.fillStyle = '#f1f5f9';
      this.ctx.font = 'bold 14px Inter, system-ui, sans-serif';
      this.ctx.fillText(Math.floor(localPlayer.mass).toString(), panelX + 12, panelY + 66);
      this.drawProgressBar(barStartX, panelY + 56, barWidth, 8, massPercent, '#3b82f6');

      // Speed indicator
      const speed = localPlayer.velocity.length();
      const speedPercent = Math.min(speed / 300, 1);
      this.ctx.fillStyle = '#94a3b8';
      this.ctx.font = '10px Inter, system-ui, sans-serif';
      this.ctx.fillText('SPEED', panelX + 12, panelY + 86);
      this.ctx.fillStyle = '#f1f5f9';
      this.ctx.font = 'bold 14px Inter, system-ui, sans-serif';
      this.ctx.fillText(Math.floor(speed).toString(), panelX + 12, panelY + 102);
      this.drawProgressBar(barStartX, panelY + 92, barWidth, 8, speedPercent, '#22c55e');

      // K/D Stats
      this.ctx.fillStyle = '#94a3b8';
      this.ctx.font = '10px Inter, system-ui, sans-serif';
      this.ctx.fillText('K / D', panelX + 12, panelY + 120);
      this.ctx.fillStyle = '#f1f5f9';
      this.ctx.font = 'bold 14px Inter, system-ui, sans-serif';
      this.ctx.fillText(`${localPlayer.kills} / ${localPlayer.deaths}`, panelX + 12, panelY + 136);

      // Kill streak
      if (sessionStats.killStreak > 0) {
        this.ctx.fillStyle = '#fbbf24';
        this.ctx.font = 'bold 14px Inter, system-ui, sans-serif';
        this.ctx.fillText(`🔥 ${sessionStats.killStreak}`, panelX + 80, panelY + 136);
      }

      // Time alive
      const timeAlive = Math.floor(sessionStats.timeAlive / 1000);
      const minutes = Math.floor(timeAlive / 60);
      const seconds = timeAlive % 60;
      this.ctx.fillStyle = '#64748b';
      this.ctx.font = '11px Inter, system-ui, sans-serif';
      this.ctx.textAlign = 'right';
      this.ctx.fillText(`⏱ ${minutes}:${seconds.toString().padStart(2, '0')}`, panelX + panelW - 12, panelY + 136);
    }

    // === BOTTOM LEFT - Session Stats ===
    if (localPlayer) {
      const panelX = padding;
      const panelY = canvas.height - padding - 80;
      const panelW = 160;
      const panelH = 80;

      this.drawPanel(panelX, panelY, panelW, panelH);

      this.ctx.fillStyle = '#94a3b8';
      this.ctx.font = '10px Inter, system-ui, sans-serif';
      this.ctx.textAlign = 'left';
      this.ctx.fillText('SESSION', panelX + 12, panelY + 18);

      this.ctx.fillStyle = '#f1f5f9';
      this.ctx.font = '11px Inter, system-ui, sans-serif';
      this.ctx.fillText(`Best Mass: ${Math.floor(sessionStats.bestMass)}`, panelX + 12, panelY + 35);
      this.ctx.fillText(`Best Streak: ${sessionStats.bestKillStreak}`, panelX + 12, panelY + 50);
      const bestSeconds = Math.floor(sessionStats.bestTimeAlive / 1000);
      const bestMin = Math.floor(bestSeconds / 60);
      const bestSec = bestSeconds % 60;
      this.ctx.fillText(`Best Time: ${bestMin}:${bestSec.toString().padStart(2, '0')}`, panelX + 12, panelY + 65);
    }

    // === RIGHT PANEL - Leaderboard ===
    const leaderboard = world.getLeaderboard().slice(0, 5);
    const lbPanelW = 170;
    const lbPanelH = 30 + leaderboard.length * 22;
    const lbPanelX = canvas.width - padding - lbPanelW;
    const lbPanelY = padding;

    this.drawPanel(lbPanelX, lbPanelY, lbPanelW, lbPanelH);

    this.ctx.fillStyle = '#94a3b8';
    this.ctx.font = 'bold 10px Inter, system-ui, sans-serif';
    this.ctx.textAlign = 'left';
    this.ctx.fillText('LEADERBOARD', lbPanelX + 12, lbPanelY + 18);

    const rankColors = ['#FFD700', '#E2E8F0', '#CD853F']; // Gold, Silver, Bronze
    leaderboard.forEach((entry, index) => {
      const isLocal = entry.id === world.localPlayerId;
      const y = lbPanelY + 38 + index * 22;

      // Rank number with medal colors for top 3
      this.ctx.fillStyle = index < 3 ? rankColors[index] : '#64748b';
      this.ctx.font = 'bold 12px Inter, system-ui, sans-serif';
      this.ctx.textAlign = 'left';
      this.ctx.fillText(`${index + 1}`, lbPanelX + 12, y);

      // Name - highlight local player
      this.ctx.fillStyle = isLocal ? '#60a5fa' : '#e2e8f0';
      this.ctx.font = `${isLocal ? 'bold ' : ''}11px Inter, system-ui, sans-serif`;
      const name = entry.name.length > 10 ? entry.name.slice(0, 10) + '…' : entry.name;
      this.ctx.fillText(name, lbPanelX + 28, y);

      // Mass
      this.ctx.fillStyle = '#94a3b8';
      this.ctx.font = '11px Inter, system-ui, sans-serif';
      this.ctx.textAlign = 'right';
      this.ctx.fillText(Math.floor(entry.mass).toString(), lbPanelX + lbPanelW - 12, y);
    });

    // === DANGER ZONE INDICATOR ===
    if (localPlayer && localPlayer.alive) {
      const distFromCenter = localPlayer.position.length();
      const safeRadius = world.getArenaSafeRadius();

      // Check arena boundary danger
      let dangerLevel = 0;
      let dangerType = '';

      if (distFromCenter > safeRadius * 0.8) {
        dangerLevel = Math.min((distFromCenter - safeRadius * 0.8) / (safeRadius * 0.2), 1);
        dangerType = 'LEAVING ARENA';
      }

      // Check proximity to well cores (instant death zones)
      for (const well of world.arena.gravityWells) {
        const dx = localPlayer.position.x - well.position.x;
        const dy = localPlayer.position.y - well.position.y;
        const distToWell = Math.sqrt(dx * dx + dy * dy);
        const dangerRadius = well.coreRadius * 3; // Warning zone

        if (distToWell < dangerRadius) {
          const coreDanger = 1 - (distToWell - well.coreRadius) / (dangerRadius - well.coreRadius);
          if (coreDanger > dangerLevel) {
            dangerLevel = Math.min(coreDanger, 1);
            dangerType = 'CORE PROXIMITY';
          }
        }
      }

      if (dangerLevel > 0) {
        const pulse = Math.sin(Date.now() / 150) * 0.3 + 0.7;

        this.ctx.fillStyle = `rgba(239, 68, 68, ${dangerLevel * pulse * 0.8})`;
        this.ctx.font = 'bold 14px Inter, system-ui, sans-serif';
        this.ctx.textAlign = 'center';
        this.ctx.fillText(`⚠ ${dangerType}`, canvas.width / 2, canvas.height - 60);
      }
    }

    // === CONTROLS HINT ===
    this.ctx.fillStyle = '#475569';
    this.ctx.font = '11px Inter, system-ui, sans-serif';
    this.ctx.textAlign = 'center';
    this.ctx.fillText(
      'W/LMB: Boost  •  SPACE: Eject Mass',
      canvas.width / 2,
      canvas.height - padding - 5
    );

    // === MINIMAP ===
    this.renderMinimap(world, canvas, padding);
  }

  private renderMinimap(world: World, canvas: HTMLCanvasElement, padding: number): void {
    const minimapSize = 120;
    const minimapX = canvas.width - padding - minimapSize;
    const minimapY = canvas.height - padding - minimapSize;
    const centerX = minimapX + minimapSize / 2;
    const centerY = minimapY + minimapSize / 2;
    const safeRadius = world.getArenaSafeRadius();
    const scale = (minimapSize / 2 - 4) / safeRadius;

    // Minimap background
    this.ctx.fillStyle = 'rgba(15, 23, 42, 0.85)';
    this.ctx.beginPath();
    this.ctx.arc(centerX, centerY, minimapSize / 2, 0, Math.PI * 2);
    this.ctx.fill();

    // Border
    this.ctx.strokeStyle = 'rgba(100, 116, 139, 0.4)';
    this.ctx.lineWidth = 1;
    this.ctx.stroke();

    // Safe zone boundary on minimap
    this.ctx.strokeStyle = 'rgba(80, 80, 120, 0.5)';
    this.ctx.beginPath();
    this.ctx.arc(centerX, centerY, safeRadius * scale, 0, Math.PI * 2);
    this.ctx.stroke();

    // Gravity wells as bright orange/yellow suns - highly visible
    for (const well of world.arena.gravityWells) {
      const wellX = centerX + well.position.x * scale;
      const wellY = centerY + well.position.y * scale;
      // Minimum 6 pixels so wells are always visible even in large universes
      const wellRadius = Math.max(6, well.coreRadius * scale);

      // Outer glow - large and bright
      const outerGlow = this.ctx.createRadialGradient(wellX, wellY, 0, wellX, wellY, wellRadius * 4);
      outerGlow.addColorStop(0, 'rgba(255, 200, 100, 0.8)');
      outerGlow.addColorStop(0.3, 'rgba(255, 150, 50, 0.5)');
      outerGlow.addColorStop(1, 'rgba(255, 50, 0, 0)');
      this.ctx.fillStyle = outerGlow;
      this.ctx.beginPath();
      this.ctx.arc(wellX, wellY, wellRadius * 4, 0, Math.PI * 2);
      this.ctx.fill();

      // Core - bright yellow/white center
      const coreGradient = this.ctx.createRadialGradient(wellX, wellY, 0, wellX, wellY, wellRadius);
      coreGradient.addColorStop(0, '#ffffcc'); // Bright yellow-white
      coreGradient.addColorStop(0.5, '#ffcc00'); // Golden
      coreGradient.addColorStop(1, '#ff6600'); // Orange
      this.ctx.fillStyle = coreGradient;
      this.ctx.beginPath();
      this.ctx.arc(wellX, wellY, wellRadius, 0, Math.PI * 2);
      this.ctx.fill();
    }

    // If no gravity wells synced yet, show center dot
    if (world.arena.gravityWells.length === 0) {
      this.ctx.fillStyle = '#ff6600';
      this.ctx.beginPath();
      this.ctx.arc(centerX, centerY, 3, 0, Math.PI * 2);
      this.ctx.fill();
    }

    // Other players as small dots
    for (const [playerId, player] of world.getPlayers()) {
      if (!player.alive) continue;
      const isLocal = playerId === world.localPlayerId;
      if (isLocal) continue; // Draw local player last

      const px = centerX + player.position.x * scale;
      const py = centerY + player.position.y * scale;

      // Clamp to minimap bounds
      const dist = Math.sqrt(Math.pow(px - centerX, 2) + Math.pow(py - centerY, 2));
      const maxDist = minimapSize / 2 - 4;
      let finalX = px;
      let finalY = py;
      if (dist > maxDist) {
        const ratio = maxDist / dist;
        finalX = centerX + (px - centerX) * ratio;
        finalY = centerY + (py - centerY) * ratio;
      }

      const color = world.getPlayerColor(player.colorIndex);
      this.ctx.fillStyle = color;
      this.ctx.beginPath();
      this.ctx.arc(finalX, finalY, 2, 0, Math.PI * 2);
      this.ctx.fill();
    }

    // Local player as larger dot with glow
    const localPlayer = world.getLocalPlayer();
    if (localPlayer && localPlayer.alive) {
      const lpx = centerX + localPlayer.position.x * scale;
      const lpy = centerY + localPlayer.position.y * scale;

      // Clamp to minimap bounds
      const dist = Math.sqrt(Math.pow(lpx - centerX, 2) + Math.pow(lpy - centerY, 2));
      const maxDist = minimapSize / 2 - 4;
      let finalX = lpx;
      let finalY = lpy;
      if (dist > maxDist) {
        const ratio = maxDist / dist;
        finalX = centerX + (lpx - centerX) * ratio;
        finalY = centerY + (lpy - centerY) * ratio;
      }

      // Glow
      this.ctx.fillStyle = 'rgba(96, 165, 250, 0.4)';
      this.ctx.beginPath();
      this.ctx.arc(finalX, finalY, 5, 0, Math.PI * 2);
      this.ctx.fill();

      // Core
      this.ctx.fillStyle = '#60a5fa';
      this.ctx.beginPath();
      this.ctx.arc(finalX, finalY, 3, 0, Math.PI * 2);
      this.ctx.fill();
    }
  }

  private drawPanel(x: number, y: number, w: number, h: number): void {
    // Background
    this.ctx.fillStyle = 'rgba(15, 23, 42, 0.85)';
    this.ctx.beginPath();
    this.ctx.roundRect(x, y, w, h, 8);
    this.ctx.fill();

    // Border
    this.ctx.strokeStyle = 'rgba(100, 116, 139, 0.3)';
    this.ctx.lineWidth = 1;
    this.ctx.stroke();
  }

  private drawProgressBar(x: number, y: number, w: number, h: number, percent: number, color: string): void {
    // Background
    this.ctx.fillStyle = 'rgba(51, 65, 85, 0.8)';
    this.ctx.beginPath();
    this.ctx.roundRect(x, y, w, h, h / 2);
    this.ctx.fill();

    // Fill
    if (percent > 0) {
      this.ctx.fillStyle = color;
      this.ctx.beginPath();
      this.ctx.roundRect(x, y, Math.max(w * percent, h), h, h / 2);
      this.ctx.fill();
    }
  }

  private renderCountdown(time: number): void {
    const canvas = this.ctx.canvas;
    const count = Math.ceil(time);

    this.ctx.fillStyle = 'rgba(0, 0, 0, 0.5)';
    this.ctx.fillRect(0, 0, canvas.width, canvas.height);

    this.ctx.fillStyle = '#ffffff';
    this.ctx.font = 'bold 120px Inter, system-ui, sans-serif';
    this.ctx.textAlign = 'center';
    this.ctx.textBaseline = 'middle';
    this.ctx.fillText(count > 0 ? count.toString() : 'GO!', canvas.width / 2, canvas.height / 2);
  }

  private renderConnectionStatus(state: RenderState): void {
    const canvas = this.ctx.canvas;
    const padding = 10;

    // Connection indicator
    let statusColor = '#22c55e'; // green

    switch (state.connectionState) {
      case 'connecting':
        statusColor = '#fbbf24';
        break;
      case 'disconnected':
      case 'error':
        statusColor = '#ef4444';
        break;
    }

    // Status dot
    this.ctx.fillStyle = statusColor;
    this.ctx.beginPath();
    this.ctx.arc(canvas.width - padding - 5, padding + 5, 5, 0, Math.PI * 2);
    this.ctx.fill();

    // RTT display
    if (state.connectionState === 'connected') {
      this.ctx.fillStyle = '#606080';
      this.ctx.font = '10px monospace';
      this.ctx.textAlign = 'right';
      this.ctx.fillText(`${Math.round(state.rtt)}ms`, canvas.width - padding - 15, padding + 8);
    }
  }

  private colorWithAlpha(color: string, alpha: number): string {
    const hex = color.replace('#', '');
    const r = parseInt(hex.substring(0, 2), 16);
    const g = parseInt(hex.substring(2, 4), 16);
    const b = parseInt(hex.substring(4, 6), 16);
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }

  private lightenColor(color: string, percent: number): string {
    const hex = color.replace('#', '');
    const r = Math.min(255, parseInt(hex.substring(0, 2), 16) + percent);
    const g = Math.min(255, parseInt(hex.substring(2, 4), 16) + percent);
    const b = Math.min(255, parseInt(hex.substring(4, 6), 16) + percent);
    return `rgb(${r}, ${g}, ${b})`;
  }
}
