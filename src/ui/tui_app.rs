//! Terminal UI — bluetui-inspired design.
//!
//! Layout: vertical column with margen 1, panels separated by blank lines.
//! Each panel is a bordered Table with a title. Active panel has Thick
//! green borders; inactive panels have plain borders.
//!
//! Navigation (vim-style):
//!   h/← / l/→  — change panel (left/right)
//!   j/↓ / k/↑  — move selection within panel (wrap-around)
//!   Tab        — next panel
//!   Shift+Tab  — prev panel
//!   Enter      — cycle value / activate
//!   g / G      — top / bottom of list
//!   ?          — toggle help overlay
//!   r          — start recording
//!   s          — stop recording
//!   p          — pause/resume
//!   q/Ctrl+c   — quit

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Terminal;

use crate::audio::{detect_backend, list_sources, AudioSource};
use crate::config::{AudioFormat, RecordingConfig, ResourceTier, VideoFormat, VideoQuality};
use crate::errors::{AppError, AppResult};
use crate::presets::{self, Preset};
use crate::recorder::{RecorderController, RecorderHandle, RecorderStatus};
use crate::shared;
use crate::wayland::{self, SessionKind, WlrOutput};

type Tui = Terminal<CrosstermBackend<Stdout>>;

// ---------------------------------------------------------------------------
// Style helpers — bluetui palette
// ---------------------------------------------------------------------------

const GREEN: Color = Color::Green;
const YELLOW: Color = Color::Yellow;
const RED: Color = Color::Red;
const DARK_GRAY: Color = Color::DarkGray;
const WHITE: Color = Color::White;
const GRAY: Color = Color::Gray;
const BLUE: Color = Color::Blue;

fn active_border(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN))
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(GREEN).add_modifier(Modifier::BOLD))
}

fn inactive_border(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", title))
}

fn selected_style() -> Style {
    Style::default().bg(DARK_GRAY).fg(WHITE).add_modifier(Modifier::BOLD)
}



// ---------------------------------------------------------------------------
// Panels (vim-style focus management)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Panel {
    Profile,
    Output,
    Video,
    Audio,
    Input,
    Record,
}

impl Panel {
    const ALL: &'static [Panel] = &[
        Panel::Profile,
        Panel::Output,
        Panel::Video,
        Panel::Audio,
        Panel::Input,
        Panel::Record,
    ];

    fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        if idx == 0 {
            *Self::ALL.last().unwrap()
        } else {
            Self::ALL[idx - 1]
        }
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct AppState {
    config: RecordingConfig,
    tier: ResourceTier,
    presets: Vec<Preset>,
    outputs: Vec<WlrOutput>,
    audio_sources: Vec<AudioSource>,
    #[allow(dead_code)]
    session: SessionKind,
    handle: Option<RecorderHandle>,
    paused: bool,
    geometry: String,
    focused: Panel,
    // Selection indices within each panel
    sel_preset: usize,
    sel_output: usize,
    sel_audio: usize,
    sel_quality: usize,
    sel_video_fmt: usize,
    sel_audio_fmt: usize,
    sel_tier: usize,
    // Recording
    recording_started: Option<Instant>,
    countdown_ends_at: Option<Instant>,
    // Help overlay
    show_help: bool,
    log_lines: Vec<String>,
    panel_areas: std::collections::HashMap<Panel, Rect>,
    /// When Some, the TUI is in text input mode (editing output path).
    /// Keystrokes go to the input buffer instead of normal navigation.
    input_mode: Option<InputState>,
}

/// State for text input popups (e.g. editing the output path).
struct InputState {
    buffer: String,
    label: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run() -> AppResult<()> {
    enable_raw_mode().map_err(AppError::Io)?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).map_err(AppError::Io)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(AppError::Io)?;
    terminal.hide_cursor().ok();

    let result = main_loop(&mut terminal);

    disable_raw_mode().ok();
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture).ok();
    result
}

