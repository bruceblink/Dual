use rodio::{OutputStreamBuilder, Sink, buffer::SamplesBuffer};
use std::sync::mpsc::{self, SyncSender};

const SAMPLE_RATE: u32 = 44_100;
const DURATION_MILLIS: u32 = 80;
const FREQUENCY_HZ: f32 = 880.0;

/// Sends optional sound cues to a worker that owns the output stream and never blocks the game loop.
pub(super) struct AudioFeedback {
    commands: SyncSender<f32>,
}

impl AudioFeedback {
    /// Starts audio initialization separately so missing devices cannot interrupt the client loop.
    pub(super) fn start() -> Option<Self> {
        let (commands, receiver) = mpsc::sync_channel(4);
        let worker = std::thread::Builder::new()
            .name("dual-audio-feedback".to_owned())
            .spawn(move || {
                let mut stream = match OutputStreamBuilder::open_default_stream() {
                    Ok(stream) => stream,
                    Err(error) => {
                        eprintln!("Audio feedback is unavailable: {error}");
                        return;
                    }
                };
                stream.log_on_drop(false);
                let sink = Sink::connect_new(stream.mixer());
                while let Ok(volume) = receiver.recv() {
                    if volume <= 0.0 {
                        continue;
                    }
                    sink.set_volume(volume);
                    sink.append(SamplesBuffer::new(1, SAMPLE_RATE, charge_ready_samples()));
                }
            });

        match worker {
            Ok(_) => Some(Self { commands }),
            Err(error) => {
                eprintln!("Audio feedback could not start: {error}");
                None
            }
        }
    }

    /// Drops a cue rather than blocking simulation when the small audio queue is full.
    pub(super) fn play_charge_ready(&self, volume: f32) {
        if volume > 0.0 {
            let _ = self.commands.try_send(volume.clamp(0.0, 1.0));
        }
    }
}

/// Builds the Java baseline's short 880 Hz cue as normalized mono samples.
fn charge_ready_samples() -> Vec<f32> {
    let sample_count = SAMPLE_RATE * DURATION_MILLIS / 1_000;
    (0..sample_count)
        .map(|index| {
            let phase = std::f32::consts::TAU * FREQUENCY_HZ * index as f32 / SAMPLE_RATE as f32;
            phase.sin() * (90.0 / 128.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_charge_ready_tone_matches_the_java_duration_and_frequency() {
        let samples = charge_ready_samples();
        assert_eq!(
            samples.len(),
            (SAMPLE_RATE * DURATION_MILLIS / 1_000) as usize
        );
        assert_eq!(samples[0], 0.0);
        assert!(
            (samples[1]
                - (std::f32::consts::TAU * FREQUENCY_HZ / SAMPLE_RATE as f32).sin()
                    * (90.0 / 128.0))
                .abs()
                < f32::EPSILON
        );
        assert!(samples.iter().all(|sample| (-1.0..=1.0).contains(sample)));
    }
}
