//! Error types used across the application.
//!
//! We use [`anyhow::Error`] for ergonomics at the boundaries (CLI, top-level
//! handlers) and a typed [`AppError`] for predictable failures inside library
//! modules so callers can match on the variant if they want to recover.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("No se encontró el ejecutable 'wf-recorder' en el PATH. Instálalo con el gestor de paquetes de tu distro (xbps-install wf-recorder / apt install wf-recorder / pacman -S wf-recorder).")]
    WfRecorderNotFound,

    #[error("No se encontró 'wlr-randr' en el PATH. Es necesario para listar los monitores Wayland disponibles. Instálalo con tu gestor de paquetes.")]
    WlrRandrNotFound,

    #[error("No se encontró 'slurp' en el PATH. Es necesario para seleccionar un área de la pantalla interactivamente.")]
    SlurpNotFound,

    #[error("No se encontró 'ffmpeg' en el PATH. Es necesario para la post-procesamiento de GIF.")]
    FfmpegNotFound,

    #[error("wf-recorder terminó con código {0}: {1}")]
    RecorderExit(i32, String),

    #[error("Error de E/S: {0}")]
    Io(#[from] std::io::Error),

    #[error("Error al parsear la configuración: {0}")]
    ConfigParse(String),
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::ConfigParse(e.to_string())
    }
}

impl From<toml::de::Error> for AppError {
    fn from(e: toml::de::Error) -> Self {
        AppError::ConfigParse(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
