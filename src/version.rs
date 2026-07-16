//! Version constants for wf-recorder-gui.
//!
//! Kept in a separate module so the About dialog and the CLI can both
//! reference it without circular deps.

/// Semantic version string shown in the About dialog and `--version`.
pub const VERSION: &str = "1.0-beta";

/// One-line description shown in the About dialog.
pub const DESCRIPTION: &str = "Grabador de pantalla para compositores wlroots y Smithay (Sway, Wayfire, Labwc, Cage, dwl, Niri)";

/// Application name as shown to the user.
pub const APP_NAME: &str = "WFRecorderGUI";

/// License identifier (SPDX).
pub const LICENSE: &str = "MIT";
