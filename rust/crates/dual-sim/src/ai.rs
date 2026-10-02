//! Deterministic AI input generation.
//!
//! AI is intentionally outside [`super::Simulation`]. It reads an immutable
//! frame snapshot and returns the same `PlayerInput` type used by humans and
//! network peers, so it cannot bypass collision, weapon, or score rules.

use super::{ArrowSnapshot, FrameSnapshot, LONGBOW_CHARGE_FRAMES, PlayerInput, PlayerPhase, Side};

/// Fairness settings copied from the Java baseline's three selectable profiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiDifficulty {
    Basic,
    Standard,
    Advanced,
}

impl AiDifficulty {
    const fn plan_update_frames(self) -> u16 {
        match self {
            Self::Basic => 18,
            Self::Standard => 10,
            Self::Advanced => 5,
        }
    }

    const fn kill_attempt_probability(self) -> f32 {
        match self {
            Self::Basic => 0.20,
            Self::Standard => 0.30,
            Self::Advanced => 0.45,
        }
    }

    const fn evade_probability(self) -> f32 {
        match self {
            Self::Basic => 0.55,
            Self::Standard => 0.70,
            Self::Advanced => 0.85,
        }
    }

    const fn idle_move_probability(self) -> f32 {
        match self {
            Self::Basic => 0.20,
            Self::Standard => 0.20,
            Self::Advanced => 0.35,
        }
    }

    const fn longbow_release_probability(self) -> f32 {
        match self {
            Self::Basic => 0.03,
            Self::Standard => 0.05,
            Self::Advanced => 0.12,
        }
    }

    const fn fake_charge_probability(self) -> f32 {
        match self {
            Self::Basic => 0.0,
            Self::Standard => 0.0,
            Self::Advanced => 0.15,
        }
    }

    const fn intercept_aim_probability(self) -> f32 {
        match self {
            Self::Basic => 0.0,
            Self::Standard => 0.0,
            Self::Advanced => 0.35,
        }
    }
}

/// High-level AI behavior selected for the next bounded planning interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiPlan {
    Move,
    Jab,
    Kill,
    Evade,
}

/// Stateful but deterministic input producer for one side of a simulation.
pub struct AiController {
    side: Side,
    difficulty: AiDifficulty,
    random: DeterministicRandom,
    current_plan: AiPlan,
    plan_frames_remaining: u16,
}

impl AiController {
    /// Creates an AI controller whose random decisions are isolated by seed.
    pub fn new(side: Side, difficulty: AiDifficulty, seed: u64) -> Self {
        Self {
            side,
            difficulty,
            random: DeterministicRandom::new(seed ^ side_seed(side)),
            current_plan: AiPlan::Move,
            plan_frames_remaining: 0,
        }
    }

    pub const fn side(&self) -> Side {
        self.side
    }

    pub const fn difficulty(&self) -> AiDifficulty {
        self.difficulty
    }

    pub const fn current_plan(&self) -> AiPlan {
        self.current_plan
    }

    /// Reads a frame snapshot and produces one input snapshot without mutating
    /// the simulation or any player, arrow, score, or round state.
    pub fn next_input(&mut self, snapshot: &FrameSnapshot) -> PlayerInput {
        let player = snapshot.players[self.side.index()];
        if player.phase == PlayerPhase::Dead || snapshot.round_result.is_some() {
            return PlayerInput::empty();
        }

        let keep_charging =
            player.phase == PlayerPhase::LongbowCharge && self.current_plan == AiPlan::Kill;
        if self.plan_frames_remaining == 0 && !keep_charging {
            self.current_plan = self.choose_plan(snapshot);
            self.plan_frames_remaining = self.difficulty.plan_update_frames();
        }
        self.plan_frames_remaining = self.plan_frames_remaining.saturating_sub(1);

        let opponent = snapshot.players[self.side.opponent().index()];
        let intercept_angle = (player.phase == PlayerPhase::ShortbowAction)
            .then(|| nearest_incoming_arrow(self.side, player.position, snapshot))
            .flatten()
            .filter(|_| {
                self.random
                    .chance(self.difficulty.intercept_aim_probability())
            })
            .map(|arrow| {
                (arrow.position.y - player.position.y).atan2(arrow.position.x - player.position.x)
            });
        let aim_angle = intercept_angle.or_else(|| {
            Some(
                (opponent.position.y - player.position.y)
                    .atan2(opponent.position.x - player.position.x),
            )
        });
        let mut input = match self.current_plan {
            AiPlan::Move => movement_away(player.position, opponent.position, aim_angle),
            AiPlan::Evade => {
                self.evade_input(player.position, opponent.position, snapshot, aim_angle)
            }
            AiPlan::Jab => PlayerInput {
                shortbow: snapshot.frame % 2 == 0
                    && player.shortbow_ammo > 0
                    && player.shortbow_cooldown_frames == 0,
                intercept_aim: intercept_angle.is_some(),
                ..movement_toward(player.position, opponent.position, aim_angle)
            },
            AiPlan::Kill => {
                let release = player.phase == PlayerPhase::LongbowCharge
                    && player.longbow_charge_frames >= LONGBOW_CHARGE_FRAMES
                    && self
                        .random
                        .chance(self.difficulty.longbow_release_probability());
                PlayerInput {
                    longbow: !release,
                    ..movement_toward(player.position, opponent.position, aim_angle)
                }
            }
        };
        input.aim_angle = aim_angle;
        input.intercept_aim = intercept_angle.is_some();
        input
    }