fn main_loop(terminal: &mut Tui) -> AppResult<()> {
    let app_config = crate::config::AppConfig::load_or_default();
    let session = wayland::detect_session();
    let outputs = if session.is_wlroots() {
        wayland::list_outputs().unwrap_or_default()
    } else {
        Vec::new()
    };
    let audio_sources = list_sources();
    let presets = presets::for_tier(app_config.tier_filter);

    let mut state = AppState {
        config: app_config.recording.clone(),
        tier: app_config.tier_filter,
        presets,
        outputs,
        audio_sources,
        session,
        handle: None,
        paused: false,
        geometry: String::new(),
        focused: Panel::Profile,
        sel_preset: 0,
        sel_output: 0,
        sel_audio: 0,
        sel_quality: 0,
        sel_video_fmt: 0,
        sel_audio_fmt: 0,
        sel_tier: 0,
        recording_started: None,
        countdown_ends_at: None,
        show_help: false,
        log_lines: Vec::new(),
        panel_areas: std::collections::HashMap::new(),
        input_mode: None,
    };

    // Initialize selection indices from config
    state.sel_quality = VideoQuality::ALL.iter().position(|q| *q == state.config.quality).unwrap_or(0);
    state.sel_video_fmt = VideoFormat::ALL.iter().position(|f| *f == state.config.video_format).unwrap_or(0);
    state.sel_audio_fmt = AudioFormat::ALL.iter().position(|f| *f == state.config.audio_format).unwrap_or(0);
    state.sel_tier = ResourceTier::ALL.iter().position(|t| *t == state.tier).unwrap_or(1);

    let poll_timeout = Duration::from_millis(250);

    loop {
        terminal.draw(|f| draw(f, &mut state))?;

        if event::poll(poll_timeout).map_err(AppError::Io)? {
            let ev = event::read().map_err(AppError::Io)?;
            match ev {
                Event::Key(key) => {
                    if handle_key(key, &mut state) {
                        break;
                    }
                }
                Event::Mouse(mouse_event) => {
                    handle_mouse(mouse_event, &mut state);
                }
                _ => {}
            }
        }

        // Tick: update notifications TTL, countdown, recorder poll
        tick(&mut state);
    }

    // Save config on exit
    let mut config_to_save = app_config;
    config_to_save.recording = state.config.clone();
    if let Err(e) = config_to_save.save() {
        log::warn!("failed to save config on exit: {e}");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Key handler — vim-style
// ---------------------------------------------------------------------------

/// Handle keys when in text input mode (editing output path).
/// Enter confirms, Esc cancels, Backspace deletes, other chars are appended.
fn handle_input_key(key: KeyEvent, state: &mut AppState) -> bool {
    let input = state.input_mode.as_mut().unwrap();
    match key.code {
        KeyCode::Enter => {
            // Confirm: apply the buffer to config
            let path = std::path::PathBuf::from(&input.buffer);
            state.config.output_dir = path;
            state.input_mode = None;
        }
        KeyCode::Esc => {
            // Cancel
            state.input_mode = None;
        }
        KeyCode::Backspace => {
            input.buffer.pop();
        }
        KeyCode::Char(c) => {
            input.buffer.push(c);
        }
        _ => {}
    }
    false
}

fn handle_mouse(mouse: crossterm::event::MouseEvent, state: &mut AppState) {
    use crossterm::event::{MouseEventKind, MouseButton};
    let x = mouse.column;
    let y = mouse.row;

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // Find which panel contains the click coordinates.
            for (panel, area) in &state.panel_areas {
                if x >= area.x && x < area.x + area.width
                    && y >= area.y && y < area.y + area.height
                {
                    state.focused = *panel;
                    return;
                }
            }
        }
        MouseEventKind::ScrollDown => {
            let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
            handle_key(key, state);
        }
        MouseEventKind::ScrollUp => {
            let key = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
            handle_key(key, state);
        }
        _ => {}
    }
}

