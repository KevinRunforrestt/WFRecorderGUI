//! Persistent application configuration.
//!
//! Stored as TOML at `$XDG_CONFIG_HOME/wf-recorder-gui/config.toml` (falls
//! back to `~/.config/wf-recorder-gui/config.toml`). Only the *last used*
//! values are persisted — preset definitions live in [`crate::presets`].

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

/// Container format chosen by the user. Each one maps to a wf-recorder
/// codec + muxer + pixel format combination, see [`crate::recorder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoFormat {
    Mp4,
    Webm,
    Mkv,
    Gif,
}

impl VideoFormat {
    pub const ALL: &[VideoFormat] = &[
        VideoFormat::Mp4,
        VideoFormat::Webm,
        VideoFormat::Mkv,
        VideoFormat::Gif,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VideoFormat::Mp4 => "MP4 (H.264)",
            VideoFormat::Webm => "WebM (VP9)",
            VideoFormat::Mkv => "MKV (H.264)",
            VideoFormat::Gif => "GIF (animated)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            VideoFormat::Mp4 => "mp4",
            VideoFormat::Webm => "webm",
            VideoFormat::Mkv => "mkv",
            VideoFormat::Gif => "gif",
        }
    }
}

/// Audio codec for the captured stream. wf-recorder routes the audio
/// through ffmpeg's libavcodec, so any encoder works as long as the
/// container supports it (e.g. Opus in MKV/WebM, MP3 in MP4/MKV).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    Aac,
    Mp3,
    Ogg,
    Opus,
    None,
}

impl AudioFormat {
    pub const ALL: &[AudioFormat] = &[
        AudioFormat::Aac,
        AudioFormat::Mp3,
        AudioFormat::Ogg,
        AudioFormat::Opus,
        AudioFormat::None,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AudioFormat::Aac => "AAC",
            AudioFormat::Mp3 => "MP3",
            AudioFormat::Ogg => "Vorbis (OGG)",
            AudioFormat::Opus => "Opus",
            AudioFormat::None => "Sin audio",
        }
    }

    pub fn ffmpeg_codec(self) -> Option<&'static str> {
        match self {
            AudioFormat::Aac => Some("aac"),
            AudioFormat::Mp3 => Some("libmp3lame"),
            AudioFormat::Ogg => Some("libvorbis"),
            AudioFormat::Opus => Some("libopus"),
            AudioFormat::None => None,
        }
    }
}

/// Target output resolution. The screen is captured at native resolution
/// and scaled down in post-processing (ffmpeg -vf scale=W:H). This
/// produces smaller files that are easier to decode on weak CPUs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoQuality {
    /// No scaling — keep the native capture resolution.
    Original,
    /// 256×144 — tiny preview quality, minimal file size.
    P144,
    /// 426×240 — very low quality, good for slow connections.
    P240,
    /// 640×360 — low quality, small file.
    P360,
    /// 1280×720 — standard HD, good balance for low-resource PCs.
    P720,
    /// 1920×1080 — full HD.
    P1080,
    /// User-specified width/height (see `manual_width`/`manual_height`).
    Manual,
}

impl VideoQuality {
    pub const ALL: &[VideoQuality] = &[
        VideoQuality::Original,
        VideoQuality::P144,
        VideoQuality::P240,
        VideoQuality::P360,
        VideoQuality::P720,
        VideoQuality::P1080,
        VideoQuality::Manual,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VideoQuality::Original => "Original (sin escalar)",
            VideoQuality::P144 => "144p",
            VideoQuality::P240 => "240p",
            VideoQuality::P360 => "360p",
            VideoQuality::P720 => "720p (HD)",
            VideoQuality::P1080 => "1080p (Full HD)",
            VideoQuality::Manual => "Manual",
        }
    }

    /// Returns (width, height) for this quality, or None for Original.
    /// For Manual, the caller must provide manual_width/manual_height.
    pub fn dimensions(self, manual_w: u32, manual_h: u32) -> Option<(u32, u32)> {
        match self {
            VideoQuality::Original => None,
            VideoQuality::P144 => Some((256, 144)),
            VideoQuality::P240 => Some((426, 240)),
            VideoQuality::P360 => Some((640, 360)),
            VideoQuality::P720 => Some((1280, 720)),
            VideoQuality::P1080 => Some((1920, 1080)),
            VideoQuality::Manual => {
                if manual_w > 0 && manual_h > 0 {
                    Some((manual_w, manual_h))
                } else {
                    None
                }
            }
        }
    }
}

