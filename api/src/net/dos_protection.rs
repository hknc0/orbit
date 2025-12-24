use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// Configuration for DoS protection
#[derive(Debug, Clone)]
pub struct DoSConfig {
    /// Maximum total concurrent connections
    pub max_connections_total: usize,
    /// Maximum connections per IP address
    pub max_connections_per_ip: usize,
    /// Maximum messages per second per connection
    pub max_messages_per_second: u32,
    /// Maximum message size in bytes
    pub max_message_size: usize,
    /// Time window for rate limiting
    pub rate_limit_window: Duration,
    /// Ban duration for repeat offenders
    pub ban_duration: Duration,
    /// Number of violations before ban
    pub violations_before_ban: u32,
}

impl Default for DoSConfig {
    fn default() -> Self {
        Self {
            max_connections_total: 1000,
            max_connections_per_ip: 5,
            max_messages_per_second: 100,
            max_message_size: 65536,
            rate_limit_window: Duration::from_secs(1),
            ban_duration: Duration::from_secs(300),
            violations_before_ban: 5,
        }
    }
}

/// Tracks rate limiting for a single connection
#[derive(Debug)]
struct ConnectionRateLimit {
    message_count: u32,
    window_start: Instant,
    violations: u32,
}

impl ConnectionRateLimit {
    fn new() -> Self {
        Self {
            message_count: 0,
            window_start: Instant::now(),
            violations: 0,
        }
    }

    fn check_and_increment(&mut self, max_per_second: u32, window: Duration) -> bool {
        let now = Instant::now();

        // Reset window if expired
        if now.duration_since(self.window_start) >= window {
            self.window_start = now;
            self.message_count = 0;
        }

        self.message_count += 1;

        if self.message_count > max_per_second {
            self.violations += 1;
            false
        } else {
            true
        }
    }
}

/// Tracks IP-based bans
#[derive(Debug)]
struct IpBan {
    banned_at: Instant,
    reason: String,
}

/// DoS protection manager
pub struct DoSProtection {
    config: DoSConfig,
    /// Connections per IP
    ip_connections: HashMap<IpAddr, usize>,
    /// Rate limits per connection ID
    connection_rates: HashMap<u64, ConnectionRateLimit>,
    /// Banned IPs
    banned_ips: HashMap<IpAddr, IpBan>,
    /// Total active connections
    total_connections: usize,
}

impl DoSProtection {
    pub fn new(config: DoSConfig) -> Self {
        Self {
            config,
            ip_connections: HashMap::new(),
            connection_rates: HashMap::new(),
            banned_ips: HashMap::new(),
            total_connections: 0,
        }
    }

    /// Check if a new connection from this IP is allowed
    pub fn check_connection(&self, ip: IpAddr) -> Result<(), DoSError> {
        // Check if IP is banned
        if let Some(ban) = self.banned_ips.get(&ip) {
            if ban.banned_at.elapsed() < self.config.ban_duration {
                return Err(DoSError::IpBanned(ban.reason.clone()));
            }
        }

        // Check total connection limit
        if self.total_connections >= self.config.max_connections_total {
            return Err(DoSError::TooManyConnections);
        }

        // Check per-IP limit
        let ip_count = self.ip_connections.get(&ip).copied().unwrap_or(0);
        if ip_count >= self.config.max_connections_per_ip {
            return Err(DoSError::TooManyConnectionsFromIp);
        }

        Ok(())
    }

    /// Register a new connection
    pub fn register_connection(&mut self, ip: IpAddr) -> Result<u64, DoSError> {
        self.check_connection(ip)?;

        // Generate random connection ID (avoid collisions)
        let connection_id = loop {
            let id = rand::random::<u64>();
            if !self.connection_rates.contains_key(&id) {
                break id;
            }
        };

        *self.ip_connections.entry(ip).or_insert(0) += 1;
        self.connection_rates
            .insert(connection_id, ConnectionRateLimit::new());
        self.total_connections += 1;

        Ok(connection_id)
    }

