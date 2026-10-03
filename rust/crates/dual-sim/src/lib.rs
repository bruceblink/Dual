//! Deterministic rules for the Dual archery game.
//!
//! The simulation deliberately has no window, renderer, audio device, or
//! network runtime dependency. A caller supplies one input snapshot per player
//! and receives a copyable frame snapshot plus gameplay events. Macroquad and
//! the Relay will consume this API later without being allowed to mutate the
//! rules directly.

use dual_protocol::{RoundResult, WinnerSide};

mod ai;
mod tactical;

pub use ai::{AiController, AiDifficulty, AiPlan};
use tactical::TacticalEventRecorder;
pub use tactical::{TacticalEvent, TacticalEventKind};

pub const FPS: u32 = 60;
pub const ARENA_WIDTH: f32 = 1_280.0;
pub const ARENA_HEIGHT: f32 = 720.0;
pub const PLAYER_BODY_SIZE: f32 = 32.0;
pub const PLAYER_RADIUS: f32 = PLAYER_BODY_SIZE * 0.5;
pub const PLAYER_MAX_VX: f32 = 10.0;
pub const PLAYER_MAX_VY: f32 = 7.0;
pub const PLAYER_FRICTION: f32 = 0.92;
pub const PLAYER_BOUNCE: f32 = 0.5;
pub const PLAYER_THRUST_SPEED: f32 = 8.0;
pub const DAMAGED_FRAMES: u16 = 45;
pub const DAMAGED_END_FEEDBACK_FRAMES: u16 = 8;
pub const SHORTBOW_MAX_CONSECUTIVE_PRESSURES: u8 = 2;
pub const SHORTBOW_ARROW_RADIUS: f32 = 8.0;
pub const SHORTBOW_ARROW_START_SPEED: f32 = 24.0;
pub const SHORTBOW_ARROW_TERMINAL_SPEED: f32 = 8.0;
pub const SHORTBOW_MAX_AMMO: u8 = 3;
pub const SHORTBOW_AMMO_RECOVERY_FRAMES: u16 = 60;
pub const SHORTBOW_FIRE_INTERVAL_FRAMES: u16 = 12;
pub const SHORTBOW_ACTION_FRAMES: u16 = 4;
pub const TACTICAL_OPENING_WINDOW_FRAMES: u64 = 90;
pub const LONGBOW_SPEED: f32 = 64.0;
pub const LONGBOW_COMPONENT_INTERVAL: f32 = 24.0;
pub const LONGBOW_SHAFT_COUNT: usize = 5;
pub const LONGBOW_COMPONENT_RADIUS: f32 = 16.0;
pub const LONGBOW_CHARGE_FRAMES: u16 = 30;
pub const LONGBOW_RECOVERY_FRAMES: u16 = 21;
pub const LONGBOW_CHARGE_MOVE_RATIO: f32 = 0.5;
pub const LONGBOW_AIM_SPEED_RATIO: f32 = 0.1;
pub const LONGBOW_AUTO_AIM_RANGE: f32 = 520.0;
pub const MATCH_ROUNDS_TO_WIN: u8 = 3;

/// A small two-dimensional value type shared by players, arrows, and geometry.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn plus(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }

    pub fn minus(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }

    pub fn scale(self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    pub fn normalized_or(self, fallback: Self) -> Self {
        let length_squared = self.length_squared();
        if length_squared <= f32::EPSILON {
            return fallback;
        }
        self.scale(length_squared.sqrt().recip())
    }

    pub fn from_angle(angle: f32) -> Self {
        Self::new(angle.cos(), angle.sin())
    }
}

/// The two arena layouts currently available to the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaKind {
    Open,
    CentralCover,
}

impl ArenaKind {
    pub const fn central_cover_rect(self) -> Option<ArenaRect> {
        match self {
            Self::Open => None,
            Self::CentralCover => Some(ArenaRect::new(640.0, 360.0, 280.0, 80.0)),
        }
    }

    pub fn blocks_circle(self, center: Vec2, radius: f32) -> bool {
        self.central_cover_rect()
            .is_some_and(|rect| rect.contains_circle(center, radius))
    }

    pub fn first_cover_impact(self, start: Vec2, end: Vec2, radius: f32) -> Option<CoverImpact> {
        self.central_cover_rect()
            .and_then(|rect| rect.find_first_circle_impact(start, end, radius))
    }

    pub fn has_clear_projectile_path(self, start: Vec2, end: Vec2, radius: f32) -> bool {
        self.first_cover_impact(start, end, radius).is_none()
    }

    /// Moves an overlapped player to the nearest cover face and reflects its
    /// velocity component, matching the Java arena collision rule.
    pub fn resolve_player(self, position: &mut Vec2, velocity: &mut Vec2) {
        let Some(rect) = self.central_cover_rect() else {
            return;
        };
        if !rect.contains_circle(*position, PLAYER_RADIUS) {
            return;
        }

        let push_left = position.x - (rect.left() - PLAYER_RADIUS);
        let push_right = rect.right() + PLAYER_RADIUS - position.x;
        let push_top = position.y - (rect.top() - PLAYER_RADIUS);
        let push_bottom = rect.bottom() + PLAYER_RADIUS - position.y;
        let smallest_push = push_left.min(push_right).min(push_top).min(push_bottom);

        if smallest_push == push_left {
            position.x = rect.left() - PLAYER_RADIUS;
            velocity.x = -velocity.x.abs();
        } else if smallest_push == push_right {
            position.x = rect.right() + PLAYER_RADIUS;
            velocity.x = velocity.x.abs();
        } else if smallest_push == push_top {
            position.y = rect.top() - PLAYER_RADIUS;
            velocity.y = -velocity.y.abs();
        } else {
            position.y = rect.bottom() + PLAYER_RADIUS;
            velocity.y = velocity.y.abs();
        }
    }
}

/// One axis-aligned piece of solid cover in logical arena coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArenaRect {
    pub center: Vec2,
    pub width: f32,
    pub height: f32,
}

impl ArenaRect {
    pub const fn new(center_x: f32, center_y: f32, width: f32, height: f32) -> Self {
        Self {
            center: Vec2::new(center_x, center_y),
            width,
            height,
        }
    }

    pub fn left(self) -> f32 {
        self.center.x - self.width * 0.5
    }

    pub fn right(self) -> f32 {
        self.center.x + self.width * 0.5
    }

    pub fn top(self) -> f32 {
        self.center.y - self.height * 0.5
    }

    pub fn bottom(self) -> f32 {
        self.center.y + self.height * 0.5
    }

    pub fn contains_circle(self, center: Vec2, radius: f32) -> bool {
        let closest_x = center.x.clamp(self.left(), self.right());
        let closest_y = center.y.clamp(self.top(), self.bottom());
        let delta = center.minus(Vec2::new(closest_x, closest_y));
        delta.length_squared() < radius * radius
    }

