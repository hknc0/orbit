// Main game controller for multiplayer client

import { World } from './World';
import { GameTransport, type ConnectionState } from '@/net/Transport';
import { StateSync } from '@/net/StateSync';
import { InputSystem } from '@/systems/InputSystem';
import { RenderSystem } from '@/systems/RenderSystem';
import type { ServerMessage, GameEvent, MatchPhase, PlayerId } from '@/net/Protocol';

export type GamePhase = 'menu' | 'connecting' | 'countdown' | 'playing' | 'ended' | 'disconnected';

export interface GameEvents {
  onPhaseChange: (phase: GamePhase) => void;
  onKillFeed: (killerName: string, victimName: string) => void;
  onConnectionError: (error: string) => void;
}

export class Game {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;

  // Core systems
  private world: World;
  private transport: GameTransport;
  private stateSync: StateSync;
  private inputSystem: InputSystem;
  private renderSystem: RenderSystem;

  // Game state
  private phase: GamePhase = 'menu';
  private animationFrameId: number = 0;
  private lastTime: number = 0;
  private inputSequence: number = 0;

  // Event handlers
  private events: GameEvents;

  // Spectator heartbeat interval (keeps spectators alive, prevents idle kick)
  private spectatorHeartbeatInterval: number | null = null;

  // Viewport info for server-side entity filtering
  private lastReportedZoom: number = 1.0;
  private readonly VIEWPORT_REPORT_THRESHOLD = 0.05; // Report when zoom changes by 5%

  // Server URL (set via setServer, secure default to localhost)
  private serverUrl: string = 'https://localhost:4433';
  private certHash?: string;

  constructor(canvas: HTMLCanvasElement, events: GameEvents) {
    this.canvas = canvas;
    const ctx = canvas.getContext('2d');
    if (!ctx) {
      throw new Error('Failed to get 2D canvas context');
    }
    this.ctx = ctx;
    this.events = events;

    // Initialize systems
    this.world = new World();
    this.stateSync = new StateSync();
    this.inputSystem = new InputSystem(canvas);
    this.renderSystem = new RenderSystem(ctx);

    // Initialize transport with event handlers
    this.transport = new GameTransport({
      onStateChange: this.handleConnectionStateChange.bind(this),
      onMessage: this.handleServerMessage.bind(this),
      onError: this.handleConnectionError.bind(this),
    });

    // Set up spectator click-to-follow
    this.canvas.addEventListener('click', this.handleSpectatorClick.bind(this));
  }

  // Handle click to follow a player or gravity well in spectator mode
  private handleSpectatorClick(e: MouseEvent): void {
    try {
      // Only handle clicks in spectator mode during gameplay
      if (!this.world.isSpectator || (this.phase !== 'playing' && this.phase !== 'countdown')) {
        return;
      }

      // Convert screen coords to world coords
      const worldPos = this.renderSystem.screenToWorld(e.clientX, e.clientY);

      // Validate world position
      if (!worldPos || !isFinite(worldPos.x) || !isFinite(worldPos.y)) {
        console.warn('Invalid world position from screenToWorld');
        return;
      }

      // Find the closest alive player to the click position
      let closestPlayer: { id: string; distance: number } | null = null;
      const playerClickRadius = 100; // Max distance to select a player

      const players = this.world.getPlayers();
      for (const player of players.values()) {
        if (!player.alive) continue;
        if (!player.position || !isFinite(player.position.x) || !isFinite(player.position.y)) continue;

        const dx = player.position.x - worldPos.x;
        const dy = player.position.y - worldPos.y;
        const distance = Math.sqrt(dx * dx + dy * dy);

        // Account for player size (mass affects visual size)
        const playerRadius = Math.sqrt(player.mass || 100) * 2;
        const adjustedDistance = Math.max(0, distance - playerRadius);

        if (adjustedDistance <= playerClickRadius) {
          if (!closestPlayer || adjustedDistance < closestPlayer.distance) {
            closestPlayer = { id: player.id, distance: adjustedDistance };
          }
        }
      }

      // Find the closest gravity well to the click position
      let closestWell: { id: number; distance: number } | null = null;
      const wellClickRadius = 150; // Max distance to select a gravity well

      for (const well of this.world.arena.gravityWells) {
        if (!well.position || !isFinite(well.position.x) || !isFinite(well.position.y)) continue;

        const dx = well.position.x - worldPos.x;
        const dy = well.position.y - worldPos.y;
        const distance = Math.sqrt(dx * dx + dy * dy);

        // Account for well visual size (coreRadius + glow)
        const wellVisualRadius = well.coreRadius * 2.5;
        const adjustedDistance = Math.max(0, distance - wellVisualRadius);

        if (adjustedDistance <= wellClickRadius) {
          if (!closestWell || adjustedDistance < closestWell.distance) {
            closestWell = { id: well.id, distance: adjustedDistance };
          }
        }
      }

      // Priority: player first, then well (players are more interactive)
      if (closestPlayer) {
        // Follow this player - validate ID before sending
        if (typeof closestPlayer.id === 'string' && closestPlayer.id.length > 0) {
          this.world.setSpectateWell(null); // Clear any well target
          this.setSpectateTarget(closestPlayer.id);
        } else {
          console.warn('Invalid player ID:', closestPlayer.id);
        }
      } else if (closestWell) {
        // Follow this gravity well
        this.setSpectateTarget(null); // Clear player target (don't send to server)
        this.world.spectateTargetId = null;
        this.world.setSpectateWell(closestWell.id);
      } else if (this.world.spectateTargetId !== null || this.world.spectateWellId !== null) {
        // Clicked on empty space while following something - return to full view
        this.world.setSpectateWell(null);
        this.setSpectateTarget(null);
      }
    } catch (err) {
      console.error('Error in spectator click handler:', err);
    }
  }

