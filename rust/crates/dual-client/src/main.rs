use audio_feedback::AudioFeedback;
use dual_sim::{
    ARENA_HEIGHT, ARENA_WIDTH, AiController, AiDifficulty, ArenaKind, FPS, FrameSnapshot,
    PLAYER_BODY_SIZE, PlayerInput, PlayerPhase, Side, SimEvent, Simulation, SimulationConfig,
    Vec2 as WorldVec2,
};
use macroquad::prelude::*;
use settings::AudioSettings;

mod audio_feedback;
mod effects;
mod settings;

use effects::VisualEffects;

const LOGICAL_STEP_SECONDS: f32 = 1.0 / FPS as f32;
const MAX_CATCH_UP_STEPS: usize = 8;
const MAX_PRESS_EDGE_RENDER_FRAMES: u8 = 8;
const MATCH_SEED: u64 = 0x4455_414c;

/// Selects which input producers own the two simulation sides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameMode {
    Demo,
    VersusAi,
    LocalTwoPlayer,
}

/// Carries presentation-only state from the loop into the immutable-snapshot HUD.
struct HudState<'a> {
    mode_label: &'a str,
    arena: ArenaKind,
    game_mode: GameMode,
    paused: bool,
    settings_open: bool,
    audio_volume_label: &'a str,
}

/// Defines the clickable actions on the settings overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsPointerAction {
    Open,
    Close,
    DecreaseVolume,
    ToggleMute,
    IncreaseVolume,
}

/// Retains one-frame weapon presses until the next fixed simulation tick.
#[derive(Default)]
struct PressEdges {
    player_one_shortbow: bool,
    player_one_age: u8,
    player_two_shortbow: bool,
    player_two_age: u8,
}

impl PressEdges {
    /// Queues render-frame edges and expires them if a fixed tick stalls or focus changes.
    fn capture(&mut self, player_one: bool, player_two: bool) {
        Self::capture_one(
            &mut self.player_one_shortbow,
            &mut self.player_one_age,
            player_one,
        );
        Self::capture_one(
            &mut self.player_two_shortbow,
            &mut self.player_two_age,
            player_two,
        );
    }

    fn capture_one(pending: &mut bool, age: &mut u8, pressed: bool) {
        if pressed {
            *pending = true;
            *age = MAX_PRESS_EDGE_RENDER_FRAMES;
        } else if *pending {
            *age = age.saturating_sub(1);
            if *age == 0 {
                *pending = false;
            }
        }
    }

    /// Delivers a queued player-one shot once, then clears its pending edge.
    fn take_player_one_shortbow(&mut self) -> bool {
        let pressed = self.player_one_shortbow;
        self.player_one_shortbow = false;
        self.player_one_age = 0;
        pressed
    }

    /// Delivers the queued player-two shot once, then clears its pending edge.
    fn take_player_two_shortbow(&mut self) -> bool {
        let pressed = self.player_two_shortbow;
        self.player_two_shortbow = false;
        self.player_two_age = 0;
        pressed
    }

    /// Drops queued shots while paused or after changing/resetting a match.
    fn clear(&mut self) {
        self.player_one_shortbow = false;
        self.player_one_age = 0;
        self.player_two_shortbow = false;
        self.player_two_age = 0;
    }
}

/// Captures Miniquad focus-loss notifications that Macroquad clears from held input.
#[derive(Default)]
struct WindowFocusEvents {
    lost_focus: bool,
}

impl WindowFocusEvents {
    /// Consumes one focus-loss notification so the caller can pause exactly once.
    fn take_lost_focus(&mut self) -> bool {
        std::mem::take(&mut self.lost_focus)
    }
}

impl macroquad::miniquad::EventHandler for WindowFocusEvents {
    fn update(&mut self) {}

    fn draw(&mut self) {}

    fn window_minimized_event(&mut self) {
        self.lost_focus = true;
    }
}

/// Maps the fixed 1280×720 rules world into a centered, aspect-preserving window.
#[derive(Clone, Copy)]
struct Viewport {
    /// Window-space origin of the logical arena.
    left: f32,
    /// Window-space top edge after aspect-ratio letterboxing.
    top: f32,
    /// Uniform scale applied to world coordinates and drawing sizes.
    scale: f32,
}

