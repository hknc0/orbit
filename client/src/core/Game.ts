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

  }

  // Configure server connection
  setServer(url: string, certHash?: string): void {
    this.serverUrl = url;
    this.certHash = certHash;
  }

  // Start connecting and playing
  async start(playerName: string): Promise<void> {
    this.setPhase('connecting');
    this.inputSequence = 0;

    try {
      await this.transport.connect(this.serverUrl, this.certHash);

      // Send join request
      await this.transport.sendReliable({
        type: 'JoinRequest',
        playerName,
      });
    } catch (err) {
      this.setPhase('disconnected');
      this.events.onConnectionError(err instanceof Error ? err.message : 'Connection failed');
    }
  }

  // Disconnect and return to menu
  disconnect(): void {
    this.transport.disconnect();
    this.world.reset();
    this.stateSync.reset();
    this.setPhase('menu');
    this.stopGameLoop();
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
      this.render();

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

  private render(): void {
    // Clear canvas
    this.ctx.fillStyle = '#0a0a1a';
    this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);

    // Render game world
    this.renderSystem.render(this.world, {
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
    });
  }

  private handleConnectionStateChange(state: ConnectionState): void {
    if (state === 'disconnected' && this.phase !== 'menu') {
      this.setPhase('disconnected');
    }
  }

  private handleServerMessage(message: ServerMessage): void {

    switch (message.type) {
      case 'JoinAccepted':
        this.handleJoinAccepted(message.playerId);
        break;

      case 'JoinRejected':
        this.events.onConnectionError(`Join rejected: ${message.reason}`);
        this.disconnect();
        break;

      case 'Snapshot':
        this.stateSync.applySnapshot(message.snapshot);
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
    }
  }

  private handleJoinAccepted(playerId: PlayerId): void {
    this.world.localPlayerId = playerId;
    this.stateSync.setLocalPlayerId(playerId);

    // Hide UI and show game - server will send PhaseChange to set actual match phase
    // For now, transition to countdown to hide the connecting screen
    this.setPhase('countdown');

    // Start game loop
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
