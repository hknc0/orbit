//! Prometheus-compatible metrics endpoint
//!
//! Exposes game server metrics in Prometheus format for Grafana dashboards.
//! Default endpoint: http://localhost:9090/metrics

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{info, warn, debug};

/// Metrics registry for the game server
#[derive(Debug)]
pub struct Metrics {
    // Player counts
    pub total_players: AtomicU64,
    pub human_players: AtomicU64,
    pub bot_players: AtomicU64,
    pub alive_players: AtomicU64,

    // Entity counts
    pub projectile_count: AtomicU64,
    pub debris_count: AtomicU64,
    pub gravity_well_count: AtomicU64,

    // Tick timing (microseconds)
    pub tick_time_us: AtomicU64,
    pub tick_time_p95_us: AtomicU64,
    pub tick_time_p99_us: AtomicU64,
    pub tick_time_max_us: AtomicU64,

    // Performance status (0=Excellent, 1=Good, 2=Warning, 3=Critical, 4=Catastrophic)
    pub performance_status: AtomicU64,
    pub budget_usage_percent: AtomicU64,

    // Tick counter
    pub tick_count: AtomicU64,

    // Network stats
    pub connections_active: AtomicU64,
    pub messages_sent: AtomicU64,
    pub messages_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,

    // Game state
    pub match_time_seconds: AtomicU64,
    pub arena_scale: AtomicU64, // Stored as scale * 100 (e.g., 1.5 = 150)
    pub arena_radius: AtomicU64, // Arena escape radius in world units
    pub arena_gravity_wells: AtomicU64, // Number of gravity wells

    // Server uptime
    start_time: Instant,

    // Simulation mode metrics
    pub simulation_enabled: AtomicU64,      // 0 or 1
    pub simulation_target_bots: AtomicU64,  // Current target bot count
    pub simulation_cycle_progress: AtomicU64, // Progress through cycle (0-100)

