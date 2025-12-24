use std::net::{IpAddr, Ipv4Addr};

use crate::game::constants::{debris_spawning, gravity_waves};

// ============================================================================
// Configuration Validation Constants
// ============================================================================

/// Gravity influence radius bounds (world units)
/// Min: Minimum meaningful influence distance for limited mode
/// Max: Maximum practical range (beyond this, use unlimited mode)
mod gravity_bounds {
    pub const INFLUENCE_RADIUS_MIN: f32 = 1000.0;
    pub const INFLUENCE_RADIUS_MAX: f32 = 20000.0;
}

/// Arena scaling bounds
mod arena_bounds {
    /// Well count bounds (prevents excessive memory usage from malicious config)
    pub const MIN_WELLS_LOWER: usize = 1;
    pub const MIN_WELLS_UPPER: usize = 1000;

    /// Area per well bounds (square units)
    /// Lower = more wells, Upper = fewer wells
    pub const WELLS_PER_AREA_MIN: f32 = 100_000.0;
    pub const WELLS_PER_AREA_MAX: f32 = 5_000_000.0;
}

/// Safely parse a float from string, rejecting NaN and Infinity values
/// Returns None for invalid, NaN, or infinite values
#[inline]
fn parse_safe_f32(s: &str) -> Option<f32> {
    s.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// Gravity calculation mode
/// Controls how wells exert influence on entities
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GravityRangeMode {
    /// Limited range: Only wells within influence_radius affect entities
    /// Uses spatial grid for O(nearby) lookups - efficient for dense well fields
    #[default]
    Limited,
    /// Unlimited range: All wells affect all entities regardless of distance
    /// Uses cache-optimized batch processing for O(W) per entity
    Unlimited,
}

impl GravityRangeMode {
    /// Parse from string (case-insensitive)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "limited" => Some(Self::Limited),
            "unlimited" => Some(Self::Unlimited),
            _ => None,
        }
    }
}

/// Gravity system configuration
/// Controls gravity calculation behavior and performance tuning
#[derive(Debug, Clone)]
pub struct GravityConfig {
    /// Gravity calculation mode (limited vs unlimited range)
    pub range_mode: GravityRangeMode,
    /// Influence radius for limited mode (units)
    /// Wells beyond this distance don't affect entities
    pub influence_radius: f32,
}

impl Default for GravityConfig {
    fn default() -> Self {
        Self {
            range_mode: GravityRangeMode::Limited,
            influence_radius: 5000.0,
        }
    }
}

impl GravityConfig {
    /// Load config from environment variables, falling back to defaults
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Gravity range mode
        if let Ok(val) = std::env::var("GRAVITY_RANGE_MODE") {
            if let Some(mode) = GravityRangeMode::from_str(&val) {
                config.range_mode = mode;
            } else {
                tracing::warn!(
                    "Invalid GRAVITY_RANGE_MODE '{}', must be 'limited' or 'unlimited', using default",
                    val
                );
            }
        }

        // Influence radius (only used in limited mode)
        if let Ok(val) = std::env::var("GRAVITY_INFLUENCE_RADIUS") {
            if let Some(parsed) = parse_safe_f32(&val) {
                if (gravity_bounds::INFLUENCE_RADIUS_MIN..=gravity_bounds::INFLUENCE_RADIUS_MAX)
                    .contains(&parsed)
                {
                    config.influence_radius = parsed;
                } else {
                    tracing::warn!(
                        "GRAVITY_INFLUENCE_RADIUS must be {}-{}, using default",
                        gravity_bounds::INFLUENCE_RADIUS_MIN,
                        gravity_bounds::INFLUENCE_RADIUS_MAX
                    );
                }
            } else {
                tracing::warn!("GRAVITY_INFLUENCE_RADIUS invalid value, using default");
            }
        }

        tracing::info!(
            "Gravity config: mode={:?}, influence_radius={}",
            config.range_mode,
            config.influence_radius
        );

        config
    }
}

/// Server configuration
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields are part of public API, may be used by consumers
pub struct ServerConfig {
    /// Address to bind the server to
    pub bind_address: IpAddr,
    /// Port to listen on
    pub port: u16,
    /// Maximum number of concurrent game rooms
    pub max_rooms: usize,
    /// Maximum players per room (including bots)
    pub max_players_per_room: usize,
    /// Maximum human players per room
    pub max_humans_per_room: usize,
    /// Enable TLS (required for WebTransport)
    pub tls_enabled: bool,
    /// Path to TLS certificate file (if not using self-signed)
    pub tls_cert_path: Option<String>,
    /// Path to TLS key file (if not using self-signed)
    pub tls_key_path: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            // Use const to avoid runtime parsing
            bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            port: 4433,
            max_rooms: 100,
            max_players_per_room: 10,
            max_humans_per_room: 10,
            tls_enabled: true,
            tls_cert_path: None,
            tls_key_path: None,
        }
    }
}