fn handle_key(key: KeyEvent, state: &mut AppState) -> bool {
    // If in input mode, all keys go to the text buffer
    if state.input_mode.is_some() {
        return handle_input_key(key, state);
    }

    // Global keys (work in all panels)
    match key.code {
        KeyCode::Char('q') => return true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Char('?') => {
            state.show_help = !state.show_help;
            return false;
        }
        KeyCode::Esc => {
            state.show_help = false;
            return false;
        }
        _ => {}
    }

    // Help overlay: only Esc or ? closes it (handled above)
    if state.show_help {
        return false;
    }

    // Recording control keys (global)
    match key.code {
        KeyCode::Char('r') if key.modifiers == KeyModifiers::NONE => {
            start_recording(state);
            return false;
        }
        KeyCode::Char('s') if key.modifiers == KeyModifiers::NONE => {
            if let Some(h) = state.handle.as_mut() {
                let _ = h.stop();
            }
            return false;
        }
        KeyCode::Char('p') if key.modifiers == KeyModifiers::NONE => {
            toggle_pause(state);
            return false;
        }
        _ => {}
    }

    // Panel navigation (vim-style: h/l = prev/next panel, Tab = next)
    match key.code {
        KeyCode::Tab | KeyCode::Char('l') if key.modifiers == KeyModifiers::NONE => {
            state.focused = state.focused.next();
            return false;
        }
        KeyCode::BackTab | KeyCode::Char('h') if key.modifiers == KeyModifiers::NONE => {
            state.focused = state.focused.prev();
            return false;
        }
        _ => {}
    }

    // Panel-specific handlers
    match state.focused {
        Panel::Profile => handle_profile_key(key, state),
        Panel::Output => handle_output_key(key, state),
        Panel::Video => handle_video_key(key, state),
        Panel::Audio => handle_audio_key(key, state),
        Panel::Input => handle_input_panel_key(key, state),
        Panel::Record => handle_record_key(key, state),
    }

    false
}

fn handle_record_key(key: KeyEvent, state: &mut AppState) {
    match key.code {
        KeyCode::Enter => {
            start_recording(state);
        }
        KeyCode::Char(' ') => {
            toggle_pause(state);
        }
        _ => {}
    }
}

fn handle_profile_key(key: KeyEvent, state: &mut AppState) {
    let n = state.presets.len().max(1);
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            state.sel_preset = (state.sel_preset + 1) % (n + 1);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.sel_preset = if state.sel_preset == 0 { n } else { state.sel_preset - 1 };
        }
        KeyCode::Char('g') => state.sel_preset = 0,
        KeyCode::Char('G') => state.sel_preset = n,
        KeyCode::Enter => {
            if state.sel_preset == 0 {
            } else if let Some(p) = state.presets.get(state.sel_preset - 1) {
                shared::apply_preset(&mut state.config, p);
                // Sync selection indices
                state.sel_quality = VideoQuality::ALL.iter().position(|q| *q == state.config.quality).unwrap_or(0);
                state.sel_video_fmt = VideoFormat::ALL.iter().position(|f| *f == state.config.video_format).unwrap_or(0);
                state.sel_audio_fmt = AudioFormat::ALL.iter().position(|f| *f == state.config.audio_format).unwrap_or(0);
            }
        }
        KeyCode::Char('t') => {
            // Cycle tier
            state.sel_tier = (state.sel_tier + 1) % ResourceTier::ALL.len();
            state.tier = ResourceTier::ALL[state.sel_tier];
            state.presets = presets::for_tier(state.tier);
            state.sel_preset = 0;
        }
        _ => {}
    }
}

fn handle_output_key(key: KeyEvent, state: &mut AppState) {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            state.sel_video_fmt = (state.sel_video_fmt + 1) % VideoFormat::ALL.len();
            state.config.video_format = VideoFormat::ALL[state.sel_video_fmt];
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.sel_video_fmt = if state.sel_video_fmt == 0 { VideoFormat::ALL.len() - 1 } else { state.sel_video_fmt - 1 };
            state.config.video_format = VideoFormat::ALL[state.sel_video_fmt];
        }
        KeyCode::Char('f') => {
            state.sel_video_fmt = (state.sel_video_fmt + 1) % VideoFormat::ALL.len();
            state.config.video_format = VideoFormat::ALL[state.sel_video_fmt];
        }
        KeyCode::Char('o') => {
            // Open text input for editing the output path
            state.input_mode = Some(InputState {
                buffer: state.config.output_dir.to_string_lossy().to_string(),
                label: "Ruta de guardado".to_string(),
            });
        }
        _ => {}
    }
}

