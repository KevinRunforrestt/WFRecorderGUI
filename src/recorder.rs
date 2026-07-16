//! Recorder controller: spawn, monitor and stop wf-recorder.
//!
//! The controller owns the child process handle and exposes a
//! [`RecorderHandle`] that other parts of the program can poll for status.
//! Stopping is done with SIGINT so ffmpeg can flush its buffers and write
//! the trailer cleanly — killing with SIGKILL would leave the file
//! unplayable.
//!
//! All blocking operations (waiting for the child, reading stderr) happen
//! on a dedicated worker thread; the controller's public API is non-blocking
//! and safe to call from the UI thread.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use crate::audio::{detect_backend, AudioBackend};
use crate::config::{AudioFormat, RecordingConfig, VideoFormat};
use crate::errors::{AppError, AppResult};

/// Recording status as seen by the UI. The transitions are:
///   Idle → Recording → Stopping → Idle      (clean stop)
///   Idle → Recording → Failed → Idle        (wf-recorder crashed)
///   Idle → Failed                            (failed to spawn)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecorderStatus {
    Idle,
    Recording {
        started_at: Instant,
        output_path: PathBuf,
    },
    #[allow(dead_code)]
    Stopping,
    Failed {
        message: String,
    },
}

impl RecorderStatus {
    pub fn is_recording(&self) -> bool {
        matches!(self, RecorderStatus::Recording { .. } | RecorderStatus::Stopping)
    }

    /// Short label for the status bar. Does NOT include the elapsed time
    /// — that goes in a separate widget (`elapsed_label`) so we don't
    /// show two timers at once.
    pub fn label(&self) -> String {
        match self {
            RecorderStatus::Idle => "Inactivo".to_string(),
            RecorderStatus::Recording { .. } => "Grabando".to_string(),
            RecorderStatus::Stopping => "Deteniendo…".to_string(),
            RecorderStatus::Failed { message } => format!("Error: {message}"),
        }
    }
}

/// Internal message from the worker thread to the controller.
enum WorkerMsg {
    Started(PathBuf),
    StderrLine(String),
    Exited { code: Option<i32> },
    IoError(String),
}

/// A handle returned by [`RecorderController::start`]. The recording
/// continues until either [`RecorderHandle::stop`] is called or the
/// handle is dropped (in which case the worker sends SIGINT first).
pub struct RecorderHandle {
    /// Child process handle. Public so the GUI/TUI can access the PID for
    /// pause/resume via SIGSTOP/SIGCONT.
    pub child: Arc<Mutex<Option<Child>>>,
    pub output_path: PathBuf,
    rx: mpsc::Receiver<WorkerMsg>,
    last_status: RecorderStatus,
    stderr_lines: Vec<String>,
    /// Set to true when the user explicitly asked to stop. We use this to
    /// distinguish a clean exit (status Stopping → Idle) from a crash
    /// (status Recording → Failed).
    stop_requested: bool,
}