impl ServerConfig {
    /// Load config from environment or use defaults
    pub fn load_or_default() -> Self {
        let mut config = Self::default();

        if let Ok(addr) = std::env::var("BIND_ADDRESS") {
            if let Ok(parsed) = addr.parse() {
                config.bind_address = parsed;
            } else {
                tracing::warn!("Invalid BIND_ADDRESS '{}', using default", addr);
            }
        }

        if let Ok(port) = std::env::var("PORT") {
            if let Ok(parsed) = port.parse::<u16>() {
                if parsed > 0 {
                    config.port = parsed;
                } else {
                    tracing::warn!("PORT must be > 0, using default");
                }
            } else {
                tracing::warn!("Invalid PORT '{}', using default", port);
            }
        }

        if let Ok(max_rooms) = std::env::var("MAX_ROOMS") {
            if let Ok(parsed) = max_rooms.parse::<usize>() {
                if parsed > 0 && parsed <= 10000 {
                    config.max_rooms = parsed;
                } else {
                    tracing::warn!("MAX_ROOMS must be 1-10000, using default");
                }
            } else {
                tracing::warn!("Invalid MAX_ROOMS '{}', using default", max_rooms);
            }
        }

        if let Ok(cert_path) = std::env::var("TLS_CERT_PATH") {
            config.tls_cert_path = Some(cert_path);
        }

        if let Ok(key_path) = std::env::var("TLS_KEY_PATH") {
            config.tls_key_path = Some(key_path);
        }

        config
    }

    /// Validate configuration after loading
    pub fn validate(&self) -> Result<(), String> {
        if self.port == 0 {
            return Err("Port cannot be 0".to_string());
        }
        if self.max_rooms == 0 {
            return Err("max_rooms must be at least 1".to_string());
        }
        if self.max_players_per_room == 0 {
            return Err("max_players_per_room must be at least 1".to_string());
        }
        if self.max_humans_per_room > self.max_players_per_room {
            return Err("max_humans_per_room cannot exceed max_players_per_room".to_string());
        }
        Ok(())
    }
}

/// Gravity wave explosion configuration
/// Controls the random well explosions that create expanding shockwaves
/// All values can be overridden via GRAVITY_WAVE_* environment variables
#[derive(Debug, Clone)]
pub struct GravityWaveConfig {
    /// Master switch - when false, wells never explode
    pub enabled: bool,
    /// Wave expansion speed (units/second)
    pub wave_speed: f32,
    /// Thickness of the wave front where players get pushed (units)
    pub wave_front_thickness: f32,
    /// Base impulse force applied to players
    pub wave_base_impulse: f32,
    /// Maximum wave radius before despawning
    pub wave_max_radius: f32,
    /// Warning/charging duration before explosion (seconds)
    pub charge_duration: f32,
    /// Minimum time between explosions per well (seconds)
    pub min_explosion_delay: f32,
    /// Maximum time between explosions per well (seconds)
    pub max_explosion_delay: f32,
}

impl Default for GravityWaveConfig {
    fn default() -> Self {
        Self {
            enabled: gravity_waves::ENABLED,
            wave_speed: gravity_waves::WAVE_SPEED,
            wave_front_thickness: gravity_waves::WAVE_FRONT_THICKNESS,
            wave_base_impulse: gravity_waves::WAVE_BASE_IMPULSE,
            wave_max_radius: gravity_waves::WAVE_MAX_RADIUS,
            charge_duration: gravity_waves::CHARGE_DURATION,
            min_explosion_delay: gravity_waves::MIN_EXPLOSION_DELAY,
            max_explosion_delay: gravity_waves::MAX_EXPLOSION_DELAY,
        }
    }
}

impl GravityWaveConfig {
    /// Load config from environment variables, falling back to defaults
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Feature flag
        if let Ok(val) = std::env::var("GRAVITY_WAVE_ENABLED") {
            config.enabled = val.to_lowercase() == "true" || val == "1";
        }

