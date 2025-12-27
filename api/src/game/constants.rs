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

    // Speed scaling constants - agar.io style where larger players are slower
    /// Reference mass for speed scaling (at this mass, thrust multiplier = 1.0)
    pub const SPEED_REFERENCE_MASS: f32 = 100.0;
    /// Exponent for speed scaling curve (0.5 = sqrt, agar.io style)
    /// Higher = steeper penalty for large mass, 0.0 = no scaling
    /// Note: Currently hardcoded to use sqrt() for performance, this constant is for documentation
    #[allow(dead_code)]
    pub const SPEED_SCALING_EXPONENT: f32 = 0.5;
    /// Minimum thrust multiplier (prevents huge players from being immobile)
    pub const SPEED_MIN_MULTIPLIER: f32 = 0.25;
    /// Maximum thrust multiplier (prevents tiny players from being too fast)
    pub const SPEED_MAX_MULTIPLIER: f32 = 3.5;
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
    /// Maximum spawn distance from center (increased from 350 for more spawn area)
    pub const ZONE_MAX: f32 = 500.0;
    /// Initial spawn velocity (fixed, like orbit-poc)
    pub const INITIAL_VELOCITY: f32 = 50.0;
    /// Minimum distance from other players when spawning
    pub const SAFE_DISTANCE: f32 = 80.0;
    /// Maximum attempts to find safe spawn position (increased from 10 for high bot counts)
    pub const MAX_SPAWN_ATTEMPTS: u32 = 30;
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
#[allow(dead_code)] // Constants for client reference and future use
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
#[allow(dead_code)] // Constants for configuration reference
pub mod debris_spawning {
    /// Master switch to enable/disable debris spawning
    /// When false, no debris will spawn (useful for testing)
    /// ENV: DEBRIS_SPAWN_ENABLED (true/false)
    pub const ENABLED: bool = true;

    /// Maximum number of debris particles in the game at once
    /// Now optimized with spatial hashing for O(n) collision detection
    /// Higher values = more crowded but still performant
    /// ENV: DEBRIS_MAX_COUNT
    pub const MAX_COUNT: usize = 500;

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

    /// DEPRECATED: Use ArenaScalingConfig.wells_per_area instead
    /// Kept for backwards compatibility with legacy update_for_player_count method
    /// Number of players required per orbital gravity well (player-based scaling)
    #[deprecated(since = "0.2.0", note = "Use area-based well scaling via ArenaScalingConfig.wells_per_area")]
    #[allow(dead_code)]
    pub const BASE_PLAYERS_PER_WELL: usize = 50;

    /// DEPRECATED: Use ArenaScalingConfig.min_wells instead (no max limit in new system)
    /// Kept for backwards compatibility with legacy update_for_player_count method
    #[deprecated(since = "0.2.0", note = "Use area-based well scaling - no hard cap in new system")]
    #[allow(dead_code)]
    pub const MAX_ORBITAL_WELLS: usize = 20;

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

    /// Maximum number of wells that can be charging (pre-explosion warning) at once
    /// Prevents visual chaos and performance issues from many simultaneous explosions
    /// Wells waiting to charge will queue until a slot opens
    /// ENV: GRAVITY_WAVE_MAX_CONCURRENT_CHARGING
    pub const MAX_CONCURRENT_CHARGING: usize = 3;
}

/// Calculate radius from mass
#[inline]
pub fn mass_to_radius(mass: f32) -> f32 {
    mass.sqrt() * mass::RADIUS_SCALE
}

/// Calculate mass from radius (inverse of mass_to_radius)
/// Available for spawning entities with specific visual sizes
#[inline]
#[allow(dead_code)]
pub fn radius_to_mass(radius: f32) -> f32 {
    (radius / mass::RADIUS_SCALE).powi(2)
}

