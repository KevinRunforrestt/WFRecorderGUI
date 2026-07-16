//! Wayland / wlroots compositor detection and output listing.
//!
//! wf-recorder only works on wlroots-based compositors that implement the
//! `wlr-screencopy-unstable-v1` protocol. On X11 or non-wlroots Wayland
//! compositors (GNOME, KDE Plasma older than 6.x) it will fail. We detect
//! the situation up-front and surface a clear warning to the user.
//!
//! Output enumeration uses `wlr-randr --json` (the canonical tool for
//! wlroots compositors). We avoid the ScreenCast xdg-desktop-portal here
//! because it returns a PipeWire node id, not an output name, and wf-recorder
//! needs the latter for its `-o` flag.

use std::process::Command;

use serde::Deserialize;

use crate::errors::{AppError, AppResult};

/// Result of detecting the session type. We are deliberately conservative:
/// `Wlroots` is only returned when we have strong evidence the compositor
/// implements `wlr-screencopy-unstable-v1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    /// Compositor that implements wlr-screencopy (Sway, Wayfire, Labwc,
    /// Cage, dwl, Niri, Hyprland, River, …). wf-recorder will work.
    /// The variant name is historical — not all of these use wlroots.
    Wlroots,
    /// Wayland but not wlroots (GNOME, KDE, weston, …). wf-recorder likely
    /// won't work.
    WaylandOther,
    /// X11 session. wf-recorder won't work.
    X11,
    /// No graphical session detected (tty, headless container, …).
    None,
}

impl SessionKind {
    pub fn is_wlroots(self) -> bool {
        matches!(self, SessionKind::Wlroots)
    }

    pub fn label(self) -> &'static str {
        match self {
            SessionKind::Wlroots => "Wayland (wlroots)",
            SessionKind::WaylandOther => "Wayland (no wlroots)",
            SessionKind::X11 => "X11",
            SessionKind::None => "Sin sesión",
        }
    }

    /// One-line message suitable for a warning dialog.
    pub fn warning(self) -> Option<&'static str> {
        match self {
            SessionKind::Wlroots => None,
            SessionKind::WaylandOther => Some(
                "Tu compositor Wayland no parece estar basado en wlroots. \
                 wf-recorder requiere wlr-screencopy y podría no funcionar.",
            ),
            SessionKind::X11 => Some(
                "Estás en X11. wf-recorder solo funciona en compositores \
                 Wayland con wlr-screencopy (Sway, Wayfire, Labwc, Cage, \
                 dwl, Niri, …). La grabación probablemente fallará.",
            ),
            SessionKind::None => Some(
                "No se detectó sesión gráfica (WAYLAND_DISPLAY y DISPLAY \
                 ambos vacíos). wf-recorder no podrá capturar la pantalla.",
            ),
        }
    }
}

/// Compositors known to implement `wlr-screencopy-unstable-v1`, the
/// protocol wf-recorder needs. This includes wlroots-based compositors
/// (Sway, Wayfire, Labwc, Cage, dwl, swayfx, gamescope), Smithay-based
/// compositors (Niri), and Hyprland (which has its own backend but still
/// implements wlr-screencopy). River is also included because it
/// implements the protocol even though it's Smithay-based.
///
/// The name is historical — not all entries here actually use wlroots,
/// but they all implement the wlr-screencopy protocol that wf-recorder
/// requires.
const SCREENCOPY_COMPATIBLE_DESKTOPS: &[&str] = &[
    "sway",
    "SWAY",
    "Hyprland",
    "hyprland",
    "wayfire",
    "Wayfire",
    "labwc",
    "Labwc",
    "river",
    "River",
    "cage",
    "Cage",
    "dwl",
    "niri",
    "Niri",
    "swayfx",
    "SwayFX",
    "gamescope",
    "Gamescope",
];

/// Detect the current session kind by inspecting environment variables.
/// We do NOT spawn any subprocess for this — env vars are enough for a
/// first-pass detection, and the actual wf-recorder run will give the
/// definitive answer if the env was misleading.
pub fn detect_session() -> SessionKind {
    let wayland_display = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x_display = std::env::var_os("DISPLAY").is_some();
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let current_desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

    if !wayland_display && !x_display {
        return SessionKind::None;
    }

    if wayland_display || session_type == "wayland" {
        if SCREENCOPY_COMPATIBLE_DESKTOPS
            .iter()
            .any(|d| current_desktop.split(':').any(|part| part == *d))
        {
            return SessionKind::Wlroots;
        }
        // Even if XDG_CURRENT_DESKTOP is unknown, if wlr-randr is installed
        // we assume wlroots — it's a wlroots-specific tool.
        if which::which("wlr-randr").is_ok() {
            return SessionKind::Wlroots;
        }
        return SessionKind::WaylandOther;
    }

    if x_display || session_type == "x11" {
        return SessionKind::X11;
    }

    SessionKind::None
}

/// Parsed entry from `wlr-randr --json`.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct WlrOutput {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub focused: bool,
    pub modes: Vec<WlrMode>,
    pub current_mode: Option<WlrMode>,
    #[serde(default)]
    pub position: WlrPosition,
    #[serde(default)]
    pub transform: String,
    #[serde(default)]
    pub scale: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct WlrMode {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub refresh: f64,
    #[serde(default)]
    pub current: bool,
    #[serde(default)]
    pub preferred: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
pub struct WlrPosition {
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
}

impl WlrOutput {
    pub fn resolution_label(&self) -> String {
        self.current_mode
            .as_ref()
            .map(|m| format!("{}×{} {:.0}Hz", m.width, m.height, m.refresh))
            .unwrap_or_else(|| "—".into())
    }
}

/// List available outputs via `wlr-randr --json`. Returns an error if
/// wlr-randr is not installed or returns non-zero.
pub fn list_outputs() -> AppResult<Vec<WlrOutput>> {
    which::which("wlr-randr")
        .map_err(|_| AppError::WlrRandrNotFound)?;
    let out = Command::new("wlr-randr")
        .arg("--json")
        .output()
        .map_err(AppError::Io)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("wlr-randr failed: {stderr}"),
        )));
    }
    let parsed: Vec<WlrOutput> = serde_json::from_slice(&out.stdout)
        .map_err(|e| AppError::ConfigParse(format!("wlr-randr JSON parse error: {e}")))?;
    Ok(parsed)
}

/// Run `slurp` to let the user pick a region interactively, returning the
/// geometry string in `WxH+X+Y` format. Returns `Ok(None)` if the user
/// cancelled (slurp exits non-zero without printing anything).
pub fn pick_region() -> AppResult<Option<String>> {
    which::which("slurp").map_err(|_| AppError::SlurpNotFound)?;
    let out = Command::new("slurp")
        .arg("-f")
        .arg("%wx%h+%x+%y")
        .output()
        .map_err(AppError::Io)?;
    if !out.status.success() {
        // slurp exits with code 1 when the user cancels (Esc / right-click).
        return Ok(None);
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_a_variant() {
        // We can't assert a specific value since this depends on the runtime
        // environment, but it must never panic.
        let _ = detect_session();
    }

    #[test]
    fn warning_is_some_for_x11() {
        assert!(SessionKind::X11.warning().is_some());
        assert!(SessionKind::Wlroots.warning().is_none());
    }
}
