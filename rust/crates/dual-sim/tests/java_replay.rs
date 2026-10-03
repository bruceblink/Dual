use dual_protocol::WinnerSide;
use dual_sim::{
    ArrowKind, PlayerInput, PlayerPhase, PlayerSnapshot, Side, SimEvent, Simulation,
    SimulationConfig, TacticalEventKind, Vec2,
};

const JAVA_MOVEMENT_REPLAY: &str = include_str!("../../../fixtures/replay/java-movement-180.csv");
const JAVA_LONGBOW_MATCH_REPLAY: &str =
    include_str!("../../../fixtures/replay/java-longbow-match.csv");
const JAVA_SHORTBOW_HIT_REPLAY: &str =
    include_str!("../../../fixtures/replay/java-shortbow-hit.csv");
const JAVA_SHORTBOW_INTERCEPTION_REPLAY: &str =
    include_str!("../../../fixtures/replay/java-shortbow-interception.csv");
const JAVA_COVER_IMPACT_REPLAY: &str =
    include_str!("../../../fixtures/replay/java-cover-impact.csv");
const JAVA_TACTICAL_COMBO_REPLAY: &str =
    include_str!("../../../fixtures/replay/java-tactical-combo.csv");
const JAVA_CHARGE_DISRUPT_REPLAY: &str =
    include_str!("../../../fixtures/replay/java-charge-disrupt.csv");
const JAVA_COVER_PLAYER_REPLAY: &str =
    include_str!("../../../fixtures/replay/java-cover-player.csv");
// Processing's angle lookup and Rust's float trig can differ by tiny subpixel amounts.
const MATCH_FLOAT_TOLERANCE: f32 = 0.001;

#[test]
fn fixed_movement_inputs_match_the_java_baseline_every_frame() {
    let mut simulation = Simulation::new(SimulationConfig::default(), 0);
    let mut compared_frames = 0;

    for line in JAVA_MOVEMENT_REPLAY.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields = line.split(',').collect::<Vec<_>>();
        assert_eq!(fields.len(), 27, "malformed Java replay row: {line}");

        let frame = fields[0]
            .parse::<u64>()
            .expect("Java replay frame must be an integer");
        assert_eq!(frame, simulation.frame() + 1, "replay frame ordering");

        let player_one_mask = parse_mask(fields[1]);
        let player_two_mask = parse_mask(fields[2]);
        simulation.step([input_for(player_one_mask), input_for(player_two_mask)]);

        let players = simulation.players();
        assert_player_matches(&fields, 3, players[0], frame, "one");
        assert_player_matches(&fields, 15, players[1], frame, "two");
        compared_frames += 1;
    }

    assert_eq!(compared_frames, 180, "expected the complete Java replay");
}

#[test]
fn fixed_longbow_inputs_match_three_java_rounds_and_the_complete_match() {
    let mut simulation = Simulation::new(SimulationConfig::default(), 0);
    let mut current_round = 1;
    let mut compared_frames = 0;
    let mut last_global_frame = None;

    for line in JAVA_LONGBOW_MATCH_REPLAY.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields = line.split(',').collect::<Vec<_>>();
        assert!(
            fields.len() >= 38,
            "malformed Java match replay row: {line}"
        );
        let global_frame = parse_integer(fields[0], "global frame");
        let round = parse_integer(fields[1], "round");
        let round_frame = parse_integer(fields[2], "round frame");

        if round != current_round {
            assert_eq!(round, current_round + 1, "round sequence");
            assert!(
                simulation.round_result().is_some(),
                "round {current_round} must finish before the next round starts"
            );
            simulation.reset_round();
            current_round = round;
        }
        assert_eq!(
            round_frame,
            simulation.frame() + 1,
            "round-local frame ordering"
        );
        if let Some(previous_frame) = last_global_frame {
            assert_eq!(global_frame, previous_frame + 1, "global frame ordering");
        } else {
            assert_eq!(
                global_frame, 0,
                "the replay must start at global frame zero"
            );
        }
        last_global_frame = Some(global_frame);

        let player_one_mask = parse_mask(fields[3]);
        let player_two_mask = parse_mask(fields[4]);
        simulation.step([input_for(player_one_mask), input_for(player_two_mask)]);
        let snapshot = simulation.snapshot();

        assert_trace_snapshot(&fields, &snapshot, global_frame, round);
        compared_frames += 1;
    }

    assert_eq!(compared_frames, 111, "expected all three scripted rounds");
    let final_result = simulation
        .round_result()
        .expect("the Java replay must finish the third round");
    assert_eq!(final_result.round_number(), 3);
    assert_eq!(final_result.winner(), WinnerSide::SideOne);
    assert!(final_result.match_complete());
}

