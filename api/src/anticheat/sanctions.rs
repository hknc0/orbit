use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use crate::game::state::PlayerId;

/// Types of sanctions that can be applied
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SanctionType {
    /// Temporary kick from current game
    Kick,
    /// Short-term ban (e.g., 5 minutes)
    ShortBan,
    /// Medium-term ban (e.g., 1 hour)
    MediumBan,
    /// Long-term ban (e.g., 24 hours)
    LongBan,
    /// Permanent ban
    PermanentBan,
}

impl SanctionType {
    /// Get the duration of this sanction type
    pub fn duration(&self) -> Option<Duration> {
        match self {
            SanctionType::Kick => Some(Duration::from_secs(0)),
            SanctionType::ShortBan => Some(Duration::from_secs(5 * 60)),       // 5 minutes
            SanctionType::MediumBan => Some(Duration::from_secs(60 * 60)),     // 1 hour
            SanctionType::LongBan => Some(Duration::from_secs(24 * 60 * 60)),  // 24 hours
            SanctionType::PermanentBan => None,                                 // Permanent
        }
    }
}

/// Reason for a sanction
#[derive(Debug, Clone)]
pub enum SanctionReason {
    /// Cheat detected (specify which)
    CheatDetected(String),
    /// Rate limiting violation
    RateLimitViolation,
    /// Behavioral analysis flag
    SuspiciousBehavior(String),
    /// DoS attack attempt
    DoSAttempt,
    /// Invalid input spam
    InvalidInputSpam,
    /// Manual ban by admin
    ManualBan(String),
}

impl std::fmt::Display for SanctionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SanctionReason::CheatDetected(cheat) => write!(f, "Cheat detected: {}", cheat),
            SanctionReason::RateLimitViolation => write!(f, "Rate limit violation"),
            SanctionReason::SuspiciousBehavior(behavior) => {
                write!(f, "Suspicious behavior: {}", behavior)
            }
            SanctionReason::DoSAttempt => write!(f, "DoS attack attempt"),
            SanctionReason::InvalidInputSpam => write!(f, "Invalid input spam"),
            SanctionReason::ManualBan(reason) => write!(f, "Manual ban: {}", reason),
        }
    }
}

/// A ban record
#[derive(Debug, Clone)]
pub struct BanRecord {
    pub player_id: Option<PlayerId>,
    pub ip_address: Option<IpAddr>,
    pub sanction_type: SanctionType,
    pub reason: SanctionReason,
    pub created_at: Instant,
    pub expires_at: Option<Instant>,
    pub violation_count: u32,
}

impl BanRecord {
    pub fn new(
        player_id: Option<PlayerId>,
        ip_address: Option<IpAddr>,
        sanction_type: SanctionType,
        reason: SanctionReason,
    ) -> Self {
        let now = Instant::now();
        let expires_at = sanction_type.duration().map(|d| now + d);

        Self {
            player_id,
            ip_address,
            sanction_type,
            reason,
            created_at: now,
            expires_at,
            violation_count: 1,
        }
    }

    /// Check if ban has expired
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(expires) => Instant::now() >= expires,
            None => false, // Permanent ban never expires
        }
    }

    /// Get remaining ban duration
    pub fn remaining(&self) -> Option<Duration> {
        match self.expires_at {
            Some(expires) => {
                let now = Instant::now();
                if now >= expires {
                    Some(Duration::ZERO)
                } else {
                    Some(expires - now)
                }
            }
            None => None, // Permanent
        }
    }
}

/// Ban list managing all bans
pub struct BanList {
    /// Bans by player ID
    player_bans: HashMap<PlayerId, BanRecord>,
    /// Bans by IP address
    ip_bans: HashMap<IpAddr, BanRecord>,
    /// Violation history by player ID (for escalation)
    violation_history: HashMap<PlayerId, Vec<(Instant, SanctionReason)>>,
    /// Configuration
    escalation_window: Duration,
    violations_for_escalation: u32,
}

impl BanList {
    pub fn new() -> Self {
        Self {
            player_bans: HashMap::new(),
            ip_bans: HashMap::new(),
            violation_history: HashMap::new(),
            escalation_window: Duration::from_secs(24 * 60 * 60), // 24 hours
            violations_for_escalation: 3,
        }
    }

