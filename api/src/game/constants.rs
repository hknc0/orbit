/// Physics constants - CRITICAL: DRAG = 0.002 (exponential decay), NOT 0.98!
pub mod physics {
    /// Gravitational constant
    pub const G: f32 = 6.67;
    /// Mass of the central body (black hole/star)
    pub const CENTRAL_MASS: f32 = 10_000.0;
    /// Drag coefficient (exponential decay per tick)
    /// Applied as: velocity *= (1.0 - DRAG)
    pub const DRAG: f32 = 0.002;
    /// Maximum velocity magnitude
    pub const MAX_VELOCITY: f32 = 500.0;
    /// Server tick rate in Hz
    pub const TICK_RATE: u32 = 30;
    /// Delta time per tick in seconds
    pub const DT: f32 = 1.0 / 30.0;
    /// Tick duration in milliseconds
    pub const TICK_DURATION_MS: u64 = 1000 / TICK_RATE as u64;
}

/// Mass-related constants
pub mod mass {
    /// Starting mass for players
    pub const STARTING: f32 = 100.0;
    /// Minimum mass before death
    pub const MINIMUM: f32 = 10.0;
    /// Maximum mass that can be absorbed in one collision
    pub const ABSORPTION_CAP: f32 = 200.0;
    /// Percentage of victim's mass absorbed on kill
    pub const ABSORPTION_RATE: f32 = 0.7;
    /// Radius scaling factor: radius = sqrt(mass) * RADIUS_SCALE
    pub const RADIUS_SCALE: f32 = 2.0;
}

/// Boost/thrust constants
pub mod boost {
    /// Base thrust force
    pub const BASE_THRUST: f32 = 200.0;
    /// Base mass cost per tick of boosting
    pub const BASE_COST: f32 = 2.0;
    /// Mass-proportional cost multiplier
    pub const MASS_COST_RATIO: f32 = 0.01;
}

/// Mass ejection (projectile) constants
pub mod eject {
    /// Minimum charge time in seconds
    pub const MIN_CHARGE_TIME: f32 = 0.2;
    /// Maximum charge time in seconds
    pub const MAX_CHARGE_TIME: f32 = 1.0;
    /// Minimum mass that can be ejected
    pub const MIN_MASS: f32 = 10.0;
    /// Maximum mass as ratio of player's current mass
    pub const MAX_MASS_RATIO: f32 = 0.5;
    /// Minimum ejection velocity
    pub const MIN_VELOCITY: f32 = 100.0;
    /// Maximum ejection velocity
    pub const MAX_VELOCITY: f32 = 300.0;
    /// Projectile lifetime in seconds
    pub const LIFETIME: f32 = 8.0;
}

/// Collision resolution constants
pub mod collision {
    /// Momentum ratio for overwhelming victory (clean kill)
    pub const OVERWHELM_THRESHOLD: f32 = 2.0;
    /// Momentum ratio for decisive victory (kill with cost)
    pub const DECISIVE_THRESHOLD: f32 = 1.5;
    /// Coefficient of restitution for elastic collisions
    pub const RESTITUTION: f32 = 0.8;
}

/// Arena/zone constants
pub mod arena {
    /// Radius of the central danger zone (instant death)
    pub const CORE_RADIUS: f32 = 50.0;
    /// Inner safe zone radius
    pub const INNER_RADIUS: f32 = 200.0;
    /// Middle zone radius
    pub const MIDDLE_RADIUS: f32 = 400.0;
    /// Outer zone radius
    pub const OUTER_RADIUS: f32 = 600.0;
    /// Escape radius (beyond this, mass drains)
    pub const ESCAPE_RADIUS: f32 = 800.0;
    /// Time between zone collapses in seconds
    pub const COLLAPSE_INTERVAL: f32 = 30.0;
    /// Number of collapse phases
    pub const COLLAPSE_PHASES: u8 = 8;
    /// Mass drain rate when outside escape radius
    pub const ESCAPE_MASS_DRAIN: f32 = 10.0;
}

/// Spawn constants
pub mod spawn {
    /// Duration of spawn protection in seconds
    pub const PROTECTION_DURATION: f32 = 3.0;
    /// Minimum spawn distance from center
    pub const ZONE_MIN: f32 = 250.0;
    /// Maximum spawn distance from center
    pub const ZONE_MAX: f32 = 350.0;
    /// Initial spawn velocity (fixed, like orbit-poc)
    pub const INITIAL_VELOCITY: f32 = 50.0;
    /// Minimum distance from other players when spawning
    pub const SAFE_DISTANCE: f32 = 80.0;
    /// Maximum attempts to find safe spawn position
    pub const MAX_SPAWN_ATTEMPTS: u32 = 10;
    /// Delay before respawning after death (seconds)
    pub const RESPAWN_DELAY: f32 = 2.0;
}