impl Viewport {
    /// Reads the current window dimensions and builds its logical-canvas transform.
    fn from_window() -> Self {
        Self::for_size(screen_width(), screen_height())
    }

    /// Keeps the arena fully visible at any positive window size without stretching it.
    fn for_size(width: f32, height: f32) -> Self {
        let scale = (width / ARENA_WIDTH)
            .min(height / ARENA_HEIGHT)
            .max(f32::EPSILON);
        Self {
            left: (width - ARENA_WIDTH * scale) * 0.5,
            top: (height - ARENA_HEIGHT * scale) * 0.5,
            scale,
        }
    }

    /// Converts one rules-world point into window pixels.
    fn world_to_screen(self, point: WorldVec2) -> macroquad::math::Vec2 {
        vec2(
            self.left + point.x * self.scale,
            self.top + point.y * self.scale,
        )
    }

    /// Converts mouse coordinates back into the simulation's fixed logical arena.
    fn screen_to_world(self, point: macroquad::math::Vec2) -> WorldVec2 {
        WorldVec2::new(
            (point.x - self.left) / self.scale,
            (point.y - self.top) / self.scale,
        )
    }

    /// Scales one logical drawing dimension with the arena.
    fn logical_size(self, size: f32) -> f32 {
        size * self.scale
    }
}

/// Requests the resizable, high-DPI desktop window used by the local client.
fn window_conf() -> Conf {
    Conf {
        window_title: "Dual".to_owned(),
        window_width: 1920,
        window_height: 1080,
        window_resizable: true,
        high_dpi: true,
        ..Default::default()
    }
}