fn handle_video_key(key: KeyEvent, state: &mut AppState) {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            state.sel_quality = (state.sel_quality + 1) % VideoQuality::ALL.len();
            state.config.quality = VideoQuality::ALL[state.sel_quality];
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.sel_quality = if state.sel_quality == 0 { VideoQuality::ALL.len() - 1 } else { state.sel_quality - 1 };
            state.config.quality = VideoQuality::ALL[state.sel_quality];
        }
        KeyCode::Left | KeyCode::Char('h') if state.focused == Panel::Video => {
            // Decrease FPS
            let v = state.config.fps as i32 - 1;
            state.config.fps = v.clamp(15, 144) as u32;
        }
        KeyCode::Right | KeyCode::Char('l') if state.focused == Panel::Video => {
            // Increase FPS
            let v = state.config.fps as i32 + 1;
            state.config.fps = v.clamp(15, 144) as u32;
        }
        KeyCode::Char('d') => {
            state.config.use_dmabuf = !state.config.use_dmabuf;
        }
        KeyCode::Char('c') => {
            state.config.record_cursor = !state.config.record_cursor;
        }
        KeyCode::Char('t') => {
            // Cycle countdown: 0 → 3 → 5 → 10 → 0
            state.config.countdown_secs = match state.config.countdown_secs {
                0 => 3,
                3 => 5,
                5 => 10,
                _ => 0,
            };
        }
        _ => {}
    }
}

fn handle_audio_key(key: KeyEvent, state: &mut AppState) {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            state.sel_audio_fmt = (state.sel_audio_fmt + 1) % AudioFormat::ALL.len();
            state.config.audio_format = AudioFormat::ALL[state.sel_audio_fmt];
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.sel_audio_fmt = if state.sel_audio_fmt == 0 { AudioFormat::ALL.len() - 1 } else { state.sel_audio_fmt - 1 };
            state.config.audio_format = AudioFormat::ALL[state.sel_audio_fmt];
        }
        KeyCode::Char('a') => {
            // Cycle audio source
            let n = state.audio_sources.len() + 1;
            state.sel_audio = (state.sel_audio + 1) % n;
            if state.sel_audio == 0 {
                state.config.audio_source.clear();
                state.config.audio_source_desc.clear();
            } else if let Some(a) = state.audio_sources.get(state.sel_audio - 1) {
                state.config.audio_source = a.name.clone();
                state.config.audio_source_desc = a.description.clone();
            }
        }
        _ => {}
    }
}