  // Configure server connection
  setServer(url: string, certHash?: string): void {
    this.serverUrl = url;
    this.certHash = certHash;
  }

  // Start connecting and playing
  async start(playerName: string, colorIndex: number, isSpectator: boolean = false): Promise<void> {
    this.setPhase('connecting');
    this.inputSequence = 0;

    try {
      await this.transport.connect(this.serverUrl, this.certHash);

      // Send join request
      await this.transport.sendReliable({
        type: 'JoinRequest',
        playerName,
        colorIndex,
        isSpectator,
      });
    } catch (err) {
      this.setPhase('disconnected');
      this.events.onConnectionError(err instanceof Error ? err.message : 'Connection failed');
    }
  }

  // Set spectator follow target (null = full map view)
  setSpectateTarget(targetId: string | null): void {
    this.world.spectateTargetId = targetId;
    this.transport.sendReliable({
      type: 'SpectateTarget',
      targetId,
    });
  }

  // Switch from spectator to player mode
  switchToPlayer(colorIndex: number): void {
    this.transport.sendReliable({
      type: 'SwitchToPlayer',
      colorIndex,
    });
  }

  // Disconnect and return to menu
  disconnect(): void {
    this.stopSpectatorHeartbeat();
    this.transport.disconnect();
    this.world.reset();
    this.stateSync.reset();
    this.inputSystem.reset();    // Reset state but keep listeners for next game
    this.renderSystem.reset();   // Clear camera and trails for fresh start
    this.setPhase('menu');
    this.stopGameLoop();
  }

  // Full cleanup when game instance is disposed
  destroy(): void {
    this.disconnect();
    this.inputSystem.destroy();
  }

  // Start spectator heartbeat (sends periodic pings to avoid idle kick)
  private startSpectatorHeartbeat(): void {
    this.stopSpectatorHeartbeat(); // Clear any existing
    // Send ping every 15 seconds to stay active (server timeout is 45s)
    this.spectatorHeartbeatInterval = window.setInterval(() => {
      if (this.world.isSpectator) {
        this.transport.sendReliable({
          type: 'Ping',
          timestamp: Date.now(),
        });
      }
    }, 15000);
  }

  // Stop spectator heartbeat
  private stopSpectatorHeartbeat(): void {
    if (this.spectatorHeartbeatInterval !== null) {
      clearInterval(this.spectatorHeartbeatInterval);
      this.spectatorHeartbeatInterval = null;
    }
  }

  // Report viewport zoom to server for entity filtering optimization
  // Server uses this to skip sending entities too small to see at current zoom
  private reportViewportIfChanged(): void {
    const currentZoom = this.renderSystem.getCurrentZoom();
    const delta = Math.abs(currentZoom - this.lastReportedZoom);

    if (delta > this.VIEWPORT_REPORT_THRESHOLD) {
      this.transport.sendReliable({
        type: 'ViewportInfo',
        zoom: currentZoom,
      });
      this.lastReportedZoom = currentZoom;
    }
  }

  // Main game loop
  private loop(currentTime: number): void {
    try {
      const dt = Math.min((currentTime - this.lastTime) / 1000, 0.1);
      this.lastTime = currentTime;

      // Update input and send to server
      if (this.phase === 'playing' || this.phase === 'countdown') {
        this.processInput(dt);
      }

      // Get interpolated state and update world
      const interpolatedState = this.stateSync.getInterpolatedState();
      if (interpolatedState) {
        this.world.updateFromState(interpolatedState);
      }

      // Render
      this.render(dt);

      // Report viewport changes to server for entity filtering
      this.reportViewportIfChanged();

      // Continue loop
      if (this.phase !== 'menu' && this.phase !== 'disconnected') {
        this.animationFrameId = requestAnimationFrame(this.loop.bind(this));
      }
    } catch (error) {
      console.error('Game loop error:', error);
      // Continue the loop even if there's an error
      if (this.phase !== 'menu' && this.phase !== 'disconnected') {
        this.animationFrameId = requestAnimationFrame(this.loop.bind(this));
      }
    }
  }