        // Wave expansion speed
        if let Ok(val) = std::env::var("GRAVITY_WAVE_SPEED") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed > 0.0 && parsed <= 2000.0 {
                    config.wave_speed = parsed;
                } else {
                    tracing::warn!("GRAVITY_WAVE_SPEED must be 0-2000, using default");
                }
            }
        }

        // Wave front thickness
        if let Ok(val) = std::env::var("GRAVITY_WAVE_FRONT_THICKNESS") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed > 0.0 && parsed <= 500.0 {
                    config.wave_front_thickness = parsed;
                } else {
                    tracing::warn!("GRAVITY_WAVE_FRONT_THICKNESS must be 0-500, using default");
                }
            }
        }

        // Base impulse force
        if let Ok(val) = std::env::var("GRAVITY_WAVE_BASE_IMPULSE") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.0 && parsed <= 1000.0 {
                    config.wave_base_impulse = parsed;
                } else {
                    tracing::warn!("GRAVITY_WAVE_BASE_IMPULSE must be 0-1000, using default");
                }
            }
        }

        // Maximum wave radius
        if let Ok(val) = std::env::var("GRAVITY_WAVE_MAX_RADIUS") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed > 100.0 && parsed <= 10000.0 {
                    config.wave_max_radius = parsed;
                } else {
                    tracing::warn!("GRAVITY_WAVE_MAX_RADIUS must be 100-10000, using default");
                }
            }
        }

        // Charge duration
        if let Ok(val) = std::env::var("GRAVITY_WAVE_CHARGE_DURATION") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.0 && parsed <= 10.0 {
                    config.charge_duration = parsed;
                } else {
                    tracing::warn!("GRAVITY_WAVE_CHARGE_DURATION must be 0-10, using default");
                }
            }
        }

        // Minimum explosion delay
        if let Ok(val) = std::env::var("GRAVITY_WAVE_MIN_DELAY") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 5.0 && parsed <= 600.0 {
                    config.min_explosion_delay = parsed;
                } else {
                    tracing::warn!("GRAVITY_WAVE_MIN_DELAY must be 5-600, using default");
                }
            }
        }

        // Maximum explosion delay
        if let Ok(val) = std::env::var("GRAVITY_WAVE_MAX_DELAY") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= config.min_explosion_delay && parsed <= 600.0 {
                    config.max_explosion_delay = parsed;
                } else {
                    tracing::warn!("GRAVITY_WAVE_MAX_DELAY must be >= min_delay and <= 600, using default");
                }
            }
        }

        // Log config if enabled
        if config.enabled {
            tracing::info!(
                "Gravity waves enabled: speed={}, impulse={}, delay={}-{}s",
                config.wave_speed,
                config.wave_base_impulse,
                config.min_explosion_delay,
                config.max_explosion_delay
            );
        } else {
            tracing::info!("Gravity waves disabled");
        }

        config
    }

    /// Generate a random explosion delay using this config
    pub fn random_explosion_delay(&self) -> f32 {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        rng.gen_range(self.min_explosion_delay..self.max_explosion_delay)
    }
}

/// Debris spawning configuration
/// Controls the random debris particles that players can collect for mass
/// All values can be overridden via DEBRIS_* environment variables
#[derive(Debug, Clone)]
pub struct DebrisSpawnConfig {
    /// Master switch - when false, no debris spawns
    pub enabled: bool,
    /// Maximum debris count in the game
    pub max_count: usize,
    /// Initial debris spawn counts per zone
    pub initial_inner: usize,
    pub initial_middle: usize,
    pub initial_outer: usize,
    /// Spawn rates per second per zone for each size
    pub spawn_rate_inner_small: f32,
    pub spawn_rate_middle_small: f32,
    pub spawn_rate_outer_small: f32,
    pub spawn_rate_inner_medium: f32,
    pub spawn_rate_middle_medium: f32,
    pub spawn_rate_outer_medium: f32,
    pub spawn_rate_inner_large: f32,
    pub spawn_rate_middle_large: f32,
    pub spawn_rate_outer_large: f32,
    /// Orbital velocity range for spawned debris
    pub orbital_velocity_min: f32,
    pub orbital_velocity_max: f32,
    /// Debris lifetime in seconds before decay
    pub lifetime: f32,
    /// Initial debris count per gravity well (feeding zones)
    pub well_debris_count: usize,
    /// Spawn rate per second for debris around gravity wells
    pub well_spawn_rate: f32,
}