    /// Unregister a connection
    pub fn unregister_connection(&mut self, connection_id: u64, ip: IpAddr) {
        self.connection_rates.remove(&connection_id);

        if let Some(count) = self.ip_connections.get_mut(&ip) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.ip_connections.remove(&ip);
            }
        }

        self.total_connections = self.total_connections.saturating_sub(1);
    }

    /// Check if a message from this connection is allowed
    pub fn check_message(&mut self, connection_id: u64, size: usize) -> Result<(), DoSError> {
        // Check message size
        if size > self.config.max_message_size {
            return Err(DoSError::MessageTooLarge(size));
        }

        // Check rate limit
        if let Some(rate) = self.connection_rates.get_mut(&connection_id) {
            // Check if already exceeded violation limit
            if rate.violations >= self.config.violations_before_ban {
                return Err(DoSError::ViolationLimitExceeded);
            }

            if !rate.check_and_increment(
                self.config.max_messages_per_second,
                self.config.rate_limit_window,
            ) {
                return Err(DoSError::RateLimitExceeded);
            }
        }

        Ok(())
    }

    /// Ban an IP address
    #[allow(dead_code)]
    pub fn ban_ip(&mut self, ip: IpAddr, reason: String) {
        self.banned_ips.insert(
            ip,
            IpBan {
                banned_at: Instant::now(),
                reason,
            },
        );
    }

    /// Unban an IP address
    #[allow(dead_code)]
    pub fn unban_ip(&mut self, ip: IpAddr) -> bool {
        self.banned_ips.remove(&ip).is_some()
    }

    /// Check if an IP is banned
    #[allow(dead_code)]
    pub fn is_banned(&self, ip: IpAddr) -> bool {
        if let Some(ban) = self.banned_ips.get(&ip) {
            ban.banned_at.elapsed() < self.config.ban_duration
        } else {
            false
        }
    }

    /// Clean up expired bans
    #[allow(dead_code)]
    pub fn cleanup_expired_bans(&mut self) -> usize {
        let ban_duration = self.config.ban_duration;
        let before = self.banned_ips.len();
        self.banned_ips
            .retain(|_, ban| ban.banned_at.elapsed() < ban_duration);
        before - self.banned_ips.len()
    }

    /// Get current connection count
    #[allow(dead_code)]
    pub fn connection_count(&self) -> usize {
        self.total_connections
    }

    /// Get connections from an IP
    #[allow(dead_code)]
    pub fn connections_from_ip(&self, ip: IpAddr) -> usize {
        self.ip_connections.get(&ip).copied().unwrap_or(0)
    }

    /// Get violation count for a connection
    #[allow(dead_code)]
    pub fn violation_count(&self, connection_id: u64) -> u32 {
        self.connection_rates
            .get(&connection_id)
            .map(|r| r.violations)
            .unwrap_or(0)
    }
}

impl Default for DoSProtection {
    fn default() -> Self {
        Self::new(DoSConfig::default())
    }
}