/// Calculate thrust multiplier based on player mass
/// Returns 1.0 at reference mass (100), higher for smaller mass (faster), lower for larger mass (slower)
/// Uses sqrt curve for agar.io style feel: multiplier = sqrt(100/mass)
///
/// Performance: Uses fast sqrt instead of powf, inlined for hot path
#[inline]
pub fn mass_to_thrust_multiplier(mass: f32) -> f32 {
    // Use sqrt directly instead of powf(0.5) for better performance
    // sqrt(reference/mass) = sqrt(100/mass)
    let ratio = boost::SPEED_REFERENCE_MASS / mass.max(mass::MINIMUM);
    let multiplier = ratio.sqrt();
    // Clamp to prevent extreme values
    if multiplier < boost::SPEED_MIN_MULTIPLIER {
        boost::SPEED_MIN_MULTIPLIER
    } else if multiplier > boost::SPEED_MAX_MULTIPLIER {
        boost::SPEED_MAX_MULTIPLIER
    } else {
        multiplier
    }
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
        // Spawn zone should be outside inner radius and within outer radius
        assert!(spawn::ZONE_MIN > arena::INNER_RADIUS);
        assert!(spawn::ZONE_MAX < arena::OUTER_RADIUS);
    }

    // === SPEED SCALING TESTS ===

    #[test]
    fn test_thrust_multiplier_at_reference_mass() {
        // At reference mass (100), multiplier should be exactly 1.0
        let multiplier = mass_to_thrust_multiplier(boost::SPEED_REFERENCE_MASS);
        assert!(
            (multiplier - 1.0).abs() < 0.001,
            "At reference mass 100, multiplier should be 1.0, got {}",
            multiplier
        );
    }

    #[test]
    fn test_thrust_multiplier_small_mass_is_faster() {
        // Smaller mass should have higher multiplier (faster acceleration)
        let small = mass_to_thrust_multiplier(25.0);
        let reference = mass_to_thrust_multiplier(100.0);
        assert!(
            small > reference,
            "Small mass (25) should be faster than reference: {} vs {}",
            small,
            reference
        );
        // sqrt(100/25) = sqrt(4) = 2.0
        assert!(
            (small - 2.0).abs() < 0.001,
            "Mass 25 should have multiplier 2.0, got {}",
            small
        );
    }

    #[test]
    fn test_thrust_multiplier_large_mass_is_slower() {
        // Larger mass should have lower multiplier (slower acceleration)
        let large = mass_to_thrust_multiplier(400.0);
        let reference = mass_to_thrust_multiplier(100.0);
        assert!(
            large < reference,
            "Large mass (400) should be slower than reference: {} vs {}",
            large,
            reference
        );
        // sqrt(100/400) = sqrt(0.25) = 0.5
        assert!(
            (large - 0.5).abs() < 0.001,
            "Mass 400 should have multiplier 0.5, got {}",
            large
        );
    }

    #[test]
    fn test_thrust_multiplier_monotonically_decreasing() {
        // As mass increases, multiplier should decrease
        let masses = [10.0, 25.0, 50.0, 100.0, 200.0, 400.0, 800.0];
        for i in 0..masses.len() - 1 {
            let m1 = mass_to_thrust_multiplier(masses[i]);
            let m2 = mass_to_thrust_multiplier(masses[i + 1]);
            assert!(
                m1 > m2,
                "Multiplier should decrease: mass {} ({}) > mass {} ({})",
                masses[i],
                m1,
                masses[i + 1],
                m2
            );
        }
    }

    #[test]
    fn test_thrust_multiplier_clamped_min() {
        // Very large mass should be clamped to minimum multiplier
        let huge = mass_to_thrust_multiplier(100000.0);
        assert!(
            (huge - boost::SPEED_MIN_MULTIPLIER).abs() < 0.001,
            "Huge mass should be clamped to min {}, got {}",
            boost::SPEED_MIN_MULTIPLIER,
            huge
        );
    }

    #[test]
    fn test_thrust_multiplier_clamped_max() {
        // Mass at or below minimum should be clamped to max multiplier
        let at_min = mass_to_thrust_multiplier(mass::MINIMUM);
        // sqrt(100/10) = sqrt(10) ≈ 3.16, which is within max (3.5)
        assert!(
            at_min <= boost::SPEED_MAX_MULTIPLIER,
            "At minimum mass, multiplier {} should be <= max {}",
            at_min,
            boost::SPEED_MAX_MULTIPLIER
        );

        // Test below minimum (should use minimum mass in calculation)
        let below_min = mass_to_thrust_multiplier(1.0);
        assert_eq!(
            at_min, below_min,
            "Below minimum should use minimum mass: {} vs {}",
            at_min, below_min
        );
    }

    #[test]
    fn test_thrust_multiplier_known_values() {
        // Test specific known values for sqrt curve
        let test_cases: [(f32, f32); 5] = [
            (100.0, 1.0),      // sqrt(100/100) = 1.0
            (25.0, 2.0),       // sqrt(100/25) = 2.0
            (400.0, 0.5),      // sqrt(100/400) = 0.5
            (10.0, 3.162),     // sqrt(100/10) ≈ 3.162
            (1000.0, 0.316),   // sqrt(100/1000) ≈ 0.316
        ];

        for (mass, expected) in test_cases {
            let actual = mass_to_thrust_multiplier(mass);
            // Allow for clamping
            let clamped_expected = expected
                .max(boost::SPEED_MIN_MULTIPLIER)
                .min(boost::SPEED_MAX_MULTIPLIER);
            assert!(
                (actual - clamped_expected).abs() < 0.01,
                "Mass {} expected multiplier ~{}, got {}",
                mass,
                clamped_expected,
                actual
            );
        }
    }

    #[test]
    fn test_thrust_multiplier_no_nan_or_inf() {
        // Ensure no NaN or Inf for edge cases
        let edge_cases = [0.0, -1.0, f32::MIN, f32::MAX, f32::EPSILON];
        for mass in edge_cases {
            let m = mass_to_thrust_multiplier(mass);
            assert!(!m.is_nan(), "NaN for mass {}", mass);
            assert!(!m.is_infinite(), "Infinite for mass {}", mass);
            assert!(m >= boost::SPEED_MIN_MULTIPLIER);
            assert!(m <= boost::SPEED_MAX_MULTIPLIER);
        }
    }
}