impl Default for DebrisSpawnConfig {
    fn default() -> Self {
        Self {
            enabled: debris_spawning::ENABLED,
            max_count: debris_spawning::MAX_COUNT,
            initial_inner: debris_spawning::INITIAL_INNER,
            initial_middle: debris_spawning::INITIAL_MIDDLE,
            initial_outer: debris_spawning::INITIAL_OUTER,
            spawn_rate_inner_small: debris_spawning::SPAWN_RATE_INNER_SMALL,
            spawn_rate_middle_small: debris_spawning::SPAWN_RATE_MIDDLE_SMALL,
            spawn_rate_outer_small: debris_spawning::SPAWN_RATE_OUTER_SMALL,
            spawn_rate_inner_medium: debris_spawning::SPAWN_RATE_INNER_MEDIUM,
            spawn_rate_middle_medium: debris_spawning::SPAWN_RATE_MIDDLE_MEDIUM,
            spawn_rate_outer_medium: debris_spawning::SPAWN_RATE_OUTER_MEDIUM,
            spawn_rate_inner_large: debris_spawning::SPAWN_RATE_INNER_LARGE,
            spawn_rate_middle_large: debris_spawning::SPAWN_RATE_MIDDLE_LARGE,
            spawn_rate_outer_large: debris_spawning::SPAWN_RATE_OUTER_LARGE,
            orbital_velocity_min: debris_spawning::ORBITAL_VELOCITY_MIN,
            orbital_velocity_max: debris_spawning::ORBITAL_VELOCITY_MAX,
            lifetime: debris_spawning::LIFETIME,
            well_debris_count: 8,   // 8 debris per well initially
            well_spawn_rate: 2.0,   // 2 debris per second around wells
        }
    }
}

impl DebrisSpawnConfig {
    /// Load config from environment variables, falling back to defaults
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Feature flag
        if let Ok(val) = std::env::var("DEBRIS_SPAWN_ENABLED") {
            config.enabled = val.to_lowercase() == "true" || val == "1";
        }

        // Max count
        if let Ok(val) = std::env::var("DEBRIS_MAX_COUNT") {
            if let Ok(parsed) = val.parse::<usize>() {
                if parsed > 0 && parsed <= 1000 {
                    config.max_count = parsed;
                } else {
                    tracing::warn!("DEBRIS_MAX_COUNT must be 1-1000, using default");
                }
            }
        }

        // Initial spawn counts
        if let Ok(val) = std::env::var("DEBRIS_INITIAL_INNER") {
            if let Ok(parsed) = val.parse::<usize>() {
                config.initial_inner = parsed.min(500);
            }
        }
        if let Ok(val) = std::env::var("DEBRIS_INITIAL_MIDDLE") {
            if let Ok(parsed) = val.parse::<usize>() {
                config.initial_middle = parsed.min(500);
            }
        }
        if let Ok(val) = std::env::var("DEBRIS_INITIAL_OUTER") {
            if let Ok(parsed) = val.parse::<usize>() {
                config.initial_outer = parsed.min(500);
            }
        }

        // Spawn rates - inner zone
        if let Ok(val) = std::env::var("DEBRIS_SPAWN_RATE_INNER_SMALL") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.0 && parsed <= 20.0 {
                    config.spawn_rate_inner_small = parsed;
                }
            }
        }
        if let Ok(val) = std::env::var("DEBRIS_SPAWN_RATE_INNER_MEDIUM") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.0 && parsed <= 10.0 {
                    config.spawn_rate_inner_medium = parsed;
                }
            }
        }
        if let Ok(val) = std::env::var("DEBRIS_SPAWN_RATE_INNER_LARGE") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.0 && parsed <= 5.0 {
                    config.spawn_rate_inner_large = parsed;
                }
            }
        }

        // Spawn rates - middle zone
        if let Ok(val) = std::env::var("DEBRIS_SPAWN_RATE_MIDDLE_SMALL") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.0 && parsed <= 20.0 {
                    config.spawn_rate_middle_small = parsed;
                }
            }
        }
        if let Ok(val) = std::env::var("DEBRIS_SPAWN_RATE_MIDDLE_MEDIUM") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.0 && parsed <= 10.0 {
                    config.spawn_rate_middle_medium = parsed;
                }
            }
        }
        if let Ok(val) = std::env::var("DEBRIS_SPAWN_RATE_MIDDLE_LARGE") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.0 && parsed <= 5.0 {
                    config.spawn_rate_middle_large = parsed;
                }
            }
        }

        // Spawn rates - outer zone
        if let Ok(val) = std::env::var("DEBRIS_SPAWN_RATE_OUTER_SMALL") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.0 && parsed <= 20.0 {
                    config.spawn_rate_outer_small = parsed;
                }
            }
        }
        if let Ok(val) = std::env::var("DEBRIS_SPAWN_RATE_OUTER_MEDIUM") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.0 && parsed <= 10.0 {
                    config.spawn_rate_outer_medium = parsed;
                }
            }
        }
        if let Ok(val) = std::env::var("DEBRIS_SPAWN_RATE_OUTER_LARGE") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.0 && parsed <= 5.0 {
                    config.spawn_rate_outer_large = parsed;
                }
            }
        }

        // Orbital velocity range
        if let Ok(val) = std::env::var("DEBRIS_ORBITAL_VELOCITY_MIN") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.0 && parsed <= 100.0 {
                    config.orbital_velocity_min = parsed;
                }
            }
        }
        if let Ok(val) = std::env::var("DEBRIS_ORBITAL_VELOCITY_MAX") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= config.orbital_velocity_min && parsed <= 200.0 {
                    config.orbital_velocity_max = parsed;
                }
            }
        }

        // Lifetime
        if let Ok(val) = std::env::var("DEBRIS_LIFETIME") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 10.0 && parsed <= 300.0 {
                    config.lifetime = parsed;
                }
            }
        }

        // Log config
        if config.enabled {
            tracing::info!(
                "Debris spawning enabled: max_count={}, initial={}/{}/{}",
                config.max_count,
                config.initial_inner,
                config.initial_middle,
                config.initial_outer
            );
        } else {
            tracing::info!("Debris spawning disabled");
        }

        config
    }

    /// Get total spawn rate for a zone (all sizes combined)
    /// Available for metrics and debugging
    #[allow(dead_code)]
    pub fn total_spawn_rate(&self, zone: &str) -> f32 {
        match zone {
            "inner" => {
                self.spawn_rate_inner_small
                    + self.spawn_rate_inner_medium
                    + self.spawn_rate_inner_large
            }
            "middle" => {
                self.spawn_rate_middle_small
                    + self.spawn_rate_middle_medium
                    + self.spawn_rate_middle_large
            }
            "outer" => {
                self.spawn_rate_outer_small
                    + self.spawn_rate_outer_medium
                    + self.spawn_rate_outer_large
            }
            _ => 0.0,
        }
    }
}

