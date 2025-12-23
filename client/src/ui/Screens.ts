// UI Screens for menu, end game, and connection states
// Uses safe DOM methods instead of innerHTML

// Player colors (must match Constants.ts PLAYER_COLORS)
const PLAYER_COLORS = [
  '#3b82f6', // blue
  '#ef4444', // red
  '#22c55e', // green
  '#f59e0b', // amber
  '#8b5cf6', // purple
  '#ec4899', // pink
  '#06b6d4', // cyan
  '#f97316', // orange
  '#84cc16', // lime
  '#6366f1', // indigo
];

const STORAGE_KEY_NAME = 'orbit-royale-player-name';
const STORAGE_KEY_COLOR = 'orbit-royale-player-color';

export class Screens {
  private menuScreen: HTMLElement;
  private endScreen: HTMLElement;
  private connectingScreen: HTMLElement;
  private errorScreen: HTMLElement;

  private playerNameInput: HTMLInputElement | null = null;
  private endTitle: HTMLElement | null = null;
  private endPlacement: HTMLElement | null = null;
  private endKills: HTMLElement | null = null;
  private errorMessage: HTMLElement | null = null;

  private selectedColorIndex: number = 0;
  private colorSwatches: HTMLElement[] = [];

  constructor() {
    // Load saved preferences
    this.loadPreferences();

    this.menuScreen = this.createMenuScreen();
    this.endScreen = this.createEndScreen();
    this.connectingScreen = this.createConnectingScreen();
    this.errorScreen = this.createErrorScreen();
  }

  private loadPreferences(): void {
    try {
      const savedColor = localStorage.getItem(STORAGE_KEY_COLOR);
      if (savedColor !== null) {
        const colorIndex = parseInt(savedColor, 10);
        if (!isNaN(colorIndex) && colorIndex >= 0 && colorIndex < PLAYER_COLORS.length) {
          this.selectedColorIndex = colorIndex;
        }
      }
    } catch {
      // localStorage not available
    }
  }

  private savePreferences(): void {
    try {
      if (this.playerNameInput?.value) {
        localStorage.setItem(STORAGE_KEY_NAME, this.playerNameInput.value);
      }
      localStorage.setItem(STORAGE_KEY_COLOR, String(this.selectedColorIndex));
    } catch {
      // localStorage not available
    }
  }

  private createElement<K extends keyof HTMLElementTagNameMap>(
    tag: K,
    className?: string,
    textContent?: string
  ): HTMLElementTagNameMap[K] {
    const el = document.createElement(tag);
    if (className) el.className = className;
    if (textContent) el.textContent = textContent;
    return el;
  }