/// Runs input collection, the bounded fixed-step simulation loop, and snapshot rendering.
#[macroquad::main(window_conf)]
async fn main() {
    let audio_feedback = AudioFeedback::start();
    let mut arena = ArenaKind::Open;
    let mut difficulty = AiDifficulty::Standard;
    let mut game_mode = GameMode::Demo;
    let mut simulation = new_simulation(arena);
    let mut ai = create_ai_pair(game_mode, difficulty, simulation.seed());
    let mut accumulator = 0.0;
    let mut paused = false;
    let mut settings_open = false;
    let mut audio_settings = AudioSettings::default();
    let mut visual_effects = VisualEffects::default();
    let mut press_edges = PressEdges::default();
    let mut focus_events = WindowFocusEvents::default();
    let focus_subscriber = macroquad::input::utils::register_input_subscriber();

    loop {
        let viewport = Viewport::from_window();
        macroquad::input::utils::repeat_all_miniquad_input(&mut focus_events, focus_subscriber);
        if focus_events.take_lost_focus() {
            paused = true;
            accumulator = 0.0;
            press_edges.clear();
        }
        let settings_was_open = settings_open;
        let mut reset_match = false;
        if settings_open {
            if is_key_pressed(KeyCode::Escape) {
                settings_open = false;
            } else if is_key_pressed(KeyCode::Equal) || is_key_pressed(KeyCode::KpAdd) {
                audio_settings.increase_volume();
            } else if is_key_pressed(KeyCode::Minus) || is_key_pressed(KeyCode::KpSubtract) {
                audio_settings.decrease_volume();
            } else if is_key_pressed(KeyCode::M) {
                audio_settings.toggle_mute();
            } else if let Some(action) = settings_pointer_action(viewport, true) {
                match action {
                    SettingsPointerAction::DecreaseVolume => audio_settings.decrease_volume(),
                    SettingsPointerAction::ToggleMute => audio_settings.toggle_mute(),
                    SettingsPointerAction::IncreaseVolume => audio_settings.increase_volume(),
                    SettingsPointerAction::Close => settings_open = false,
                    SettingsPointerAction::Open => {}
                }
            }
            accumulator = 0.0;
            press_edges.clear();
        } else if is_key_pressed(KeyCode::O)
            || settings_pointer_action(viewport, false) == Some(SettingsPointerAction::Open)
        {
            settings_open = true;
            accumulator = 0.0;
            press_edges.clear();
        } else {
            if is_key_pressed(KeyCode::P) {
                paused = !paused;
            }
            if is_key_pressed(KeyCode::Key1) {
                difficulty = AiDifficulty::Basic;
                game_mode = GameMode::VersusAi;
                reset_match = true;
            } else if is_key_pressed(KeyCode::Key2) {
                difficulty = AiDifficulty::Standard;
                game_mode = GameMode::VersusAi;
                reset_match = true;
            } else if is_key_pressed(KeyCode::Key3) {
                difficulty = AiDifficulty::Advanced;
                game_mode = GameMode::VersusAi;
                reset_match = true;
            } else if is_key_pressed(KeyCode::Key4) {
                game_mode = GameMode::LocalTwoPlayer;
                reset_match = true;
            } else if is_key_pressed(KeyCode::M) {
                game_mode = GameMode::Demo;
                reset_match = true;
            }
            if is_key_pressed(KeyCode::C) {
                arena = match arena {
                    ArenaKind::Open => ArenaKind::CentralCover,
                    ArenaKind::CentralCover => ArenaKind::Open,
                };
                reset_match = true;
            }
            if reset_match {
                simulation = new_simulation(arena);
                ai = create_ai_pair(game_mode, difficulty, simulation.seed());
                visual_effects.clear();
                paused = false;
                accumulator = 0.0;
                press_edges.clear();
            }
        }

        if paused || settings_open || settings_was_open {
            press_edges.clear();
        } else {
            press_edges.capture(
                game_mode != GameMode::Demo
                    && (is_key_pressed(KeyCode::Z) || is_mouse_button_pressed(MouseButton::Left)),
                game_mode == GameMode::LocalTwoPlayer && is_key_pressed(KeyCode::B),
            );
        }

        let settings_transition_freeze = settings_open || settings_was_open;
        if should_advance_simulation(paused, settings_transition_freeze) {
            accumulator += get_frame_time().min(0.25);
        }
        let mut steps = 0;
        while should_advance_simulation(paused, settings_transition_freeze)
            && accumulator >= LOGICAL_STEP_SECONDS
            && steps < MAX_CATCH_UP_STEPS
        {
            let snapshot = simulation.snapshot();
            let player_one_input = if let Some(ai) = &mut ai[0] {
                ai.next_input(&snapshot)
            } else {
                read_local_input(&snapshot, viewport, press_edges.take_player_one_shortbow())
            };
            let player_two_input = if let Some(ai) = &mut ai[1] {
                ai.next_input(&snapshot)
            } else if game_mode == GameMode::LocalTwoPlayer {
                read_local_opponent_input(press_edges.take_player_two_shortbow())
            } else {
                PlayerInput::empty()
            };
            simulation.step([player_one_input, player_two_input]);
            visual_effects.advance(&simulation.snapshot());
            if simulation.events().contains(&SimEvent::LongbowChargeReady {
                attacker: Side::One,
            }) || simulation.events().contains(&SimEvent::LongbowChargeReady {
                attacker: Side::Two,
            }) {
                if let Some(audio_feedback) = &audio_feedback {
                    audio_feedback.play_charge_ready(audio_settings.volume());
                }
            }
            accumulator -= LOGICAL_STEP_SECONDS;
            steps += 1;
        }
        if steps == MAX_CATCH_UP_STEPS {
            accumulator = accumulator.min(LOGICAL_STEP_SECONDS);
        }

        if !settings_open && is_key_pressed(KeyCode::R) {
            if simulation.round_result().is_some() && simulation.score().is_complete() {
                simulation = new_simulation(arena);
                ai = create_ai_pair(game_mode, difficulty, simulation.seed());
            } else {
                simulation.reset_round();
            }
            visual_effects.clear();
            accumulator = 0.0;
            press_edges.clear();
        }

        clear_background(Color::from_rgba(24, 28, 36, 255));
        let mode_label = match game_mode {
            GameMode::Demo => "DEMO",
            GameMode::LocalTwoPlayer => "LOCAL TWO PLAYER",
            GameMode::VersusAi => match difficulty {
                AiDifficulty::Basic => "AI: BASIC",
                AiDifficulty::Standard => "AI: STANDARD",
                AiDifficulty::Advanced => "AI: ADVANCED",
            },
        };
        draw_snapshot(
            &simulation,
            &simulation.snapshot(),
            &visual_effects,
            viewport,
            HudState {
                mode_label,
                arena,
                game_mode,
                paused,
                settings_open,
                audio_volume_label: audio_settings.label(),
            },
        );
        next_frame().await;
    }
}

