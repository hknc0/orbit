// Must match server constants from api/src/game/constants.rs

export const PHYSICS = {
  G: 6.67,
  CENTRAL_MASS: 10_000,
  DRAG: 0.002,
  MAX_VELOCITY: 500,
  TICK_RATE: 30,
  DT: 1 / 30,
} as const;

export const MASS = {
  STARTING: 100,
  MINIMUM: 10,
  ABSORPTION_CAP: 200,
  ABSORPTION_RATE: 0.7,
  RADIUS_SCALE: 2.0,
} as const;

export const BOOST = {
  BASE_THRUST: 200,
  BASE_COST: 2,
  MASS_COST_RATIO: 0.01,
  // Speed scaling constants - agar.io style where larger players are slower
  SPEED_REFERENCE_MASS: 100,
  SPEED_SCALING_EXPONENT: 0.5, // sqrt curve
  SPEED_MIN_MULTIPLIER: 0.25,
  SPEED_MAX_MULTIPLIER: 3.5,
} as const;

/**
 * Calculate thrust multiplier based on player mass
 * Returns 1.0 at reference mass (100), higher for smaller (faster), lower for larger (slower)
 * Uses sqrt curve for agar.io style feel
 * @param mass - Player's current mass
 * @returns Thrust multiplier (clamped between 0.25 and 3.5)
 */
export function massToThrustMultiplier(mass: number): number {
  // Use sqrt directly for performance (equivalent to pow(ratio, 0.5))
  const ratio = BOOST.SPEED_REFERENCE_MASS / Math.max(mass, MASS.MINIMUM);
  const multiplier = Math.sqrt(ratio);
  // Clamp to prevent extreme values
  if (multiplier < BOOST.SPEED_MIN_MULTIPLIER) {
    return BOOST.SPEED_MIN_MULTIPLIER;
  }
  if (multiplier > BOOST.SPEED_MAX_MULTIPLIER) {
    return BOOST.SPEED_MAX_MULTIPLIER;
  }
  return multiplier;
}

export const EJECT = {
  MIN_CHARGE_TIME: 0.2,
  MAX_CHARGE_TIME: 1.0,
  MIN_MASS: 10,
  MAX_MASS_RATIO: 0.5,
  MIN_VELOCITY: 100,
  MAX_VELOCITY: 300,
  LIFETIME: 8,
} as const;

export const ARENA = {
  CORE_RADIUS: 50,
  INNER_RADIUS: 200,
  MIDDLE_RADIUS: 400,
  OUTER_RADIUS: 600,
  ESCAPE_RADIUS: 800,
  COLLAPSE_INTERVAL: 30,
  COLLAPSE_PHASES: 8,
  COLLAPSE_DURATION: 3,
} as const;

export const SPAWN = {
  PROTECTION_DURATION: 3,
  ZONE_MIN: 250,
  ZONE_MAX: 350,
} as const;

export const MATCH = {
  DURATION: 300,
  COUNTDOWN: 3,
} as const;

export const NETWORK = {
  INTERPOLATION_DELAY_MS: 100,
  SNAPSHOT_BUFFER_SIZE: 32,
  INPUT_BUFFER_SIZE: 64,
  RECONNECT_ATTEMPTS: 3,
  PING_INTERVAL_MS: 1000,
} as const;

// Player colors matching server (20 colors for smooth gradient selection)
export const PLAYER_COLORS = [
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
] as const;