/// Arena scaling configuration
/// Controls dynamic arena sizing based on player count and simulation mode
/// All values can be overridden via ARENA_* environment variables
#[derive(Debug, Clone)]
pub struct ArenaScalingConfig {
    // Growth/Shrink behavior
    /// How fast arena grows towards target (0.01-0.1)
    pub grow_lerp: f32,
    /// How fast arena shrinks towards target (0.001-0.05)
    pub shrink_lerp: f32,
    /// Ticks to wait before shrinking (0-300)
    pub shrink_delay_ticks: u32,

    // Size limits
    /// Minimum arena escape radius (500-2000)
    pub min_escape_radius: f32,
    /// Maximum arena scale multiplier (5-20)
    pub max_escape_multiplier: f32,
    /// Arena growth units per player (5-50)
    pub growth_per_player: f32,
    /// Player count before arena starts growing (1-50)
    pub player_threshold: usize,

    // Well positioning
    /// Minimum well distance ratio from center (0.1-0.4)
    pub well_min_ratio: f32,
    /// Maximum well distance ratio from center (0.6-0.95)
    pub well_max_ratio: f32,
    /// Square units per gravity well (area-based scaling)
    /// Lower = more wells. Default 500_000 = 1 well per 500K sq units
    pub wells_per_area: f32,
    /// Minimum wells regardless of area (1+)
    pub min_wells: usize,

    // Well ring distribution (percentages of escape_radius)
    pub ring_inner_min: f32,
    pub ring_inner_max: f32,
    pub ring_middle_min: f32,
    pub ring_middle_max: f32,
    pub ring_outer_min: f32,
    pub ring_outer_max: f32,

    // Supermassive black hole
    pub supermassive_mass_mult: f32,
    pub supermassive_core_mult: f32,
}

impl Default for ArenaScalingConfig {
    fn default() -> Self {
        Self {
            grow_lerp: 0.02,
            shrink_lerp: 0.005,
            shrink_delay_ticks: 150,
            min_escape_radius: 800.0,
            max_escape_multiplier: 10.0,
            growth_per_player: 25.0,
            player_threshold: 1,
            well_min_ratio: 0.20,
            well_max_ratio: 0.85,
            wells_per_area: 5_000_000.0, // 1 well per 5M square units (sparse wells)
            min_wells: 1, // Only 1 minimum for small arenas
            ring_inner_min: 0.25,
            ring_inner_max: 0.40,
            ring_middle_min: 0.45,
            ring_middle_max: 0.65,
            ring_outer_min: 0.70,
            ring_outer_max: 0.90,
            supermassive_mass_mult: 3.0,
            supermassive_core_mult: 2.5,
        }
    }
}

