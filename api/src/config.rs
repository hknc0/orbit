use std::net::{IpAddr, Ipv4Addr};

use crate::game::constants::{debris_spawning, gravity_waves};

/// Server configuration
#[derive(Debug, Clone)]
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
        assert_eq!(config.max_count, 200);
        assert_eq!(config.initial_inner, 50);
        assert_eq!(config.initial_middle, 40);
        assert_eq!(config.initial_outer, 30);
        assert_eq!(config.spawn_rate_inner_small, 2.0);
    }

    #[test]
    fn test_debris_spawn_total_rate() {
        let config = DebrisSpawnConfig::default();
        let inner_total = config.total_spawn_rate("inner");
        assert!((inner_total - 2.6).abs() < 0.01); // 2.0 + 0.5 + 0.1 = 2.6
    }
}