    /// Add a ban
    pub fn add_ban(&mut self, record: BanRecord) {
        if let Some(player_id) = record.player_id {
            self.player_bans.insert(player_id, record.clone());

            // Record violation history
            self.violation_history
                .entry(player_id)
                .or_default()
                .push((Instant::now(), record.reason.clone()));
        }

        if let Some(ip) = record.ip_address {
            self.ip_bans.insert(ip, record);
        }
    }

    /// Check if a player is banned
    pub fn is_player_banned(&self, player_id: PlayerId) -> Option<&BanRecord> {
        self.player_bans.get(&player_id).filter(|b| !b.is_expired())
    }

    /// Check if an IP is banned
    pub fn is_ip_banned(&self, ip: IpAddr) -> Option<&BanRecord> {
        self.ip_bans.get(&ip).filter(|b| !b.is_expired())
    }

    /// Check if either player or IP is banned
    pub fn is_banned(&self, player_id: Option<PlayerId>, ip: Option<IpAddr>) -> Option<&BanRecord> {
        if let Some(pid) = player_id {
            if let Some(ban) = self.is_player_banned(pid) {
                return Some(ban);
            }
        }

        if let Some(ip_addr) = ip {
            if let Some(ban) = self.is_ip_banned(ip_addr) {
                return Some(ban);
            }
        }

        None
    }

    /// Remove a player ban
    pub fn remove_player_ban(&mut self, player_id: PlayerId) -> Option<BanRecord> {
        self.player_bans.remove(&player_id)
    }

    /// Remove an IP ban
    pub fn remove_ip_ban(&mut self, ip: IpAddr) -> Option<BanRecord> {
        self.ip_bans.remove(&ip)
    }

    /// Get escalated sanction type based on history
    /// Note: This counts existing violations. The caller adds the current violation after.
    pub fn get_escalated_sanction(&self, player_id: PlayerId) -> SanctionType {
        if let Some(history) = self.violation_history.get(&player_id) {
            // Count recent violations (including the one we're about to add)
            let now = Instant::now();
            let recent_count = history
                .iter()
                .filter(|(time, _)| now.duration_since(*time) < self.escalation_window)
                .count() as u32
                + 1; // +1 for the current violation being processed

            match recent_count {
                1 => SanctionType::Kick,
                2 => SanctionType::ShortBan,
                3 => SanctionType::MediumBan,
                4..=5 => SanctionType::LongBan,
                _ => SanctionType::PermanentBan,
            }
        } else {
            SanctionType::Kick
        }
    }

    /// Apply a sanction with automatic escalation
    pub fn apply_sanction(
        &mut self,
        player_id: PlayerId,
        ip: Option<IpAddr>,
        reason: SanctionReason,
    ) -> SanctionType {
        let sanction_type = self.get_escalated_sanction(player_id);

        let record = BanRecord::new(Some(player_id), ip, sanction_type, reason);
        self.add_ban(record);

        sanction_type
    }

    /// Clean up expired bans
    pub fn cleanup_expired(&mut self) -> usize {
        let before = self.player_bans.len() + self.ip_bans.len();

        self.player_bans.retain(|_, ban| !ban.is_expired());
        self.ip_bans.retain(|_, ban| !ban.is_expired());

        // Also clean up old violation history
        let cutoff = Instant::now() - self.escalation_window;
        for history in self.violation_history.values_mut() {
            history.retain(|(time, _)| *time > cutoff);
        }
        self.violation_history.retain(|_, v| !v.is_empty());

        let after = self.player_bans.len() + self.ip_bans.len();
        before - after
    }

    /// Get total ban count
    pub fn total_bans(&self) -> usize {
        self.player_bans.len() + self.ip_bans.len()
    }

    /// Get active (non-expired) ban count
    pub fn active_bans(&self) -> usize {
        let player_active = self.player_bans.values().filter(|b| !b.is_expired()).count();
        let ip_active = self.ip_bans.values().filter(|b| !b.is_expired()).count();
        player_active + ip_active
    }
}

impl Default for BanList {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn test_player_id() -> PlayerId {
        uuid::Uuid::new_v4()
    }