#[test]
fn fixed_shortbow_inputs_match_java_hit_recovery_and_arrow_interception() {
    let close_players = SimulationConfig {
        spawn_positions: [Vec2::new(640.0, 520.0), Vec2::new(640.0, 480.0)],
        ..SimulationConfig::default()
    };
    assert_scenario_replay(JAVA_SHORTBOW_HIT_REPLAY, close_players, 120);

    let crossing_arrows = SimulationConfig {
        spawn_positions: [Vec2::new(200.0, 360.0), Vec2::new(300.0, 360.0)],
        ..SimulationConfig::default()
    };
    assert_scenario_replay(JAVA_SHORTBOW_INTERCEPTION_REPLAY, crossing_arrows, 7);

    let central_cover = SimulationConfig {
        arena: dual_sim::ArenaKind::CentralCover,
        spawn_positions: [Vec2::new(400.0, 360.0), Vec2::new(880.0, 360.0)],
        ..SimulationConfig::default()
    };
    assert_scenario_replay(JAVA_COVER_IMPACT_REPLAY, central_cover, 12);
    let central_cover_player = SimulationConfig {
        arena: dual_sim::ArenaKind::CentralCover,
        spawn_positions: [Vec2::new(450.0, 360.0), Vec2::new(880.0, 360.0)],
        ..SimulationConfig::default()
    };
    assert_scenario_replay(JAVA_COVER_PLAYER_REPLAY, central_cover_player, 60);
    assert_scenario_replay(JAVA_TACTICAL_COMBO_REPLAY, close_players, 36);
    assert_scenario_replay(JAVA_CHARGE_DISRUPT_REPLAY, close_players, 12);
}

fn assert_scenario_replay(replay: &str, config: SimulationConfig, expected_frames: u64) {
    let mut simulation = Simulation::new(config, 0);
    let mut compared_frames = 0;

    for line in replay.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split(',').collect::<Vec<_>>();
        assert!(
            fields.len() >= 38,
            "malformed Java scenario replay row: {line}"
        );
        let global_frame = parse_integer(fields[0], "global frame");
        let round = parse_integer(fields[1], "round");
        let round_frame = parse_integer(fields[2], "round frame");
        assert_eq!(round, 1, "shortbow scenario round");
        assert_eq!(
            round_frame,
            simulation.frame() + 1,
            "global frame {global_frame}, round-local frame"
        );

        simulation.step([
            input_for(parse_mask(fields[3])),
            input_for(parse_mask(fields[4])),
        ]);
        let snapshot = simulation.snapshot();
        assert_trace_snapshot(&fields, &snapshot, global_frame, 1);
        compared_frames += 1;
    }

    assert_eq!(compared_frames, expected_frames);
}

fn assert_trace_snapshot(
    fields: &[&str],
    snapshot: &dual_sim::FrameSnapshot,
    global_frame: u64,
    round: u64,
) {
    assert_eq!(
        parse_integer(fields[0], "global frame"),
        global_frame,
        "global frame index"
    );
    assert_eq!(
        parse_integer(fields[1], "round"),
        round,
        "global frame {global_frame}, round"
    );
    assert_eq!(
        parse_integer(fields[2], "round frame"),
        snapshot.frame,
        "global frame {global_frame}, round-local frame"
    );
    assert_player_matches_with_tolerance(fields, 5, snapshot.players[0], global_frame, "one");
    assert_player_matches_with_tolerance(fields, 18, snapshot.players[1], global_frame, "two");

    assert_eq!(
        parse_integer(fields[31], "player one score"),
        u64::from(snapshot.score.player_one_wins),
        "global frame {global_frame}, player one score"
    );
    assert_eq!(
        parse_integer(fields[32], "player two score"),
        u64::from(snapshot.score.player_two_wins),
        "global frame {global_frame}, player two score"
    );

    let (result_round, result_winner, match_complete) = snapshot
        .round_result
        .map(|result| {
            (
                u64::from(result.round_number()),
                match result.winner() {
                    WinnerSide::SideOne => 1,
                    WinnerSide::SideTwo => 2,
                },
                u64::from(result.match_complete()),
            )
        })
        .unwrap_or((0, 0, 0));
    assert_eq!(
        parse_integer(fields[33], "result round"),
        result_round,
        "global frame {global_frame}, result round"
    );
    assert_eq!(
        parse_integer(fields[34], "result winner"),
        result_winner,
        "global frame {global_frame}, result winner"
    );
    assert_eq!(
        parse_integer(fields[35], "match completion flag"),
        match_complete,
        "global frame {global_frame}, match completion"
    );

    assert_eq!(
        parse_integer(fields[36], "event flags"),
        event_flags(&snapshot.events),
        "global frame {global_frame}, gameplay events"
    );
    assert_arrows_match(fields, &snapshot.arrows, global_frame);
}