    /// Solves the moving-circle/rectangle contact in the same face/corner
    /// order as the Java baseline, preserving first-contact timing for arrows.
    pub fn find_first_circle_impact(
        self,
        start: Vec2,
        end: Vec2,
        radius: f32,
    ) -> Option<CoverImpact> {
        assert!(radius > 0.0, "collision radius must be positive");
        let movement = end.minus(start);
        if self.contains_circle(start, radius) {
            return Some(self.project_blocked_point(start, movement));
        }

        let mut earliest = None;
        earliest = earlier(
            earliest,
            self.face_impact(
                start,
                movement,
                self.left() - radius,
                true,
                self.left(),
                -1.0,
            ),
        );
        earliest = earlier(
            earliest,
            self.face_impact(
                start,
                movement,
                self.right() + radius,
                true,
                self.right(),
                1.0,
            ),
        );
        earliest = earlier(
            earliest,
            self.face_impact(
                start,
                movement,
                self.top() - radius,
                false,
                self.top(),
                -1.0,
            ),
        );
        earliest = earlier(
            earliest,
            self.face_impact(
                start,
                movement,
                self.bottom() + radius,
                false,
                self.bottom(),
                1.0,
            ),
        );

        earliest = earlier(
            earliest,
            self.corner_impact(
                start,
                movement,
                radius,
                Vec2::new(self.left(), self.top()),
                Vec2::new(-1.0, -1.0),
            ),
        );
        earliest = earlier(
            earliest,
            self.corner_impact(
                start,
                movement,
                radius,
                Vec2::new(self.right(), self.top()),
                Vec2::new(1.0, -1.0),
            ),
        );
        earliest = earlier(
            earliest,
            self.corner_impact(
                start,
                movement,
                radius,
                Vec2::new(self.left(), self.bottom()),
                Vec2::new(-1.0, 1.0),
            ),
        );
        earlier(
            earliest,
            self.corner_impact(
                start,
                movement,
                radius,
                Vec2::new(self.right(), self.bottom()),
                Vec2::new(1.0, 1.0),
            ),
        )
    }

    /// Tests one expanded rectangle face and rejects contacts beyond its edge.
    fn face_impact(
        self,
        start: Vec2,
        movement: Vec2,
        expanded_coordinate: f32,
        vertical_face: bool,
        surface_coordinate: f32,
        normal_sign: f32,
    ) -> Option<CoverImpact> {
        let movement_axis = if vertical_face {
            movement.x
        } else {
            movement.y
        };
        if movement_axis.abs() <= f32::EPSILON || movement_axis * normal_sign >= 0.0 {
            return None;
        }

        let start_axis = if vertical_face { start.x } else { start.y };
        let time_ratio = (expanded_coordinate - start_axis) / movement_axis;
        if !(0.0..=1.0).contains(&time_ratio) {
            return None;
        }

        let other_coordinate = if vertical_face {
            start.y + movement.y * time_ratio
        } else {
            start.x + movement.x * time_ratio
        };
        let minimum = if vertical_face {
            self.top()
        } else {
            self.left()
        };
        let maximum = if vertical_face {
            self.bottom()
        } else {
            self.right()
        };
        if !(minimum..=maximum).contains(&other_coordinate) {
            return None;
        }

        Some(if vertical_face {
            CoverImpact {
                time_ratio,
                point: Vec2::new(surface_coordinate, other_coordinate),
                normal: Vec2::new(normal_sign, 0.0),
            }
        } else {
            CoverImpact {
                time_ratio,
                point: Vec2::new(other_coordinate, surface_coordinate),
                normal: Vec2::new(0.0, normal_sign),
            }
        })
    }

    /// Solves a swept circle against one corner and keeps only its outside quadrant.
    fn corner_impact(
        self,
        start: Vec2,
        movement: Vec2,
        radius: f32,
        corner: Vec2,
        corner_sides: Vec2,
    ) -> Option<CoverImpact> {
        let movement_squared = movement.length_squared();
        if movement_squared <= f32::EPSILON {
            return None;
        }

        let offset = start.minus(corner);
        let projection = offset.dot(movement);
        let distance_offset = offset.length_squared() - radius * radius;
        let discriminant = projection * projection - movement_squared * distance_offset;
        if discriminant < 0.0 {
            return None;
        }

        let time_ratio = (-projection - discriminant.sqrt()) / movement_squared;
        if !(0.0..=1.0).contains(&time_ratio) {
            return None;
        }
        let center = start.plus(movement.scale(time_ratio));
        if (center.x - corner.x) * corner_sides.x < -1.0e-4
            || (center.y - corner.y) * corner_sides.y < -1.0e-4
        {
            return None;
        }

        let normal = center.minus(corner).scale(1.0 / radius);
        if movement.dot(normal) >= 0.0 {
            return None;
        }
        Some(CoverImpact {
            time_ratio,
            point: corner,
            normal,
        })
    }

    /// Projects a circle that starts inside cover onto its nearest credible face.
    fn project_blocked_point(self, center: Vec2, movement: Vec2) -> CoverImpact {
        let closest = Vec2::new(
            center.x.clamp(self.left(), self.right()),
            center.y.clamp(self.top(), self.bottom()),
        );
        let delta = center.minus(closest);
        let distance_squared = delta.length_squared();
        if distance_squared > f32::EPSILON {
            return CoverImpact {
                time_ratio: 0.0,
                point: closest,
                normal: delta.scale(1.0 / distance_squared.sqrt()),
            };
        }

        if movement.x.abs() >= movement.y.abs() && movement.x != 0.0 {
            return if movement.x > 0.0 {
                CoverImpact {
                    time_ratio: 0.0,
                    point: Vec2::new(self.left(), center.y),
                    normal: Vec2::new(-1.0, 0.0),
                }
            } else {
                CoverImpact {
                    time_ratio: 0.0,
                    point: Vec2::new(self.right(), center.y),
                    normal: Vec2::new(1.0, 0.0),
                }
            };
        }
        if movement.y != 0.0 {
            return if movement.y > 0.0 {
                CoverImpact {
                    time_ratio: 0.0,
                    point: Vec2::new(center.x, self.top()),
                    normal: Vec2::new(0.0, -1.0),
                }
            } else {
                CoverImpact {
                    time_ratio: 0.0,
                    point: Vec2::new(center.x, self.bottom()),
                    normal: Vec2::new(0.0, 1.0),
                }
            };
        }

        let left_distance = center.x - self.left();
        let right_distance = self.right() - center.x;
        let top_distance = center.y - self.top();
        let bottom_distance = self.bottom() - center.y;
        let minimum_distance = left_distance
            .min(right_distance)
            .min(top_distance)
            .min(bottom_distance);
        if minimum_distance == left_distance {
            CoverImpact {
                time_ratio: 0.0,
                point: Vec2::new(self.left(), center.y),
                normal: Vec2::new(-1.0, 0.0),
            }
        } else if minimum_distance == right_distance {
            CoverImpact {
                time_ratio: 0.0,
                point: Vec2::new(self.right(), center.y),
                normal: Vec2::new(1.0, 0.0),
            }
        } else if minimum_distance == top_distance {
            CoverImpact {
                time_ratio: 0.0,
                point: Vec2::new(center.x, self.top()),
                normal: Vec2::new(0.0, -1.0),
            }
        } else {
            CoverImpact {
                time_ratio: 0.0,
                point: Vec2::new(center.x, self.bottom()),
                normal: Vec2::new(0.0, 1.0),
            }
        }
    }
}

