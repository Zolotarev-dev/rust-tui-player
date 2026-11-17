use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use first_test::player::{PlaybackStatus, PlayerAction, PlayerCommand, PlayerState};
use first_test::sanitize::sanitize_for_display;
use first_test::tracks::{discover_tracks, Track};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("first-test-{prefix}-{pid}-{nanos}"))
}

fn create_file(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(path, b"").ok();
}

// --- Sanitization tests ---

#[test]
fn sanitization_strips_ansi_injection_sequences() {
    let injected = "song\x1b[31;1mRED\x1b[0m\x1b]0;evil-title\x07name.mp3";
    let clean = sanitize_for_display(injected);
    assert_eq!(clean, "songREDname.mp3");
    assert!(!clean.contains('\x1b'));
    assert!(!clean.contains('\x07'));
}

#[test]
fn sanitization_strips_ascii_control_characters() {
    let input = "a\x00b\x01c\x1fd\x7fe\nf\rg\th";
    let clean = sanitize_for_display(input);
    assert_eq!(clean, "abcdefgh");
    for ch in clean.chars() {
        assert!(ch > '\x1f' && ch != '\x7f');
    }
}

#[test]
fn sanitization_preserves_normal_text() {
    let input = "Track 01 - Música.flac";
    assert_eq!(sanitize_for_display(input), input);
}

// --- Extension filtering tests ---

#[test]
fn discover_tracks_filters_supported_extensions_recursively() {
    let root = unique_temp_dir("ext");
    fs::create_dir_all(root.join("sub")).ok();
    create_file(&root.join("a.mp3"));
    create_file(&root.join("b.FLAC"));
    create_file(&root.join("c.ogg"));
    create_file(&root.join("d.wav"));
    create_file(&root.join("sub/e.Mp3"));
    create_file(&root.join("sub/f.txt"));
    create_file(&root.join("sub/g.m4a"));
    create_file(&root.join("noext"));

    let tracks = discover_tracks(&root).unwrap_or_else(|e| panic!("discover failed: {e}"));
    let names: Vec<String> = tracks.iter().map(|t| t.name.clone()).collect();

    assert!(names.iter().any(|n| n == "a.mp3"));
    assert!(names.iter().any(|n| n == "b.FLAC"));
    assert!(names.iter().any(|n| n == "c.ogg"));
    assert!(names.iter().any(|n| n == "d.wav"));
    assert!(names.iter().any(|n| n == "e.Mp3"));
    assert!(!names.iter().any(|n| n == "f.txt"));
    assert!(!names.iter().any(|n| n == "g.m4a"));
    assert!(!names.iter().any(|n| n == "noext"));

    let _ = fs::remove_dir_all(&root);
}

// --- Symlink safety tests ---

#[test]
fn discover_tracks_does_not_follow_symlink_outside_target_file() {
    let base = unique_temp_dir("symfile");
    let root = base.join("music");
    let outside = base.join("outside");
    fs::create_dir_all(&root).ok();
    fs::create_dir_all(&outside).ok();
    create_file(&root.join("inside.mp3"));
    create_file(&outside.join("secret.mp3"));

    // Symlink inside root pointing to file outside root -> must be ignored.
    let link_path = root.join("leak.mp3");
    symlink(outside.join("secret.mp3"), &link_path).ok();

    let tracks = discover_tracks(&root).unwrap_or_else(|e| panic!("discover failed: {e}"));
    let names: Vec<String> = tracks.iter().map(|t| t.name.clone()).collect();

    assert!(names.iter().any(|n| n == "inside.mp3"));
    assert!(!names.iter().any(|n| n == "leak.mp3"));
    assert!(!names.iter().any(|n| n == "secret.mp3"));

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn discover_tracks_does_not_follow_symlink_outside_target_dir() {
    let base = unique_temp_dir("symdir");
    let root = base.join("music");
    let outside = base.join("outside");
    fs::create_dir_all(&root).ok();
    fs::create_dir_all(&outside).ok();
    create_file(&root.join("inside.mp3"));
    create_file(&outside.join("secret.mp3"));

    // Symlink inside root pointing to directory outside root -> must be ignored.
    let link_dir = root.join("linked_out");
    symlink(&outside, &link_dir).ok();

    let tracks = discover_tracks(&root).unwrap_or_else(|e| panic!("discover failed: {e}"));
    let names: Vec<String> = tracks.iter().map(|t| t.name.clone()).collect();

    assert!(names.iter().any(|n| n == "inside.mp3"));
    assert!(!names.iter().any(|n| n == "secret.mp3"));

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn discover_tracks_handles_symlink_loops_without_hanging() {
    let root = unique_temp_dir("loop");
    let sub = root.join("sub");
    fs::create_dir_all(&sub).ok();
    create_file(&sub.join("song.mp3"));

    // Create a directory symlink loop: sub/loop -> root
    let loop_link = sub.join("loop");
    symlink(&root, &loop_link).ok();

    // Also a self-loop symlink for good measure.
    let self_link = root.join("self");
    symlink(&root, &self_link).ok();

    let tracks = discover_tracks(&root).unwrap_or_else(|e| panic!("discover failed: {e}"));
    let names: Vec<String> = tracks.iter().map(|t| t.name.clone()).collect();

    // Should find the song exactly once and not hang / recurse infinitely.
    let song_count = names.iter().filter(|n| n.as_str() == "song.mp3").count();
    assert_eq!(song_count, 1);

    let _ = fs::remove_dir_all(&root);
}

// --- Player state machine / command handling tests ---

fn make_state() -> PlayerState {
    PlayerState::new(vec![
        Track {
            path: PathBuf::from("/m/1.mp3"),
            name: "1.mp3".to_string(),
        },
        Track {
            path: PathBuf::from("/m/2.mp3"),
            name: "2.mp3".to_string(),
        },
    ])
}

#[test]
fn player_command_handling_navigation_and_play() {
    let mut state = make_state();
    assert_eq!(state.selected, 0);
    assert_eq!(state.handle_command(PlayerCommand::Down), PlayerAction::None);
    assert_eq!(state.selected, 1);
    let action = state.handle_command(PlayerCommand::PlaySelected);
    assert_eq!(action, PlayerAction::Play(PathBuf::from("/m/2.mp3")));
    assert_eq!(state.status, PlaybackStatus::Playing);
    assert_eq!(state.current_index, Some(1));
}

#[test]
fn player_command_handling_pause_resume_and_volume() {
    let mut state = make_state();
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

    assert_eq!(
        state.handle_command(PlayerCommand::VolumeDown),
        PlayerAction::SetVolume(95)
    );
    assert_eq!(state.volume, 95);
    assert_eq!(
        state.handle_command(PlayerCommand::VolumeUp),
        PlayerAction::SetVolume(100)
    );
    assert_eq!(state.volume, 100);
}

#[test]
fn player_command_handling_quit_and_stop_events() {
    let mut state = make_state();
    assert_eq!(state.handle_command(PlayerCommand::Quit), PlayerAction::Quit);
    state.handle_command(PlayerCommand::PlaySelected);
    assert_eq!(state.status, PlaybackStatus::Playing);
    state.on_audio_finished();
    assert_eq!(state.status, PlaybackStatus::Stopped);
    state.status = PlaybackStatus::Playing;
    state.on_audio_stopped();
    assert_eq!(state.status, PlaybackStatus::Stopped);
}