fn handle_input_panel_key(key: KeyEvent, state: &mut AppState) {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            let n = state.outputs.len() + 1;
            state.sel_output = (state.sel_output + 1) % n;
            if state.sel_output == 0 {
                state.config.output.clear();
            } else if let Some(o) = state.outputs.get(state.sel_output - 1) {
                state.config.output = o.name.clone();
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let n = state.outputs.len() + 1;
            state.sel_output = if state.sel_output == 0 { n - 1 } else { state.sel_output - 1 };
            if state.sel_output == 0 {
                state.config.output.clear();
            } else if let Some(o) = state.outputs.get(state.sel_output - 1) {
                state.config.output = o.name.clone();
            }
        }
        KeyCode::Char('g') => {
            match wayland::pick_region() {
                Ok(Some(g)) => {
                    state.geometry = g.clone();
                    state.config.geometry = g;
                }
                Ok(None) => {}
                Err(e) => log::error!("slurp: {}", e),
            }
        }
        KeyCode::Char(' ') => {
            toggle_pause(state);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Recording control
// ---------------------------------------------------------------------------

fn start_recording(state: &mut AppState) {
    if state.handle.is_some() {
        return;
    }
    state.config.geometry = state.geometry.clone();
    let countdown = state.config.countdown_secs;
    if countdown > 0 {
        state.countdown_ends_at = Some(Instant::now() + Duration::from_secs(countdown as u64));
        return;
    }
    spawn_recorder(state);
}

fn spawn_recorder(state: &mut AppState) {
    match RecorderController::start(&state.config) {
        Ok(h) => {
            state.handle = Some(h);
            state.recording_started = Some(Instant::now());
            state.log_lines.clear();
        }
        Err(e) => {
            log::error!("{}", e.to_string());
        }
    }
}

fn toggle_pause(state: &mut AppState) {
    let pid_opt = state.handle.as_ref().and_then(|h| {
        h.child.lock().ok().and_then(|guard| {
            guard.as_ref().map(|child| child.id() as i32)
        })
    });

    if let Some(pid) = pid_opt {
        let sig = if state.paused { 18 } else { 19 };
        unsafe {
            extern "C" { fn kill(pid: i32, sig: i32) -> i32; }
            let _ = kill(pid, sig);
        }
        state.paused = !state.paused;
    }
}

// ---------------------------------------------------------------------------
// Tick — update timers, poll recorder
// ---------------------------------------------------------------------------

fn tick(state: &mut AppState) {
    // Countdown
    if let Some(end) = state.countdown_ends_at {
        let remaining = end.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            state.countdown_ends_at = None;
            spawn_recorder(state);
        }
    }

    // Poll recorder
    let new_status = if let Some(h) = state.handle.as_mut() {
        h.poll()
    } else {
        None
    };

    if let Some(s) = new_status {
        if let Some(h) = state.handle.as_ref() {
            let stderr = h.stderr();
            if !stderr.is_empty() {
                state.log_lines = stderr.to_vec();
            }
        }
        match &s {
            RecorderStatus::Idle => {
                let cfg = state.config.clone();
                let path = state.handle.as_ref().map(|h| h.output_path.clone()).unwrap_or_default();
                state.handle = None;
                state.paused = false;
                state.recording_started = None;
                if !path.as_os_str().is_empty() {
                    std::thread::spawn(move || {
                        crate::recorder::post_process(&path, &cfg);
                    });
                }
            }
            RecorderStatus::Failed { message } => {
                state.handle = None;
                state.paused = false;
                state.recording_started = None;
                log::error!("{}", message.clone());
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn draw(f: &mut ratatui::Frame, state: &mut AppState) {
    let size = f.area();

    // Clear panel_areas from previous frame
    state.panel_areas.clear();

    // Main layout: panels + help bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(size);

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(5),
            Constraint::Min(8),
        ])
        .split(panels[0]);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Min(5),
        ])
        .split(panels[1]);

    // Save panel areas for mouse click detection
    state.panel_areas.insert(Panel::Profile, left[0]);
    state.panel_areas.insert(Panel::Output, left[1]);
    state.panel_areas.insert(Panel::Video, left[2]);
    state.panel_areas.insert(Panel::Audio, right[0]);
    state.panel_areas.insert(Panel::Input, right[1]);
    state.panel_areas.insert(Panel::Record, right[2]);

    draw_profile_panel(f, state, left[0]);
    draw_output_panel(f, state, left[1]);
    draw_video_panel(f, state, left[2]);
    draw_audio_panel(f, state, right[0]);
    draw_input_panel(f, state, right[1]);
    draw_record_panel(f, state, right[2]);

    // Help bar
    draw_help_bar(f, state, chunks[1]);

    // Notifications (floating, top-right)

    // Help overlay
    if state.show_help {
        draw_help_overlay(f, size);
    }

    // Input popup (output path editor)
    if state.input_mode.is_some() {
        draw_input_popup(f, state, size);
    }
}

fn draw_profile_panel(f: &mut ratatui::Frame, state: &mut AppState, area: Rect) {
    let active = state.focused == Panel::Profile;
    let block = if active { active_border("Perfil") } else { inactive_border("Perfil") };

    let mut items: Vec<ListItem> = vec![ListItem::new("(personalizado)").style(Style::default().fg(GRAY))];
    for p in &state.presets {
        items.push(ListItem::new(p.name.clone()));
    }

    let mut list_state = ListState::default();
    list_state.select(Some(state.sel_preset));

    let list = List::new(items)
        .block(block)
        .highlight_style(selected_style())
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut list_state);
}

