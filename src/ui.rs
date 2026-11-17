//! TUI rendering and input handling using ratatui + crossterm.

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;

use crate::audio::{AudioCommand, AudioEvent, AudioHandle};
use crate::player::{PlaybackStatus, PlayerAction, PlayerCommand, PlayerState};
use crate::sanitize::sanitize_for_display;

/// UI-only state for displaying transient information like last audio error.
#[derive(Debug, Default, Clone)]
struct UiState {
    last_error: Option<String>,
}

/// Run the main TUI loop. Terminal is restored on exit and on errors/panic as
/// much as reasonably possible via a panic hook and scopeguard-like cleanup.
pub fn run(state: &mut PlayerState, audio: &AudioHandle) -> Result<(), String> {
    let mut terminal = setup_terminal().map_err(|e| format!("Failed to setup terminal: {e}"))?;

    // Install a panic hook that attempts to restore the terminal.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal_raw();
        original_hook(info);
    }));

    let result = run_loop(&mut terminal, state, audio);

    // Restore panic hook and terminal.
    let _ = std::panic::take_hook();
    let restore_result = restore_terminal(&mut terminal);

    // Prefer returning the loop error if any; otherwise the restore error.
    match (result, restore_result) {
        (Err(e), _) => Err(e),
        (Ok(()), Err(e)) => Err(format!("Failed to restore terminal: {e}")),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Best-effort raw restore for panic hook (no terminal handle available).
fn restore_terminal_raw() -> io::Result<()> {
    disable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen)?;
    // show cursor via crossterm
    execute!(stdout, crossterm::cursor::Show)?;
    Ok(())
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut PlayerState,
    audio: &AudioHandle,
) -> Result<(), String> {
    let mut list_state = ListState::default();
    let mut ui_state = UiState::default();
    if !state.tracks.is_empty() {
        list_state.select(Some(state.selected));
    }

    loop {
        // Drain audio events (non-blocking) to keep state in sync.
        while let Ok(ev) = audio.event_rx.try_recv() {
            apply_audio_event(state, &mut ui_state, ev);
        }

        list_state.select(if state.tracks.is_empty() {
            None
        } else {
            Some(state.selected.min(state.tracks.len().saturating_sub(1)))
        });

        terminal
            .draw(|f| draw_ui(f, state, &ui_state, &mut list_state))
            .map_err(|e| format!("Draw error: {e}"))?;

        if event::poll(Duration::from_millis(100)).map_err(|e| format!("Event poll error: {e}"))? {
            if let Event::Key(key) =
                event::read().map_err(|e| format!("Event read error: {e}"))?
            {
                if let Some(cmd) = key_to_command(key) {
                    let action = state.handle_command(cmd);
                    if handle_action(action, audio, &mut ui_state)? {
                        return Ok(());
                    }
                }
            }
        }
    }
}

fn key_to_command(key: KeyEvent) -> Option<PlayerCommand> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(PlayerCommand::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(PlayerCommand::Down),
        KeyCode::Enter => Some(PlayerCommand::PlaySelected),
        KeyCode::Char(' ') => Some(PlayerCommand::TogglePause),
        KeyCode::Char('+') | KeyCode::Char('=') => Some(PlayerCommand::VolumeUp),
        KeyCode::Char('-') | KeyCode::Char('_') => Some(PlayerCommand::VolumeDown),
        KeyCode::Char('q') | KeyCode::Char('Q') => Some(PlayerCommand::Quit),
        _ => None,
    }
}

fn handle_action(
    action: PlayerAction,
    audio: &AudioHandle,
    ui_state: &mut UiState,
) -> Result<bool, String> {
    match action {
        PlayerAction::None => Ok(false),
        PlayerAction::Play(path) => {
            // Clear previous error on explicit new play attempt.
            ui_state.last_error = None;
            audio
                .cmd_tx
                .send(AudioCommand::Play(path))
                .map_err(|e| format!("Failed to send play command: {e}"))?;
            Ok(false)
        }
        PlayerAction::Pause => {
            audio
                .cmd_tx
                .send(AudioCommand::Pause)
                .map_err(|e| format!("Failed to send pause command: {e}"))?;
            Ok(false)
        }
        PlayerAction::Resume => {
            audio
                .cmd_tx
                .send(AudioCommand::Resume)
                .map_err(|e| format!("Failed to send resume command: {e}"))?;
            Ok(false)
        }
        PlayerAction::SetVolume(v) => {
            audio
                .cmd_tx
                .send(AudioCommand::SetVolume((v as f32) / 100.0))
                .map_err(|e| format!("Failed to send volume command: {e}"))?;
            Ok(false)
        }
        PlayerAction::Quit => {
            // Best-effort shutdown of audio thread.
            let _ = audio.cmd_tx.send(AudioCommand::Shutdown);
            Ok(true)
        }
    }
}

