//! System tray icon via the StatusNotifierItem D-Bus spec (ksni crate).
//!
//! The tray reflects the current recorder status with three visual states:
//!
//! - **Idle**: a generic "video-display" icon
//! - **Recording**: a red "media-record" icon with a tooltip showing elapsed time
//! - **Failed**: a "dialog-warning" icon
//!
//! Icon names follow the FreeDesktop Icon Naming Spec so they are resolved
//! from whatever icon theme the user has configured (Adwaita, Papirus,
//! Breeze, …). This is the GTK-nature the spec asked us to preserve.

use std::sync::{Arc, Mutex};

use ksni::menu::StandardItem;
use ksni::{Handle, MenuItem, ToolTip, Tray};
use tokio::sync::mpsc as async_mpsc;

use crate::recorder::RecorderStatus;

/// Commands the GUI can send to the tray to request an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayCmd {
    ToggleWindow,
    Start,
    Stop,
    Pause,
    Quit,
}

/// Tray-side state. Cloned cheaply so the GUI thread can update it.
#[derive(Debug, Clone, Default)]
pub struct TrayState {
    pub status: Option<RecorderStatus>,
    pub window_visible: bool,
    /// Whether the recording is currently paused (SIGSTOP). Drives the
    /// "Pausar" / "Reanudar" label in the tray menu.
    pub paused: bool,
}

pub struct AppTray {
    state: Arc<Mutex<TrayState>>,
    cmd_tx: async_mpsc::Sender<TrayCmd>,
}

impl AppTray {
    pub fn new(cmd_tx: async_mpsc::Sender<TrayCmd>, state: Arc<Mutex<TrayState>>) -> Self {
        Self { state, cmd_tx }
    }

    fn icon_name_for(status: &Option<RecorderStatus>) -> &'static str {
        match status {
            Some(RecorderStatus::Recording { .. }) => "media-record",
            Some(RecorderStatus::Stopping) => "media-playback-stop",
            Some(RecorderStatus::Failed { .. }) => "dialog-warning",
            _ => "camera-video",
        }
    }

    fn status_label(status: &Option<RecorderStatus>) -> String {
        match status {
            Some(RecorderStatus::Recording { .. }) => {
                "wf-recorder-gui · Grabando pantalla"
            }
            Some(RecorderStatus::Stopping) => "wf-recorder-gui · Deteniendo…",
            Some(RecorderStatus::Failed { .. }) => "wf-recorder-gui · Error",
            Some(RecorderStatus::Idle) | None => "wf-recorder-gui · Listo para grabar",
        }
        .to_string()
    }
}

impl Tray for AppTray {
    fn id(&self) -> String {
        "wf-recorder-gui".to_string()
    }

    fn icon_name(&self) -> String {
        let state = self.state.lock().map(|s| s.clone()).unwrap_or_default();
        Self::icon_name_for(&state.status).to_string()
    }

    fn title(&self) -> String {
        "wf-recorder-gui".to_string()
    }