    /// Selects combat intent from damage state, incoming arrows, distance, and one seeded roll.
    fn choose_plan(&mut self, snapshot: &FrameSnapshot) -> AiPlan {
        let player = snapshot.players[self.side.index()];
        let opponent = snapshot.players[self.side.opponent().index()];
        if opponent.phase == PlayerPhase::Damaged
            && self
                .random
                .chance(self.difficulty.kill_attempt_probability())
        {
            return AiPlan::Kill;
        }
        if !matches!(opponent.phase, PlayerPhase::Damaged | PlayerPhase::Dead)
            && self
                .random
                .chance(self.difficulty.fake_charge_probability())
        {
            return AiPlan::Kill;
        }

        if let Some(arrow) = nearest_incoming_arrow(self.side, player.position, snapshot) {
            if arrow.position.minus(player.position).length_squared() < 40_000.0
                && self.random.chance(self.difficulty.evade_probability())
            {
                return AiPlan::Evade;
            }
        }

        if player.position.minus(opponent.position).length_squared() < 100_000.0
            && self.random.chance(self.difficulty.evade_probability())
        {
            return AiPlan::Move;
        }
        if self.random.chance(self.difficulty.idle_move_probability()) {
            AiPlan::Move
        } else {
            AiPlan::Jab
        }
    }

    /// Chooses a perpendicular escape direction when a live projectile approaches.
    fn evade_input(
        &self,
        player_position: super::Vec2,
        opponent_position: super::Vec2,
        snapshot: &FrameSnapshot,
        aim_angle: Option<f32>,
    ) -> PlayerInput {
        let Some(arrow) = nearest_incoming_arrow(self.side, player_position, snapshot) else {
            return movement_away(player_position, opponent_position, aim_angle);
        };
        let to_player = player_position.minus(arrow.position);
        let perpendicular = if arrow.velocity.x * to_player.y - arrow.velocity.y * to_player.x > 0.0
        {
            super::Vec2::new(-arrow.velocity.y, arrow.velocity.x)
        } else {
            super::Vec2::new(arrow.velocity.y, -arrow.velocity.x)
        };
        movement_from_vector(perpendicular, aim_angle)
    }
}

/// Finds the closest enemy arrow whose velocity points toward the controlled player.
fn nearest_incoming_arrow(
    side: Side,
    player_position: super::Vec2,
    snapshot: &FrameSnapshot,
) -> Option<ArrowSnapshot> {
    snapshot
        .arrows
        .iter()
        .filter(|arrow| arrow.owner == side.opponent())
        .filter(|arrow| {
            let to_player = player_position.minus(arrow.position);
            arrow.velocity.dot(to_player) > 0.0
        })
        .min_by(|first, second| {
            first
                .position
                .minus(player_position)
                .length_squared()
                .total_cmp(&second.position.minus(player_position).length_squared())
        })
        .copied()
}

/// Converts a target offset into a normalized directional input.
fn movement_toward(
    player_position: super::Vec2,
    target_position: super::Vec2,
    aim_angle: Option<f32>,
) -> PlayerInput {
    movement_from_vector(target_position.minus(player_position), aim_angle)
}

/// Converts the opposite target offset into a normalized escape input.
fn movement_away(
    player_position: super::Vec2,
    target_position: super::Vec2,
    aim_angle: Option<f32>,
) -> PlayerInput {
    movement_from_vector(player_position.minus(target_position), aim_angle)
}

/// Maps a vector to four directional buttons and preserves the selected aim angle.
fn movement_from_vector(vector: super::Vec2, aim_angle: Option<f32>) -> PlayerInput {
    PlayerInput {
        up: vector.y < -1.0,
        down: vector.y > 1.0,
        left: vector.x < -1.0,
        right: vector.x > 1.0,
        aim_angle,
        ..PlayerInput::empty()
    }
}

