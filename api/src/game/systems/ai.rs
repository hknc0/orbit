use rand::Rng;
use rayon::prelude::*;

use crate::game::constants::ai::*;
use crate::game::state::{GameState, Player, PlayerId};
use crate::game::systems::arena::{get_zone, Zone};
use crate::net::protocol::PlayerInput;
use crate::util::vec2::Vec2;

/// AI behavior mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiBehavior {
    /// Orbit around center
    Orbit,
    /// Chase a target
    Chase,
    /// Flee from a threat
    Flee,
    /// Collect nearby debris/mass
    Collect,
    /// Idle/patrol
    Idle,
}

/// AI state for a bot
#[derive(Debug, Clone)]
pub struct AiState {
    pub behavior: AiBehavior,
    pub target_id: Option<PlayerId>,
    pub decision_timer: f32,
    pub aim_direction: Vec2,
    pub thrust_direction: Vec2,
    pub wants_boost: bool,
    pub wants_fire: bool,
    pub charge_time: f32,
    pub personality: AiPersonality,
}

/// AI personality traits
#[derive(Debug, Clone)]
pub struct AiPersonality {
    /// How likely to chase (0.0-1.0)
    pub aggression: f32,
    /// How far to stay from center (radius preference)
    pub preferred_radius: f32,
    /// How accurate the aim is (0.0-1.0)
    pub accuracy: f32,
    /// Reaction time variance
    pub reaction_variance: f32,
}

impl AiPersonality {
    pub fn random() -> Self {
        let mut rng = rand::thread_rng();
        Self {
            aggression: rng.gen_range(0.2..0.8),
            preferred_radius: rng.gen_range(250.0..400.0),
            accuracy: rng.gen_range(0.5..0.9),
            reaction_variance: rng.gen_range(0.1..0.5),
        }
    }
}

impl Default for AiPersonality {
    fn default() -> Self {
        Self {
            aggression: 0.5,
            preferred_radius: 300.0,
            accuracy: 0.7,
            reaction_variance: 0.3,
        }
    }
}

impl Default for AiState {
    fn default() -> Self {
        Self {
            behavior: AiBehavior::Idle,
            target_id: None,
            decision_timer: 0.0,
            aim_direction: Vec2::ZERO,
            thrust_direction: Vec2::ZERO,
            wants_boost: false,
            wants_fire: false,
            charge_time: 0.0,
            personality: AiPersonality::default(),
        }
    }
}

impl AiState {
    pub fn new() -> Self {
        Self {
            personality: AiPersonality::random(),
            ..Default::default()
        }
    }
}

/// AI manager for all bots
pub struct AiManager {
    states: std::collections::HashMap<PlayerId, AiState>,
}

impl AiManager {
    pub fn new() -> Self {
        Self {
            states: std::collections::HashMap::new(),
        }
    }

    pub fn register_bot(&mut self, player_id: PlayerId) {
        self.states.insert(player_id, AiState::new());
    }

    pub fn unregister_bot(&mut self, player_id: PlayerId) {
        self.states.remove(&player_id);
    }

    pub fn get(&self, player_id: PlayerId) -> Option<&AiState> {
        self.states.get(&player_id)
    }

    pub fn get_mut(&mut self, player_id: PlayerId) -> Option<&mut AiState> {
        self.states.get_mut(&player_id)
    }

    /// Update all AI decisions
    /// Uses rayon for parallel decision computation, then applies updates sequentially
    pub fn update(&mut self, state: &GameState, dt: f32) {
        // Collect current states for parallel processing
        let states_snapshot: Vec<(PlayerId, AiState)> = self.states
            .iter()
            .map(|(&id, state)| (id, state.clone()))
            .collect();

        // Compute decisions in parallel
        let decisions: Vec<(PlayerId, AiState)> = states_snapshot
            .into_par_iter()
            .map(|(bot_id, mut ai_state)| {
                update_ai_decision(&mut ai_state, bot_id, state, dt);
                (bot_id, ai_state)
            })
            .collect();

        // Apply decisions (sequential - requires mutable access)
        for (bot_id, new_state) in decisions {
            if let Some(ai_state) = self.states.get_mut(&bot_id) {
                *ai_state = new_state;
            }
        }
    }