/// Creates the seeded input producers for the selected two-player mode.
fn create_ai_pair(
    mode: GameMode,
    difficulty: AiDifficulty,
    seed: u64,
) -> [Option<AiController>; 2] {
    match mode {
        GameMode::Demo => [
            Some(AiController::new(Side::One, difficulty, seed)),
            Some(AiController::new(Side::Two, difficulty, seed)),
        ],
        GameMode::VersusAi => [None, Some(AiController::new(Side::Two, difficulty, seed))],
        GameMode::LocalTwoPlayer => [None, None],
    }
}

/// Rebuilds a clean match after arena or play-mode changes.
fn new_simulation(arena: ArenaKind) -> Simulation {
    Simulation::new(
        SimulationConfig {
            arena,
            ..SimulationConfig::default()
        },
        MATCH_SEED,
    )
}

/// Settings and pause overlays both freeze rule time without changing match state.
fn should_advance_simulation(paused: bool, settings_open: bool) -> bool {
    !paused && !settings_open
}

/// Turns the settings button and overlay hit targets into presentation-only menu actions.
fn settings_pointer_action(
    viewport: Viewport,
    settings_open: bool,
) -> Option<SettingsPointerAction> {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return None;
    }
    let (mouse_x, mouse_y) = mouse_position();
    let point = viewport.screen_to_world(vec2(mouse_x, mouse_y));
    if settings_open {
        [
            (
                SettingsPointerAction::DecreaseVolume,
                WorldVec2::new(500.0, 380.0),
                116.0,
                48.0,
            ),
            (
                SettingsPointerAction::ToggleMute,
                WorldVec2::new(640.0, 380.0),
                180.0,
                48.0,
            ),
            (
                SettingsPointerAction::IncreaseVolume,
                WorldVec2::new(780.0, 380.0),
                116.0,
                48.0,
            ),
            (
                SettingsPointerAction::Close,
                WorldVec2::new(640.0, 500.0),
                240.0,
                48.0,
            ),
        ]
        .into_iter()
        .find_map(|(action, center, width, height)| {
            point_in_button(point, center, width, height).then_some(action)
        })
    } else {
        point_in_button(point, WorldVec2::new(1180.0, 44.0), 152.0, 40.0)
            .then_some(SettingsPointerAction::Open)
    }
}

/// Checks a rules-world point against one centered menu button rectangle.
fn point_in_button(point: WorldVec2, center: WorldVec2, width: f32, height: f32) -> bool {
    (center.x - width * 0.5..=center.x + width * 0.5).contains(&point.x)
        && (center.y - height * 0.5..=center.y + height * 0.5).contains(&point.y)
}

/// Samples movement, aim, and the already-latched shortbow edge for player one.
fn read_local_input(
    snapshot: &FrameSnapshot,
    viewport: Viewport,
    shortbow_pressed: bool,
) -> PlayerInput {
    let mouse = {
        let (x, y) = mouse_position();
        viewport.screen_to_world(vec2(x, y))
    };
    let player = snapshot.players[Side::One.index()];
    PlayerInput {
        up: is_key_down(KeyCode::W),
        down: is_key_down(KeyCode::S),
        left: is_key_down(KeyCode::A),
        right: is_key_down(KeyCode::D),
        shortbow: shortbow_pressed,
        longbow: is_key_down(KeyCode::X) || is_mouse_button_down(MouseButton::Right),
        aim_angle: Some((mouse.y - player.position.y).atan2(mouse.x - player.position.x)),
        intercept_aim: false,
    }
}

/// Maps IJKL/B/V controls to the same input intent used by AI and network players.
fn read_local_opponent_input(shortbow_pressed: bool) -> PlayerInput {
    PlayerInput {
        up: is_key_down(KeyCode::I),
        down: is_key_down(KeyCode::K),
        left: is_key_down(KeyCode::J),
        right: is_key_down(KeyCode::L),
        shortbow: shortbow_pressed,
        longbow: is_key_down(KeyCode::V),
        aim_angle: None,
        intercept_aim: false,
    }
}