impl RecorderHandle {
    /// Poll for status updates from the worker thread. Returns the latest
    /// status, or `None` if the worker has finished and there are no more
    /// messages. Should be called from the UI tick (e.g. GLib `timeout_add`).
    pub fn poll(&mut self) -> Option<RecorderStatus> {
        let mut new_status = None;
        // Drain all pending messages, keep only the last status transition.
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                WorkerMsg::Started(p) => {
                    self.output_path = p.clone();
                    new_status = Some(RecorderStatus::Recording {
                        started_at: Instant::now(),
                        output_path: p,
                    });
                }
                WorkerMsg::StderrLine(l) => {
                    // Keep the last ~500 stderr lines for the diagnostics panel.
                    if self.stderr_lines.len() >= 500 {
                        self.stderr_lines.remove(0);
                    }
                    self.stderr_lines.push(l);
                }
                WorkerMsg::Exited { code } => {
                    new_status = Some(if self.stop_requested {
                        RecorderStatus::Idle
                    } else {
                        // Build a useful error message that includes the
                        // last few stderr lines so the user can see exactly
                        // why wf-recorder failed (e.g. "compositor doesn't
                        // support wlr-screencopy" or "audio source not found").
                        let stderr_tail: String = self
                            .stderr_lines
                            .iter()
                            .rev()
                            .take(5)
                            .rev()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n");
                        let msg = match code {
                            Some(0) => "wf-recorder exited unexpectedly".to_string(),
                            Some(c) => {
                                if stderr_tail.is_empty() {
                                    format!("wf-recorder exited with code {c}")
                                } else {
                                    format!(
                                        "wf-recorder exited with code {c}:\n\n{stderr_tail}"
                                    )
                                }
                            }
                            None => "wf-recorder was killed".to_string(),
                        };
                        RecorderStatus::Failed { message: msg }
                    });
                }
                WorkerMsg::IoError(e) => {
                    new_status = Some(RecorderStatus::Failed { message: e });
                }
            }
        }
        if let Some(s) = new_status.clone() {
            self.last_status = s;
        }
        new_status
    }

    /// Latest known status (does not poll).
    pub fn status(&self) -> RecorderStatus {
        self.last_status.clone()
    }

    /// Stderr captured from wf-recorder, oldest first. Useful to display in
    /// an "diagnostics" expander when a recording fails.
    pub fn stderr(&self) -> &[String] {
        &self.stderr_lines
    }

    /// Request a clean stop (SIGINT). Returns immediately; the actual
    /// status transition to Idle happens when the worker thread observes
    /// the child exit.
    pub fn stop(&mut self) -> AppResult<()> {
        self.stop_requested = true;
        let guard = self
            .child
            .lock()
            .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        if let Some(child) = guard.as_ref() {
            // SIGINT == signal 2 on Linux. nix crate would be cleaner but
            // adding it just for one constant is overkill.
            #[cfg(unix)]
            {
                let pid = child.id() as i32;
                // SAFETY: kill(2) with a valid PID and a standard signal is
                // safe to call. We use SIGINT (2) for a clean wf-recorder
                // shutdown so ffmpeg flushes the file trailer.
                let r = libc_kill(pid, libc_signal_int());
                if r != 0 {
                    return Err(AppError::Io(std::io::Error::last_os_error()));
                }
            }
            #[cfg(not(unix))]
            {
                let _ = child;
                return Err(AppError::Io(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "SIGINT stop is only supported on Unix",
                )));
            }
        }
        Ok(())
    }
}

impl Drop for RecorderHandle {
    fn drop(&mut self) {
        // Best-effort clean stop when the handle is dropped without an
        // explicit `stop()` call. If the child is already gone this is a no-op.
        let _ = self.stop();
    }
}

// Tiny libc shims so we don't pull in the `nix` crate just for kill(2).
// We declare them as `extern "C"` and wrap them in unsafe; the constants
// are stable POSIX values.
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}
fn libc_kill(pid: i32, sig: i32) -> i32 {
    unsafe { kill(pid, sig) }
}
fn libc_signal_int() -> i32 {
    2 // SIGINT
}

/// The controller is a thin façade: there is at most one active recording
/// per process. Holding the controller lets you `start()` a new recording.
pub struct RecorderController;

