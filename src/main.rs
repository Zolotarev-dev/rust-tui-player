use std::env;
use std::path::PathBuf;
use std::process;

use rust_tui_player::audio::{spawn_audio_engine, AudioCommand};
use rust_tui_player::player::PlayerState;
use rust_tui_player::tracks::discover_tracks;
use rust_tui_player::ui;

fn main() {
    let root = parse_root_arg();
    let tracks = match discover_tracks(&root) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error discovering tracks in '{}': {e}", root.display());
            process::exit(1);
        }
    };

    let mut state = PlayerState::new(tracks);
    let audio = spawn_audio_engine();

    // Set initial volume on the audio engine.
    let _ = audio
        .cmd_tx
        .send(AudioCommand::SetVolume(state.volume_as_f32()));

    if let Err(e) = ui::run(&mut state, &audio) {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}

fn parse_root_arg() -> PathBuf {
    let mut args = env::args_os();
    let _program = args.next();
    match args.next() {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from("./music"),
    }
}
