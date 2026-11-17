//! Player state machine and command handling logic, independent of audio I/O.

use std::path::PathBuf;

use crate::tracks::Track;

/// Playback status for the current track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Playing,
    Paused,
}

/// UI-level command derived from key input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerCommand {
    Up,
    Down,
    PlaySelected,
    TogglePause,
    VolumeUp,
    VolumeDown,
    Quit,
}

/// Actions the UI should request from the audio engine in response to a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerAction {
    None,
    Play(PathBuf),
    Pause,
    Resume,
    SetVolume(u8), // 0..=100
    Quit,
}

/// Player state: selection, playback status, and volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerState {
    pub tracks: Vec<Track>,
    pub selected: usize,
    pub status: PlaybackStatus,
    pub current_index: Option<usize>,
    pub volume: u8, // 0..=100
}

impl PlayerState {
    pub fn new(tracks: Vec<Track>) -> Self {
        Self {
            tracks,
            selected: 0,
            status: PlaybackStatus::Stopped,
            current_index: None,
            volume: 100,
        }
    }

    pub fn selected_track(&self) -> Option<&Track> {
        self.tracks.get(self.selected)
    }

    pub fn current_track(&self) -> Option<&Track> {
        self.current_index.and_then(|i| self.tracks.get(i))
    }

    /// Apply a UI command and return the corresponding audio/UI action.
    pub fn handle_command(&mut self, cmd: PlayerCommand) -> PlayerAction {
        match cmd {
            PlayerCommand::Up => {
                if !self.tracks.is_empty() && self.selected > 0 {
                    self.selected -= 1;
                }
                PlayerAction::None
            }
            PlayerCommand::Down => {
                if !self.tracks.is_empty() && self.selected + 1 < self.tracks.len() {
                    self.selected += 1;
                }
                PlayerAction::None
            }
            PlayerCommand::PlaySelected => {
                if let Some(track) = self.selected_track() {
                    let path = track.path.clone();
                    self.current_index = Some(self.selected);
                    self.status = PlaybackStatus::Playing;
                    PlayerAction::Play(path)
                } else {
                    PlayerAction::None
                }
            }
            PlayerCommand::TogglePause => match self.status {
                PlaybackStatus::Playing => {
                    self.status = PlaybackStatus::Paused;
                    PlayerAction::Pause
                }
                PlaybackStatus::Paused => {
                    self.status = PlaybackStatus::Playing;
                    PlayerAction::Resume
                }
                PlaybackStatus::Stopped => PlayerAction::None,
            },
            PlayerCommand::VolumeUp => {
                self.volume = self.volume.saturating_add(5).min(100);
                PlayerAction::SetVolume(self.volume)
            }
            PlayerCommand::VolumeDown => {
                self.volume = self.volume.saturating_sub(5);
                PlayerAction::SetVolume(self.volume)
            }
            PlayerCommand::Quit => PlayerAction::Quit,
        }
    }

    /// Update state in response to audio engine events.
    pub fn on_audio_stopped(&mut self) {
        self.status = PlaybackStatus::Stopped;
    }

    pub fn on_audio_finished(&mut self) {
        self.status = PlaybackStatus::Stopped;
    }

    pub fn on_audio_paused(&mut self) {
        if self.status == PlaybackStatus::Playing {
            self.status = PlaybackStatus::Paused;
        }
    }

    pub fn on_audio_resumed(&mut self) {
        if self.status == PlaybackStatus::Paused {
            self.status = PlaybackStatus::Playing;
        }
    }

    pub fn on_volume_changed(&mut self, volume: u8) {
        self.volume = volume.min(100);
    }

    pub fn volume_as_f32(&self) -> f32 {
        (self.volume as f32) / 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_tracks() -> Vec<Track> {
        vec![
            Track {
                path: PathBuf::from("/music/a.mp3"),
                name: "a.mp3".to_string(),
            },
            Track {
                path: PathBuf::from("/music/b.mp3"),
                name: "b.mp3".to_string(),
            },
            Track {
                path: PathBuf::from("/music/c.mp3"),
                name: "c.mp3".to_string(),
            },
        ]
    }

    #[test]
    fn navigation_up_down_bounds() {
        let mut state = PlayerState::new(sample_tracks());
        assert_eq!(state.selected, 0);
        state.handle_command(PlayerCommand::Up);
        assert_eq!(state.selected, 0);
        state.handle_command(PlayerCommand::Down);
        assert_eq!(state.selected, 1);
        state.handle_command(PlayerCommand::Down);
        assert_eq!(state.selected, 2);
        state.handle_command(PlayerCommand::Down);
        assert_eq!(state.selected, 2);
        state.handle_command(PlayerCommand::Up);
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn play_selected_sets_current_and_playing() {
        let mut state = PlayerState::new(sample_tracks());
        state.selected = 1;
        let action = state.handle_command(PlayerCommand::PlaySelected);
        assert_eq!(action, PlayerAction::Play(PathBuf::from("/music/b.mp3")));
        assert_eq!(state.current_index, Some(1));
        assert_eq!(state.status, PlaybackStatus::Playing);
    }

    #[test]
    fn toggle_pause_transitions() {
        let mut state = PlayerState::new(sample_tracks());
        assert_eq!(
            state.handle_command(PlayerCommand::TogglePause),
            PlayerAction::None
        );
        state.handle_command(PlayerCommand::PlaySelected);
        assert_eq!(
            state.handle_command(PlayerCommand::TogglePause),
            PlayerAction::Pause
        );
        assert_eq!(state.status, PlaybackStatus::Paused);
        assert_eq!(
            state.handle_command(PlayerCommand::TogglePause),
            PlayerAction::Resume
        );
        assert_eq!(state.status, PlaybackStatus::Playing);
    }

    #[test]
    fn volume_bounds_and_step() {
        let mut state = PlayerState::new(sample_tracks());
        assert_eq!(state.volume, 100);
        state.handle_command(PlayerCommand::VolumeUp);
        assert_eq!(state.volume, 100);
        for _ in 0..30 {
            state.handle_command(PlayerCommand::VolumeDown);
        }
        assert_eq!(state.volume, 0);
        state.handle_command(PlayerCommand::VolumeUp);
        assert_eq!(state.volume, 5);
    }

    #[test]
    fn quit_action() {
        let mut state = PlayerState::new(sample_tracks());
        assert_eq!(state.handle_command(PlayerCommand::Quit), PlayerAction::Quit);
    }

    #[test]
    fn empty_tracks_navigation_safe() {
        let mut state = PlayerState::new(Vec::new());
        state.handle_command(PlayerCommand::Up);
        state.handle_command(PlayerCommand::Down);
        assert_eq!(state.handle_command(PlayerCommand::PlaySelected), PlayerAction::None);
        assert_eq!(state.selected, 0);
        assert!(state.current_track().is_none());
    }
}
