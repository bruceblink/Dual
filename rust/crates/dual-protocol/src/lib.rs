//! Wire-compatible protocol primitives shared by the Rust client and Relay.
//!
//! The Java client and Relay currently use fixed-size big-endian frames. This
//! crate keeps those byte-level rules in one place so the Rust implementation
//! can interoperate with both Java components without changing the protocol.

use core::fmt;

/// Message type byte values used by the Dual TCP protocol.
pub mod message_type {
    /// One input snapshot, followed by flags and a quantized aim angle.
    pub const INPUT: u8 = 0x00;
    /// Shared random seed sent by the Relay during the handshake.
    pub const START: u8 = 0x01;
    /// Handshake acknowledgement sent by a client.
    pub const START_ACK: u8 = 0x02;
    /// Graceful disconnect notification.
    pub const DISCONNECT: u8 = 0x03;
    /// Completed round and score snapshot.
    pub const ROUND_RESULT: u8 = 0x04;
    /// Request to start the next round or reset the match.
    pub const REMATCH_REQUEST: u8 = 0x05;
}

/// Fixed frame lengths, including the leading message type byte.
pub mod frame_length {
    /// `[type][flags][uint16 aim_angle]`.
    pub const INPUT: usize = 4;
    /// `[type][int32 seed]`.
    pub const START: usize = 5;
    /// `[type][round][winner][score_one][score_two][complete]`.
    pub const ROUND_RESULT: usize = 6;
    /// `[type][round][match_reset]`.
    pub const REMATCH_REQUEST: usize = 3;
}

const UP_MASK: u8 = 0x01;
const DOWN_MASK: u8 = 0x02;
const LEFT_MASK: u8 = 0x04;
const RIGHT_MASK: u8 = 0x08;
const SHORTBOW_MASK: u8 = 0x10;
const LONGBOW_MASK: u8 = 0x20;
const HAS_AIM_MASK: u8 = 0x40;
const KNOWN_INPUT_FLAGS_MASK: u8 = 0x7f;
const ANGLE_STEPS: f32 = 65_536.0;
const TAU: f32 = core::f32::consts::TAU;

/// Errors raised while validating fixed-size protocol frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// The frame did not have the required byte length.
    InvalidLength { expected: usize, actual: usize },
    /// The leading type byte did not match the decoder being used.
    UnexpectedType { expected: u8, actual: u8 },
    /// An input frame used a reserved flag bit.
    UnknownInputFlags { flags: u8 },
    /// An input frame supplied an angle without setting `HAS_AIM`.
    AimAngleWithoutFlag { quantized_angle: u16 },
    /// An angle supplied by an application was not finite.
    NonFiniteAimAngle,
    /// A round number must fit in a positive byte.
    InvalidRoundNumber { round: u8 },
    /// The winner byte must identify one of the two player sides.
    InvalidWinnerSide { side: u8 },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength { expected, actual } => {
                write!(
                    formatter,
                    "invalid frame length: expected {expected}, got {actual}"
                )
            }
            Self::UnexpectedType { expected, actual } => {
                write!(
                    formatter,
                    "unexpected message type: expected {expected:#04x}, got {actual:#04x}"
                )
            }
            Self::UnknownInputFlags { flags } => {
                write!(
                    formatter,
                    "input frame contains unknown flags: {flags:#04x}"
                )
            }
            Self::AimAngleWithoutFlag { quantized_angle } => write!(
                formatter,
                "input frame has an angle without HAS_AIM: {quantized_angle}"
            ),
            Self::NonFiniteAimAngle => formatter.write_str("aim angle must be finite"),
            Self::InvalidRoundNumber { round } => {
                write!(formatter, "round number must be positive: {round}")
            }
            Self::InvalidWinnerSide { side } => {
                write!(formatter, "winner side must be 0 or 1: {side}")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

/// The six button states and optional absolute aim angle sent for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputFrame {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    shortbow: bool,
    longbow: bool,
    aim_angle: Option<f32>,
}

impl InputFrame {
    /// Builds an input snapshot. `aim_angle` is sent only when it is `Some`.
    pub const fn new(
        up: bool,
        down: bool,
        left: bool,
        right: bool,
        shortbow: bool,
        longbow: bool,
        aim_angle: Option<f32>,
    ) -> Self {
        Self {
            up,
            down,
            left,
            right,
            shortbow,
            longbow,
            aim_angle,
        }
    }