fn draw_output_panel(f: &mut ratatui::Frame, state: &mut AppState, area: Rect) {
    let active = state.focused == Panel::Output;
    let block = if active { active_border("Archivo") } else { inactive_border("Archivo") };

    let items: Vec<ListItem> = VideoFormat::ALL.iter().enumerate().map(|(i, fmt)| {
        let style = if i == state.sel_video_fmt {
            selected_style()
        } else {
            Style::default()
        };
        let marker = if i == state.sel_video_fmt { "▶ " } else { "  " };
        ListItem::new(format!("{}{}", marker, fmt.label())).style(style)
    }).collect();

    let mut all_items: Vec<ListItem> = Vec::new();
    all_items.push(ListItem::new(format!("  Ruta: {}", state.config.output_dir.display())).style(Style::default().fg(YELLOW)));
    all_items.push(ListItem::new(format!("  → {}", crate::config::default_filename(&state.config))).style(Style::default().fg(GRAY)));
    all_items.push(ListItem::new("").style(Style::default()));
    all_items.extend(items);

    let list = List::new(all_items).block(block);
    f.render_widget(list, area);
}

fn draw_video_panel(f: &mut ratatui::Frame, state: &mut AppState, area: Rect) {
    let active = state.focused == Panel::Video;
    let block = if active { active_border("Video") } else { inactive_border("Video") };

    let items: Vec<ListItem> = VideoQuality::ALL.iter().enumerate().map(|(i, q)| {
        let style = if i == state.sel_quality {
            selected_style()
        } else {
            Style::default()
        };
        let marker = if i == state.sel_quality { "▶ " } else { "  " };
        ListItem::new(format!("{}{}", marker, q.label())).style(style)
    }).collect();

    // Build info lines
    let mut all_items: Vec<ListItem> = Vec::new();
    all_items.push(ListItem::new(format!("  FPS: {}", state.config.fps)).style(Style::default().fg(YELLOW)));
    all_items.push(ListItem::new(format!("  CRF: {}", state.config.crf)).style(Style::default().fg(YELLOW)));
    all_items.push(ListItem::new(format!("  DMA-BUF: {}", if state.config.use_dmabuf { "\u{f00c}" } else { "\u{f00d}" })).style(Style::default().fg(YELLOW)));
    all_items.push(ListItem::new(format!("  Cursor: {}", if state.config.record_cursor { "\u{f00c}" } else { "\u{f00d}" })).style(Style::default().fg(YELLOW)));
    all_items.push(ListItem::new(format!("  Cuenta regresiva: {}s", state.config.countdown_secs)).style(Style::default().fg(YELLOW)));
    all_items.push(ListItem::new("").style(Style::default()));
    all_items.extend(items);

    let list = List::new(all_items).block(block);
    f.render_widget(list, area);
}

