package com.likanug.dual.game;

import com.likanug.dual.App;
import com.likanug.dual.actor.ActorGroup;
import com.likanug.dual.actor.arrow.AbstractArrowActor;
import com.likanug.dual.actor.arrow.LongbowArrowHead;
import com.likanug.dual.actor.arrow.LongbowArrowShaft;
import com.likanug.dual.actor.arrow.ShortbowArrow;
import com.likanug.dual.actor.player.PlayerActor;
import com.likanug.dual.inputDevice.KeyInput;
import com.likanug.dual.playerEngine.AiDifficulty;
import com.likanug.dual.state.PlayGameState;
import com.likanug.dual.state.PlayerActorState;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

import static org.junit.jupiter.api.Assertions.assertEquals;

/** Keeps the complete fixed-input longbow match trace tied to the Java rules baseline. */
class JavaMatchReplayGoldenTest {

    private static final Path REPLAY_INPUT =
            Path.of("rust", "fixtures", "replay", "longbow-match-inputs.tsv");
    private static final Path JAVA_GOLDEN =
            Path.of("rust", "fixtures", "replay", "java-longbow-match.csv");

    static final String HEADER =
            "#global_frame,round,round_frame,p1_mask,p2_mask,"
                    + "p1_x,p1_y,p1_vx,p1_vy,p1_aim,p1_phase,p1_ammo,p1_ammo_recovery,p1_cooldown,p1_charge,p1_recovery,p1_damage,p1_pressure,"
                    + "p2_x,p2_y,p2_vx,p2_vy,p2_aim,p2_phase,p2_ammo,p2_ammo_recovery,p2_cooldown,p2_charge,p2_recovery,p2_damage,p2_pressure,"
                    + "score_one,score_two,result_round,result_winner,result_match_complete,event_flags,arrow_count,"
                    + "arrows(owner,kind,x,y,vx,vy,rotation)";

    @Test
    void fixedLongbowInputsMatchTheJavaThreeRoundMatch() throws IOException {
        assertEquals(Files.readString(JAVA_GOLDEN).strip(), recordJavaTrace().strip());
    }

    /** Runs the no-render combat update order and snapshots every rule frame for Rust replay. */
    static String recordJavaTrace() throws IOException {
        App app = new App();
        KeyInput playerOneInput = new KeyInput();
        KeyInput playerTwoInput = new KeyInput();
        app.setCurrentKeyInput(playerOneInput);
        app.setSecondKeyInput(playerTwoInput);
        GameSystem game = new GameSystem(
                false,
                false,
                app,
                true,
                AiDifficulty.STANDARD,
                ArenaLayout.open());
        app.setSystem(game);

        StringBuilder trace = new StringBuilder(HEADER).append('\n');
        List<InputRange> ranges = readInputRanges();
        int globalFrame = 0;
        for (int round = 1; round <= 3; round++) {
            if (round > 1) game.resetRound();
            PlayGameState playState = new PlayGameState(app);
            PlayerActor[] players = {
                    (PlayerActor) game.getMyGroup().getPlayer(),
                    (PlayerActor) game.getOtherGroup().getPlayer(),
            };
            int expectedRoundFrame = 0;
            boolean roundFinished = false;

            for (InputRange range : ranges) {
                if (range.round() != round) continue;
                if (range.firstFrame() != expectedRoundFrame || range.lastFrame() < range.firstFrame()) {
                    throw new IllegalStateException("Replay ranges must be contiguous within each round.");
                }

                for (int frame = range.firstFrame(); frame <= range.lastFrame(); frame++) {
                    applyInput(playerOneInput, range.playerOneMask());
                    applyInput(playerTwoInput, range.playerTwoMask());
                    RoundCombatStats.Snapshot statsBefore = game.getRoundCombatStats();
                    int tacticalEventsBefore = game.getTacticalEventLog().size();
                    boolean[] chargeReadyBefore = {
                            players[0].isChargeReadyFeedbackShown(),
                            players[1].isChargeReadyFeedbackShown(),
                    };
                    int playerOneWinsBefore = game.getMatchScore().getPlayerOneWins();
                    int playerTwoWinsBefore = game.getMatchScore().getPlayerTwoWins();

                    game.advanceCombatFrame();
                    game.getMyGroup().update();
                    game.getOtherGroup().update();
                    game.resolveArenaCollisions();
                    game.getMyGroup().actPlayer();
                    game.getOtherGroup().actPlayer();
                    game.resolveArrowCoverCollisions();
                    game.getMyGroup().actArrows();
                    game.getOtherGroup().actArrows();
                    playState.checkCollision(game);
                    playState.checkStateTransition(game);

                    RoundCombatStats.Snapshot statsAfter = game.getRoundCombatStats();
                    List<TacticalEvent> currentTacticalEvents = game.getTacticalEventLog();
                    int eventFlags = eventFlags(
                            statsBefore,
                            statsAfter,
                            chargeReadyBefore,
                            players,
                            false,
                            currentTacticalEvents.subList(tacticalEventsBefore, currentTacticalEvents.size()));
                    int resultRound = 0;
                    int resultWinner = 0;
                    int resultMatchComplete = 0;
                    int playerOneWins = game.getMatchScore().getPlayerOneWins();
                    int playerTwoWins = game.getMatchScore().getPlayerTwoWins();
                    if (playerOneWins + playerTwoWins > playerOneWinsBefore + playerTwoWinsBefore) {
                        resultRound = playerOneWins + playerTwoWins;
                        resultWinner = playerOneWins > playerOneWinsBefore ? 1 : 2;
                        resultMatchComplete = game.getMatchScore().isMatchComplete() ? 1 : 0;
                        roundFinished = true;
                    }

                    trace.append(globalFrame++)
                            .append(',').append(round)
                            .append(',').append(frame + 1)
                            .append(',').append(range.playerOneMask())
                            .append(',').append(range.playerTwoMask());
                    appendPlayer(trace, players[0], !game.getMyGroup().getPlayer().isNull());
                    appendPlayer(trace, players[1], !game.getOtherGroup().getPlayer().isNull());
                    trace.append(',').append(playerOneWins)
                            .append(',').append(playerTwoWins)
                            .append(',').append(resultRound)
                            .append(',').append(resultWinner)
                            .append(',').append(resultMatchComplete)
                            .append(',').append(eventFlags);
                    appendArrows(trace, game.getMyGroup(), game.getOtherGroup());
                    trace.append('\n');

                    if (roundFinished) break;
                }
                if (roundFinished) break;
                expectedRoundFrame = range.lastFrame() + 1;
            }

            if (!roundFinished) {
                throw new IllegalStateException("Scripted longbow round did not produce a lethal result.");
            }
            if (playerOneWins(game, round) != round) {
                throw new IllegalStateException("Every scripted longbow round must be won by player one.");
            }
        }
        return trace.toString();
    }