    /// Returns an all-released input snapshot with no aim angle.
    pub const fn empty() -> Self {
        Self::new(false, false, false, false, false, false, None)
    }

    /// Returns the six button flags in the wire representation.
    pub const fn flags(self) -> u8 {
        (if self.up { UP_MASK } else { 0 })
            | (if self.down { DOWN_MASK } else { 0 })
            | (if self.left { LEFT_MASK } else { 0 })
            | (if self.right { RIGHT_MASK } else { 0 })
            | (if self.shortbow { SHORTBOW_MASK } else { 0 })
            | (if self.longbow { LONGBOW_MASK } else { 0 })
            | (if self.aim_angle.is_some() {
                HAS_AIM_MASK
            } else {
                0
            })
    }

    pub const fn up(self) -> bool {
        self.up
    }

    pub const fn down(self) -> bool {
        self.down
    }

    pub const fn left(self) -> bool {
        self.left
    }

    pub const fn right(self) -> bool {
        self.right
    }

    pub const fn shortbow(self) -> bool {
        self.shortbow
    }

    pub const fn longbow(self) -> bool {
        self.longbow
    }

    pub const fn aim_angle(self) -> Option<f32> {
        self.aim_angle
    }
}

/// Encodes one input snapshot into the Java-compatible four-byte frame.
pub fn encode_input_frame(input: InputFrame) -> Result<[u8; frame_length::INPUT], ProtocolError> {
    let quantized_angle = match input.aim_angle {
        Some(angle) => quantize_aim_angle(angle)?,
        None => 0,
    };
    Ok([
        message_type::INPUT,
        input.flags(),
        (quantized_angle >> 8) as u8,
        quantized_angle as u8,
    ])
}

/// Decodes and validates one Java-compatible input frame.
pub fn decode_input_frame(frame: &[u8]) -> Result<InputFrame, ProtocolError> {
    ensure_length(frame, frame_length::INPUT)?;
    ensure_type(frame[0], message_type::INPUT)?;

    let flags = frame[1];
    if flags & !KNOWN_INPUT_FLAGS_MASK != 0 {
        return Err(ProtocolError::UnknownInputFlags { flags });
    }

    let quantized_angle = u16::from_be_bytes([frame[2], frame[3]]);
    if flags & HAS_AIM_MASK == 0 && quantized_angle != 0 {
        return Err(ProtocolError::AimAngleWithoutFlag { quantized_angle });
    }

    Ok(InputFrame::new(
        flags & UP_MASK != 0,
        flags & DOWN_MASK != 0,
        flags & LEFT_MASK != 0,
        flags & RIGHT_MASK != 0,
        flags & SHORTBOW_MASK != 0,
        flags & LONGBOW_MASK != 0,
        (flags & HAS_AIM_MASK != 0).then(|| dequantize_aim_angle(quantized_angle)),
    ))
}

/// Converts a finite angle in radians into Java's unsigned 16-bit turn fraction.
pub fn quantize_aim_angle(angle: f32) -> Result<u16, ProtocolError> {
    if !angle.is_finite() {
        return Err(ProtocolError::NonFiniteAimAngle);
    }
    let normalized = angle.rem_euclid(TAU);
    Ok(((normalized / TAU * ANGLE_STEPS).round() as u32 & 0xffff) as u16)
}

/// Restores an unsigned 16-bit turn fraction into the `[0, 2π)` range.
pub fn dequantize_aim_angle(quantized_angle: u16) -> f32 {
    f32::from(quantized_angle) * TAU / ANGLE_STEPS
}

/// Encodes the shared signed 32-bit seed sent during a Relay handshake.
pub fn encode_start(seed: i32) -> [u8; frame_length::START] {
    let mut frame = [0; frame_length::START];
    frame[0] = message_type::START;
    frame[1..].copy_from_slice(&seed.to_be_bytes());
    frame
}

/// Decodes the shared seed from a Java-compatible start frame.
pub fn decode_start(frame: &[u8]) -> Result<i32, ProtocolError> {
    ensure_length(frame, frame_length::START)?;
    ensure_type(frame[0], message_type::START)?;
    Ok(i32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]))
}