fn draw_audio_panel(f: &mut ratatui::Frame, state: &mut AppState, area: Rect) {
    let active = state.focused == Panel::Audio;
    let title = format!("Audio · {}", detect_backend().label());
    let block = if active { active_border(&title) } else { inactive_border(&title) };

    let mut items: Vec<ListItem> = Vec::new();

    // Audio source
    let src_display = if state.config.audio_source.is_empty() {
        "(sin audio)".to_string()
    } else {
        state.config.audio_source_desc.clone()
    };
    items.push(ListItem::new(format!("  Fuente: {}", src_display)).style(Style::default().fg(YELLOW)));

    // Audio format
    items.push(ListItem::new("").style(Style::default()));
    for (i, fmt) in AudioFormat::ALL.iter().enumerate() {
        let style = if i == state.sel_audio_fmt { selected_style() } else { Style::default() };
        let marker = if i == state.sel_audio_fmt { "▶ " } else { "  " };
        items.push(ListItem::new(format!("{}{}", marker, fmt.label())).style(style));
    }

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_input_panel(f: &mut ratatui::Frame, state: &mut AppState, area: Rect) {
    let active = state.focused == Panel::Input;
    let block = if active { active_border("Entrada") } else { inactive_border("Entrada") };

    let mut items: Vec<ListItem> = Vec::new();

    // Monitor
    let mon_display = if state.config.output.is_empty() {
        "(automático)".to_string()
    } else {
        state.config.output.clone()
    };
    items.push(ListItem::new(format!("  Monitor: {}", mon_display)).style(Style::default().fg(YELLOW)));

    // Region
    let reg_display = if state.geometry.is_empty() {
        "(pantalla completa)".to_string()
    } else {
        state.geometry.clone()
    };
    items.push(ListItem::new(format!("  Región: {}", reg_display)).style(Style::default().fg(YELLOW)));

    // Available outputs
    items.push(ListItem::new("").style(Style::default()));
    items.push(ListItem::new("  Monitores disponibles:").style(Style::default().fg(GRAY)));
    for (i, o) in state.outputs.iter().enumerate() {
        let style = if i + 1 == state.sel_output { selected_style() } else { Style::default() };
        let marker = if i + 1 == state.sel_output { "▶ " } else { "  " };
        items.push(ListItem::new(format!("{}{} · {}", marker, o.name, o.resolution_label())).style(style));
    }
    if state.outputs.is_empty() {
        items.push(ListItem::new("  (no se pudieron listar)").style(Style::default().fg(RED)));
    }

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_record_panel(f: &mut ratatui::Frame, state: &mut AppState, area: Rect) {
    let active = state.focused == Panel::Record;
    let block = if active { active_border("Grabación") } else { inactive_border("Grabación") };

    let mut items: Vec<ListItem> = Vec::new();

    // Status line
    if let Some(started) = state.recording_started {
        if state.handle.is_some() {
            let elapsed = started.elapsed();
            let secs = elapsed.as_secs();
            let timer = format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60);
            let icon = if state.paused { "\u{f04c}" } else { "\u{f111}" };
            let color = if state.paused { YELLOW } else { RED };
            items.push(ListItem::new(format!("  {} {}", icon, timer)).style(Style::default().fg(color).add_modifier(Modifier::BOLD)));

            // File size
            if let Some(h) = state.handle.as_ref() {
                if let Ok(meta) = std::fs::metadata(&h.output_path) {
                    let mb = meta.len() as f64 / 1_048_576.0;
                    let size_str = if mb >= 1.0 { format!("{:.1} MB", mb) } else { format!("{} KB", meta.len() / 1024) };
                    items.push(ListItem::new(format!("  📁 {}", size_str)).style(Style::default().fg(GRAY)));
                }
            }
            items.push(ListItem::new(format!("  {} {}", if state.paused { "Pausado" } else { "Grabando" }, if state.paused { "\u{f04c}" } else { "\u{f111}" })).style(Style::default().fg(if state.paused { YELLOW } else { RED })));
        }
    } else {
        // Countdown
        if let Some(end) = state.countdown_ends_at {
            let remaining = end.saturating_duration_since(Instant::now());
            let secs = remaining.as_secs();
            items.push(ListItem::new(format!("  ⏳ Iniciando en {}…", secs)).style(Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)));
        } else {
            items.push(ListItem::new("  ● Inactivo").style(Style::default().fg(GREEN)));
        }
    }

    items.push(ListItem::new("").style(Style::default()));

    // Action hints
    if state.handle.is_some() {
        items.push(ListItem::new("  [Enter] Detener  [Space] Pausar").style(Style::default().fg(GRAY)));
    } else {
        items.push(ListItem::new("  [Enter] Grabar  [r] Grabar").style(Style::default().fg(GRAY)));
    }

    // Output path
    if let Some(h) = state.handle.as_ref() {
        items.push(ListItem::new(format!("  → {}", h.output_path.display())).style(Style::default().fg(GRAY)));
    }

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_help_bar(f: &mut ratatui::Frame, state: &mut AppState, area: Rect) {
    let help_text = match state.focused {
        Panel::Profile => " j/k: navegar · Enter: aplicar · t: tier · Tab: siguiente · Click: enfocar · r: grabar · q: salir ",
        Panel::Output => " j/k: formato · f: ciclar · o: editar ruta · Tab: siguiente · Click: enfocar · r: grabar · q: salir ",
        Panel::Video => " j/k: calidad · h/l: FPS · d: DMA-BUF · c: cursor · t: cuenta regresiva · Tab: siguiente · r: grabar · q: salir ",
        Panel::Audio => " j/k: códec · a: fuente · Tab: siguiente · Click: enfocar · r: grabar · q: salir ",
        Panel::Input => " j/k: monitor · g: región (slurp) · Tab: siguiente · Click: enfocar · r: grabar · q: salir ",
        Panel::Record => " Enter: grabar/detener · Space: pausa · Tab: siguiente · Click: enfocar · q: salir ",
    };

    let p = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BLUE))
                .title(" Atajos ")
                .title_style(Style::default().fg(BLUE))
        )
        .style(Style::default().fg(GRAY))
        .alignment(Alignment::Center);
    f.render_widget(p, area);
}