  private createMenuScreen(): HTMLElement {
    const screen = this.createElement('div', 'screen');
    screen.id = 'menu-screen';

    const container = this.createElement('div', 'menu-container');

    // Title section
    const titleSection = this.createElement('div', 'title-section');
    const title = this.createElement('h1', 'game-title');
    const titleOrbit = this.createElement('span', 'title-orbit', 'ORBIT');
    const titleRoyale = this.createElement('span', 'title-royale', 'ROYALE');
    title.appendChild(titleOrbit);
    title.appendChild(titleRoyale);
    const subtitle = this.createElement('p', 'game-subtitle', 'MULTIPLAYER BATTLE ROYALE');
    titleSection.appendChild(title);
    titleSection.appendChild(subtitle);

    // Name input
    const nameContainer = this.createElement('div', 'name-input-container');
    const nameInput = document.createElement('input');
    nameInput.type = 'text';
    nameInput.id = 'player-name';
    nameInput.className = 'name-input';
    nameInput.placeholder = 'Enter your name';
    nameInput.maxLength = 16;
    // Load saved name
    try {
      const savedName = localStorage.getItem(STORAGE_KEY_NAME);
      if (savedName) nameInput.value = savedName;
    } catch { /* localStorage not available */ }
    nameContainer.appendChild(nameInput);
    this.playerNameInput = nameInput;

    // Color picker
    const colorContainer = this.createElement('div', 'color-picker-container');
    const colorLabel = this.createElement('span', 'color-label', 'COLOR');
    colorContainer.appendChild(colorLabel);
    const colorSwatchesContainer = this.createElement('div', 'color-swatches');
    PLAYER_COLORS.forEach((color, index) => {
      const swatch = this.createElement('div', 'color-swatch');
      swatch.style.backgroundColor = color;
      if (index === this.selectedColorIndex) {
        swatch.classList.add('selected');
      }
      swatch.addEventListener('click', () => this.selectColor(index));
      colorSwatchesContainer.appendChild(swatch);
      this.colorSwatches.push(swatch);
    });
    colorContainer.appendChild(colorSwatchesContainer);

    // Play button
    const playBtn = this.createElement('button', 'btn-primary');
    playBtn.id = 'play-btn';
    const btnText = this.createElement('span', 'btn-text', 'CONNECT');
    const btnIcon = this.createElement('span', 'btn-icon', '\u25B6');
    playBtn.appendChild(btnText);
    playBtn.appendChild(btnIcon);

    // Controls panel
    const controlsPanel = this.createElement('div', 'controls-panel');
    const controlsTitle = this.createElement('div', 'controls-title', 'CONTROLS');
    controlsPanel.appendChild(controlsTitle);

    const controls = [
      { key: 'LMB / W', desc: 'Boost thrust' },
      { key: 'SPACE', desc: 'Hold to charge, release to eject' },
    ];

    controls.forEach(({ key, desc }) => {
      const row = this.createElement('div', 'control-row');
      const keyEl = this.createElement('span', 'control-key', key);
      const descEl = this.createElement('span', 'control-desc', desc);
      row.appendChild(keyEl);
      row.appendChild(descEl);
      controlsPanel.appendChild(row);
    });

    // Objective row
    const objectiveRow = this.createElement('div', 'control-row objective');
    const objectiveKey = this.createElement('span', 'control-key', 'OBJECTIVE');
    const objectiveDesc = this.createElement('span', 'control-desc', 'Be the last one standing');
    objectiveRow.appendChild(objectiveKey);
    objectiveRow.appendChild(objectiveDesc);
    controlsPanel.appendChild(objectiveRow);

    // Version tag
    const versionTag = this.createElement('div', 'version-tag', 'v0.1.0 Multiplayer');

    container.appendChild(titleSection);
    container.appendChild(nameContainer);
    container.appendChild(colorContainer);
    container.appendChild(playBtn);
    container.appendChild(controlsPanel);
    container.appendChild(versionTag);
    screen.appendChild(container);

    return screen;
  }

  private selectColor(index: number): void {
    // Remove selection from previous
    this.colorSwatches[this.selectedColorIndex]?.classList.remove('selected');
    // Add selection to new
    this.selectedColorIndex = index;
    this.colorSwatches[index]?.classList.add('selected');
  }

  private createEndScreen(): HTMLElement {
    const screen = this.createElement('div', 'screen hidden');
    screen.id = 'end-screen';

    const container = this.createElement('div', 'end-container');

    // Title
    const title = this.createElement('h1', 'end-title');
    this.endTitle = title;

    // Stats
    const stats = this.createElement('div', 'end-stats');

    const placementBox = this.createElement('div', 'stat-box');
    const placementLabel = this.createElement('span', 'stat-label', 'FINAL RANK');
    const placementValue = this.createElement('span', 'stat-value end-placement');
    this.endPlacement = placementValue;
    placementBox.appendChild(placementLabel);
    placementBox.appendChild(placementValue);

    const killsBox = this.createElement('div', 'stat-box');
    const killsLabel = this.createElement('span', 'stat-label', 'ELIMINATIONS');
    const killsValue = this.createElement('span', 'stat-value end-kills');
    this.endKills = killsValue;
    killsBox.appendChild(killsLabel);
    killsBox.appendChild(killsValue);

    stats.appendChild(placementBox);
    stats.appendChild(killsBox);

    // Restart button
    const restartBtn = this.createElement('button', 'btn-primary');
    restartBtn.id = 'restart-btn';
    const btnText = this.createElement('span', 'btn-text', 'PLAY AGAIN');
    const btnIcon = this.createElement('span', 'btn-icon', '\u21BB');
    restartBtn.appendChild(btnText);
    restartBtn.appendChild(btnIcon);

    container.appendChild(title);
    container.appendChild(stats);
    container.appendChild(restartBtn);
    screen.appendChild(container);

    return screen;
  }

  private createConnectingScreen(): HTMLElement {
    const screen = this.createElement('div', 'screen hidden');
    screen.id = 'connecting-screen';

    const container = this.createElement('div', 'connecting-container');
    const spinner = this.createElement('div', 'spinner');
    const text = this.createElement('p', 'connecting-text', 'Connecting to server...');

    container.appendChild(spinner);
    container.appendChild(text);
    screen.appendChild(container);

    return screen;
  }

