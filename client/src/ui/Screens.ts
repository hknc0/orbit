// UI Screens for menu, end game, and connection states
// Uses safe DOM methods instead of innerHTML

// Player colors (must match Constants.ts PLAYER_COLORS)
const PLAYER_COLORS = [
  '#ef4444', // red
  '#f97316', // orange
  '#f59e0b', // amber
  '#eab308', // yellow
  '#84cc16', // lime
  '#22c55e', // green
  '#10b981', // emerald
  '#14b8a6', // teal
  '#06b6d4', // cyan
  '#0ea5e9', // sky
  '#3b82f6', // blue
  '#6366f1', // indigo
  '#8b5cf6', // violet
  '#a855f7', // purple
  '#d946ef', // fuchsia
  '#ec4899', // pink
  '#f43f5e', // rose
  '#78716c', // stone
  '#64748b', // slate
  '#ffffff', // white
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

    // Animated background stars
    const starsContainer = this.createElement('div', 'stars-container');
    for (let i = 0; i < 50; i++) {
      const star = this.createElement('div', 'star');
      star.style.left = `${Math.random() * 100}%`;
      star.style.top = `${Math.random() * 100}%`;
      star.style.animationDelay = `${Math.random() * 3}s`;
      star.style.animationDuration = `${2 + Math.random() * 3}s`;
      starsContainer.appendChild(star);
    }
    screen.appendChild(starsContainer);

    const container = this.createElement('div', 'menu-container');

    // Logo/Title - minimal and elegant
    const logoSection = this.createElement('div', 'logo-section');
    const logoIcon = this.createElement('div', 'logo-icon');
    // Create orbital rings
    for (let i = 0; i < 3; i++) {
      const ring = this.createElement('div', 'orbit-ring');
      logoIcon.appendChild(ring);
    }
    const logoDot = this.createElement('div', 'logo-dot');
    logoIcon.appendChild(logoDot);
    logoSection.appendChild(logoIcon);

    const titleGroup = this.createElement('div', 'title-group');
    const title = this.createElement('h1', 'game-title', 'ORBIT');
    const titleAccent = this.createElement('span', 'title-accent', 'ROYALE');
    titleGroup.appendChild(title);
    titleGroup.appendChild(titleAccent);
    logoSection.appendChild(titleGroup);

    // Main form area
    const formArea = this.createElement('div', 'form-area');

    // Name input - minimal underline style
    const nameContainer = this.createElement('div', 'name-input-container');
    const nameInput = document.createElement('input');
    nameInput.type = 'text';
    nameInput.id = 'player-name';
    nameInput.className = 'name-input';
    nameInput.placeholder = 'Your name';
    nameInput.maxLength = 16;
    nameInput.autocomplete = 'off';
    nameInput.spellcheck = false;
    try {
      const savedName = localStorage.getItem(STORAGE_KEY_NAME);
      if (savedName) nameInput.value = savedName;
    } catch { /* localStorage not available */ }
    nameContainer.appendChild(nameInput);
    this.playerNameInput = nameInput;

    // Color picker - slider with preview
    const colorContainer = this.createElement('div', 'color-picker-container');

    const colorPreview = this.createElement('div', 'color-preview');
    colorPreview.style.backgroundColor = PLAYER_COLORS[this.selectedColorIndex];

    const sliderWrapper = this.createElement('div', 'slider-wrapper');
    const colorSlider = document.createElement('input');
    colorSlider.type = 'range';
    colorSlider.min = '0';
    colorSlider.max = String(PLAYER_COLORS.length - 1);
    colorSlider.value = String(this.selectedColorIndex);
    colorSlider.className = 'color-slider';
    // Create gradient background with hard color stops (no blending)
    const gradientStops = PLAYER_COLORS.map((c, i) => {
      const start = (i / PLAYER_COLORS.length) * 100;
      const end = ((i + 1) / PLAYER_COLORS.length) * 100;
      return `${c} ${start}%, ${c} ${end}%`;
    }).join(', ');
    colorSlider.style.setProperty('--slider-gradient', `linear-gradient(to right, ${gradientStops})`);

    colorSlider.addEventListener('input', () => {
      const index = parseInt(colorSlider.value, 10);
      this.selectedColorIndex = index;
      colorPreview.style.backgroundColor = PLAYER_COLORS[index];
    });

    sliderWrapper.appendChild(colorSlider);
    colorContainer.appendChild(colorPreview);
    colorContainer.appendChild(sliderWrapper);

    // Play button - clean and prominent
    const playBtn = this.createElement('button', 'btn-play');
    playBtn.id = 'play-btn';
    playBtn.textContent = 'PLAY';

    // Spectate button - secondary style
    const spectateBtn = this.createElement('button', 'btn-spectate');
    spectateBtn.id = 'spectate-btn';
    spectateBtn.textContent = 'SPECTATE';

    formArea.appendChild(nameContainer);
    formArea.appendChild(colorContainer);
    formArea.appendChild(playBtn);
    formArea.appendChild(spectateBtn);

    // Controls section - minimal but complete
    const controlsSection = this.createElement('div', 'controls-section');

    const controls = [
      { keys: ['W', 'A', 'S', 'D'], desc: 'Move' },
      { keys: ['Click', 'W'], desc: 'Boost thrust' },
      { keys: ['Space'], desc: 'Hold to charge, release to eject' },
    ];

    controls.forEach(({ keys, desc }) => {
      const row = this.createElement('div', 'control-row');
      const keysContainer = this.createElement('div', 'control-keys');
      keys.forEach((key, i) => {
        if (i > 0) keysContainer.appendChild(document.createTextNode(' '));
        const kbd = this.createElement('kbd', undefined, key);
        keysContainer.appendChild(kbd);
      });
      const descEl = this.createElement('span', 'control-desc', desc);
      row.appendChild(keysContainer);
      row.appendChild(descEl);
      controlsSection.appendChild(row);
    });

    // Objective
    const objective = this.createElement('div', 'objective-text', 'Be the last one standing');
    controlsSection.appendChild(objective);

    container.appendChild(logoSection);
    container.appendChild(formArea);
    container.appendChild(controlsSection);
    screen.appendChild(container);

    return screen;
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
        background: radial-gradient(ellipse at center, #0f172a 0%, #0a1020 50%, #050810 100%);
        color: #e0e8f0;
        font-family: 'Inter', system-ui, sans-serif;
        z-index: 100;
        overflow: hidden;
      }

      .screen.hidden {
        display: none;
      }

      /* Animated stars background */
      .stars-container {
        position: absolute;
        top: 0;
        left: 0;
        right: 0;
        bottom: 0;
        overflow: hidden;
        pointer-events: none;
      }

      .star {
        position: absolute;
        width: 2px;
        height: 2px;
        background: #fff;
        border-radius: 50%;
        opacity: 0;
        animation: twinkle 3s ease-in-out infinite;
      }

      .star:nth-child(3n) { width: 3px; height: 3px; }
      .star:nth-child(5n) { background: #00ffff; }
      .star:nth-child(7n) { background: #fbbf24; }

      @keyframes twinkle {
        0%, 100% { opacity: 0; transform: scale(0.5); }
        50% { opacity: 0.6; transform: scale(1); }
      }

      /* Menu container */
      .menu-container {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 2rem;
        position: relative;
        z-index: 1;
      }

      .end-container, .connecting-container, .error-container {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 2rem;
        position: relative;
        z-index: 1;
      }

      /* Logo section */
      .logo-section {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 1rem;
      }

      .logo-icon {
        position: relative;
        width: 100px;
        height: 100px;
      }

      .orbit-ring {
        position: absolute;
        top: 50%;
        left: 50%;
        border: 2px solid rgba(0, 255, 255, 0.5);
        border-radius: 50%;
        animation: orbit-spin 20s linear infinite;
        box-shadow: 0 0 8px rgba(0, 255, 255, 0.2);
      }

      .orbit-ring:nth-child(1) {
        width: 50px;
        height: 50px;
        margin: -25px 0 0 -25px;
        border-color: rgba(0, 255, 255, 0.6);
      }

      .orbit-ring:nth-child(2) {
        width: 75px;
        height: 75px;
        margin: -37.5px 0 0 -37.5px;
        animation-duration: 30s;
        animation-direction: reverse;
        border-color: rgba(0, 255, 255, 0.45);
      }

      .orbit-ring:nth-child(3) {
        width: 100px;
        height: 100px;
        margin: -50px 0 0 -50px;
        animation-duration: 45s;
        border-color: rgba(0, 255, 255, 0.3);
      }

      .logo-dot {
        position: absolute;
        top: 50%;
        left: 50%;
        width: 14px;
        height: 14px;
        margin: -7px 0 0 -7px;
        background: #00ffff;
        border-radius: 50%;
        box-shadow: 0 0 20px rgba(0, 255, 255, 0.6), 0 0 40px rgba(0, 255, 255, 0.3);
      }

      @keyframes orbit-spin {
        to { transform: rotate(360deg); }
      }

      .title-group {
        text-align: center;
      }

      .game-title {
        font-family: 'Orbitron', sans-serif;
        font-size: clamp(2rem, 8vw, 3rem);
        font-weight: 700;
        font-style: normal;
        letter-spacing: 0.15em;
        margin: 0;
        color: #00ffff;
        text-shadow: 0 0 30px rgba(0, 255, 255, 0.4);
      }

      .title-accent {
        display: block;
        font-family: 'Orbitron', sans-serif;
        font-size: 0.9rem;
        font-weight: 400;
        font-style: normal;
        letter-spacing: 0.4em;
        color: #00ffff;
        opacity: 0.6;
        margin-top: 0.25rem;
        margin-left: 0.4em;
        text-shadow: 0 0 20px rgba(0, 255, 255, 0.3);
      }

      /* Form area - HUD panel style */
      .form-area {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 1.25rem;
        width: 100%;
        max-width: 320px;
        padding: 1.5rem;
        background: rgba(15, 23, 42, 0.85);
        border: 1px solid rgba(100, 150, 255, 0.15);
        border-radius: 4px;
        box-shadow: 0 0 20px rgba(0, 0, 0, 0.3);
      }

      .name-input-container {
        width: 100%;
      }

      .name-input {
        width: 100%;
        padding: 0.75rem 1rem;
        font-family: 'Inter', sans-serif;
        font-size: 1rem;
        background: rgba(0, 0, 0, 0.3);
        border: 1px solid rgba(100, 150, 255, 0.2);
        border-radius: 3px;
        color: #fff;
        text-align: center;
        outline: none;
        transition: all 0.2s ease;
      }

      .name-input:focus {
        border-color: rgba(100, 150, 255, 0.5);
        box-shadow: 0 0 10px rgba(100, 150, 255, 0.15);
      }

      .name-input::placeholder {
        color: rgba(255, 255, 255, 0.35);
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

      /* Color picker - slider */
      .color-picker-container {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        width: 100%;
      }

      .color-preview {
        width: 32px;
        height: 32px;
        border-radius: 50%;
        border: 2px solid rgba(255, 255, 255, 0.3);
        box-shadow: 0 0 12px currentColor;
        flex-shrink: 0;
        transition: background-color 0.15s ease;
      }

      .slider-wrapper {
        flex: 1;
        display: flex;
        align-items: center;
      }

      .color-slider {
        -webkit-appearance: none;
        appearance: none;
        width: 100%;
        height: 8px;
        background: var(--slider-gradient);
        border-radius: 4px;
        outline: none;
        cursor: pointer;
      }

      .color-slider::-webkit-slider-thumb {
        -webkit-appearance: none;
        appearance: none;
        width: 18px;
        height: 18px;
        background: #fff;
        border-radius: 50%;
        cursor: pointer;
        box-shadow: 0 0 6px rgba(0, 0, 0, 0.4);
        border: 2px solid rgba(255, 255, 255, 0.9);
        transition: transform 0.1s ease;
      }

      .color-slider::-webkit-slider-thumb:hover {
        transform: scale(1.1);
      }

      .color-slider::-moz-range-thumb {
        width: 18px;
        height: 18px;
        background: #fff;
        border-radius: 50%;
        cursor: pointer;
        box-shadow: 0 0 6px rgba(0, 0, 0, 0.4);
        border: 2px solid rgba(255, 255, 255, 0.9);
      }

      /* Play button - HUD style */
      .btn-play {
        width: 100%;
        padding: 0.875rem 2rem;
        font-family: 'Orbitron', sans-serif;
        font-size: 1rem;
        font-weight: 600;
        letter-spacing: 0.2em;
        background: linear-gradient(180deg, rgba(0, 255, 255, 0.15) 0%, rgba(0, 255, 255, 0.05) 100%);
        color: #00ffff;
        border: 1px solid #00ffff;
        border-radius: 3px;
        cursor: pointer;
        transition: all 0.2s ease;
        text-shadow: 0 0 10px rgba(0, 255, 255, 0.5);
        box-shadow: 0 0 15px rgba(0, 255, 255, 0.2), inset 0 0 15px rgba(0, 255, 255, 0.05);
      }

      .btn-play:hover {
        background: linear-gradient(180deg, rgba(0, 255, 255, 0.25) 0%, rgba(0, 255, 255, 0.1) 100%);
        box-shadow: 0 0 25px rgba(0, 255, 255, 0.35), inset 0 0 20px rgba(0, 255, 255, 0.1);
        transform: translateY(-1px);
      }

      .btn-play:active {
        transform: translateY(0);
      }

      /* Spectate button - secondary style */
      .btn-spectate {
        width: 100%;
        padding: 0.6rem 1.5rem;
        font-family: 'Orbitron', sans-serif;
        font-size: 0.75rem;
        font-weight: 500;
        letter-spacing: 0.15em;
        background: transparent;
        color: rgba(160, 180, 200, 0.7);
        border: 1px solid rgba(100, 150, 255, 0.2);
        border-radius: 3px;
        cursor: pointer;
        transition: all 0.2s ease;
      }

      .btn-spectate:hover {
        color: #00ffff;
        border-color: rgba(0, 255, 255, 0.4);
        background: rgba(0, 255, 255, 0.05);
      }

      /* btn-primary for end/error screens */
      .btn-primary {
        padding: 0.875rem 2rem;
        font-family: 'Orbitron', sans-serif;
        font-size: 0.875rem;
        font-weight: 600;
        letter-spacing: 0.15em;
        background: linear-gradient(180deg, rgba(0, 255, 255, 0.15) 0%, rgba(0, 255, 255, 0.05) 100%);
        color: #00ffff;
        border: 1px solid #00ffff;
        border-radius: 3px;
        cursor: pointer;
        transition: all 0.2s ease;
        box-shadow: 0 0 15px rgba(0, 255, 255, 0.2);
      }

      .btn-primary:hover {
        background: linear-gradient(180deg, rgba(0, 255, 255, 0.25) 0%, rgba(0, 255, 255, 0.1) 100%);
        box-shadow: 0 0 25px rgba(0, 255, 255, 0.35);
      }

      .btn-primary:active {
        transform: translateY(0);
      }

      /* Controls section - HUD panel style */
      .controls-section {
        display: flex;
        flex-direction: column;
        gap: 0.6rem;
        padding: 1.25rem 1.5rem;
        background: rgba(15, 23, 42, 0.75);
        border: 1px solid rgba(100, 150, 255, 0.15);
        border-radius: 4px;
        min-width: 300px;
      }

      .control-row {
        display: flex;
        align-items: center;
        gap: 1rem;
      }

      .control-keys {
        display: flex;
        gap: 0.3rem;
        min-width: 110px;
      }

      .control-keys kbd {
        font-family: 'Orbitron', monospace;
        font-size: 0.65rem;
        padding: 0.3rem 0.5rem;
        background: rgba(251, 191, 36, 0.1);
        border: 1px solid rgba(251, 191, 36, 0.3);
        border-radius: 3px;
        color: #fbbf24;
      }

      .control-desc {
        font-size: 0.8rem;
        color: rgba(200, 210, 230, 0.7);
      }

      .objective-text {
        font-family: 'Orbitron', sans-serif;
        font-size: 0.7rem;
        font-weight: 400;
        color: #4ade80;
        text-align: center;
        margin-top: 0.5rem;
        padding-top: 0.75rem;
        border-top: 1px solid rgba(100, 150, 255, 0.1);
        letter-spacing: 0.1em;
        text-shadow: 0 0 10px rgba(74, 222, 128, 0.3);
      }

      /* End screen */
      .end-title {
        font-family: 'Orbitron', sans-serif;
        font-size: clamp(2.5rem, 8vw, 4rem);
        font-weight: 700;
        letter-spacing: 0.1em;
        margin: 0 0 1rem 0;
      }

      .end-title.victory {
        color: #4ade80;
        text-shadow: 0 0 30px rgba(74, 222, 128, 0.5);
      }

      .end-title.defeat {
        color: #ef4444;
        text-shadow: 0 0 30px rgba(239, 68, 68, 0.5);
      }

      .end-stats {
        display: flex;
        gap: 2rem;
        margin-bottom: 1.5rem;
      }

      .stat-box {
        text-align: center;
        min-width: 100px;
        padding: 1rem 1.5rem;
        background: rgba(10, 14, 20, 0.8);
        border: 1px solid rgba(0, 255, 255, 0.2);
        border-radius: 4px;
      }

      .stat-label {
        display: block;
        font-family: 'Orbitron', sans-serif;
        font-size: 0.6rem;
        color: rgba(160, 180, 200, 0.7);
        letter-spacing: 0.15em;
        margin-bottom: 0.5rem;
        text-transform: uppercase;
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

      /* Connecting screen */
      .spinner {
        width: 50px;
        height: 50px;
        border: 2px solid rgba(0, 255, 255, 0.2);
        border-top-color: #00ffff;
        border-radius: 50%;
        animation: spin 0.8s linear infinite;
        box-shadow: 0 0 15px rgba(0, 255, 255, 0.2);
      }

      @keyframes spin {
        to { transform: rotate(360deg); }
      }

      .connecting-text {
        font-family: 'Orbitron', sans-serif;
        font-size: 0.9rem;
        font-weight: 400;
        color: rgba(0, 255, 255, 0.7);
        letter-spacing: 0.15em;
      }

      /* Error screen */
      .error-title {
        font-family: 'Orbitron', sans-serif;
        font-size: 1.5rem;
        font-weight: 600;
        color: #ef4444;
        margin: 0;
        letter-spacing: 0.1em;
        text-shadow: 0 0 20px rgba(239, 68, 68, 0.4);
      }

      .error-message {
        font-size: 0.875rem;
        color: rgba(200, 210, 230, 0.7);
        max-width: 320px;
        text-align: center;
        line-height: 1.6;
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

  onSpectate(callback: () => void): void {
    const btn = this.menuScreen.querySelector('#spectate-btn');
    btn?.addEventListener('click', () => {
      // Spectators don't need a name, but use one if provided
      this.savePreferences();
      callback();
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