    // Rolling tick times for percentile calculation (VecDeque for O(1) pop_front)
    tick_history: RwLock<VecDeque<u64>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            total_players: AtomicU64::new(0),
            human_players: AtomicU64::new(0),
            bot_players: AtomicU64::new(0),
            alive_players: AtomicU64::new(0),
            projectile_count: AtomicU64::new(0),
            debris_count: AtomicU64::new(0),
            gravity_well_count: AtomicU64::new(0),
            tick_time_us: AtomicU64::new(0),
            tick_time_p95_us: AtomicU64::new(0),
            tick_time_p99_us: AtomicU64::new(0),
            tick_time_max_us: AtomicU64::new(0),
            performance_status: AtomicU64::new(0),
            budget_usage_percent: AtomicU64::new(0),
            tick_count: AtomicU64::new(0),
            connections_active: AtomicU64::new(0),
            messages_sent: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            match_time_seconds: AtomicU64::new(0),
            arena_scale: AtomicU64::new(100),
            arena_radius: AtomicU64::new(0),
            arena_gravity_wells: AtomicU64::new(0),
            start_time: Instant::now(),
            simulation_enabled: AtomicU64::new(0),
            simulation_target_bots: AtomicU64::new(0),
            simulation_cycle_progress: AtomicU64::new(0),
            tick_history: RwLock::new(VecDeque::with_capacity(1000)),
        }
    }

    /// Record a tick time and update percentiles
    pub fn record_tick_time(&self, duration: Duration) {
        let us = duration.as_micros() as u64;
        self.tick_time_us.store(us, Ordering::Relaxed);
        self.tick_count.fetch_add(1, Ordering::Relaxed);

        // Update rolling history for percentiles
        let mut history = self.tick_history.write();
        history.push_back(us);

        // Keep last 1000 samples - O(1) with VecDeque
        while history.len() > 1000 {
            history.pop_front();
        }

        // Calculate percentiles
        if history.len() >= 10 {
            let mut sorted: Vec<u64> = history.iter().copied().collect();
            sorted.sort_unstable();

            let p95_idx = (sorted.len() as f32 * 0.95) as usize;
            let p99_idx = (sorted.len() as f32 * 0.99) as usize;

            self.tick_time_p95_us.store(sorted[p95_idx.min(sorted.len() - 1)], Ordering::Relaxed);
            self.tick_time_p99_us.store(sorted[p99_idx.min(sorted.len() - 1)], Ordering::Relaxed);
            self.tick_time_max_us.store(sorted.last().copied().unwrap_or(0), Ordering::Relaxed);
        }
    }

    /// Get uptime in seconds
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Generate Prometheus-format metrics output
    pub fn to_prometheus(&self) -> String {
        let mut output = String::with_capacity(4096);

        // Helper macro for metrics
        macro_rules! metric {
            ($name:expr, $help:expr, $type:expr, $value:expr) => {
                output.push_str(&format!(
                    "# HELP {} {}\n# TYPE {} {}\n{} {}\n",
                    $name, $help, $name, $type, $name, $value
                ));
            };
        }

        // Player metrics
        metric!("orbit_royale_players_total", "Total number of players", "gauge",
            self.total_players.load(Ordering::Relaxed));
        metric!("orbit_royale_players_human", "Number of human players", "gauge",
            self.human_players.load(Ordering::Relaxed));
        metric!("orbit_royale_players_bot", "Number of bot players", "gauge",
            self.bot_players.load(Ordering::Relaxed));
        metric!("orbit_royale_players_alive", "Number of alive players", "gauge",
            self.alive_players.load(Ordering::Relaxed));

        // Entity metrics
        metric!("orbit_royale_projectiles", "Number of active projectiles", "gauge",
            self.projectile_count.load(Ordering::Relaxed));
        metric!("orbit_royale_debris", "Number of debris entities", "gauge",
            self.debris_count.load(Ordering::Relaxed));
        metric!("orbit_royale_gravity_wells", "Number of gravity wells", "gauge",
            self.gravity_well_count.load(Ordering::Relaxed));

        // Performance metrics
        metric!("orbit_royale_tick_time_microseconds", "Current tick time in microseconds", "gauge",
            self.tick_time_us.load(Ordering::Relaxed));
        metric!("orbit_royale_tick_time_p95_microseconds", "95th percentile tick time", "gauge",
            self.tick_time_p95_us.load(Ordering::Relaxed));
        metric!("orbit_royale_tick_time_p99_microseconds", "99th percentile tick time", "gauge",
            self.tick_time_p99_us.load(Ordering::Relaxed));
        metric!("orbit_royale_tick_time_max_microseconds", "Maximum tick time", "gauge",
            self.tick_time_max_us.load(Ordering::Relaxed));
        metric!("orbit_royale_tick_count", "Total ticks processed", "counter",
            self.tick_count.load(Ordering::Relaxed));

        // Budget metrics
        metric!("orbit_royale_performance_status", "Performance status (0=Excellent, 4=Catastrophic)", "gauge",
            self.performance_status.load(Ordering::Relaxed));
        metric!("orbit_royale_budget_usage_percent", "Tick budget usage percentage", "gauge",
            self.budget_usage_percent.load(Ordering::Relaxed));

        // Human-readable performance status as a label
        let status_name = match self.performance_status.load(Ordering::Relaxed) {
            0 => "excellent",
            1 => "good",
            2 => "warning",
            3 => "critical",
            _ => "catastrophic",
        };
        output.push_str(&format!(
            "# HELP orbit_royale_performance_state Human-readable performance state\n# TYPE orbit_royale_performance_state gauge\norbit_royale_performance_state{{state=\"{}\"}} 1\n",
            status_name
        ));

        // Network metrics
        metric!("orbit_royale_connections_active", "Active WebTransport connections", "gauge",
            self.connections_active.load(Ordering::Relaxed));
        metric!("orbit_royale_messages_sent_total", "Total messages sent", "counter",
            self.messages_sent.load(Ordering::Relaxed));
        metric!("orbit_royale_messages_received_total", "Total messages received", "counter",
            self.messages_received.load(Ordering::Relaxed));
        metric!("orbit_royale_bytes_sent_total", "Total bytes sent", "counter",
            self.bytes_sent.load(Ordering::Relaxed));
        metric!("orbit_royale_bytes_received_total", "Total bytes received", "counter",
            self.bytes_received.load(Ordering::Relaxed));

        // Game state
        metric!("orbit_royale_match_time_seconds", "Current match time", "gauge",
            self.match_time_seconds.load(Ordering::Relaxed));
        metric!("orbit_royale_arena_scale", "Arena scale factor (x100)", "gauge",
            self.arena_scale.load(Ordering::Relaxed));
        metric!("orbit_royale_arena_radius", "Arena escape radius in world units", "gauge",
            self.arena_radius.load(Ordering::Relaxed));
        metric!("orbit_royale_arena_gravity_wells", "Number of gravity wells", "gauge",
            self.arena_gravity_wells.load(Ordering::Relaxed));
        metric!("orbit_royale_uptime_seconds", "Server uptime in seconds", "counter",
            self.uptime_seconds());

        // Simulation mode metrics
        metric!("orbit_royale_simulation_enabled", "Simulation mode enabled (0/1)", "gauge",
            self.simulation_enabled.load(Ordering::Relaxed));
        metric!("orbit_royale_simulation_target_bots", "Current target bot count in simulation", "gauge",
            self.simulation_target_bots.load(Ordering::Relaxed));
        metric!("orbit_royale_simulation_cycle_progress", "Simulation cycle progress (0-100%)", "gauge",
            self.simulation_cycle_progress.load(Ordering::Relaxed));

        output
    }

    /// Generate JSON format metrics (alternative for direct API access)
    pub fn to_json(&self) -> String {
        format!(r#"{{
  "players": {{
    "total": {},
    "human": {},
    "bot": {},
    "alive": {}
  }},
  "entities": {{
    "projectiles": {},
    "debris": {},
    "gravity_wells": {}
  }},
  "performance": {{
    "tick_time_us": {},
    "tick_time_p95_us": {},
    "tick_time_p99_us": {},
    "tick_time_max_us": {},
    "tick_count": {},
    "status": {},
    "status_name": "{}",
    "budget_percent": {}
  }},
  "network": {{
    "connections": {},
    "messages_sent": {},
    "messages_received": {},
    "bytes_sent": {},
    "bytes_received": {}
  }},
  "game": {{
    "match_time_seconds": {},
    "arena_scale": {},
    "uptime_seconds": {}
  }}
}}"#,
            self.total_players.load(Ordering::Relaxed),
            self.human_players.load(Ordering::Relaxed),
            self.bot_players.load(Ordering::Relaxed),
            self.alive_players.load(Ordering::Relaxed),
            self.projectile_count.load(Ordering::Relaxed),
            self.debris_count.load(Ordering::Relaxed),
            self.gravity_well_count.load(Ordering::Relaxed),
            self.tick_time_us.load(Ordering::Relaxed),
            self.tick_time_p95_us.load(Ordering::Relaxed),
            self.tick_time_p99_us.load(Ordering::Relaxed),
            self.tick_time_max_us.load(Ordering::Relaxed),
            self.tick_count.load(Ordering::Relaxed),
            self.performance_status.load(Ordering::Relaxed),
            match self.performance_status.load(Ordering::Relaxed) {
                0 => "excellent",
                1 => "good",
                2 => "warning",
                3 => "critical",
                _ => "catastrophic",
            },
            self.budget_usage_percent.load(Ordering::Relaxed),
            self.connections_active.load(Ordering::Relaxed),
            self.messages_sent.load(Ordering::Relaxed),
            self.messages_received.load(Ordering::Relaxed),
            self.bytes_sent.load(Ordering::Relaxed),
            self.bytes_received.load(Ordering::Relaxed),
            self.match_time_seconds.load(Ordering::Relaxed),
            self.arena_scale.load(Ordering::Relaxed) as f32 / 100.0,
            self.uptime_seconds(),
        )
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Start the metrics HTTP server
pub async fn start_metrics_server(metrics: Arc<Metrics>, port: u16) -> anyhow::Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;

    info!("Metrics server listening on http://{}/metrics", addr);

    loop {
        let (mut socket, peer) = listener.accept().await?;
        let metrics = metrics.clone();

        tokio::spawn(async move {
            let mut buffer = [0u8; 1024];

            match socket.read(&mut buffer).await {
                Ok(n) if n > 0 => {
                    let request = String::from_utf8_lossy(&buffer[..n]);

                    // Parse the request line
                    let response = if request.starts_with("GET /metrics") {
                        let body = metrics.to_prometheus();
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    } else if request.starts_with("GET /metrics/json") || request.starts_with("GET /json") {
                        let body = metrics.to_json();
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    } else if request.starts_with("GET /health") || request.starts_with("GET /") {
                        let body = "OK";
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    } else {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                    };

                    if let Err(e) = socket.write_all(response.as_bytes()).await {
                        debug!("Failed to write metrics response to {}: {}", peer, e);
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    debug!("Failed to read from metrics socket {}: {}", peer, e);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_new() {
        let metrics = Metrics::new();
        assert_eq!(metrics.total_players.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.tick_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_record_tick_time() {
        let metrics = Metrics::new();

        // Record some tick times
        for i in 0..100 {
            metrics.record_tick_time(Duration::from_micros(100 + i * 10));
        }

        assert_eq!(metrics.tick_count.load(Ordering::Relaxed), 100);
        assert!(metrics.tick_time_p95_us.load(Ordering::Relaxed) > 0);
        assert!(metrics.tick_time_p99_us.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn test_prometheus_format() {
        let metrics = Metrics::new();
        metrics.total_players.store(50, Ordering::Relaxed);
        metrics.human_players.store(5, Ordering::Relaxed);
        metrics.bot_players.store(45, Ordering::Relaxed);

        let output = metrics.to_prometheus();

        assert!(output.contains("orbit_royale_players_total 50"));
        assert!(output.contains("orbit_royale_players_human 5"));
        assert!(output.contains("orbit_royale_players_bot 45"));
        assert!(output.contains("# HELP"));
        assert!(output.contains("# TYPE"));
    }

    #[test]
    fn test_json_format() {
        let metrics = Metrics::new();
        metrics.total_players.store(100, Ordering::Relaxed);

        let output = metrics.to_json();

        assert!(output.contains("\"total\": 100"));
        assert!(output.contains("\"players\":"));
        assert!(output.contains("\"performance\":"));
    }

    #[test]
    fn test_uptime() {
        let metrics = Metrics::new();
        std::thread::sleep(Duration::from_millis(10));
        assert!(metrics.uptime_seconds() >= 0);
    }
}
