//! Shared helpers used by both the GTK GUI and the TUI.

use crate::config::{RecordingConfig, ResourceTier};
use crate::presets::Preset;

/// Apply a preset's recording-only fields to an existing config, preserving
/// session-specific fields (output dir, audio source, output, geometry).
pub fn apply_preset(cfg: &mut RecordingConfig, preset: &Preset) {
    cfg.video_format = preset.config.video_format;
    cfg.audio_format = preset.config.audio_format;
    cfg.fps = preset.config.fps;
    cfg.crf = preset.config.crf;
    cfg.video_bitrate = preset.config.video_bitrate;
    cfg.audio_bitrate = preset.config.audio_bitrate;
    cfg.use_dmabuf = preset.config.use_dmabuf;
    cfg.preset = preset.config.preset.clone();
    cfg.quality = preset.config.quality;
}

/// Infer a resource tier from FPS + CRF values. Used for display only.
pub fn infer_tier(cfg: &RecordingConfig) -> ResourceTier {
    if cfg.fps >= 60 && cfg.crf <= 20 {
        ResourceTier::High
    } else if cfg.fps >= 30 && cfg.crf <= 25 {
        ResourceTier::Medium
    } else {
        ResourceTier::Low
    }
}

/// Sanitize a user-provided filename to prevent path traversal attacks.
/// Strips path separators, parent directory references, and other
/// dangerous characters. Returns a safe basename.
pub fn sanitize_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == '\0' {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Remove leading dots (prevents hidden files and ".." traversal).
    let sanitized = sanitized.trim_start_matches('.');
    // If empty after sanitization, return empty (caller falls back to auto).
    if sanitized.is_empty() {
        return String::new();
    }
    sanitized.to_string()
}