    /// Generate input for a bot
    pub fn get_input(&self, player_id: PlayerId, tick: u64) -> Option<PlayerInput> {
        let ai = self.states.get(&player_id)?;

        Some(PlayerInput {
            sequence: tick,
            tick,
            thrust: ai.thrust_direction,
            aim: ai.aim_direction,
            boost: ai.wants_boost,
            fire: ai.wants_fire,
            fire_released: !ai.wants_fire && ai.charge_time > 0.0,
        })
    }
}

impl Default for AiManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Update AI decision for a single bot
fn update_ai_decision(ai: &mut AiState, bot_id: PlayerId, state: &GameState, dt: f32) {
    // Update decision timer
    ai.decision_timer -= dt;

    if ai.decision_timer <= 0.0 {
        // Make new decision
        ai.decision_timer = DECISION_INTERVAL * (1.0 + rand::thread_rng().gen_range(-0.2..0.2));
        decide_behavior(ai, bot_id, state);
    }

    // Execute current behavior
    execute_behavior(ai, bot_id, state, dt);
}

/// Decide which behavior to use
fn decide_behavior(ai: &mut AiState, bot_id: PlayerId, state: &GameState) {
    let bot = match state.get_player(bot_id) {
        Some(p) if p.alive => p,
        _ => return,
    };

    let mut rng = rand::thread_rng();

    // Find nearest threat and nearest target
    let (nearest_threat, nearest_target) = find_nearest_players(bot, state);

    // Check if we should flee
    if let Some((threat_id, threat_dist)) = nearest_threat {
        if let Some(threat) = state.get_player(threat_id) {
            // Flee if threat is much bigger and close
            if threat.mass > bot.mass * (1.0 / FLEE_MASS_RATIO) && threat_dist < AGGRESSION_RADIUS {
                ai.behavior = AiBehavior::Flee;
                ai.target_id = Some(threat_id);
                return;
            }
        }
    }

    // Check if we should chase
    if rng.gen::<f32>() < ai.personality.aggression {
        if let Some((target_id, target_dist)) = nearest_target {
            if let Some(target) = state.get_player(target_id) {
                // Chase if we're bigger or similar size
                if bot.mass >= target.mass * FLEE_MASS_RATIO && target_dist < AGGRESSION_RADIUS * 2.0
                {
                    ai.behavior = AiBehavior::Chase;
                    ai.target_id = Some(target_id);
                    return;
                }
            }
        }
    }

    // Check if we should collect
    if !state.debris.is_empty() || !state.projectiles.is_empty() {
        if rng.gen::<f32>() < 0.3 {
            ai.behavior = AiBehavior::Collect;
            ai.target_id = None;
            return;
        }
    }

    // Default to orbit
    ai.behavior = AiBehavior::Orbit;
    ai.target_id = None;
}

/// Execute the current behavior
fn execute_behavior(ai: &mut AiState, bot_id: PlayerId, state: &GameState, dt: f32) {
    let bot = match state.get_player(bot_id) {
        Some(p) if p.alive => p,
        _ => return,
    };

    match ai.behavior {
        AiBehavior::Orbit => execute_orbit(ai, bot, state),
        AiBehavior::Chase => execute_chase(ai, bot, state),
        AiBehavior::Flee => execute_flee(ai, bot, state),
        AiBehavior::Collect => execute_collect(ai, bot, state),
        AiBehavior::Idle => execute_idle(ai, bot),
    }

    // Update firing logic
    update_firing(ai, bot, state, dt);
}