fn parse_mask(value: &str) -> u8 {
    value
        .parse()
        .expect("Java replay input mask must be an integer")
}

fn parse_integer(value: &str, field: &str) -> u64 {
    value
        .parse()
        .unwrap_or_else(|_| panic!("Java replay {field} must be an integer"))
}

fn input_for(mask: u8) -> PlayerInput {
    PlayerInput {
        up: mask & 0x01 != 0,
        down: mask & 0x02 != 0,
        left: mask & 0x04 != 0,
        right: mask & 0x08 != 0,
        shortbow: mask & 0x10 != 0,
        longbow: mask & 0x20 != 0,
        ..PlayerInput::empty()
    }
}

fn assert_player_matches_with_tolerance(
    fields: &[&str],
    start: usize,
    player: PlayerSnapshot,
    frame: u64,
    side: &str,
) {
    let float_values = [
        ("x", player.position.x),
        ("y", player.position.y),
        ("vx", player.velocity.x),
        ("vy", player.velocity.y),
        ("aim", player.aim_angle),
    ];
    for (offset, (name, actual)) in float_values.into_iter().enumerate() {
        assert_float_matches(fields[start + offset], actual, frame, side, name);
    }

    let integer_values = [
        ("phase", phase_code(player.phase)),
        ("ammo", u64::from(player.shortbow_ammo)),
        ("ammo_recovery", u64::from(player.shortbow_recovery_frames)),
        ("cooldown", u64::from(player.shortbow_cooldown_frames)),
        ("charge", u64::from(player.longbow_charge_frames)),
        ("recovery", u64::from(player.longbow_recovery_frames)),
        ("damage", u64::from(player.damage_remaining_frames)),
        ("pressure", u64::from(player.pressure_count)),
    ];
    for (offset, (name, actual)) in integer_values.into_iter().enumerate() {
        assert_eq!(
            parse_integer(fields[start + 5 + offset], name),
            actual,
            "global frame {frame}, player {side}, field {name}"
        );
    }
}

fn assert_float_matches(expected_bits: &str, actual: f32, frame: u64, owner: &str, field: &str) {
    let expected = u32::from_str_radix(expected_bits, 16)
        .expect("Java replay float must be an eight-digit hexadecimal bit pattern");
    let expected = f32::from_bits(expected);
    assert!(
        (actual - expected).abs() <= MATCH_FLOAT_TOLERANCE,
        "global frame {frame}, {owner} {field}: Java={expected:?}, Rust={actual:?}, tolerance={MATCH_FLOAT_TOLERANCE}"
    );
}

fn event_flags(events: &[SimEvent]) -> u64 {
    events.iter().fold(0, |flags, event| {
        flags
            | match event {
                SimEvent::LongbowChargeReady {
                    attacker: Side::One,
                } => 1,
                SimEvent::LongbowChargeReady {
                    attacker: Side::Two,
                } => 1 << 1,
                SimEvent::LongbowFired {
                    attacker: Side::One,
                } => 1 << 2,
                SimEvent::LongbowFired {
                    attacker: Side::Two,
                } => 1 << 3,
                SimEvent::LongbowHit {
                    attacker: Side::One,
                    ..
                } => 1 << 4,
                SimEvent::LongbowHit {
                    attacker: Side::Two,
                    ..
                } => 1 << 5,
                SimEvent::ShortbowFired {
                    attacker: Side::One,
                } => 1 << 6,
                SimEvent::ShortbowFired {
                    attacker: Side::Two,
                } => 1 << 7,
                SimEvent::ShortbowHit {
                    attacker: Side::One,
                    ..
                } => 1 << 8,
                SimEvent::ShortbowHit {
                    attacker: Side::Two,
                    ..
                } => 1 << 9,
                SimEvent::Tactical(event) => match (event.kind, event.attacker) {
                    (TacticalEventKind::Pressure, Some(Side::One)) => 1 << 12,
                    (TacticalEventKind::Pressure, Some(Side::Two)) => 1 << 13,
                    (TacticalEventKind::Opening, Some(Side::One)) => 1 << 14,
                    (TacticalEventKind::Opening, Some(Side::Two)) => 1 << 15,
                    (TacticalEventKind::Disrupt, Some(Side::One)) => 1 << 16,
                    (TacticalEventKind::Disrupt, Some(Side::Two)) => 1 << 17,
                    (TacticalEventKind::Finish, Some(Side::One)) => 1 << 18,
                    (TacticalEventKind::Finish, Some(Side::Two)) => 1 << 19,
                    (TacticalEventKind::Intercept, None) => 1 << 20,
                    _ => 0,
                },
                SimEvent::ArrowIntercepted { .. } => 1 << 10,
                SimEvent::CoverImpact { .. } => 1 << 11,
            }
    })
}