impl RecorderController {
    /// Spawn wf-recorder with the given config. Returns a handle that the
    /// caller must keep alive (and periodically `poll()`) for as long as
    /// the recording should run.
    pub fn start(cfg: &RecordingConfig) -> AppResult<RecorderHandle> {
        // Pre-flight: make sure wf-recorder is installed.
        which::which("wf-recorder").map_err(|_| AppError::WfRecorderNotFound)?;

        // Validate output dir + build output path.
        let out_dir = validate_output_dir(&cfg.output_dir)?;
        let filename = crate::config::default_filename(cfg);
        let filename = crate::config::ensure_extension(&filename, cfg.video_format);
        let output_path = out_dir.join(&filename);

        // Build the argv list. We always pass `-y` (overwrite) and `-f`
        // (output file) so wf-recorder doesn't block on stdin prompts.
        //
        // IMPORTANT: in wf-recorder's CLI, `-f` is the OUTPUT FILE and `-o`
        // is the WAYLAND OUTPUT NAME (the monitor to capture). They are NOT
        // interchangeable. Mixing them up causes wf-recorder to look for a
        // monitor named "<file path>" and exit with code 1.
        let mut args: Vec<String> = Vec::with_capacity(16);
        args.push("-y".into());
        args.push("-f".into());
        args.push(output_path.to_string_lossy().into_owned());

        // Codec / format selection.
        let (codec, pix_fmt) = codec_for_format(cfg.video_format);
        args.push("-c".into());
        args.push(codec.into());
        args.push("-x".into());
        args.push(pix_fmt.into());

        // Codec params: CRF, preset, and key encoding optimizations.
        //
        // OPTIMIZATION NOTES (tuned for low-resource PCs like i5-4570):
        //   - preset: controls encode speed vs compression. ultrafast = least
        //     CPU during recording (ideal for low-resource PCs).
        //   - tune=zerolatency: reduces encoding latency.
        //   - bf=0: disables B-frames → faster decoding in mpv.
        //   - g=<fps*2>: keyframe interval = 2 seconds → fast seeking.
        args.push("-p".into());
        args.push(format!("crf={}", cfg.crf));
        args.push("-p".into());
        args.push(format!("preset={}", cfg.preset));
        args.push("-p".into());
        args.push("tune=zerolatency".into());
        args.push("-p".into());
        args.push("bf=0".into());
        args.push("-p".into());
        args.push(format!("g={}", cfg.fps * 2));
        if cfg.video_bitrate > 0 {
            args.push("-p".into());
            args.push(format!("bitrate={}k", cfg.video_bitrate));
        }

        // Frame rate.
        args.push("-r".into());
        args.push(cfg.fps.to_string());

        // DMA-BUF / damage capture mode.
        if !cfg.use_dmabuf {
            args.push("--no-dmabuf".into());
        }

        // Wayland output (monitor) selection. Empty = let wf-recorder pick
        // the first output. This is the `-o` flag — it expects a monitor
        // name like "eDP-1", NOT a file path.
        if !cfg.output.is_empty() {
            args.push("-o".into());
            args.push(cfg.output.clone());
        }

        // Geometry override (from slurp).
        if !cfg.geometry.is_empty() {
            args.push("-g".into());
            args.push(cfg.geometry.clone());
        }

        // Audio backend + source.
        //
        // IMPORTANT: wf-recorder's `-a` flag expects DIFFERENT values
        // depending on the backend:
        //   - PulseAudio: the source *name* (e.g. "alsa_input.pci-...analog-stereo")
        //   - PipeWire:   the node *description* (human-readable string like
        //                 "Realtek ALC295 Analog"), NOT the technical name.
        //
        // This is a common source of "no audio" bugs. We keep both fields
        // in RecordingConfig and pick the right one here.
        let backend = detect_backend();
        if !cfg.audio_source.is_empty() && cfg.audio_format != AudioFormat::None {
            if let Some(backend_arg) = backend.wf_recorder_arg() {
                args.push("--audio-backend".into());
                args.push(backend_arg.into());
            }
            args.push("-a".into());
            let audio_arg = match backend {
                AudioBackend::Pipewire => {
                    // For PipeWire, prefer the description if available;
                    // fall back to the name if the UI didn't populate it.
                    if !cfg.audio_source_desc.is_empty() {
                        cfg.audio_source_desc.clone()
                    } else {
                        cfg.audio_source.clone()
                    }
                }
                AudioBackend::Pulseaudio | AudioBackend::None => cfg.audio_source.clone(),
            };
            args.push(audio_arg);

            // Audio codec + bitrate via -p (codec params forwarded to ffmpeg).
            if let Some(acodec) = cfg.audio_format.ffmpeg_codec() {
                args.push("-p".into());
                args.push(format!("audio_codec={acodec}"));
                args.push("-p".into());
                args.push(format!("audio_bitrate={}k", cfg.audio_bitrate));
            }
        }

        // Cursor.
        if !cfg.record_cursor {
            args.push("--no-cursor".into());
        }

        log::info!("spawning wf-recorder with args: {:?}", args);
        log::info!("output file: {}", output_path.display());
        log::info!("audio backend: {}", backend.label());

        let mut cmd = Command::new("wf-recorder");
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let child = cmd.spawn().map_err(|e| {
            AppError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("failed to spawn wf-recorder: {e}"),
            ))
        })?;

        // Spawn a worker thread that reads stderr and waits for exit.
        let (tx, rx) = mpsc::channel::<WorkerMsg>();
        let mut child_to_watch: Child = child;
        let stderr = child_to_watch.stderr.take();
        let stdout = child_to_watch.stdout.take();
        let initial_path = output_path.clone();

        let child_arc = Arc::new(Mutex::new(Some(child_to_watch)));
        let child_for_worker = Arc::clone(&child_arc);
        let tx_for_worker = tx.clone();

        thread::spawn(move || {
            // We don't need stdout (wf-recorder doesn't write anything
            // meaningful to it) but we drain it so the pipe doesn't fill
            // up and block the child.
            if let Some(stdout) = stdout {
                let _ = thread::spawn(move || {
                    let mut reader = BufReader::new(stdout);
                    let mut buf = String::new();
                    while reader.read_line(&mut buf).map(|n| n > 0).unwrap_or(false) {
                        buf.clear();
                    }
                });
            }
            if let Some(stderr) = stderr {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(l) => {
                            log::debug!("wf-recorder: {l}");
                            if tx_for_worker.send(WorkerMsg::StderrLine(l)).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            log::warn!("stderr read error: {e}");
                            break;
                        }
                    }
                }
            }
            // Wait for the child to exit. We hold the lock briefly so the
            // controller's `stop()` call doesn't race with our wait().
            let exit_code = {
                let mut guard = match child_for_worker.lock() {
                    Ok(g) => g,
                    Err(e) => {
                        let _ = tx_for_worker.send(WorkerMsg::IoError(format!("lock poisoned: {e}")));
                        return;
                    }
                };
                let child = match guard.as_mut() {
                    Some(c) => c,
                    None => return,
                };
                match child.wait() {
                    Ok(status) => status.code(),
                    Err(e) => {
                        let _ = tx_for_worker.send(WorkerMsg::IoError(format!("wait failed: {e}")));
                        return;
                    }
                }
            };
            let _ = tx_for_worker.send(WorkerMsg::Exited { code: exit_code });
        });

        // Emit the "Started" event right away so the UI flips to Recording
        // without waiting for the first stderr line.
        let _ = tx.send(WorkerMsg::Started(initial_path.clone()));

        Ok(RecorderHandle {
            child: child_arc,
            output_path: initial_path,
            rx,
            last_status: RecorderStatus::Recording {
                started_at: Instant::now(),
                output_path: output_path.clone(),
            },
            stderr_lines: Vec::new(),
            stop_requested: false,
        })
    }
}

