const VOLUME_LEVELS: [f32; 3] = [0.0, 0.5, 1.0];

/// Stores the Java client's three feedback volume levels for the settings overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AudioSettings {
    level_index: usize,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            level_index: VOLUME_LEVELS.len() - 1,
        }
    }
}

impl AudioSettings {
    pub(super) fn volume(self) -> f32 {
        VOLUME_LEVELS[self.level_index]
    }

    pub(super) fn increase_volume(&mut self) {
        self.level_index = (self.level_index + 1).min(VOLUME_LEVELS.len() - 1);
    }

    pub(super) fn decrease_volume(&mut self) {
        self.level_index = self.level_index.saturating_sub(1);
    }

    /// Matches the Java settings control: mute any active level, or restore full volume.
    pub(super) fn toggle_mute(&mut self) {
        self.level_index = if self.volume() == 0.0 {
            VOLUME_LEVELS.len() - 1
        } else {
            0
        };
    }

    pub(super) fn label(self) -> &'static str {
        match self.level_index {
            0 => "Muted",
            1 => "50%",
            _ => "100%",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_controls_clamp_at_muted_and_full_levels() {
        let mut settings = AudioSettings::default();
        assert_eq!(settings.volume(), 1.0);
        assert_eq!(settings.label(), "100%");

        settings.increase_volume();
        assert_eq!(settings.label(), "100%");
        settings.decrease_volume();
        assert_eq!(settings.volume(), 0.5);
        settings.decrease_volume();
        settings.decrease_volume();
        assert_eq!(settings.volume(), 0.0);
        assert_eq!(settings.label(), "Muted");
    }

    #[test]
    fn mute_shortcut_toggles_between_silence_and_full_volume() {
        let mut settings = AudioSettings::default();
        settings.decrease_volume();
        settings.toggle_mute();
        assert_eq!(settings.volume(), 0.0);
        settings.toggle_mute();
        assert_eq!(settings.volume(), 1.0);
    }
}