fn execute_orbit(ai: &mut AiState, bot: &Player, state: &GameState) {
    // Find nearest gravity well to orbit around
    let nearest_well = state.arena.gravity_wells.iter()
        .min_by(|a, b| {
            let dist_a = (a.position - bot.position).length_sq();
            let dist_b = (b.position - bot.position).length_sq();
            dist_a.partial_cmp(&dist_b).unwrap_or(std::cmp::Ordering::Equal)
        });

    let well_pos = nearest_well.map(|w| w.position).unwrap_or(Vec2::ZERO);
    let to_well = well_pos - bot.position;
    let current_radius = to_well.length();
    let target_radius = ai.personality.preferred_radius;

    // Get perpendicular direction for orbit around the well (not origin)
    let tangent = to_well.perpendicular().normalize();

    // Adjust toward preferred radius from the well
    let radial = if current_radius > target_radius + 20.0 {
        to_well.normalize() * 0.5 // Move toward well
    } else if current_radius < target_radius - 20.0 {
        -to_well.normalize() * 0.5 // Move away from well
    } else {
        Vec2::ZERO
    };

    ai.thrust_direction = (tangent + radial).normalize();

    // Light boost to maintain orbit - only if significantly below orbital velocity
    let orbital_vel = crate::game::systems::gravity::orbital_velocity(current_radius);
    ai.wants_boost = bot.velocity.length() < orbital_vel * 0.6;
}

fn execute_chase(ai: &mut AiState, bot: &Player, state: &GameState) {
    let target = match ai.target_id.and_then(|id| state.get_player(id)) {
        Some(t) if t.alive => t,
        _ => {
            ai.behavior = AiBehavior::Idle;
            return;
        }
    };

    // Lead the target based on velocity
    let to_target = target.position - bot.position;
    let distance = to_target.length();
    let time_to_reach = distance / (bot.velocity.length() + 100.0);
    let predicted_pos = target.position + target.velocity * time_to_reach * 0.5;

    let chase_dir = (predicted_pos - bot.position).normalize();

    ai.thrust_direction = chase_dir;
    ai.wants_boost = distance > 100.0;
    ai.aim_direction = chase_dir;
}

fn execute_flee(ai: &mut AiState, bot: &Player, state: &GameState) {
    let threat = match ai.target_id.and_then(|id| state.get_player(id)) {
        Some(t) if t.alive => t,
        _ => {
            ai.behavior = AiBehavior::Idle;
            return;
        }
    };

    // Flee direction is away from threat
    let flee_dir = (bot.position - threat.position).normalize();

    // Also try to stay in arena
    let zone = get_zone(bot.position, &state.arena);
    let adjusted_dir = if zone == Zone::Escape || zone == Zone::Outside {
        // Blend flee direction with direction toward center
        let to_center = -bot.position.normalize();
        (flee_dir + to_center).normalize()
    } else {
        flee_dir
    };

    ai.thrust_direction = adjusted_dir;
    ai.wants_boost = true;
    ai.aim_direction = -flee_dir; // Aim at threat while fleeing
}

fn execute_collect(ai: &mut AiState, bot: &Player, state: &GameState) {
    // Find nearest collectible
    let nearest_debris = state
        .debris
        .iter()
        .map(|d| (d.position, d.position.distance_to(bot.position)))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let nearest_proj = state
        .projectiles
        .iter()
        .filter(|p| p.owner_id != bot.id)
        .map(|p| (p.position, p.position.distance_to(bot.position)))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let target_pos = match (nearest_debris, nearest_proj) {
        (Some((pos_d, dist_d)), Some((pos_p, dist_p))) => {
            if dist_d < dist_p {
                Some(pos_d)
            } else {
                Some(pos_p)
            }
        }
        (Some((pos, _)), None) => Some(pos),
        (None, Some((pos, _))) => Some(pos),
        (None, None) => None,
    };

    if let Some(pos) = target_pos {
        ai.thrust_direction = (pos - bot.position).normalize();
        ai.wants_boost = false;
    } else {
        ai.behavior = AiBehavior::Orbit;
    }
}