/// AI bot constants
pub mod ai {
    /// Number of AI bots to fill the game
    pub const COUNT: usize = 9;
    /// Time between AI decision updates in seconds
    pub const DECISION_INTERVAL: f32 = 0.5;
    /// Distance at which AI becomes aggressive
    pub const AGGRESSION_RADIUS: f32 = 200.0;
    /// Mass ratio at which AI flees instead of fights
    pub const FLEE_MASS_RATIO: f32 = 0.5;
}

/// Game/match constants
pub mod game {
    /// Maximum match duration in seconds
    pub const MATCH_DURATION: f32 = 300.0;
    /// Countdown time before match starts
    pub const COUNTDOWN: f32 = 3.0;
    /// Minimum players to start (including bots)
    pub const MIN_PLAYERS: usize = 2;
}

/// Networking constants
pub mod net {
    /// Maximum reliable message size
    pub const MAX_MESSAGE_SIZE: usize = 65536;
    /// Maximum datagram (unreliable) size
    pub const MAX_DATAGRAM_SIZE: usize = 1200;
    /// Snapshot send rate (can be lower than tick rate)
    pub const SNAPSHOT_RATE: u32 = 20;
    /// Input buffer size (ticks)
    pub const INPUT_BUFFER_SIZE: usize = 10;
}

/// Debris (collectible particle) spawning constants
/// Debris spawns randomly across zones for players to collect and gain mass
/// All values can be overridden via DEBRIS_* environment variables
pub mod debris_spawning {
    /// Master switch to enable/disable debris spawning
    /// When false, no debris will spawn (useful for testing)
    /// ENV: DEBRIS_SPAWN_ENABLED (true/false)
    pub const ENABLED: bool = true;

    /// Maximum number of debris particles in the game at once
    /// Higher values = more crowded, but impacts performance
    /// ENV: DEBRIS_MAX_COUNT
    pub const MAX_COUNT: usize = 200;

    /// Initial debris count spawned at game start per zone
    /// ENV: DEBRIS_INITIAL_INNER, DEBRIS_INITIAL_MIDDLE, DEBRIS_INITIAL_OUTER
    pub const INITIAL_INNER: usize = 50;
    pub const INITIAL_MIDDLE: usize = 40;
    pub const INITIAL_OUTER: usize = 30;

    /// Spawn rates per second per zone for small debris
    /// Inner zone (near gravity wells) has highest spawn rate
    /// ENV: DEBRIS_SPAWN_RATE_INNER_SMALL, etc.
    pub const SPAWN_RATE_INNER_SMALL: f32 = 2.0;
    pub const SPAWN_RATE_MIDDLE_SMALL: f32 = 1.0;
    pub const SPAWN_RATE_OUTER_SMALL: f32 = 0.5;

    /// Spawn rates per second per zone for medium debris
    /// Medium debris is rarer but worth more mass
    /// ENV: DEBRIS_SPAWN_RATE_INNER_MEDIUM, etc.
    pub const SPAWN_RATE_INNER_MEDIUM: f32 = 0.5;
    pub const SPAWN_RATE_MIDDLE_MEDIUM: f32 = 0.3;
    pub const SPAWN_RATE_OUTER_MEDIUM: f32 = 0.1;

    /// Spawn rates per second per zone for large debris
    /// Large debris is rare but very valuable
    /// ENV: DEBRIS_SPAWN_RATE_INNER_LARGE, etc.
    pub const SPAWN_RATE_INNER_LARGE: f32 = 0.1;
    pub const SPAWN_RATE_MIDDLE_LARGE: f32 = 0.05;
    pub const SPAWN_RATE_OUTER_LARGE: f32 = 0.02;

    /// Size distribution weights when spawning (for random selection)
    /// Higher weight = more likely to spawn that size
    pub const WEIGHT_SMALL: f32 = 0.7;
    pub const WEIGHT_MEDIUM: f32 = 0.25;
    pub const WEIGHT_LARGE: f32 = 0.05;

    /// Initial orbital velocity range for spawned debris (units/second)
    /// Debris gets a small orbital velocity to make it orbit naturally
    pub const ORBITAL_VELOCITY_MIN: f32 = 10.0;
    pub const ORBITAL_VELOCITY_MAX: f32 = 30.0;

    /// Debris lifetime in seconds before it decays and disappears
    /// Keeps the arena fresh - old uncollected debris fades away
    /// ENV: DEBRIS_LIFETIME
    pub const LIFETIME: f32 = 90.0;
}

