// Input system for capturing player input and packaging for network

import { Vec2 } from '@/utils/Vec2';
import { EJECT } from '@/utils/Constants';
import type { World } from '@/core/World';
import type { PlayerInput } from '@/net/Protocol';

export class InputSystem {
  private canvas: HTMLCanvasElement;
  private mousePos: Vec2 = new Vec2();
  private isMouseBoostHeld: boolean = false;
  private isKeyBoostHeld: boolean = false;
  private isEjectHeld: boolean = false;
  private wasEjectHeld: boolean = false;
  private ejectChargeTime: number = 0;
  private aimDirection: Vec2 = new Vec2(1, 0);

  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;
    this.setupListeners();
  }

  private resetInputState(): void {
    this.isMouseBoostHeld = false;
    this.isKeyBoostHeld = false;
    this.isEjectHeld = false;
  }

  private setupListeners(): void {
    // Reset inputs when window loses focus
    window.addEventListener('blur', () => this.resetInputState());
    document.addEventListener('visibilitychange', () => {
      if (document.hidden) this.resetInputState();
    });

    this.canvas.addEventListener('mousemove', (e) => {
      this.mousePos.set(e.clientX, e.clientY);
    });

    this.canvas.addEventListener('mousedown', (e) => {
      if (e.button === 0) {
        this.isMouseBoostHeld = true;
      }
    });

    this.canvas.addEventListener('mouseup', (e) => {
      if (e.button === 0) {
        this.isMouseBoostHeld = false;
      }
    });

    this.canvas.addEventListener('mouseleave', () => {
      this.isMouseBoostHeld = false;
    });

    // Keyboard controls
    window.addEventListener('keydown', (e) => {
      if (e.repeat) return;

      switch (e.code) {
        case 'Space':
          e.preventDefault();
          this.isEjectHeld = true;
          break;
        case 'KeyW':
          this.isKeyBoostHeld = true;
          break;
      }
    });

    window.addEventListener('keyup', (e) => {
      switch (e.code) {
        case 'Space':
          this.isEjectHeld = false;
          break;
        case 'KeyW':
          this.isKeyBoostHeld = false;
          break;
      }
    });
  }

  private get isBoostHeld(): boolean {
    return this.isMouseBoostHeld || this.isKeyBoostHeld;
  }

  update(world: World, dt: number): void {
    const localPlayer = world.getLocalPlayer();
    if (!localPlayer) return;

    // Calculate aim direction from mouse position
    const centerX = this.canvas.width / 2;
    const centerY = this.canvas.height / 2;

    // Mouse position relative to screen center (where player is rendered)
    const dx = this.mousePos.x - centerX;
    const dy = this.mousePos.y - centerY;
    const len = Math.sqrt(dx * dx + dy * dy);

    if (len > 0) {
      this.aimDirection.set(dx / len, dy / len);
    }

    // Track eject charging
    if (this.isEjectHeld) {
      this.ejectChargeTime += dt;
    }

    // NOTE: wasEjectHeld is updated in createInput() AFTER checking for release
  }

  // Create a player input for sending to server
  createInput(sequence: number, tick: number): PlayerInput {
    const thrust = this.isBoostHeld ? this.aimDirection.clone() : new Vec2(0, 0);

    // Detect fire release (space was held last frame, not held now)
    const fireReleased = !this.isEjectHeld && this.wasEjectHeld;

    // Build input
    const input: PlayerInput = {
      sequence,
      tick,
      thrust,
      aim: this.aimDirection.clone(),
      boost: this.isBoostHeld,
      fire: this.isEjectHeld,
      fireReleased,
    };

    // Update previous state AFTER creating input
    this.wasEjectHeld = this.isEjectHeld;

    // Reset charge time on release
    if (fireReleased) {
      this.ejectChargeTime = 0;
    }

    return input;
  }

  getAimDirection(): Vec2 {
    return this.aimDirection;
  }

  getChargeRatio(): number {
    if (!this.isEjectHeld) return 0;
    return Math.min(this.ejectChargeTime, EJECT.MAX_CHARGE_TIME) / EJECT.MAX_CHARGE_TIME;
  }

  isCharging(): boolean {
    return this.isEjectHeld;
  }

  isBoosting(): boolean {
    return this.isBoostHeld;
  }
}