fn execute_idle(ai: &mut AiState, bot: &Player) {
    // Just maintain current velocity with slight adjustments
    ai.thrust_direction = Vec2::ZERO;
    ai.wants_boost = false;

    // Slowly turn to face velocity direction
    if bot.velocity.length_sq() > 10.0 {
        ai.aim_direction = bot.velocity.normalize();
    }
}

fn update_firing(ai: &mut AiState, bot: &Player, state: &GameState, dt: f32) {
    // Only fire at valid targets when chasing
    if ai.behavior != AiBehavior::Chase && ai.behavior != AiBehavior::Flee {
        ai.wants_fire = false;
        ai.charge_time = 0.0;
        return;
    }

    let target = match ai.target_id.and_then(|id| state.get_player(id)) {
        Some(t) if t.alive => t,
        _ => {
            ai.wants_fire = false;
            return;
        }
    };

    let distance = bot.position.distance_to(target.position);

    // Only fire at reasonable range
    if distance > 300.0 {
        ai.wants_fire = false;
        ai.charge_time = 0.0;
        return;
    }

    // Add some inaccuracy based on personality
    let mut rng = rand::thread_rng();
    let accuracy_offset = (1.0 - ai.personality.accuracy) * rng.gen_range(-0.3..0.3);
    let aim_to_target = (target.position - bot.position).normalize();
    ai.aim_direction = aim_to_target.rotate(accuracy_offset);

    // Charge and fire logic
    if ai.wants_fire {
        ai.charge_time += dt;

        // Release when charged enough or randomly
        let charge_threshold = 0.3 + rng.gen_range(0.0..0.5);
        if ai.charge_time > charge_threshold {
            ai.wants_fire = false;
        }
    } else if ai.charge_time > 0.0 {
        // Just released
        ai.charge_time = 0.0;
    } else {
        // Decide whether to start charging
        if rng.gen::<f32>() < 0.02 {
            ai.wants_fire = true;
        }
    }
}

/// Find nearest threat (bigger player) and nearest target (smaller player)
fn find_nearest_players(
    bot: &Player,
    state: &GameState,
) -> (Option<(PlayerId, f32)>, Option<(PlayerId, f32)>) {
    let mut nearest_threat: Option<(PlayerId, f32)> = None;
    let mut nearest_target: Option<(PlayerId, f32)> = None;

    for player in state.players.values() {
        if player.id == bot.id || !player.alive || player.is_bot {
            continue;
        }

        let dist = bot.position.distance_to(player.position);

        if player.mass > bot.mass * 1.2 {
            // Threat - update if closer than current nearest
            let dominated = nearest_threat.map_or(true, |(_, d)| dist < d);
            if dominated {
                nearest_threat = Some((player.id, dist));
            }
        } else {
            // Target - update if closer than current nearest
            let dominated = nearest_target.map_or(true, |(_, d)| dist < d);
            if dominated {
                nearest_target = Some((player.id, dist));
            }
        }
    }

    (nearest_threat, nearest_target)
}