/// Keeps the earliest contact so projectile feedback matches collision ordering.
fn earlier(current: Option<CoverImpact>, candidate: Option<CoverImpact>) -> Option<CoverImpact> {
    match (current, candidate) {
        (None, next) => next,
        (Some(previous), None) => Some(previous),
        (Some(previous), Some(next)) => {
            if next.time_ratio < previous.time_ratio {
                Some(next)
            } else {
                Some(previous)
            }
        }
    }
}

/// The first point and outward normal where a projectile meets cover.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoverImpact {
    pub time_ratio: f32,
    pub point: Vec2,
    pub normal: Vec2,
}

/// Identifies one combatant and provides stable array indexing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    One,
    Two,
}

impl Side {
    pub const fn index(self) -> usize {
        match self {
            Self::One => 0,
            Self::Two => 1,
        }
    }

    pub const fn opponent(self) -> Self {
        match self {
            Self::One => Self::Two,
            Self::Two => Self::One,
        }
    }

    const fn winner_side(self) -> WinnerSide {
        match self {
            Self::One => WinnerSide::SideOne,
            Self::Two => WinnerSide::SideTwo,
        }
    }
}

/// One frame of player intent. The simulation owns all resulting state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub shortbow: bool,
    pub longbow: bool,
    pub aim_angle: Option<f32>,
    /// Allows an AI shortbow action to use its intercept decision instead of auto-aim.
    pub intercept_aim: bool,
}

impl PlayerInput {
    pub const fn empty() -> Self {
        Self {
            up: false,
            down: false,
            left: false,
            right: false,
            shortbow: false,
            longbow: false,
            aim_angle: None,
            intercept_aim: false,
        }
    }

    pub fn from_protocol(input: dual_protocol::InputFrame) -> Self {
        Self {
            up: input.up(),
            down: input.down(),
            left: input.left(),
            right: input.right(),
            shortbow: input.shortbow(),
            longbow: input.longbow(),
            aim_angle: input.aim_angle(),
            intercept_aim: false,
        }
    }
}

/// Player phase visible to the renderer and replay tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerPhase {
    Move,
    ShortbowAction,
    LongbowCharge,
    Damaged,
    Dead,
}

/// Mutable state owned by the rules layer for one player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerState {
    pub side: Side,
    pub position: Vec2,
    pub velocity: Vec2,
    pub aim_angle: f32,
    pub phase: PlayerPhase,
    pub shortbow_ammo: u8,
    pub shortbow_recovery_frames: u16,
    pub shortbow_cooldown_frames: u16,
    pub shortbow_action_frames: u16,
    pub longbow_charge_frames: u16,
    pub longbow_recovery_frames: u16,
    pub damage_remaining_frames: u16,
    pub pressure_count: u8,
    pub damage_end_feedback_frames: u16,
}

impl PlayerState {
    fn new(side: Side, position: Vec2) -> Self {
        Self {
            side,
            position,
            velocity: Vec2::default(),
            aim_angle: 0.0,
            phase: PlayerPhase::Move,
            shortbow_ammo: SHORTBOW_MAX_AMMO,
            shortbow_recovery_frames: 0,
            shortbow_cooldown_frames: 0,
            shortbow_action_frames: 0,
            longbow_charge_frames: 0,
            longbow_recovery_frames: 0,
            damage_remaining_frames: 0,
            pressure_count: 0,
            damage_end_feedback_frames: 0,
        }
    }
}

/// Distinguishes the two projectile families for collision and rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowKind {
    Shortbow,
    LongbowShaft,
    LongbowHead,
}

impl ArrowKind {
    const fn radius(self) -> f32 {
        match self {
            Self::Shortbow => SHORTBOW_ARROW_RADIUS,
            Self::LongbowShaft | Self::LongbowHead => LONGBOW_COMPONENT_RADIUS,
        }
    }

    const fn is_lethal(self) -> bool {
        matches!(self, Self::LongbowShaft | Self::LongbowHead)
    }
}

/// Mutable projectile state; previous position enables swept collisions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrowState {
    pub owner: Side,
    pub kind: ArrowKind,
    pub position: Vec2,
    pub previous_position: Vec2,
    pub velocity: Vec2,
    pub speed: f32,
    pub rotation_angle: f32,
    pub removed: bool,
}

impl ArrowState {
    fn new(owner: Side, kind: ArrowKind, position: Vec2, angle: f32, speed: f32) -> Self {
        Self {
            owner,
            kind,
            position,
            previous_position: position,
            velocity: Vec2::from_angle(angle).scale(speed),
            speed,
            rotation_angle: angle,
            removed: false,
        }
    }

    fn update(&mut self) {
        self.previous_position = self.position;
        if self.kind == ArrowKind::Shortbow {
            self.velocity = Vec2::from_angle(self.rotation_angle).scale(self.speed);
        }
        self.position = self.position.plus(self.velocity);
        if self.kind == ArrowKind::Shortbow {
            self.speed += (SHORTBOW_ARROW_TERMINAL_SPEED - self.speed) * 0.1;
        }
        let radius = self.kind.radius();
        if self.position.x < -radius
            || self.position.x > ARENA_WIDTH + radius
            || self.position.y < -radius
            || self.position.y > ARENA_HEIGHT + radius
        {
            self.removed = true;
        }
    }
}

/// Immutable score state for the first-to-N match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchScore {
    pub rounds_to_win: u8,
    pub player_one_wins: u8,
    pub player_two_wins: u8,
}

impl MatchScore {
    fn new(rounds_to_win: u8) -> Self {
        assert!(rounds_to_win > 0, "rounds_to_win must be positive");
        Self {
            rounds_to_win,
            player_one_wins: 0,
            player_two_wins: 0,
        }
    }

    pub fn is_complete(self) -> bool {
        self.player_one_wins >= self.rounds_to_win || self.player_two_wins >= self.rounds_to_win
    }
}

/// Observable simulation event consumed by HUD and replay tooling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SimEvent {
    ShortbowFired { attacker: Side },
    LongbowFired { attacker: Side },
    LongbowChargeReady { attacker: Side },
    ShortbowHit { attacker: Side, target: Side },
    LongbowHit { attacker: Side, target: Side },
    Tactical(TacticalEvent),
    ArrowIntercepted { position: Vec2 },
    CoverImpact { position: Vec2, normal: Vec2 },
}

/// Configuration that does not change while a simulation is running.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimulationConfig {
    pub arena: ArenaKind,
    pub rounds_to_win: u8,
    pub spawn_positions: [Vec2; 2],
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            arena: ArenaKind::Open,
            rounds_to_win: MATCH_ROUNDS_TO_WIN,
            spawn_positions: [Vec2::new(640.0, 620.0), Vec2::new(640.0, 100.0)],
        }
    }
}

/// Copy of one player's state for rendering, diagnostics, or a replay frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerSnapshot {
    pub side: Side,
    pub position: Vec2,
    pub velocity: Vec2,
    pub aim_angle: f32,
    pub phase: PlayerPhase,
    pub shortbow_ammo: u8,
    pub shortbow_recovery_frames: u16,
    pub shortbow_cooldown_frames: u16,
    pub longbow_charge_frames: u16,
    pub longbow_recovery_frames: u16,
    pub damage_remaining_frames: u16,
    pub pressure_count: u8,
    pub damage_end_feedback_frames: u16,
}