    fn test_ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))
    }

    #[test]
    fn test_sanction_type_duration() {
        assert!(SanctionType::Kick.duration().unwrap().is_zero());
        assert!(SanctionType::ShortBan.duration().unwrap() > Duration::ZERO);
        assert!(SanctionType::PermanentBan.duration().is_none());
    }

    #[test]
    fn test_ban_record_expiry() {
        let record = BanRecord::new(
            Some(test_player_id()),
            None,
            SanctionType::Kick,
            SanctionReason::RateLimitViolation,
        );

        // Kick expires immediately
        assert!(record.is_expired());
    }

    #[test]
    fn test_ban_record_permanent() {
        let record = BanRecord::new(
            Some(test_player_id()),
            None,
            SanctionType::PermanentBan,
            SanctionReason::ManualBan("Test".to_string()),
        );

        // Permanent never expires
        assert!(!record.is_expired());
        assert!(record.remaining().is_none());
    }

    #[test]
    fn test_ban_list_add_player_ban() {
        let mut list = BanList::new();
        let player_id = test_player_id();

        let record = BanRecord::new(
            Some(player_id),
            None,
            SanctionType::ShortBan,
            SanctionReason::CheatDetected("Test".to_string()),
        );

        list.add_ban(record);

        assert!(list.is_player_banned(player_id).is_some());
    }

    #[test]
    fn test_ban_list_add_ip_ban() {
        let mut list = BanList::new();
        let ip = test_ip();

        let record = BanRecord::new(
            None,
            Some(ip),
            SanctionType::ShortBan,
            SanctionReason::DoSAttempt,
        );

        list.add_ban(record);

        assert!(list.is_ip_banned(ip).is_some());
    }

    #[test]
    fn test_ban_list_is_banned_checks_both() {
        let mut list = BanList::new();
        let player_id = test_player_id();
        let ip = test_ip();

        // Ban IP only
        let record = BanRecord::new(
            None,
            Some(ip),
            SanctionType::ShortBan,
            SanctionReason::DoSAttempt,
        );
        list.add_ban(record);

        // Different player but same IP should be caught
        let result = list.is_banned(Some(test_player_id()), Some(ip));
        assert!(result.is_some());

        // Different IP, unbanned player should pass
        let other_ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let result = list.is_banned(Some(player_id), Some(other_ip));
        assert!(result.is_none());
    }

    #[test]
    fn test_ban_list_remove() {
        let mut list = BanList::new();
        let player_id = test_player_id();

        let record = BanRecord::new(
            Some(player_id),
            None,
            SanctionType::ShortBan,
            SanctionReason::RateLimitViolation,
        );
        list.add_ban(record);

        let removed = list.remove_player_ban(player_id);
        assert!(removed.is_some());
        assert!(list.is_player_banned(player_id).is_none());
    }

    #[test]
    fn test_escalation() {
        let mut list = BanList::new();
        let player_id = test_player_id();

        // First violation
        let s1 = list.apply_sanction(player_id, None, SanctionReason::RateLimitViolation);
        assert_eq!(s1, SanctionType::Kick);

        // Second violation
        let s2 = list.apply_sanction(player_id, None, SanctionReason::InvalidInputSpam);
        assert_eq!(s2, SanctionType::ShortBan);

        // Third violation
        let s3 = list.apply_sanction(player_id, None, SanctionReason::RateLimitViolation);
        assert_eq!(s3, SanctionType::MediumBan);
    }

    #[test]
    fn test_cleanup_expired() {
        let mut list = BanList::new();

        // Add a kick (expires immediately)
        let record = BanRecord::new(
            Some(test_player_id()),
            None,
            SanctionType::Kick,
            SanctionReason::RateLimitViolation,
        );
        list.add_ban(record);

        let cleaned = list.cleanup_expired();
        assert_eq!(cleaned, 1);
        assert_eq!(list.total_bans(), 0);
    }

    #[test]
    fn test_active_bans_count() {
        let mut list = BanList::new();

        // Add expired kick
        let record1 = BanRecord::new(
            Some(test_player_id()),
            None,
            SanctionType::Kick,
            SanctionReason::RateLimitViolation,
        );
        list.add_ban(record1);

        // Add non-expired ban
        let record2 = BanRecord::new(
            Some(test_player_id()),
            None,
            SanctionType::ShortBan,
            SanctionReason::CheatDetected("test".to_string()),
        );
        list.add_ban(record2);

        assert_eq!(list.total_bans(), 2);
        assert_eq!(list.active_bans(), 1);
    }

    #[test]
    fn test_sanction_reason_display() {
        let reason = SanctionReason::CheatDetected("Speedhack".to_string());
        assert!(reason.to_string().contains("Speedhack"));

        let reason = SanctionReason::DoSAttempt;
        assert!(reason.to_string().contains("DoS"));
    }
}