    private static List<InputRange> readInputRanges() throws IOException {
        List<InputRange> ranges = new ArrayList<>();
        for (String line : Files.readAllLines(REPLAY_INPUT)) {
            if (line.isBlank() || line.startsWith("#")) continue;
            String[] fields = line.split("\t");
            if (fields.length != 5) {
                throw new IllegalStateException("Replay range must contain five tab-separated fields.");
            }
            ranges.add(new InputRange(
                    Integer.parseInt(fields[0]),
                    Integer.parseInt(fields[1]),
                    Integer.parseInt(fields[2]),
                    Integer.parseInt(fields[3]),
                    Integer.parseInt(fields[4])));
        }
        return ranges;
    }

    private static int playerOneWins(GameSystem game, int expectedRound) {
        int wins = game.getMatchScore().getPlayerOneWins();
        if (wins != expectedRound || game.getMatchScore().getPlayerTwoWins() != 0) {
            throw new IllegalStateException("Longbow match score did not reach the expected round total.");
        }
        return wins;
    }

    static int eventFlags(
            RoundCombatStats.Snapshot before,
            RoundCombatStats.Snapshot after,
            boolean[] chargeReadyBefore,
            PlayerActor[] players) {
        return eventFlags(before, after, chargeReadyBefore, players, false, List.of());
    }

    static int eventFlags(
            RoundCombatStats.Snapshot before,
            RoundCombatStats.Snapshot after,
            boolean[] chargeReadyBefore,
            PlayerActor[] players,
            boolean coverImpact) {
        return eventFlags(before, after, chargeReadyBefore, players, coverImpact, List.of());
    }

    static int eventFlags(
            RoundCombatStats.Snapshot before,
            RoundCombatStats.Snapshot after,
            boolean[] chargeReadyBefore,
            PlayerActor[] players,
            boolean coverImpact,
            List<TacticalEvent> tacticalEvents) {
        int flags = 0;
        if (!chargeReadyBefore[0] && players[0].isChargeReadyFeedbackShown()) flags |= 1;
        if (!chargeReadyBefore[1] && players[1].isChargeReadyFeedbackShown()) flags |= 1 << 1;
        flags |= statEventFlags(before.playerOne(), after.playerOne(), 1);
        flags |= statEventFlags(before.playerTwo(), after.playerTwo(), 2);
        if (after.interceptionCount() > before.interceptionCount()) flags |= 1 << 10;
        if (coverImpact) flags |= 1 << 11;
        flags |= tacticalEventFlags(tacticalEvents);
        return flags;
    }