    fn tool_tip(&self) -> ToolTip {
        let state = self.state.lock().map(|s| s.clone()).unwrap_or_default();
        let label = Self::status_label(&state.status);
        ToolTip {
            title: "wf-recorder-gui".into(),
            description: label,
            ..ToolTip::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let state = self.state.lock().map(|s| s.clone()).unwrap_or_default();
        let is_recording = state
            .status
            .as_ref()
            .map(|s| s.is_recording())
            .unwrap_or(false);
        let is_paused = state.paused;

        let toggle_label = if state.window_visible {
            "Ocultar ventana"
        } else {
            "Mostrar ventana"
        };
        let toggle_item = MenuItem::Standard(StandardItem {
            label: toggle_label.into(),
            activate: Box::new(move |tray: &mut Self| {
                let _ = tray.cmd_tx.blocking_send(TrayCmd::ToggleWindow);
            }),
            ..StandardItem::default()
        });

        let record_or_stop = if is_recording {
            MenuItem::Standard(StandardItem {
                label: "Detener grabación".into(),
                icon_name: "media-playback-stop".into(),
                activate: Box::new(move |tray: &mut Self| {
                    let _ = tray.cmd_tx.blocking_send(TrayCmd::Stop);
                }),
                ..StandardItem::default()
            })
        } else {
            MenuItem::Standard(StandardItem {
                label: "Iniciar grabación".into(),
                icon_name: "media-record".into(),
                activate: Box::new(move |tray: &mut Self| {
                    let _ = tray.cmd_tx.blocking_send(TrayCmd::Start);
                }),
                ..StandardItem::default()
            })
        };

        // Pause / resume item — only shown while recording.
        let pause_item = if is_recording {
            let (pause_label, pause_icon) = if is_paused {
                ("Reanudar grabación", "media-playback-start")
            } else {
                ("Pausar grabación", "media-playback-pause")
            };
            Some(MenuItem::Standard(StandardItem {
                label: pause_label.into(),
                icon_name: pause_icon.into(),
                activate: Box::new(move |tray: &mut Self| {
                    let _ = tray.cmd_tx.blocking_send(TrayCmd::Pause);
                }),
                ..StandardItem::default()
            }))
        } else {
            None
        };

        let quit_item = MenuItem::Standard(StandardItem {
            label: "Salir".into(),
            icon_name: "application-exit".into(),
            activate: Box::new(move |tray: &mut Self| {
                let _ = tray.cmd_tx.blocking_send(TrayCmd::Quit);
            }),
            ..StandardItem::default()
        });

        let mut menu = vec![
            toggle_item,
            MenuItem::Separator,
            record_or_stop,
        ];
        if let Some(p) = pause_item {
            menu.push(p);
        }
        menu.push(MenuItem::Separator);
        menu.push(quit_item);
        menu
    }
}

/// Spawn the tray in a background thread. Returns:
/// - The shared [`TrayState`] so the GUI can push status updates
/// - A receiver for tray-triggered commands
/// - The ksni [`Handle`] so the GUI can call `update()` when state changes
pub fn spawn() -> (
    Arc<Mutex<TrayState>>,
    async_mpsc::Receiver<TrayCmd>,
    Handle<AppTray>,
) {
    let (cmd_tx, cmd_rx) = async_mpsc::channel::<TrayCmd>(16);
    let state = Arc::new(Mutex::new(TrayState::default()));
    let tray = AppTray::new(cmd_tx, Arc::clone(&state));

    let service = ksni::TrayService::new(tray);
    let handle = service.handle();
    service.spawn();

    (state, cmd_rx, handle)
}

/// Push a new status to the shared tray state and refresh the D-Bus object.
/// Cheap to call: if the status didn't change we skip the D-Bus round-trip.
pub fn refresh(
    state: &Arc<Mutex<TrayState>>,
    handle: &Handle<AppTray>,
    status: RecorderStatus,
) {
    let changed = {
        let mut s = match state.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let changed = s.status.as_ref() != Some(&status);
        if changed {
            s.status = Some(status);
        }
        changed
    };
    if changed {
        handle.update(|_tray| {
            // The Tray impl reads from the shared state, so we don't need
            // to mutate `tray` directly — we just trigger a redraw.
        });
    }
}

/// Update the `window_visible` flag (called when the main window is
/// hidden/shown) and refresh the menu.
pub fn set_window_visible(
    state: &Arc<Mutex<TrayState>>,
    handle: &Handle<AppTray>,
    visible: bool,
) {
    {
        let mut s = match state.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if s.window_visible == visible {
            return;
        }
        s.window_visible = visible;
    }
    handle.update(|_tray| {});
}

/// Update the `paused` flag so the tray menu shows "Reanudar" instead of
/// "Pausar" when the recording is paused via SIGSTOP.
pub fn set_paused(
    state: &Arc<Mutex<TrayState>>,
    handle: &Handle<AppTray>,
    paused: bool,
) {
    {
        let mut s = match state.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if s.paused == paused {
            return;
        }
        s.paused = paused;
    }
    handle.update(|_tray| {});
}