/// Errors from DoS protection checks
#[derive(Debug, Clone, thiserror::Error)]
pub enum DoSError {
    #[error("IP is banned: {0}")]
    IpBanned(String),
    #[error("Too many total connections")]
    TooManyConnections,
    #[error("Too many connections from this IP")]
    TooManyConnectionsFromIp,
    #[error("Message too large: {0} bytes")]
    MessageTooLarge(usize),
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    #[error("Too many violations, connection terminated")]
    ViolationLimitExceeded,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn test_ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))
    }

    fn test_ip_2() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2))
    }

    #[test]
    fn test_register_connection() {
        let mut dos = DoSProtection::default();
        let ip = test_ip();

        let id = dos.register_connection(ip).unwrap();
        assert_eq!(dos.connection_count(), 1);
        assert_eq!(dos.connections_from_ip(ip), 1);
        // ID is now random, just verify it was assigned
        assert!(dos.violation_count(id) == 0);
    }

    #[test]
    fn test_unregister_connection() {
        let mut dos = DoSProtection::default();
        let ip = test_ip();

        let id = dos.register_connection(ip).unwrap();
        dos.unregister_connection(id, ip);

        assert_eq!(dos.connection_count(), 0);
        assert_eq!(dos.connections_from_ip(ip), 0);
    }

    #[test]
    fn test_max_connections_per_ip() {
        let config = DoSConfig {
            max_connections_per_ip: 2,
            ..Default::default()
        };
        let mut dos = DoSProtection::new(config);
        let ip = test_ip();

        dos.register_connection(ip).unwrap();
        dos.register_connection(ip).unwrap();

        let result = dos.register_connection(ip);
        assert!(matches!(result, Err(DoSError::TooManyConnectionsFromIp)));
    }

    #[test]
    fn test_max_total_connections() {
        let config = DoSConfig {
            max_connections_total: 2,
            max_connections_per_ip: 10,
            ..Default::default()
        };
        let mut dos = DoSProtection::new(config);

        dos.register_connection(test_ip()).unwrap();
        dos.register_connection(test_ip_2()).unwrap();

        let result = dos.register_connection(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 3)));
        assert!(matches!(result, Err(DoSError::TooManyConnections)));
    }

    #[test]
    fn test_message_size() {
        let config = DoSConfig {
            max_message_size: 100,
            ..Default::default()
        };
        let mut dos = DoSProtection::new(config);
        let ip = test_ip();

        let id = dos.register_connection(ip).unwrap();

        assert!(dos.check_message(id, 50).is_ok());
        assert!(matches!(
            dos.check_message(id, 200),
            Err(DoSError::MessageTooLarge(_))
        ));
    }

    #[test]
    fn test_rate_limiting() {
        let config = DoSConfig {
            max_messages_per_second: 3,
            rate_limit_window: Duration::from_secs(1),
            ..Default::default()
        };
        let mut dos = DoSProtection::new(config);
        let ip = test_ip();

        let id = dos.register_connection(ip).unwrap();

        // First 3 should pass
        assert!(dos.check_message(id, 10).is_ok());
        assert!(dos.check_message(id, 10).is_ok());
        assert!(dos.check_message(id, 10).is_ok());

        // 4th should fail
        assert!(matches!(
            dos.check_message(id, 10),
            Err(DoSError::RateLimitExceeded)
        ));
    }

    #[test]
    fn test_ban_ip() {
        let mut dos = DoSProtection::default();
        let ip = test_ip();

        dos.ban_ip(ip, "Test ban".to_string());

        assert!(dos.is_banned(ip));
        assert!(matches!(
            dos.check_connection(ip),
            Err(DoSError::IpBanned(_))
        ));
    }

    #[test]
    fn test_unban_ip() {
        let mut dos = DoSProtection::default();
        let ip = test_ip();

        dos.ban_ip(ip, "Test ban".to_string());
        assert!(dos.is_banned(ip));

        dos.unban_ip(ip);
        assert!(!dos.is_banned(ip));
    }

    #[test]
    fn test_violation_accumulation() {
        let config = DoSConfig {
            max_messages_per_second: 1,
            violations_before_ban: 3,
            ..Default::default()
        };
        let mut dos = DoSProtection::new(config);
        let ip = test_ip();

        let id = dos.register_connection(ip).unwrap();

        // Use up the limit
        dos.check_message(id, 10).unwrap();

        // Generate violations
        assert!(dos.check_message(id, 10).is_err());
        assert_eq!(dos.violation_count(id), 1);

        assert!(dos.check_message(id, 10).is_err());
        assert_eq!(dos.violation_count(id), 2);

        assert!(dos.check_message(id, 10).is_err());
        assert_eq!(dos.violation_count(id), 3);

        // Should now get violation limit exceeded
        assert!(matches!(
            dos.check_message(id, 10),
            Err(DoSError::ViolationLimitExceeded)
        ));
    }

    #[test]
    fn test_default_config() {
        let config = DoSConfig::default();
        assert_eq!(config.max_connections_total, 1000);
        assert_eq!(config.max_connections_per_ip, 5);
        assert_eq!(config.max_messages_per_second, 100);
    }

    #[test]
    fn test_different_ips_independent() {
        let config = DoSConfig {
            max_connections_per_ip: 1,
            ..Default::default()
        };
        let mut dos = DoSProtection::new(config);

        dos.register_connection(test_ip()).unwrap();
        dos.register_connection(test_ip_2()).unwrap();

        // Each IP has independent limits
        assert_eq!(dos.connections_from_ip(test_ip()), 1);
        assert_eq!(dos.connections_from_ip(test_ip_2()), 1);
    }
}