impl From<PlayerState> for PlayerSnapshot {
    fn from(player: PlayerState) -> Self {
        Self {
            side: player.side,
            position: player.position,
            velocity: player.velocity,
            aim_angle: player.aim_angle,
            phase: player.phase,
            shortbow_ammo: player.shortbow_ammo,
            shortbow_recovery_frames: player.shortbow_recovery_frames,
            shortbow_cooldown_frames: player.shortbow_cooldown_frames,
            longbow_charge_frames: player.longbow_charge_frames,
            longbow_recovery_frames: player.longbow_recovery_frames,
            damage_remaining_frames: player.damage_remaining_frames,
            pressure_count: player.pressure_count,
            damage_end_feedback_frames: player.damage_end_feedback_frames,
        }
    }
}

/// Copy of one projectile state with removed arrows omitted by `snapshot`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrowSnapshot {
    pub owner: Side,
    pub kind: ArrowKind,
    pub position: Vec2,
    pub previous_position: Vec2,
    pub velocity: Vec2,
    pub rotation_angle: f32,
}

impl From<ArrowState> for ArrowSnapshot {
    fn from(arrow: ArrowState) -> Self {
        Self {
            owner: arrow.owner,
            kind: arrow.kind,
            position: arrow.position,
            previous_position: arrow.previous_position,
            velocity: arrow.velocity,
            rotation_angle: arrow.rotation_angle,
        }
    }
}

/// A complete immutable frame view. Rendering must only read this structure.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameSnapshot {
    pub frame: u64,
    pub seed: u64,
    pub players: [PlayerSnapshot; 2],
    pub arrows: Vec<ArrowSnapshot>,
    pub score: MatchScore,
    pub round_result: Option<RoundResult>,
    pub events: Vec<SimEvent>,
}

/// Deterministic, renderer-independent Dual rules.
pub struct Simulation {
    config: SimulationConfig,
    seed: u64,
    frame: u64,
    players: [PlayerState; 2],
    arrows: Vec<ArrowState>,
    score: MatchScore,
    last_round_result: Option<RoundResult>,
    previous_shortbow: [bool; 2],
    buffered_shortbow_frames: [u16; 2],
    tactical_events: TacticalEventRecorder,
    events: Vec<SimEvent>,
}

impl Simulation {
    /// Creates a fresh match with the configured layout, seed, and spawn points.
    pub fn new(config: SimulationConfig, seed: u64) -> Self {
        Self {
            players: [
                PlayerState::new(Side::One, config.spawn_positions[0]),
                PlayerState::new(Side::Two, config.spawn_positions[1]),
            ],
            score: MatchScore::new(config.rounds_to_win),
            config,
            seed,
            frame: 0,
            arrows: Vec::new(),
            last_round_result: None,
            previous_shortbow: [false; 2],
            buffered_shortbow_frames: [0; 2],
            tactical_events: TacticalEventRecorder::default(),
            events: Vec::new(),
        }
    }

    pub fn config(&self) -> SimulationConfig {
        self.config
    }

    pub const fn seed(&self) -> u64 {
        self.seed
    }

    pub const fn frame(&self) -> u64 {
        self.frame
    }

    pub fn snapshot(&self) -> FrameSnapshot {
        FrameSnapshot {
            frame: self.frame,
            seed: self.seed,
            players: [self.players[0].into(), self.players[1].into()],
            arrows: self
                .arrows
                .iter()
                .filter(|arrow| !arrow.removed)
                .copied()
                .map(ArrowSnapshot::from)
                .collect(),
            score: self.score,
            round_result: self.last_round_result,
            events: self.events.clone(),
        }
    }

    /// Exposes this frame's events without allocating a cloned render snapshot.
    pub fn events(&self) -> &[SimEvent] {
        &self.events
    }

    pub fn round_result(&self) -> Option<RoundResult> {
        self.last_round_result
    }

    pub fn score(&self) -> MatchScore {
        self.score
    }

    /// Reports whether this side still has a pressure-created longbow opening.
    pub fn has_tactical_opening(&self, side: Side) -> bool {
        self.tactical_events.has_active_opening(side, self.frame)
    }

    pub fn players(&self) -> [PlayerSnapshot; 2] {
        [self.players[0].into(), self.players[1].into()]
    }

    /// Advances exactly one 60 FPS rules frame from two input snapshots.
    pub fn step(&mut self, inputs: [PlayerInput; 2]) {
        if self.last_round_result.is_some() {
            return;
        }
        self.events.clear();
        self.update_arrows();
        self.update_players(inputs);
        self.resolve_cover_impacts();
        self.resolve_arrow_interception();
        self.resolve_player_hits();
        self.arrows.retain(|arrow| !arrow.removed);
        self.previous_shortbow = [inputs[0].shortbow, inputs[1].shortbow];
        self.frame = self.frame.saturating_add(1);
    }

    /// Restores both players and projectiles for the next round while retaining
    /// the match score. A completed match stays frozen until `reset_match`.
    pub fn reset_round(&mut self) {
        if self.score.is_complete() {
            return;
        }
        self.frame = 0;
        self.players = [
            PlayerState::new(Side::One, self.config.spawn_positions[0]),
            PlayerState::new(Side::Two, self.config.spawn_positions[1]),
        ];
        self.arrows.clear();
        self.last_round_result = None;
        self.previous_shortbow = [false; 2];
        self.buffered_shortbow_frames = [0; 2];
        self.tactical_events.reset();
        self.events.clear();
    }

    pub fn reset_match(&mut self) {
        self.score = MatchScore::new(self.config.rounds_to_win);
        self.reset_round();
    }

    /// Advances active projectiles once and marks those that leave the arena.
    fn update_arrows(&mut self) {
        for arrow in &mut self.arrows {
            if !arrow.removed {
                arrow.update();
            }
        }
    }

    /// Updates shared physics first, then applies each side's intent and timers.
    fn update_players(&mut self, inputs: [PlayerInput; 2]) {
        for index in 0..2 {
            if self.players[index].phase == PlayerPhase::Dead {
                continue;
            }
            self.update_player_physics(index);
            self.tick_player_timers(index);
            self.buffered_shortbow_frames[index] =
                self.buffered_shortbow_frames[index].saturating_sub(1);
        }

        for (index, input) in inputs.into_iter().enumerate() {
            if self.players[index].phase == PlayerPhase::Dead {
                continue;
            }
            let shortbow_edge = input.shortbow && !self.previous_shortbow[index];
            if shortbow_edge && self.players[index].phase != PlayerPhase::LongbowCharge {
                self.buffered_shortbow_frames[index] = 6;
            }

            let phase_before_update = self.players[index].phase;
            match phase_before_update {
                PlayerPhase::Move => self.update_moving_player(index, input),
                PlayerPhase::ShortbowAction => self.update_shortbow_action(index, input),
                PlayerPhase::LongbowCharge => self.update_longbow_charge(index, input),
                PlayerPhase::Damaged => self.update_damaged_player(index),
                PlayerPhase::Dead => {}
            }
        }
    }

