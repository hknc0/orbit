use std::net::IpAddr;

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
            bind_address: "0.0.0.0".parse().unwrap(),
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
            }
        }

        if let Ok(port) = std::env::var("PORT") {
            if let Ok(parsed) = port.parse() {
                config.port = parsed;
            }
        }

        if let Ok(max_rooms) = std::env::var("MAX_ROOMS") {
            if let Ok(parsed) = max_rooms.parse() {
                config.max_rooms = parsed;
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