fn apply_audio_event(state: &mut PlayerState, ui_state: &mut UiState, ev: AudioEvent) {
    match ev {
        AudioEvent::NowPlaying(_) => {
            state.status = PlaybackStatus::Playing;
            // Clear stale error on successful playback start.
            ui_state.last_error = None;
        }
        AudioEvent::Paused => state.on_audio_paused(),
        AudioEvent::Resumed => state.on_audio_resumed(),
        AudioEvent::Stopped => state.on_audio_stopped(),
        AudioEvent::VolumeChanged(v) => {
            let pct = (v * 100.0).round().clamp(0.0, 100.0) as u8;
            state.on_volume_changed(pct);
        }
        AudioEvent::Error(msg) => {
            // Store sanitized error for explicit display; keep UI responsive.
            ui_state.last_error = Some(sanitize_for_display(&msg));
        }
        AudioEvent::Finished => state.on_audio_finished(),
    }
}

fn draw_ui(
    f: &mut ratatui::Frame,
    state: &PlayerState,
    ui_state: &UiState,
    list_state: &mut ListState,
) {
    let (name, status_str) = match state.current_track() {
        Some(t) => (sanitize_for_display(&t.name), status_label(state.status)),
        None => ("<none>".to_string(), status_label(state.status)),
    };

    let mut status_text = format!(
        "Track: {} | Status: {} | Volume: {}% | Keys: ↑/↓ j/k Enter Space +/- q",
        name, status_str, state.volume
    );
    if let Some(err) = &ui_state.last_error {
        // Error is already sanitized when stored, but sanitize again defensively before render.
        let safe_err = sanitize_for_display(err);
        status_text.push_str(&format!(" | Error: {safe_err}"));
    }
    let status_text = sanitize_for_display(&status_text);

    // The block borders consume 2 columns and 2 rows; compute the wrapped line count
    // manually so the Status block always fits exactly (no private API, no clipping).
    let inner_width = f.area().width.saturating_sub(2).max(1);
    let mut lines = 1usize;
    let mut line_len = 0usize;
    for ch in status_text.chars() {
        if ch == '\n' {
            lines += 1;
            line_len = 0;
        } else {
            line_len += 1;
            if line_len > inner_width as usize {
                lines += 1;
                line_len = 0;
            }
        }
    }
    // +2 for the top and bottom borders of Borders::ALL; cap so the block never eats the screen.
    let status_height = (lines + 2) as u16;
    let status_height = status_height.min(f.area().height.saturating_sub(1).max(1));
    let list_height = f.area().height.saturating_sub(status_height).max(1);

    // The Status block must stay visible in full on narrow terminals: wrap its text and give
    // the block exactly the height it needs instead of a fixed 4 rows, otherwise the key
    // hints are silently clipped at the bottom of the window.
    let status = Paragraph::new(status_text)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("Status"));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(list_height), Constraint::Length(status_height)])
        .split(f.area());

    let items: Vec<ListItem> = state
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let marker = if state.current_index == Some(i) { "▶ " } else { "  " };
            let name = sanitize_for_display(&t.name);
            ListItem::new(Line::from(Span::raw(format!("{marker}{name}"))))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Tracks"))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");

    f.render_stateful_widget(list, chunks[0], list_state);
    f.render_widget(status, chunks[1]);
}

fn status_label(status: PlaybackStatus) -> &'static str {
    match status {
        PlaybackStatus::Stopped => "Stopped",
        PlaybackStatus::Playing => "Playing",
        PlaybackStatus::Paused => "Paused",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracks::Track;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    fn sample_state() -> PlayerState {
        PlayerState::new(vec![Track {
            path: PathBuf::from("/music/song.mp3"),
            name: "song.mp3".to_string(),
        }])
    }

    /// Renders the UI in a narrow terminal and asserts the key hints are not clipped.
    #[test]
    fn narrow_window_keeps_status_hints_visible() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).expect("terminal init");
        let state = sample_state();
        let mut list_state = ListState::default();
        let ui_state = UiState::default();

        terminal
            .draw(|f| draw_ui(f, &state, &ui_state, &mut list_state))
            .expect("draw");

        let buffer = terminal.backend_mut().buffer();
        let text: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol().to_string())
            .collect();

        // The key hints must survive as whole words, not be truncated at the right border.
        assert!(text.contains("Enter"), "Enter hint missing from narrow render");
        assert!(text.contains("Space"), "Space hint missing from narrow render");
        assert!(text.contains("q"), "Quit hint missing from narrow render");
        assert!(text.contains("Status"), "Status block title missing");
        assert!(text.contains("Tracks"), "Tracks block title missing");
    }
}