fn assert_arrows_match(fields: &[&str], arrows: &[dual_sim::ArrowSnapshot], frame: u64) {
    let arrow_count = parse_integer(fields[37], "arrow count") as usize;
    assert_eq!(
        arrows.len(),
        arrow_count,
        "global frame {frame}, live arrow count"
    );
    assert_eq!(
        fields.len(),
        38 + arrow_count * 7,
        "global frame {frame}, replay arrow columns"
    );

    for (index, arrow) in arrows.iter().enumerate() {
        let start = 38 + index * 7;
        let owner = match arrow.owner {
            Side::One => 1,
            Side::Two => 2,
        };
        let kind = match arrow.kind {
            ArrowKind::Shortbow => 0,
            ArrowKind::LongbowShaft => 1,
            ArrowKind::LongbowHead => 2,
        };
        assert_eq!(
            parse_integer(fields[start], "arrow owner"),
            owner,
            "global frame {frame}, arrow {index}, owner"
        );
        assert_eq!(
            parse_integer(fields[start + 1], "arrow kind"),
            kind,
            "global frame {frame}, arrow {index}, kind"
        );
        let float_values = [
            ("x", arrow.position.x),
            ("y", arrow.position.y),
            ("vx", arrow.velocity.x),
            ("vy", arrow.velocity.y),
            ("rotation", arrow.rotation_angle),
        ];
        for (offset, (name, actual)) in float_values.into_iter().enumerate() {
            assert_float_matches(
                fields[start + 2 + offset],
                actual,
                frame,
                &format!("arrow {index}"),
                name,
            );
        }
    }
}

fn assert_player_matches(
    fields: &[&str],
    start: usize,
    player: PlayerSnapshot,
    frame: u64,
    side: &str,
) {
    let float_values = [
        ("x", player.position.x),
        ("y", player.position.y),
        ("vx", player.velocity.x),
        ("vy", player.velocity.y),
        ("aim", player.aim_angle),
    ];

    for (offset, (name, actual)) in float_values.into_iter().enumerate() {
        let expected = u32::from_str_radix(fields[start + offset], 16)
            .expect("Java replay float must be an eight-digit hexadecimal bit pattern");
        assert_eq!(
            actual.to_bits(),
            expected,
            "frame {frame}, player {side}, field {name}"
        );
    }

    let integer_values = [
        ("phase", phase_code(player.phase)),
        ("ammo", u64::from(player.shortbow_ammo)),
        ("cooldown", u64::from(player.shortbow_cooldown_frames)),
        ("charge", u64::from(player.longbow_charge_frames)),
        ("recovery", u64::from(player.longbow_recovery_frames)),
        ("damage", u64::from(player.damage_remaining_frames)),
        ("pressure", u64::from(player.pressure_count)),
    ];

    for (offset, (name, actual)) in integer_values.into_iter().enumerate() {
        let expected = fields[start + 5 + offset]
            .parse::<u64>()
            .expect("Java replay state field must be an integer");
        assert_eq!(
            actual, expected,
            "frame {frame}, player {side}, field {name}"
        );
    }
}

fn phase_code(phase: PlayerPhase) -> u64 {
    match phase {
        PlayerPhase::Move => 0,
        PlayerPhase::ShortbowAction => 1,
        PlayerPhase::LongbowCharge => 2,
        PlayerPhase::Damaged => 3,
        PlayerPhase::Dead => 4,
    }
}