    /// Moves by prior-frame velocity, then applies arena constraints and friction
    /// in the same order as the Java actor update.
    fn update_player_physics(&mut self, index: usize) {
        self.players[index].position = self.players[index]
            .position
            .plus(self.players[index].velocity);
        self.clamp_player_to_arena(index);
        self.players[index].velocity.x *= PLAYER_FRICTION;
        self.players[index].velocity.y *= PLAYER_FRICTION;
    }

    /// Advances bounded weapon and feedback timers for one living player.
    fn tick_player_timers(&mut self, index: usize) {
        let player = &mut self.players[index];
        if player.shortbow_ammo < SHORTBOW_MAX_AMMO {
            player.shortbow_recovery_frames = player.shortbow_recovery_frames.saturating_add(1);
            if player.shortbow_recovery_frames >= SHORTBOW_AMMO_RECOVERY_FRAMES {
                player.shortbow_ammo += 1;
                player.shortbow_recovery_frames = 0;
            }
        } else {
            player.shortbow_recovery_frames = 0;
        }
        player.shortbow_cooldown_frames = player.shortbow_cooldown_frames.saturating_sub(1);
        player.shortbow_action_frames = player.shortbow_action_frames.saturating_sub(1);
        player.longbow_recovery_frames = player.longbow_recovery_frames.saturating_sub(1);
        player.damage_end_feedback_frames = player.damage_end_feedback_frames.saturating_sub(1);
    }

    /// Applies movement, buffered shortbow fire, and longbow entry in priority order.
    fn update_moving_player(&mut self, index: usize, input: PlayerInput) {
        self.apply_movement(index, input, 1.0);
        if self.players[index].longbow_recovery_frames > 0 {
            return;
        }
        if self.buffered_shortbow_frames[index] > 0
            && self.players[index].shortbow_cooldown_frames == 0
            && self.players[index].shortbow_ammo > 0
        {
            self.buffered_shortbow_frames[index] = 0;
            self.update_aim(index, input, false);
            self.fire_shortbow(index);
            self.players[index].phase = PlayerPhase::ShortbowAction;
            self.players[index].shortbow_action_frames = SHORTBOW_ACTION_FRAMES;
            return;
        }
        if input.longbow {
            self.buffered_shortbow_frames[index] = 0;
            self.update_aim(index, input, true);
            self.players[index].phase = PlayerPhase::LongbowCharge;
            self.players[index].longbow_charge_frames = 1;
            let side = self.players[index].side;
            if let Some(event) = self
                .tactical_events
                .record_longbow_charge_started(side, self.frame.saturating_add(1))
            {
                self.events.push(SimEvent::Tactical(event));
            }
        }
    }

    /// Keeps the fixed shortbow follow-through active while movement continues.
    fn update_shortbow_action(&mut self, index: usize, input: PlayerInput) {
        self.update_aim(index, input, false);
        self.apply_movement(index, input, 1.0);
        if self.players[index].shortbow_action_frames == 0 {
            self.players[index].phase = PlayerPhase::Move;
        }
    }

    /// Advances or cancels charge, releasing only after the full charge threshold.
    fn update_longbow_charge(&mut self, index: usize, input: PlayerInput) {
        self.update_aim(index, input, true);
        self.apply_movement(index, input, LONGBOW_CHARGE_MOVE_RATIO);
        if input.longbow {
            let previous_charge = self.players[index].longbow_charge_frames;
            let charge_frames = (previous_charge + 1).min(LONGBOW_CHARGE_FRAMES);
            self.players[index].longbow_charge_frames = charge_frames;
            if previous_charge < LONGBOW_CHARGE_FRAMES && charge_frames == LONGBOW_CHARGE_FRAMES {
                self.events.push(SimEvent::LongbowChargeReady {
                    attacker: self.players[index].side,
                });
            }
            return;
        }

        if self.players[index].longbow_charge_frames >= LONGBOW_CHARGE_FRAMES {
            self.fire_longbow(index);
        } else {
            let side = self.players[index].side;
            self.tactical_events
                .record_longbow_charge_cancelled(side, self.frame.saturating_add(1));
            self.players[index].longbow_charge_frames = 0;
            self.players[index].phase = PlayerPhase::Move;
        }
    }

    /// Counts down vulnerability and clears its refresh limit when the window ends.
    fn update_damaged_player(&mut self, index: usize) {
        let player = &mut self.players[index];
        player.damage_remaining_frames = player.damage_remaining_frames.saturating_sub(1);
        if player.damage_remaining_frames == 0 {
            player.pressure_count = 0;
            player.damage_end_feedback_frames = DAMAGED_END_FEEDBACK_FRAMES;
            player.phase = PlayerPhase::Move;
        }
    }

    /// Converts directional intent into capped acceleration without moving this frame.
    fn apply_movement(&mut self, index: usize, input: PlayerInput, move_ratio: f32) {
        let mut acceleration = Vec2::new(
            f32::from(input.right) - f32::from(input.left),
            f32::from(input.down) - f32::from(input.up),
        );
        if acceleration.x != 0.0 && acceleration.y != 0.0 {
            acceleration = acceleration.scale(std::f32::consts::FRAC_1_SQRT_2);
        }
        let player = &mut self.players[index];
        player.velocity.x =
            (player.velocity.x + acceleration.x * move_ratio).clamp(-PLAYER_MAX_VX, PLAYER_MAX_VX);
        player.velocity.y =
            (player.velocity.y + acceleration.y * move_ratio).clamp(-PLAYER_MAX_VY, PLAYER_MAX_VY);
    }

    /// Resolves arena bounds and cover overlaps after the previous velocity moves.
    fn clamp_player_to_arena(&mut self, index: usize) {
        let player = &mut self.players[index];
        if player.position.x < PLAYER_RADIUS {
            player.position.x = PLAYER_RADIUS;
            player.velocity.x *= -PLAYER_BOUNCE;
        }
        if player.position.x > ARENA_WIDTH - PLAYER_RADIUS {
            player.position.x = ARENA_WIDTH - PLAYER_RADIUS;
            player.velocity.x *= -PLAYER_BOUNCE;
        }
        if player.position.y < PLAYER_RADIUS {
            player.position.y = PLAYER_RADIUS;
            player.velocity.y *= -PLAYER_BOUNCE;
        }
        if player.position.y > ARENA_HEIGHT - PLAYER_RADIUS {
            player.position.y = ARENA_HEIGHT - PLAYER_RADIUS;
            player.velocity.y *= -PLAYER_BOUNCE;
        }
        self.config
            .arena
            .resolve_player(&mut player.position, &mut player.velocity);
    }