/// Hardware resource tier. Used to filter the built-in preset list so
/// the user is not overwhelmed by options that won't run well on their
/// machine. The user can still override and pick any preset manually.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceTier {
    /// Low-resource PCs (older laptops, single-core VMs, ARM SBCs).
    Low,
    /// Mid-range desktops/laptops (4+ cores, integrated graphics).
    Medium,
    /// High-end machines (8+ cores, dedicated GPU with VAAPI/NVENC).
    High,
}

impl ResourceTier {
    pub const ALL: &[ResourceTier] = &[
        ResourceTier::Low,
        ResourceTier::Medium,
        ResourceTier::High,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ResourceTier::Low => "Bajos recursos",
            ResourceTier::Medium => "Medios recursos",
            ResourceTier::High => "Altos recursos",
        }
    }

}

/// User-facing recording options. These are the values the GUI/TUI edit
/// and that get passed to [`crate::recorder::RecorderController::start`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingConfig {
    /// Selected Wayland output name (e.g. "eDP-1"). Empty = let wf-recorder
    /// auto-pick the first output.
    pub output: String,
    /// Geometry override in `WxH+X+Y` format. Empty = full output.
    pub geometry: String,
    /// Audio source name (PipeWire node name or PulseAudio source name).
    /// Empty = no audio.
    pub audio_source: String,
    /// Human-readable description of the audio source (for PipeWire backend,
    /// wf-recorder's `-a` flag expects the description, not the technical
    /// name). Kept in sync with `audio_source` by the UI.
    pub audio_source_desc: String,
    /// Video container/codec.
    pub video_format: VideoFormat,
    /// Audio codec.
    pub audio_format: AudioFormat,
    /// Frames per second (15..=144).
    pub fps: u32,
    /// CRF value (0..=51, lower = better quality, larger file).
    pub crf: u32,
    /// Video bitrate hint in kbps (0 = use CRF only).
    pub video_bitrate: u32,
    /// Audio bitrate in kbps.
    pub audio_bitrate: u32,
    /// Output directory (file name is auto-generated from timestamp).
    pub output_dir: PathBuf,
    /// Use DMA-BUF capture path (default true; disable for some VAAPI issues).
    pub use_dmabuf: bool,
    /// Record cursor.
    pub record_cursor: bool,
    /// Countdown duration in seconds before recording starts (0 = no countdown).
    pub countdown_secs: u32,
    /// Custom output file name (without extension). Empty = auto-generated
    /// from timestamp.
    pub custom_filename: String,
    /// x264 encoding preset. Controls speed/quality tradeoff:
    ///   ultrafast = fastest encode, largest file, lowest quality
    ///   superfast = fast encode, large file (wf-recorder default)
    ///   veryfast  = balanced
    ///   faster    = slower encode, smaller file
    ///   fast      = slower encode, smaller file
    /// For low-resource PCs use ultrafast; for high-end use veryfast.
    pub preset: String,
    /// Target output resolution. The recording is captured at native
    /// resolution and scaled down in post-processing (ffmpeg -vf scale).
    /// Original = no scaling. Lower resolutions = smaller files and
    /// easier playback on weak CPUs.
    pub quality: VideoQuality,
    /// Manual resolution width (used only when quality == Manual).
    pub manual_width: u32,
    /// Manual resolution height (used only when quality == Manual).
    pub manual_height: u32,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            output: String::new(),
            geometry: String::new(),
            audio_source: String::new(),
            audio_source_desc: String::new(),
            video_format: VideoFormat::Mp4,
            audio_format: AudioFormat::Aac,
            fps: 30,
            crf: 20,
            video_bitrate: 0,
            audio_bitrate: 128,
            output_dir: dirs::video_dir()
                .or_else(dirs::download_dir)
                .unwrap_or_else(|| PathBuf::from("/tmp")),
            use_dmabuf: true,
            record_cursor: true,
            countdown_secs: 0,
            custom_filename: String::new(),
            preset: "superfast".into(),
            quality: VideoQuality::Original,
            manual_width: 1280,
            manual_height: 720,
        }
    }
}

/// Top-level persisted config: just the last-used recording options and
/// a few UI preferences. Heavy preset definitions are NOT persisted here,
/// they live in `presets/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub recording: RecordingConfig,
    /// Selected resource tier filter for the preset list.
    pub tier_filter: ResourceTier,
    /// Whether to show the X11 warning banner (it can be dismissed).
    pub show_x11_warning: bool,
    /// Customizable keyboard shortcuts.
    #[serde(default)]
    pub keybinds: Keybinds,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            recording: RecordingConfig::default(),
            tier_filter: ResourceTier::Medium,
            show_x11_warning: true,
            keybinds: Keybinds::default(),
        }
    }
}

