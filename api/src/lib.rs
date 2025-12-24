//! Orbit Royale Server Library
//!
//! A real-time multiplayer game server using WebTransport.
//!
//! # Features
//!
//! - `anticheat` - Anti-cheat system with input validation, rate limiting, and behavior analysis (enabled by default)
//! - `lobby` - Advanced lobby system with rooms, matchmaking, and session management (enabled by default)
//! - `minimal` - Build without optional features for testing/debugging

pub mod config;
pub mod util;
pub mod game;
pub mod net;
pub mod metrics;

// Feature-gated modules (enabled by default)
#[cfg(feature = "lobby")]
pub mod lobby;

#[cfg(feature = "anticheat")]
pub mod anticheat;

// AI Simulation Manager (optional, requires API key)
#[cfg(feature = "ai_manager")]
pub mod ai_manager;
