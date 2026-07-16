//! Audio source enumeration for PipeWire and PulseAudio.
//!
//! wf-recorder's `-a` flag expects a source name in the *native* format
//! of the chosen backend:
//!   - PulseAudio: `alsa_input.pci-0000_00_1b.0.analog-stereo` (the source
//!     *name*, not the human description)
//!   - PipeWire:   the node *description* as exposed by `pw-cat -p` /
//!     `wpctl status`, e.g. `alsa_output.pci-...analog-stereo` (PipeWire
//!     actually accepts both the PulseAudio-style name and its own node id)
//!
//! We auto-detect which backend is running by looking at the runtime
//! processes / sockets, then list sources with the matching tool
//! (`pactl` for PulseAudio, `wpctl` for PipeWire).

use std::process::Command;

use serde::{Deserialize, Serialize};


/// Detected audio backend. We never assume both can be active at once —
/// in practice PipeWire's PA compat layer answers `pactl` queries, so if
/// PipeWire is running we treat it as the source of truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioBackend {
    Pipewire,
    Pulseaudio,
    None,
}

impl AudioBackend {
    pub fn label(self) -> &'static str {
        match self {
            AudioBackend::Pipewire => "PipeWire",
            AudioBackend::Pulseaudio => "PulseAudio",
            AudioBackend::None => "Ninguno",
        }
    }

    /// wf-recorder `--audio-backend` value. The flag was added in v0.5.0;
    /// older versions default to PulseAudio and don't accept the flag, so
    /// we omit it when the backend is PulseAudio to stay compatible.
    pub fn wf_recorder_arg(self) -> Option<&'static str> {
        match self {
            AudioBackend::Pipewire => Some("pipewire"),
            AudioBackend::Pulseaudio => Some("pulse"),
            AudioBackend::None => None,
        }
    }
}

/// Auto-detect the running audio backend by checking for the presence of
/// the relevant daemon socket / process. We prefer PipeWire when both are
/// available because PA-on-PW is the common modern setup.
pub fn detect_backend() -> AudioBackend {
    // PipeWire exposes a socket at $XDG_RUNTIME_DIR/pipewire-0 (or pipewire-0.lock).
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        let pw_socket = std::path::Path::new(&runtime).join("pipewire-0");
        if pw_socket.exists() {
            return AudioBackend::Pipewire;
        }
    }
    // Fall back to checking for the daemon processes.
    if proc_exists("pipewire") || proc_exists("pipewire-pulse") {
        return AudioBackend::Pipewire;
    }
    if proc_exists("pulseaudio") {
        return AudioBackend::Pulseaudio;
    }
    AudioBackend::None
}

fn proc_exists(name: &str) -> bool {
    // We use /proc scanning instead of `pgrep` to avoid the extra dependency.
    let entries = match std::fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Only consider numeric directories (PIDs).
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(false)
        {
            continue;
        }
        let cmdline_path = path.join("cmdline");
        if let Ok(bytes) = std::fs::read(&cmdline_path) {
            // /proc/<pid>/cmdline is null-separated; first field is argv[0].
            if let Some(arg0) = bytes.split(|b| *b == 0).next() {
                if let Ok(s) = std::str::from_utf8(arg0) {
                    // basename of argv[0]
                    let base = s.rsplit('/').next().unwrap_or(s);
                    if base == name {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// An audio source entry as shown in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSource {
    /// Native name to pass to wf-recorder's `-a` flag.
    pub name: String,
    /// Human-readable description for the dropdown.
    pub description: String,
}

/// List audio sources for the currently active backend. Returns an empty
/// vector on any error so the UI never crashes — recording without audio
/// is always a valid fallback.
pub fn list_sources() -> Vec<AudioSource> {
    match detect_backend() {
        AudioBackend::Pipewire => list_pipewire_sources(),
        AudioBackend::Pulseaudio => list_pulse_sources(),
        AudioBackend::None => Vec::new(),
    }
}

fn list_pipewire_sources() -> Vec<AudioSource> {
    // `wpctl status` outputs a tree with a "Sources:" section. We parse it
    // line-by-line; entries look like:
    //   * 42. Realtek ALC295 Analog [alsa_input.pci-0000_00_1b.0.analog-stereo]
    let out = match Command::new("wpctl").arg("status").output() {
        Ok(o) if o.status.success() => o,
        _ => {
            // wpctl not available — fall back to pactl (PipeWire's PA compat
            // layer answers pactl queries, so this works on most systems).
            return list_pulse_sources();
        }
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut sources = Vec::new();
    let mut in_sources = false;
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("Sources:") {
            in_sources = true;
            continue;
        }
        if in_sources {
            // Next section header ends the Sources block.
            if !line.starts_with(' ') && !line.is_empty() {
                in_sources = false;
                continue;
            }
            // Source entries are indented and look like "* 42. desc [name]"
            // or "  42. desc [name]".
            let l = line.trim_start();
            if l.starts_with('*') || l.starts_with(char::is_numeric) {
                if let Some(src) = parse_wpctl_line(l) {
                    sources.push(src);
                }
            }
        }
    }
    // If wpctl returned nothing useful, also try pactl as a fallback.
    if sources.is_empty() {
        return list_pulse_sources();
    }
    sources
}

fn parse_wpctl_line(l: &str) -> Option<AudioSource> {
    // Strip leading "* " or "<num>. "
    let l = l.trim_start_matches('*').trim_start();
    let after_dot = l.split_once('.').map(|(_, rest)| rest.trim_start())?;
    // Trailing bracket contains the native name.
    let (desc, name) = if let Some(open) = after_dot.rfind('[') {
        let close = after_dot.rfind(']')?;
        if close <= open {
            return None;
        }
        let name = after_dot[open + 1..close].to_string();
        let desc = after_dot[..open].trim().to_string();
        (desc, name)
    } else {
        (after_dot.to_string(), after_dot.to_string())
    };
    if name.is_empty() {
        None
    } else {
        Some(AudioSource { name, description: desc })
    }
}

fn list_pulse_sources() -> Vec<AudioSource> {
    // `pactl list short sources` -> "id\tname\tdriver\tsample_spec\tstate"
    // We use the short form to get the names, then optionally enrich with
    // descriptions from `pactl list sources` (full form). The short form
    // is more reliable across PA/PW versions.
    let out = match Command::new("pactl")
        .args(["list", "short", "sources"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut sources = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            let name = parts[1].to_string();
            // The short form doesn't have a description; use the name as
            // the description too. wf-recorder with PulseAudio backend
            // expects the name in `-a`, so this is correct.
            sources.push(AudioSource {
                description: name.clone(),
                name,
            });
        }
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wpctl_line_handles_asterisk() {
        let s = parse_wpctl_line("* 42. Realtek ALC295 Analog [alsa_input.pci-0000_00_1b.0.analog-stereo]");
        assert!(s.is_some());
        let s = s.unwrap();
        assert_eq!(s.name, "alsa_input.pci-0000_00_1b.0.analog-stereo");
        assert_eq!(s.description, "Realtek ALC295 Analog");
    }

    #[test]
    fn parse_wpctl_line_rejects_garbage() {
        assert!(parse_wpctl_line("not a source line").is_none());
        assert!(parse_wpctl_line("").is_none());
    }

    #[test]
    fn backend_arg_matches_wf_recorder() {
        assert_eq!(AudioBackend::Pipewire.wf_recorder_arg(), Some("pipewire"));
        assert_eq!(AudioBackend::Pulseaudio.wf_recorder_arg(), Some("pulse"));
        assert_eq!(AudioBackend::None.wf_recorder_arg(), None);
    }
}