  private createErrorScreen(): HTMLElement {
    const screen = this.createElement('div', 'screen hidden');
    screen.id = 'error-screen';

    const container = this.createElement('div', 'error-container');
    const title = this.createElement('h2', 'error-title', 'Connection Error');
    const message = this.createElement('p', 'error-message');
    this.errorMessage = message;

    const retryBtn = this.createElement('button', 'btn-primary');
    retryBtn.id = 'retry-btn';
    const btnText = this.createElement('span', 'btn-text', 'RETRY');
    const btnIcon = this.createElement('span', 'btn-icon', '\u21BB');
    retryBtn.appendChild(btnText);
    retryBtn.appendChild(btnIcon);

    container.appendChild(title);
    container.appendChild(message);
    container.appendChild(retryBtn);
    screen.appendChild(container);

    return screen;
  }

  mount(): void {
    const style = document.createElement('style');
    style.textContent = `
      .screen {
        position: fixed;
        top: 0;
        left: 0;
        right: 0;
        bottom: 0;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        background: radial-gradient(ellipse at center, #15152a 0%, #0a0a1a 50%, #050510 100%);
        color: #f0f4ff;
        font-family: 'Inter', system-ui, sans-serif;
        z-index: 100;
        overflow: hidden;
      }

      .screen::before {
        content: '';
        position: absolute;
        top: 0;
        left: 0;
        right: 0;
        bottom: 0;
        background:
          repeating-linear-gradient(
            0deg,
            transparent,
            transparent 2px,
            rgba(0, 0, 0, 0.03) 2px,
            rgba(0, 0, 0, 0.03) 4px
          );
        pointer-events: none;
      }

      .screen.hidden {
        display: none;
      }

      .menu-container, .end-container, .connecting-container, .error-container {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 2rem;
        position: relative;
        z-index: 1;
      }

      .title-section {
        text-align: center;
        margin-bottom: 1rem;
      }

      .game-title {
        font-family: 'Orbitron', sans-serif;
        font-size: clamp(2.5rem, 10vw, 4.5rem);
        font-weight: 700;
        margin: 0;
        line-height: 1.1;
        text-shadow:
          0 0 20px rgba(0, 255, 255, 0.5),
          0 0 40px rgba(0, 255, 255, 0.3);
      }

      .title-orbit {
        display: block;
        background: linear-gradient(180deg, #00ffff, #0088ff);
        -webkit-background-clip: text;
        -webkit-text-fill-color: transparent;
        background-clip: text;
      }

      .title-royale {
        display: block;
        background: linear-gradient(180deg, #ff00ff, #8b5cf6);
        -webkit-background-clip: text;
        -webkit-text-fill-color: transparent;
        background-clip: text;
        font-size: clamp(2rem, 8vw, 3.5rem);
        letter-spacing: 0.3em;
        margin-left: 0.3em;
      }

      .game-subtitle {
        font-size: clamp(0.65rem, 2vw, 0.9rem);
        color: #d0d0e8;
        letter-spacing: 0.4em;
        margin-top: 1rem;
        text-transform: uppercase;
        text-shadow: 0 0 10px rgba(208, 208, 232, 0.3);
      }

      .name-input-container {
        width: 100%;
        max-width: 300px;
      }

      .name-input {
        width: 100%;
        padding: 1rem 1.5rem;
        font-family: 'Inter', sans-serif;
        font-size: 1.1rem;
        background: rgba(10, 10, 30, 0.8);
        border: 2px solid rgba(100, 150, 255, 0.3);
        border-radius: 4px;
        color: #f0f4ff;
        text-align: center;
        outline: none;
        transition: border-color 0.3s ease;
      }

      .name-input:focus {
        border-color: #00ffff;
        box-shadow: 0 0 10px rgba(0, 255, 255, 0.3);
      }

      .name-input::placeholder {
        color: #606080;
      }

      .name-input.error {
        border-color: #ef4444;
        animation: shake 0.3s ease;
      }

      .name-input.error::placeholder {
        color: #ef4444;
      }

      @keyframes shake {
        0%, 100% { transform: translateX(0); }
        25% { transform: translateX(-5px); }
        75% { transform: translateX(5px); }
      }

      .color-picker-container {
        display: flex;
        align-items: center;
        gap: 1rem;
      }

      .color-label {
        font-family: 'Orbitron', sans-serif;
        font-size: 0.75rem;
        color: #606080;
        letter-spacing: 0.15em;
      }

      .color-swatches {
        display: flex;
        gap: 0.5rem;
      }

      .color-swatch {
        width: 28px;
        height: 28px;
        border-radius: 50%;
        cursor: pointer;
        border: 2px solid transparent;
        transition: all 0.2s ease;
        box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
      }

      .color-swatch:hover {
        transform: scale(1.15);
        box-shadow: 0 0 12px currentColor;
      }

      .color-swatch.selected {
        border-color: #fff;
        box-shadow: 0 0 15px currentColor, 0 0 5px #fff;
        transform: scale(1.1);
      }

      .btn-primary {
        display: flex;
        align-items: center;
        gap: 1rem;
        padding: 1rem 2.5rem;
        font-family: 'Orbitron', sans-serif;
        font-size: clamp(1rem, 3vw, 1.3rem);
        font-weight: 700;
        letter-spacing: 0.2em;
        background: linear-gradient(135deg, rgba(0, 255, 255, 0.15) 0%, rgba(139, 92, 246, 0.15) 100%);
        color: #00ffff;
        border: 2px solid #00ffff;
        border-radius: 4px;
        cursor: pointer;
        transition: all 0.3s ease;
        position: relative;
        overflow: hidden;
        box-shadow:
          0 0 20px rgba(0, 255, 255, 0.3),
          0 0 40px rgba(0, 255, 255, 0.15),
          inset 0 0 15px rgba(0, 255, 255, 0.1);
      }

      .btn-primary:hover {
        background: linear-gradient(135deg, rgba(0, 255, 255, 0.2) 0%, rgba(139, 92, 246, 0.2) 100%);
        transform: scale(1.02);
      }

      .btn-primary:active {
        transform: scale(0.98);
      }

      .btn-primary:disabled {
        opacity: 0.5;
        cursor: not-allowed;
        transform: none;
      }

      .controls-panel {
        background: rgba(10, 10, 30, 0.8);
        border: 1px solid rgba(100, 150, 255, 0.3);
        border-radius: 4px;
        padding: clamp(1rem, 3vw, 1.5rem) clamp(1rem, 4vw, 2rem);
        min-width: min(320px, 85vw);
        max-width: 90vw;
      }

      .controls-title {
        font-family: 'Orbitron', sans-serif;
        font-size: 0.75rem;
        color: #a0a0c0;
        letter-spacing: 0.3em;
        margin-bottom: 1rem;
        text-align: center;
      }

      .control-row {
        display: grid;
        grid-template-columns: auto 1fr;
        gap: 1rem;
        align-items: center;
        padding: 0.6rem 0;
        border-bottom: 1px solid rgba(100, 150, 255, 0.1);
      }

      .control-row:last-child {
        border-bottom: none;
      }

      .control-row.objective {
        margin-top: 0.5rem;
        padding-top: 1rem;
        border-top: 1px solid rgba(100, 150, 255, 0.2);
        border-bottom: none;
      }

      .control-key {
        font-family: 'Orbitron', monospace;
        font-size: 0.75rem;
        color: #fbbf24;
        background: rgba(251, 191, 36, 0.1);
        padding: 0.3rem 0.6rem;
        border-radius: 3px;
        border: 1px solid rgba(251, 191, 36, 0.3);
        min-width: 90px;
        text-align: center;
      }

      .control-desc {
        font-size: 0.85rem;
        color: #c8c8e0;
        text-align: left;
      }

      .version-tag {
        font-family: 'Orbitron', monospace;
        font-size: 0.7rem;
        color: #606080;
        letter-spacing: 0.2em;
        position: fixed;
        bottom: 1rem;
        right: 1rem;
        z-index: 101;
      }

      .end-title {
        font-family: 'Orbitron', sans-serif;
        font-size: 4rem;
        font-weight: 700;
        margin: 0 0 1rem 0;
        text-shadow: 0 0 30px currentColor;
      }

      .end-title.victory {
        color: #4ade80;
      }

      .end-title.defeat {
        color: #ef4444;
      }

      .end-stats {
        display: flex;
        gap: 2rem;
        margin-bottom: 1rem;
      }

      .stat-box {
        background: rgba(10, 10, 30, 0.8);
        border: 1px solid rgba(100, 150, 255, 0.3);
        border-radius: 4px;
        padding: 1.5rem 2rem;
        text-align: center;
        min-width: 140px;
      }

      .stat-label {
        display: block;
        font-size: 0.7rem;
        color: #a0a0c0;
        letter-spacing: 0.2em;
        margin-bottom: 0.5rem;
      }

      .stat-value {
        display: block;
        font-family: 'Orbitron', monospace;
        font-size: 2rem;
        font-weight: 700;
        color: #00ffff;
      }

      .stat-value.end-kills {
        color: #fbbf24;
      }

      .spinner {
        width: 50px;
        height: 50px;
        border: 3px solid rgba(0, 255, 255, 0.3);
        border-top-color: #00ffff;
        border-radius: 50%;
        animation: spin 1s linear infinite;
      }

      @keyframes spin {
        to { transform: rotate(360deg); }
      }

      .connecting-text {
        font-size: 1.2rem;
        color: #a0a0c0;
      }

      .error-title {
        font-family: 'Orbitron', sans-serif;
        font-size: 2rem;
        color: #ef4444;
        margin: 0;
      }

      .error-message {
        font-size: 1rem;
        color: #a0a0c0;
        max-width: 400px;
        text-align: center;
      }
    `;
    document.head.appendChild(style);
    document.body.appendChild(this.menuScreen);
    document.body.appendChild(this.endScreen);
    document.body.appendChild(this.connectingScreen);
    document.body.appendChild(this.errorScreen);
  }