  private processInput(dt: number): void {
    // Update input system
    this.inputSystem.update(this.world, dt);

    // Create input message
    const input = this.inputSystem.createInput(
      this.inputSequence++,
      this.stateSync.getCurrentTick() + 1
    );

    // Record for prediction
    this.stateSync.recordInput(input);

    // Send to server (unreliable for frequent updates)
    this.transport.sendUnreliable(input);
  }

  private render(deltaTime: number): void {
    // Clear canvas
    this.ctx.fillStyle = '#0a0a1a';
    this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);

    // Render game world
    this.renderSystem.render(
      this.world,
      {
        phase: this.phase,
        matchTime: this.world.getMatchTime(),
        input: {
          aimDirection: this.inputSystem.getAimDirection(),
          chargeRatio: this.inputSystem.getChargeRatio(),
          isCharging: this.inputSystem.isCharging(),
          isBoosting: this.inputSystem.isBoosting(),
        },
        rtt: this.transport.getRtt(),
        connectionState: this.transport.getState(),
      },
      deltaTime
    );
  }

  private handleConnectionStateChange(state: ConnectionState): void {
    if (state === 'disconnected' && this.phase !== 'menu') {
      this.setPhase('disconnected');
    }
  }

  private handleServerMessage(message: ServerMessage): void {

    switch (message.type) {
      case 'JoinAccepted':
        this.handleJoinAccepted(message.playerId, message.isSpectator);
        break;

      case 'JoinRejected':
        this.events.onConnectionError(this.formatRejectionMessage(message.reason));
        this.disconnect();
        break;

      case 'Snapshot':
        this.stateSync.applySnapshot(message.snapshot);
        // Update AI status from snapshot
        this.world.aiStatus = message.snapshot.aiStatus ?? null;
        break;

      case 'Delta':
        this.stateSync.applyDelta(message.delta);
        break;

      case 'Event':
        this.handleGameEvent(message.event);
        break;

      case 'PhaseChange':
        this.handlePhaseChange(message.phase, message.countdown);
        break;

      case 'Kicked':
        this.events.onConnectionError(`Kicked: ${message.reason}`);
        this.disconnect();
        break;

      case 'Pong':
        // RTT is handled in transport
        break;

      case 'SpectatorModeChanged':
        this.world.setSpectatorMode(message.isSpectator);
        // Start/stop heartbeat based on spectator mode
        if (message.isSpectator) {
          this.startSpectatorHeartbeat();
        } else {
          this.stopSpectatorHeartbeat();
        }
        break;
    }
  }

  private handleJoinAccepted(playerId: PlayerId, isSpectator: boolean): void {
    this.world.localPlayerId = playerId;
    this.world.setSpectatorMode(isSpectator);
    this.stateSync.setLocalPlayerId(playerId);

    // Start spectator heartbeat if joining as spectator
    if (isSpectator) {
      this.startSpectatorHeartbeat();
    }

    // Start game loop but stay in connecting phase until first snapshot arrives
    // This prevents flicker from showing game before player data is ready
    this.lastTime = performance.now();
    this.animationFrameId = requestAnimationFrame(this.loop.bind(this));
  }

  private handlePhaseChange(phase: MatchPhase, _countdown: number): void {
    switch (phase) {
      case 'waiting':
        // Server is in waiting phase (lobby) - hide UI and show game view
        this.setPhase('countdown');
        break;
      case 'countdown':
        this.setPhase('countdown');
        break;
      case 'playing':
        this.setPhase('playing');
        break;
      case 'ended':
        this.setPhase('ended');
        break;
    }
  }

  private handleGameEvent(event: GameEvent): void {
    switch (event.type) {
      case 'PlayerKilled':
        this.events.onKillFeed(event.killerName, event.victimName);
        break;

      case 'PlayerJoined':
        this.world.setPlayerName(event.playerId, event.name);
        break;

      case 'PlayerLeft':
        // Player will be removed from next snapshot
        break;

      case 'MatchStarted':
        this.setPhase('playing');
        break;

      case 'MatchEnded':
        this.setPhase('ended');
        break;

      case 'ZoneCollapse':
        this.world.arena.collapsePhase = event.phase;
        this.world.arena.isCollapsing = true;
        break;

      case 'PlayerDeflection': {
        // Get color from one of the players involved
        const playerA = this.world.getPlayer(event.playerA);
        const color = playerA
          ? this.world.getPlayerColor(playerA.colorIndex)
          : '#ffffff';

        // Add collision effect at the collision point
        this.world.addCollisionEffect(event.position, event.intensity, color);

        // Trigger directional screen shake if local player is involved
        if (
          event.playerA === this.world.localPlayerId ||
          event.playerB === this.world.localPlayerId
        ) {
          const localPlayer = this.world.getLocalPlayer();
          if (localPlayer) {
            // Direction from collision point toward player
            const dir = {
              x: localPlayer.position.x - event.position.x,
              y: localPlayer.position.y - event.position.y,
            };
            this.renderSystem.triggerShake(event.intensity, dir);
          } else {
            this.renderSystem.triggerShake(event.intensity);
          }
        } else if (this.world.isSpectator) {
          // Spectator mode: shake when spectate target is involved in collision
          const spectateTarget = this.world.getSpectateTarget();
          if (
            spectateTarget &&
            (event.playerA === this.world.spectateTargetId ||
              event.playerB === this.world.spectateTargetId)
          ) {
            const dir = {
              x: spectateTarget.position.x - event.position.x,
              y: spectateTarget.position.y - event.position.y,
            };
            this.renderSystem.triggerShake(event.intensity, dir);
          }
        }
        break;
      }

      case 'GravityWellCharging': {
        // Add charging effect for warning
        this.world.addChargingWell(event.position, event.wellId);
        break;
      }

      case 'GravityWaveExplosion': {
        // Add expanding wave effect
        this.world.addGravityWaveEffect(event.position, event.strength, event.wellId);

        // Get viewer position for shake calculation
        let viewerPos: { x: number; y: number } | null = null;

        const localPlayer = this.world.getLocalPlayer();
        if (localPlayer && localPlayer.alive) {
          // Regular player mode
          viewerPos = localPlayer.position;
        } else if (this.world.isSpectator) {
          // Spectator mode: use spectate target position
          const spectateTarget = this.world.getSpectateTarget();
          if (spectateTarget) {
            viewerPos = spectateTarget.position;
          } else {
            // Full map view or following a well: use arena center
            const spectateWell = this.world.getSpectateWell();
            viewerPos = spectateWell?.position ?? { x: 0, y: 0 };
          }
        }

        // Trigger screen shake based on distance to viewer
        if (viewerPos) {
          const dx = viewerPos.x - event.position.x;
          const dy = viewerPos.y - event.position.y;
          const dist = Math.sqrt(dx * dx + dy * dy);

          // Scale maxDist with arena size (base 1500 at scale 1.0)
          const maxDist = 1500 * this.world.arena.scale;

          if (dist < maxDist) {
            const falloff = 1 - dist / maxDist;
            // Quadratic falloff for realistic distance attenuation
            const intensity = event.strength * falloff * falloff * 0.8;

            // Only shake if above threshold (prevents micro-shakes)
            if (intensity > 0.15) {
              this.renderSystem.triggerShake(intensity);
            }
          }
        }
        break;
      }

      case 'GravityWellDestroyed': {
        // Mark well as destroyed in StateSync (filters from interpolated state)
        this.stateSync.markWellDestroyed(event.wellId);
        // Also remove from World's arena state immediately
        this.world.removeGravityWell(event.wellId);
        break;
      }
    }
  }

  private handleConnectionError(error: Error): void {
    this.events.onConnectionError(error.message);
    this.setPhase('disconnected');
  }

  private setPhase(phase: GamePhase): void {
    if (this.phase !== phase) {
      this.phase = phase;
      this.events.onPhaseChange(phase);
    }
  }

  private formatRejectionMessage(reason: string): string {
    // Convert server rejection reasons to user-friendly messages
    if (reason.includes('Server at capacity')) {
      return 'Server is full. Please try again in a moment.';
    }
    if (reason.includes('Invalid name') || reason.includes('Name too')) {
      return 'Please enter a valid player name (2-20 characters).';
    }
    if (reason.includes('rate limit') || reason.includes('too many')) {
      return 'Too many connection attempts. Please wait a moment.';
    }
    if (reason.includes('banned') || reason.includes('blocked')) {
      return 'You have been temporarily blocked. Please try again later.';
    }
    if (reason.includes('maintenance') || reason.includes('restarting')) {
      return 'Server is undergoing maintenance. Please try again shortly.';
    }
    // Default: show the server message if it's already user-friendly
    return reason;
  }

  private stopGameLoop(): void {
    if (this.animationFrameId) {
      cancelAnimationFrame(this.animationFrameId);
      this.animationFrameId = 0;
    }
  }

  // Public getters
  getPhase(): GamePhase {
    return this.phase;
  }

  getWorld(): World {
    return this.world;
  }

  getRtt(): number {
    return this.transport.getRtt();
  }
}
