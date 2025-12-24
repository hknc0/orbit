//! Projectile system for player mass ejection
//!
//! Provides charge-based firing mechanics with event tracking.
//! The `fire_direct` function is available for AI/scripted firing.

#![allow(dead_code)] // API functions kept for future use

use crate::game::constants::eject::*;
use crate::game::constants::mass::MINIMUM;
use crate::game::state::{GameState, PlayerId};
use crate::net::protocol::PlayerInput;
use crate::util::vec2::Vec2;

/// Projectile events for game event system
#[derive(Debug, Clone)]
pub enum ProjectileEvent {
    /// Projectile was fired - fields available for event handlers
    Fired {
        owner_id: PlayerId,
        projectile_id: u64,
        position: Vec2,
        velocity: Vec2,
        mass: f32,
    },
}

/// State for tracking player's charge
#[derive(Debug, Clone, Default)]
pub struct ChargeState {
    pub is_charging: bool,
    pub charge_time: f32,
    pub aim_direction: Vec2,
}

impl ChargeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get charge progress (0.0 to 1.0)
    pub fn charge_progress(&self) -> f32 {
        if !self.is_charging {
            return 0.0;
        }
        ((self.charge_time - MIN_CHARGE_TIME) / (MAX_CHARGE_TIME - MIN_CHARGE_TIME)).clamp(0.0, 1.0)
    }

    /// Get projected mass based on charge
    pub fn projected_mass(&self, player_mass: f32) -> f32 {
        let progress = self.charge_progress();
        let mass_ratio = progress * MAX_MASS_RATIO;
        (player_mass * mass_ratio).max(MIN_MASS).min(player_mass - MINIMUM)
    }

    /// Get projected velocity based on charge
    /// Quick tap = fast/small, Full charge = slow/big (like orbit-poc)
    pub fn projected_velocity(&self) -> f32 {
        let progress = self.charge_progress();
        // Inverted: less charge = faster projectile
        MAX_VELOCITY - progress * (MAX_VELOCITY - MIN_VELOCITY)
    }
}

/// Player charge states (stored separately for efficiency)
pub struct ChargeManager {
    charges: std::collections::HashMap<PlayerId, ChargeState>,
}

impl ChargeManager {
    pub fn new() -> Self {
        Self {
            charges: std::collections::HashMap::new(),
        }
    }

    pub fn get(&self, player_id: PlayerId) -> Option<&ChargeState> {
        self.charges.get(&player_id)
    }

    pub fn get_mut(&mut self, player_id: PlayerId) -> &mut ChargeState {
        self.charges.entry(player_id).or_default()
    }

    pub fn remove(&mut self, player_id: PlayerId) {
        self.charges.remove(&player_id);
    }

    pub fn reset(&mut self, player_id: PlayerId) {
        if let Some(charge) = self.charges.get_mut(&player_id) {
            *charge = ChargeState::default();
        }
    }
}

impl Default for ChargeManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Process player fire input and update charge state
pub fn process_input(
    state: &mut GameState,
    player_id: PlayerId,
    input: &PlayerInput,
    charge_manager: &mut ChargeManager,
    dt: f32,
) -> Option<ProjectileEvent> {
    let player = state.get_player(player_id)?;
    if !player.alive {
        return None;
    }

    let charge = charge_manager.get_mut(player_id);

    if input.fire {
        // Start or continue charging
        if !charge.is_charging {
            charge.is_charging = true;
            charge.charge_time = 0.0;
        }

        charge.charge_time = (charge.charge_time + dt).min(MAX_CHARGE_TIME);

        // Update aim direction while charging
        if input.aim.length_sq() > 0.01 {
            charge.aim_direction = input.aim.normalize();
        }

        None
    } else if input.fire_released && charge.is_charging {
        // Fire released - always fire (even quick taps)
        // Quick taps fire smaller/faster projectiles
        let event = fire_projectile(state, player_id, charge);
        charge_manager.reset(player_id);
        event
    } else {
        // Not firing
        if charge.is_charging {
            charge_manager.reset(player_id);
        }
        None
    }
}