impl ArenaScalingConfig {
    /// Load config from environment variables, falling back to defaults
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Growth/Shrink behavior
        if let Ok(val) = std::env::var("ARENA_GROW_LERP") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.01 && parsed <= 0.1 {
                    config.grow_lerp = parsed;
                } else {
                    tracing::warn!("ARENA_GROW_LERP must be 0.01-0.1, using default");
                }
            }
        }

        if let Ok(val) = std::env::var("ARENA_SHRINK_LERP") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.001 && parsed <= 0.05 {
                    config.shrink_lerp = parsed;
                } else {
                    tracing::warn!("ARENA_SHRINK_LERP must be 0.001-0.05, using default");
                }
            }
        }

        if let Ok(val) = std::env::var("ARENA_SHRINK_DELAY_TICKS") {
            if let Ok(parsed) = val.parse::<u32>() {
                if parsed <= 300 {
                    config.shrink_delay_ticks = parsed;
                } else {
                    tracing::warn!("ARENA_SHRINK_DELAY_TICKS must be 0-300, using default");
                }
            }
        }

        // Size limits
        if let Ok(val) = std::env::var("ARENA_MIN_RADIUS") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 500.0 && parsed <= 2000.0 {
                    config.min_escape_radius = parsed;
                } else {
                    tracing::warn!("ARENA_MIN_RADIUS must be 500-2000, using default");
                }
            }
        }

        if let Ok(val) = std::env::var("ARENA_MAX_MULTIPLIER") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 5.0 && parsed <= 20.0 {
                    config.max_escape_multiplier = parsed;
                } else {
                    tracing::warn!("ARENA_MAX_MULTIPLIER must be 5-20, using default");
                }
            }
        }

        if let Ok(val) = std::env::var("ARENA_GROWTH_PER_PLAYER") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 5.0 && parsed <= 50.0 {
                    config.growth_per_player = parsed;
                } else {
                    tracing::warn!("ARENA_GROWTH_PER_PLAYER must be 5-50, using default");
                }
            }
        }

        if let Ok(val) = std::env::var("ARENA_PLAYER_THRESHOLD") {
            if let Ok(parsed) = val.parse::<usize>() {
                if parsed >= 1 && parsed <= 50 {
                    config.player_threshold = parsed;
                } else {
                    tracing::warn!("ARENA_PLAYER_THRESHOLD must be 1-50, using default");
                }
            }
        }

        // Well positioning
        if let Ok(val) = std::env::var("ARENA_WELL_MIN_RATIO") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.1 && parsed <= 0.4 {
                    config.well_min_ratio = parsed;
                } else {
                    tracing::warn!("ARENA_WELL_MIN_RATIO must be 0.1-0.4, using default");
                }
            }
        }

        if let Ok(val) = std::env::var("ARENA_WELL_MAX_RATIO") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.6 && parsed <= 0.95 {
                    config.well_max_ratio = parsed;
                } else {
                    tracing::warn!("ARENA_WELL_MAX_RATIO must be 0.6-0.95, using default");
                }
            }
        }

        // Area-based well scaling (CRITICAL: must reject NaN/Infinity to prevent division issues)
        if let Ok(val) = std::env::var("ARENA_WELLS_PER_AREA") {
            if let Some(parsed) = parse_safe_f32(&val) {
                if (arena_bounds::WELLS_PER_AREA_MIN..=arena_bounds::WELLS_PER_AREA_MAX)
                    .contains(&parsed)
                {
                    config.wells_per_area = parsed;
                } else {
                    tracing::warn!(
                        "ARENA_WELLS_PER_AREA must be {}-{}, using default",
                        arena_bounds::WELLS_PER_AREA_MIN,
                        arena_bounds::WELLS_PER_AREA_MAX
                    );
                }
            } else {
                tracing::warn!("ARENA_WELLS_PER_AREA invalid value, using default");
            }
        }

        if let Ok(val) = std::env::var("ARENA_MIN_WELLS") {
            if let Ok(parsed) = val.parse::<usize>() {
                if (arena_bounds::MIN_WELLS_LOWER..=arena_bounds::MIN_WELLS_UPPER).contains(&parsed)
                {
                    config.min_wells = parsed;
                } else {
                    tracing::warn!(
                        "ARENA_MIN_WELLS must be {}-{}, using default",
                        arena_bounds::MIN_WELLS_LOWER,
                        arena_bounds::MIN_WELLS_UPPER
                    );
                }
            }
        }

        // Ring distribution
        if let Ok(val) = std::env::var("ARENA_RING_INNER_MIN") {
            if let Ok(parsed) = val.parse::<f32>() {
                config.ring_inner_min = parsed.clamp(0.1, 0.5);
            }
        }
        if let Ok(val) = std::env::var("ARENA_RING_INNER_MAX") {
            if let Ok(parsed) = val.parse::<f32>() {
                config.ring_inner_max = parsed.clamp(config.ring_inner_min, 0.6);
            }
        }
        if let Ok(val) = std::env::var("ARENA_RING_MIDDLE_MIN") {
            if let Ok(parsed) = val.parse::<f32>() {
                config.ring_middle_min = parsed.clamp(0.3, 0.7);
            }
        }
        if let Ok(val) = std::env::var("ARENA_RING_MIDDLE_MAX") {
            if let Ok(parsed) = val.parse::<f32>() {
                config.ring_middle_max = parsed.clamp(config.ring_middle_min, 0.8);
            }
        }
        if let Ok(val) = std::env::var("ARENA_RING_OUTER_MIN") {
            if let Ok(parsed) = val.parse::<f32>() {
                config.ring_outer_min = parsed.clamp(0.5, 0.9);
            }
        }
        if let Ok(val) = std::env::var("ARENA_RING_OUTER_MAX") {
            if let Ok(parsed) = val.parse::<f32>() {
                config.ring_outer_max = parsed.clamp(config.ring_outer_min, 0.95);
            }
        }

        // Supermassive black hole
        if let Ok(val) = std::env::var("ARENA_SUPERMASSIVE_MASS") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 1.0 && parsed <= 10.0 {
                    config.supermassive_mass_mult = parsed;
                }
            }
        }
        if let Ok(val) = std::env::var("ARENA_SUPERMASSIVE_CORE") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 1.0 && parsed <= 5.0 {
                    config.supermassive_core_mult = parsed;
                }
            }
        }

        tracing::info!(
            "Arena scaling: grow_lerp={}, shrink_lerp={}, delay={}, wells_per_area={}, min_wells={}",
            config.grow_lerp,
            config.shrink_lerp,
            config.shrink_delay_ticks,
            config.wells_per_area,
            config.min_wells
        );

        config
    }
}