fn draw_help_overlay(f: &mut ratatui::Frame, area: Rect) {
    let help_text = "\
wf-recorder-tui — Atajos\n\
\n\
Navegación:\n\
  h / ←        Panel anterior\n\
  l / → / Tab  Siguiente panel\n\
  j / ↓        Bajar selección (wrap)\n\
  k / ↑        Subir selección (wrap)\n\
  g            Inicio de lista\n\
  G            Fin de lista\n\
  Click        Enfocar panel con el ratón\n\
  Scroll       Mover selección con la rueda\n\
\n\
Grabación:\n\
  r            Iniciar grabación\n\
  s            Detener grabación\n\
  p / Space    Pausar / reanudar\n\
  Enter        Acción contextual (aplicar / grabar / detener)\n\
\n\
Perfil:\n\
  t            Cambiar tier (bajos / medios / altos)\n\
\n\
Archivo:\n\
  f            Cambiar formato de video\n\
  o            Editar ruta de guardado (Enter confirma, Esc cancela)\n\
\n\
Video:\n\
  d            Toggle DMA-BUF\n\
  c            Toggle cursor\n\
  t            Cambiar cuenta regresiva (0/3/5/10s)\n\
\n\
Audio:\n\
  a            Cambiar fuente de audio\n\
\n\
Entrada:\n\
  g            Seleccionar región (slurp)\n\
\n\
General:\n\
  ?            Mostrar/ocultar esta ayuda\n\
  Esc          Cerrar ayuda / cancelar\n\
  q / Ctrl+c   Salir\n\
\n\
Versión: 1.0-beta · MIT";

    let overlay = centered_rect(70, 85, area);
    f.render_widget(Clear, overlay);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN))
        .title(" Ayuda — pulsa ? o Esc para cerrar ")
        .title_style(Style::default().fg(GREEN).add_modifier(Modifier::BOLD));
    let p = Paragraph::new(help_text)
        .block(block)
        .style(Style::default())
        .alignment(Alignment::Left);
    f.render_widget(p, overlay);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Draw the text input popup for editing the output path.
fn draw_input_popup(f: &mut ratatui::Frame, state: &mut AppState, area: Rect) {
    let input = state.input_mode.as_ref().unwrap();
    let popup = centered_rect(80, 20, area);
    f.render_widget(Clear, popup);

    let text = format!(
        "{}\n\n  > {}_\n\n  Enter: confirmar · Esc: cancelar · Backspace: borrar",
        input.label,
        input.buffer
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN))
        .title(" Editar ruta ")
        .title_style(Style::default().fg(GREEN).add_modifier(Modifier::BOLD));

    let p = Paragraph::new(text)
        .block(block)
        .style(Style::default());
    f.render_widget(p, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