/// Fire a projectile from a player
fn fire_projectile(
    state: &mut GameState,
    player_id: PlayerId,
    charge: &ChargeState,
) -> Option<ProjectileEvent> {
    let player = state.get_player_mut(player_id)?;

    // Calculate projectile properties
    let mass = charge.projected_mass(player.mass);
    let speed = charge.projected_velocity();

    // Ensure player has enough mass
    if player.mass - mass < MINIMUM {
        return None;
    }

    // Deduct mass from player
    player.mass -= mass;

    // Calculate spawn position (at edge of player)
    let direction = if charge.aim_direction.length_sq() > 0.01 {
        charge.aim_direction.normalize()
    } else {
        Vec2::from_angle(player.rotation)
    };

    let player_radius = crate::game::constants::mass_to_radius(player.mass + mass);
    let spawn_offset = player_radius + 5.0;
    let position = player.position + direction * spawn_offset;

    // Calculate velocity (relative to player + ejection speed)
    let velocity = player.velocity + direction * speed;

    // Apply recoil to player
    let recoil = direction * (-speed * mass / player.mass * 0.5);
    player.velocity += recoil;

    // Create projectile
    let projectile_id = state.add_projectile(player_id, position, velocity, mass);

    Some(ProjectileEvent::Fired {
        owner_id: player_id,
        projectile_id,
        position,
        velocity,
        mass,
    })
}

