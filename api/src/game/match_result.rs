//! Match result and ranking system
//!
//! Computes final match results and player rankings.

#![allow(dead_code)] // Match result fields for UI/API consumption

use crate::game::state::{GameState, MatchPhase, PlayerId};

/// Match result information
#[derive(Debug, Clone)]
pub struct MatchResult {
    pub winner_id: Option<PlayerId>,
    pub winner_name: Option<String>,
    pub rankings: Vec<PlayerRanking>,
    pub match_duration: f32,
    pub total_kills: u32,
}

/// Player ranking in match results
#[derive(Debug, Clone)]
pub struct PlayerRanking {
    pub player_id: PlayerId,
    pub name: String,
    pub rank: u32,
    pub kills: u32,
    pub deaths: u32,
    pub final_mass: f32,
    pub survived: bool,
    pub is_bot: bool,
}

/// Determine match result from game state
pub fn determine_result(state: &GameState) -> MatchResult {
    let mut rankings: Vec<PlayerRanking> = state
        .players
        .values()
        .map(|p| PlayerRanking {
            player_id: p.id,
            name: p.name.clone(),
            rank: 0,
            kills: p.kills,
            deaths: p.deaths,
            final_mass: p.mass,
            survived: p.alive,
            is_bot: p.is_bot,
        })
        .collect();

    // Sort by: survived (desc), kills (desc), mass (desc)
    rankings.sort_by(|a, b| {
        b.survived
            .cmp(&a.survived)
            .then_with(|| b.kills.cmp(&a.kills))
            .then_with(|| b.final_mass.partial_cmp(&a.final_mass).unwrap_or(std::cmp::Ordering::Equal))
    });

    // Assign ranks
    for (i, ranking) in rankings.iter_mut().enumerate() {
        ranking.rank = (i + 1) as u32;
    }

    let total_kills: u32 = rankings.iter().map(|r| r.kills).sum();

    let (winner_id, winner_name) = if let Some(first) = rankings.first() {
        if first.survived {
            (Some(first.player_id), Some(first.name.clone()))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    MatchResult {
        winner_id,
        winner_name,
        rankings,
        match_duration: state.match_state.match_time,
        total_kills,
    }
}

/// Check if match should end
pub fn check_match_end(state: &GameState) -> Option<MatchEndReason> {
    // Not playing
    if state.match_state.phase != MatchPhase::Playing {
        return None;
    }

    let alive_count = state.alive_count();
    let alive_human_count = state.alive_human_count();

    // Only one player left
    if alive_count <= 1 {
        return Some(MatchEndReason::LastManStanding);
    }

    // All humans dead (bots win)
    if alive_human_count == 0 && !state.players.values().all(|p| p.is_bot) {
        return Some(MatchEndReason::AllHumansDead);
    }

    // Time limit reached
    if state.match_state.match_time >= crate::game::constants::game::MATCH_DURATION {
        return Some(MatchEndReason::TimeLimit);
    }

    None
}

/// Reason why match ended
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchEndReason {
    /// Only one player remaining
    LastManStanding,
    /// All human players died
    AllHumansDead,
    /// Time limit reached
    TimeLimit,
    /// Match was cancelled
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::state::Player;
    use crate::util::vec2::Vec2;
    use uuid::Uuid;

    fn create_player(name: &str, alive: bool, kills: u32, mass: f32, is_bot: bool) -> Player {
        Player {
            id: Uuid::new_v4(),
            name: name.to_string(),
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            rotation: 0.0,
            mass,
            alive,
            kills,
            deaths: 0,
            spawn_protection: 0.0,
            is_bot,
            color_index: 0,
            respawn_timer: 0.0,
            spawn_tick: 0,
        }
    }

    #[test]
    fn test_determine_result_single_winner() {
        let mut state = GameState::new();
        state.add_player(create_player("Winner", true, 5, 200.0, false));
        state.add_player(create_player("Loser1", false, 2, 50.0, false));
        state.add_player(create_player("Loser2", false, 1, 30.0, false));

        let result = determine_result(&state);

        assert!(result.winner_id.is_some());
        assert_eq!(result.winner_name.as_deref(), Some("Winner"));
        assert_eq!(result.rankings[0].name, "Winner");
        assert_eq!(result.rankings[0].rank, 1);
    }

    #[test]
    fn test_determine_result_no_survivors() {
        let mut state = GameState::new();
        state.add_player(create_player("Dead1", false, 3, 0.0, false));
        state.add_player(create_player("Dead2", false, 2, 0.0, false));

        let result = determine_result(&state);

        assert!(result.winner_id.is_none());
        // Still ranked by kills
        assert_eq!(result.rankings[0].name, "Dead1");
    }

    #[test]
    fn test_ranking_order() {
        let mut state = GameState::new();
        state.add_player(create_player("HighKills", true, 10, 100.0, false));
        state.add_player(create_player("HighMass", true, 5, 300.0, false));
        state.add_player(create_player("Dead", false, 15, 0.0, false));

        let result = determine_result(&state);

        // Alive beats dead
        assert!(result.rankings[0].survived);
        assert!(result.rankings[1].survived);
        assert!(!result.rankings[2].survived);

        // Among alive, kills matter
        assert_eq!(result.rankings[0].name, "HighKills");
    }

    #[test]
    fn test_check_match_end_last_man() {
        let mut state = GameState::new();
        state.match_state.phase = MatchPhase::Playing;
        state.add_player(create_player("Winner", true, 0, 100.0, false));
        state.add_player(create_player("Dead", false, 0, 0.0, false));

        let reason = check_match_end(&state);
        assert_eq!(reason, Some(MatchEndReason::LastManStanding));
    }

    #[test]
    fn test_check_match_end_all_humans_dead() {
        let mut state = GameState::new();
        state.match_state.phase = MatchPhase::Playing;
        state.add_player(create_player("Human", false, 0, 0.0, false));
        state.add_player(create_player("Bot1", true, 0, 100.0, true));
        state.add_player(create_player("Bot2", true, 0, 100.0, true));

        let reason = check_match_end(&state);
        assert_eq!(reason, Some(MatchEndReason::AllHumansDead));
    }

    #[test]
    fn test_check_match_end_time_limit() {
        let mut state = GameState::new();
        state.match_state.phase = MatchPhase::Playing;
        state.match_state.match_time = crate::game::constants::game::MATCH_DURATION + 1.0;
        state.add_player(create_player("P1", true, 0, 100.0, false));
        state.add_player(create_player("P2", true, 0, 100.0, false));

        let reason = check_match_end(&state);
        assert_eq!(reason, Some(MatchEndReason::TimeLimit));
    }

    #[test]
    fn test_check_match_end_not_ended() {
        let mut state = GameState::new();
        state.match_state.phase = MatchPhase::Playing;
        state.match_state.match_time = 60.0;
        state.add_player(create_player("P1", true, 0, 100.0, false));
        state.add_player(create_player("P2", true, 0, 100.0, false));

        let reason = check_match_end(&state);
        assert!(reason.is_none());
    }

    #[test]
    fn test_check_match_end_not_playing() {
        let mut state = GameState::new();
        state.match_state.phase = MatchPhase::Waiting;
        state.add_player(create_player("P1", true, 0, 100.0, false));

        let reason = check_match_end(&state);
        assert!(reason.is_none());
    }

    #[test]
    fn test_total_kills() {
        let mut state = GameState::new();
        state.add_player(create_player("P1", true, 5, 100.0, false));
        state.add_player(create_player("P2", false, 3, 0.0, false));
        state.add_player(create_player("P3", false, 2, 0.0, false));

        let result = determine_result(&state);
        assert_eq!(result.total_kills, 10);
    }
}
