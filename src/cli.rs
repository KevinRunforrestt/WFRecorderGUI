//! CLI definition (clap derive).
//!
//! The binary has four subcommands:
//!
//! - `gui` (default): launch the GTK3 GUI.
//! - `tui`: launch the terminal UI.
//! - `info`: print diagnostic info (session kind, audio backend, outputs,
//!   audio sources) and exit. Useful for bug reports.
//! - `about`: print version + description and exit.

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "wf-recorder-gui",
    version = crate::version::VERSION,
    author = "wf-recorder-gui contributors",
    about = "Grabador de pantalla GTK3 + TUI para wf-recorder en compositores wlroots",
    long_about = "Un frontend limpio y respetuoso con el tema del sistema para wf-recorder. \
                  Ofrece una GUI GTK3, una TUI en ratatui, un icono de bandeja y \
                  integración con xdg-desktop-portal."
)]
pub struct Cli {
    /// Aumentar verbosidad del log (-v info, -vv debug, -vvv trace).
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Option<Cmd>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Cmd {
    /// Lanzar la GUI GTK3 (por defecto).
    Gui,

    /// Lanzar la interfaz de terminal (TUI).
    Tui,

    /// Imprimir información de diagnóstico y salir (para reportes de bugs).
    Info,

    /// Mostrar información de versión y salir.
    About,
}

impl Cli {
    /// Resolve the effective subcommand, defaulting to `Gui`.
    pub fn effective_cmd(&self) -> Cmd {
        self.command.clone().unwrap_or(Cmd::Gui)
    }
}
