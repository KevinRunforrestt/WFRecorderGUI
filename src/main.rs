//! wf-recorder-gui: GTK3 GUI and TUI for wf-recorder on wlroots compositors.
//!
//! See `README.md` for usage. The binary has three subcommands:
//! - `gui` (default): launch the GTK3 GUI
//! - `tui`: launch the terminal UI
//! - `info`: print diagnostic information and exit

#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::too_many_lines)]

mod audio;
mod cli;
mod config;
mod errors;
mod portals;
mod presets;
mod recorder;
mod shared;
mod tray;
mod ui;
mod version;
mod wayland;

use clap::Parser;

use crate::cli::{Cmd, Cli};
use crate::errors::AppResult;

fn main() {
    // Busybox-style dispatch: if the binary is invoked as `wf-recorder-tui`
    // (e.g. via a symlink), launch the TUI directly without parsing args.
    // This lets distros ship a single binary with two names.
    let argv0_full = std::env::args().next().unwrap_or_default();
    let argv0 = argv0_full.rsplit('/').next().unwrap_or(&argv0_full);
    let force_tui = argv0 == "wf-recorder-tui";

    let cli = Cli::parse();
    init_logging(cli.verbose);

    let cmd = if force_tui { Cmd::Tui } else { cli.effective_cmd() };
    log::info!("starting wf-recorder-gui in {cmd:?} mode (argv0={argv0})");

    let result: AppResult<()> = match cmd {
        Cmd::Gui => ui::gtk_app::run(),
        Cmd::Tui => ui::tui_app::run(),
        Cmd::Info => {
            print_info();
            Ok(())
        }
        Cmd::About => {
            print_about();
            Ok(())
        }
    };

    if let Err(e) = result {
        log::error!("fatal: {e}");
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn init_logging(verbose: u8) {
    let filter = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let mut builder = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(filter));
    builder.format_timestamp(Some(env_logger::TimestampPrecision::Millis));
    builder.init();
}

fn print_info() {
    println!("=== wf-recorder-gui diagnostic info ===");
    println!();
    println!("== Session ==");
    let session = wayland::detect_session();
    println!("Kind: {}", session.label());
    println!("WAYLAND_DISPLAY: {}", std::env::var("WAYLAND_DISPLAY").unwrap_or_default());
    println!("DISPLAY: {}", std::env::var("DISPLAY").unwrap_or_default());
    println!("XDG_SESSION_TYPE: {}", std::env::var("XDG_SESSION_TYPE").unwrap_or_default());
    println!("XDG_CURRENT_DESKTOP: {}", std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default());
    println!();

    println!("== Audio backend ==");
    let backend = audio::detect_backend();
    println!("Detected: {}", backend.label());
    println!();

    println!("== Outputs (wlr-randr) ==");
    match wayland::list_outputs() {
        Ok(outputs) => {
            if outputs.is_empty() {
                println!("(none)");
            } else {
                for o in &outputs {
                    println!("• {} — {} ({})", o.name, o.description, o.resolution_label());
                }
            }
        }
        Err(e) => println!("error: {e}"),
    }
    println!();

    println!("== Audio sources ==");
    let sources = audio::list_sources();
    if sources.is_empty() {
        println!("(none)");
    } else {
        for s in &sources {
            println!("• {} — {}", s.description, s.name);
        }
    }
    println!();

    println!("== External tools ==");
    for tool in &["wf-recorder", "wlr-randr", "slurp", "ffmpeg", "pactl", "wpctl"] {
        let ok = which::which(tool).is_ok();
        println!("• {}: {}", tool, if ok { "✓" } else { "✗" });
    }
    println!();

    println!("== Built-in presets ==");
    for p in presets::builtins() {
        println!("• [{}] {} — {}fps CRF{} {}", p.tier.label(), p.name, p.config.fps, p.config.crf, p.config.video_format.label());
    }
}

fn print_about() {
    println!("{} {}", version::APP_NAME, version::VERSION);
    println!("{}", version::DESCRIPTION);
    println!("Licencia: {}", version::LICENSE);
    println!();
    println!("Características:");
    println!("  • GUI GTK3 sin CSD inspirada en SimpleScreenRecorder");
    println!("  • TUI con pestañas tipo navegador (Config / Logs / Info / About)");
    println!("  • Icono de bandeja: cámara (idle) / punto rojo (grabando) / advertencia (error)");
    println!("  • Menú de bandeja con Iniciar / Detener / Pausar");
    println!("  • Sistema de presets por nivel de hardware (bajos / medios / altos)");
    println!("  • Formatos: MP4 (H.264), WebM (VP9), MKV (H.264), GIF");
    println!("  • Audio: AAC, MP3, Vorbis, Opus (auto-detección PipeWire/PulseAudio)");
    println!("  • Cambiador de FPS (15-144), CRF, bitrate, DMA-BUF, cursor");
    println!("  • Cuenta regresiva editable (0-30 s) con colores");
    println!("  • Nombre de archivo editable");
    println!("  • Botones centrados con icono + texto");
    println!("  • Integración xdg-desktop-portal (FileChooser + GlobalShortcuts)");
    println!("  • Funciona en wlroots (Sway, Wayfire, Labwc, Cage, dwl) y Smithay (Niri)");
    println!();
    println!("Usa 'wf-recorder-gui --help' para ver las opciones.");
}

