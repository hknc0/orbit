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
  input?: InputState;
  rtt: number;
  connectionState: ConnectionState;
}

// Trail point for local player
interface TrailPoint {
  x: number;
  y: number;
  timestamp: number;
}

export class RenderSystem {
  private ctx: CanvasRenderingContext2D;
  private cameraOffset: Vec2 = new Vec2();
  private targetCameraOffset: Vec2 = new Vec2();
  private cameraInitialized: boolean = false;
  private readonly CAMERA_SMOOTHING = 0.1;
  private _densityLogged = false; // Debug flag

  // Dynamic zoom based on speed
  private currentZoom: number = 1.0;
  private targetZoom: number = 1.0;
  private readonly ZOOM_SMOOTHING = 0.05;
  private readonly ZOOM_MIN = 0.45; // Max zoom out at high speed
  private readonly ZOOM_MAX = 1.0;  // Normal zoom at rest
  private readonly SPEED_FOR_MAX_ZOOM_OUT = 250; // Speed at which max zoom out is reached

  // Track previous speeds to detect acceleration (for other players' boost flames)
  private previousSpeeds: Map<string, number> = new Map();

  // Trails for all players
  private playerTrails: Map<string, TrailPoint[]> = new Map();
  private lastTrailPositions: Map<string, { x: number; y: number }> = new Map();
  private readonly TRAIL_MAX_LENGTH = 30;
  private readonly TRAIL_POINT_LIFETIME = 400; // ms
  private readonly TRAIL_MIN_DISTANCE = 8; // minimum distance between trail points

  constructor(ctx: CanvasRenderingContext2D) {
    this.ctx = ctx;
  }

  private updatePlayerTrails(world: World): void {
    const now = Date.now();
    const players = world.getPlayers();

    // Update trails for all alive players
    for (const player of players.values()) {
      if (!player.alive) {
        this.playerTrails.delete(player.id);
        this.lastTrailPositions.delete(player.id);
        continue;
      }

      let trail = this.playerTrails.get(player.id);
      if (!trail) {
        trail = [];
        this.playerTrails.set(player.id, trail);
      }

      // Remove old trail points
      while (trail.length > 0 && now - trail[0].timestamp > this.TRAIL_POINT_LIFETIME) {
        trail.shift();
      }

      // Add new trail point if moved enough distance
      const pos = player.position;
      const lastPos = this.lastTrailPositions.get(player.id);

      if (lastPos) {
        const dx = pos.x - lastPos.x;
        const dy = pos.y - lastPos.y;
        const dist = Math.sqrt(dx * dx + dy * dy);

        if (dist >= this.TRAIL_MIN_DISTANCE) {
          trail.push({ x: pos.x, y: pos.y, timestamp: now });
          this.lastTrailPositions.set(player.id, { x: pos.x, y: pos.y });

          while (trail.length > this.TRAIL_MAX_LENGTH) {
            trail.shift();
          }
        }
      } else {
        this.lastTrailPositions.set(player.id, { x: pos.x, y: pos.y });
      }
    }

    // Lazy cleanup of trails for players who left
    if (this.playerTrails.size > players.size + 5) {
      for (const id of this.playerTrails.keys()) {
        if (!players.has(id)) {
          this.playerTrails.delete(id);
          this.lastTrailPositions.delete(id);
        }
      }
    }
  }

  private renderPlayerTrails(world: World): void {
    const now = Date.now();

    for (const [playerId, trail] of this.playerTrails) {
      if (trail.length < 2) continue;

      const player = world.getPlayer(playerId);
      if (!player || !player.alive) continue;

      const color = world.getPlayerColor(player.colorIndex);
      const radius = world.massToRadius(player.mass);

      for (let i = 0; i < trail.length; i++) {
        const point = trail[i];
        const age = now - point.timestamp;
        const lifeRatio = Math.max(0, 1 - age / this.TRAIL_POINT_LIFETIME);
        const indexRatio = i / trail.length;

        const alpha = lifeRatio * indexRatio * 0.4;
        if (alpha < 0.02) continue;

        const trailRadius = radius * (0.25 + indexRatio * 0.5);

        this.ctx.fillStyle = this.colorWithAlpha(color, alpha);
        this.ctx.beginPath();
        this.ctx.arc(point.x, point.y, Math.max(trailRadius, 2), 0, Math.PI * 2);
        this.ctx.fill();
      }
    }
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

      // Snap camera on first frame (avoid jump from origin)
      if (!this.cameraInitialized) {
        this.cameraOffset.copy(this.targetCameraOffset);
        this.cameraInitialized = true;
      }

      // Calculate dynamic zoom based on speed
      const speed = localPlayer.velocity.length();
      const speedRatio = Math.min(speed / this.SPEED_FOR_MAX_ZOOM_OUT, 1);
      this.targetZoom = this.ZOOM_MAX - (this.ZOOM_MAX - this.ZOOM_MIN) * speedRatio;
    } else {
      this.targetCameraOffset.set(centerX, centerY);
      this.targetZoom = this.ZOOM_MAX;
      this.cameraInitialized = false; // Reset so next spawn snaps camera
    }

    // Smooth camera interpolation
    this.cameraOffset.x +=
      (this.targetCameraOffset.x - this.cameraOffset.x) * this.CAMERA_SMOOTHING;
    this.cameraOffset.y +=
      (this.targetCameraOffset.y - this.cameraOffset.y) * this.CAMERA_SMOOTHING;

    // Smooth zoom interpolation
    this.currentZoom += (this.targetZoom - this.currentZoom) * this.ZOOM_SMOOTHING;

    // Update trails for all players
    this.updatePlayerTrails(world);

    this.ctx.save();
    // Apply zoom centered on screen
    this.ctx.translate(centerX, centerY);
    this.ctx.scale(this.currentZoom, this.currentZoom);
    this.ctx.translate(-centerX, -centerY);
    // Then apply camera offset
    this.ctx.translate(this.cameraOffset.x, this.cameraOffset.y);

    // Reset any lingering canvas state that might cause visual artifacts
    this.ctx.setLineDash([]);
    this.ctx.globalAlpha = 1.0;

    // Render in order (back to front)
    this.renderArena(world);
    this.renderDeathEffects(world);
    this.renderPlayerTrails(world);
    this.renderProjectiles(world);
    this.renderPlayers(world, state.input?.isBoosting ?? false);