/// Customizable keyboard shortcuts. Each entry is stored as a string in
/// GTK accelerator format (e.g. "<Control><Alt>r"). The GUI parses these
/// with `gtk::accelerator_parse`; the TUI parses them with its own simple
/// parser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keybinds {
    /// Start recording.
    pub start: String,
    /// Stop recording.
    pub stop: String,
    /// Pause / resume recording.
    pub pause: String,
}

impl Default for Keybinds {
    fn default() -> Self {
        Self {
            start: "<Control><Alt>r".into(),
            stop: "<Control><Alt>s".into(),
            pause: "<Control><Alt>p".into(),
        }
    }
}

impl Keybinds {
    /// Parse a GTK accelerator string into (ctrl, alt, shift, key_name).
    /// key_name is the lowercase name of the key (e.g. "r", "f1", "space").
    /// Returns None if the string is invalid.
    pub fn parse_accel(s: &str) -> Option<(bool, bool, bool, String)> {
        let s = s.to_lowercase();
        let ctrl = s.contains("<control>") || s.contains("<ctrl>");
        let alt = s.contains("<alt>") || s.contains("<mod1>");
        let shift = s.contains("<shift>");
        // Extract the key name after the last '>'. This handles both
        // single chars ("r") and named keys ("f1", "space", "return").
        let key = s
            .split('>')
            .last()?
            .trim()
            .to_string();
        if key.is_empty() {
            return None;
        }
        Some((ctrl, alt, shift, key))
    }
}

impl AppConfig {
    /// Returns the path to the config file, creating the parent directory
    /// if needed. Errors propagate as [`AppError::Io`].
    pub fn path() -> AppResult<PathBuf> {
        let base = dirs::config_dir()
            .ok_or_else(|| AppError::ConfigParse("XDG config dir not available".into()))?;
        let dir = base.join("wf-recorder-gui");
        fs::create_dir_all(&dir)?;
        Ok(dir.join("config.toml"))
    }

    /// Load from disk, falling back to defaults if the file does not exist
    /// or is malformed (we log a warning but never crash the UI over a
    /// corrupted config).
    pub fn load_or_default() -> Self {
        let path = match Self::path() {
            Ok(p) => p,
            Err(e) => {
                log::warn!("could not resolve config path: {e}");
                return Self::default();
            }
        };
        if !path.exists() {
            return Self::default();
        }
        match fs::read_to_string(&path) {
            Ok(s) => match toml::from_str::<Self>(&s) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("config at {} is malformed ({e}), using defaults", path.display());
                    Self::default()
                }
            },
            Err(e) => {
                log::warn!("could not read {}: {e}", path.display());
                Self::default()
            }
        }
    }

    /// Persist to disk atomically (write to .tmp then rename).
    pub fn save(&self) -> AppResult<()> {
        let path = Self::path()?;
        let s = toml::to_string_pretty(self)
            .map_err(|e| AppError::ConfigParse(e.to_string()))?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, s)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }
}

/// Generate a filename. If `cfg.custom_filename` is non-empty, use it
/// (with the right extension appended); otherwise generate one from the
/// current timestamp like `Recording_2024-07-12_18-30-45.mp4`.
pub fn default_filename(cfg: &RecordingConfig) -> String {
    let ext = cfg.video_format.extension();
    if !cfg.custom_filename.trim().is_empty() {
        let safe = crate::shared::sanitize_filename(&cfg.custom_filename);
        if !safe.is_empty() {
            return ensure_extension(&safe, cfg.video_format);
        }
    }
    let ts = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    format!("Recording_{ts}.{ext}")
}

/// Convenience helper to ensure a path has the right extension for the
/// chosen video format. If the user's filename already ends with the
/// right extension it is left untouched; otherwise it's appended.
pub fn ensure_extension(name: &str, fmt: VideoFormat) -> String {
    let want = format!(".{}", fmt.extension());
    if name.to_ascii_lowercase().ends_with(&want) {
        name.to_string()
    } else {
        format!("{name}{}", want)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_extension_appends_when_missing() {
        assert_eq!(ensure_extension("clip", VideoFormat::Mp4), "clip.mp4");
        assert_eq!(ensure_extension("clip.mp4", VideoFormat::Mp4), "clip.mp4");
        assert_eq!(ensure_extension("clip.MP4", VideoFormat::Mp4), "clip.MP4");
    }

    #[test]
    fn default_filename_has_extension() {
        let cfg = RecordingConfig::default();
        let name = default_filename(&cfg);
        assert!(name.ends_with(".mp4"));
        assert!(name.starts_with("Recording_"));
    }

    #[test]
    fn config_roundtrip() {
        let cfg = AppConfig::default();
        let s = toml::to_string(&cfg).unwrap();
        let back: AppConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg.recording.fps, back.recording.fps);
        assert_eq!(cfg.recording.video_format, back.recording.video_format);
    }
}