/// Renders a frame snapshot without changing simulation state.
fn draw_snapshot(
    simulation: &Simulation,
    snapshot: &FrameSnapshot,
    visual_effects: &VisualEffects,
    viewport: Viewport,
    hud: HudState<'_>,
) {
    let arena_top_left = viewport.world_to_screen(WorldVec2::new(0.0, 0.0));
    draw_rectangle(
        arena_top_left.x,
        arena_top_left.y,
        viewport.logical_size(ARENA_WIDTH),
        viewport.logical_size(ARENA_HEIGHT),
        Color::from_rgba(96, 96, 96, 255),
    );
    draw_grid(viewport);
    if simulation.config().arena == ArenaKind::CentralCover {
        draw_cover(viewport);
    }
    for arrow in &snapshot.arrows {
        draw_arrow(*arrow, viewport);
    }
    for player in snapshot.players {
        draw_player(
            player.position,
            player.side,
            player.phase,
            player.aim_angle,
            viewport,
        );
    }
    visual_effects.draw(viewport.left, viewport.top, viewport.scale);
    draw_hud(snapshot, viewport, hud);
}

/// Draws the arena grid inside the letterboxed logical canvas.
fn draw_grid(viewport: Viewport) {
    let grid_color = Color::from_rgba(112, 112, 112, 160);
    for index in 1..6 {
        let fraction = index as f32 / 6.0;
        let x = viewport.left + ARENA_WIDTH * viewport.scale * fraction;
        let y = viewport.top + ARENA_HEIGHT * viewport.scale * fraction;
        draw_line(
            x,
            viewport.top,
            x,
            viewport.top + ARENA_HEIGHT * viewport.scale,
            viewport.logical_size(1.0),
            grid_color,
        );
        draw_line(
            viewport.left,
            y,
            viewport.left + ARENA_WIDTH * viewport.scale,
            y,
            viewport.logical_size(1.0),
            grid_color,
        );
    }
}

/// Draws central cover in the same world coordinates used by collision rules.
fn draw_cover(viewport: Viewport) {
    let cover = ArenaKind::CentralCover
        .central_cover_rect()
        .expect("central cover has one rectangle");
    let top_left = viewport.world_to_screen(WorldVec2::new(cover.left(), cover.top()));
    draw_rectangle(
        top_left.x,
        top_left.y,
        viewport.logical_size(cover.width),
        viewport.logical_size(cover.height),
        Color::from_rgba(64, 64, 72, 235),
    );
    draw_rectangle_lines(
        top_left.x,
        top_left.y,
        viewport.logical_size(cover.width),
        viewport.logical_size(cover.height),
        viewport.logical_size(2.0),
        Color::from_rgba(16, 16, 20, 220),
    );
}

/// Draws one projectile from its immutable simulation snapshot.
fn draw_arrow(arrow: dual_sim::ArrowSnapshot, viewport: Viewport) {
    let position = viewport.world_to_screen(arrow.position);
    let direction = WorldVec2::new(arrow.rotation_angle.cos(), arrow.rotation_angle.sin());
    let length = match arrow.kind {
        dual_sim::ArrowKind::Shortbow => 18.0,
        dual_sim::ArrowKind::LongbowShaft => 28.0,
        dual_sim::ArrowKind::LongbowHead => 42.0,
    };
    let tail = viewport.world_to_screen(arrow.position.minus(direction.scale(length)));
    let color = if arrow.kind == dual_sim::ArrowKind::LongbowHead {
        Color::from_rgba(192, 64, 64, 255)
    } else {
        BLACK
    };
    draw_line(
        tail.x,
        tail.y,
        position.x,
        position.y,
        viewport.logical_size(if arrow.kind == dual_sim::ArrowKind::LongbowHead {
            4.0
        } else {
            2.0
        }),
        color,
    );
}