/// Player side identifiers used in round-result frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WinnerSide {
    SideOne = 0,
    SideTwo = 1,
}

impl TryFrom<u8> for WinnerSide {
    type Error = ProtocolError;

    fn try_from(side: u8) -> Result<Self, Self::Error> {
        match side {
            0 => Ok(Self::SideOne),
            1 => Ok(Self::SideTwo),
            side => Err(ProtocolError::InvalidWinnerSide { side }),
        }
    }
}

/// Immutable snapshot of one completed round from the sender's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundResult {
    round_number: u8,
    winner: WinnerSide,
    player_one_wins: u8,
    player_two_wins: u8,
    match_complete: bool,
}

impl RoundResult {
    /// Builds a round result; round zero is rejected by the decoder and constructor.
    pub const fn new(
        round_number: u8,
        winner: WinnerSide,
        player_one_wins: u8,
        player_two_wins: u8,
        match_complete: bool,
    ) -> Result<Self, ProtocolError> {
        if round_number == 0 {
            return Err(ProtocolError::InvalidRoundNumber {
                round: round_number,
            });
        }
        Ok(Self {
            round_number,
            winner,
            player_one_wins,
            player_two_wins,
            match_complete,
        })
    }

    pub const fn round_number(self) -> u8 {
        self.round_number
    }

    pub const fn winner(self) -> WinnerSide {
        self.winner
    }

    pub const fn player_one_wins(self) -> u8 {
        self.player_one_wins
    }

    pub const fn player_two_wins(self) -> u8 {
        self.player_two_wins
    }

    pub const fn match_complete(self) -> bool {
        self.match_complete
    }

    /// Returns the same outcome from the opponent's local perspective.
    pub const fn mirrored(self) -> Self {
        Self {
            round_number: self.round_number,
            winner: match self.winner {
                WinnerSide::SideOne => WinnerSide::SideTwo,
                WinnerSide::SideTwo => WinnerSide::SideOne,
            },
            player_one_wins: self.player_two_wins,
            player_two_wins: self.player_one_wins,
            match_complete: self.match_complete,
        }
    }
}

/// Encodes one completed round into its fixed six-byte frame.
pub const fn encode_round_result(result: RoundResult) -> [u8; frame_length::ROUND_RESULT] {
    [
        message_type::ROUND_RESULT,
        result.round_number,
        result.winner as u8,
        result.player_one_wins,
        result.player_two_wins,
        result.match_complete as u8,
    ]
}

/// Decodes and validates one completed-round frame.
pub fn decode_round_result(frame: &[u8]) -> Result<RoundResult, ProtocolError> {
    ensure_length(frame, frame_length::ROUND_RESULT)?;
    ensure_type(frame[0], message_type::ROUND_RESULT)?;
    RoundResult::new(
        frame[1],
        WinnerSide::try_from(frame[2])?,
        frame[3],
        frame[4],
        frame[5] != 0,
    )
}

/// Immutable request to start another round or reset a completed match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RematchRequest {
    round_number: u8,
    match_reset: bool,
}

impl RematchRequest {
    /// Builds a rematch request; round zero is rejected.
    pub const fn new(round_number: u8, match_reset: bool) -> Result<Self, ProtocolError> {
        if round_number == 0 {
            return Err(ProtocolError::InvalidRoundNumber {
                round: round_number,
            });
        }
        Ok(Self {
            round_number,
            match_reset,
        })
    }

    pub const fn round_number(self) -> u8 {
        self.round_number
    }

    pub const fn match_reset(self) -> bool {
        self.match_reset
    }
}

/// Encodes one rematch request into its fixed three-byte frame.
pub const fn encode_rematch_request(
    request: RematchRequest,
) -> [u8; frame_length::REMATCH_REQUEST] {
    [
        message_type::REMATCH_REQUEST,
        request.round_number,
        request.match_reset as u8,
    ]
}

/// Decodes and validates one rematch request frame.
pub fn decode_rematch_request(frame: &[u8]) -> Result<RematchRequest, ProtocolError> {
    ensure_length(frame, frame_length::REMATCH_REQUEST)?;
    ensure_type(frame[0], message_type::REMATCH_REQUEST)?;
    RematchRequest::new(frame[1], frame[2] != 0)
}

