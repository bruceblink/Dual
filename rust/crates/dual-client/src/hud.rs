use dual_sim::{
    FPS, FrameSnapshot, LONGBOW_CHARGE_FRAMES, PlayerCombatStats, PlayerPhase, PlayerSnapshot,
    SHORTBOW_AMMO_RECOVERY_FRAMES, SHORTBOW_MAX_AMMO, Side, SimEvent, TacticalEvent,
    TacticalEventKind,
};
use macroquad::prelude::*;

use crate::{GameMode, Viewport};

const TACTICAL_FEEDBACK_SECONDS: f32 = 0.75;
const STATUS_PANEL_COLOR: Color = Color::new(0.04, 0.05, 0.07, 0.72);

/// Retains the latest tactical event for a short, presentation-only HUD notice.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct TacticalFeedback {
    event: Option<TacticalEvent>,
    elapsed_seconds: f32,
}

impl TacticalFeedback {
    /// Stores only newly observed events so fixed-step catch-up cannot restart their fade.
    pub(super) fn record(&mut self, events: &[SimEvent]) {
        let latest = events.iter().rev().find_map(|event| match event {
            SimEvent::Tactical(event) => Some(*event),
            _ => None,
        });
        if let Some(event) = latest
            && self.event != Some(event)
        {
            self.event = Some(event);
            self.elapsed_seconds = 0.0;
        }
    }

    /// Advances the notice on render time and freezes it under pause or settings overlays.
    pub(super) fn advance(&mut self, frame_seconds: f32, frozen: bool) {
        if !frozen && self.event.is_some() {
            self.elapsed_seconds += frame_seconds.clamp(0.0, 0.25);
        }
    }