/// Generate bot names
pub fn generate_bot_name() -> String {
    let prefixes = ["Nova", "Star", "Cosmic", "Orbit", "Luna", "Solar", "Astro", "Nebula"];
    let suffixes = ["X", "Prime", "Alpha", "Beta", "One", "Zero", "Max", "Pro"];

    let mut rng = rand::thread_rng();
    format!(
        "{}{}",
        prefixes[rng.gen_range(0..prefixes.len())],
        suffixes[rng.gen_range(0..suffixes.len())]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::state::MatchPhase;
    use uuid::Uuid;

    fn create_test_state() -> GameState {
        let mut state = GameState::new();
        state.match_state.phase = MatchPhase::Playing;
        state
    }

    fn create_bot(position: Vec2, mass: f32) -> Player {
        Player {
            id: Uuid::new_v4(),
            name: "Bot".to_string(),
            position,
            velocity: Vec2::ZERO,
            rotation: 0.0,
            mass,
            alive: true,
            kills: 0,
            deaths: 0,
            spawn_protection: 0.0,
            is_bot: true,
            color_index: 0,
            respawn_timer: 0.0,
        }
    }

    #[test]
    fn test_ai_state_default() {
        let ai = AiState::default();
        assert_eq!(ai.behavior, AiBehavior::Idle);
        assert!(ai.target_id.is_none());
    }

    #[test]
    fn test_ai_manager_register() {
        let mut manager = AiManager::new();
        let bot_id = Uuid::new_v4();

        manager.register_bot(bot_id);

        assert!(manager.get(bot_id).is_some());
    }

    #[test]
    fn test_ai_manager_unregister() {
        let mut manager = AiManager::new();
        let bot_id = Uuid::new_v4();

        manager.register_bot(bot_id);
        manager.unregister_bot(bot_id);

        assert!(manager.get(bot_id).is_none());
    }

    #[test]
    fn test_ai_generates_input() {
        let mut manager = AiManager::new();
        let bot_id = Uuid::new_v4();

        manager.register_bot(bot_id);

        let input = manager.get_input(bot_id, 100);
        assert!(input.is_some());
        assert_eq!(input.unwrap().tick, 100);
    }

    #[test]
    fn test_personality_random() {
        let p1 = AiPersonality::random();
        let p2 = AiPersonality::random();

        // Very unlikely to be exactly equal
        assert!(
            (p1.aggression - p2.aggression).abs() > 0.001
                || (p1.accuracy - p2.accuracy).abs() > 0.001
        );
    }

    #[test]
    fn test_bot_name_generation() {
        let name1 = generate_bot_name();
        let name2 = generate_bot_name();

        assert!(!name1.is_empty());
        assert!(!name2.is_empty());
        // Names might occasionally be the same, but format should be consistent
    }

    #[test]
    fn test_orbit_behavior() {
        let mut ai = AiState::default();
        ai.behavior = AiBehavior::Orbit;
        ai.personality.preferred_radius = 300.0;

        let state = create_test_state();
        let bot = create_bot(Vec2::new(300.0, 0.0), 100.0);
        execute_orbit(&mut ai, &bot, &state);

        // Should be trying to orbit (tangent direction)
        assert!(ai.thrust_direction.length_sq() > 0.01);
    }

    #[test]
    fn test_flee_sets_boost() {
        let mut state = create_test_state();
        let mut ai = AiState::default();

        let bot = create_bot(Vec2::new(100.0, 0.0), 50.0);
        let bot_id = bot.id;
        let threat = create_bot(Vec2::new(150.0, 0.0), 200.0);
        let threat_id = threat.id;

        state.add_player(bot);
        state.add_player(threat);

        ai.behavior = AiBehavior::Flee;
        ai.target_id = Some(threat_id);

        let bot_ref = state.get_player(bot_id).unwrap();
        execute_flee(&mut ai, bot_ref, &state);

        assert!(ai.wants_boost);
    }

    #[test]
    fn test_decision_timer() {
        let mut state = create_test_state();
        let mut manager = AiManager::new();

        let bot = create_bot(Vec2::new(300.0, 0.0), 100.0);
        let bot_id = bot.id;
        state.add_player(bot);

        manager.register_bot(bot_id);

        // Set a high initial timer to test decrement
        if let Some(ai) = manager.get_mut(bot_id) {
            ai.decision_timer = 1.0;
        }

        let initial_timer = manager.get(bot_id).unwrap().decision_timer;
        manager.update(&state, 0.1);

        let new_timer = manager.get(bot_id).unwrap().decision_timer;
        // Timer should have decreased by dt
        assert!(new_timer < initial_timer);
        assert!((new_timer - 0.9).abs() < 0.01);
    }
}