/// AI Simulation Manager configuration
/// Controls the autonomous AI that monitors and adjusts simulation parameters
/// All values can be overridden via AI_* environment variables
#[derive(Debug, Clone)]
pub struct AIManagerConfig {
    /// Master switch - when false, AI manager is disabled
    pub enabled: bool,
    /// Claude API key (required if enabled)
    pub api_key: Option<String>,
    /// Minutes between AI evaluations (1-60)
    pub eval_interval_minutes: u32,
    /// Maximum decisions to keep in history (10-1000)
    pub max_history: usize,
    /// Minimum confidence to act on recommendations (0.0-1.0)
    pub confidence_threshold: f32,
    /// Claude model to use
    pub model: String,
    /// Path to decision history file
    pub history_file: String,
}

impl Default for AIManagerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: None,
            eval_interval_minutes: 2,
            max_history: 100,
            confidence_threshold: 0.7,
            model: "claude-sonnet-4-5".to_string(),
            history_file: "data/ai_decisions.json".to_string(),
        }
    }
}

impl AIManagerConfig {
    /// Load config from environment variables, falling back to defaults
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Feature flag
        if let Ok(val) = std::env::var("AI_ENABLED") {
            config.enabled = val.to_lowercase() == "true" || val == "1";
        }

        // API key (required if enabled) - use ORBIT_API_KEY for the Anthropic API key
        if let Ok(val) = std::env::var("ORBIT_API_KEY") {
            if !val.is_empty() {
                config.api_key = Some(val);
            }
        }

        // Evaluation interval
        if let Ok(val) = std::env::var("AI_EVAL_INTERVAL_MINUTES") {
            if let Ok(parsed) = val.parse::<u32>() {
                if parsed >= 1 && parsed <= 60 {
                    config.eval_interval_minutes = parsed;
                } else {
                    tracing::warn!("AI_EVAL_INTERVAL_MINUTES must be 1-60, using default");
                }
            }
        }

        // Max history
        if let Ok(val) = std::env::var("AI_MAX_HISTORY") {
            if let Ok(parsed) = val.parse::<usize>() {
                if parsed >= 10 && parsed <= 1000 {
                    config.max_history = parsed;
                } else {
                    tracing::warn!("AI_MAX_HISTORY must be 10-1000, using default");
                }
            }
        }

        // Confidence threshold
        if let Ok(val) = std::env::var("AI_CONFIDENCE_THRESHOLD") {
            if let Ok(parsed) = val.parse::<f32>() {
                if parsed >= 0.0 && parsed <= 1.0 {
                    config.confidence_threshold = parsed;
                } else {
                    tracing::warn!("AI_CONFIDENCE_THRESHOLD must be 0.0-1.0, using default");
                }
            }
        }

        // Model
        if let Ok(val) = std::env::var("AI_MODEL") {
            if !val.is_empty() {
                config.model = val;
            }
        }

        // History file path
        if let Ok(val) = std::env::var("AI_HISTORY_FILE") {
            if !val.is_empty() {
                config.history_file = val;
            }
        }

