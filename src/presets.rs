//! Preset system: built-in presets (one per resource tier × video format)
//! plus user-saved presets stored as JSON in the config dir.
//!
//! A preset is just a [`crate::config::RecordingConfig`] with a name and
//! a tier tag. We don't subclass — when the user picks a preset we copy
//! its fields onto the live config.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::{AudioFormat, RecordingConfig, ResourceTier, VideoFormat};
use crate::errors::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    /// Stable id (used for the user-saved JSON file name).
    pub id: String,
    /// Human-readable name shown in the UI.
    pub name: String,
    /// Hardware tier (used for filtering in the UI).
    pub tier: ResourceTier,
    /// Whether this preset is built-in (cannot be deleted) or user-defined.
    pub builtin: bool,
    /// The recording config to apply when this preset is selected.
    pub config: RecordingConfig,
}

impl Preset {
    /// Directory where user presets live as `<id>.json`.
    pub fn user_dir() -> AppResult<PathBuf> {
        let base = dirs::config_dir()
            .ok_or_else(|| AppError::ConfigParse("XDG config dir not available".into()))?;
        let dir = base.join("wf-recorder-gui").join("presets");
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn user_path(id: &str) -> AppResult<PathBuf> {
        Ok(Self::user_dir()?.join(format!("{id}.json")))
    }

    /// Load every user-saved preset. Errors on individual files are logged
    /// and skipped — a single corrupted preset must not break the list.
    pub fn load_user_all() -> Vec<Preset> {
        let dir = match Self::user_dir() {
            Ok(d) => d,
            Err(e) => {
                log::warn!("could not access user presets dir: {e}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => return out,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            match fs::read_to_string(&p) {
                Ok(s) => match serde_json::from_str::<Preset>(&s) {
                    Ok(mut preset) => {
                        preset.builtin = false;
                        out.push(preset);
                    }
                    Err(e) => log::warn!("skipping malformed preset {}: {e}", p.display()),
                },
                Err(e) => log::warn!("could not read {}: {e}", p.display()),
            }
        }
        out
    }

    pub fn save_user(&self) -> AppResult<()> {
        let path = Self::user_path(&self.id)?;
        let s = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, s)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }
}

/// All built-in presets, generated at runtime so they always match the
/// current default output directory.
pub fn builtins() -> Vec<Preset> {
    let mut out = Vec::new();
    for tier in ResourceTier::ALL {
        for fmt in VideoFormat::ALL {
            out.push(builtin_for(*tier, *fmt));
        }
    }
    out
}

fn builtin_for(tier: ResourceTier, fmt: VideoFormat) -> Preset {
    // Each tier has optimized codec params for the best speed/quality ratio:
    //
    // LOW: preset=ultrafast (minimal CPU during recording), CRF=28 (smaller
    //   files, easier to decode), no DMA-BUF, quality=720p (half the pixels
    //   of 1080p → half the encoding work). VAAPI on (offload to GPU).
    //
    // MEDIUM: preset=superfast (balanced), CRF=23, DMA-BUF on, quality=1080p.
    //   VAAPI on if available.
    //
    // HIGH: preset=veryfast (better compression since CPU can handle it),
    //   CRF=18 (high quality), DMA-BUF on, 60fps for smooth motion,
    //   quality=Original (native resolution).
    let (id, name, fps, crf, bitrate, audio_fmt, audio_br, dmabuf, preset, quality) = match tier {
        ResourceTier::Low => (
            "low",
            "Bajos recursos",
            24u32,
            28u32,
            0u32,
            AudioFormat::Mp3,
            96u32,
            false,
            "ultrafast",
            crate::config::VideoQuality::P720,
        ),
        ResourceTier::Medium => (
            "medium",
            "Medios recursos",
            30,
            23,
            0,
            AudioFormat::Aac,
            128,
            true,
            "superfast",
            crate::config::VideoQuality::P1080,
        ),
        ResourceTier::High => (
            "high",
            "Altos recursos",
            60,
            18,
            0,
            AudioFormat::Opus,
            192,
            true,
            "veryfast",
            crate::config::VideoQuality::Original,
        ),
    };
    let fmt_label = fmt.label().split(' ').next().unwrap_or("Custom").to_lowercase();
    let id = format!("{id}-{fmt_label}");
    let name = format!("{name} · {}", fmt.label());
    Preset {
        id,
        name,
        tier,
        builtin: true,
        config: RecordingConfig {
            video_format: fmt,
            audio_format: audio_fmt,
            fps,
            crf,
            video_bitrate: bitrate,
            audio_bitrate: audio_br,
            use_dmabuf: dmabuf,
            preset: preset.into(),
            quality,
            ..RecordingConfig::default()
        },
    }
}

/// Combined list: built-ins first (sorted by tier then format), then
/// user presets sorted by name.
pub fn all() -> Vec<Preset> {
    let mut v = builtins();
    let mut user = Preset::load_user_all();
    user.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v.extend(user);
    v
}

/// Filter presets by tier. Used by the GUI/TUI when the user picks a
/// tier from the dropdown.
pub fn for_tier(tier: ResourceTier) -> Vec<Preset> {
    all().into_iter().filter(|p| p.tier == tier).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_have_unique_ids() {
        let bs = builtins();
        let mut ids: Vec<_> = bs.iter().map(|p| p.id.clone()).collect();
        ids.sort();
        let len_before = ids.len();
        ids.dedup();
        assert_eq!(len_before, ids.len(), "duplicate preset ids");
    }

    #[test]
    fn builtins_cover_all_combinations() {
        let bs = builtins();
        assert_eq!(bs.len(), ResourceTier::ALL.len() * VideoFormat::ALL.len());
    }

    #[test]
    fn low_tier_uses_no_dmabuf() {
        let p = builtin_for(ResourceTier::Low, VideoFormat::Mp4);
        assert!(!p.config.use_dmabuf);
        assert!(p.config.fps <= 30);
    }

    #[test]
    fn high_tier_uses_opus() {
        let p = builtin_for(ResourceTier::High, VideoFormat::Mkv);
        assert_eq!(p.config.audio_format, AudioFormat::Opus);
        assert_eq!(p.config.fps, 60);
    }
}
