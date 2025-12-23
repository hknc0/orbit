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
  private ejectChargeTime: number = 0;
  private aimDirection: Vec2 = new Vec2(1, 0);

  // Latching flag for eject release - persists until consumed
  private pendingEjectRelease: boolean = false;

  // Keyboard directional input
  private keyDirection: Vec2 = new Vec2();
  private keysHeld: Set<string> = new Set();
  private useKeyboardAim: boolean = false;

  // Store listener references for cleanup
  private listeners: { target: EventTarget; type: string; handler: EventListener }[] = [];

  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;
    this.setupListeners();
  }

  /** Reset input state without removing listeners (for game restart) */
  reset(): void {
    this.isMouseBoostHeld = false;
    this.isKeyBoostHeld = false;
    this.isEjectHeld = false;
    this.pendingEjectRelease = false;
    this.ejectChargeTime = 0;
    this.keysHeld.clear();
    this.keyDirection.set(0, 0);
  }

  private resetInputState(): void {
    this.reset();
  }

  private addListener<K extends keyof WindowEventMap>(
    target: Window,
    type: K,
    handler: (e: WindowEventMap[K]) => void
  ): void;
  private addListener<K extends keyof DocumentEventMap>(
    target: Document,
    type: K,
    handler: (e: DocumentEventMap[K]) => void
  ): void;
  private addListener<K extends keyof HTMLElementEventMap>(
    target: HTMLElement,
    type: K,
    handler: (e: HTMLElementEventMap[K]) => void
  ): void;
  private addListener(target: EventTarget, type: string, handler: EventListener): void {
    target.addEventListener(type, handler);
    this.listeners.push({ target, type, handler });
  }

  private setupListeners(): void {
    // Reset inputs when window loses focus
    this.addListener(window, 'blur', () => this.resetInputState());
    this.addListener(document, 'visibilitychange', () => {
      if (document.hidden) this.resetInputState();
    });

    this.addListener(this.canvas, 'mousemove', (e: MouseEvent) => {
      this.mousePos.set(e.clientX, e.clientY);
      // Switch back to mouse aim when mouse moves significantly
      if (Math.abs(e.movementX) > 3 || Math.abs(e.movementY) > 3) {
        this.useKeyboardAim = false;
      }
    });

    this.addListener(this.canvas, 'mousedown', (e: MouseEvent) => {
      if (e.button === 0) {
        this.isMouseBoostHeld = true;
      }
    });

    this.addListener(this.canvas, 'mouseup', (e: MouseEvent) => {
      if (e.button === 0) {
        this.isMouseBoostHeld = false;
      }
    });

    this.addListener(this.canvas, 'mouseleave', () => {
      this.isMouseBoostHeld = false;
    });

    // Keyboard controls
    this.addListener(window, 'keydown', (e: KeyboardEvent) => {
      if (e.repeat) return;

      switch (e.code) {
        case 'Space':
          e.preventDefault();
          this.isEjectHeld = true;
          break;
        case 'ShiftLeft':
        case 'ShiftRight':
          this.isKeyBoostHeld = true;
          break;
        // WASD directional keys
        case 'KeyW':
        case 'KeyA':
        case 'KeyS':
        case 'KeyD':
        // Arrow keys
        case 'ArrowUp':
        case 'ArrowDown':
        case 'ArrowLeft':
        case 'ArrowRight':
          this.keysHeld.add(e.code);
          this.updateKeyDirection();
          this.useKeyboardAim = true;
          break;
      }
    });

    this.addListener(window, 'keyup', (e: KeyboardEvent) => {
      switch (e.code) {
        case 'Space':
          // Latch the release so it's not missed between ticks
          if (this.isEjectHeld) {
            this.pendingEjectRelease = true;
          }
          this.isEjectHeld = false;
          break;
        case 'ShiftLeft':
        case 'ShiftRight':
          this.isKeyBoostHeld = false;
          break;
        case 'KeyW':
        case 'KeyA':
        case 'KeyS':
        case 'KeyD':
        case 'ArrowUp':
        case 'ArrowDown':
        case 'ArrowLeft':
        case 'ArrowRight':
          this.keysHeld.delete(e.code);
          this.updateKeyDirection();
          break;
      }
    });
  }

  /** Clean up all event listeners to prevent memory leaks */
  destroy(): void {
    for (const { target, type, handler } of this.listeners) {
      target.removeEventListener(type, handler);
    }
    this.listeners = [];
    this.resetInputState();
  }

  private updateKeyDirection(): void {
    let x = 0;
    let y = 0;

    // WASD
    if (this.keysHeld.has('KeyW') || this.keysHeld.has('ArrowUp')) y -= 1;
    if (this.keysHeld.has('KeyS') || this.keysHeld.has('ArrowDown')) y += 1;
    if (this.keysHeld.has('KeyA') || this.keysHeld.has('ArrowLeft')) x -= 1;
    if (this.keysHeld.has('KeyD') || this.keysHeld.has('ArrowRight')) x += 1;

    // Normalize diagonal movement
    const len = Math.sqrt(x * x + y * y);
    if (len > 0) {
      this.keyDirection.set(x / len, y / len);
      this.isKeyBoostHeld = true; // Auto-boost when using directional keys
    } else {
      this.keyDirection.set(0, 0);
      this.isKeyBoostHeld = false;
    }
  }

  private get isBoostHeld(): boolean {
    return this.isMouseBoostHeld || this.isKeyBoostHeld;
  }

  update(world: World, dt: number): void {
    const localPlayer = world.getLocalPlayer();
    if (!localPlayer) return;

    // Calculate aim direction based on input mode
    if (this.useKeyboardAim && this.keyDirection.lengthSq() > 0) {
      // Use keyboard direction
      this.aimDirection.copy(this.keyDirection);
    } else {
      // Use mouse direction
      const centerX = this.canvas.width / 2;
      const centerY = this.canvas.height / 2;

      const dx = this.mousePos.x - centerX;
      const dy = this.mousePos.y - centerY;
      const len = Math.sqrt(dx * dx + dy * dy);

      if (len > 0) {
        this.aimDirection.set(dx / len, dy / len);
      }
    }

    // Track eject charging
    if (this.isEjectHeld) {
      this.ejectChargeTime += dt;
    }
  }

  // Create a player input for sending to server
  createInput(sequence: number, tick: number): PlayerInput {
    const thrust = this.isBoostHeld ? this.aimDirection.clone() : new Vec2(0, 0);

    // Use latched release flag (persists until consumed)
    const fireReleased = this.pendingEjectRelease;

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

    // Consume the release flag after sending
    if (fireReleased) {
      this.pendingEjectRelease = false;
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
