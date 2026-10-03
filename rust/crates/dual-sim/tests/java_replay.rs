use dual_sim::{PlayerInput, PlayerPhase, PlayerSnapshot, Simulation, SimulationConfig};

const JAVA_MOVEMENT_REPLAY: &str = include_str!("../../../fixtures/replay/java-movement-180.csv");

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

fn parse_mask(value: &str) -> u8 {
    value
        .parse()
        .expect("Java replay input mask must be an integer")
}

fn input_for(mask: u8) -> PlayerInput {
    PlayerInput {
        up: mask & 0x01 != 0,
        down: mask & 0x02 != 0,
        left: mask & 0x04 != 0,
        right: mask & 0x08 != 0,
        ..PlayerInput::empty()
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
