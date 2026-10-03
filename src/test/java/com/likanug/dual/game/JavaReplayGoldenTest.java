package com.likanug.dual.game;

import com.likanug.dual.App;
import com.likanug.dual.actor.player.PlayerActor;
import com.likanug.dual.inputDevice.KeyInput;
import com.likanug.dual.playerEngine.AiDifficulty;
import com.likanug.dual.state.PlayerActorState;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Locale;

import static org.junit.jupiter.api.Assertions.assertEquals;

/** Keeps the committed movement replay tied to the unmodified Java rule baseline. */
class JavaReplayGoldenTest {

    private static final Path REPLAY_INPUT =
            Path.of("rust", "fixtures", "replay", "movement-inputs.tsv");
    private static final Path JAVA_GOLDEN =
            Path.of("rust", "fixtures", "replay", "java-movement-180.csv");

    @Test
    void fixedLocalInputsMatchTheRecordedJavaMovementTrace() throws IOException {
        assertEquals(Files.readString(JAVA_GOLDEN).strip(), recordJavaTrace().strip());
    }

    /** Runs the same update/collision/input order as PlayGameState without invoking Processing drawing. */
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
        PlayerActor[] players = {
                (PlayerActor) game.getMyGroup().getPlayer(),
                (PlayerActor) game.getOtherGroup().getPlayer(),
        };
        StringBuilder trace = new StringBuilder(
                "# frame,p1_mask,p2_mask,p1_x,p1_y,p1_vx,p1_vy,p1_aim,p1_phase,p1_ammo,p1_cooldown,p1_charge,p1_recovery,p1_damage,p1_pressure,"
                        + "p2_x,p2_y,p2_vx,p2_vy,p2_aim,p2_phase,p2_ammo,p2_cooldown,p2_charge,p2_recovery,p2_damage,p2_pressure\n");

        int frame = 0;
        for (String line : Files.readAllLines(REPLAY_INPUT)) {
            if (line.isBlank() || line.startsWith("#")) continue;
            String[] fields = line.split("\t");
            int firstFrame = Integer.parseInt(fields[0]);
            int lastFrame = Integer.parseInt(fields[1]);
            int playerOneMask = Integer.parseInt(fields[2]);
            int playerTwoMask = Integer.parseInt(fields[3]);
            if (firstFrame != frame || lastFrame < firstFrame) {
                throw new IllegalStateException("Replay input ranges must be contiguous and ordered.");
            }

            for (int replayFrame = firstFrame; replayFrame <= lastFrame; replayFrame++) {
                applyMovement(playerOneInput, playerOneMask);
                applyMovement(playerTwoInput, playerTwoMask);
                game.getMyGroup().update();
                game.getOtherGroup().update();
                game.resolveArenaCollisions();
                game.getMyGroup().actPlayer();
                game.getOtherGroup().actPlayer();
                game.resolveArrowCoverCollisions();
                game.getMyGroup().actArrows();
                game.getOtherGroup().actArrows();

                trace.append(replayFrame + 1)
                        .append(',').append(playerOneMask)
                        .append(',').append(playerTwoMask);
                appendPlayer(trace, players[0]);
                appendPlayer(trace, players[1]);
                trace.append('\n');
            }
            frame = lastFrame + 1;
        }
        return trace.toString();
    }

    private static void applyMovement(KeyInput input, int mask) {
        input.isWPressed = (mask & 0x01) != 0;
        input.isSPressed = (mask & 0x02) != 0;
        input.isAPressed = (mask & 0x04) != 0;
        input.isDPressed = (mask & 0x08) != 0;
    }

    private static void appendPlayer(StringBuilder trace, PlayerActor player) {
        trace.append(',').append(floatBits(player.getxPosition()))
                .append(',').append(floatBits(player.getyPosition()))
                .append(',').append(floatBits(player.getxVelocity()))
                .append(',').append(floatBits(player.getyVelocity()))
                .append(',').append(floatBits(player.getAimAngle()))
                .append(',').append(phaseCode(player))
                .append(',').append(player.getShortbowAmmo().getAvailableAmmo())
                .append(',').append(player.getShortbowCooldownFrameCount())
                .append(',').append(player.getChargedFrameCount())
                .append(',').append(player.getLongbowRecoveryFrameCount())
                .append(',').append(player.getDamageRemainingFrameCount())
                .append(',').append(player.getShortbowPressure().getConsecutiveRefreshes());
    }

    private static String floatBits(float value) {
        return String.format(Locale.ROOT, "%08x", Float.floatToRawIntBits(value));
    }

    private static int phaseCode(PlayerActor player) {
        PlayerActorState state = player.getState();
        if (state.isDamaged()) return 3;
        if (state.isDrawingLongBow()) return 2;
        if (player.getShortbowActionFrameCount() > 0) return 1;
        return 0;
    }
}
