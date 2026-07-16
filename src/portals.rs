//! xdg-desktop-portal integration — GlobalShortcuts only.
//!
//! The FileChooser portal was removed because the GTK GUI uses GTK's native
//! file chooser (which already routes through the portal), and the TUI
//! doesn't need a directory picker (the output path is configured in the
//! GUI or config file).
//!
//! GlobalShortcuts is used to register system-wide hotkeys for
//! start/stop/pause. **Limitation**: `xdg-desktop-portal-wlr` does NOT
//! implement this interface, so on most wlroots compositors this will
//! gracefully fail and the caller falls back to local in-app hotkeys.

use std::sync::Arc;

use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures_util::StreamExt;
use tokio::sync::mpsc as async_mpsc;

/// A single registered shortcut.
#[derive(Debug, Clone)]
pub struct ShortcutDef {
    pub id: String,
    pub description: String,
    pub preferred_trigger: String,
}

/// The default shortcuts we try to register.
pub fn default_shortcuts() -> Vec<ShortcutDef> {
    vec![
        ShortcutDef {
            id: "start".into(),
            description: "Iniciar grabación".into(),
            preferred_trigger: "Ctrl+Alt+R".into(),
        },
        ShortcutDef {
            id: "stop".into(),
            description: "Detener grabación".into(),
            preferred_trigger: "Ctrl+Alt+S".into(),
        },
        ShortcutDef {
            id: "pause".into(),
            description: "Pausar/reanudar grabación".into(),
            preferred_trigger: "Ctrl+Alt+P".into(),
        },
    ]
}

/// Event emitted by the GlobalShortcuts portal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutEvent {
    Activated { id: String },
    Deactivated { id: String },
    Closed,
}

/// Handle returned by [`try_register_global_shortcuts`].
pub struct GlobalShortcutsHandle {
    pub events: async_mpsc::Receiver<ShortcutEvent>,
}

/// Try to register global shortcuts. Returns `None` if the portal is not
/// available. The caller should fall back to local hotkeys in that case.
pub fn try_register_global_shortcuts(shortcuts: Vec<ShortcutDef>) -> Option<GlobalShortcutsHandle> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(1)
        .build()
        .ok()?;

    let (tx, rx) = async_mpsc::channel::<ShortcutEvent>(16);

    rt.spawn(async move {
        let gs = match GlobalShortcuts::new().await {
            Ok(s) => s,
            Err(e) => {
                log::info!("GlobalShortcuts portal unavailable: {e}");
                let _ = tx.send(ShortcutEvent::Closed).await;
                return;
            }
        };
        let gs = Arc::new(gs);

        let session = match gs.create_session(Default::default()).await {
            Ok(s) => s,
            Err(e) => {
                log::info!("GlobalShortcuts create_session failed: {e}");
                let _ = tx.send(ShortcutEvent::Closed).await;
                return;
            }
        };

        let defs: Vec<NewShortcut> = shortcuts
            .iter()
            .map(|s| {
                NewShortcut::new(&s.id, &s.description)
                    .preferred_trigger(s.preferred_trigger.as_str())
            })
            .collect();
        if let Err(e) = gs
            .bind_shortcuts(&session, &defs, None, Default::default())
            .await
        {
            log::info!("GlobalShortcuts bind_shortcuts failed: {e}");
            let _ = tx.send(ShortcutEvent::Closed).await;
            return;
        }

        log::info!("GlobalShortcuts registered: {} shortcuts", defs.len());

        let mut activated_stream = match gs.receive_activated().await {
            Ok(s) => s,
            Err(e) => {
                log::warn!("receive_activated failed: {e}");
                let _ = tx.send(ShortcutEvent::Closed).await;
                return;
            }
        };
        let mut deactivated_stream = match gs.receive_deactivated().await {
            Ok(s) => s,
            Err(e) => {
                log::warn!("receive_deactivated failed: {e}");
                let _ = tx.send(ShortcutEvent::Closed).await;
                return;
            }
        };

        loop {
            tokio::select! {
                Some(ev) = activated_stream.next() => {
                    let id = ev.shortcut_id().to_string();
                    if tx.send(ShortcutEvent::Activated { id }).await.is_err() {
                        break;
                    }
                }
                Some(ev) = deactivated_stream.next() => {
                    let id = ev.shortcut_id().to_string();
                    if tx.send(ShortcutEvent::Deactivated { id }).await.is_err() {
                        break;
                    }
                }
                else => { break; }
            }
        }
        let _ = tx.send(ShortcutEvent::Closed).await;
    });

    std::mem::forget(rt);
    Some(GlobalShortcutsHandle { events: rx })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shortcuts_have_unique_ids() {
        let s = default_shortcuts();
        let mut ids: Vec<_> = s.iter().map(|x| x.id.clone()).collect();
        ids.sort();
        let len = ids.len();
        ids.dedup();
        assert_eq!(len, ids.len());
    }
}