/// Direct fire function (without charge, for AI)
pub fn fire_direct(
    state: &mut GameState,
    player_id: PlayerId,
    direction: Vec2,
    charge_amount: f32,
) -> Option<ProjectileEvent> {
    let player = state.get_player(player_id)?;
    if !player.alive {
        return None;
    }

    // Create artificial charge state
    let mut charge = ChargeState::new();
    charge.is_charging = true;
    charge.charge_time = MIN_CHARGE_TIME + charge_amount * (MAX_CHARGE_TIME - MIN_CHARGE_TIME);
    charge.aim_direction = direction.normalize();

    fire_projectile(state, player_id, &charge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::state::Player;
    use uuid::Uuid;

    fn create_test_state() -> (GameState, Uuid) {
        let mut state = GameState::new();
        let player_id = Uuid::new_v4();
        let player = Player {
            id: player_id,
            name: "Test".to_string(),
            position: Vec2::new(300.0, 0.0),
            velocity: Vec2::ZERO,
            rotation: 0.0,
            mass: 100.0,
            alive: true,
            kills: 0,
            deaths: 0,
            spawn_protection: 0.0,
            is_bot: false,
            color_index: 0,
            respawn_timer: 0.0,
        };
        state.add_player(player);
        (state, player_id)
    }

    #[test]
    fn test_charge_state_default() {
        let charge = ChargeState::default();
        assert!(!charge.is_charging);
        assert_eq!(charge.charge_time, 0.0);
    }

    #[test]
    fn test_charge_progress() {
        let mut charge = ChargeState::new();
        charge.is_charging = true;
        charge.charge_time = MIN_CHARGE_TIME;

        assert!((charge.charge_progress() - 0.0).abs() < 0.01);

        charge.charge_time = MAX_CHARGE_TIME;
        assert!((charge.charge_progress() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_projected_mass() {
        let mut charge = ChargeState::new();
        charge.is_charging = true;
        charge.charge_time = MAX_CHARGE_TIME;

        let mass = charge.projected_mass(100.0);

        // Should be MAX_MASS_RATIO of player mass
        assert!(mass > 0.0);
        assert!(mass <= 100.0 * MAX_MASS_RATIO);
    }

    #[test]
    fn test_projected_velocity() {
        let mut charge = ChargeState::new();
        charge.is_charging = true;

        // Quick tap = fast projectile (MAX_VELOCITY)
        charge.charge_time = MIN_CHARGE_TIME;
        let v_tap = charge.projected_velocity();
        assert!((v_tap - MAX_VELOCITY).abs() < 1.0);

        // Full charge = slow projectile (MIN_VELOCITY)
        charge.charge_time = MAX_CHARGE_TIME;
        let v_full = charge.projected_velocity();
        assert!((v_full - MIN_VELOCITY).abs() < 1.0);
    }

    #[test]
    fn test_charge_manager() {
        let mut manager = ChargeManager::new();
        let player_id = Uuid::new_v4();

        // Get creates default
        let charge = manager.get_mut(player_id);
        charge.is_charging = true;

        // Get returns same state
        let charge2 = manager.get(player_id).unwrap();
        assert!(charge2.is_charging);

        // Reset clears state
        manager.reset(player_id);
        let charge3 = manager.get(player_id).unwrap();
        assert!(!charge3.is_charging);
    }

    #[test]
    fn test_fire_input_starts_charging() {
        let (mut state, player_id) = create_test_state();
        let mut charge_manager = ChargeManager::new();

        let input = PlayerInput {
            fire: true,
            aim: Vec2::new(1.0, 0.0),
            ..Default::default()
        };

        process_input(&mut state, player_id, &input, &mut charge_manager, 0.1);

        let charge = charge_manager.get(player_id).unwrap();
        assert!(charge.is_charging);
    }

    #[test]
    fn test_fire_release_creates_projectile() {
        let (mut state, player_id) = create_test_state();
        let mut charge_manager = ChargeManager::new();

        // Start charging
        let input1 = PlayerInput {
            fire: true,
            aim: Vec2::new(1.0, 0.0),
            ..Default::default()
        };

        // Charge for enough time
        for _ in 0..10 {
            process_input(&mut state, player_id, &input1, &mut charge_manager, 0.1);
        }

        // Release
        let input2 = PlayerInput {
            fire: false,
            fire_released: true,
            aim: Vec2::new(1.0, 0.0),
            ..Default::default()
        };

        let event = process_input(&mut state, player_id, &input2, &mut charge_manager, 0.1);

        assert!(event.is_some());
        assert_eq!(state.projectiles.len(), 1);
    }

    #[test]
    fn test_quick_tap_fires_small_fast_projectile() {
        let (mut state, player_id) = create_test_state();
        let initial_mass = state.get_player(player_id).unwrap().mass;
        let mut charge_manager = ChargeManager::new();

        // Brief charge (quick tap)
        let input1 = PlayerInput {
            fire: true,
            aim: Vec2::new(1.0, 0.0),
            ..Default::default()
        };
        process_input(&mut state, player_id, &input1, &mut charge_manager, 0.01);

        // Immediate release
        let input2 = PlayerInput {
            fire: false,
            fire_released: true,
            aim: Vec2::new(1.0, 0.0),
            ..Default::default()
        };
        let event = process_input(&mut state, player_id, &input2, &mut charge_manager, 0.1);

        // Quick taps should fire a small projectile
        assert!(event.is_some());
        assert_eq!(state.projectiles.len(), 1);

        // Player should have lost MIN_MASS
        assert!(state.get_player(player_id).unwrap().mass < initial_mass);
    }

    #[test]
    fn test_firing_deducts_mass() {
        let (mut state, player_id) = create_test_state();
        let initial_mass = state.get_player(player_id).unwrap().mass;
        let mut charge_manager = ChargeManager::new();

        // Charge
        {
            let charge = charge_manager.get_mut(player_id);
            charge.is_charging = true;
            charge.charge_time = MAX_CHARGE_TIME;
            charge.aim_direction = Vec2::new(1.0, 0.0);
        }

        // Release
        let input = PlayerInput {
            fire: false,
            fire_released: true,
            aim: Vec2::new(1.0, 0.0),
            ..Default::default()
        };

        process_input(&mut state, player_id, &input, &mut charge_manager, 0.1);

        assert!(state.get_player(player_id).unwrap().mass < initial_mass);
    }

    #[test]
    fn test_fire_direct() {
        let (mut state, player_id) = create_test_state();

        let event = fire_direct(&mut state, player_id, Vec2::new(1.0, 0.0), 0.5);

        assert!(event.is_some());
        assert_eq!(state.projectiles.len(), 1);
    }

    #[test]
    fn test_dead_player_cannot_fire() {
        let (mut state, player_id) = create_test_state();
        state.get_player_mut(player_id).unwrap().alive = false;

        let event = fire_direct(&mut state, player_id, Vec2::new(1.0, 0.0), 1.0);

        assert!(event.is_none());
    }

    #[test]
    fn test_recoil_applied() {
        let (mut state, player_id) = create_test_state();
        state.get_player_mut(player_id).unwrap().velocity = Vec2::ZERO;

        fire_direct(&mut state, player_id, Vec2::new(1.0, 0.0), 0.5);

        // Player should have negative x velocity (recoil)
        assert!(state.get_player(player_id).unwrap().velocity.x < 0.0);
    }

    #[test]
    fn test_projectile_inherits_player_velocity() {
        let (mut state, player_id) = create_test_state();
        state.get_player_mut(player_id).unwrap().velocity = Vec2::new(50.0, 0.0);

        fire_direct(&mut state, player_id, Vec2::new(0.0, 1.0), 0.5);

        // Projectile velocity should include player velocity
        assert!(state.projectiles[0].velocity.x > 0.0);
        assert!(state.projectiles[0].velocity.y > 0.0);
    }
}
