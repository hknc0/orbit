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

// Trail point for motion effects
interface TrailPoint {
  x: number;
  y: number;
  timestamp: number;
  radius: number; // Capture radius at point creation for smooth size transitions
}

// Motion effect configuration - all values scale with player radius
// Base radius = 20 (mass 100), so multipliers are relative to that
const MOTION_FX = {
  // Trail configuration
  TRAIL_LIFETIME_BASE: 550,        // ms - base lifetime at radius 20
  TRAIL_LIFETIME_SCALE: 0.35,      // Larger players have slightly longer trails
  TRAIL_MAX_POINTS: 32,            // Max trail points (fixed for memory)
  TRAIL_MIN_DIST_RATIO: 0.35,      // Min distance = radius * this ratio (lower = denser trail)
  TRAIL_START_RADIUS_RATIO: 0.18,  // Trail starts at this fraction of player radius
  TRAIL_END_RADIUS_RATIO: 0.7,     // Trail ends at this fraction of player radius
  TRAIL_MAX_ALPHA: 0.6,            // Maximum trail opacity
  TRAIL_MIN_SPEED: 30,             // Speed below which trail starts fading
  TRAIL_FULL_SPEED: 80,            // Speed at which trail is fully visible

  // Boost flame configuration
  FLAME_LENGTH_BASE: 1.6,          // Flame length = radius * this (at rest)
  FLAME_LENGTH_SPEED_SCALE: 0.002, // Additional length per speed unit (reduced for better size scaling)
  FLAME_WIDTH_RATIO: 0.5,          // Flame width = radius * this
  FLAME_MIN_SPEED: 15,             // Minimum speed to show flame
  FLAME_SPARK_THRESHOLD: 100,      // Speed threshold for sparks
  FLAME_SPARK_COUNT_SCALE: 50,     // Speed per additional spark

  // Shared animation timing (for synchronized effects)
  FLICKER_SPEED_SLOW: 0.02,
  FLICKER_SPEED_MED: 0.035,
  FLICKER_SPEED_FAST: 0.06,
} as const;

export class RenderSystem {
  private ctx: CanvasRenderingContext2D;
  private cameraOffset: Vec2 = new Vec2();
  private targetCameraOffset: Vec2 = new Vec2();
  private cameraInitialized: boolean = false;
  private gameStartTime: number = 0;
  private readonly CAMERA_SMOOTHING = 0.1;

  // Dynamic zoom based on speed
  private currentZoom: number = 1.0;
  private targetZoom: number = 1.0;
  private readonly ZOOM_SMOOTHING = 0.05;
  private readonly ZOOM_MIN = 0.45; // Max zoom out at high speed
  private readonly ZOOM_MAX = 1.0;  // Normal zoom at rest
  private readonly SPEED_FOR_MAX_ZOOM_OUT = 250; // Speed at which max zoom out is reached

  // Spectator mode zoom settings
  private readonly SPECTATOR_ZOOM_MIN = 0.1; // Minimum zoom for full map view
  private readonly SPECTATOR_ARENA_PADDING = 2.5; // How much arena padding for full view

  // Smooth zoom transitions for large changes (spectator follow mode switch)
  private zoomTransitionStart: number = 0;
  private zoomTransitionFrom: number = 1.0;
  private zoomTransitionTo: number = 1.0;
  private lastTargetZoom: number = 1.0;
  private readonly ZOOM_TRANSITION_DURATION = 800; // ms for full transition
  private readonly ZOOM_TRANSITION_THRESHOLD = 0.2; // Delta that triggers smooth transition

  // Smooth camera position transitions for large changes (spectator view switch)
  private cameraTransitionStart: number = 0;
  private cameraTransitionFrom: Vec2 = new Vec2();
  private cameraTransitionTo: Vec2 = new Vec2();
  private lastTargetCameraOffset: Vec2 = new Vec2();
  private readonly CAMERA_TRANSITION_DURATION = 800; // ms for full transition
  private readonly CAMERA_TRANSITION_THRESHOLD = 200; // Distance that triggers smooth transition

  // Track previous speeds to detect acceleration (for other players' boost flames)
  private previousSpeeds: Map<string, number> = new Map();

  // Player birth effect duration (bornTime comes from StateSync)
  private readonly PLAYER_BIRTH_DURATION = 800; // ms

  // Unified motion trails for all players (consolidates trail + thrust visuals)
  private playerTrails: Map<string, TrailPoint[]> = new Map();
  private lastTrailPositions: Map<string, { x: number; y: number; radius: number }> = new Map();

  // Screen shake effect
  private shakeOffset = { x: 0, y: 0 };
  private shakeIntensity = 0;
  private readonly SHAKE_DECAY = 0.85;
  private readonly MAX_SHAKE = 12;

  // Arena scale tracking (for growth indicator)
  private scaleHistory: number[] = [];
  private readonly SCALE_HISTORY_SIZE = 60; // ~1 second at 60fps
  private scaleDirection: 'growing' | 'shrinking' | 'stable' = 'stable';

  // Performance: Cache parsed RGB values to avoid repeated hex parsing
  private colorCache: Map<string, { r: number; g: number; b: number }> = new Map();

  // Performance: Pre-computed sin/cos for 8-particle birth effect (fixed angles)
  private static readonly PARTICLE_ANGLES = (() => {
    const angles: { cos: number; sin: number }[] = [];
    for (let i = 0; i < 8; i++) {
      const angle = (i / 8) * Math.PI * 2;
      angles.push({ cos: Math.cos(angle), sin: Math.sin(angle) });
    }
    return angles;
  })();

  constructor(ctx: CanvasRenderingContext2D) {
    this.ctx = ctx;
  }

  // Effect quality levels based on zoom for spectator optimization
  // Reduces rendering cost when zoomed out (effects too small to see)
  // Full-view spectators always use reduced/minimal (viewing all entities)
  private getEffectQuality(world: World): 'full' | 'reduced' | 'minimal' {
    // Full-view spectators: viewing entire arena with all entities
    // Use reduced/minimal quality regardless of zoom (arena may be small)
    // Note: Following a player or well is NOT full-view mode
    if (world.isSpectator && world.spectateTargetId === null && world.spectateWellId === null) {
      if (this.currentZoom > 0.3) return 'reduced';
      return 'minimal';
    }

    // Players AND follow-mode spectators use normal thresholds
    // (follow-mode sees same AOI as target player)
    if (this.currentZoom > 0.4) return 'full';
    if (this.currentZoom > 0.2) return 'reduced';
    return 'minimal';
  }

  // Performance: Get cached RGB values, parse only once per color
  private getRGB(color: string): { r: number; g: number; b: number } {
    let rgb = this.colorCache.get(color);
    if (!rgb) {
      const hex = color.replace('#', '');
      rgb = {
        r: parseInt(hex.substring(0, 2), 16),
        g: parseInt(hex.substring(2, 4), 16),
        b: parseInt(hex.substring(4, 6), 16),
      };
      this.colorCache.set(color, rgb);
    }
    return rgb;
  }

  private updatePlayerTrails(world: World): void {
    const now = Date.now();
    const players = world.getPlayers();

    // Update trails for all alive players with size-scaled parameters
    for (const player of players.values()) {
      if (!player.alive) {
        this.playerTrails.delete(player.id);
        this.lastTrailPositions.delete(player.id);
        continue;
      }

      const radius = world.massToRadius(player.mass);

      // Size-scaled trail lifetime: larger players = slightly longer trails
      const trailLifetime = MOTION_FX.TRAIL_LIFETIME_BASE * (1 + (radius / 20 - 1) * MOTION_FX.TRAIL_LIFETIME_SCALE);

      let trail = this.playerTrails.get(player.id);
      if (!trail) {
        trail = [];
        this.playerTrails.set(player.id, trail);
      }

      // Remove expired trail points (age-based culling)
      while (trail.length > 0 && now - trail[0].timestamp > trailLifetime) {
        trail.shift();
      }

      // Size-scaled minimum distance between trail points
      const minDist = radius * MOTION_FX.TRAIL_MIN_DIST_RATIO;
      const pos = player.position;
      const lastPos = this.lastTrailPositions.get(player.id);

      if (lastPos) {
        const dx = pos.x - lastPos.x;
        const dy = pos.y - lastPos.y;
        const distSq = dx * dx + dy * dy; // Avoid sqrt when possible

        if (distSq >= minDist * minDist) {
          // Store radius with trail point for smooth size transitions during growth/shrink
          trail.push({ x: pos.x, y: pos.y, timestamp: now, radius });
          this.lastTrailPositions.set(player.id, { x: pos.x, y: pos.y, radius });

          // Cap trail points for memory efficiency
          while (trail.length > MOTION_FX.TRAIL_MAX_POINTS) {
            trail.shift();
          }
        }
      } else {
        this.lastTrailPositions.set(player.id, { x: pos.x, y: pos.y, radius });
      }
    }

    // Lazy cleanup of trails for players who left (batch cleanup)
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
    // Skip trails entirely when very zoomed out (sub-pixel rendering)
    const quality = this.getEffectQuality(world);
    if (quality === 'minimal') return;

    const now = Date.now();
    const ctx = this.ctx;
    const renderGlow = quality === 'full';

    for (const [playerId, trail] of this.playerTrails) {
      if (trail.length < 2) continue;

      const player = world.getPlayer(playerId);
      if (!player || !player.alive) continue;

      const color = world.getPlayerColor(player.colorIndex);
      const currentRadius = world.massToRadius(player.mass);

      // Speed-based trail visibility - fade out when moving slowly to avoid flickering circles
      const speed = player.velocity.length();
      if (speed < MOTION_FX.TRAIL_MIN_SPEED) continue; // Skip trail entirely when very slow
      const speedFade = speed >= MOTION_FX.TRAIL_FULL_SPEED ? 1.0 :
        (speed - MOTION_FX.TRAIL_MIN_SPEED) / (MOTION_FX.TRAIL_FULL_SPEED - MOTION_FX.TRAIL_MIN_SPEED);

      // All players now have transparent fill, so skip trail points too close to player
      // to avoid flickering through the hollow center
      const minDistFromPlayer = currentRadius * 1.5;
      const minDistSq = minDistFromPlayer * minDistFromPlayer;
      const playerX = player.position.x;
      const playerY = player.position.y;

      // Performance: Set fillStyle once per player, use globalAlpha for transparency
      ctx.fillStyle = color;

      // Size-scaled trail lifetime for consistent fading
      const trailLifetime = MOTION_FX.TRAIL_LIFETIME_BASE * (1 + (currentRadius / 20 - 1) * MOTION_FX.TRAIL_LIFETIME_SCALE);
      const invTrailLen = 1 / trail.length; // Pre-compute division

      // Render trail points from oldest to newest for proper layering
      for (let i = 0; i < trail.length; i++) {
        const point = trail[i];

        // Skip trail points too close to player (avoids flickering through hollow center)
        const dx = point.x - playerX;
        const dy = point.y - playerY;
        if (dx * dx + dy * dy < minDistSq) continue;

        const age = now - point.timestamp;
        const lifeRatio = 1 - age / trailLifetime;
        if (lifeRatio <= 0) continue; // Early exit for expired points

        const indexRatio = (i + 1) * invTrailLen; // +1 so first point isn't invisible

        // Smooth alpha: combines age fade, position fade, and speed fade
        const alpha = lifeRatio * indexRatio * speedFade * MOTION_FX.TRAIL_MAX_ALPHA;
        if (alpha < 0.015) continue;

        // Use stored radius for smooth size transitions, interpolate toward current
        const pointRadius = point.radius * 0.7 + currentRadius * 0.3;

        // Trail size grows from start to end of trail
        const trailRadius = pointRadius * (MOTION_FX.TRAIL_START_RADIUS_RATIO + indexRatio * (MOTION_FX.TRAIL_END_RADIUS_RATIO - MOTION_FX.TRAIL_START_RADIUS_RATIO));

        // Outer glow (subtle, larger) - skip when reduced quality
        if (renderGlow) {
          ctx.globalAlpha = alpha * 0.3;
          ctx.beginPath();
          ctx.arc(point.x, point.y, trailRadius * 1.4, 0, Math.PI * 2);
          ctx.fill();
        }

        // Core trail point - use globalAlpha (fillStyle already set)
        ctx.globalAlpha = alpha;
        ctx.beginPath();
        ctx.arc(point.x, point.y, trailRadius > 1.5 ? trailRadius : 1.5, 0, Math.PI * 2);
        ctx.fill();
      }
    }
    // Reset globalAlpha after trail rendering
    ctx.globalAlpha = 1.0;
  }