  getPlayerName(): string {
    // Sanitize: trim, remove control characters, limit length
    const raw = this.playerNameInput?.value || '';
    return this.sanitizeName(raw);
  }

  private sanitizeName(name: string): string {
    return name
      .trim()
      // Remove control characters and null bytes
      .replace(/[\x00-\x1F\x7F]/g, '')
      // Remove HTML-like tags
      .replace(/<[^>]*>/g, '')
      // Collapse multiple spaces
      .replace(/\s+/g, ' ')
      // Limit length
      .slice(0, 16);
  }

  getSelectedColor(): number {
    // Ensure color index is within valid range
    return Math.max(0, Math.min(this.selectedColorIndex, PLAYER_COLORS.length - 1));
  }

  private validateName(): boolean {
    const name = this.getPlayerName();
    if (!name || name.length < 1) {
      this.playerNameInput?.classList.add('error');
      this.playerNameInput?.setAttribute('placeholder', 'Name required!');
      this.playerNameInput?.focus();
      // Remove error state when user starts typing
      const removeError = () => {
        this.playerNameInput?.classList.remove('error');
        this.playerNameInput?.setAttribute('placeholder', 'Enter your name');
        this.playerNameInput?.removeEventListener('input', removeError);
      };
      this.playerNameInput?.addEventListener('input', removeError);
      return false;
    }
    // Save preferences when valid
    this.savePreferences();
    return true;
  }