/// Gravity wave explosion constants
/// Wells randomly explode creating expanding shockwaves that push players outward
/// All values can be overridden via GRAVITY_WAVE_* environment variables
pub mod gravity_waves {
    /// Master switch to enable/disable gravity wave explosions
    /// When false, wells never explode (useful for testing or gameplay variety)
    /// ENV: GRAVITY_WAVE_ENABLED (true/false)
    pub const ENABLED: bool = true;

    /// Wave expansion speed in units per second
    /// Higher = faster expanding rings, less time to react
    /// Lower = slower, more time to reposition
    /// ENV: GRAVITY_WAVE_SPEED
    pub const WAVE_SPEED: f32 = 300.0;

    /// Thickness of the wave front in units
    /// This is the "band" where players get pushed
    /// Thicker = easier to get caught, longer push duration
    /// ENV: GRAVITY_WAVE_FRONT_THICKNESS
    pub const WAVE_FRONT_THICKNESS: f32 = 80.0;

    /// Base impulse force applied to players when wave passes
    /// This is added to player velocity (instant push)
    /// Higher = more dramatic knockback
    /// ENV: GRAVITY_WAVE_BASE_IMPULSE
    pub const WAVE_BASE_IMPULSE: f32 = 180.0;

    /// Maximum radius before wave despawns
    /// Waves expand until they reach this radius then disappear
    /// Should be large enough to affect nearby players
    /// ENV: GRAVITY_WAVE_MAX_RADIUS
    pub const WAVE_MAX_RADIUS: f32 = 2000.0;

    /// Duration of the charging/warning phase in seconds
    /// Players see pulsing glow for this long before explosion
    /// Longer = more time to escape, shorter = more surprise
    /// ENV: GRAVITY_WAVE_CHARGE_DURATION
    pub const CHARGE_DURATION: f32 = 2.0;

    /// Minimum time between explosions per well (seconds)
    /// Random delay is chosen between MIN and MAX
    /// ENV: GRAVITY_WAVE_MIN_DELAY
    pub const MIN_EXPLOSION_DELAY: f32 = 30.0;

    /// Maximum time between explosions per well (seconds)
    /// Longer range = more unpredictable timing
    /// ENV: GRAVITY_WAVE_MAX_DELAY
    pub const MAX_EXPLOSION_DELAY: f32 = 90.0;
}

/// Calculate radius from mass
#[inline]
pub fn mass_to_radius(mass: f32) -> f32 {
    mass.sqrt() * mass::RADIUS_SCALE
}

/// Calculate mass from radius
#[inline]
pub fn radius_to_mass(radius: f32) -> f32 {
    (radius / mass::RADIUS_SCALE).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mass_to_radius() {
        let mass = 100.0;
        let radius = mass_to_radius(mass);
        assert!((radius - 20.0).abs() < 0.001); // sqrt(100) * 2 = 20
    }

    #[test]
    fn test_radius_to_mass() {
        let radius = 20.0;
        let mass = radius_to_mass(radius);
        assert!((mass - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_mass_radius_roundtrip() {
        let original_mass = 150.0;
        let radius = mass_to_radius(original_mass);
        let recovered_mass = radius_to_mass(radius);
        assert!((original_mass - recovered_mass).abs() < 0.001);
    }

    #[test]
    fn test_drag_is_exponential_decay() {
        // Verify DRAG is small (exponential decay coefficient)
        assert!(physics::DRAG < 0.1);
        assert!(physics::DRAG > 0.0);
        // After 1 tick: velocity * (1 - 0.002) = velocity * 0.998
        let drag_factor = 1.0 - physics::DRAG;
        assert!((drag_factor - 0.998).abs() < 0.0001);
    }

    #[test]
    fn test_tick_rate() {
        assert_eq!(physics::TICK_RATE, 30);
        assert!((physics::DT - 1.0 / 30.0).abs() < 0.0001);
    }

    #[test]
    fn test_collision_thresholds_ordering() {
        assert!(collision::OVERWHELM_THRESHOLD > collision::DECISIVE_THRESHOLD);
        assert!(collision::DECISIVE_THRESHOLD > 1.0);
    }

    #[test]
    fn test_arena_radii_ordering() {
        assert!(arena::CORE_RADIUS < arena::INNER_RADIUS);
        assert!(arena::INNER_RADIUS < arena::MIDDLE_RADIUS);
        assert!(arena::MIDDLE_RADIUS < arena::OUTER_RADIUS);
        assert!(arena::OUTER_RADIUS < arena::ESCAPE_RADIUS);
    }

    #[test]
    fn test_spawn_zone_in_safe_area() {
        // Spawn zone should be between inner and middle radius
        assert!(spawn::ZONE_MIN > arena::INNER_RADIUS);
        assert!(spawn::ZONE_MAX < arena::MIDDLE_RADIUS);
    }
}