  render(world: World, state: RenderState): void {
    const canvas = this.ctx.canvas;
    const centerX = canvas.width / 2;
    const centerY = canvas.height / 2;

    // Update camera - handle spectator mode, follow mode, or local player
    if (world.isSpectator) {
      // Spectator mode
      if (world.isFullMapView()) {
        // Full map view: center on arena, zoom out to show entire arena
        this.targetCameraOffset.set(centerX, centerY);
        // Zoom out to fit arena - use arena safe radius scaled appropriately
        const arenaRadius = world.arena.outerRadius * world.arena.scale;
        const minDimension = Math.min(canvas.width, canvas.height);
        this.targetZoom = Math.max(this.SPECTATOR_ZOOM_MIN, minDimension / (arenaRadius * this.SPECTATOR_ARENA_PADDING));
      } else if (world.spectateTargetId !== null) {
        // Follow mode: track the spectated player
        const spectateTarget = world.getSpectateTarget();
        if (spectateTarget && spectateTarget.position && spectateTarget.velocity) {
          // Validate position before using
          const posX = spectateTarget.position.x;
          const posY = spectateTarget.position.y;
          if (isFinite(posX) && isFinite(posY)) {
            this.targetCameraOffset.set(centerX - posX, centerY - posY);
          }
          // Smooth zoom based on target's speed - safely get velocity length
          let speed = 0;
          try {
            // velocity might not be a Vec2 instance, handle gracefully
            if (typeof spectateTarget.velocity.length === 'function') {
              speed = spectateTarget.velocity.length() ?? 0;
            } else if (spectateTarget.velocity.x !== undefined && spectateTarget.velocity.y !== undefined) {
              // Fallback for plain object
              speed = Math.sqrt(spectateTarget.velocity.x ** 2 + spectateTarget.velocity.y ** 2);
            }
          } catch {
            speed = 0;
          }
          if (!isFinite(speed)) speed = 0;
          const speedRatio = Math.min(speed / this.SPEED_FOR_MAX_ZOOM_OUT, 1);
          this.targetZoom = this.ZOOM_MAX - (this.ZOOM_MAX - this.ZOOM_MIN) * speedRatio;
        } else {
          // Target not found in current state - fall back to full map view
          // This can happen briefly when target dies or leaves
          this.targetCameraOffset.set(centerX, centerY);
          const arenaRadius = world.arena.outerRadius * world.arena.scale;
          const minDimension = Math.min(canvas.width, canvas.height);
          this.targetZoom = Math.max(this.SPECTATOR_ZOOM_MIN, minDimension / (arenaRadius * this.SPECTATOR_ARENA_PADDING));
        }
      } else if (world.spectateWellId !== null) {
        // Follow mode: track the spectated gravity well
        const spectateWell = world.getSpectateWell();
        if (spectateWell && spectateWell.position) {
          const posX = spectateWell.position.x;
          const posY = spectateWell.position.y;
          if (isFinite(posX) && isFinite(posY)) {
            this.targetCameraOffset.set(centerX - posX, centerY - posY);
          }
          // Static zoom for wells - zoom in to see the star in detail
          // Larger wells get slightly more zoom out
          const wellZoomFactor = Math.min(1, 50 / spectateWell.coreRadius);
          this.targetZoom = this.ZOOM_MAX * wellZoomFactor;
        } else {
          // Well not found (destroyed?) - fall back to full map view
          this.targetCameraOffset.set(centerX, centerY);
          const arenaRadius = world.arena.outerRadius * world.arena.scale;
          const minDimension = Math.min(canvas.width, canvas.height);
          this.targetZoom = Math.max(this.SPECTATOR_ZOOM_MIN, minDimension / (arenaRadius * this.SPECTATOR_ARENA_PADDING));
        }
      }
      // Initialize camera for spectator mode
      if (!this.cameraInitialized) {
        this.cameraOffset.copy(this.targetCameraOffset);
        this.cameraInitialized = true;
      }
    } else {
      // Normal player mode: follow local player (only when alive)
      const localPlayer = world.getLocalPlayer();
      if (localPlayer && localPlayer.alive) {
        this.targetCameraOffset.set(
          centerX - localPlayer.position.x,
          centerY - localPlayer.position.y
        );

        // Snap camera on first frame or respawn (avoid jump)
        if (!this.cameraInitialized) {
          this.cameraOffset.copy(this.targetCameraOffset);
          this.cameraInitialized = true;
          if (this.gameStartTime === 0) {
            this.gameStartTime = Date.now();
          }
        }

        // Calculate dynamic zoom based on speed
        const speed = localPlayer.velocity.length();
        const speedRatio = Math.min(speed / this.SPEED_FOR_MAX_ZOOM_OUT, 1);
        this.targetZoom = this.ZOOM_MAX - (this.ZOOM_MAX - this.ZOOM_MIN) * speedRatio;
      } else {
        // Player dead or not found - reset for next spawn
        this.cameraInitialized = false;
        this.targetZoom = this.ZOOM_MAX;
      }
    }

    // Smooth camera interpolation with cinematic transitions for large changes
    this.updateCameraPosition();

    // Smooth zoom interpolation with cinematic transitions for large changes
    this.updateZoom();

    // Update trails for all players
    this.updatePlayerTrails(world);

    this.ctx.save();
    // Apply zoom centered on screen
    this.ctx.translate(centerX, centerY);
    this.ctx.scale(this.currentZoom, this.currentZoom);
    this.ctx.translate(-centerX, -centerY);
    // Update and apply shake
    this.updateShake();
    // Then apply camera offset with shake
    // Divide shake by zoom to keep screen-space shake constant regardless of zoom level
    this.ctx.translate(
      this.cameraOffset.x + this.shakeOffset.x / this.currentZoom,
      this.cameraOffset.y + this.shakeOffset.y / this.currentZoom
    );

    // Reset any lingering canvas state that might cause visual artifacts
    this.ctx.setLineDash([]);
    this.ctx.globalAlpha = 1.0;

    // Render in order (back to front)
    this.renderArena(world);
    this.renderChargingWells(world);
    this.renderGravityWaves(world);
    this.renderDeathEffects(world);
    this.renderCollisionEffects(world);
    this.renderPlayerTrails(world);                                    // Trails first (back)
    this.renderBoostFlames(world, state.input?.isBoosting ?? false);   // Flames on top of trails
    this.renderDebris(world);
    this.renderProjectiles(world);
    this.renderPlayerBodies(world);                                    // Bodies on top

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
    const quality = this.getEffectQuality(world);

    // Universe boundary (dynamic, contains all wells)
    this.ctx.strokeStyle = 'rgba(100, 100, 150, 0.3)';
    this.ctx.lineWidth = 2;
    this.ctx.beginPath();
    this.ctx.arc(0, 0, safeRadius, 0, Math.PI * 2);
    this.ctx.stroke();

    // Render gravity wells (suns) - each is its own "solar system"
    if (wells.length > 0) {
      for (const well of wells) {
        this.renderGravityWell(well.position.x, well.position.y, well.coreRadius, well.mass, well.id, well.bornTime, quality);

        // Draw orbit zones around each well (subtle rings) - skip at minimal quality
        if (quality !== 'minimal') {
          this.renderWellZones(well.position.x, well.position.y, well.coreRadius);
        }

        // Spectator follow indicator - subtle ring around followed well
        if (world.isSpectator && world.spectateWellId === well.id) {
          const indicatorRadius = well.coreRadius * 2.5;
          this.ctx.strokeStyle = 'rgba(255, 255, 255, 0.15)';
          this.ctx.lineWidth = 1;
          this.ctx.setLineDash([4, 8]);
          this.ctx.beginPath();
          this.ctx.arc(well.position.x, well.position.y, indicatorRadius, 0, Math.PI * 2);
          this.ctx.stroke();
          this.ctx.setLineDash([]);
        }
      }
    } else {
      // Fallback: single well at center
      this.renderGravityWell(0, 0, world.arena.coreRadius, 1000000, 0, 0, quality);
      if (quality !== 'minimal') {
        this.renderWellZones(0, 0, world.arena.coreRadius);
      }
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

  // Duration of birth animation in ms
  private readonly WELL_BIRTH_DURATION = 2500;

  private renderGravityWell(
    x: number,
    y: number,
    coreRadius: number,
    mass: number,
    wellId: number,
    bornTime: number,
    quality: 'full' | 'reduced' | 'minimal' = 'full'
  ): void {
    // Central supermassive black hole is always id 0 and near origin
    const isCentral = wellId === 0 && Math.abs(x) < 50 && Math.abs(y) < 50;

    // Calculate birth animation progress (0 = just born, 1 = fully materialized)
    const now = performance.now();
    const birthAge = bornTime > 0 ? now - bornTime : this.WELL_BIRTH_DURATION;
    const birthProgress = Math.min(1, birthAge / this.WELL_BIRTH_DURATION);

    // Skip rendering if not yet visible (shouldn't happen, but safety check)
    if (birthProgress <= 0) return;

    // Apply birth animation effects (skip at minimal quality for performance)
    this.ctx.save();
    if (birthProgress < 1 && quality !== 'minimal') {
      // Easing function for smooth animation (ease-out cubic)
      const eased = 1 - Math.pow(1 - birthProgress, 3);

      // Scale up from 0 to full size
      const scale = eased;
      this.ctx.translate(x, y);
      this.ctx.scale(scale, scale);
      this.ctx.translate(-x, -y);

      // Fade in with slight overshoot glow
      this.ctx.globalAlpha = eased;

      // Birth glow effect (expanding ring that fades) - only at full quality
      if (quality === 'full') {
        const birthGlowRadius = coreRadius * (1.5 + (1 - birthProgress) * 3);
        const birthGlow = this.ctx.createRadialGradient(x, y, coreRadius, x, y, birthGlowRadius);
        birthGlow.addColorStop(0, `rgba(255, 255, 255, ${0.6 * (1 - birthProgress)})`);
        birthGlow.addColorStop(0.5, `rgba(200, 220, 255, ${0.3 * (1 - birthProgress)})`);
        birthGlow.addColorStop(1, 'rgba(100, 150, 255, 0)');
        this.ctx.fillStyle = birthGlow;
        this.ctx.beginPath();
        this.ctx.arc(x, y, birthGlowRadius, 0, Math.PI * 2);
        this.ctx.fill();
      }
    } else if (birthProgress < 1 && quality === 'minimal') {
      // Minimal quality: just scale, no glow
      const eased = 1 - Math.pow(1 - birthProgress, 3);
      this.ctx.translate(x, y);
      this.ctx.scale(eased, eased);
      this.ctx.translate(-x, -y);
      this.ctx.globalAlpha = eased;
    }

    if (isCentral) {
      // === SUPERMASSIVE BLACK HOLE (Gargantua-inspired) ===
      // Quality levels optimize rendering for zoomed-out spectators
      const ctx = this.ctx;
      const eh = coreRadius; // Event horizon

      if (quality === 'minimal') {
        // MINIMAL: Iconic Gargantua look with simplified rendering (4 passes)
        // Still impressive but skips expensive effects (particles, Doppler, bloom, shadows)

        const diskInner = eh * 1.02;
        const diskOuter = eh * 3.5;

        // 1. Ambient glow (warm halo around everything)
        const ambientGlow = ctx.createRadialGradient(x, y, eh, x, y, eh * 4);
        ambientGlow.addColorStop(0, 'rgba(255, 200, 140, 0.15)');
        ambientGlow.addColorStop(0.5, 'rgba(255, 150, 80, 0.06)');
        ambientGlow.addColorStop(1, 'rgba(200, 100, 50, 0)');
        ctx.fillStyle = ambientGlow;
        ctx.beginPath();
        ctx.arc(x, y, eh * 4, 0, Math.PI * 2);
        ctx.fill();

        // 2. Accretion disk (the iconic horizontal ellipse)
        ctx.save();
        ctx.translate(x, y);
        ctx.scale(1, 0.18);
        const diskGrad = ctx.createRadialGradient(0, 0, diskInner, 0, 0, diskOuter);
        diskGrad.addColorStop(0, 'rgba(255, 250, 230, 0.9)');
        diskGrad.addColorStop(0.3, 'rgba(255, 200, 120, 0.6)');
        diskGrad.addColorStop(0.7, 'rgba(255, 140, 60, 0.25)');
        diskGrad.addColorStop(1, 'rgba(180, 80, 30, 0)');
        ctx.fillStyle = diskGrad;
        ctx.beginPath();
        ctx.arc(0, 0, diskOuter, 0, Math.PI * 2);
        ctx.arc(0, 0, diskInner, 0, Math.PI * 2, true);
        ctx.fill();
        ctx.restore();

        // 3. Event horizon (pure black void)
        ctx.fillStyle = '#000000';
        ctx.beginPath();
        ctx.arc(x, y, eh, 0, Math.PI * 2);
        ctx.fill();

        // 4. Photon ring (bright edge glow hugging the event horizon)
        const photonGrad = ctx.createRadialGradient(x, y, eh * 0.97, x, y, eh * 1.25);
        photonGrad.addColorStop(0, 'rgba(255, 255, 250, 0.9)');
        photonGrad.addColorStop(0.4, 'rgba(255, 240, 200, 0.7)');
        photonGrad.addColorStop(1, 'rgba(255, 180, 100, 0)');
        ctx.fillStyle = photonGrad;
        ctx.beginPath();
        ctx.arc(x, y, eh * 1.25, 0, Math.PI * 2);
        ctx.fill();
      } else if (quality === 'reduced') {
        // REDUCED: Skip particles, Doppler, bloom; use simpler gradients
        const diskInner = eh * 1.01;
        const diskOuterRadius = eh * 4.5;

        // Simplified outer glow (2 stops instead of 4)
        const glowRadius = eh * 6;
        const ambientGlow = ctx.createRadialGradient(x, y, eh, x, y, glowRadius);
        ambientGlow.addColorStop(0, 'rgba(255, 220, 180, 0.12)');
        ambientGlow.addColorStop(1, 'rgba(200, 120, 60, 0)');
        ctx.fillStyle = ambientGlow;
        ctx.beginPath();
        ctx.arc(x, y, glowRadius, 0, Math.PI * 2);
        ctx.fill();

        // Simplified halo (3 stops instead of 5, no bloom)
        const haloOuter = eh * 1.8;
        const haloInner = eh * 1.08;
        ctx.save();
        ctx.translate(x, y);
        const haloGrad = ctx.createRadialGradient(0, 0, haloInner, 0, 0, haloOuter);
        haloGrad.addColorStop(0, 'rgba(255, 255, 250, 0.9)');
        haloGrad.addColorStop(0.5, 'rgba(255, 220, 170, 0.4)');
        haloGrad.addColorStop(1, 'rgba(220, 140, 80, 0)');
        ctx.fillStyle = haloGrad;
        ctx.beginPath();
        ctx.arc(0, 0, haloOuter, 0, Math.PI * 2);
        ctx.arc(0, 0, haloInner, 0, Math.PI * 2, true);
        ctx.fill();
        ctx.restore();

        // Simplified accretion disk (3 stops instead of 6, no Doppler)
        ctx.save();
        ctx.translate(x, y);
        ctx.scale(1, 0.18);
        const diskGrad = ctx.createRadialGradient(0, 0, diskInner, 0, 0, diskOuterRadius);
        diskGrad.addColorStop(0, 'rgba(255, 250, 230, 0.85)');
        diskGrad.addColorStop(0.4, 'rgba(255, 180, 100, 0.4)');
        diskGrad.addColorStop(1, 'rgba(150, 70, 30, 0)');
        ctx.fillStyle = diskGrad;
        ctx.beginPath();
        ctx.arc(0, 0, diskOuterRadius, 0, Math.PI * 2);
        ctx.arc(0, 0, diskInner, 0, Math.PI * 2, true);
        ctx.fill();
        ctx.restore();

        // Event horizon (pure black core)
        ctx.fillStyle = '#000000';
        ctx.beginPath();
        ctx.arc(x, y, eh, 0, Math.PI * 2);
        ctx.fill();

        // Simplified photon ring (2 stops, no composite)
        const photonGlow = ctx.createRadialGradient(x, y, eh * 0.98, x, y, eh * 1.3);
        photonGlow.addColorStop(0, 'rgba(255, 250, 230, 0.7)');
        photonGlow.addColorStop(1, 'rgba(255, 200, 140, 0)');
        ctx.fillStyle = photonGlow;
        ctx.beginPath();
        ctx.arc(x, y, eh * 1.3, 0, Math.PI * 2);
        ctx.fill();
      } else {
        // FULL: All effects (original implementation)
        const time = performance.now() / 1000;
        const diskWidth = eh * 4.5;

        // 1. Outer ambient glow - soft warm halo
        const glowRadius = eh * 8;
        const ambientGlow = ctx.createRadialGradient(x, y, eh, x, y, glowRadius);
        ambientGlow.addColorStop(0, 'rgba(255, 240, 200, 0.18)');
        ambientGlow.addColorStop(0.3, 'rgba(255, 190, 130, 0.1)');
        ambientGlow.addColorStop(0.6, 'rgba(220, 130, 70, 0.04)');
        ambientGlow.addColorStop(1, 'rgba(150, 80, 40, 0)');
        ctx.fillStyle = ambientGlow;
        ctx.beginPath();
        ctx.arc(x, y, glowRadius, 0, Math.PI * 2);
        ctx.fill();

        // 2. Gravitational lensing halo
        const haloOuter = eh * 1.8;
        const haloInner = eh * 1.08;
        ctx.save();
        ctx.translate(x, y);
        const haloGrad = ctx.createRadialGradient(0, 0, haloInner, 0, 0, haloOuter);
        haloGrad.addColorStop(0, 'rgba(255, 255, 250, 0.95)');
        haloGrad.addColorStop(0.25, 'rgba(255, 250, 235, 0.8)');
        haloGrad.addColorStop(0.5, 'rgba(255, 230, 190, 0.5)');
        haloGrad.addColorStop(0.75, 'rgba(255, 190, 130, 0.2)');
        haloGrad.addColorStop(1, 'rgba(220, 140, 80, 0)');
        ctx.fillStyle = haloGrad;
        ctx.beginPath();
        ctx.arc(0, 0, haloOuter, 0, Math.PI * 2);
        ctx.arc(0, 0, haloInner, 0, Math.PI * 2, true);
        ctx.fill();

        // Soft bloom
        ctx.globalCompositeOperation = 'lighter';
        ctx.shadowBlur = 15;
        ctx.shadowColor = 'rgba(255, 250, 235, 0.5)';
        ctx.fillStyle = 'rgba(255, 255, 248, 0.15)';
        ctx.beginPath();
        ctx.arc(0, 0, haloInner * 1.1, 0, Math.PI * 2);
        ctx.fill();
        ctx.shadowBlur = 0;
        ctx.globalCompositeOperation = 'source-over';
        ctx.restore();

        // 3. Horizontal accretion disk
        const diskInner = eh * 1.01;
        const diskOuterRadius = eh * 4.5;
        ctx.save();
        ctx.translate(x, y);
        ctx.scale(1, 0.18);
        const diskGrad = ctx.createRadialGradient(0, 0, diskInner, 0, 0, diskOuterRadius);
        diskGrad.addColorStop(0, 'rgba(255, 255, 240, 0.95)');
        diskGrad.addColorStop(0.12, 'rgba(255, 240, 200, 0.8)');
        diskGrad.addColorStop(0.3, 'rgba(255, 200, 130, 0.55)');
        diskGrad.addColorStop(0.5, 'rgba(255, 150, 70, 0.35)');
        diskGrad.addColorStop(0.75, 'rgba(200, 100, 40, 0.15)');
        diskGrad.addColorStop(1, 'rgba(150, 70, 30, 0)');
        ctx.fillStyle = diskGrad;
        ctx.beginPath();
        ctx.arc(0, 0, diskOuterRadius, 0, Math.PI * 2);
        ctx.arc(0, 0, diskInner, 0, Math.PI * 2, true);
        ctx.fill();

        // Doppler brightening
        ctx.globalCompositeOperation = 'lighter';
        const dopplerGrad = ctx.createLinearGradient(-diskOuterRadius, 0, diskOuterRadius, 0);
        dopplerGrad.addColorStop(0, 'rgba(255, 160, 100, 0.08)');
        dopplerGrad.addColorStop(0.5, 'rgba(255, 230, 180, 0.15)');
        dopplerGrad.addColorStop(1, 'rgba(255, 255, 230, 0.25)');
        ctx.fillStyle = dopplerGrad;
        ctx.beginPath();
        ctx.arc(0, 0, diskOuterRadius * 0.95, 0, Math.PI * 2);
        ctx.arc(0, 0, diskInner * 1.05, 0, Math.PI * 2, true);
        ctx.fill();
        ctx.globalCompositeOperation = 'source-over';
        ctx.restore();

        // 4. Spiraling particles in disk
        ctx.save();
        ctx.translate(x, y);
        ctx.lineCap = 'round';
        ctx.lineWidth = 2;
        ctx.beginPath();
        for (let i = 0; i < 8; i++) {
          const phase = (time * 0.4 + i * 0.4) % 3;
          const progress = phase / 3;
          const spiralAngle = (i / 8) * Math.PI * 2 + progress * Math.PI * 4;
          const r = diskInner * 1.5 + (diskWidth - diskInner) * (1 - progress * progress);
          const px = Math.cos(spiralAngle) * r;
          const py = Math.sin(spiralAngle) * r * 0.18;
          const stretch = 10 * progress;
          const dx = Math.cos(spiralAngle + Math.PI * 0.5) * stretch;
          const dy = Math.sin(spiralAngle + Math.PI * 0.5) * stretch * 0.18;
          ctx.moveTo(px - dx, py - dy);
          ctx.lineTo(px, py);
        }
        ctx.globalAlpha = 0.3;
        ctx.strokeStyle = 'rgba(255, 230, 180, 1)';
        ctx.stroke();
        ctx.globalAlpha = 1;
        ctx.restore();

        // 5. Event horizon (pure black core)
        ctx.fillStyle = '#000000';
        ctx.beginPath();
        ctx.arc(x, y, eh, 0, Math.PI * 2);
        ctx.fill();

        // Subtle depth shading
        const horizonGrad = ctx.createRadialGradient(
          x - eh * 0.2, y - eh * 0.15, eh * 0.1,
          x, y, eh
        );
        horizonGrad.addColorStop(0, 'rgba(10, 10, 15, 1)');
        horizonGrad.addColorStop(0.5, 'rgba(0, 0, 0, 1)');
        horizonGrad.addColorStop(1, 'rgba(5, 5, 8, 0.95)');
        ctx.fillStyle = horizonGrad;
        ctx.beginPath();
        ctx.arc(x, y, eh, 0, Math.PI * 2);
        ctx.fill();

        // 6. Soft photon ring glow
        ctx.save();
        ctx.translate(x, y);
        const photonGlow = ctx.createRadialGradient(0, 0, eh * 0.95, 0, 0, eh * 1.4);
        photonGlow.addColorStop(0, 'rgba(0, 0, 0, 0)');
        photonGlow.addColorStop(0.4, 'rgba(255, 250, 230, 0.6)');
        photonGlow.addColorStop(0.6, 'rgba(255, 255, 245, 0.8)');
        photonGlow.addColorStop(0.75, 'rgba(255, 240, 200, 0.5)');
        photonGlow.addColorStop(1, 'rgba(255, 200, 140, 0)');
        ctx.globalCompositeOperation = 'lighter';
        ctx.fillStyle = photonGlow;
        ctx.beginPath();
        ctx.arc(0, 0, eh * 1.4, 0, Math.PI * 2);
        ctx.fill();
        ctx.globalCompositeOperation = 'source-over';
        ctx.restore();
      }
    } else {
      // === NORMAL GRAVITY WELL (star/sun) ===
      // Quality levels optimize rendering for zoomed-out spectators

      // Generate deterministic "random" values from well id (stable identifier)
      const seed = Math.abs(wellId * 7919 + 104729) % 10000;
      const starType = seed % 6;

      // Star color palettes based on stellar classification
      type StarColors = {
        core: [number, number, number];
        mid: [number, number, number];
        outer: [number, number, number];
        glow: [number, number, number];
      };

      const starTypes: StarColors[] = [
        { core: [200, 220, 255], mid: [150, 180, 255], outer: [100, 140, 255], glow: [80, 120, 255] },
        { core: [220, 230, 255], mid: [180, 200, 255], outer: [140, 170, 255], glow: [100, 150, 255] },
        { core: [255, 255, 255], mid: [240, 245, 255], outer: [220, 230, 250], glow: [200, 210, 240] },
        { core: [255, 250, 200], mid: [255, 220, 120], outer: [255, 180, 60], glow: [255, 150, 30] },
        { core: [255, 200, 150], mid: [255, 150, 80], outer: [255, 100, 40], glow: [255, 80, 20] },
        { core: [255, 180, 160], mid: [255, 120, 100], outer: [220, 80, 60], glow: [180, 50, 30] },
      ];

      const colors = starTypes[starType];

      if (quality === 'minimal') {
        // MINIMAL: Star-like appearance visible when zoomed out (2 passes)
        // Larger glow so it's visible at small screen sizes
        const glowRadius = coreRadius * 2.5;
        const outerGlow = this.ctx.createRadialGradient(x, y, coreRadius * 0.3, x, y, glowRadius);
        outerGlow.addColorStop(0, `rgba(${colors.glow[0]}, ${colors.glow[1]}, ${colors.glow[2]}, 0.5)`);
        outerGlow.addColorStop(0.4, `rgba(${colors.glow[0]}, ${colors.glow[1]}, ${colors.glow[2]}, 0.2)`);
        outerGlow.addColorStop(1, `rgba(${colors.glow[0]}, ${colors.glow[1]}, ${colors.glow[2]}, 0)`);
        this.ctx.fillStyle = outerGlow;
        this.ctx.beginPath();
        this.ctx.arc(x, y, glowRadius, 0, Math.PI * 2);
        this.ctx.fill();

        // Core with bright white-hot center (star-like gradient)
        const coreGrad = this.ctx.createRadialGradient(x, y, 0, x, y, coreRadius);
        coreGrad.addColorStop(0, 'rgba(255, 255, 255, 1)'); // White-hot center
        coreGrad.addColorStop(0.25, `rgba(${colors.core[0]}, ${colors.core[1]}, ${colors.core[2]}, 0.95)`);
        coreGrad.addColorStop(1, `rgba(${colors.outer[0]}, ${colors.outer[1]}, ${colors.outer[2]}, 0.6)`);
        this.ctx.fillStyle = coreGrad;
        this.ctx.beginPath();
        this.ctx.arc(x, y, coreRadius, 0, Math.PI * 2);
        this.ctx.fill();
      } else if (quality === 'reduced') {
        // REDUCED: Skip corona shimmer, use simpler gradients (2 stops instead of 3-4)
        // Simplified outer glow
        const glowRadius = coreRadius * 1.5;
        const outerGlow = this.ctx.createRadialGradient(x, y, coreRadius * 0.6, x, y, glowRadius);
        outerGlow.addColorStop(0, `rgba(${colors.glow[0]}, ${colors.glow[1]}, ${colors.glow[2]}, 0.3)`);
        outerGlow.addColorStop(1, `rgba(${colors.glow[0]}, ${colors.glow[1]}, ${colors.glow[2]}, 0)`);
        this.ctx.fillStyle = outerGlow;
        this.ctx.beginPath();
        this.ctx.arc(x, y, glowRadius, 0, Math.PI * 2);
        this.ctx.fill();

        // Simplified core gradient (2 stops)
        const gradient = this.ctx.createRadialGradient(x, y, 0, x, y, coreRadius);
        gradient.addColorStop(0, `rgba(${colors.core[0]}, ${colors.core[1]}, ${colors.core[2]}, 0.95)`);
        gradient.addColorStop(1, `rgba(${colors.outer[0]}, ${colors.outer[1]}, ${colors.outer[2]}, 0.4)`);
        this.ctx.fillStyle = gradient;
        this.ctx.beginPath();
        this.ctx.arc(x, y, coreRadius, 0, Math.PI * 2);
        this.ctx.fill();
      } else {
        // FULL: All effects (original implementation)
        const variation = (seed % 100) / 100;
        const vary = (c: number, amount: number) => Math.min(255, Math.max(0, c + (variation - 0.5) * amount));

        // Outer glow based on mass
        const glowRadius = coreRadius * (1.5 + Math.log10(mass) * 0.1);
        const outerGlow = this.ctx.createRadialGradient(x, y, coreRadius * 0.5, x, y, glowRadius);
        outerGlow.addColorStop(0, `rgba(${vary(colors.glow[0], 20)}, ${vary(colors.glow[1], 20)}, ${vary(colors.glow[2], 20)}, 0.35)`);
        outerGlow.addColorStop(0.5, `rgba(${vary(colors.glow[0], 30)}, ${vary(colors.glow[1], 30)}, ${vary(colors.glow[2], 30)}, 0.15)`);
        outerGlow.addColorStop(1, `rgba(${colors.glow[0]}, ${colors.glow[1]}, ${colors.glow[2]}, 0)`);
        this.ctx.fillStyle = outerGlow;
        this.ctx.beginPath();
        this.ctx.arc(x, y, glowRadius, 0, Math.PI * 2);
        this.ctx.fill();

        // Corona/surface activity (subtle shimmer effect for larger stars)
        if (coreRadius > 40) {
          const time = Date.now() / 2000;
          const coronaRadius = coreRadius * 1.15;
          this.ctx.save();
          this.ctx.globalAlpha = 0.3 + Math.sin(time * 2 + seed) * 0.1;
          const corona = this.ctx.createRadialGradient(x, y, coreRadius * 0.9, x, y, coronaRadius);
          corona.addColorStop(0, `rgba(${colors.core[0]}, ${colors.core[1]}, ${colors.core[2]}, 0.4)`);
          corona.addColorStop(1, `rgba(${colors.mid[0]}, ${colors.mid[1]}, ${colors.mid[2]}, 0)`);
          this.ctx.fillStyle = corona;
          this.ctx.beginPath();
          this.ctx.arc(x, y, coronaRadius, 0, Math.PI * 2);
          this.ctx.fill();
          this.ctx.restore();
        }

        // Core gradient
        const gradient = this.ctx.createRadialGradient(x, y, 0, x, y, coreRadius);
        gradient.addColorStop(0, `rgba(${vary(colors.core[0], 15)}, ${vary(colors.core[1], 15)}, ${vary(colors.core[2], 15)}, 0.95)`);
        gradient.addColorStop(0.3, `rgba(${vary(colors.mid[0], 20)}, ${vary(colors.mid[1], 20)}, ${vary(colors.mid[2], 20)}, 0.85)`);
        gradient.addColorStop(0.7, `rgba(${vary(colors.outer[0], 25)}, ${vary(colors.outer[1], 25)}, ${vary(colors.outer[2], 25)}, 0.6)`);
        gradient.addColorStop(1, `rgba(${colors.outer[0]}, ${colors.outer[1]}, ${colors.outer[2]}, 0.25)`);
        this.ctx.fillStyle = gradient;
        this.ctx.beginPath();
        this.ctx.arc(x, y, coreRadius, 0, Math.PI * 2);
        this.ctx.fill();

        // Bright center spot (photosphere highlight)
        const centerGlow = this.ctx.createRadialGradient(x, y, 0, x, y, coreRadius * 0.4);
        centerGlow.addColorStop(0, `rgba(255, 255, 255, 0.6)`);
        centerGlow.addColorStop(0.5, `rgba(${colors.core[0]}, ${colors.core[1]}, ${colors.core[2]}, 0.3)`);
        centerGlow.addColorStop(1, `rgba(${colors.core[0]}, ${colors.core[1]}, ${colors.core[2]}, 0)`);
        this.ctx.fillStyle = centerGlow;
        this.ctx.beginPath();
        this.ctx.arc(x, y, coreRadius * 0.4, 0, Math.PI * 2);
        this.ctx.fill();
      }
    }

    // Restore context after birth animation transforms
    this.ctx.restore();
  }

  // Debris sizes: Small=3, Medium=5, Large=8
  private static readonly DEBRIS_RADIUS = [3, 5, 8];
  // Debris colors: greenish for small, yellowish for medium, orange for large
  private static readonly DEBRIS_COLORS = [
    'rgba(100, 200, 100, 0.8)',  // Small - dim green
    'rgba(200, 200, 100, 0.9)',  // Medium - yellow
    'rgba(255, 180, 80, 1.0)',   // Large - bright orange
  ];

  private renderDebris(world: World): void {
    const debris = world.getDebris();
    if (debris.size === 0) return;

    // Batch draw by size for efficiency
    for (let size = 0; size < 3; size++) {
      const radius = RenderSystem.DEBRIS_RADIUS[size];
      const color = RenderSystem.DEBRIS_COLORS[size];

      this.ctx.fillStyle = color;
      this.ctx.beginPath();

      for (const d of debris.values()) {
        if (d.size !== size) continue;
        // Draw debris as small filled circle
        this.ctx.moveTo(d.position.x + radius, d.position.y);
        this.ctx.arc(d.position.x, d.position.y, radius, 0, Math.PI * 2);
      }

      this.ctx.fill();
    }
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

  // Render boost flames separately (rendered first, behind trails)
  private renderBoostFlames(world: World, localPlayerBoosting: boolean): void {
    // Skip flames entirely when very zoomed out (sub-pixel rendering)
    const quality = this.getEffectQuality(world);
    if (quality === 'minimal') return;

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
      const isLocal = player.id === world.localPlayerId;

      // Determine if flame should show
      const speed = player.velocity.length();
      let showFlame = false;

      if (isLocal) {
        showFlame = localPlayerBoosting;
      } else {
        const prevSpeed = this.previousSpeeds.get(player.id) ?? 0;
        const isAccelerating = speed > prevSpeed + 2;
        const hasHighSpeed = speed > 80;
        showFlame = isAccelerating || (hasHighSpeed && speed > prevSpeed);
      }

      this.previousSpeeds.set(player.id, speed);

      if (showFlame) {
        this.renderBoostFlame(player.position, player.velocity, radius, quality);
      }
    }
  }

  // Render player bodies (rendered last, on top of trails and flames)
  private renderPlayerBodies(world: World): void {
    const players = world.getPlayers();
    const now = Date.now();
    const quality = this.getEffectQuality(world);

    for (const player of players.values()) {
      if (!player.alive) continue;

      const radius = world.massToRadius(player.mass);
      const color = world.getPlayerColor(player.colorIndex);
      const isLocal = player.id === world.localPlayerId;

      // Birth effect - expanding rings when player spawns (skip at minimal quality)
      // bornTime > 0 means show animation, 0 means skip (entered AOI, not actually spawned)
      if (quality !== 'minimal' && player.bornTime > 0) {
        const birthAge = now - player.bornTime;
        if (birthAge < this.PLAYER_BIRTH_DURATION) {
          this.renderPlayerBirthEffect(player.position, radius, color, birthAge / this.PLAYER_BIRTH_DURATION, quality);
        }
      }

      // Kill effect - golden pulsing glow when player gets a kill (skip at minimal quality)
      if (quality !== 'minimal') {
        const killProgress = world.getKillEffectProgress(player.id);
        if (killProgress > 0) {
          this.renderKillEffect(player.position, radius, killProgress);
        }
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

      // Spectator follow indicator - subtle ring around followed player
      const isSpectateTarget = world.isSpectator && world.spectateTargetId === player.id;
      if (isSpectateTarget) {
        this.ctx.strokeStyle = 'rgba(34, 211, 238, 0.5)';
        this.ctx.lineWidth = 1.5;
        this.ctx.setLineDash([4, 4]);
        this.ctx.beginPath();
        this.ctx.arc(player.position.x, player.position.y, radius + 8, 0, Math.PI * 2);
        this.ctx.stroke();
        this.ctx.setLineDash([]);
      }

      // Player body - semi-transparent fill with solid outline (same style for all)
      // Semi-transparent fill
      this.ctx.fillStyle = this.colorWithAlpha(color, 0.15);
      this.ctx.beginPath();
      this.ctx.arc(player.position.x, player.position.y, radius, 0, Math.PI * 2);
      this.ctx.fill();

      // Solid color outline
      this.ctx.strokeStyle = color;
      this.ctx.lineWidth = 3;
      this.ctx.beginPath();
      this.ctx.arc(player.position.x, player.position.y, radius, 0, Math.PI * 2);
      this.ctx.stroke();

      // Outer glow ring (skip at minimal quality)
      if (quality !== 'minimal') {
        this.ctx.strokeStyle = this.colorWithAlpha(color, 0.4);
        this.ctx.lineWidth = 2;
        this.ctx.beginPath();
        this.ctx.arc(player.position.x, player.position.y, radius + 4, 0, Math.PI * 2);
        this.ctx.stroke();
      }

      // Direction indicator
      const dirX = Math.cos(player.rotation);
      const dirY = Math.sin(player.rotation);
      this.ctx.strokeStyle = color;
      this.ctx.lineWidth = 2;
      this.ctx.beginPath();
      this.ctx.moveTo(player.position.x, player.position.y);
      this.ctx.lineTo(
        player.position.x + dirX * radius * 0.8,
        player.position.y + dirY * radius * 0.8
      );
      this.ctx.stroke();

      // Player name with human/bot hint (skip at minimal quality - text too small)
      if (quality !== 'minimal') {
        const nameY = player.position.y - radius - 10;
        const playerName = world.getPlayerName(player.id);

        // Bot names are dimmed, humans are bright white with cyan dot
        this.ctx.textAlign = 'center';
        this.ctx.font = player.isBot ? '11px Inter, system-ui, sans-serif' : '12px Inter, system-ui, sans-serif';

        if (player.isBot) {
          this.ctx.fillStyle = '#64748b';
          this.ctx.fillText(playerName, player.position.x, nameY);
        } else if (!isLocal) {
          // Other human player: cyan dot + name
          this.ctx.fillStyle = '#22d3ee';
          this.ctx.fillText('•', player.position.x - this.ctx.measureText(playerName).width / 2 - 8, nameY);
          this.ctx.fillStyle = '#ffffff';
          this.ctx.fillText(playerName, player.position.x, nameY);
        } else {
          // Local player: just white name
          this.ctx.fillStyle = '#ffffff';
          this.ctx.fillText(playerName, player.position.x, nameY);
        }
      }
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

  // Player birth effect - expanding rings with player color
  private renderPlayerBirthEffect(position: Vec2, radius: number, color: string, progress: number, quality: 'full' | 'reduced' | 'minimal' = 'full'): void {
    // Ease-out cubic for smooth animation
    const eased = 1 - Math.pow(1 - progress, 3);
    const fadeOut = 1 - eased;

    // Inner glow - bright core that fades
    const innerGlowRadius = radius * (1.2 + fadeOut * 0.5);
    const innerGlow = this.ctx.createRadialGradient(
      position.x, position.y, 0,
      position.x, position.y, innerGlowRadius
    );
    innerGlow.addColorStop(0, `rgba(255, 255, 255, ${fadeOut * 0.7})`);
    innerGlow.addColorStop(0.4, this.colorWithAlpha(color, fadeOut * 0.5));
    innerGlow.addColorStop(1, this.colorWithAlpha(color, 0));

    this.ctx.fillStyle = innerGlow;
    this.ctx.beginPath();
    this.ctx.arc(position.x, position.y, innerGlowRadius, 0, Math.PI * 2);
    this.ctx.fill();

    // Expanding ring 1 - main ring
    const ring1Radius = radius * (1 + eased * 2.5);
    const ring1Alpha = fadeOut * 0.8;
    this.ctx.strokeStyle = this.colorWithAlpha(color, ring1Alpha);
    this.ctx.lineWidth = 3 * fadeOut + 1;
    this.ctx.beginPath();
    this.ctx.arc(position.x, position.y, ring1Radius, 0, Math.PI * 2);
    this.ctx.stroke();

    // Expanding ring 2 - delayed secondary ring
    if (progress > 0.15) {
      const ring2Progress = (progress - 0.15) / 0.85;
      const ring2Eased = 1 - Math.pow(1 - ring2Progress, 3);
      const ring2FadeOut = 1 - ring2Eased;
      const ring2Radius = radius * (1 + ring2Eased * 3.5);
      const ring2Alpha = ring2FadeOut * 0.5;

      this.ctx.strokeStyle = this.colorWithAlpha(color, ring2Alpha);
      this.ctx.lineWidth = 2 * ring2FadeOut + 0.5;
      this.ctx.beginPath();
      this.ctx.arc(position.x, position.y, ring2Radius, 0, Math.PI * 2);
      this.ctx.stroke();
    }

    // Particle burst effect - use pre-computed sin/cos and globalAlpha (skip when reduced quality)
    if (quality !== 'full') return; // Skip particles at reduced quality

    const particleDist = radius * (0.8 + eased * 2);
    const particleSize = (3 + radius * 0.05) * fadeOut;

    if (particleSize > 0.5) {
      this.ctx.fillStyle = color;
      this.ctx.globalAlpha = fadeOut * 0.7;
      const angles = RenderSystem.PARTICLE_ANGLES;
      for (let i = 0; i < 8; i++) {
        const particleX = position.x + angles[i].cos * particleDist;
        const particleY = position.y + angles[i].sin * particleDist;
        this.ctx.beginPath();
        this.ctx.arc(particleX, particleY, particleSize, 0, Math.PI * 2);
        this.ctx.fill();
      }
      this.ctx.globalAlpha = 1.0;
    }
  }

  private renderDeathEffects(world: World): void {
    const effects = world.getDeathEffects();
    if (effects.length === 0) return;

    const ctx = this.ctx;
    const quality = this.getEffectQuality(world);

    for (const effect of effects) {
      const { position, color, radius } = effect;
      const progress = Math.max(0.001, effect.progress);
      if (progress <= 0) continue;

      const rgb = this.getRGB(color);
      const invProgress = 1 - progress;

      // Scale factor based on player size (baseline radius ~20)
      const scale = Math.max(0.5, radius / 20);

      // Cubic ease for explosive initial burst
      const easeOut = 1 - invProgress * invProgress * invProgress;
      const easeIn = progress * progress;

      // === Quick bright flash (first 20%) ===
      if (progress > 0.8) {
        const flashT = (progress - 0.8) / 0.2;
        ctx.globalAlpha = flashT * 0.7;
        ctx.fillStyle = '#ffffff';
        ctx.beginPath();
        ctx.arc(position.x, position.y, radius * flashT, 0, Math.PI * 2);
        ctx.fill();
      }

      // === Expanding shockwave ring ===
      const ringRadius = radius * 0.5 + easeOut * radius * 2.5;
      const heat = progress * 0.4;
      const wr = Math.round(rgb.r + (255 - rgb.r) * heat);
      const wg = Math.round(rgb.g + (255 - rgb.g) * heat);
      const wb = Math.round(rgb.b + (255 - rgb.b) * heat);
      ctx.globalAlpha = easeIn * 0.3;
      ctx.strokeStyle = `rgb(${wr}, ${wg}, ${wb})`;
      ctx.lineWidth = Math.max(2, 3 * progress * scale);
      ctx.beginPath();
      ctx.arc(position.x, position.y, ringRadius, 0, Math.PI * 2);
      ctx.stroke();

      // Skip particles at minimal quality (just show ring)
      if (quality === 'minimal') {
        ctx.globalAlpha = 1;
        continue;
      }

      // Particle color (white-hot → player color)
      const particleHeat = progress * 0.3;
      const pr = Math.round(rgb.r + (255 - rgb.r) * particleHeat);
      const pg = Math.round(rgb.g + (255 - rgb.g) * particleHeat);
      const pb = Math.round(rgb.b + (255 - rgb.b) * particleHeat);
      ctx.strokeStyle = `rgb(${pr}, ${pg}, ${pb})`;
      ctx.lineCap = 'round';

      // === Primary debris (4 larger, slower, motion-stretched) ===
      ctx.lineWidth = Math.max(3, 5 * progress * scale);
      ctx.globalAlpha = easeIn * 0.45;
      ctx.beginPath();

      for (let i = 0; i < 4; i++) {
        const { cos, sin } = RenderSystem.PARTICLE_ANGLES[i * 2];
        const dist = radius * 0.4 + easeOut * radius * 1.8;
        const px = position.x + cos * dist;
        const py = position.y + sin * dist;
        // Motion stretch: trails behind, shrinks as particle slows
        const stretch = 5 * progress * scale;
        ctx.moveTo(px - cos * stretch, py - sin * stretch);
        ctx.lineTo(px, py);
      }
      ctx.stroke();

      // === Secondary debris (4 smaller, faster, further) - skip at reduced quality ===
      if (quality === 'full') {
        ctx.lineWidth = Math.max(2, 3.5 * progress * scale);
        ctx.globalAlpha = easeIn * 0.3;
        ctx.beginPath();

        for (let i = 0; i < 4; i++) {
          const { cos, sin } = RenderSystem.PARTICLE_ANGLES[i * 2 + 1];
          const dist = radius * 0.6 + easeOut * radius * 2.8;
          const px = position.x + cos * dist;
          const py = position.y + sin * dist;
          const stretch = 4 * progress * scale;
          ctx.moveTo(px - cos * stretch, py - sin * stretch);
          ctx.lineTo(px, py);
        }
        ctx.stroke();
      }

      ctx.lineCap = 'butt';

      // === Inner hot core ===
      ctx.globalAlpha = easeIn * 0.2;
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(position.x, position.y, radius * 0.2 + easeOut * radius * 0.5, 0, Math.PI * 2);
      ctx.fill();

      ctx.globalAlpha = 1;
    }
  }

  // Trigger screen shake (called when local player is in collision)
  triggerShake(intensity: number): void {
    this.shakeIntensity = Math.min(this.shakeIntensity + intensity * 8, this.MAX_SHAKE);
  }

  // Update shake (called each frame)

  // Smooth camera position with cinematic transitions for large changes (e.g., spectator view switch)
  private updateCameraPosition(): void {
    const now = performance.now();
    const dx = this.targetCameraOffset.x - this.lastTargetCameraOffset.x;
    const dy = this.targetCameraOffset.y - this.lastTargetCameraOffset.y;
    const targetDelta = Math.sqrt(dx * dx + dy * dy);

    // Detect significant target change - start a new transition
    if (targetDelta > this.CAMERA_TRANSITION_THRESHOLD) {
      this.cameraTransitionStart = now;
      this.cameraTransitionFrom.copy(this.cameraOffset);
      this.cameraTransitionTo.copy(this.targetCameraOffset);
      this.lastTargetCameraOffset.copy(this.targetCameraOffset);
    } else if (targetDelta > 5) {
      // Small target changes - update the transition target smoothly
      this.cameraTransitionTo.copy(this.targetCameraOffset);
      this.lastTargetCameraOffset.copy(this.targetCameraOffset);
    }

    // Check if we're in a major transition
    if (this.cameraTransitionStart > 0) {
      const elapsed = now - this.cameraTransitionStart;
      const duration = this.CAMERA_TRANSITION_DURATION;

      if (elapsed < duration) {
        // Time-based animation with ease-in-out cubic
        const progress = elapsed / duration;
        const eased = progress < 0.5
          ? 4 * progress * progress * progress
          : 1 - Math.pow(-2 * progress + 2, 3) / 2;

        this.cameraOffset.x = this.cameraTransitionFrom.x +
          (this.cameraTransitionTo.x - this.cameraTransitionFrom.x) * eased;
        this.cameraOffset.y = this.cameraTransitionFrom.y +
          (this.cameraTransitionTo.y - this.cameraTransitionFrom.y) * eased;
      } else {
        // Transition complete
        this.cameraOffset.copy(this.cameraTransitionTo);
        this.cameraTransitionStart = 0;
      }
    } else {
      // Normal exponential smoothing for small/continuous changes (player following)
      this.cameraOffset.x += (this.targetCameraOffset.x - this.cameraOffset.x) * this.CAMERA_SMOOTHING;
      this.cameraOffset.y += (this.targetCameraOffset.y - this.cameraOffset.y) * this.CAMERA_SMOOTHING;
    }
  }

  // Smooth zoom with cinematic transitions for large changes (e.g., spectator follow mode)
  private updateZoom(): void {
    const now = performance.now();
    const targetDelta = Math.abs(this.targetZoom - this.lastTargetZoom);

    // Detect significant target change - start a new transition
    if (targetDelta > this.ZOOM_TRANSITION_THRESHOLD) {
      this.zoomTransitionStart = now;
      this.zoomTransitionFrom = this.currentZoom;
      this.zoomTransitionTo = this.targetZoom;
      this.lastTargetZoom = this.targetZoom;
    } else if (targetDelta > 0.01) {
      // Small target changes - update the transition target smoothly
      this.zoomTransitionTo = this.targetZoom;
      this.lastTargetZoom = this.targetZoom;
    }

    // Check if we're in a major transition
    if (this.zoomTransitionStart > 0) {
      const elapsed = now - this.zoomTransitionStart;
      const duration = this.ZOOM_TRANSITION_DURATION;

      if (elapsed < duration) {
        // Time-based animation with ease-in-out cubic
        const progress = elapsed / duration;
        const eased = progress < 0.5
          ? 4 * progress * progress * progress
          : 1 - Math.pow(-2 * progress + 2, 3) / 2;

        this.currentZoom = this.zoomTransitionFrom +
          (this.zoomTransitionTo - this.zoomTransitionFrom) * eased;
      } else {
        // Transition complete
        this.currentZoom = this.zoomTransitionTo;
        this.zoomTransitionStart = 0;
      }
    } else {
      // Normal exponential smoothing for small/continuous changes (speed-based zoom)
      this.currentZoom += (this.targetZoom - this.currentZoom) * this.ZOOM_SMOOTHING;
    }
  }

  private updateShake(): void {
    if (this.shakeIntensity > 0.5) {
      const angle = Math.random() * Math.PI * 2;
      this.shakeOffset.x = Math.cos(angle) * this.shakeIntensity;
      this.shakeOffset.y = Math.sin(angle) * this.shakeIntensity;
      this.shakeIntensity *= this.SHAKE_DECAY;
    } else {
      this.shakeOffset.x = 0;
      this.shakeOffset.y = 0;
      this.shakeIntensity = 0;
    }
  }

  private renderCollisionEffects(world: World): void {
    // Skip collision effects entirely when very zoomed out
    const quality = this.getEffectQuality(world);
    if (quality === 'minimal') return;

    const effects = world.getCollisionEffects();

    for (const effect of effects) {
      const { position, progress, intensity, color } = effect;

      // Skip if too faded
      if (progress < 0.05) continue;

      const baseAlpha = progress * intensity;
      const ctx = this.ctx;

      // 1. Central flash (quick fade)
      if (progress > 0.5) {
        const flashProgress = (progress - 0.5) * 2;
        const flashRadius = Math.max(1, 15 + (1 - flashProgress) * 20);

        const gradient = ctx.createRadialGradient(
          position.x, position.y, 0,
          position.x, position.y, flashRadius
        );
        gradient.addColorStop(0, `rgba(255, 255, 255, ${flashProgress * 0.8})`);
        gradient.addColorStop(0.5, this.colorWithAlpha(color, flashProgress * 0.5));
        gradient.addColorStop(1, 'rgba(255, 255, 255, 0)');

        ctx.fillStyle = gradient;
        ctx.beginPath();
        ctx.arc(position.x, position.y, flashRadius, 0, Math.PI * 2);
        ctx.fill();
      }

      // 2. Expanding ring
      const ringRadius = Math.max(1, 10 + (1 - progress) * 50 * intensity);
      ctx.strokeStyle = this.colorWithAlpha(color, baseAlpha * 0.7);
      ctx.lineWidth = Math.max(0.5, 2 + progress * 2);
      ctx.beginPath();
      ctx.arc(position.x, position.y, ringRadius, 0, Math.PI * 2);
      ctx.stroke();

      // 3. Particles (6 particles, no shadow blur for performance) - skip at reduced quality
      if (quality === 'full') {
        const particleCount = 6;
        for (let i = 0; i < particleCount; i++) {
          const angle = (i / particleCount) * Math.PI * 2;
          const dist = (1 - progress) * 40 * intensity;
          const px = position.x + Math.cos(angle) * dist;
          const py = position.y + Math.sin(angle) * dist;
          const size = Math.max(0.5, 2 + progress * 3);

          ctx.fillStyle = this.colorWithAlpha(color, baseAlpha * 0.6);
          ctx.beginPath();
          ctx.arc(px, py, size, 0, Math.PI * 2);
          ctx.fill();
        }
      }
    }
  }

  // Render charging wells (pulsing warning before explosion)
  private renderChargingWells(world: World): void {
    const chargingWells = world.getChargingWells();
    if (chargingWells.length === 0) return;

    const ctx = this.ctx;
    // Build lookup map for O(1) well access (scales to 1000s of wells)
    const wellMap = new Map(world.arena.gravityWells.map(w => [w.id, w]));

    for (const well of chargingWells) {
      const { progress } = well;

      // Find the corresponding well to get current position and core radius
      // Wells orbit, so we must use current interpolated position, not stale event position
      const wellData = wellMap.get(well.wellId);
      if (!wellData) continue; // Well may have been removed

      const position = wellData.position;
      const coreRadius = wellData.coreRadius;

      // Pulsing intensity increases as explosion approaches
      const pulseSpeed = 5 + progress * 15; // Faster as it gets closer
      const pulse = 0.5 + 0.5 * Math.sin(Date.now() / (1000 / pulseSpeed));
      const intensity = progress * pulse;

      // Inner warning glow
      const glowRadius = coreRadius * (1.5 + progress * 0.5);
      const gradient = ctx.createRadialGradient(
        position.x, position.y, coreRadius * 0.5,
        position.x, position.y, glowRadius
      );
      gradient.addColorStop(0, `rgba(255, 100, 50, ${intensity * 0.6})`);
      gradient.addColorStop(0.5, `rgba(255, 50, 20, ${intensity * 0.3})`);
      gradient.addColorStop(1, 'rgba(255, 0, 0, 0)');

      ctx.fillStyle = gradient;
      ctx.beginPath();
      ctx.arc(position.x, position.y, glowRadius, 0, Math.PI * 2);
      ctx.fill();

      // Pulsing ring
      ctx.strokeStyle = `rgba(255, 150, 50, ${intensity * 0.8})`;
      ctx.lineWidth = 2 + progress * 3;
      ctx.beginPath();
      ctx.arc(position.x, position.y, coreRadius * (1.2 + pulse * 0.3), 0, Math.PI * 2);
      ctx.stroke();
    }
  }

  // Render expanding gravity wave effects
  private renderGravityWaves(world: World): void {
    const waves = world.getGravityWaveEffects();
    const ctx = this.ctx;

    // Wave constants (should match server)
    const WAVE_MAX_RADIUS = 2000;
    const WAVE_FRONT_THICKNESS = 80;

    for (const wave of waves) {
      const { position, progress, strength } = wave;

      // Current wave radius
      const radius = progress * WAVE_MAX_RADIUS;

      // Alpha decreases as wave expands
      const alpha = (1 - progress) * strength;
      if (alpha < 0.02) continue;

      // Main wave ring
      const ringInner = Math.max(0, radius - WAVE_FRONT_THICKNESS * 0.5);
      const ringOuter = radius + WAVE_FRONT_THICKNESS * 0.5;

      // Gradient for wave front
      const gradient = ctx.createRadialGradient(
        position.x, position.y, ringInner,
        position.x, position.y, ringOuter
      );

      // Orange/red wave color
      gradient.addColorStop(0, 'rgba(255, 100, 50, 0)');
      gradient.addColorStop(0.3, `rgba(255, 150, 80, ${alpha * 0.4})`);
      gradient.addColorStop(0.5, `rgba(255, 200, 100, ${alpha * 0.6})`);
      gradient.addColorStop(0.7, `rgba(255, 150, 80, ${alpha * 0.4})`);
      gradient.addColorStop(1, 'rgba(255, 100, 50, 0)');

      ctx.fillStyle = gradient;
      ctx.beginPath();
      ctx.arc(position.x, position.y, ringOuter, 0, Math.PI * 2);
      ctx.arc(position.x, position.y, ringInner, 0, Math.PI * 2, true);
      ctx.fill();

      // Brighter leading edge
      ctx.strokeStyle = `rgba(255, 220, 150, ${alpha * 0.8})`;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(position.x, position.y, radius, 0, Math.PI * 2);
      ctx.stroke();

      // Inner glow (fades quickly)
      if (progress < 0.3) {
        const innerAlpha = (0.3 - progress) / 0.3 * strength;
        const innerGradient = ctx.createRadialGradient(
          position.x, position.y, 0,
          position.x, position.y, radius * 0.5
        );
        innerGradient.addColorStop(0, `rgba(255, 255, 200, ${innerAlpha * 0.5})`);
        innerGradient.addColorStop(0.5, `rgba(255, 200, 100, ${innerAlpha * 0.3})`);
        innerGradient.addColorStop(1, 'rgba(255, 150, 50, 0)');

        ctx.fillStyle = innerGradient;
        ctx.beginPath();
        ctx.arc(position.x, position.y, radius * 0.5, 0, Math.PI * 2);
        ctx.fill();
      }
    }
  }

  private renderBoostFlame(position: Vec2, velocity: Vec2, radius: number, quality: 'full' | 'reduced' | 'minimal' = 'full'): void {
    const speed = velocity.length();
    if (speed < MOTION_FX.FLAME_MIN_SPEED) return;

    const ctx = this.ctx;
    const time = performance.now();

    // Flame direction is opposite to velocity (exhaust behind player)
    const invSpeed = 1 / speed;
    const dirX = -velocity.x * invSpeed;
    const dirY = -velocity.y * invSpeed;

    // Flame origin at back of player, scaled with radius
    const flameX = position.x + dirX * radius;
    const flameY = position.y + dirY * radius;

    // Size-scaled flame dimensions
    // Length scales with radius and speed (larger players = proportionally larger flames)
    const baseLen = radius * MOTION_FX.FLAME_LENGTH_BASE + speed * MOTION_FX.FLAME_LENGTH_SPEED_SCALE * radius;
    const flameWidth = radius * MOTION_FX.FLAME_WIDTH_RATIO;

    // Organic flicker using shared timing constants
    const flicker = 0.88 + Math.sin(time * MOTION_FX.FLICKER_SPEED_MED) * 0.08 +
                          Math.sin(time * MOTION_FX.FLICKER_SPEED_FAST) * 0.04;
    const flameLen = baseLen * flicker;

    // Perpendicular direction for flame width
    const perpX = -dirY;
    const perpY = dirX;

    // === LAYER 1: Outer glow (soft, pulsing) ===
    const glowAlpha = 0.22 + Math.sin(time * MOTION_FX.FLICKER_SPEED_SLOW) * 0.08;
    ctx.fillStyle = `rgba(255, 100, 30, ${glowAlpha})`;
    ctx.beginPath();
    ctx.moveTo(flameX + perpX * flameWidth * 1.5, flameY + perpY * flameWidth * 1.5);
    ctx.lineTo(flameX - perpX * flameWidth * 1.5, flameY - perpY * flameWidth * 1.5);
    ctx.lineTo(flameX + dirX * flameLen * 1.15, flameY + dirY * flameLen * 1.15);
    ctx.closePath();
    ctx.fill();

    // === LAYER 2: Main outer flame (orange-red) ===
    ctx.fillStyle = 'rgba(255, 120, 40, 0.88)';
    ctx.beginPath();
    ctx.moveTo(flameX + perpX * flameWidth, flameY + perpY * flameWidth);
    ctx.lineTo(flameX - perpX * flameWidth, flameY - perpY * flameWidth);
    ctx.lineTo(flameX + dirX * flameLen, flameY + dirY * flameLen);
    ctx.closePath();
    ctx.fill();

    // === LAYER 3: Middle flame (orange-yellow, independent flicker) ===
    const midFlicker = 0.92 + Math.sin(time * MOTION_FX.FLICKER_SPEED_FAST + 1) * 0.08;
    ctx.fillStyle = 'rgba(255, 180, 60, 0.92)';
    ctx.beginPath();
    ctx.moveTo(flameX + perpX * flameWidth * 0.62, flameY + perpY * flameWidth * 0.62);
    ctx.lineTo(flameX - perpX * flameWidth * 0.62, flameY - perpY * flameWidth * 0.62);
    ctx.lineTo(flameX + dirX * flameLen * 0.72 * midFlicker, flameY + dirY * flameLen * 0.72 * midFlicker);
    ctx.closePath();
    ctx.fill();

    // === LAYER 4: Inner core (bright yellow-white, fastest flicker) ===
    const coreFlicker = 0.85 + Math.sin(time * 0.08 + 2) * 0.15;
    ctx.fillStyle = 'rgba(255, 245, 190, 1)';
    ctx.beginPath();
    ctx.moveTo(flameX + perpX * flameWidth * 0.28, flameY + perpY * flameWidth * 0.28);
    ctx.lineTo(flameX - perpX * flameWidth * 0.28, flameY - perpY * flameWidth * 0.28);
    ctx.lineTo(flameX + dirX * flameLen * 0.42 * coreFlicker, flameY + dirY * flameLen * 0.42 * coreFlicker);
    ctx.closePath();
    ctx.fill();

    // === SPARKS: Size-scaled particles at higher speeds (skip when reduced quality) ===
    if (quality === 'full' && speed > MOTION_FX.FLAME_SPARK_THRESHOLD) {
      const sparkCount = Math.min(4, 1 + Math.floor((speed - MOTION_FX.FLAME_SPARK_THRESHOLD) / MOTION_FX.FLAME_SPARK_COUNT_SCALE));
      // Spark size scales with player radius
      const baseSparkSize = Math.max(1.5, radius * 0.08);

      for (let i = 0; i < sparkCount; i++) {
        const sparkPhase = (time * 0.012 + i * 2.1) % 1;
        const sparkDist = flameLen * (0.55 + sparkPhase * 0.55);
        const sparkOffset = Math.sin(time * MOTION_FX.FLICKER_SPEED_SLOW + i * 3) * flameWidth * 0.35;
        const sparkX = flameX + dirX * sparkDist + perpX * sparkOffset;
        const sparkY = flameY + dirY * sparkDist + perpY * sparkOffset;
        const sparkAlpha = (1 - sparkPhase) * 0.9;
        const sparkSize = baseSparkSize * (1 - sparkPhase * 0.4);

        ctx.fillStyle = `rgba(255, 225, 160, ${sparkAlpha})`;
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
      this.ctx.fillStyle = '#00ffff';
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
      this.ctx.fillStyle = '#00ffff';
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

      // Name - cyan for local, white+dot for humans, dim gray for bots
      const name = entry.name.length > 9 ? entry.name.slice(0, 9) + '…' : entry.name;
      this.ctx.font = `${isLocal ? 'bold ' : ''}11px Inter, system-ui, sans-serif`;

      if (isLocal) {
        this.ctx.fillStyle = '#00ffff';
        this.ctx.fillText(name, Math.round(lbPanelX + 32), y + 4);
      } else if (entry.isBot) {
        this.ctx.fillStyle = '#64748b';
        this.ctx.fillText(name, Math.round(lbPanelX + 32), y + 4);
      } else {
        // Other human: cyan dot + white name
        this.ctx.fillStyle = '#22d3ee';
        this.ctx.fillText('•', Math.round(lbPanelX + 30), y + 4);
        this.ctx.fillStyle = '#e2e8f0';
        this.ctx.fillText(name, Math.round(lbPanelX + 38), y + 4);
      }

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

    // === AI MANAGER STATUS (replaces keyboard controls hint) ===
    if (world.aiStatus?.enabled) {
      const ai = world.aiStatus;
      const statusText = `AI: ${ai.decisionsTotal} decisions  •  ${ai.successRate}% success  •  ${ai.confidence}% confidence`;
      this.ctx.font = '10px Inter, system-ui, sans-serif';
      const textWidth = this.ctx.measureText(statusText).width;
      const pillW = textWidth + 32;
      const pillH = 22;
      const pillX = Math.round((canvas.width - pillW) / 2);
      const pillY = Math.round(canvas.height - padding - pillH - 2);

      // Pill background with AI-themed color
      this.ctx.fillStyle = 'rgba(15, 23, 42, 0.85)';
      this.ctx.beginPath();
      this.ctx.roundRect(pillX, pillY, pillW, pillH, pillH / 2);
      this.ctx.fill();

      // Border color based on success rate
      const borderColor = ai.successRate >= 70 ? 'rgba(34, 197, 94, 0.4)' : // green
                          ai.successRate >= 50 ? 'rgba(251, 191, 36, 0.4)' : // yellow
                          'rgba(239, 68, 68, 0.4)'; // red
      this.ctx.strokeStyle = borderColor;
      this.ctx.lineWidth = 1;
      this.ctx.stroke();

      // AI icon
      this.ctx.fillStyle = '#a78bfa'; // Purple for AI
      this.ctx.textAlign = 'left';
      this.ctx.fillText('🤖', pillX + 8, pillY + 15);

      // Status text
      this.ctx.fillStyle = '#94a3b8';
      this.ctx.textAlign = 'center';
      this.ctx.fillText(statusText, Math.round(canvas.width / 2) + 6, pillY + 15);
    }

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

    // === SPECTATOR MODE INDICATOR ===
    if (world.isSpectator) {
      const specPanelW = 280;
      const specPanelH = 40;
      const specPanelX = Math.round((canvas.width - specPanelW) / 2);
      const specPanelY = padding;

      // Panel background
      this.ctx.fillStyle = 'rgba(15, 23, 42, 0.9)';
      this.ctx.beginPath();
      this.ctx.roundRect(specPanelX, specPanelY, specPanelW, specPanelH, 6);
      this.ctx.fill();

      // Cyan border
      this.ctx.strokeStyle = 'rgba(0, 255, 255, 0.4)';
      this.ctx.lineWidth = 1;
      this.ctx.stroke();

      // Icon and text
      this.ctx.font = 'bold 12px Inter, system-ui, sans-serif';
      this.ctx.textAlign = 'center';

      if (world.spectateTargetId) {
        // Following a player
        const target = world.getPlayer(world.spectateTargetId);
        const targetName = target?.name || 'Unknown';
        this.ctx.fillStyle = '#00ffff';
        this.ctx.fillText(`👁 FOLLOWING: ${targetName}`, specPanelX + specPanelW / 2, specPanelY + 17);
        this.ctx.fillStyle = '#64748b';
        this.ctx.font = '10px Inter, system-ui, sans-serif';
        this.ctx.fillText('Click empty space to return to full view', specPanelX + specPanelW / 2, specPanelY + 32);
      } else if (world.spectateWellId !== null) {
        // Following a gravity well
        const well = world.getSpectateWell();
        const isCentral = well && well.id === 0;
        const wellName = isCentral ? 'Black Hole' : `Star #${world.spectateWellId}`;
        this.ctx.fillStyle = '#fbbf24'; // Amber for stars
        this.ctx.fillText(`⭐ VIEWING: ${wellName}`, specPanelX + specPanelW / 2, specPanelY + 17);
        this.ctx.fillStyle = '#64748b';
        this.ctx.font = '10px Inter, system-ui, sans-serif';
        this.ctx.fillText('Click empty space to return to full view', specPanelX + specPanelW / 2, specPanelY + 32);
      } else {
        // Full map view
        this.ctx.fillStyle = '#a78bfa';
        this.ctx.fillText('👁 SPECTATOR MODE', specPanelX + specPanelW / 2, specPanelY + 17);
        this.ctx.fillStyle = '#64748b';
        this.ctx.font = '10px Inter, system-ui, sans-serif';
        this.ctx.fillText('Click a player or star to follow', specPanelX + specPanelW / 2, specPanelY + 32);
      }
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

      // Subtle density colormap: muted blue → teal → warm glow
      const getDensityColor = (t: number): [number, number, number] => {
        // Muted 3-stop gradient for subtle background appearance
        if (t < 0.4) {
          // Dark blue-teal (low density - barely visible)
          const s = t / 0.4;
          return [
            Math.floor(20 + 20 * s),      // 20 → 40
            Math.floor(50 + 60 * s),      // 50 → 110
            Math.floor(80 + 40 * s)       // 80 → 120
          ];
        } else if (t < 0.7) {
          // Teal to muted amber (medium density)
          const s = (t - 0.4) / 0.3;
          return [
            Math.floor(40 + 80 * s),      // 40 → 120
            Math.floor(110 + 30 * s),     // 110 → 140
            Math.floor(120 - 60 * s)      // 120 → 60
          ];
        } else {
          // Muted amber to warm orange (high density)
          const s = (t - 0.7) / 0.3;
          return [
            Math.floor(120 + 60 * s),     // 120 → 180
            Math.floor(140 - 20 * s),     // 140 → 120
            Math.floor(60 - 20 * s)       // 60 → 40
          ];
        }
      };

      // Render with soft gradients as subtle background
      for (let gy = 0; gy < GRID_SIZE; gy++) {
        for (let gx = 0; gx < GRID_SIZE; gx++) {
          const idx = gy * GRID_SIZE + gx;
          const density = densityGrid[idx];

          if (density > 0) {
            // Cell center position on minimap
            const cellCenterX = centerX - gridPixelSize / 2 + (gx + 0.5) * cellPixelSize;
            const cellCenterY = centerY - gridPixelSize / 2 + (gy + 0.5) * cellPixelSize;

            // Intensity with strong gamma for subtle low-end
            const rawIntensity = Math.min(density / maxDensity, 1);
            const intensity = Math.pow(rawIntensity, 0.7); // Gentler curve

            const [r, g, b] = getDensityColor(intensity);

            // Soft blob with gentle overlap
            const blobRadius = cellPixelSize * 1.1;
            const gradient = this.ctx.createRadialGradient(
              cellCenterX, cellCenterY, 0,
              cellCenterX, cellCenterY, blobRadius
            );

            // Very subtle alpha - background hint only
            const baseAlpha = 0.08 + intensity * 0.17; // 0.08-0.25 range
            gradient.addColorStop(0, `rgba(${r}, ${g}, ${b}, ${baseAlpha})`);
            gradient.addColorStop(0.4, `rgba(${r}, ${g}, ${b}, ${baseAlpha * 0.6})`);
            gradient.addColorStop(0.7, `rgba(${r}, ${g}, ${b}, ${baseAlpha * 0.25})`);
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

    // 1b. Gravity wells - central black hole (purple) and stars (orange)
    for (let i = 0; i < world.arena.gravityWells.length; i++) {
      const well = world.arena.gravityWells[i];
      const wellX = centerX + well.position.x * scale;
      const wellY = centerY + well.position.y * scale;

      // Central supermassive black hole is always index 0 and near origin
      const isCentral = i === 0 && Math.abs(well.position.x) < 50 && Math.abs(well.position.y) < 50;

      // Only draw if within minimap bounds
      const dist = Math.sqrt(Math.pow(wellX - centerX, 2) + Math.pow(wellY - centerY, 2));
      if (dist < minimapSize / 2 - 2) {
        if (isCentral) {
          // Black hole with warm orange accretion glow (matching main render)
          // Outer glow
          this.ctx.fillStyle = 'rgba(255, 160, 80, 0.4)';
          this.ctx.beginPath();
          this.ctx.arc(wellX, wellY, 5, 0, Math.PI * 2);
          this.ctx.fill();
          // Black core
          this.ctx.fillStyle = '#000000';
          this.ctx.beginPath();
          this.ctx.arc(wellX, wellY, 2.5, 0, Math.PI * 2);
          this.ctx.fill();
          // Bright ring
          this.ctx.strokeStyle = 'rgba(255, 200, 140, 0.8)';
          this.ctx.lineWidth = 1;
          this.ctx.beginPath();
          this.ctx.arc(wellX, wellY, 2.5, 0, Math.PI * 2);
          this.ctx.stroke();
        } else {
          // Orange dot for normal stars, size based on core radius
          const dotSize = Math.max(1.5, well.coreRadius / 35);
          this.ctx.fillStyle = '#ff9944';
          this.ctx.beginPath();
          this.ctx.arc(wellX, wellY, dotSize, 0, Math.PI * 2);
          this.ctx.fill();
        }
      }
    }

    // 1c. Charging wells - pulsing warning indicator
    const chargingWells = world.getChargingWells();
    for (const charging of chargingWells) {
      const wellX = centerX + charging.position.x * scale;
      const wellY = centerY + charging.position.y * scale;
      const dist = Math.sqrt(Math.pow(wellX - centerX, 2) + Math.pow(wellY - centerY, 2));

      if (dist < minimapSize / 2 - 2) {
        const pulse = 0.5 + 0.5 * Math.sin(Date.now() / 100);
        const alpha = charging.progress * pulse;

        // Pulsing red/orange glow
        this.ctx.fillStyle = `rgba(255, 100, 50, ${alpha * 0.6})`;
        this.ctx.beginPath();
        this.ctx.arc(wellX, wellY, 5 + charging.progress * 3, 0, Math.PI * 2);
        this.ctx.fill();

        // Warning ring
        this.ctx.strokeStyle = `rgba(255, 150, 50, ${alpha})`;
        this.ctx.lineWidth = 1.5;
        this.ctx.beginPath();
        this.ctx.arc(wellX, wellY, 4 + pulse * 2, 0, Math.PI * 2);
        this.ctx.stroke();
      }
    }

    // 1d. Gravity waves - expanding rings
    const waves = world.getGravityWaveEffects();
    const WAVE_MAX_RADIUS = 2000;

    for (const wave of waves) {
      const waveX = centerX + wave.position.x * scale;
      const waveY = centerY + wave.position.y * scale;
      const waveRadius = wave.progress * WAVE_MAX_RADIUS * scale;

      // Only draw if within minimap and visible
      const alpha = (1 - wave.progress) * wave.strength;
      if (alpha < 0.1 || waveRadius < 1) continue;

      // Expanding ring on minimap
      this.ctx.strokeStyle = `rgba(255, 180, 80, ${alpha * 0.8})`;
      this.ctx.lineWidth = 1.5;
      this.ctx.beginPath();
      this.ctx.arc(waveX, waveY, waveRadius, 0, Math.PI * 2);
      this.ctx.stroke();
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

    // 2. Notable players (high mass) - thin red circle indicators
    const notablePlayers = world.getNotablePlayers();
    const visiblePlayerIds = new Set(world.getPlayers().keys());

    for (const notable of notablePlayers) {
      // Skip if already visible as regular player or is local player
      if (visiblePlayerIds.has(notable.id) || notable.id === world.localPlayerId) continue;

      const pos = clampToMinimap(
        centerX + notable.position.x * scale,
        centerY + notable.position.y * scale
      );

      // Thin red circle outline
      this.ctx.strokeStyle = '#ef4444';
      this.ctx.lineWidth = 1;
      this.ctx.beginPath();
      this.ctx.arc(pos.x, pos.y, 4, 0, Math.PI * 2);
      this.ctx.stroke();
    }

    // 3. Other players (nearby/visible) - dynamic filtering based on player count
    const totalAlive = world.getAlivePlayerCount();
    // Smooth scaling: 100 at 0 players → ~250 at 200+ players
    // Formula: base + (playerCount / scale)^curve
    const massThreshold = Math.min(100 + Math.pow(totalAlive / 1.5, 0.8), 300);

    for (const [playerId, player] of world.getPlayers()) {
      if (!player.alive) continue;
      if (playerId === world.localPlayerId) continue;
      if (player.mass < massThreshold) continue;

      const pos = clampToMinimap(
        centerX + player.position.x * scale,
        centerY + player.position.y * scale
      );

      // Smaller dots to reduce overlap
      const dotSize = Math.min(1.5 + (player.mass - massThreshold) / 150, 3);
      const color = world.getPlayerColor(player.colorIndex);

      // Opacity based on mass - lower mass = more transparent
      const opacity = Math.min(0.5 + (player.mass - massThreshold) / (massThreshold * 2), 1);

      // Parse hex color to RGB for opacity
      const r = parseInt(color.slice(1, 3), 16);
      const g = parseInt(color.slice(3, 5), 16);
      const b = parseInt(color.slice(5, 7), 16);

      // No outline - just colored fill with opacity
      this.ctx.fillStyle = `rgba(${r}, ${g}, ${b}, ${opacity})`;
      this.ctx.beginPath();
      this.ctx.arc(pos.x, pos.y, dotSize, 0, Math.PI * 2);
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
        // Alive: Use player's chosen color
        // Pulsing effect for extra visibility
        const pulse = 0.7 + 0.3 * Math.sin(Date.now() / 200);
        const pulseSize = 1 + 0.15 * Math.sin(Date.now() / 300);
        const playerColor = world.getPlayerColor(localPlayer.colorIndex);

        // Parse hex color to RGB for glow
        const r = parseInt(playerColor.slice(1, 3), 16);
        const g = parseInt(playerColor.slice(3, 5), 16);
        const b = parseInt(playerColor.slice(5, 7), 16);

        // Outer pulsing glow - player color
        this.ctx.fillStyle = `rgba(${r}, ${g}, ${b}, ${0.15 * pulse})`;
        this.ctx.beginPath();
        this.ctx.arc(pos.x, pos.y, 16 * pulseSize, 0, Math.PI * 2);
        this.ctx.fill();

        // Strong black outline for contrast
        this.ctx.strokeStyle = '#000000';
        this.ctx.lineWidth = 4;
        this.ctx.beginPath();
        this.ctx.arc(pos.x, pos.y, 7, 0, Math.PI * 2);
        this.ctx.stroke();

        // Player color ring (hollow - no fill)
        this.ctx.strokeStyle = playerColor;
        this.ctx.lineWidth = 2;
        this.ctx.beginPath();
        this.ctx.arc(pos.x, pos.y, 7, 0, Math.PI * 2);
        this.ctx.stroke();

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

    // === INDICATORS (left of minimap) ===
    const indicatorX = minimapX - 10;

    // --- Arena Scale Indicator ---
    const currentScale = world.arena.scale;

    // Track scale over time to detect growth/shrink
    this.scaleHistory.push(currentScale);
    if (this.scaleHistory.length > this.SCALE_HISTORY_SIZE) {
      this.scaleHistory.shift();
    }

    // Compare current scale to scale from ~1 second ago
    if (this.scaleHistory.length >= this.SCALE_HISTORY_SIZE) {
      const oldScale = this.scaleHistory[0];
      const scaleDelta = currentScale - oldScale;
      if (scaleDelta > 0.002) {
        this.scaleDirection = 'growing';
      } else if (scaleDelta < -0.002) {
        this.scaleDirection = 'shrinking';
      } else {
        this.scaleDirection = 'stable';
      }
    }

    // Scale display with direction arrow
    this.ctx.font = '10px Inter, system-ui, sans-serif';
    this.ctx.textAlign = 'right';

    const scaleText = `${currentScale.toFixed(1)} AU`;
    const arrowColor = this.scaleDirection === 'growing' ? 'rgba(74, 222, 128, 0.9)' :
                       this.scaleDirection === 'shrinking' ? 'rgba(251, 146, 60, 0.9)' :
                       'rgba(148, 163, 184, 0.3)';

    // Scale value with galaxy icon (fixed position)
    this.ctx.fillStyle = 'rgba(139, 92, 246, 0.8)'; // Purple
    this.ctx.fillText(scaleText, indicatorX, minimapY + 12);

    // Arrow at fixed position (always same spot, just different visibility)
    const arrow = this.scaleDirection === 'growing' ? '↑' :
                  this.scaleDirection === 'shrinking' ? '↓' : '·';
    this.ctx.fillStyle = arrowColor;
    this.ctx.fillText(arrow, indicatorX + 10, minimapY + 12);

    // --- Zoom Bar Indicator ---
    const barX = indicatorX;
    const barY = minimapY + minimapSize / 2 + 10;
    const barHeight = 30;
    const barWidth = 3;
    const zoomRatio = (this.currentZoom - this.ZOOM_MIN) / (this.ZOOM_MAX - this.ZOOM_MIN);
    const fillHeight = barHeight * zoomRatio;

    // Bar background
    this.ctx.fillStyle = 'rgba(30, 41, 59, 0.5)';
    this.ctx.fillRect(barX - barWidth / 2, barY - barHeight / 2, barWidth, barHeight);

    // Filled portion (bottom-up: more fill = more zoomed in)
    this.ctx.fillStyle = 'rgba(96, 165, 250, 0.6)';
    this.ctx.fillRect(
      barX - barWidth / 2,
      barY + barHeight / 2 - fillHeight,
      barWidth,
      fillHeight
    );
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
    const rgb = this.getRGB(color);
    return `rgba(${rgb.r}, ${rgb.g}, ${rgb.b}, ${alpha})`;
  }

  private lightenColor(color: string, percent: number): string {
    const rgb = this.getRGB(color);
    const r = Math.min(255, rgb.r + percent);
    const g = Math.min(255, rgb.g + percent);
    const b = Math.min(255, rgb.b + percent);
    return `rgb(${r}, ${g}, ${b})`;
  }

  /**
   * Convert screen coordinates to world coordinates
   * Used for click-to-follow in spectator mode
   */
  screenToWorld(screenX: number, screenY: number): Vec2 {
    const canvas = this.ctx.canvas;
    const centerX = canvas.width / 2;
    const centerY = canvas.height / 2;

    // Reverse the camera transformation:
    // 1. Screen -> centered coords
    // 2. Undo zoom
    // 3. Undo camera offset
    // Safeguard: prevent division by zero/invalid zoom
    const zoom = this.currentZoom > 0.001 ? this.currentZoom : 0.1;
    const worldX = (screenX - centerX) / zoom + centerX - this.cameraOffset.x;
    const worldY = (screenY - centerY) / zoom + centerY - this.cameraOffset.y;

    return new Vec2(worldX, worldY);
  }

  /** Reset render state for new game session */
  reset(): void {
    this.cameraOffset.set(0, 0);
    this.targetCameraOffset.set(0, 0);
    this.cameraInitialized = false;
    this.gameStartTime = 0;
    this.currentZoom = this.ZOOM_MAX;
    this.targetZoom = this.ZOOM_MAX;
    this.zoomTransitionStart = 0;
    this.zoomTransitionFrom = this.ZOOM_MAX;
    this.zoomTransitionTo = this.ZOOM_MAX;
    this.lastTargetZoom = this.ZOOM_MAX;
    this.cameraTransitionStart = 0;
    this.cameraTransitionFrom.set(0, 0);
    this.cameraTransitionTo.set(0, 0);
    this.lastTargetCameraOffset.set(0, 0);
    this.previousSpeeds.clear();
    this.playerTrails.clear();
    this.lastTrailPositions.clear();
    this.shakeIntensity = 0;
    this.shakeOffset = { x: 0, y: 0 };
    this.scaleHistory = [];
    this.scaleDirection = 'stable';
  }

  /** Get current zoom level (for viewport info reporting to server) */
  getCurrentZoom(): number {
    return this.currentZoom;
  }
}
