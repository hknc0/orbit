use std::net::{IpAddr, Ipv4Addr};

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
}