    /// Uses target lock before manual aim for longbow and always auto-aims the shortbow.
    fn update_aim(&mut self, index: usize, input: PlayerInput, use_longbow_lock: bool) {
        let target = self.players[index].side.opponent().index();
        let delta = self.players[target]
            .position
            .minus(self.players[index].position);
        let target_distance_squared = delta.length_squared();
        let target_in_longbow_range = target_distance_squared <= LONGBOW_AUTO_AIM_RANGE.powi(2);
        let target_path_is_clear = self.config.arena.has_clear_projectile_path(
            self.players[index].position,
            self.players[target].position,
            if use_longbow_lock {
                LONGBOW_COMPONENT_RADIUS
            } else {
                SHORTBOW_ARROW_RADIUS
            },
        );
        if !use_longbow_lock && input.intercept_aim {
            if let Some(angle) = input.aim_angle {
                self.players[index].aim_angle = angle;
                return;
            }
        }
        if !use_longbow_lock || (target_in_longbow_range && target_path_is_clear) {
            self.players[index].aim_angle = delta.y.atan2(delta.x);
        } else if let Some(angle) = input.aim_angle {
            self.players[index].aim_angle = angle;
        } else {
            let horizontal = f32::from(input.right) - f32::from(input.left);
            self.players[index].aim_angle +=
                horizontal * LONGBOW_AIM_SPEED_RATIO * std::f32::consts::TAU / FPS as f32;
        }
    }

    /// Consumes one reserve arrow and creates a nonlethal projectile at its launch offset.
    fn fire_shortbow(&mut self, index: usize) {
        let player = self.players[index];
        if player.shortbow_ammo == 0 || player.shortbow_cooldown_frames > 0 {
            return;
        }
        let direction = Vec2::from_angle(player.aim_angle);
        let position = player.position.plus(direction.scale(24.0));
        self.arrows.push(ArrowState::new(
            player.side,
            ArrowKind::Shortbow,
            position,
            player.aim_angle,
            SHORTBOW_ARROW_START_SPEED,
        ));
        let player_mut = &mut self.players[index];
        player_mut.shortbow_ammo -= 1;
        player_mut.shortbow_recovery_frames = 0;
        player_mut.shortbow_cooldown_frames = SHORTBOW_FIRE_INTERVAL_FRAMES;
        self.events.push(SimEvent::ShortbowFired {
            attacker: player.side,
        });
    }

    /// Creates the ordered shaft/head volley and starts the post-shot recovery timer.
    fn fire_longbow(&mut self, index: usize) {
        let player = self.players[index];
        let direction = Vec2::from_angle(player.aim_angle);
        for shaft_index in 0..LONGBOW_SHAFT_COUNT {
            let position = player
                .position
                .plus(direction.scale(shaft_index as f32 * LONGBOW_COMPONENT_INTERVAL));
            self.arrows.push(ArrowState::new(
                player.side,
                ArrowKind::LongbowShaft,
                position,
                player.aim_angle,
                LONGBOW_SPEED,
            ));
        }
        let head_position = player
            .position
            .plus(direction.scale(LONGBOW_SHAFT_COUNT as f32 * LONGBOW_COMPONENT_INTERVAL));
        self.arrows.push(ArrowState::new(
            player.side,
            ArrowKind::LongbowHead,
            head_position,
            player.aim_angle,
            LONGBOW_SPEED,
        ));
        let player_mut = &mut self.players[index];
        player_mut.longbow_charge_frames = 0;
        player_mut.longbow_recovery_frames = LONGBOW_RECOVERY_FRAMES;
        player_mut.phase = PlayerPhase::Move;
        self.events.push(SimEvent::LongbowFired {
            attacker: player.side,
        });
    }

    /// Removes every projectile whose swept path reaches solid cover this frame.
    fn resolve_cover_impacts(&mut self) {
        for arrow in &mut self.arrows {
            if arrow.removed {
                continue;
            }
            if let Some(impact) = self.config.arena.first_cover_impact(
                arrow.previous_position,
                arrow.position,
                arrow.kind.radius(),
            ) {
                arrow.removed = true;
                self.events.push(SimEvent::CoverImpact {
                    position: impact.point,
                    normal: impact.normal,
                });
            }
        }
    }

    /// Resolves only the earliest opposing-arrow contact in the current frame.
    fn resolve_arrow_interception(&mut self) {
        let mut earliest: Option<(usize, usize, f32, Vec2)> = None;
        for first_index in 0..self.arrows.len() {
            let first = self.arrows[first_index];
            if first.removed {
                continue;
            }
            for second_index in (first_index + 1)..self.arrows.len() {
                let second = self.arrows[second_index];
                if second.removed || first.owner == second.owner {
                    continue;
                }
                let Some((time_ratio, position)) = swept_circle_collision(first, second) else {
                    continue;
                };
                if earliest.is_none_or(|candidate| time_ratio < candidate.2) {
                    earliest = Some((first_index, second_index, time_ratio, position));
                }
            }
        }
        if let Some((first_index, second_index, _, position)) = earliest {
            self.arrows[first_index].removed = true;
            self.arrows[second_index].removed = true;
            self.events.push(SimEvent::ArrowIntercepted { position });
            self.events
                .push(SimEvent::Tactical(TacticalEvent::intercept(
                    self.frame.saturating_add(1),
                )));
        }
    }

    /// Processes player one then player two, allowing both sides to hit in one frame.
    fn resolve_player_hits(&mut self) {
        for attacker in [Side::One, Side::Two] {
            let target = attacker.opponent();
            for arrow_index in 0..self.arrows.len() {
                let arrow = self.arrows[arrow_index];
                if arrow.removed || arrow.owner != attacker {
                    continue;
                }
                let target_player = self.players[target.index()];
                if target_player.phase == PlayerPhase::Dead {
                    break;
                }
                let distance = arrow
                    .position
                    .minus(target_player.position)
                    .length_squared();
                let collision_distance = arrow.kind.radius() + PLAYER_RADIUS;
                if distance >= collision_distance * collision_distance {
                    continue;
                }

                self.arrows[arrow_index].removed = true;
                if arrow.kind.is_lethal() {
                    self.players[target.index()].phase = PlayerPhase::Dead;
                    self.events.push(SimEvent::LongbowHit { attacker, target });
                    if let Some(event) = self
                        .tactical_events
                        .record_longbow_finish(attacker, self.frame.saturating_add(1))
                    {
                        self.events.push(SimEvent::Tactical(event));
                    }
                    break;
                }

                let thrust = arrow
                    .velocity
                    .normalized_or(Vec2::from_angle(arrow.rotation_angle))
                    .scale(PLAYER_THRUST_SPEED);
                let charge_interrupted =
                    self.players[target.index()].phase == PlayerPhase::LongbowCharge;
                let pressure_refreshed = self.players[target.index()].pressure_count
                    < SHORTBOW_MAX_CONSECUTIVE_PRESSURES;
                let target_player = &mut self.players[target.index()];
                target_player.velocity = thrust;
                target_player.phase = PlayerPhase::Damaged;
                if pressure_refreshed {
                    target_player.pressure_count += 1;
                    target_player.damage_remaining_frames = DAMAGED_FRAMES;
                    target_player.damage_end_feedback_frames = 0;
                }
                if charge_interrupted && pressure_refreshed {
                    target_player.longbow_charge_frames = 0;
                    self.tactical_events
                        .record_longbow_charge_cancelled(target, self.frame.saturating_add(1));
                }
                self.events.push(SimEvent::ShortbowHit { attacker, target });
                let pressure_event = self
                    .tactical_events
                    .record_pressure(attacker, self.frame.saturating_add(1));
                self.events.push(SimEvent::Tactical(pressure_event));
                if charge_interrupted {
                    self.events.push(SimEvent::Tactical(TacticalEvent::for_side(
                        attacker,
                        TacticalEventKind::Disrupt,
                        self.frame.saturating_add(1),
                    )));
                }
            }
        }

        if self.players[Side::One.index()].phase == PlayerPhase::Dead
            || self.players[Side::Two.index()].phase == PlayerPhase::Dead
        {
            let winner = if self.players[Side::One.index()].phase == PlayerPhase::Dead {
                Side::Two
            } else {
                Side::One
            };
            self.finish_round(winner);
        }
    }