    private static int tacticalEventFlags(List<TacticalEvent> events) {
        int flags = 0;
        for (TacticalEvent event : events) {
            int side = event.attacker() == PlayerSide.ONE ? 0 : 1;
            int bit = switch (event.type()) {
                case PRESSURE -> 12 + side;
                case OPENING -> 14 + side;
                case DISRUPT -> 16 + side;
                case FINISH -> 18 + side;
                case INTERCEPT -> 20;
            };
            flags |= 1 << bit;
        }
        return flags;
    }

    private static int statEventFlags(
            RoundCombatStats.PlayerSnapshot before,
            RoundCombatStats.PlayerSnapshot after,
            int side) {
        int flags = 0;
        int firedBit = side == 1 ? 2 : 3;
        int hitBit = side == 1 ? 4 : 5;
        int shortbowFiredBit = side == 1 ? 6 : 7;
        int shortbowHitBit = side == 1 ? 8 : 9;
        if (after.longbowShots() > before.longbowShots()) flags |= 1 << firedBit;
        if (after.longbowHits() > before.longbowHits()) flags |= 1 << hitBit;
        if (after.shortbowShots() > before.shortbowShots()) flags |= 1 << shortbowFiredBit;
        if (after.shortbowHits() > before.shortbowHits()) flags |= 1 << shortbowHitBit;
        return flags;
    }

    static void applyInput(KeyInput input, int mask) {
        input.isWPressed = (mask & 0x01) != 0;
        input.isSPressed = (mask & 0x02) != 0;
        input.isAPressed = (mask & 0x04) != 0;
        input.isDPressed = (mask & 0x08) != 0;
        input.isZPressed = (mask & 0x10) != 0;
        input.isXPressed = (mask & 0x20) != 0;
    }

    static void appendPlayer(StringBuilder trace, PlayerActor player, boolean alive) {
        trace.append(',').append(floatBits(player.getxPosition()))
                .append(',').append(floatBits(player.getyPosition()))
                .append(',').append(floatBits(player.getxVelocity()))
                .append(',').append(floatBits(player.getyVelocity()))
                .append(',').append(floatBits(player.getAimAngle()))
                .append(',').append(alive ? phaseCode(player) : 4)
                .append(',').append(player.getShortbowAmmo().getAvailableAmmo())
                .append(',').append(player.getShortbowAmmo().getRecoveryFrameCount())
                .append(',').append(player.getShortbowCooldownFrameCount())
                .append(',').append(player.getChargedFrameCount())
                .append(',').append(player.getLongbowRecoveryFrameCount())
                .append(',').append(player.getDamageRemainingFrameCount())
                .append(',').append(player.getShortbowPressure().getConsecutiveRefreshes());
    }

    private static int phaseCode(PlayerActor player) {
        PlayerActorState state = player.getState();
        if (state.isDamaged()) return 3;
        if (state.isDrawingLongBow()) return 2;
        if (player.getShortbowActionFrameCount() > 0) return 1;
        return 0;
    }

    static void appendArrows(StringBuilder trace, ActorGroup playerOne, ActorGroup playerTwo) {
        List<AbstractArrowActor> playerOneArrows = liveArrows(playerOne);
        List<AbstractArrowActor> playerTwoArrows = liveArrows(playerTwo);
        trace.append(',').append(playerOneArrows.size() + playerTwoArrows.size());
        appendArrowGroup(trace, playerOneArrows, 1);
        appendArrowGroup(trace, playerTwoArrows, 2);
    }

    private static List<AbstractArrowActor> liveArrows(ActorGroup group) {
        return group.getArrowList().stream()
                .filter(arrow -> !group.getRemovingArrowList().contains(arrow))
                .toList();
    }

    private static void appendArrowGroup(StringBuilder trace, List<AbstractArrowActor> arrows, int side) {
        for (AbstractArrowActor arrow : arrows) {
            trace.append(',').append(side)
                    .append(',').append(arrowKind(arrow))
                    .append(',').append(floatBits(arrow.getxPosition()))
                    .append(',').append(floatBits(arrow.getyPosition()))
                    .append(',').append(floatBits(arrow.getxVelocity()))
                    .append(',').append(floatBits(arrow.getyVelocity()))
                    .append(',').append(floatBits(arrow.getRotationAngle()));
        }
    }

    private static int arrowKind(AbstractArrowActor arrow) {
        if (arrow instanceof ShortbowArrow) return 0;
        if (arrow instanceof LongbowArrowShaft) return 1;
        if (arrow instanceof LongbowArrowHead) return 2;
        throw new IllegalArgumentException("Unsupported Java replay projectile: " + arrow.getClass().getName());
    }

    private static String floatBits(float value) {
        return String.format(Locale.ROOT, "%08x", Float.floatToRawIntBits(value));
    }

    private record InputRange(int round, int firstFrame, int lastFrame, int playerOneMask, int playerTwoMask) {
    }
}