/// Draws one player body and aim guide from simulation-owned state.
fn draw_player(
    position: WorldVec2,
    side: Side,
    phase: PlayerPhase,
    aim_angle: f32,
    viewport: Viewport,
) {
    let center = viewport.world_to_screen(position);
    let body_color = if side == Side::One {
        Color::from_rgba(244, 244, 244, 255)
    } else {
        Color::from_rgba(20, 20, 24, 255)
    };
    let outline = if side == Side::One { BLACK } else { WHITE };
    let body_size = viewport.logical_size(PLAYER_BODY_SIZE);
    draw_rectangle(
        center.x - body_size * 0.5,
        center.y - body_size * 0.5,
        body_size,
        body_size,
        body_color,
    );
    draw_rectangle_lines(
        center.x - body_size * 0.5,
        center.y - body_size * 0.5,
        body_size,
        body_size,
        viewport.logical_size(2.0),
        outline,
    );
    if phase != PlayerPhase::Dead {
        let aim =
            viewport.world_to_screen(position.plus(WorldVec2::from_angle(aim_angle).scale(60.0)));
        draw_line(
            center.x,
            center.y,
            aim.x,
            aim.y,
            viewport.logical_size(if phase == PlayerPhase::LongbowCharge {
                3.0
            } else {
                1.0
            }),
            if phase == PlayerPhase::Damaged {
                YELLOW
            } else {
                Color::from_rgba(96, 208, 232, 220)
            },
        );
    }
}

/// Displays match state, controls, pause feedback, and the completed-round prompt.
fn draw_hud(snapshot: &FrameSnapshot, viewport: Viewport, hud: HudState<'_>) {
    let (player_one_label, player_two_label) = if hud.game_mode == GameMode::LocalTwoPlayer {
        ("PLAYER 1", "PLAYER 2")
    } else {
        ("YOU", "RIVAL")
    };
    let score = format!(
        "ROUND {}  |  {player_one_label} {} - {} {player_two_label}  |  FIRST TO {}",
        snapshot.score.player_one_wins + snapshot.score.player_two_wins + 1,
        snapshot.score.player_one_wins,
        snapshot.score.player_two_wins,
        snapshot.score.rounds_to_win
    );
    draw_text(
        &score,
        viewport.left + viewport.logical_size(24.0),
        viewport.top + viewport.logical_size(32.0),
        viewport.logical_size(20.0),
        WHITE,
    );
    let controls = match hud.game_mode {
        GameMode::Demo => "DEMO: AI vs AI  |  1-3 vs AI  4 local duel  M demo  O settings",
        GameMode::VersusAi => {
            "WASD move / mouse aim / Z or click: shortbow / X or right click: longbow  O settings"
        }
        GameMode::LocalTwoPlayer => "P1: WASD / mouse / Z-X     P2: IJKL / B-V  O settings",
    };
    draw_text(
        controls,
        viewport.left + viewport.logical_size(24.0),
        viewport.top + viewport.logical_size(58.0),
        viewport.logical_size(15.0),
        Color::from_rgba(224, 224, 224, 220),
    );
    draw_menu_button(
        viewport,
        WorldVec2::new(1180.0, 44.0),
        152.0,
        40.0,
        "SETTINGS",
    );
    draw_text(
        format!(
            "{}  |  {}  |  C arena  P pause  R reset",
            hud.mode_label,
            if hud.arena == ArenaKind::CentralCover {
                "CENTRAL COVER"
            } else {
                "OPEN"
            }
        ),
        viewport.left + viewport.logical_size(24.0),
        viewport.top + viewport.logical_size(80.0),
        viewport.logical_size(14.0),
        Color::from_rgba(232, 192, 96, 255),
    );
    if hud.paused {
        draw_rectangle(
            viewport.left,
            viewport.top,
            viewport.logical_size(ARENA_WIDTH),
            viewport.logical_size(ARENA_HEIGHT),
            Color::from_rgba(8, 12, 18, 155),
        );
        let label = "PAUSED - PRESS P TO RESUME";
        let font_size = viewport.logical_size(36.0);
        let measure = measure_text(label, None, font_size as u16, 1.0);
        draw_text(
            label,
            viewport.left + (viewport.logical_size(ARENA_WIDTH) - measure.width) * 0.5,
            viewport.top + viewport.logical_size(360.0),
            font_size,
            WHITE,
        );
    }
    if let Some(result) = snapshot.round_result {
        draw_rectangle(
            viewport.left,
            viewport.top,
            viewport.logical_size(ARENA_WIDTH),
            viewport.logical_size(ARENA_HEIGHT),
            Color::from_rgba(8, 12, 18, 155),
        );
        let winner = match (result.winner(), hud.game_mode == GameMode::LocalTwoPlayer) {
            (dual_protocol::WinnerSide::SideOne, true) => "PLAYER 1 WINS THE ROUND",
            (dual_protocol::WinnerSide::SideTwo, true) => "PLAYER 2 WINS THE ROUND",
            (dual_protocol::WinnerSide::SideOne, false) => "YOU WIN THE ROUND",
            (dual_protocol::WinnerSide::SideTwo, false) => "RIVAL WINS THE ROUND",
        };
        let completed = if result.match_complete() {
            "MATCH COMPLETE - PRESS R TO PLAY AGAIN"
        } else {
            "PRESS R TO START THE NEXT ROUND"
        };
        let winner_size = viewport.logical_size(42.0);
        let winner_measure = measure_text(winner, None, winner_size as u16, 1.0);
        draw_text(
            winner,
            viewport.left + (viewport.logical_size(ARENA_WIDTH) - winner_measure.width) * 0.5,
            viewport.top + viewport.logical_size(336.0),
            winner_size,
            WHITE,
        );
        let hint_size = viewport.logical_size(20.0);
        let hint_measure = measure_text(completed, None, hint_size as u16, 1.0);
        draw_text(
            completed,
            viewport.left + (viewport.logical_size(ARENA_WIDTH) - hint_measure.width) * 0.5,
            viewport.top + viewport.logical_size(384.0),
            hint_size,
            Color::from_rgba(232, 192, 96, 255),
        );
    }
    if hud.settings_open {
        draw_settings_overlay(viewport, hud.audio_volume_label);
    }
}