  showMenu(): void {
    this.hideAll();
    this.menuScreen.classList.remove('hidden');
  }

  hideMenu(): void {
    this.menuScreen.classList.add('hidden');
  }

  showConnecting(): void {
    this.hideAll();
    this.connectingScreen.classList.remove('hidden');
  }

  hideConnecting(): void {
    this.connectingScreen.classList.add('hidden');
  }

  showEnd(isVictory: boolean, placement: number, kills: number): void {
    this.hideAll();

    if (this.endTitle) {
      this.endTitle.textContent = isVictory ? 'VICTORY' : 'DEFEATED';
      this.endTitle.className = `end-title ${isVictory ? 'victory' : 'defeat'}`;
    }
    if (this.endPlacement) {
      this.endPlacement.textContent = `#${placement}`;
    }
    if (this.endKills) {
      this.endKills.textContent = kills.toString();
    }

    this.endScreen.classList.remove('hidden');
  }

  hideEnd(): void {
    this.endScreen.classList.add('hidden');
  }

  showError(message: string): void {
    this.hideAll();

    if (this.errorMessage) {
      this.errorMessage.textContent = message;
    }

    this.errorScreen.classList.remove('hidden');
  }

  hideError(): void {
    this.errorScreen.classList.add('hidden');
  }

  hideAll(): void {
    this.menuScreen.classList.add('hidden');
    this.endScreen.classList.add('hidden');
    this.connectingScreen.classList.add('hidden');
    this.errorScreen.classList.add('hidden');
  }

  onPlay(callback: () => void): void {
    const btn = this.menuScreen.querySelector('#play-btn');
    btn?.addEventListener('click', () => {
      if (this.validateName()) {
        callback();
      }
    });

    this.playerNameInput?.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && this.validateName()) {
        callback();
      }
    });
  }

  onRestart(callback: () => void): void {
    const btn = this.endScreen.querySelector('#restart-btn');
    btn?.addEventListener('click', callback);
  }

  onRetry(callback: () => void): void {
    const btn = this.errorScreen.querySelector('#retry-btn');
    btn?.addEventListener('click', callback);
  }
}