/// Map a [`VideoFormat`] to the (codec, pix_fmt) pair wf-recorder needs.
/// For GIF we use ffmpeg's palette generation path: we record to a temp
/// MP4 and then post-process with ffmpeg to produce the final GIF.
fn codec_for_format(fmt: VideoFormat) -> (&'static str, &'static str) {
    match fmt {
        VideoFormat::Mp4 | VideoFormat::Mkv => ("libx264", "yuv420p"),
        VideoFormat::Webm => ("libvpx-vp9", "yuv420p"),
        VideoFormat::Gif => ("libx264", "yuv420p"), // intermediate; converted later
    }
}

fn validate_output_dir(p: &Path) -> AppResult<PathBuf> {
    if !p.is_absolute() {
        return Err(AppError::ConfigParse(format!(
            "output_dir must be absolute, got {}",
            p.display()
        )));
    }
    if !p.exists() {
        std::fs::create_dir_all(p)?;
    }
    Ok(p.to_path_buf())
}

/// Post-process a recording into a GIF using ffmpeg. wf-recorder can't
/// emit GIF directly with good quality, so we capture to MP4 first and
/// then run the standard two-pass palettegen → paletteuse pipeline.
///
/// Returns the path to the generated GIF (the intermediate MP4 is deleted).
pub fn convert_to_gif(mp4: &Path) -> AppResult<PathBuf> {
    which::which("ffmpeg").map_err(|_| AppError::FfmpegNotFound)?;
    let gif = mp4.with_extension("gif");
    let palette = mp4.with_extension("palette.png");

    // Pass 1: generate the palette.
    let s1 = Command::new("ffmpeg")
        .args([
            "-y", "-i", &mp4.to_string_lossy(),
            "-vf", "palettegen=stats_mode=diff",
            &palette.to_string_lossy(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .map_err(AppError::Io)?;
    if !s1.success() {
        let _ = std::fs::remove_file(&palette);
        return Err(AppError::RecorderExit(
            s1.code().unwrap_or(-1),
            "palettegen pass failed".into(),
        ));
    }

    // Pass 2: apply the palette.
    let s2 = Command::new("ffmpeg")
        .args([
            "-y", "-i", &mp4.to_string_lossy(),
            "-i", &palette.to_string_lossy(),
            "-lavfi", "paletteuse=dither=bayer:bayer_scale=5",
            &gif.to_string_lossy(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .map_err(AppError::Io)?;
    let _ = std::fs::remove_file(&palette);
    if !s2.success() {
        return Err(AppError::RecorderExit(
            s2.code().unwrap_or(-1),
            "paletteuse pass failed".into(),
        ));
    }

    // Best-effort cleanup of the intermediate MP4.
    let _ = std::fs::remove_file(mp4);
    Ok(gif)
}

/// Post-process an MP4 file to move the moov atom to the start of the file.
///
/// When wf-recorder writes an MP4, the moov atom (which contains the index
/// of all frames) ends up at the END of the file. This means media players
/// like mpv have to seek to the end before they can start playing, which
/// causes sluggish playback — especially on slow disks or network mounts.
///
/// `ffmpeg -movflags +faststart` remuxes the file (no re-encoding, so it's
/// very fast — just copies the streams) and puts the moov atom at the start.
/// The result is instant playback in mpv and any other player.
///
/// This is a no-op for MKV/WebM (those containers don't have this problem).
pub fn faststart_mp4(path: &Path) -> AppResult<()> {
    which::which("ffmpeg").map_err(|_| AppError::FfmpegNotFound)?;
    let ext = path.extension().unwrap_or_default().to_string_lossy();
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let parent = path.parent().unwrap_or(std::path::Path::new("."));
    let tmp = parent.join(format!(".{}_faststart.{}", stem, ext));

    log::info!("applying faststart to {}", path.display());

    // Use .output() instead of .status() so stderr is actually read
    // (prevents deadlock when ffmpeg's stderr buffer fills up).
    let output = Command::new("ffmpeg")
        .args([
            "-y",
            "-i", &path.to_string_lossy(),
            "-c", "copy",
            "-movflags", "+faststart",
            &tmp.to_string_lossy(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(AppError::Io)?;

    if !output.status.success() {
        let _ = std::fs::remove_file(&tmp);
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!("faststart remux failed: {}, keeping original", stderr.lines().last().unwrap_or("?"));
        return Ok(());
    }
    std::fs::rename(&tmp, path)?;
    log::info!("faststart applied to {}", path.display());
    Ok(())
}

/// Post-process a recording: scale (if needed) + faststart (MP4) or
/// convert (GIF). This is the main entry point called by the GUI/TUI
/// after a recording finishes.
///
/// The scaling step uses ffmpeg's `-vf scale=W:H` with Lanczos filter
/// for good quality. The faststart step moves the moov atom to the
/// start of the MP4 for instant playback in mpv.
pub fn post_process(path: &Path, cfg: &RecordingConfig) {
    let was_scaled = cfg.quality.dimensions(cfg.manual_width, cfg.manual_height).is_some();

    // Step 1: Scale if quality != Original.
    // scale_video already applies faststart for MP4, so we skip the
    // separate faststart step when scaling was done.
    if was_scaled {
        if let Some((w, h)) = cfg.quality.dimensions(cfg.manual_width, cfg.manual_height) {
            if let Err(e) = scale_video(path, w, h) {
                log::warn!("scaling failed: {e}");
                // If scaling failed, still try faststart below.
            }
        }
    }

    // Step 2: Format-specific post-processing (skip faststart if scaling
    // already did it).
    match cfg.video_format {
        VideoFormat::Mp4 => {
            if !was_scaled {
                if let Err(e) = faststart_mp4(path) {
                    log::warn!("faststart failed: {e}");
                }
            }
        }
        VideoFormat::Gif => {
            if let Err(e) = convert_to_gif(path) {
                log::warn!("GIF conversion failed: {e}");
            }
        }
        VideoFormat::Mkv | VideoFormat::Webm => {}
    }
}

/// Scale a video to the target resolution using ffmpeg. The scaling uses
/// the Lanczos filter for good quality at small sizes. The original file
/// is replaced in-place (via a temp file + rename).
///
/// CRITICAL: the temp file must have the SAME extension as the original
/// (e.g. `.mp4`) so ffmpeg can detect the output format. Using `.tmp`
/// causes ffmpeg to fail silently because it doesn't recognize the format.
fn scale_video(path: &Path, width: u32, height: u32) -> AppResult<()> {
    which::which("ffmpeg").map_err(|_| AppError::FfmpegNotFound)?;

    // Build a temp file path with the correct extension so ffmpeg can
    // detect the output container format.
    let ext = path.extension().unwrap_or_default().to_string_lossy();
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let parent = path.parent().unwrap_or(std::path::Path::new("."));
    let tmp = parent.join(format!(".{}_scaling.{}", stem, ext));

    log::info!(
        "scaling {} to {}x{} (temp: {})",
        path.display(), width, height, tmp.display()
    );

    // CRITICAL: use Stdio::null() for stderr, NOT Stdio::piped().
    // With .status(), piped stderr is never read → if ffmpeg outputs more
    // than 64KB to stderr (which it does by default), the process deadlocks
    // and the scaling never completes.
    // Use high-quality encoding settings for the scale step since it's a
    // one-time post-processing operation (not real-time). The original
    // recording was already compressed by wf-recorder, so re-encoding with
    // low quality (ultrafast+CRF23) would compound artifacts and look much
    // worse than a native 360p recording (like YouTube's).
    //
    // - preset=medium: good balance of quality and speed for post-processing
    // - crf=18: visually lossless — no visible quality loss from re-encoding
    // - pix_fmt=yuv420p: maximum compatibility with players
    // - setsar=1: fix sample aspect ratio after scaling
    // - movflags=+faststart: put moov atom at start (saves a separate pass)
    let is_mp4 = ext == "mp4";
    let mut args: Vec<String> = vec![
        "-y".into(),
        "-i".into(),
        path.to_string_lossy().into_owned(),
        "-vf".into(),
        format!("scale={}:{}:flags=lanczos,setsar=1", width, height),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "medium".into(),
        "-crf".into(),
        "18".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "copy".into(),
    ];
    if is_mp4 {
        args.push("-movflags".into());
        args.push("+faststart".into());
    }
    args.push(tmp.to_string_lossy().into_owned());

    let output = Command::new("ffmpeg")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(AppError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_file(&tmp);
        log::error!("ffmpeg scaling failed: {}", stderr);
        return Err(AppError::RecorderExit(
            output.status.code().unwrap_or(-1),
            format!("ffmpeg scaling failed: {}", stderr.lines().last().unwrap_or("unknown")),
        ));
    }

    // Replace the original with the scaled version.
    std::fs::rename(&tmp, path)?;
    log::info!("scaling complete: {} is now {}x{}", path.display(), width, height);
    Ok(())
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_for_mp4_uses_h264_yuv420p() {
        let (c, p) = codec_for_format(VideoFormat::Mp4);
        assert_eq!(c, "libx264");
        assert_eq!(p, "yuv420p");
    }

    #[test]
    fn codec_for_webm_uses_vp9() {
        let (c, _) = codec_for_format(VideoFormat::Webm);
        assert_eq!(c, "libvpx-vp9");
    }

    #[test]
    fn status_label_changes_with_state() {
        let idle = RecorderStatus::Idle;
        assert_eq!(idle.label(), "Inactivo");
        let failed = RecorderStatus::Failed { message: "boom".into() };
        assert!(failed.label().contains("boom"));
    }
}