/// Shows volume controls above the frozen match without resetting the current round.
fn draw_settings_overlay(viewport: Viewport, audio_volume_label: &str) {
    draw_rectangle(
        viewport.left,
        viewport.top,
        viewport.logical_size(ARENA_WIDTH),
        viewport.logical_size(ARENA_HEIGHT),
        Color::from_rgba(8, 12, 18, 255),
    );
    let center_x = viewport.left + viewport.logical_size(ARENA_WIDTH) * 0.5;
    let title = "SETTINGS";
    let title_size = viewport.logical_size(38.0);
    let title_measure = measure_text(title, None, title_size as u16, 1.0);
    draw_text(
        title,
        center_x - title_measure.width * 0.5,
        viewport.top + viewport.logical_size(270.0),
        title_size,
        WHITE,
    );
    let volume = format!("VOLUME  {audio_volume_label}");
    let volume_size = viewport.logical_size(26.0);
    let volume_measure = measure_text(&volume, None, volume_size as u16, 1.0);
    draw_text(
        &volume,
        center_x - volume_measure.width * 0.5,
        viewport.top + viewport.logical_size(330.0),
        volume_size,
        Color::from_rgba(232, 192, 96, 255),
    );
    draw_menu_button(viewport, WorldVec2::new(500.0, 380.0), 116.0, 48.0, "-");
    draw_menu_button(viewport, WorldVec2::new(640.0, 380.0), 180.0, 48.0, "MUTE");
    draw_menu_button(viewport, WorldVec2::new(780.0, 380.0), 116.0, 48.0, "+");
    let controls = "Click - / + to change volume, M to mute";
    let controls_size = viewport.logical_size(18.0);
    let controls_measure = measure_text(controls, None, controls_size as u16, 1.0);
    draw_text(
        controls,
        center_x - controls_measure.width * 0.5,
        viewport.top + viewport.logical_size(444.0),
        controls_size,
        Color::from_rgba(224, 224, 224, 255),
    );
    draw_menu_button(viewport, WorldVec2::new(640.0, 500.0), 240.0, 48.0, "BACK");
}