fn ensure_length(frame: &[u8], expected: usize) -> Result<(), ProtocolError> {
    if frame.len() == expected {
        Ok(())
    } else {
        Err(ProtocolError::InvalidLength {
            expected,
            actual: frame.len(),
        })
    }
}

fn ensure_type(actual: u8, expected: u8) -> Result<(), ProtocolError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ProtocolError::UnexpectedType { expected, actual })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_all_buttons_match_java_flag_layout() {
        let input = InputFrame::new(true, true, true, true, true, true, None);
        assert_eq!(input.flags(), 0x3f);
        assert_eq!(
            encode_input_frame(input).expect("finite input"),
            [0x00, 0x3f, 0x00, 0x00]
        );
    }

    #[test]
    fn input_angle_uses_big_endian_quantized_turn_fraction() {
        let input = InputFrame::new(
            false,
            false,
            false,
            false,
            false,
            false,
            Some(-core::f32::consts::FRAC_PI_2),
        );
        assert_eq!(
            encode_input_frame(input).expect("finite angle"),
            [0x00, 0x40, 0xc0, 0x00]
        );

        let decoded = decode_input_frame(&[0x00, 0x40, 0xc0, 0x00]).expect("valid input");
        assert!(
            (decoded.aim_angle().expect("aim present") - 3.0 * core::f32::consts::FRAC_PI_2).abs()
                < 0.0001
        );
    }

    #[test]
    fn start_round_and_rematch_vectors_match_java_wire_format() {
        assert_eq!(encode_start(-42), [0x01, 0xff, 0xff, 0xff, 0xd6]);
        assert_eq!(
            decode_start(&[0x01, 0xff, 0xff, 0xff, 0xd6]).expect("valid start"),
            -42
        );

        let result = RoundResult::new(3, WinnerSide::SideTwo, 2, 1, true).expect("valid result");
        assert_eq!(encode_round_result(result), [0x04, 3, 1, 2, 1, 1]);
        assert_eq!(
            decode_round_result(&[0x04, 3, 1, 2, 1, 1]).expect("valid result"),
            result
        );

        let request = RematchRequest::new(3, true).expect("valid request");
        assert_eq!(encode_rematch_request(request), [0x05, 3, 1]);
        assert_eq!(
            decode_rematch_request(&[0x05, 3, 1]).expect("valid request"),
            request
        );
    }

    #[test]
    fn result_mirror_swaps_winner_and_scores() {
        let result = RoundResult::new(2, WinnerSide::SideOne, 2, 1, false).expect("valid result");
        assert_eq!(result.mirrored().winner(), WinnerSide::SideTwo);
        assert_eq!(result.mirrored().player_one_wins(), 1);
        assert_eq!(result.mirrored().player_two_wins(), 2);
    }

    #[test]
    fn malformed_frames_are_rejected() {
        assert_eq!(
            decode_input_frame(&[0x00]).unwrap_err(),
            ProtocolError::InvalidLength {
                expected: 4,
                actual: 1
            }
        );
        assert_eq!(
            decode_input_frame(&[0x00, 0x80, 0x00, 0x00]).unwrap_err(),
            ProtocolError::UnknownInputFlags { flags: 0x80 }
        );
        assert_eq!(
            decode_input_frame(&[0x00, 0x00, 0x00, 0x01]).unwrap_err(),
            ProtocolError::AimAngleWithoutFlag { quantized_angle: 1 }
        );
        assert_eq!(
            decode_round_result(&[0x04, 0x00, 0x00, 0x00, 0x00, 0x00]).unwrap_err(),
            ProtocolError::InvalidRoundNumber { round: 0 }
        );
        assert_eq!(
            decode_round_result(&[0x04, 0x01, 0x02, 0x00, 0x00, 0x00]).unwrap_err(),
            ProtocolError::InvalidWinnerSide { side: 2 }
        );
        assert_eq!(
            decode_rematch_request(&[0x05, 0x00, 0x00]).unwrap_err(),
            ProtocolError::InvalidRoundNumber { round: 0 }
        );
        assert_eq!(
            encode_input_frame(InputFrame::new(
                false,
                false,
                false,
                false,
                false,
                false,
                Some(f32::NAN)
            ))
            .unwrap_err(),
            ProtocolError::NonFiniteAimAngle
        );
    }
}
