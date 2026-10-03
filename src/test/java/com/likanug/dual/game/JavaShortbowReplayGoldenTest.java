package com.likanug.dual.game;

import com.likanug.dual.App;
import com.likanug.dual.actor.ActorGroup;
import com.likanug.dual.actor.player.PlayerActor;
import com.likanug.dual.inputDevice.KeyInput;
import com.likanug.dual.playerEngine.AiDifficulty;
import com.likanug.dual.state.PlayGameState;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;

/** Captures Java shortbow hit recovery and swept interception behavior for Rust replay. */
class JavaShortbowReplayGoldenTest {

    private static final Path FIXTURE_DIRECTORY = Path.of("rust", "fixtures", "replay");

    @Test
    void fixedShortbowInputsMatchTheJavaHitAndInterceptionTraces() throws IOException {
        assertEquals(
                Files.readString(FIXTURE_DIRECTORY.resolve("java-shortbow-hit.csv")).strip(),
                recordTrace(
                        "shortbow-hit-inputs.tsv",
                        640.0F,
                        520.0F,
                        640.0F,
                        480.0F).strip());
        assertEquals(
                Files.readString(FIXTURE_DIRECTORY.resolve("java-shortbow-interception.csv")).strip(),
                recordTrace(
                        "shortbow-interception-inputs.tsv",
                        200.0F,
                        360.0F,
                        300.0F,
                        360.0F).strip());
        assertEquals(
                Files.readString(FIXTURE_DIRECTORY.resolve("java-cover-impact.csv")).strip(),
                recordTrace(
                        "cover-impact-inputs.tsv",
                        400.0F,
                        360.0F,
                        880.0F,
                        360.0F,
                        ArenaLayout.centralCover()).strip());
        assertEquals(
                Files.readString(FIXTURE_DIRECTORY.resolve("java-cover-player.csv")).strip(),
                recordTrace(
                        "cover-player-inputs.tsv",
                        450.0F,
                        360.0F,
                        880.0F,
                        360.0F,
                        ArenaLayout.centralCover()).strip());
        assertEquals(
                Files.readString(FIXTURE_DIRECTORY.resolve("java-tactical-combo.csv")).strip(),
                recordTrace(
                        "tactical-combo-inputs.tsv",
                        640.0F,
                        520.0F,
                        640.0F,
                        480.0F).strip());
        assertEquals(
                Files.readString(FIXTURE_DIRECTORY.resolve("java-charge-disrupt.csv")).strip(),
                recordTrace(
                        "charge-disrupt-inputs.tsv",
                        640.0F,
                        520.0F,
                        640.0F,
                        480.0F).strip());
    }

    /** Runs the shared no-render combat update order for one controlled local scenario. */
    static String recordTrace(
            String inputFile,
            float playerOneX,
            float playerOneY,
            float playerTwoX,
            float playerTwoY) throws IOException {
        return recordTrace(inputFile, playerOneX, playerOneY, playerTwoX, playerTwoY, ArenaLayout.open());
    }