    /// Scores one completed round and freezes its immutable wire-compatible result.
    fn finish_round(&mut self, winner: Side) {
        match winner {
            Side::One => self.score.player_one_wins += 1,
            Side::Two => self.score.player_two_wins += 1,
        }
        let round_number = self.score.player_one_wins + self.score.player_two_wins;
        self.last_round_result = Some(
            RoundResult::new(
                round_number,
                winner.winner_side(),
                self.score.player_one_wins,
                self.score.player_two_wins,
                self.score.is_complete(),
            )
            .expect("round number is positive after scoring"),
        );
    }
}

/// Returns first-contact time and midpoint for two projectile paths during one tick.
fn swept_circle_collision(first: ArrowState, second: ArrowState) -> Option<(f32, Vec2)> {
    let first_step = first.position.minus(first.previous_position);
    let second_step = second.position.minus(second.previous_position);
    let relative_start = first.previous_position.minus(second.previous_position);
    let relative_step = first_step.minus(second_step);
    let collision_distance = first.kind.radius() + second.kind.radius();
    let start_distance_squared = relative_start.length_squared();
    let collision_distance_squared = collision_distance * collision_distance;
    let relative_speed_squared = relative_step.length_squared();
    let approach_dot = relative_start.dot(relative_step);

    let time_ratio = if start_distance_squared < collision_distance_squared {
        0.0
    } else {
        if relative_speed_squared <= 0.0 || approach_dot >= 0.0 {
            return None;
        }
        let distance_offset = start_distance_squared - collision_distance_squared;
        let discriminant = approach_dot * approach_dot - relative_speed_squared * distance_offset;
        if discriminant <= 0.0 {
            return None;
        }
        let first_contact = (-approach_dot - discriminant.sqrt()) / relative_speed_squared;
        if !(0.0..=1.0).contains(&first_contact) {
            return None;
        }
        first_contact
    };

    let first_impact = first.previous_position.plus(first_step.scale(time_ratio));
    let second_impact = second.previous_position.plus(second_step.scale(time_ratio));
    Some((time_ratio, first_impact.plus(second_impact).scale(0.5)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_until_event(simulation: &mut Simulation, inputs: [PlayerInput; 2], expected: SimEvent) {
        for _ in 0..120 {
            simulation.step(inputs);
            if simulation.snapshot().events.contains(&expected) {
                return;
            }
        }
        panic!("expected event {expected:?} was not observed");
    }

    #[test]
    fn default_spawns_and_empty_input_are_deterministic() {
        let mut first = Simulation::new(SimulationConfig::default(), 42);
        let mut second = Simulation::new(SimulationConfig::default(), 42);
        for _ in 0..30 {
            first.step([PlayerInput::empty(); 2]);
            second.step([PlayerInput::empty(); 2]);
        }
        assert_eq!(first.snapshot(), second.snapshot());
        assert_eq!(first.players()[0].position, Vec2::new(640.0, 620.0));
        assert_eq!(first.players()[1].position, Vec2::new(640.0, 100.0));
    }

    #[test]
    fn diagonal_movement_is_normalized_and_bounds_bounce() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 1);
        let input = PlayerInput {
            up: true,
            right: true,
            ..PlayerInput::empty()
        };
        simulation.step([input, PlayerInput::empty()]);
        let first_frame = simulation.players()[0];
        assert_eq!(first_frame.position, Vec2::new(640.0, 620.0));
        assert!(first_frame.velocity.x > 0.0);
        assert!(first_frame.velocity.y < 0.0);
        simulation.step([input, PlayerInput::empty()]);
        let player = simulation.players()[0];
        assert!(player.position.x > 640.0);
        assert!(player.position.y < 620.0);
        assert!(player.velocity.x.abs() <= PLAYER_MAX_VX);
        assert!(player.velocity.y.abs() <= PLAYER_MAX_VY);
    }

    #[test]
    fn shortbow_hit_applies_knockback_and_damage_window() {
        let config = SimulationConfig {
            spawn_positions: [Vec2::new(640.0, 620.0), Vec2::new(640.0, 580.0)],
            ..SimulationConfig::default()
        };
        let mut simulation = Simulation::new(config, 5);
        let input = PlayerInput {
            shortbow: true,
            aim_angle: Some(-std::f32::consts::FRAC_PI_2),
            ..PlayerInput::empty()
        };
        run_until_event(
            &mut simulation,
            [input, PlayerInput::empty()],
            SimEvent::ShortbowHit {
                attacker: Side::One,
                target: Side::Two,
            },
        );
        let target = simulation.players()[1];
        assert_eq!(target.phase, PlayerPhase::Damaged);
        assert_eq!(target.damage_remaining_frames, DAMAGED_FRAMES);
        assert!(target.velocity.y < 0.0);
        simulation.step([PlayerInput::empty(), PlayerInput::empty()]);
        assert!(simulation.players()[1].position.y < 580.0);
    }

    #[test]
    fn third_shortbow_hit_keeps_knockback_without_refreshing_pressure() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 15);
        let target = &mut simulation.players[1];
        target.phase = PlayerPhase::Damaged;
        target.damage_remaining_frames = 10;
        target.pressure_count = SHORTBOW_MAX_CONSECUTIVE_PRESSURES;
        simulation.arrows.push(ArrowState::new(
            Side::One,
            ArrowKind::Shortbow,
            target.position,
            0.0,
            SHORTBOW_ARROW_START_SPEED,
        ));

        simulation.resolve_player_hits();

        let target = simulation.players()[1];
        assert_eq!(target.phase, PlayerPhase::Damaged);
        assert_eq!(target.damage_remaining_frames, 10);
        assert_eq!(target.pressure_count, SHORTBOW_MAX_CONSECUTIVE_PRESSURES);
        assert!(target.velocity.x > 0.0);
    }

    #[test]
    fn opposing_shortbow_arrows_can_hit_both_players_in_one_frame() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 17);
        for attacker in [Side::One, Side::Two] {
            let target_position = simulation.players[attacker.opponent().index()].position;
            simulation.arrows.push(ArrowState::new(
                attacker,
                ArrowKind::Shortbow,
                target_position,
                0.0,
                SHORTBOW_ARROW_START_SPEED,
            ));
        }

        simulation.resolve_player_hits();

        assert_eq!(
            simulation
                .events
                .iter()
                .filter(|event| matches!(event, SimEvent::ShortbowHit { .. }))
                .count(),
            2
        );
        assert_eq!(simulation.players()[0].pressure_count, 1);
        assert_eq!(simulation.players()[1].pressure_count, 1);
    }

    #[test]
    fn simultaneous_lethal_hits_keep_player_two_win_tiebreak() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 18);
        for attacker in [Side::One, Side::Two] {
            let target_position = simulation.players[attacker.opponent().index()].position;
            simulation.arrows.push(ArrowState::new(
                attacker,
                ArrowKind::LongbowHead,
                target_position,
                0.0,
                LONGBOW_SPEED,
            ));
        }

        simulation.resolve_player_hits();

        let result = simulation
            .round_result()
            .expect("simultaneous lethal hits finish the round");
        assert_eq!(result.winner(), WinnerSide::SideTwo);
        assert_eq!(simulation.players()[0].phase, PlayerPhase::Dead);
        assert_eq!(simulation.players()[1].phase, PlayerPhase::Dead);
    }

    #[test]
    fn ending_a_damage_window_resets_its_shortbow_pressure_limit() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 16);
        simulation.players[0].phase = PlayerPhase::Damaged;
        simulation.players[0].damage_remaining_frames = 1;
        simulation.players[0].pressure_count = SHORTBOW_MAX_CONSECUTIVE_PRESSURES;
        simulation.step([PlayerInput::empty(); 2]);

        let player = simulation.players()[0];
        assert_eq!(player.phase, PlayerPhase::Move);
        assert_eq!(player.damage_remaining_frames, 0);
        assert_eq!(player.pressure_count, 0);
        assert_eq!(
            player.damage_end_feedback_frames,
            DAMAGED_END_FEEDBACK_FRAMES
        );
    }

    #[test]
    fn shortbow_uses_auto_aim_instead_of_manual_mouse_angle() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 11);
        let input = PlayerInput {
            shortbow: true,
            aim_angle: Some(0.0),
            ..PlayerInput::empty()
        };
        simulation.step([input, PlayerInput::empty()]);
        let arrow = simulation.snapshot().arrows[0];
        assert!((arrow.rotation_angle + std::f32::consts::FRAC_PI_2).abs() < 0.0001);
        assert_eq!(SHORTBOW_ARROW_RADIUS, 8.0);
    }

    #[test]
    fn longbow_lock_takes_priority_over_manual_angle_for_visible_targets() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 12);
        let input = PlayerInput {
            longbow: true,
            aim_angle: Some(0.0),
            ..PlayerInput::empty()
        };
        simulation.step([input, PlayerInput::empty()]);
        assert!((simulation.players()[0].aim_angle + std::f32::consts::FRAC_PI_2).abs() < 0.0001);
    }

    #[test]
    fn longbow_uses_manual_angle_when_target_is_outside_lock_range() {
        let config = SimulationConfig {
            spawn_positions: [Vec2::new(640.0, 620.0), Vec2::new(640.0, 20.0)],
            ..SimulationConfig::default()
        };
        let mut simulation = Simulation::new(config, 13);
        let input = PlayerInput {
            longbow: true,
            aim_angle: Some(0.25),
            ..PlayerInput::empty()
        };
        simulation.step([input, PlayerInput::empty()]);
        assert_eq!(simulation.players()[0].aim_angle, 0.25);
    }

    #[test]
    fn explicit_ai_intercept_aim_overrides_shortbow_auto_aim() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 14);
        simulation.players[0].phase = PlayerPhase::ShortbowAction;
        simulation.players[0].shortbow_action_frames = SHORTBOW_ACTION_FRAMES;
        simulation.arrows.push(ArrowState::new(
            Side::Two,
            ArrowKind::Shortbow,
            Vec2::new(640.0, 570.0),
            std::f32::consts::FRAC_PI_2,
            8.0,
        ));
        let input = PlayerInput {
            aim_angle: Some(std::f32::consts::FRAC_PI_2),
            intercept_aim: true,
            ..PlayerInput::empty()
        };
        simulation.step([input, PlayerInput::empty()]);
        assert!((simulation.players()[0].aim_angle - std::f32::consts::FRAC_PI_2).abs() < 0.0001);
    }

    #[test]
    fn central_cover_blocks_a_projectile_path() {
        let cover = ArenaKind::CentralCover;
        let impact = cover
            .first_cover_impact(Vec2::new(400.0, 360.0), Vec2::new(880.0, 360.0), 20.0)
            .expect("path should touch central cover");
        assert!(impact.point.x <= 500.0);
        assert_eq!(impact.normal, Vec2::new(-1.0, 0.0));
        assert!(!cover.has_clear_projectile_path(
            Vec2::new(400.0, 360.0),
            Vec2::new(880.0, 360.0),
            20.0
        ));
    }

    #[test]
    fn longbow_release_scores_a_round_and_freezes_result() {
        let config = SimulationConfig {
            spawn_positions: [Vec2::new(640.0, 620.0), Vec2::new(640.0, 120.0)],
            rounds_to_win: 1,
            ..SimulationConfig::default()
        };
        let mut simulation = Simulation::new(config, 7);
        let held = PlayerInput {
            longbow: true,
            aim_angle: Some(-std::f32::consts::FRAC_PI_2),
            ..PlayerInput::empty()
        };
        for _ in 0..LONGBOW_CHARGE_FRAMES {
            simulation.step([held, PlayerInput::empty()]);
        }
        simulation.step([PlayerInput::empty(), PlayerInput::empty()]);
        for _ in 0..20 {
            simulation.step([PlayerInput::empty(), PlayerInput::empty()]);
            if simulation.round_result().is_some() {
                break;
            }
        }
        let result = simulation
            .round_result()
            .expect("longbow should finish the round");
        assert_eq!(result.winner(), WinnerSide::SideOne);
        assert!(result.match_complete());
        let frozen = simulation.snapshot();
        simulation.step([held, held]);
        assert_eq!(simulation.snapshot(), frozen);
    }

    #[test]
    fn longbow_charge_ready_emits_one_event_at_the_threshold() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 44);
        let charging = PlayerInput {
            longbow: true,
            ..PlayerInput::empty()
        };
        let mut ready_events = 0;

        for _ in 0..LONGBOW_CHARGE_FRAMES {
            simulation.step([charging, PlayerInput::empty()]);
            ready_events += simulation
                .events()
                .iter()
                .filter(|event| {
                    **event
                        == SimEvent::LongbowChargeReady {
                            attacker: Side::One,
                        }
                })
                .count();
        }
        assert_eq!(ready_events, 1);

        simulation.step([charging, PlayerInput::empty()]);
        assert!(
            !simulation.events().contains(&SimEvent::LongbowChargeReady {
                attacker: Side::One,
            })
        );
    }

    #[test]
    fn reset_round_clears_projectiles_and_preserves_score() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 9);
        let input = PlayerInput {
            shortbow: true,
            ..PlayerInput::empty()
        };
        simulation.step([input, PlayerInput::empty()]);
        assert!(!simulation.snapshot().arrows.is_empty());
        simulation.reset_round();
        let snapshot = simulation.snapshot();
        assert!(snapshot.arrows.is_empty());
        assert_eq!(snapshot.score.player_one_wins, 0);
        assert_eq!(snapshot.frame, 0);
    }
}
