//! Audio engine running in a separate thread, communicating via mpsc channels.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use rodio::{Decoder, OutputStream, Sink};

/// Commands sent from the UI thread to the audio thread.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioCommand {
    Play(PathBuf),
    Pause,
    Resume,
    SetVolume(f32), // 0.0 ..= 1.0
    Stop,
    Shutdown,
}

/// Events sent from the audio thread back to the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioEvent {
    NowPlaying(PathBuf),
    Paused,
    Resumed,
    Stopped,
    VolumeChanged(f32),
    Error(String),
    Finished,
}

/// Handle owned by the UI thread to control the audio engine.
pub struct AudioHandle {
    pub cmd_tx: Sender<AudioCommand>,
    pub event_rx: Receiver<AudioEvent>,
}

/// Spawn the audio engine thread and return a handle for communication.
pub fn spawn_audio_engine() -> AudioHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel::<AudioCommand>();
    let (event_tx, event_rx) = mpsc::channel::<AudioEvent>();

    thread::spawn(move || audio_thread_main(cmd_rx, event_tx));

    AudioHandle { cmd_tx, event_rx }
}

fn audio_thread_main(cmd_rx: Receiver<AudioCommand>, event_tx: Sender<AudioEvent>) {
    // Try to initialize output stream; if it fails (e.g. no audio device),
    // keep running so commands don't block the UI, but report errors.
    let mut output: Option<(OutputStream, rodio::OutputStreamHandle)> = OutputStream::try_default().ok();
    let mut sink: Option<Sink> = None;
    let mut volume: f32 = 1.0;

    loop {
        // Detect natural end of track.
        if let Some(ref s) = sink {
            if s.empty() {
                // Only emit Finished once per track end.
                sink = None;
                let _ = event_tx.send(AudioEvent::Finished);
            }
        }

        match cmd_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(AudioCommand::Play(path)) => {
                // Stop previous sink if any.
                if let Some(s) = sink.take() {
                    s.stop();
                }

                let stream_handle = match ensure_output(&mut output) {
                    Ok(handle) => handle,
                    Err(e) => {
                        let _ = event_tx.send(AudioEvent::Error(e));
                        continue;
                    }
                };

                match Sink::try_new(stream_handle) {
                    Ok(new_sink) => match open_source(&path) {
                        Ok(source) => {
                            new_sink.set_volume(volume);
                            new_sink.append(source);
                            new_sink.play();
                            let _ = event_tx.send(AudioEvent::NowPlaying(path));
                            sink = Some(new_sink);
                        }
                        Err(e) => {
                            let _ = event_tx.send(AudioEvent::Error(e));
                        }
                    },
                    Err(e) => {
                        let _ = event_tx.send(AudioEvent::Error(format!(
                            "Failed to create audio sink: {e}"
                        )));
                    }
                }
            }
            Ok(AudioCommand::Pause) => {
                if let Some(ref s) = sink {
                    s.pause();
                    let _ = event_tx.send(AudioEvent::Paused);
                }
            }
            Ok(AudioCommand::Resume) => {
                if let Some(ref s) = sink {
                    s.play();
                    let _ = event_tx.send(AudioEvent::Resumed);
                }
            }
            Ok(AudioCommand::SetVolume(v)) => {
                volume = clamp_volume(v);
                if let Some(ref s) = sink {
                    s.set_volume(volume);
                }
                let _ = event_tx.send(AudioEvent::VolumeChanged(volume));
            }
            Ok(AudioCommand::Stop) => {
                if let Some(s) = sink.take() {
                    s.stop();
                }
                let _ = event_tx.send(AudioEvent::Stopped);
            }
            Ok(AudioCommand::Shutdown) => {
                if let Some(s) = sink.take() {
                    s.stop();
                }
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // loop to check sink.empty() / keep responsive
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(s) = sink.take() {
                    s.stop();
                }
                break;
            }
        }
    }
}

fn ensure_output(
    output: &mut Option<(OutputStream, rodio::OutputStreamHandle)>,
) -> Result<&rodio::OutputStreamHandle, String> {
    if output.is_none() {
        *output = OutputStream::try_default().ok();
    }
    output
        .as_ref()
        .map(|(_, handle)| handle)
        .ok_or_else(|| "No audio output device available".to_string())
}

fn open_source(path: &PathBuf) -> Result<Decoder<BufReader<File>>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open '{}': {e}", path.display()))?;
    let reader = BufReader::new(file);
    Decoder::new(reader).map_err(|e| format!("Failed to decode '{}': {e}", path.display()))
}

/// Clamp volume into the allowed range [0.0, 1.0].
pub fn clamp_volume(v: f32) -> f32 {
    if v.is_nan() {
        1.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::clamp_volume;

    #[test]
    fn clamp_volume_bounds() {
        assert_eq!(clamp_volume(-1.0), 0.0);
        assert_eq!(clamp_volume(0.0), 0.0);
        assert_eq!(clamp_volume(0.5), 0.5);
        assert_eq!(clamp_volume(1.0), 1.0);
        assert_eq!(clamp_volume(2.0), 1.0);
    }

    #[test]
    fn clamp_volume_nan_defaults_to_one() {
        assert_eq!(clamp_volume(f32::NAN), 1.0);
    }
}