/// Draws a scaled button that shares the hit-box coordinates used by the settings input path.
fn draw_menu_button(viewport: Viewport, center: WorldVec2, width: f32, height: f32, label: &str) {
    let top_left = viewport.world_to_screen(WorldVec2::new(
        center.x - width * 0.5,
        center.y - height * 0.5,
    ));
    draw_rectangle(
        top_left.x,
        top_left.y,
        viewport.logical_size(width),
        viewport.logical_size(height),
        Color::from_rgba(36, 44, 56, 240),
    );
    draw_rectangle_lines(
        top_left.x,
        top_left.y,
        viewport.logical_size(width),
        viewport.logical_size(height),
        viewport.logical_size(2.0),
        Color::from_rgba(96, 208, 232, 200),
    );
    let font_size = viewport.logical_size(18.0);
    let measurement = measure_text(label, None, font_size as u16, 1.0);
    let center_screen = viewport.world_to_screen(center);
    draw_text(
        label,
        center_screen.x - measurement.width * 0.5,
        center_screen.y + measurement.height * 0.35,
        font_size,
        WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_maps_full_hd_without_letterboxing() {
        let viewport = Viewport::for_size(1920.0, 1080.0);
        assert_eq!(viewport.scale, 1.5);
        assert_eq!(viewport.left, 0.0);
        assert_eq!(viewport.top, 0.0);
        assert_eq!(
            viewport.world_to_screen(WorldVec2::new(ARENA_WIDTH, ARENA_HEIGHT)),
            vec2(1920.0, 1080.0)
        );
    }

    #[test]
    fn viewport_letterboxes_and_round_trips_mouse_coordinates() {
        let viewport = Viewport::for_size(1000.0, 800.0);
        assert_eq!(viewport.left, 0.0);
        assert!(viewport.top > 0.0);
        let world_point = WorldVec2::new(314.0, 271.0);
        let screen_point = viewport.world_to_screen(world_point);
        let restored = viewport.screen_to_world(screen_point);
        assert!((restored.x - world_point.x).abs() < 0.001);
        assert!((restored.y - world_point.y).abs() < 0.001);
    }

    #[test]
    fn queued_weapon_presses_survive_until_one_fixed_tick() {
        let mut edges = PressEdges::default();
        edges.capture(true, true);
        assert!(edges.take_player_one_shortbow());
        assert!(!edges.take_player_one_shortbow());
        assert!(edges.take_player_two_shortbow());
        assert!(!edges.take_player_two_shortbow());
    }

    #[test]
    fn clearing_pressed_edges_drops_paused_or_reset_shots() {
        let mut edges = PressEdges::default();
        edges.capture(true, true);
        edges.clear();
        assert!(!edges.take_player_one_shortbow());
        assert!(!edges.take_player_two_shortbow());
    }

    #[test]
    fn queued_weapon_press_expires_after_a_stalled_fixed_step() {
        let mut edges = PressEdges::default();
        edges.capture(true, false);
        for _ in 0..MAX_PRESS_EDGE_RENDER_FRAMES {
            edges.capture(false, false);
        }
        assert!(!edges.take_player_one_shortbow());
    }

    #[test]
    fn window_focus_loss_is_reported_once_to_the_pause_loop() {
        let mut events = WindowFocusEvents::default();
        macroquad::miniquad::EventHandler::window_minimized_event(&mut events);
        assert!(events.take_lost_focus());
        assert!(!events.take_lost_focus());
    }

    #[test]
    fn mode_selection_assigns_input_ownership_to_the_expected_players() {
        let demo = create_ai_pair(GameMode::Demo, AiDifficulty::Standard, MATCH_SEED);
        assert_eq!(demo[0].as_ref().map(AiController::side), Some(Side::One));
        assert_eq!(demo[1].as_ref().map(AiController::side), Some(Side::Two));

        let versus_ai = create_ai_pair(GameMode::VersusAi, AiDifficulty::Basic, MATCH_SEED);
        assert!(versus_ai[0].is_none());
        assert_eq!(
            versus_ai[1].as_ref().map(AiController::side),
            Some(Side::Two)
        );

        let local = create_ai_pair(GameMode::LocalTwoPlayer, AiDifficulty::Advanced, MATCH_SEED);
        assert!(local.iter().all(Option::is_none));
    }

    #[test]
    fn pause_and_settings_overlays_freeze_the_simulation() {
        assert!(should_advance_simulation(false, false));
        assert!(!should_advance_simulation(true, false));
        assert!(!should_advance_simulation(false, true));
        assert!(!should_advance_simulation(true, true));
    }

    #[test]
    fn settings_buttons_use_centered_world_space_hit_boxes() {
        let minus = WorldVec2::new(500.0, 380.0);
        assert!(point_in_button(minus, minus, 116.0, 48.0));
        assert!(point_in_button(
            WorldVec2::new(558.0, 404.0),
            minus,
            116.0,
            48.0
        ));
        assert!(!point_in_button(
            WorldVec2::new(558.1, 404.0),
            minus,
            116.0,
            48.0
        ));
        assert!(!point_in_button(
            WorldVec2::new(640.0, 380.0),
            minus,
            116.0,
            48.0
        ));
    }
}