    /** Runs one replay in the supplied arena while retaining Java's player and projectile update order. */
    static String recordTrace(
            String inputFile,
            float playerOneX,
            float playerOneY,
            float playerTwoX,
            float playerTwoY,
            ArenaLayout arenaLayout) throws IOException {
        App app = new App();
        KeyInput playerOneInput = new KeyInput();
        KeyInput playerTwoInput = new KeyInput();
        app.setCurrentKeyInput(playerOneInput);
        app.setSecondKeyInput(playerTwoInput);
        GameSystem game = new GameSystem(false, false, app, true, AiDifficulty.STANDARD, arenaLayout);
        app.setSystem(game);
        PlayerActor playerOne = (PlayerActor) game.getMyGroup().getPlayer();
        PlayerActor playerTwo = (PlayerActor) game.getOtherGroup().getPlayer();
        playerOne.setxPosition(playerOneX);
        playerOne.setyPosition(playerOneY);
        playerTwo.setxPosition(playerTwoX);
        playerTwo.setyPosition(playerTwoY);
        PlayGameState playState = new PlayGameState(app);

        StringBuilder trace = new StringBuilder(JavaMatchReplayGoldenTest.HEADER).append('\n');
        List<InputRange> ranges = readInputRanges(FIXTURE_DIRECTORY.resolve(inputFile));
        int globalFrame = 0;
        int expectedFrame = 0;
        for (InputRange range : ranges) {
            if (range.firstFrame() != expectedFrame || range.lastFrame() < range.firstFrame()) {
                throw new IllegalStateException("Replay ranges must be contiguous and ordered.");
            }
            for (int frame = range.firstFrame(); frame <= range.lastFrame(); frame++) {
                JavaMatchReplayGoldenTest.applyInput(playerOneInput, range.playerOneMask());
                JavaMatchReplayGoldenTest.applyInput(playerTwoInput, range.playerTwoMask());
                RoundCombatStats.Snapshot statsBefore = game.getRoundCombatStats();
                int tacticalEventsBefore = game.getTacticalEventLog().size();
                int playerOneWinsBefore = game.getMatchScore().getPlayerOneWins();
                int playerTwoWinsBefore = game.getMatchScore().getPlayerTwoWins();
                boolean[] chargeReadyBefore = {
                        playerOne.isChargeReadyFeedbackShown(),
                        playerTwo.isChargeReadyFeedbackShown(),
                };
                game.advanceCombatFrame();
                game.getMyGroup().update();
                game.getOtherGroup().update();
                int removalsBeforeCover = removingArrowCount(game.getMyGroup(), game.getOtherGroup());
                game.resolveArenaCollisions();
                game.getMyGroup().actPlayer();
                game.getOtherGroup().actPlayer();
                game.resolveArrowCoverCollisions();
                boolean coverImpact =
                        removingArrowCount(game.getMyGroup(), game.getOtherGroup()) > removalsBeforeCover;
                game.getMyGroup().actArrows();
                game.getOtherGroup().actArrows();
                playState.checkCollision(game);
                playState.checkStateTransition(game);

                RoundCombatStats.Snapshot statsAfter = game.getRoundCombatStats();
                List<TacticalEvent> currentTacticalEvents = game.getTacticalEventLog();
                int eventFlags = JavaMatchReplayGoldenTest.eventFlags(
                        statsBefore,
                        statsAfter,
                        chargeReadyBefore,
                        new PlayerActor[] {playerOne, playerTwo},
                        coverImpact,
                        currentTacticalEvents.subList(tacticalEventsBefore, currentTacticalEvents.size()));
                int playerOneWins = game.getMatchScore().getPlayerOneWins();
                int playerTwoWins = game.getMatchScore().getPlayerTwoWins();
                boolean roundFinished =
                        playerOneWins + playerTwoWins > playerOneWinsBefore + playerTwoWinsBefore;
                trace.append(globalFrame++)
                        .append(",1,")
                        .append(frame + 1)
                        .append(',').append(range.playerOneMask())
                        .append(',').append(range.playerTwoMask());
                JavaMatchReplayGoldenTest.appendPlayer(
                        trace, playerOne, !game.getMyGroup().getPlayer().isNull());
                JavaMatchReplayGoldenTest.appendPlayer(
                        trace, playerTwo, !game.getOtherGroup().getPlayer().isNull());
                trace.append(',').append(playerOneWins)
                        .append(',').append(playerTwoWins)
                        .append(',').append(roundFinished ? playerOneWins + playerTwoWins : 0)
                        .append(',').append(roundFinished ? (playerOneWins > playerOneWinsBefore ? 1 : 2) : 0)
                        .append(',').append(roundFinished && game.getMatchScore().isMatchComplete() ? 1 : 0)
                        .append(',').append(eventFlags);
                JavaMatchReplayGoldenTest.appendArrows(trace, game.getMyGroup(), game.getOtherGroup());
                trace.append('\n');
                if (roundFinished) return trace.toString();
            }
            expectedFrame = range.lastFrame() + 1;
        }
        return trace.toString();
    }

    private static int removingArrowCount(ActorGroup playerOne, ActorGroup playerTwo) {
        return playerOne.getRemovingArrowList().size() + playerTwo.getRemovingArrowList().size();
    }

    private static List<InputRange> readInputRanges(Path path) throws IOException {
        List<InputRange> ranges = new ArrayList<>();
        for (String line : Files.readAllLines(path)) {
            if (line.isBlank() || line.startsWith("#")) continue;
            String[] fields = line.split("\t");
            if (fields.length != 4) {
                throw new IllegalStateException("Replay range must contain four tab-separated fields.");
            }
            ranges.add(new InputRange(
                    Integer.parseInt(fields[0]),
                    Integer.parseInt(fields[1]),
                    Integer.parseInt(fields[2]),
                    Integer.parseInt(fields[3])));
        }
        return ranges;
    }

    private record InputRange(int firstFrame, int lastFrame, int playerOneMask, int playerTwoMask) {
    }
}