    // Render aim indicator
    if (state.input && state.phase === 'playing') {
      this.renderAimIndicator(world, state.input);
    }

    this.ctx.restore();

    // Render UI overlay
    this.renderHUD(world, state);

    // Render charge indicator
    if (state.input?.isCharging && state.phase === 'playing') {
      this.renderChargeIndicator(state.input.chargeRatio);
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
    this.ctx.fillText('CHARGING', Math.round(canvas.width / 2), Math.round(y - 5));
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
    const players = world.getPlayers();

    // Clean up stale speed tracking (players who left) - runs every ~60 frames
    if (this.previousSpeeds.size > players.size + 10) {
      for (const id of this.previousSpeeds.keys()) {
        if (!players.has(id)) this.previousSpeeds.delete(id);
      }
    }

    for (const player of players.values()) {
      if (!player.alive) continue;

      const radius = world.massToRadius(player.mass);
      const color = world.getPlayerColor(player.colorIndex);
      const isLocal = player.id === world.localPlayerId;

      // Render boost flame based on velocity/acceleration
      const speed = player.velocity.length();
      let showFlame = false;

      if (isLocal) {
        // Local player: show when boosting input is active
        showFlame = localPlayerBoosting;
      } else {
        // Other players: detect acceleration (speed increasing) to infer boosting
        const prevSpeed = this.previousSpeeds.get(player.id) ?? 0;
        const isAccelerating = speed > prevSpeed + 2; // Speed increasing by at least 2
        const hasHighSpeed = speed > 80; // Also show if very fast and recently accelerated
        showFlame = isAccelerating || (hasHighSpeed && speed > prevSpeed);
      }

      // Track speed for next frame
      this.previousSpeeds.set(player.id, speed);

      if (showFlame) {
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
      const { position, color } = effect;
      // Guard against negative/zero progress
      const progress = Math.max(0.001, effect.progress);
      if (progress <= 0) continue;

      // Multiple expanding rings
      const numRings = 3;
      for (let i = 0; i < numRings; i++) {
        const ringProgress = Math.max(0, progress - i * 0.15);
        if (ringProgress <= 0) continue;

        const ringRadius = Math.max(1, 10 + (1 - ringProgress) * (60 + i * 20));
        const alpha = ringProgress * 0.7;

        this.ctx.strokeStyle = this.colorWithAlpha(color, alpha);
        this.ctx.lineWidth = Math.max(0.5, 4 * ringProgress);
        this.ctx.beginPath();
        this.ctx.arc(position.x, position.y, ringRadius, 0, Math.PI * 2);
        this.ctx.stroke();
      }

      // Central flash
      if (progress > 0.5) {
        const flashProgress = (progress - 0.5) * 2;
        const flashRadius = Math.max(1, 30 * flashProgress);

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
        const particleSize = Math.max(0.5, 3 * progress);

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

    const ctx = this.ctx;
    const time = performance.now();

    // Flame direction is opposite to velocity
    const dirX = -velocity.x / speed;
    const dirY = -velocity.y / speed;

    const flameX = position.x + dirX * radius;
    const flameY = position.y + dirY * radius;

    // Dynamic flame length based on speed with flicker
    const baseLen = Math.min(radius * 2, 20 + speed * 0.08);
    const flicker = 0.85 + Math.sin(time * 0.03) * 0.1 + Math.sin(time * 0.07) * 0.05;
    const flameLen = baseLen * flicker;
    const flameWidth = radius * 0.5;

    const perpX = -dirY;
    const perpY = dirX;

    // Outer glow (soft orange, no shadowBlur for performance)
    const glowAlpha = 0.25 + Math.sin(time * 0.02) * 0.1;
    ctx.fillStyle = `rgba(255, 100, 30, ${glowAlpha})`;
    ctx.beginPath();
    ctx.moveTo(flameX + perpX * flameWidth * 1.4, flameY + perpY * flameWidth * 1.4);
    ctx.lineTo(flameX - perpX * flameWidth * 1.4, flameY - perpY * flameWidth * 1.4);
    ctx.lineTo(flameX + dirX * flameLen * 1.1, flameY + dirY * flameLen * 1.1);
    ctx.closePath();
    ctx.fill();

    // Main outer flame (orange-red)
    ctx.fillStyle = 'rgba(255, 120, 40, 0.9)';
    ctx.beginPath();
    ctx.moveTo(flameX + perpX * flameWidth, flameY + perpY * flameWidth);
    ctx.lineTo(flameX - perpX * flameWidth, flameY - perpY * flameWidth);
    ctx.lineTo(flameX + dirX * flameLen, flameY + dirY * flameLen);
    ctx.closePath();
    ctx.fill();

    // Middle flame (orange-yellow)
    const midFlicker = 0.9 + Math.sin(time * 0.05 + 1) * 0.1;
    ctx.fillStyle = 'rgba(255, 180, 60, 0.95)';
    ctx.beginPath();
    ctx.moveTo(flameX + perpX * flameWidth * 0.65, flameY + perpY * flameWidth * 0.65);
    ctx.lineTo(flameX - perpX * flameWidth * 0.65, flameY - perpY * flameWidth * 0.65);
    ctx.lineTo(flameX + dirX * flameLen * 0.75 * midFlicker, flameY + dirY * flameLen * 0.75 * midFlicker);
    ctx.closePath();
    ctx.fill();

    // Inner core (bright yellow-white)
    const coreFlicker = 0.85 + Math.sin(time * 0.08 + 2) * 0.15;
    ctx.fillStyle = 'rgba(255, 240, 180, 1)';
    ctx.beginPath();
    ctx.moveTo(flameX + perpX * flameWidth * 0.3, flameY + perpY * flameWidth * 0.3);
    ctx.lineTo(flameX - perpX * flameWidth * 0.3, flameY - perpY * flameWidth * 0.3);
    ctx.lineTo(flameX + dirX * flameLen * 0.45 * coreFlicker, flameY + dirY * flameLen * 0.45 * coreFlicker);
    ctx.closePath();
    ctx.fill();

    // Sparks at higher speeds (simple dots, performant)
    if (speed > 80) {
      const sparkCount = Math.min(3, Math.floor((speed - 80) / 40) + 1);
      for (let i = 0; i < sparkCount; i++) {
        const sparkPhase = (time * 0.01 + i * 2.1) % 1;
        const sparkDist = flameLen * (0.6 + sparkPhase * 0.5);
        const sparkOffset = Math.sin(time * 0.02 + i * 3) * flameWidth * 0.3;
        const sparkX = flameX + dirX * sparkDist + perpX * sparkOffset;
        const sparkY = flameY + dirY * sparkDist + perpY * sparkOffset;
        const sparkAlpha = 1 - sparkPhase;
        const sparkSize = 2 * (1 - sparkPhase * 0.5);

        ctx.fillStyle = `rgba(255, 220, 150, ${sparkAlpha})`;
        ctx.beginPath();
        ctx.arc(sparkX, sparkY, sparkSize, 0, Math.PI * 2);
        ctx.fill();
      }
    }
  }

  private renderHUD(world: World, _state: RenderState): void {
    const canvas = this.ctx.canvas;
    const padding = 16;
    const localPlayer = world.getLocalPlayer();
    const sessionStats = world.getSessionStats();

    // Reset text baseline for consistent rendering (countdown sets it to 'middle')
    this.ctx.textBaseline = 'alphabetic';

    // === LEFT PANEL - Player Stats ===
    if (localPlayer) {
      const panelX = padding;
      const panelY = padding;
      const panelW = 190;
      const panelH = 160;

      // Enhanced panel with gradient and corner accents
      this.drawPanelEnhanced(panelX, panelY, panelW, panelH, { cornerAccents: true });

      // Rank badge with glow for top 3
      const aliveCount = world.getAlivePlayerCount();
      if (localPlayer.alive) {
        const placement = world.getPlayerPlacement(world.localPlayerId!);
        const rankBadgeColors = ['#FFD700', '#C0C0C0', '#CD7F32']; // Gold, Silver, Bronze
        const rankColor = placement <= 3 ? rankBadgeColors[placement - 1] : '#94a3b8';

        this.ctx.font = 'bold 26px Inter, system-ui, sans-serif';
        this.ctx.textAlign = 'left';
        const rankText = `#${placement}`;

        // Glow effect for top 3
        if (placement <= 3) {
          this.drawGlowText(rankText, Math.round(panelX + 14), Math.round(panelY + 32), rankColor, 1.2);
        } else {
          this.ctx.fillStyle = rankColor;
          this.ctx.fillText(rankText, Math.round(panelX + 14), Math.round(panelY + 32));
        }

        const rankWidth = this.ctx.measureText(rankText).width;
        this.ctx.fillStyle = '#64748b';
        this.ctx.font = '11px Inter, system-ui, sans-serif';
        this.ctx.fillText(`of ${aliveCount}`, Math.round(panelX + 18 + rankWidth), Math.round(panelY + 32));
      } else {
        // Dead state with pulsing effect
        const pulse = Math.sin(Date.now() / 200) * 0.2 + 0.8;
        this.ctx.font = 'bold 20px Inter, system-ui, sans-serif';
        this.ctx.textAlign = 'left';
        this.drawGlowText('DEAD', Math.round(panelX + 14), Math.round(panelY + 32), `rgba(239, 68, 68, ${pulse})`, 1);
        const deadWidth = this.ctx.measureText('DEAD').width;
        this.ctx.fillStyle = '#64748b';
        this.ctx.font = '11px Inter, system-ui, sans-serif';
        this.ctx.fillText(`${aliveCount} alive`, Math.round(panelX + 18 + deadWidth), Math.round(panelY + 32));
      }

      // Mass with enhanced bar
      const massPercent = Math.min(localPlayer.mass / 500, 1);
      const barStartX = Math.round(panelX + 58);
      const barWidth = panelW - 72;
      this.ctx.fillStyle = '#64748b';
      this.ctx.font = '9px Inter, system-ui, sans-serif';
      this.ctx.textAlign = 'left';
      this.ctx.fillText('MASS', Math.round(panelX + 14), Math.round(panelY + 52));
      this.ctx.fillStyle = '#00d4ff';
      this.ctx.font = 'bold 15px monospace';
      this.ctx.fillText(Math.floor(localPlayer.mass).toString(), Math.round(panelX + 14), Math.round(panelY + 70));
      this.drawProgressBarEnhanced(barStartX, Math.round(panelY + 58), barWidth, 10, massPercent, '#3b82f6', massPercent > 0.5);

      // Speed with enhanced bar
      const speed = localPlayer.velocity.length();
      const speedPercent = Math.min(speed / 300, 1);
      this.ctx.fillStyle = '#64748b';
      this.ctx.font = '9px Inter, system-ui, sans-serif';
      this.ctx.fillText('SPEED', Math.round(panelX + 14), Math.round(panelY + 88));
      this.ctx.fillStyle = '#4ade80';
      this.ctx.font = 'bold 15px monospace';
      this.ctx.fillText(Math.floor(speed).toString(), Math.round(panelX + 14), Math.round(panelY + 106));
      this.drawProgressBarEnhanced(barStartX, Math.round(panelY + 94), barWidth, 10, speedPercent, '#22c55e', speedPercent > 0.7);

      // K/D Stats in styled boxes
      const kdY = Math.round(panelY + 125);
      // Kills box
      this.ctx.fillStyle = 'rgba(34, 197, 94, 0.15)';
      this.ctx.beginPath();
      this.ctx.roundRect(Math.round(panelX + 14), kdY, 50, 28, 4);
      this.ctx.fill();
      this.ctx.fillStyle = '#64748b';
      this.ctx.font = '8px Inter, system-ui, sans-serif';
      this.ctx.textAlign = 'center';
      this.ctx.fillText('KILLS', Math.round(panelX + 39), kdY + 10);
      this.ctx.fillStyle = '#4ade80';
      this.ctx.font = 'bold 14px monospace';
      this.ctx.fillText(localPlayer.kills.toString(), Math.round(panelX + 39), kdY + 24);

      // Deaths box
      this.ctx.fillStyle = 'rgba(239, 68, 68, 0.15)';
      this.ctx.beginPath();
      this.ctx.roundRect(Math.round(panelX + 70), kdY, 50, 28, 4);
      this.ctx.fill();
      this.ctx.fillStyle = '#64748b';
      this.ctx.font = '8px Inter, system-ui, sans-serif';
      this.ctx.fillText('DEATHS', Math.round(panelX + 95), kdY + 10);
      this.ctx.fillStyle = '#ef4444';
      this.ctx.font = 'bold 14px monospace';
      this.ctx.fillText(localPlayer.deaths.toString(), Math.round(panelX + 95), kdY + 24);

      // Kill streak with fire glow
      if (sessionStats.killStreak > 0) {
        const streakX = Math.round(panelX + 130);
        this.ctx.fillStyle = 'rgba(251, 191, 36, 0.2)';
        this.ctx.beginPath();
        this.ctx.roundRect(streakX, kdY, 46, 28, 4);
        this.ctx.fill();
        this.ctx.font = 'bold 16px Inter, system-ui, sans-serif';
        this.ctx.textAlign = 'center';
        const pulse = Math.sin(Date.now() / 150) * 0.3 + 1;
        this.drawGlowText(`${sessionStats.killStreak}`, streakX + 23, kdY + 20, '#fbbf24', pulse);
      }

      // Time alive (compact, top right of panel)
      const timeAlive = Math.floor(sessionStats.timeAlive / 1000);
      const minutes = Math.floor(timeAlive / 60);
      const seconds = timeAlive % 60;
      this.ctx.fillStyle = '#475569';
      this.ctx.font = '10px monospace';
      this.ctx.textAlign = 'right';
      this.ctx.fillText(`${minutes}:${seconds.toString().padStart(2, '0')}`, Math.round(panelX + panelW - 14), Math.round(panelY + 18));

    }

    // === BOTTOM LEFT - Session Stats (Compact) ===
    if (localPlayer) {
      const panelX = padding;
      const panelY = Math.round(canvas.height - padding - 50);
      const panelW = 200;
      const panelH = 50;

      this.drawPanelEnhanced(panelX, panelY, panelW, panelH);

      // Title
      this.ctx.fillStyle = '#475569';
      this.ctx.font = '8px Inter, system-ui, sans-serif';
      this.ctx.textAlign = 'left';
      this.ctx.fillText('SESSION BEST', Math.round(panelX + 12), Math.round(panelY + 14));

      // Compact stats in a row
      const statsY = Math.round(panelY + 34);
      const statSpacing = 60;

      // Best Mass (cyan)
      this.ctx.fillStyle = '#00d4ff';
      this.ctx.font = 'bold 14px monospace';
      this.ctx.textAlign = 'center';
      this.ctx.fillText(Math.floor(sessionStats.bestMass).toString(), Math.round(panelX + 30), statsY);
      this.ctx.fillStyle = '#475569';
      this.ctx.font = '8px Inter, system-ui, sans-serif';
      this.ctx.fillText('MASS', Math.round(panelX + 30), statsY + 10);

      // Best Streak (orange)
      this.ctx.fillStyle = '#fbbf24';
      this.ctx.font = 'bold 14px monospace';
      this.ctx.fillText(sessionStats.bestKillStreak.toString(), Math.round(panelX + 30 + statSpacing), statsY);
      this.ctx.fillStyle = '#475569';
      this.ctx.font = '8px Inter, system-ui, sans-serif';
      this.ctx.fillText('STREAK', Math.round(panelX + 30 + statSpacing), statsY + 10);

      // Best Time (green)
      const bestSeconds = Math.floor(sessionStats.bestTimeAlive / 1000);
      const bestMin = Math.floor(bestSeconds / 60);
      const bestSec = bestSeconds % 60;
      this.ctx.fillStyle = '#4ade80';
      this.ctx.font = 'bold 14px monospace';
      this.ctx.fillText(`${bestMin}:${bestSec.toString().padStart(2, '0')}`, Math.round(panelX + 30 + statSpacing * 2), statsY);
      this.ctx.fillStyle = '#475569';
      this.ctx.font = '8px Inter, system-ui, sans-serif';
      this.ctx.fillText('TIME', Math.round(panelX + 30 + statSpacing * 2), statsY + 10);
    }

    // === RIGHT PANEL - Leaderboard ===
    const leaderboard = world.getLeaderboard().slice(0, 5);
    const lbPanelW = 185;
    const lbRowHeight = 26;
    const lbPanelH = 34 + leaderboard.length * lbRowHeight;
    const lbPanelX = canvas.width - padding - lbPanelW;
    const lbPanelY = padding;

    this.drawPanelEnhanced(lbPanelX, lbPanelY, lbPanelW, lbPanelH, { cornerAccents: true });

    this.ctx.fillStyle = '#64748b';
    this.ctx.font = 'bold 9px Inter, system-ui, sans-serif';
    this.ctx.textAlign = 'left';
    this.ctx.fillText('LEADERBOARD', Math.round(lbPanelX + 14), Math.round(lbPanelY + 16));

    const rankColors = ['#FFD700', '#C0C0C0', '#CD7F32']; // Gold, Silver, Bronze
    const maxMass = leaderboard.length > 0 ? leaderboard[0].mass : 100;

    leaderboard.forEach((entry, index) => {
      const isLocal = entry.id === world.localPlayerId;
      const y = Math.round(lbPanelY + 36 + index * lbRowHeight);

      // Highlight row for local player
      if (isLocal) {
        this.ctx.fillStyle = 'rgba(0, 255, 255, 0.08)';
        this.ctx.beginPath();
        this.ctx.roundRect(lbPanelX + 6, y - 10, lbPanelW - 12, lbRowHeight - 2, 4);
        this.ctx.fill();
        // Cyan left border indicator
        this.ctx.fillStyle = '#00ffff';
        this.ctx.fillRect(lbPanelX + 6, y - 10, 2, lbRowHeight - 2);
      }

      // Medal/rank with glow for top 3
      const rankColor = index < 3 ? rankColors[index] : '#475569';
      this.ctx.font = 'bold 13px Inter, system-ui, sans-serif';
      this.ctx.textAlign = 'left';

      if (index < 3) {
        // Glowing medal number
        this.drawGlowText(`${index + 1}`, Math.round(lbPanelX + 16), y + 4, rankColor, 0.8);
      } else {
        this.ctx.fillStyle = rankColor;
        this.ctx.fillText(`${index + 1}`, Math.round(lbPanelX + 16), y + 4);
      }

      // Name - cyan for local, white for others
      this.ctx.fillStyle = isLocal ? '#00ffff' : '#e2e8f0';
      this.ctx.font = `${isLocal ? 'bold ' : ''}11px Inter, system-ui, sans-serif`;
      const name = entry.name.length > 9 ? entry.name.slice(0, 9) + '…' : entry.name;
      this.ctx.fillText(name, Math.round(lbPanelX + 32), y + 4);

      // Mini mass bar
      const barX = lbPanelX + 100;
      const barW = 45;
      const barH = 6;
      const massPercent = entry.mass / maxMass;
      this.ctx.fillStyle = 'rgba(30, 41, 59, 0.6)';
      this.ctx.beginPath();
      this.ctx.roundRect(barX, y - 1, barW, barH, 2);
      this.ctx.fill();
      if (massPercent > 0) {
        const barColor = isLocal ? '#00ffff' : (index < 3 ? rankColor : '#3b82f6');
        this.ctx.fillStyle = barColor;
        this.ctx.beginPath();
        this.ctx.roundRect(barX, y - 1, Math.max(barW * massPercent, 4), barH, 2);
        this.ctx.fill();
      }

      // Mass number
      this.ctx.fillStyle = isLocal ? '#00ffff' : '#94a3b8';
      this.ctx.font = '10px monospace';
      this.ctx.textAlign = 'right';
      this.ctx.fillText(Math.floor(entry.mass).toString(), Math.round(lbPanelX + lbPanelW - 12), y + 4);
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
        this.ctx.fillText(`⚠ ${dangerType}`, Math.round(canvas.width / 2), Math.round(canvas.height - 60));
      }
    }

    // === CONTROLS HINT (Pill background) ===
    const controlsText = 'WASD/Arrows: Move  •  LMB/Shift: Boost  •  SPACE: Eject';
    this.ctx.font = '10px Inter, system-ui, sans-serif';
    const textWidth = this.ctx.measureText(controlsText).width;
    const pillW = textWidth + 24;
    const pillH = 22;
    const pillX = Math.round((canvas.width - pillW) / 2);
    const pillY = Math.round(canvas.height - padding - pillH - 2);

    // Pill background
    this.ctx.fillStyle = 'rgba(15, 23, 42, 0.75)';
    this.ctx.beginPath();
    this.ctx.roundRect(pillX, pillY, pillW, pillH, pillH / 2);
    this.ctx.fill();
    this.ctx.strokeStyle = 'rgba(100, 150, 255, 0.15)';
    this.ctx.lineWidth = 1;
    this.ctx.stroke();

    // Text
    this.ctx.fillStyle = '#64748b';
    this.ctx.textAlign = 'center';
    this.ctx.fillText(controlsText, Math.round(canvas.width / 2), pillY + 15);

    // === EDGE INDICATORS FOR NEAREST GRAVITY WELLS ===
    if (localPlayer && localPlayer.alive && world.arena.gravityWells.length > 0) {
      const wells = world.arena.gravityWells;
      const edgeMargin = 40;
      const indicatorSize = 8;

      // Calculate distances, filter to off-screen wells, then take top 3
      const screenMargin = 60;
      const wellData = wells.map((well) => {
        const dx = well.position.x - localPlayer.position.x;
        const dy = well.position.y - localPlayer.position.y;
        const dist = Math.sqrt(dx * dx + dy * dy);
        const angle = Math.atan2(dy, dx);
        // Check if well is visible on screen
        const wellScreenX = canvas.width / 2 + dx * this.currentZoom;
        const wellScreenY = canvas.height / 2 + dy * this.currentZoom;
        const isOnScreen = wellScreenX > screenMargin && wellScreenX < canvas.width - screenMargin &&
                           wellScreenY > screenMargin && wellScreenY < canvas.height - screenMargin;
        return { well, dist, angle, isOnScreen };
      })
      .filter(w => !w.isOnScreen) // Only off-screen wells
      .sort((a, b) => a.dist - b.dist)
      .slice(0, 3); // Top 3 nearest off-screen wells

      wellData.forEach((w) => {
        // Color based on distance
        let color = '#4ade80'; // green - close
        let alpha = 0.9;
        if (w.dist > 500) { color = '#fbbf24'; alpha = 0.8; } // yellow
        if (w.dist > 1000) { color = '#ef4444'; alpha = 0.6; } // red - far

        // Position indicator at screen edge in the direction of the well
        const screenCenterX = canvas.width / 2;
        const screenCenterY = canvas.height / 2;

        // Calculate where the line from center in direction of well intersects screen edge
        const cos = Math.cos(w.angle);
        const sin = Math.sin(w.angle);

        // Find intersection with screen edges
        const maxX = (canvas.width / 2) - edgeMargin;
        const maxY = (canvas.height / 2) - edgeMargin;

        let t = Infinity;
        if (cos !== 0) t = Math.min(t, Math.abs(maxX / cos));
        if (sin !== 0) t = Math.min(t, Math.abs(maxY / sin));

        const indicatorX = screenCenterX + cos * t;
        const indicatorY = screenCenterY + sin * t;

        // Draw arrow indicator
        this.ctx.save();
        this.ctx.globalAlpha = alpha;
        this.ctx.translate(indicatorX, indicatorY);
        this.ctx.rotate(w.angle);

        // Arrow shape
        this.ctx.fillStyle = color;
        this.ctx.beginPath();
        this.ctx.moveTo(indicatorSize, 0);
        this.ctx.lineTo(-indicatorSize * 0.6, -indicatorSize * 0.6);
        this.ctx.lineTo(-indicatorSize * 0.3, 0);
        this.ctx.lineTo(-indicatorSize * 0.6, indicatorSize * 0.6);
        this.ctx.closePath();
        this.ctx.fill();

        this.ctx.restore();

        // Distance label near indicator
        this.ctx.fillStyle = color;
        this.ctx.globalAlpha = alpha;
        this.ctx.font = 'bold 9px monospace';
        this.ctx.textAlign = 'center';
        const labelOffset = 14;
        const labelX = indicatorX - Math.cos(w.angle) * labelOffset;
        const labelY = indicatorY - Math.sin(w.angle) * labelOffset + 3;
        this.ctx.fillText(`${Math.floor(w.dist)}`, labelX, labelY);
        this.ctx.globalAlpha = 1;
      });
    }

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

    // 1. Density heatmap (16x16 grid showing player concentrations)
    const densityGrid = world.getDensityGrid();
    // Debug: log density grid once
    if (densityGrid.length > 0 && !this._densityLogged) {
      console.log('Density grid:', densityGrid.length, 'cells, sum:', densityGrid.reduce((a, b) => a + b, 0));
      this._densityLogged = true;
    }
    // Support both 8x8 (64) and 16x16 (256) grids
    const gridLength = densityGrid.length;
    if (gridLength === 64 || gridLength === 256) {
      const GRID_SIZE = Math.sqrt(gridLength);
      const gridPixelSize = minimapSize - 8;
      const cellPixelSize = gridPixelSize / GRID_SIZE;

      // Find max density for normalization (use percentile to avoid single hotspots dominating)
      const sortedDensities = densityGrid.filter(d => d > 0).sort((a, b) => b - a);
      const maxDensity = sortedDensities.length > 0
        ? Math.max(sortedDensities[Math.floor(sortedDensities.length * 0.1)] || sortedDensities[0], 1)
        : 1;

      // Save context for clipping
      this.ctx.save();
      this.ctx.beginPath();
      this.ctx.arc(centerX, centerY, minimapSize / 2 - 2, 0, Math.PI * 2);
      this.ctx.clip();

      // Render with radial gradients for smoother appearance
      for (let gy = 0; gy < GRID_SIZE; gy++) {
        for (let gx = 0; gx < GRID_SIZE; gx++) {
          const idx = gy * GRID_SIZE + gx;
          const density = densityGrid[idx];

          if (density > 0) {
            // Cell center position on minimap
            const cellCenterX = centerX - gridPixelSize / 2 + (gx + 0.5) * cellPixelSize;
            const cellCenterY = centerY - gridPixelSize / 2 + (gy + 0.5) * cellPixelSize;

            // Intensity with log scale for better distribution
            const rawIntensity = Math.min(density / maxDensity, 1);
            const intensity = Math.pow(rawIntensity, 0.6); // Gamma for visibility

            // Hot color scheme: low=blue, mid=cyan, high=yellow/orange
            let r: number, g: number, b: number;
            if (intensity < 0.5) {
              // Blue to cyan
              const t = intensity * 2;
              r = Math.floor(30 * t);
              g = Math.floor(100 + 155 * t);
              b = Math.floor(200 - 50 * t);
            } else {
              // Cyan to orange/yellow
              const t = (intensity - 0.5) * 2;
              r = Math.floor(30 + 225 * t);
              g = Math.floor(255 - 55 * t);
              b = Math.floor(150 - 130 * t);
            }

            // Radial gradient for soft blob effect
            const blobRadius = cellPixelSize * 1.2;
            const gradient = this.ctx.createRadialGradient(
              cellCenterX, cellCenterY, 0,
              cellCenterX, cellCenterY, blobRadius
            );

            const alpha = 0.15 + intensity * 0.4;
            gradient.addColorStop(0, `rgba(${r}, ${g}, ${b}, ${alpha})`);
            gradient.addColorStop(0.6, `rgba(${r}, ${g}, ${b}, ${alpha * 0.5})`);
            gradient.addColorStop(1, `rgba(${r}, ${g}, ${b}, 0)`);

            this.ctx.fillStyle = gradient;
            this.ctx.beginPath();
            this.ctx.arc(cellCenterX, cellCenterY, blobRadius, 0, Math.PI * 2);
            this.ctx.fill();
          }
        }
      }

      this.ctx.restore();
    }

    // 1b. Gravity wells - tiny orange dots (sun color)
    for (const well of world.arena.gravityWells) {
      const wellX = centerX + well.position.x * scale;
      const wellY = centerY + well.position.y * scale;

      // Only draw if within minimap bounds
      const dist = Math.sqrt(Math.pow(wellX - centerX, 2) + Math.pow(wellY - centerY, 2));
      if (dist < minimapSize / 2 - 2) {
        this.ctx.fillStyle = '#ff9944';
        this.ctx.beginPath();
        this.ctx.arc(wellX, wellY, 1.5, 0, Math.PI * 2);
        this.ctx.fill();
      }
    }

    // Helper to clamp position to minimap bounds
    const clampToMinimap = (x: number, y: number) => {
      const dist = Math.sqrt(Math.pow(x - centerX, 2) + Math.pow(y - centerY, 2));
      const maxDist = minimapSize / 2 - 4;
      if (dist > maxDist) {
        const ratio = maxDist / dist;
        return {
          x: centerX + (x - centerX) * ratio,
          y: centerY + (y - centerY) * ratio,
        };
      }
      return { x, y };
    };

    // 2. Notable players (high mass) - larger pulsing indicators visible from anywhere
    const notablePlayers = world.getNotablePlayers();
    const visiblePlayerIds = new Set(world.getPlayers().keys());

    for (const notable of notablePlayers) {
      // Skip if already visible as regular player or is local player
      if (visiblePlayerIds.has(notable.id) || notable.id === world.localPlayerId) continue;

      const pos = clampToMinimap(
        centerX + notable.position.x * scale,
        centerY + notable.position.y * scale
      );

      // Pulsing size based on mass
      const massRatio = Math.min(notable.mass / 200, 1);
      const baseSize = 3 + massRatio * 3; // 3-6px based on mass
      const pulse = 1 + 0.2 * Math.sin(Date.now() / 400);

      const color = world.getPlayerColor(notable.colorIndex);

      // Outer glow ring
      this.ctx.strokeStyle = color;
      this.ctx.lineWidth = 1.5;
      this.ctx.globalAlpha = 0.4;
      this.ctx.beginPath();
      this.ctx.arc(pos.x, pos.y, (baseSize + 3) * pulse, 0, Math.PI * 2);
      this.ctx.stroke();
      this.ctx.globalAlpha = 1;

      // Dark outline
      this.ctx.fillStyle = 'rgba(0, 0, 0, 0.6)';
      this.ctx.beginPath();
      this.ctx.arc(pos.x, pos.y, baseSize + 1, 0, Math.PI * 2);
      this.ctx.fill();

      // Colored fill
      this.ctx.fillStyle = color;
      this.ctx.beginPath();
      this.ctx.arc(pos.x, pos.y, baseSize, 0, Math.PI * 2);
      this.ctx.fill();
    }

    // 3. Other players (nearby/visible) - small dots
    for (const [playerId, player] of world.getPlayers()) {
      if (!player.alive) continue;
      if (playerId === world.localPlayerId) continue;

      const pos = clampToMinimap(
        centerX + player.position.x * scale,
        centerY + player.position.y * scale
      );

      // Colored dot with outline for visibility over heatmap
      const color = world.getPlayerColor(player.colorIndex);
      // Dark outline
      this.ctx.fillStyle = 'rgba(0, 0, 0, 0.5)';
      this.ctx.beginPath();
      this.ctx.arc(pos.x, pos.y, 3.5, 0, Math.PI * 2);
      this.ctx.fill();
      // Colored fill
      this.ctx.fillStyle = color;
      this.ctx.beginPath();
      this.ctx.arc(pos.x, pos.y, 2.5, 0, Math.PI * 2);
      this.ctx.fill();
    }

    // 2. Local player - VERY prominent, always on top (show even when dead)
    const localPlayer = world.getLocalPlayer();
    if (localPlayer) {
      const pos = clampToMinimap(
        centerX + localPlayer.position.x * scale,
        centerY + localPlayer.position.y * scale
      );

      if (localPlayer.alive) {
        // Alive: Bright LIME GREEN indicator (contrasts with orange heatmap)
        // Pulsing effect for extra visibility
        const pulse = 0.7 + 0.3 * Math.sin(Date.now() / 200);
        const pulseSize = 1 + 0.15 * Math.sin(Date.now() / 300);

        // Outer pulsing glow - lime green
        this.ctx.fillStyle = `rgba(0, 255, 100, ${0.15 * pulse})`;
        this.ctx.beginPath();
        this.ctx.arc(pos.x, pos.y, 16 * pulseSize, 0, Math.PI * 2);
        this.ctx.fill();

        // Strong black outline for contrast
        this.ctx.strokeStyle = '#000000';
        this.ctx.lineWidth = 4;
        this.ctx.beginPath();
        this.ctx.arc(pos.x, pos.y, 8, 0, Math.PI * 2);
        this.ctx.stroke();

        // Bright white ring
        this.ctx.strokeStyle = '#ffffff';
        this.ctx.lineWidth = 2;
        this.ctx.beginPath();
        this.ctx.arc(pos.x, pos.y, 8, 0, Math.PI * 2);
        this.ctx.stroke();

        // Bright lime green fill
        this.ctx.fillStyle = '#00ff64';
        this.ctx.beginPath();
        this.ctx.arc(pos.x, pos.y, 6, 0, Math.PI * 2);
        this.ctx.fill();

        // Direction indicator (small triangle pointing in aim direction)
        const rotation = localPlayer.rotation;
        const arrowDist = 11;
        const arrowX = pos.x + Math.cos(rotation) * arrowDist;
        const arrowY = pos.y + Math.sin(rotation) * arrowDist;
        this.ctx.fillStyle = '#ffffff';
        this.ctx.beginPath();
        this.ctx.moveTo(
          arrowX + Math.cos(rotation) * 4,
          arrowY + Math.sin(rotation) * 4
        );
        this.ctx.lineTo(
          arrowX + Math.cos(rotation + 2.5) * 3,
          arrowY + Math.sin(rotation + 2.5) * 3
        );
        this.ctx.lineTo(
          arrowX + Math.cos(rotation - 2.5) * 3,
          arrowY + Math.sin(rotation - 2.5) * 3
        );
        this.ctx.closePath();
        this.ctx.fill();
      } else {
        // Dead: Dimmed red X indicator
        this.ctx.globalAlpha = 0.6;

        // Dark red glow
        this.ctx.fillStyle = 'rgba(239, 68, 68, 0.2)';
        this.ctx.beginPath();
        this.ctx.arc(pos.x, pos.y, 12, 0, Math.PI * 2);
        this.ctx.fill();

        // Red X mark
        this.ctx.strokeStyle = '#ef4444';
        this.ctx.lineWidth = 2;
        this.ctx.beginPath();
        this.ctx.moveTo(pos.x - 5, pos.y - 5);
        this.ctx.lineTo(pos.x + 5, pos.y + 5);
        this.ctx.moveTo(pos.x + 5, pos.y - 5);
        this.ctx.lineTo(pos.x - 5, pos.y + 5);
        this.ctx.stroke();

        this.ctx.globalAlpha = 1.0;
      }
    }

    // Compact label above minimap
    const aliveCount = world.getAlivePlayerCount();
    this.ctx.font = '10px Inter, system-ui, sans-serif';
    this.ctx.textAlign = 'center';
    this.ctx.fillStyle = 'rgba(148, 163, 184, 0.7)';
    this.ctx.fillText(`${aliveCount} alive`, Math.round(centerX), Math.round(minimapY - 6));
  }

  // Enhanced panel with gradient background and optional corner accents
  private drawPanelEnhanced(
    x: number,
    y: number,
    w: number,
    h: number,
    options?: { cornerAccents?: boolean; glowColor?: string }
  ): void {
    const ctx = this.ctx;

    // Create gradient background
    const gradient = ctx.createLinearGradient(x, y, x, y + h);
    gradient.addColorStop(0, 'rgba(15, 23, 42, 0.92)');
    gradient.addColorStop(1, 'rgba(10, 15, 30, 0.95)');

    ctx.fillStyle = gradient;
    ctx.beginPath();
    ctx.roundRect(x, y, w, h, 8);
    ctx.fill();

    // Border with subtle glow
    ctx.strokeStyle = options?.glowColor
      ? this.colorWithAlpha(options.glowColor, 0.3)
      : 'rgba(100, 150, 255, 0.2)';
    ctx.lineWidth = 1;
    ctx.stroke();

    // Inner highlight line at top
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.05)';
    ctx.beginPath();
    ctx.moveTo(x + 12, y + 1);
    ctx.lineTo(x + w - 12, y + 1);
    ctx.stroke();

    // Corner accents
    if (options?.cornerAccents) {
      this.drawCornerAccents(x, y, w, h);
    }
  }

  // Cyan corner accent marks for sci-fi look
  private drawCornerAccents(x: number, y: number, w: number, h: number): void {
    const ctx = this.ctx;
    const accentLen = 12;
    const accentOffset = 4;

    ctx.strokeStyle = 'rgba(0, 255, 255, 0.5)';
    ctx.lineWidth = 1.5;

    // Top-left
    ctx.beginPath();
    ctx.moveTo(x + accentOffset, y + accentOffset + accentLen);
    ctx.lineTo(x + accentOffset, y + accentOffset);
    ctx.lineTo(x + accentOffset + accentLen, y + accentOffset);
    ctx.stroke();

    // Top-right
    ctx.beginPath();
    ctx.moveTo(x + w - accentOffset - accentLen, y + accentOffset);
    ctx.lineTo(x + w - accentOffset, y + accentOffset);
    ctx.lineTo(x + w - accentOffset, y + accentOffset + accentLen);
    ctx.stroke();

    // Bottom-left
    ctx.beginPath();
    ctx.moveTo(x + accentOffset, y + h - accentOffset - accentLen);
    ctx.lineTo(x + accentOffset, y + h - accentOffset);
    ctx.lineTo(x + accentOffset + accentLen, y + h - accentOffset);
    ctx.stroke();

    // Bottom-right
    ctx.beginPath();
    ctx.moveTo(x + w - accentOffset - accentLen, y + h - accentOffset);
    ctx.lineTo(x + w - accentOffset, y + h - accentOffset);
    ctx.lineTo(x + w - accentOffset, y + h - accentOffset - accentLen);
    ctx.stroke();
  }

  // Enhanced progress bar with glow effect
  private drawProgressBarEnhanced(
    x: number,
    y: number,
    w: number,
    h: number,
    percent: number,
    color: string,
    showGlow: boolean = false
  ): void {
    const ctx = this.ctx;

    // Background track
    ctx.fillStyle = 'rgba(30, 41, 59, 0.8)';
    ctx.beginPath();
    ctx.roundRect(x, y, w, h, h / 2);
    ctx.fill();

    // Inner shadow
    ctx.fillStyle = 'rgba(0, 0, 0, 0.3)';
    ctx.beginPath();
    ctx.roundRect(x + 1, y + 1, w - 2, h - 2, (h - 2) / 2);
    ctx.fill();

    // Fill
    if (percent > 0) {
      const fillWidth = Math.max(w * Math.min(percent, 1), h);

      // Glow effect
      if (showGlow) {
        ctx.save();
        ctx.shadowColor = color;
        ctx.shadowBlur = 8;
        ctx.fillStyle = color;
        ctx.beginPath();
        ctx.roundRect(x, y, fillWidth, h, h / 2);
        ctx.fill();
        ctx.restore();
      }

      // Main fill with gradient
      const fillGradient = ctx.createLinearGradient(x, y, x, y + h);
      fillGradient.addColorStop(0, this.lightenColor(color, 30));
      fillGradient.addColorStop(0.5, color);
      fillGradient.addColorStop(1, this.colorWithAlpha(color, 0.8));

      ctx.fillStyle = fillGradient;
      ctx.beginPath();
      ctx.roundRect(x, y, fillWidth, h, h / 2);
      ctx.fill();

      // Highlight shine
      ctx.fillStyle = 'rgba(255, 255, 255, 0.2)';
      ctx.beginPath();
      ctx.roundRect(x + 2, y + 1, fillWidth - 4, h / 3, h / 4);
      ctx.fill();
    }
  }

  // Draw a glowing text effect
  private drawGlowText(
    text: string,
    x: number,
    y: number,
    color: string,
    glowIntensity: number = 1
  ): void {
    const ctx = this.ctx;
    ctx.save();
    ctx.shadowColor = color;
    ctx.shadowBlur = 6 * glowIntensity;
    ctx.fillStyle = color;
    ctx.fillText(text, x, y);
    ctx.restore();
  }

  private renderConnectionStatus(state: RenderState): void {
    const canvas = this.ctx.canvas;
    const padding = 12;

    // Determine status color based on connection and latency
    let statusColor = '#22c55e'; // green - good
    let statusLabel = 'GOOD';

    if (state.connectionState === 'connecting') {
      statusColor = '#fbbf24'; // yellow
      statusLabel = 'CONNECTING';
    } else if (state.connectionState === 'disconnected' || state.connectionState === 'error') {
      statusColor = '#ef4444'; // red
      statusLabel = 'OFFLINE';
    } else if (state.rtt > 150) {
      statusColor = '#ef4444'; // red - high latency
      statusLabel = 'HIGH';
    } else if (state.rtt > 80) {
      statusColor = '#fbbf24'; // yellow - moderate latency
      statusLabel = 'OK';
    }

    // Pill background
    const pillW = 70;
    const pillH = 20;
    const pillX = Math.round(canvas.width - padding - pillW);
    const pillY = Math.round(padding - 2);

    this.ctx.fillStyle = 'rgba(15, 23, 42, 0.85)';
    this.ctx.beginPath();
    this.ctx.roundRect(pillX, pillY, pillW, pillH, pillH / 2);
    this.ctx.fill();
    this.ctx.strokeStyle = this.colorWithAlpha(statusColor, 0.3);
    this.ctx.lineWidth = 1;
    this.ctx.stroke();

    // Status dot with glow
    const dotX = pillX + 12;
    const dotY = pillY + pillH / 2;
    const dotRadius = 4;

    // Glow
    this.ctx.save();
    this.ctx.shadowColor = statusColor;
    this.ctx.shadowBlur = 8;
    this.ctx.fillStyle = statusColor;
    this.ctx.beginPath();
    this.ctx.arc(dotX, dotY, dotRadius, 0, Math.PI * 2);
    this.ctx.fill();
    this.ctx.restore();

    // Solid dot
    this.ctx.fillStyle = statusColor;
    this.ctx.beginPath();
    this.ctx.arc(dotX, dotY, dotRadius, 0, Math.PI * 2);
    this.ctx.fill();

    // RTT display
    if (state.connectionState === 'connected') {
      this.ctx.fillStyle = '#94a3b8';
      this.ctx.font = '10px monospace';
      this.ctx.textAlign = 'right';
      this.ctx.fillText(`${Math.round(state.rtt)}ms`, Math.round(pillX + pillW - 8), Math.round(pillY + 14));
    } else {
      this.ctx.fillStyle = statusColor;
      this.ctx.font = '9px Inter, system-ui, sans-serif';
      this.ctx.textAlign = 'right';
      this.ctx.fillText(statusLabel, Math.round(pillX + pillW - 8), Math.round(pillY + 14));
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