    /// Clears transient match feedback at a match, mode, or round boundary.
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    /// Keeps a confirmed tactical finish on the result page after its normal fade interval.
    pub(super) fn display(self, round_complete: bool) -> Option<TacticalFeedbackDisplay> {
        let event = self.event?;
        if round_complete && event.kind == TacticalEventKind::Finish {
            return Some(TacticalFeedbackDisplay { event, alpha: 1.0 });
        }
        let alpha = (1.0 - self.elapsed_seconds / TACTICAL_FEEDBACK_SECONDS).clamp(0.0, 1.0);
        (alpha > 0.0).then_some(TacticalFeedbackDisplay { event, alpha })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TacticalFeedbackDisplay {
    event: TacticalEvent,
    alpha: f32,
}

/// Draws weapon and pressure status from the immutable snapshot for the active players.
pub(super) fn draw_combat_status(snapshot: &FrameSnapshot, viewport: Viewport, mode: GameMode) {
    draw_player_status(
        snapshot.players[Side::One.index()],
        player_label(Side::One, mode),
        viewport,
        82.0,
        652.0,
        568.0,
        600.0,
    );
    if matches!(mode, GameMode::Demo | GameMode::LocalTwoPlayer) {
        draw_player_status(
            snapshot.players[Side::Two.index()],
            player_label(Side::Two, mode),
            viewport,
            1_198.0,
            148.0,
            190.0,
            232.0,
        );
    }
}

/// Draws the neutral action summary that accompanies every completed round.
pub(super) fn draw_round_report(snapshot: &FrameSnapshot, viewport: Viewport, mode: GameMode) {
    let report = snapshot.round_stats;
    draw_report_column(
        viewport,
        360.0,
        player_label(Side::One, mode),
        report.players[Side::One.index()],
    );
    draw_report_column(
        viewport,
        920.0,
        player_label(Side::Two, mode),
        report.players[Side::Two.index()],
    );

    let summary = format!(
        "ARROWS: INTERCEPT {}  |  ACTIVE {:.1}s",
        report.interception_count,
        report.active_frames as f32 / FPS as f32
    );
    draw_centered_text(
        &summary,
        viewport
            .world_to_screen(dual_sim::Vec2::new(640.0, 610.0))
            .x,
        viewport
            .world_to_screen(dual_sim::Vec2::new(640.0, 610.0))
            .y,
        viewport.logical_size(15.0),
        Color::from_rgba(224, 224, 224, 190),
    );
}

/// Draws one player's factual weapon and counterplay counts on the result page.
fn draw_report_column(viewport: Viewport, center_x: f32, label: &str, stats: PlayerCombatStats) {
    let lines = [
        label.to_owned(),
        format!("SHORT  {} / {}", stats.shortbow_hits, stats.shortbow_shots),
        format!("LONG   {} / {}", stats.longbow_hits, stats.longbow_shots),
        format!("CHARGE BREAKS  {}", stats.charge_breaks),
    ];
    for (index, line) in lines.iter().enumerate() {
        let y = 500.0 + index as f32 * 28.0;
        let color = if index == 0 {
            Color::from_rgba(255, 255, 255, 224)
        } else {
            Color::from_rgba(224, 224, 224, 192)
        };
        draw_centered_text(
            line,
            viewport.world_to_screen(dual_sim::Vec2::new(center_x, y)).x,
            viewport.world_to_screen(dual_sim::Vec2::new(center_x, y)).y,
            viewport.logical_size(if index == 0 { 18.0 } else { 15.0 }),
            color,
        );
    }
}

/// Maps simulation sides to concise labels that match each local play mode.
fn player_label(side: Side, mode: GameMode) -> &'static str {
    match (mode, side) {
        (GameMode::Demo, Side::One) => "P1",
        (GameMode::Demo, Side::Two) => "P2",
        (GameMode::LocalTwoPlayer, Side::One) => "P1",
        (GameMode::LocalTwoPlayer, Side::Two) => "P2",
        (GameMode::VersusAi, Side::One) => "YOU",
        (GameMode::VersusAi, Side::Two) => "RIVAL",
    }
}

/// Draws ammo, recovery, active pressure, and longbow charge without mutating rules state.
fn draw_player_status(
    player: PlayerSnapshot,
    label: &str,
    viewport: Viewport,
    center_x: f32,
    ammo_y: f32,
    pressure_y: f32,
    charge_y: f32,
) {
    let text_color = Color::from_rgba(240, 240, 240, 255);
    let center = viewport.world_to_screen(dual_sim::Vec2::new(center_x, ammo_y - 8.0));
    draw_status_panel(viewport, center_x, ammo_y - 8.0, 154.0, 52.0);
    draw_centered_text(
        &format!(
            "{label} SHORTBOW  {} / {SHORTBOW_MAX_AMMO}",
            player.shortbow_ammo
        ),
        center.x,
        viewport
            .world_to_screen(dual_sim::Vec2::new(center_x, ammo_y - 19.0))
            .y,
        viewport.logical_size(13.0),
        text_color,
    );

    let dot_gap = viewport.logical_size(14.0);
    let dot_y = viewport
        .world_to_screen(dual_sim::Vec2::new(center_x, ammo_y))
        .y;
    let dot_radius = viewport.logical_size(4.0);
    for index in 0..SHORTBOW_MAX_AMMO {
        let offset = (f32::from(index) - f32::from(SHORTBOW_MAX_AMMO - 1) * 0.5) * dot_gap;
        let color = if index < player.shortbow_ammo {
            text_color
        } else {
            Color::from_rgba(64, 68, 76, 255)
        };
        draw_circle(center.x + offset, dot_y, dot_radius, color);
    }

    if player.shortbow_ammo < SHORTBOW_MAX_AMMO {
        let progress =
            f32::from(player.shortbow_recovery_frames) / f32::from(SHORTBOW_AMMO_RECOVERY_FRAMES);
        draw_progress_bar(
            viewport,
            center_x,
            ammo_y + 14.0,
            72.0,
            4.0,
            progress,
            text_color,
        );
    }

    if player.phase == PlayerPhase::Damaged && player.pressure_count > 0 {
        draw_status_panel(viewport, center_x, pressure_y, 154.0, 28.0);
        draw_centered_text(
            &format!("{label}: UNDER PRESSURE  {} / 2", player.pressure_count),
            viewport
                .world_to_screen(dual_sim::Vec2::new(center_x, pressure_y))
                .x,
            viewport
                .world_to_screen(dual_sim::Vec2::new(center_x, pressure_y + 5.0))
                .y,
            viewport.logical_size(12.0),
            Color::from_rgba(232, 192, 96, 255),
        );
    }

    if player.phase == PlayerPhase::LongbowCharge {
        let progress = f32::from(player.longbow_charge_frames) / f32::from(LONGBOW_CHARGE_FRAMES);
        let ready = player.longbow_charge_frames >= LONGBOW_CHARGE_FRAMES;
        let charge_color = if ready {
            Color::from_rgba(232, 96, 88, 255)
        } else {
            Color::from_rgba(224, 232, 240, 255)
        };
        let label = if ready {
            format!("{label} LONGBOW READY")
        } else {
            format!(
                "{label} LONGBOW {:02}%",
                (progress.clamp(0.0, 1.0) * 100.0).round() as u8
            )
        };
        draw_status_panel(viewport, center_x, charge_y, 164.0, 32.0);
        draw_centered_text(
            &label,
            viewport
                .world_to_screen(dual_sim::Vec2::new(center_x, charge_y - 7.0))
                .x,
            viewport
                .world_to_screen(dual_sim::Vec2::new(center_x, charge_y - 7.0))
                .y,
            viewport.logical_size(12.0),
            charge_color,
        );
        draw_progress_bar(
            viewport,
            center_x,
            charge_y + 8.0,
            112.0,
            6.0,
            progress,
            charge_color,
        );
    }
}

/// Draws a faint backdrop so small status labels stay readable over the arena grid.
fn draw_status_panel(viewport: Viewport, center_x: f32, center_y: f32, width: f32, height: f32) {
    let top_left = viewport.world_to_screen(dual_sim::Vec2::new(
        center_x - width * 0.5,
        center_y - height * 0.5,
    ));
    draw_rectangle(
        top_left.x,
        top_left.y,
        viewport.logical_size(width),
        viewport.logical_size(height),
        STATUS_PANEL_COLOR,
    );
}

/// Draws one clamped horizontal progress bar in arena coordinates.
fn draw_progress_bar(
    viewport: Viewport,
    center_x: f32,
    center_y: f32,
    width: f32,
    height: f32,
    progress: f32,
    color: Color,
) {
    let left = center_x - width * 0.5;
    let top_left = viewport.world_to_screen(dual_sim::Vec2::new(left, center_y - height * 0.5));
    let scaled_width = viewport.logical_size(width);
    let scaled_height = viewport.logical_size(height);
    draw_rectangle(
        top_left.x,
        top_left.y,
        scaled_width,
        scaled_height,
        Color::from_rgba(24, 28, 36, 255),
    );
    draw_rectangle(
        top_left.x,
        top_left.y,
        scaled_width * progress.clamp(0.0, 1.0),
        scaled_height,
        color,
    );
}

/// Measures and centers one status label at its window-space baseline.
fn draw_centered_text(text: &str, center_x: f32, baseline_y: f32, font_size: f32, color: Color) {
    let measure = measure_text(text, None, font_size as u16, 1.0);
    draw_text(
        text,
        center_x - measure.width * 0.5,
        baseline_y,
        font_size,
        color,
    );
}

/// Renders concise tactical facts during combat and preserves FINISH on a result page.
pub(super) fn draw_tactical_feedback(
    feedback: TacticalFeedbackDisplay,
    viewport: Viewport,
    round_complete: bool,
) {
    let event = feedback.event;
    let color = match event.kind {
        TacticalEventKind::Pressure | TacticalEventKind::Disrupt => {
            Color::from_rgba(232, 192, 96, 255)
        }
        TacticalEventKind::Opening => Color::from_rgba(64, 176, 128, 255),
        TacticalEventKind::Finish => Color::from_rgba(192, 64, 64, 255),
        TacticalEventKind::Intercept => Color::from_rgba(96, 208, 232, 255),
    };
    let label = tactical_feedback_label(event, round_complete);
    let center_x = viewport.world_to_screen(dual_sim::Vec2::new(640.0, 0.0)).x;
    let center_y = if round_complete { 430.0 } else { 112.0 };
    let font_size = viewport.logical_size(22.0);
    draw_centered_text(
        label,
        center_x,
        viewport
            .world_to_screen(dual_sim::Vec2::new(640.0, center_y))
            .y,
        font_size,
        Color::new(color.r, color.g, color.b, color.a * feedback.alpha),
    );
}

fn tactical_feedback_label(event: TacticalEvent, round_complete: bool) -> &'static str {
    if event.kind == TacticalEventKind::Intercept {
        return "ARROWS: INTERCEPT";
    }
    match (event.attacker, event.kind, round_complete) {
        (Some(Side::One), TacticalEventKind::Pressure, _) => "YOU: PRESSURE",
        (Some(Side::Two), TacticalEventKind::Pressure, _) => "RIVAL: PRESSURE",
        (Some(Side::One), TacticalEventKind::Opening, _) => "YOU: OPENING",
        (Some(Side::Two), TacticalEventKind::Opening, _) => "RIVAL: OPENING",
        (Some(Side::One), TacticalEventKind::Disrupt, _) => "YOU: CHARGE BREAK",
        (Some(Side::Two), TacticalEventKind::Disrupt, _) => "RIVAL: CHARGE BREAK",
        (Some(Side::One), TacticalEventKind::Finish, _) => "YOU: FINISH",
        (Some(Side::Two), TacticalEventKind::Finish, _) => "RIVAL: FINISH",
        _ => "TACTICAL EVENT",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_tactical_event_replaces_the_fading_notice_only_once() {
        let pressure = TacticalEvent::for_side(Side::One, TacticalEventKind::Pressure, 12);
        let opening = TacticalEvent::for_side(Side::One, TacticalEventKind::Opening, 18);
        let mut feedback = TacticalFeedback::default();
        feedback.record(&[SimEvent::Tactical(pressure)]);
        feedback.advance(0.25, false);

        feedback.record(&[SimEvent::Tactical(pressure)]);
        assert_eq!(feedback.elapsed_seconds, 0.25);
        feedback.record(&[SimEvent::Tactical(opening)]);
        assert_eq!(feedback.elapsed_seconds, 0.0);
        assert_eq!(
            feedback.display(false).map(|display| display.event),
            Some(opening)
        );
    }

    #[test]
    fn tactical_notice_freezes_and_fades_after_three_quarters_of_a_second() {
        let event = TacticalEvent::for_side(Side::Two, TacticalEventKind::Disrupt, 3);
        let mut feedback = TacticalFeedback::default();
        feedback.record(&[SimEvent::Tactical(event)]);
        feedback.advance(0.25, true);
        assert_eq!(feedback.elapsed_seconds, 0.0);
        feedback.advance(0.25, false);
        let alpha = feedback
            .display(false)
            .expect("active tactical notice")
            .alpha;
        assert!((alpha - 2.0 / 3.0).abs() < 0.000_001);
        feedback.advance(0.25, false);
        feedback.advance(0.25, false);
        assert_eq!(feedback.display(false), None);
    }

    #[test]
    fn tactical_finish_is_preserved_on_a_result_page_after_its_fade() {
        let event = TacticalEvent::for_side(Side::One, TacticalEventKind::Finish, 44);
        let mut feedback = TacticalFeedback::default();
        feedback.record(&[SimEvent::Tactical(event)]);
        feedback.advance(0.25, false);
        feedback.advance(0.25, false);
        feedback.advance(0.25, false);

        assert_eq!(feedback.display(false), None);
        assert_eq!(
            feedback.display(true).map(|display| display.event),
            Some(event)
        );
    }

    #[test]
    fn tactical_labels_identify_side_and_special_intercepts() {
        assert_eq!(
            tactical_feedback_label(
                TacticalEvent::for_side(Side::One, TacticalEventKind::Finish, 1),
                true
            ),
            "YOU: FINISH"
        );
        assert_eq!(
            tactical_feedback_label(TacticalEvent::intercept(1), false),
            "ARROWS: INTERCEPT"
        );
    }
}