fn side_seed(side: Side) -> u64 {
    match side {
        Side::One => 0x9e37_79b9_7f4a_7c15,
        Side::Two => 0xd1b5_4a32_d192_ed03,
    }
}

/// Small deterministic generator used only by AI decision rolls.
#[derive(Clone, Copy)]
struct DeterministicRandom {
    state: u64,
}

impl DeterministicRandom {
    const fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    /// Advances a fixed-width xor-shift state without accessing global randomness.
    fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 7;
        self.state ^= self.state >> 9;
        self.state ^= self.state << 8;
        self.state
    }

    /// Draws one reproducible probability sample from the upper random bits.
    fn chance(&mut self, probability: f32) -> bool {
        let sample = (self.next_u64() >> 40) as f32 / (1_u64 << 24) as f32;
        sample < probability
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Simulation, SimulationConfig};

    #[test]
    fn same_seed_produces_same_ai_input_sequence() {
        let mut first_simulation = Simulation::new(SimulationConfig::default(), 17);
        let mut second_simulation = Simulation::new(SimulationConfig::default(), 17);
        let mut first = AiController::new(Side::One, AiDifficulty::Advanced, 99);
        let mut second = AiController::new(Side::One, AiDifficulty::Advanced, 99);
        for _ in 0..120 {
            let first_input = first.next_input(&first_simulation.snapshot());
            let second_input = second.next_input(&second_simulation.snapshot());
            assert_eq!(first_input, second_input);
            first_simulation.step([first_input, PlayerInput::empty()]);
            second_simulation.step([second_input, PlayerInput::empty()]);
        }
        assert_eq!(first_simulation.snapshot(), second_simulation.snapshot());
    }

    #[test]
    fn difficulty_profiles_keep_the_expected_reaction_order() {
        assert!(
            AiDifficulty::Advanced.plan_update_frames()
                < AiDifficulty::Standard.plan_update_frames()
        );
        assert!(
            AiDifficulty::Standard.evade_probability() > AiDifficulty::Basic.evade_probability()
        );
        assert!(
            AiDifficulty::Advanced.intercept_aim_probability()
                > AiDifficulty::Standard.intercept_aim_probability()
        );
    }

    #[test]
    fn advanced_ai_can_choose_an_incoming_arrow_intercept() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 27);
        let opponent = &mut simulation.players[Side::Two.index()];
        opponent.phase = PlayerPhase::ShortbowAction;
        opponent.shortbow_action_frames = 2;
        simulation.arrows.push(super::super::ArrowState::new(
            Side::One,
            super::super::ArrowKind::Shortbow,
            opponent.position.plus(super::super::Vec2::new(0.0, -30.0)),
            core::f32::consts::FRAC_PI_2,
            8.0,
        ));
        let snapshot = simulation.snapshot();
        let mut ai = AiController::new(Side::Two, AiDifficulty::Advanced, 37);
        let mut intercept = None;
        for _ in 0..120 {
            let input = ai.next_input(&snapshot);
            if input.intercept_aim {
                intercept = input.aim_angle;
                break;
            }
        }
        let angle = intercept.expect("advanced AI should take an intercept aim roll");
        assert!((angle + core::f32::consts::FRAC_PI_2).abs() < 0.0001);
    }

    #[test]
    fn ai_keeps_a_longbow_charge_until_its_release_roll() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 31);
        simulation.players[1].phase = PlayerPhase::LongbowCharge;
        simulation.players[1].longbow_charge_frames = 15;
        let mut ai = AiController::new(Side::Two, AiDifficulty::Basic, 41);
        ai.current_plan = AiPlan::Kill;
        let input = ai.next_input(&simulation.snapshot());
        assert_eq!(ai.current_plan, AiPlan::Kill);
        assert!(input.longbow);
    }

    #[test]
    fn ai_returns_released_input_after_completed_match() {
        let config = SimulationConfig {
            rounds_to_win: 1,
            ..SimulationConfig::default()
        };
        let mut simulation = Simulation::new(config, 5);
        let snapshot = simulation.snapshot();
        let mut ai = AiController::new(Side::Two, AiDifficulty::Basic, 2);
        assert!(ai.next_input(&snapshot).aim_angle.is_some());

        let mut finished = snapshot;
        finished.round_result = Some(
            dual_protocol::RoundResult::new(1, dual_protocol::WinnerSide::SideOne, 1, 0, true)
                .expect("valid fixture"),
        );
        assert_eq!(ai.next_input(&finished), PlayerInput::empty());
        simulation.reset_match();
    }
}