        // Validate configuration
        if config.enabled {
            if config.api_key.is_none() {
                tracing::error!("AI_ENABLED=true but ORBIT_API_KEY not set, disabling AI manager");
                config.enabled = false;
            } else {
                tracing::info!(
                    "AI manager enabled: interval={}min, model={}, threshold={}",
                    config.eval_interval_minutes,
                    config.model,
                    config.confidence_threshold
                );
            }
        } else {
            tracing::info!("AI manager disabled");
        }

        config
    }

    /// Check if AI manager should be active
    pub fn is_active(&self) -> bool {
        self.enabled && self.api_key.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.port, 4433);
        assert_eq!(config.max_rooms, 100);
        assert_eq!(config.max_players_per_room, 10);
        assert!(config.tls_enabled);
    }

    #[test]
    fn test_load_or_default() {
        let config = ServerConfig::load_or_default();
        assert!(config.port > 0);
    }

    #[test]
    fn test_gravity_wave_config_defaults() {
        let config = GravityWaveConfig::default();
        assert!(config.enabled);
        assert_eq!(config.wave_speed, 300.0);
        assert_eq!(config.wave_base_impulse, 180.0);
        assert_eq!(config.min_explosion_delay, 30.0);
        assert_eq!(config.max_explosion_delay, 90.0);
    }

    #[test]
    fn test_gravity_wave_random_delay() {
        let config = GravityWaveConfig::default();
        let delay = config.random_explosion_delay();
        assert!(delay >= config.min_explosion_delay);
        assert!(delay <= config.max_explosion_delay);
    }

    #[test]
    fn test_debris_spawn_config_defaults() {
        let config = DebrisSpawnConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_count, 500); // Increased with spatial hash optimization
        assert_eq!(config.initial_inner, 50);
        assert_eq!(config.initial_middle, 40);
        assert_eq!(config.initial_outer, 30);
        assert_eq!(config.spawn_rate_inner_small, 2.0);
        assert_eq!(config.well_debris_count, 8);
        assert_eq!(config.well_spawn_rate, 2.0);
    }

    #[test]
    fn test_debris_spawn_total_rate() {
        let config = DebrisSpawnConfig::default();
        let inner_total = config.total_spawn_rate("inner");
        assert!((inner_total - 2.6).abs() < 0.01); // 2.0 + 0.5 + 0.1 = 2.6
    }

    #[test]
    fn test_arena_scaling_config_defaults() {
        let config = ArenaScalingConfig::default();
        assert_eq!(config.grow_lerp, 0.02);
        assert_eq!(config.shrink_lerp, 0.005);
        assert_eq!(config.shrink_delay_ticks, 150);
        assert_eq!(config.min_escape_radius, 800.0);
        assert_eq!(config.wells_per_area, 5_000_000.0);
        assert_eq!(config.min_wells, 1);
        assert_eq!(config.ring_inner_min, 0.25);
        assert_eq!(config.ring_outer_max, 0.90);
    }

    #[test]
    fn test_gravity_range_mode_from_str() {
        assert_eq!(
            GravityRangeMode::from_str("limited"),
            Some(GravityRangeMode::Limited)
        );
        assert_eq!(
            GravityRangeMode::from_str("LIMITED"),
            Some(GravityRangeMode::Limited)
        );
        assert_eq!(
            GravityRangeMode::from_str("unlimited"),
            Some(GravityRangeMode::Unlimited)
        );
        assert_eq!(
            GravityRangeMode::from_str("UNLIMITED"),
            Some(GravityRangeMode::Unlimited)
        );
        assert_eq!(GravityRangeMode::from_str("invalid"), None);
        assert_eq!(GravityRangeMode::from_str(""), None);
    }

    #[test]
    fn test_gravity_range_mode_default() {
        let mode = GravityRangeMode::default();
        assert_eq!(mode, GravityRangeMode::Limited);
    }

    #[test]
    fn test_gravity_config_defaults() {
        let config = GravityConfig::default();
        assert_eq!(config.range_mode, GravityRangeMode::Limited);
        assert_eq!(config.influence_radius, 5000.0);
    }

    #[test]
    fn test_ai_manager_config_defaults() {
        let config = AIManagerConfig::default();
        assert!(!config.enabled);
        assert!(config.api_key.is_none());
        assert_eq!(config.eval_interval_minutes, 2);
        assert_eq!(config.max_history, 100);
        assert_eq!(config.confidence_threshold, 0.7);
        assert_eq!(config.model, "claude-sonnet-4-5");
        assert!(!config.is_active());
    }

    #[test]
    fn test_ai_manager_is_active() {
        let mut config = AIManagerConfig::default();
        assert!(!config.is_active());

        config.enabled = true;
        assert!(!config.is_active()); // Still false, no API key

        config.api_key = Some("test-key".to_string());
        assert!(config.is_active()); // Now active
    }
}
