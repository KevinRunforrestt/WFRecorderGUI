//! GTK3 GUI for wf-recorder — SimpleScreenRecorder-inspired layout.
//!
//! All action buttons use a centered icon+label layout via a vertical GtkBox
//! so they look symmetric regardless of the icon size.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Button, CheckButton, ComboBoxText, Entry,
    FileChooserAction, FileChooserDialog, Frame, Grid, Image, Label, Orientation, ResponseType,
    Scale, SpinButton, ToggleButton,
};

use crate::audio::{detect_backend, list_sources, AudioSource};
use crate::config::{AudioFormat, RecordingConfig, ResourceTier, VideoFormat};
use crate::errors::AppResult;
use crate::presets::{self, Preset};
use crate::recorder::{RecorderController, RecorderHandle, RecorderStatus};
use crate::shared;
use crate::tray::{self, TrayCmd, TrayState};
use crate::version;
use crate::wayland::{self, SessionKind, WlrOutput};
use crate::portals;

struct Widgets {
    preset_combo: ComboBoxText,
    tier_combo: ComboBoxText,
    output_combo: ComboBoxText,
    output_refresh_button: Button,
    region_button: Button,
    region_label: Label,
    audio_combo: ComboBoxText,
    audio_refresh_button: Button,
    audio_format_combo: ComboBoxText,
    video_format_combo: ComboBoxText,
    fps_spin: SpinButton,
    crf_scale: Scale,
    crf_value_label: Label,
    video_bitrate_spin: SpinButton,
    audio_bitrate_spin: SpinButton,
    output_dir_button: Button,
    filename_entry: Entry,
    dmabuf_check: CheckButton,
    quality_combo: ComboBoxText,
    manual_width_spin: SpinButton,
    manual_height_spin: SpinButton,
    manual_res_box: GtkBox,
    cursor_check: CheckButton,
    countdown_spin: SpinButton,
    record_button: Button,
    stop_button: Button,
    pause_button: ToggleButton,
    about_button: Button,
    keybinds_button: Button,
    status_label: Label,
    elapsed_label: Label,
    countdown_label: Label,
    output_path_label: Label,
    #[allow(dead_code)]
    warning_banner: Label,
    save_preset_button: Button,
    filename_label: Label,
    #[allow(dead_code)]
    backend_label: Label,
    output_error_label: Label,
    audio_error_label: Label,
}

struct AppState {
    config: RecordingConfig,
    presets: Vec<Preset>,
    outputs: Vec<WlrOutput>,
    audio_sources: Vec<AudioSource>,
    session: SessionKind,
    handle: Option<RecorderHandle>,
    paused: bool,
    geometry: String,
    recording_started: Option<Instant>,
    countdown_ends_at: Option<Instant>,
    /// Customizable keyboard shortcuts (mirrored from AppConfig).
    config_keybinds: crate::config::Keybinds,
}

pub fn run() -> AppResult<()> {
    if std::env::var("GTK_CSD").is_err() {
        std::env::set_var("GTK_CSD", "0");
    }

    let app = Application::builder()
        .application_id("io.github.wf-recorder-gui")
        .flags(gio::ApplicationFlags::empty())
        .build();

    app.connect_activate(move |a| {
        if let Err(e) = build_and_show(a) {
            log::error!("failed to build UI: {e}");
            show_fatal_dialog(&e.to_string());
        }
    });

    app.run();
    Ok(())
}

fn build_and_show(app: &Application) -> AppResult<()> {
    let config = crate::config::AppConfig::load_or_default();
    let session = wayland::detect_session();
    let outputs = if session.is_wlroots() {
        wayland::list_outputs().unwrap_or_default()
    } else {
        Vec::new()
    };
    let audio_sources = list_sources();
    let presets = presets::all();

    let state = Rc::new(RefCell::new(AppState {
        config: config.recording.clone(),
        presets,
        outputs,
        audio_sources,
        session,
        handle: None,
        paused: false,
        geometry: String::new(),
        recording_started: None,
        countdown_ends_at: None,
        config_keybinds: config.keybinds.clone(),
    }));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("WFRecorderGUI")
        .default_width(580)
        .default_height(600)
        .build();

    let outer = GtkBox::new(Orientation::Vertical, 0);
    window.add(&outer);

    // Warning banner
    let warning_banner = Label::new(None);
    warning_banner.set_use_markup(true);
    warning_banner.set_line_wrap(true);
    warning_banner.set_xalign(0.0);
    warning_banner.set_margins(4, 4, 8, 8);
    if let Some(msg) = session.warning() {
        let escaped = glib::markup_escape_text(msg);
        warning_banner.set_markup(&format!(
            "<span foreground=\"#b35900\" weight=\"bold\">⚠ {}</span>",
            escaped
        ));
        warning_banner.set_visible(true);
    } else {
        warning_banner.set_visible(false);
    }
    outer.add(&warning_banner);

    // Single scrolled window with all settings (compact, SSR-style)
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    let settings_vbox = GtkBox::new(Orientation::Vertical, 4);
    settings_vbox.set_margins(4, 4, 8, 8);
    scrolled.add(&settings_vbox);
    outer.pack_start(&scrolled, true, true, 0);

    // ===== Profile bar =====
    let profile_frame = make_frame("Perfil");
    let profile_box = GtkBox::new(Orientation::Horizontal, 6);
    profile_box.set_margins(6, 6, 8, 8);
    let preset_combo = ComboBoxText::new();
    preset_combo.append_text("(personalizado)");
    for p in &state.borrow().presets {
        preset_combo.append_text(&p.name);
    }
    preset_combo.set_active(Some(0));
    preset_combo.set_hexpand(true);
    let tier_label = Label::new(Some("Tier:"));
    let tier_combo = ComboBoxText::new();
    for t in ResourceTier::ALL {
        tier_combo.append_text(t.label());
    }
    tier_combo.set_active(Some(
        ResourceTier::ALL
            .iter()
            .position(|t| *t == config.tier_filter)
            .unwrap_or(1) as u32,
    ));
    let save_preset_button = Button::with_label("Guardar…");
    save_preset_button.set_tooltip_text(Some("Guardar configuración actual como preset"));
    profile_box.add(&preset_combo);
    profile_box.add(&tier_label);
    profile_box.add(&tier_combo);
    profile_box.add(&save_preset_button);
    profile_frame.add(&profile_box);
    settings_vbox.add(&profile_frame);

    // ===== File / output section =====
    let file_frame = make_frame("Archivo de salida");
    let file_grid = make_grid();
    let dir_label = Label::new(Some("Carpeta:"));
    dir_label.set_halign(gtk::Align::End);
    let output_dir_button = Button::with_label(&config.recording.output_dir.display().to_string());
    output_dir_button.set_tooltip_text(Some("Elegir carpeta de salida"));
    output_dir_button.set_hexpand(true);
    output_dir_button.set_halign(gtk::Align::Fill);
    file_grid.attach(&dir_label, 0, 0, 1, 1);
    file_grid.attach(&output_dir_button, 1, 0, 1, 1);
    // Filename entry — editable, like the folder button.
    let fname_label = Label::new(Some("Nombre:"));
    fname_label.set_halign(gtk::Align::End);
    let filename_entry = Entry::new();
    filename_entry.set_placeholder_text(Some("Recording_2024-07-12_18-30-45 (auto si vacío)"));
    filename_entry.set_text(&config.recording.custom_filename);
    filename_entry.set_tooltip_text(Some(
        "Nombre del archivo (sin extensión). Si lo dejas vacío, se generará automáticamente con la fecha y hora actuales. La extensión se añade según el formato elegido.",
    ));
    filename_entry.set_hexpand(true);
    let filename_preview = crate::config::default_filename(&config.recording);
    let filename_label = Label::new(Some(&filename_preview));
    filename_label.set_halign(gtk::Align::Start);
    filename_label.set_hexpand(true);
    filename_label.set_ellipsize(pango::EllipsizeMode::Middle);
    filename_label.set_tooltip_text(Some("Vista previa del nombre final"));
    filename_label.set_use_markup(true);
    let filename_box = GtkBox::new(Orientation::Vertical, 2);
    filename_box.add(&filename_entry);
    filename_box.add(&filename_label);
    file_grid.attach(&fname_label, 0, 1, 1, 1);
    file_grid.attach(&filename_box, 1, 1, 1, 1);
    let fmt_label = Label::new(Some("Formato:"));
    fmt_label.set_halign(gtk::Align::End);
    let video_format_combo = ComboBoxText::new();
    for fmt in VideoFormat::ALL {
        video_format_combo.append_text(fmt.label());
    }
    let vf_idx = VideoFormat::ALL
        .iter()
        .position(|f| *f == config.recording.video_format)
        .unwrap_or(0);
    video_format_combo.set_active(Some(vf_idx as u32));
    video_format_combo.set_hexpand(true);
    file_grid.attach(&fmt_label, 0, 2, 1, 1);
    file_grid.attach(&video_format_combo, 1, 2, 1, 1);
    file_frame.add(&file_grid);
    settings_vbox.add(&file_frame);

    // ===== Video section =====
    let video_frame = make_frame("Video");
    let video_grid = make_grid();
    let fps_label = Label::new(Some("FPS:"));
    fps_label.set_halign(gtk::Align::End);
    let fps_spin = SpinButton::with_range(15.0, 144.0, 1.0);
    fps_spin.set_value(config.recording.fps as f64);
    fps_spin.set_hexpand(true);
    video_grid.attach(&fps_label, 0, 0, 1, 1);
    video_grid.attach(&fps_spin, 1, 0, 1, 1);
    let crf_label = Label::new(Some("Calidad (CRF):"));
    crf_label.set_halign(gtk::Align::End);
    let crf_scale = Scale::with_range(Orientation::Horizontal, 0.0, 51.0, 1.0);
    crf_scale.set_value(config.recording.crf as f64);
    crf_scale.set_hexpand(true);
    crf_scale.set_draw_value(false);
    let crf_value_label = Label::new(Some(&config.recording.crf.to_string()));
    crf_value_label.set_width_chars(3);
    crf_value_label.set_halign(gtk::Align::End);
    video_grid.attach(&crf_label, 0, 1, 1, 1);
    video_grid.attach(&crf_scale, 1, 1, 1, 1);
    video_grid.attach(&crf_value_label, 2, 1, 1, 1);
    let vbr_label = Label::new(Some("Video bitrate (kbps):"));
    vbr_label.set_halign(gtk::Align::End);
    let video_bitrate_spin = SpinButton::with_range(0.0, 50000.0, 100.0);
    video_bitrate_spin.set_value(config.recording.video_bitrate as f64);
    video_bitrate_spin.set_hexpand(true);
    video_grid.attach(&vbr_label, 0, 2, 1, 1);
    video_grid.attach(&video_bitrate_spin, 1, 2, 1, 1);

    // Quality / resolution switcher
    let quality_label = Label::new(Some("Calidad:"));
    quality_label.set_halign(gtk::Align::End);
    let quality_combo = ComboBoxText::new();
    for q in crate::config::VideoQuality::ALL {
        quality_combo.append_text(q.label());
    }
    let q_idx = crate::config::VideoQuality::ALL
        .iter()
        .position(|q| *q == config.recording.quality)
        .unwrap_or(0);
    quality_combo.set_active(Some(q_idx as u32));
    quality_combo.set_tooltip_text(Some(
        "Resolución del video de salida. La pantalla se captura a resolución nativa y se escala en post-procesado. Resoluciones más bajas = archivos más pequeños y reproducción más fluida en PCs modestos."
    ));
    quality_combo.set_hexpand(true);
    video_grid.attach(&quality_label, 0, 3, 1, 1);
    video_grid.attach(&quality_combo, 1, 3, 1, 1);

    // Manual resolution SpinButtons — only enabled when quality == Manual.
    let manual_res_label = Label::new(Some("Resolución manual:"));
    manual_res_label.set_halign(gtk::Align::End);
    let manual_width_spin = SpinButton::with_range(16.0, 7680.0, 16.0);
    manual_width_spin.set_value(config.recording.manual_width as f64);
    manual_width_spin.set_tooltip_text(Some("Ancho en píxeles (16-7680)"));
    let x_label = Label::new(Some("×"));
    let manual_height_spin = SpinButton::with_range(16.0, 4320.0, 16.0);
    manual_height_spin.set_value(config.recording.manual_height as f64);
    manual_height_spin.set_tooltip_text(Some("Alto en píxeles (16-4320)"));
    let manual_res_box = GtkBox::new(Orientation::Horizontal, 4);
    manual_res_box.add(&manual_width_spin);
    manual_res_box.add(&x_label);
    manual_res_box.add(&manual_height_spin);
    // Disable by default; enabled when quality_combo selects Manual.
    let is_manual = config.recording.quality == crate::config::VideoQuality::Manual;
    manual_res_box.set_sensitive(is_manual);
    video_grid.attach(&manual_res_label, 0, 4, 1, 1);
    video_grid.attach(&manual_res_box, 1, 4, 1, 1);

    let dmabuf_check = CheckButton::with_label("DMA-BUF (más eficiente)");
    dmabuf_check.set_active(config.recording.use_dmabuf);
    dmabuf_check.set_tooltip_text(Some(
        "DMA-BUF es un mecanismo del kernel de Linux que permite compartir buffers de video entre procesos sin copiar los datos. En wf-recorder, habilitarlo significa que la pantalla capturada se pasa directamente al codificador de video sin pasar por la CPU.\n\n• Activado (recomendado): menor uso de CPU, mayor FPS, ideal para grabaciones largas o de alta resolución.\n• Desactivado: útil si notas corrupción de video o colores incorrectos en algunas configuraciones híbridas."
    ));
    let cursor_check = CheckButton::with_label("Grabar cursor");
    cursor_check.set_active(config.recording.record_cursor);
    let toggles_box = GtkBox::new(Orientation::Horizontal, 12);
    toggles_box.add(&dmabuf_check);
    toggles_box.add(&cursor_check);
    video_grid.attach(&toggles_box, 0, 5, 3, 1);

    // Countdown — integrated into Video frame, no separate section
    let countdown_label_top = Label::new(Some("Cuenta regresiva (s):"));
    countdown_label_top.set_halign(gtk::Align::End);
    let countdown_spin = SpinButton::with_range(0.0, 30.0, 1.0);
    countdown_spin.set_value(config.recording.countdown_secs as f64);
    countdown_spin.set_tooltip_text(Some(
        "Si es mayor que 0, al pulsar Grabar se mostrará una cuenta regresiva antes de iniciar la grabación.",
    ));
    let countdown_label = Label::new(Some(""));
    countdown_label.set_use_markup(true);
    countdown_label.set_halign(gtk::Align::Start);
    countdown_label.set_hexpand(true);
    let countdown_box_row = GtkBox::new(Orientation::Horizontal, 6);
    countdown_box_row.add(&countdown_spin);
    countdown_box_row.add(&countdown_label);
    video_grid.attach(&countdown_label_top, 0, 6, 1, 1);
    video_grid.attach(&countdown_box_row, 1, 6, 2, 1);

    video_frame.add(&video_grid);
    settings_vbox.add(&video_frame);

    // ===== Audio section =====
    let audio_frame = make_frame("Audio");
    let audio_grid = make_grid();
    let bk_label = Label::new(Some("Backend:"));
    bk_label.set_halign(gtk::Align::End);
    let backend_label = Label::new(Some(detect_backend().label()));
    backend_label.set_halign(gtk::Align::Start);
    backend_label.set_hexpand(true);
    audio_grid.attach(&bk_label, 0, 0, 1, 1);
    audio_grid.attach(&backend_label, 1, 0, 1, 1);
    let src_label = Label::new(Some("Fuente:"));
    src_label.set_halign(gtk::Align::End);
    let audio_combo = ComboBoxText::new();
    refresh_audio_combo(&audio_combo, &state.borrow().audio_sources);
    audio_combo.set_hexpand(true);
    let audio_refresh_button = Button::with_label("⟳");
    audio_refresh_button.set_tooltip_text(Some("Refrescar lista de fuentes de audio"));
    let audio_source_box = GtkBox::new(Orientation::Horizontal, 4);
    audio_source_box.add(&audio_combo);
    audio_source_box.add(&audio_refresh_button);
    audio_grid.attach(&src_label, 0, 1, 1, 1);
    audio_grid.attach(&audio_source_box, 1, 1, 1, 1);
    let audio_error_label = Label::new(None);
    audio_error_label.set_use_markup(true);
    audio_error_label.set_xalign(0.0);
    audio_error_label.set_halign(gtk::Align::Start);
    audio_error_label.set_visible(false);
    audio_grid.attach(&audio_error_label, 0, 2, 3, 1);
    let acodec_label = Label::new(Some("Códec:"));
    acodec_label.set_halign(gtk::Align::End);
    let audio_format_combo = ComboBoxText::new();
    for fmt in AudioFormat::ALL {
        audio_format_combo.append_text(fmt.label());
    }
    let af_idx = AudioFormat::ALL
        .iter()
        .position(|f| *f == config.recording.audio_format)
        .unwrap_or(0);
    audio_format_combo.set_active(Some(af_idx as u32));
    audio_format_combo.set_hexpand(true);
    audio_grid.attach(&acodec_label, 0, 3, 1, 1);
    audio_grid.attach(&audio_format_combo, 1, 3, 1, 1);
    let abr_label = Label::new(Some("Audio bitrate (kbps):"));
    abr_label.set_halign(gtk::Align::End);
    let audio_bitrate_spin = SpinButton::with_range(32.0, 512.0, 16.0);
    audio_bitrate_spin.set_value(config.recording.audio_bitrate as f64);
    audio_bitrate_spin.set_hexpand(true);
    audio_grid.attach(&abr_label, 0, 4, 1, 1);
    audio_grid.attach(&audio_bitrate_spin, 1, 4, 1, 1);
    audio_frame.add(&audio_grid);
    settings_vbox.add(&audio_frame);

    // ===== Input section =====
    let input_frame = make_frame("Entrada de pantalla");
    let input_grid = make_grid();
    let mon_label = Label::new(Some("Monitor:"));
    mon_label.set_halign(gtk::Align::End);
    let output_combo = ComboBoxText::new();
    refresh_output_combo(&output_combo, &state.borrow().outputs);
    output_combo.set_hexpand(true);
    let output_refresh_button = Button::with_label("⟳");
    output_refresh_button.set_tooltip_text(Some("Refrescar lista de monitores"));
    let output_box = GtkBox::new(Orientation::Horizontal, 4);
    output_box.add(&output_combo);
    output_box.add(&output_refresh_button);
    input_grid.attach(&mon_label, 0, 0, 1, 1);
    input_grid.attach(&output_box, 1, 0, 1, 1);
    let output_error_label = Label::new(None);
    output_error_label.set_use_markup(true);
    output_error_label.set_xalign(0.0);
    output_error_label.set_halign(gtk::Align::Start);
    output_error_label.set_visible(false);
    input_grid.attach(&output_error_label, 0, 1, 3, 1);
    let reg_label = Label::new(Some("Región:"));
    reg_label.set_halign(gtk::Align::End);
    let region_button = Button::with_label("Seleccionar región…");
    region_button.set_tooltip_text(Some("Usa slurp para elegir un área de la pantalla"));
    let region_label = Label::new(Some("(pantalla completa)"));
    region_label.set_halign(gtk::Align::Start);
    region_label.set_hexpand(true);
    let region_box = GtkBox::new(Orientation::Horizontal, 6);
    region_box.add(&region_button);
    region_box.add(&region_label);
    input_grid.attach(&reg_label, 0, 2, 1, 1);
    input_grid.attach(&region_box, 1, 2, 1, 1);
    input_frame.add(&input_grid);
    settings_vbox.add(&input_frame);

    // ===== Action bar (bottom) — centered buttons =====
    let action_bar = GtkBox::new(Orientation::Horizontal, 6);
    action_bar.set_margins(6, 6, 10, 10);
    let record_button = make_button("media-record", "Grabar", "Iniciar grabación (Ctrl+Alt+R)");
    record_button.style_context().add_class("suggested-action");
    record_button.set_hexpand(true);
    let stop_button = make_button("media-playback-stop", "Detener", "Detener y guardar (Ctrl+Alt+S)");
    stop_button.set_sensitive(false);
    stop_button.set_hexpand(true);
    let pause_button = make_toggle_button("media-playback-pause", "Pausar", "Pausar / reanudar (Ctrl+Alt+P)");
    pause_button.set_sensitive(false);
    pause_button.set_hexpand(true);
    let about_button = make_button("dialog-information", "Acerca de", "Información del programa");
    about_button.set_hexpand(false);
    let keybinds_button = make_button("preferences-desktop-keyboard", "Atajos", "Personalizar atajos de teclado");
    keybinds_button.set_hexpand(false);
    action_bar.add(&record_button);
    action_bar.add(&stop_button);
    action_bar.add(&pause_button);
    action_bar.add(&keybinds_button);
    action_bar.add(&about_button);
    outer.add(&action_bar);

    // ===== Status bar =====
    let status_bar = GtkBox::new(Orientation::Horizontal, 12);
    status_bar.set_margins(4, 6, 10, 10);
    let status_label = Label::new(Some("Inactivo"));
    status_label.set_halign(gtk::Align::Start);
    // Elapsed timer — monospace + bold for clarity.
    let elapsed_label = Label::new(Some("00:00:00"));
    elapsed_label.set_halign(gtk::Align::Start);
    elapsed_label.set_use_markup(true);
    elapsed_label.set_markup("<span font_family='monospace' weight='bold' size='large'>00:00:00</span>");
    elapsed_label.set_tooltip_text(Some("Tiempo de grabación"));
    let output_path_label = Label::new(None);
    output_path_label.set_ellipsize(pango::EllipsizeMode::Middle);
    output_path_label.set_hexpand(true);
    output_path_label.set_xalign(0.0);
    output_path_label.set_halign(gtk::Align::Start);
    output_path_label.set_tooltip_text(Some(""));
    status_bar.add(&status_label);
    status_bar.add(&elapsed_label);
    status_bar.add(&output_path_label);
    outer.add(&status_bar);

    let widgets = Rc::new(RefCell::new(Widgets {
        preset_combo,
        tier_combo,
        output_combo,
        output_refresh_button,
        region_button,
        region_label,
        audio_combo,
        audio_refresh_button,
        audio_format_combo,
        video_format_combo,
        fps_spin,
        crf_scale,
        crf_value_label,
        video_bitrate_spin,
        audio_bitrate_spin,
        output_dir_button,
        filename_entry,
        dmabuf_check,
        quality_combo,
        manual_width_spin,
        manual_height_spin,
        manual_res_box,
        cursor_check,
        countdown_spin,
        record_button: record_button.clone(),
        stop_button: stop_button.clone(),
        pause_button: pause_button.clone(),
        about_button: about_button.clone(),
        keybinds_button: keybinds_button.clone(),
        status_label: status_label.clone(),
        elapsed_label: elapsed_label.clone(),
        countdown_label: countdown_label.clone(),
        output_path_label: output_path_label.clone(),
        warning_banner: warning_banner.clone(),
        save_preset_button: save_preset_button.clone(),
        filename_label: filename_label.clone(),
        backend_label: backend_label.clone(),
        output_error_label,
        audio_error_label,
    }));

    sync_widgets_from_config(&widgets, &state.borrow().config, &state.borrow().outputs, &state.borrow().audio_sources);

    // Tray
    let (tray_state, tray_rx, tray_handle) = tray::spawn();
    {
        let st = state.borrow();
        tray::set_window_visible(&tray_state, &tray_handle, true);
        if let Some(s) = st.handle.as_ref().map(RecorderHandle::status) {
            tray::refresh(&tray_state, &tray_handle, s);
        }
    }
    let tray_state_for_gui = Arc::clone(&tray_state);
    let tray_handle_for_gui = tray_handle.clone();

    #[allow(deprecated)]
    let (tray_cmd_tx, tray_cmd_rx) =
        glib::MainContext::channel::<TrayCmd>(glib::Priority::default());
    std::thread::spawn(move || {
        let mut rx = tray_rx;
        while let Some(cmd) = rx.blocking_recv() {
            if tray_cmd_tx.send(cmd).is_err() {
                break;
            }
        }
    });

    wire_signals(&window, &state, &widgets, &tray_state_for_gui, &tray_handle_for_gui);

    {
        let state = Rc::clone(&state);
        let widgets = Rc::clone(&widgets);
        let window = window.clone();
        let tray_state = Arc::clone(&tray_state_for_gui);
        let tray_handle = tray_handle_for_gui.clone();
        tray_cmd_rx.attach(
            None,
            glib::clone!(@strong window => move |cmd: TrayCmd| {
                handle_tray_cmd(cmd, &window, &state, &widgets, &tray_state, &tray_handle);
                glib::ControlFlow::Continue
            }),
        );
    }

    // GlobalShortcuts (best-effort)
    let shortcuts = portals::default_shortcuts();
    if let Some(gs_handle) = portals::try_register_global_shortcuts(shortcuts) {
        #[allow(deprecated)]
        let (gs_tx, gs_rx) =
            glib::MainContext::channel::<portals::ShortcutEvent>(glib::Priority::default());
        std::thread::spawn(move || {
            let mut rx = gs_handle.events;
            while let Some(ev) = rx.blocking_recv() {
                if gs_tx.send(ev).is_err() {
                    break;
                }
            }
        });
        let state = Rc::clone(&state);
        let widgets = Rc::clone(&widgets);
        let tray_state = Arc::clone(&tray_state_for_gui);
        let tray_handle = tray_handle_for_gui.clone();
        gs_rx.attach(
            None,
            glib::clone!(@strong window => move |ev| {
                handle_shortcut_event(ev, &window, &state, &widgets, &tray_state, &tray_handle);
                glib::ControlFlow::Continue
            }),
        );
    } else {
        log::info!("GlobalShortcuts not available; falling back to local hotkeys");
    }

    // Poll recorder every 250ms.
    {
        let state = Rc::clone(&state);
        let widgets = Rc::clone(&widgets);
        let tray_state = Arc::clone(&tray_state_for_gui);
        let tray_handle = tray_handle_for_gui.clone();
        glib::timeout_add_local(Duration::from_millis(250), move || {
            poll_recorder(&state, &widgets, &tray_state, &tray_handle);
            glib::ControlFlow::Continue
        });
    }

    // Save config on close
    {
        let state = Rc::clone(&state);
        let config_to_save = Rc::new(RefCell::new(config.clone()));
        window.connect_delete_event(move |_, _| {
            let st = state.borrow();
            config_to_save.borrow_mut().recording = st.config.clone();
            config_to_save.borrow_mut().keybinds = st.config_keybinds.clone();
            if let Err(e) = config_to_save.borrow().save() {
                log::warn!("failed to save config on close: {e}");
            }
            glib::Propagation::Proceed
        });
    }

    window.show_all();
    if state.borrow().session.warning().is_none() {
        warning_banner.set_visible(false);
    }
    Ok(())
}

/// Build a button with a centered icon+label layout. GTK's default
/// `set_image` + `set_label` puts the icon to the left of the label, which
/// looks asymmetric when buttons have different label lengths. We wrap
/// both in a vertical Box so they stack centered.
fn make_button(icon_name: &str, label: &str, tooltip: &str) -> Button {
    let btn = Button::new();
    let vbox = GtkBox::new(Orientation::Horizontal, 6);
    vbox.set_halign(gtk::Align::Center);
    vbox.set_valign(gtk::Align::Center);
    let img = Image::from_icon_name(Some(icon_name), gtk::IconSize::Button);
    let lbl = Label::new(Some(label));
    vbox.add(&img);
    vbox.add(&lbl);
    btn.add(&vbox);
    btn.set_tooltip_text(Some(tooltip));
    btn
}

/// Same as `make_button` but returns a ToggleButton (for pause/resume).
fn make_toggle_button(icon_name: &str, label: &str, tooltip: &str) -> ToggleButton {
    let btn = ToggleButton::new();
    let vbox = GtkBox::new(Orientation::Horizontal, 6);
    vbox.set_halign(gtk::Align::Center);
    vbox.set_valign(gtk::Align::Center);
    let img = Image::from_icon_name(Some(icon_name), gtk::IconSize::Button);
    let lbl = Label::new(Some(label));
    vbox.add(&img);
    vbox.add(&lbl);
    btn.add(&vbox);
    btn.set_tooltip_text(Some(tooltip));
    btn
}

/// Helper trait to set all four margins at once.
trait MarginSetter {
    fn set_margins(&self, top: i32, bottom: i32, start: i32, end: i32);
}
impl<W: IsA<gtk::Widget>> MarginSetter for W {
    fn set_margins(&self, top: i32, bottom: i32, start: i32, end: i32) {
        self.set_margin_top(top);
        self.set_margin_bottom(bottom);
        self.set_margin_start(start);
        self.set_margin_end(end);
    }
}

fn make_frame(title: &str) -> Frame {
    let f = Frame::new(Some(title));
    f.set_label_align(0.0, 0.5);
    f.set_shadow_type(gtk::ShadowType::In);
    f
}

fn make_grid() -> Grid {
    let g = Grid::new();
    g.set_column_spacing(8);
    g.set_row_spacing(6);
    g.set_margins(6, 6, 8, 8);
    g
}

fn refresh_output_combo(combo: &ComboBoxText, outputs: &[WlrOutput]) {
    combo.remove_all();
    combo.append_text("(automático)");
    for o in outputs {
        let label = format!("{} · {}", o.name, o.resolution_label());
        combo.append_text(&label);
    }
    combo.set_active(Some(0));
}

fn refresh_audio_combo(combo: &ComboBoxText, sources: &[AudioSource]) {
    combo.remove_all();
    combo.append_text("(sin audio)");
    for s in sources {
        combo.append_text(&s.description);
    }
    combo.set_active(Some(0));
}

fn update_filename_preview(widgets: &Rc<RefCell<Widgets>>, cfg: &RecordingConfig) {
    let fname = crate::config::default_filename(cfg);
    let escaped = glib::markup_escape_text(&fname);
    let label_clone = widgets.borrow().filename_label.clone();
    label_clone.set_markup(&format!("<span foreground='#888888' size='small'>→ {}</span>", escaped));
}

fn sync_widgets_from_config(
    widgets: &Rc<RefCell<Widgets>>,
    cfg: &RecordingConfig,
    outputs: &[WlrOutput],
    audio_sources: &[AudioSource],
) {
    let (
        video_format_combo, audio_format_combo, fps_spin, crf_scale, crf_value_label,
        video_bitrate_spin, audio_bitrate_spin, output_dir_button, dmabuf_check,
        quality_combo, manual_width_spin, manual_height_spin, manual_res_box,
        cursor_check, tier_combo, output_combo, audio_combo, countdown_spin,
        filename_entry,
    ) = {
        let w = widgets.borrow();
        (
            w.video_format_combo.clone(), w.audio_format_combo.clone(), w.fps_spin.clone(),
            w.crf_scale.clone(), w.crf_value_label.clone(), w.video_bitrate_spin.clone(),
            w.audio_bitrate_spin.clone(), w.output_dir_button.clone(), w.dmabuf_check.clone(),
            w.quality_combo.clone(), w.manual_width_spin.clone(), w.manual_height_spin.clone(),
            w.manual_res_box.clone(), w.cursor_check.clone(),
            w.tier_combo.clone(), w.output_combo.clone(),
            w.audio_combo.clone(), w.countdown_spin.clone(), w.filename_entry.clone(),
        )
    };

    let vf_idx = VideoFormat::ALL.iter().position(|f| *f == cfg.video_format).unwrap_or(0);
    video_format_combo.set_active(Some(vf_idx as u32));
    let af_idx = AudioFormat::ALL.iter().position(|f| *f == cfg.audio_format).unwrap_or(0);
    audio_format_combo.set_active(Some(af_idx as u32));
    fps_spin.set_value(cfg.fps as f64);
    crf_scale.set_value(cfg.crf as f64);
    crf_value_label.set_label(&cfg.crf.to_string());
    video_bitrate_spin.set_value(cfg.video_bitrate as f64);
    audio_bitrate_spin.set_value(cfg.audio_bitrate as f64);
    output_dir_button.set_label(&cfg.output_dir.display().to_string());
    dmabuf_check.set_active(cfg.use_dmabuf);
    cursor_check.set_active(cfg.record_cursor);
    countdown_spin.set_value(cfg.countdown_secs as f64);
    filename_entry.set_text(&cfg.custom_filename);

    let q_idx = crate::config::VideoQuality::ALL
        .iter()
        .position(|q| *q == cfg.quality)
        .unwrap_or(0);
    quality_combo.set_active(Some(q_idx as u32));
    manual_width_spin.set_value(cfg.manual_width as f64);
    manual_height_spin.set_value(cfg.manual_height as f64);
    manual_res_box.set_sensitive(cfg.quality == crate::config::VideoQuality::Manual);

    let tier = shared::infer_tier(cfg);
    let tier_idx = ResourceTier::ALL.iter().position(|t| *t == tier).unwrap_or(1);
    tier_combo.set_active(Some(tier_idx as u32));

    if let Some(idx) = outputs.iter().position(|o| o.name == cfg.output) {
        output_combo.set_active(Some((idx + 1) as u32));
    } else {
        output_combo.set_active(Some(0));
    }
    if let Some(idx) = audio_sources.iter().position(|a| a.name == cfg.audio_source) {
        audio_combo.set_active(Some((idx + 1) as u32));
    } else {
        audio_combo.set_active(Some(0));
    }

    update_filename_preview(widgets, cfg);
}

fn wire_signals(
    window: &ApplicationWindow,
    state: &Rc<RefCell<AppState>>,
    widgets: &Rc<RefCell<Widgets>>,
    tray_state: &Arc<Mutex<TrayState>>,
    tray_handle: &ksni::Handle<tray::AppTray>,
) {
    let (
        tier_combo, preset_combo, output_combo, output_refresh_button, region_button, region_label,
        audio_combo, audio_refresh_button, video_format_combo, audio_format_combo, fps_spin,
        crf_scale, crf_value_label, video_bitrate_spin, audio_bitrate_spin, output_dir_button,
        dmabuf_check, quality_combo, manual_width_spin, manual_height_spin, manual_res_box,
        cursor_check, countdown_spin,
        record_button, stop_button, pause_button,
        about_button, keybinds_button, save_preset_button, output_error_label, audio_error_label, filename_entry,
    ) = {
        let w = widgets.borrow();
        (
            w.tier_combo.clone(), w.preset_combo.clone(), w.output_combo.clone(),
            w.output_refresh_button.clone(), w.region_button.clone(), w.region_label.clone(),
            w.audio_combo.clone(), w.audio_refresh_button.clone(), w.video_format_combo.clone(),
            w.audio_format_combo.clone(), w.fps_spin.clone(), w.crf_scale.clone(),
            w.crf_value_label.clone(), w.video_bitrate_spin.clone(), w.audio_bitrate_spin.clone(),
            w.output_dir_button.clone(), w.dmabuf_check.clone(),
            w.quality_combo.clone(), w.manual_width_spin.clone(), w.manual_height_spin.clone(),
            w.manual_res_box.clone(), w.cursor_check.clone(),
            w.countdown_spin.clone(), w.record_button.clone(), w.stop_button.clone(),
            w.pause_button.clone(), w.about_button.clone(), w.keybinds_button.clone(),
            w.save_preset_button.clone(),
            w.output_error_label.clone(), w.audio_error_label.clone(), w.filename_entry.clone(),
        )
    };

    // Tier combo: filter presets.
    {
        let state = Rc::clone(state);
        let preset_combo = preset_combo.clone();
        let tier_combo_inner = tier_combo.clone();
        tier_combo.connect_changed(move |_| {
            let idx = tier_combo_inner.active().unwrap_or(0) as usize;
            let tier = ResourceTier::ALL.get(idx).copied().unwrap_or(ResourceTier::Medium);
            let filtered = presets::for_tier(tier);
            preset_combo.remove_all();
            preset_combo.append_text("(personalizado)");
            for p in &filtered {
                preset_combo.append_text(&p.name);
            }
            preset_combo.set_active(Some(0));
            state.borrow_mut().presets = filtered;
        });
    }

    // Preset combo: apply.
    {
        let state = Rc::clone(state);
        let widgets = Rc::clone(widgets);
        let preset_combo_inner = preset_combo.clone();
        preset_combo.connect_changed(move |_| {
            let idx = preset_combo_inner.active().unwrap_or(0) as usize;
            if idx == 0 {
                return;
            }
            let (new_cfg, outputs_clone, audio_sources_clone) = {
                let st = state.borrow();
                if let Some(p) = st.presets.get(idx - 1) {
                    let mut cfg = st.config.clone();
                    shared::apply_preset(&mut cfg, p);
                    (cfg, st.outputs.clone(), st.audio_sources.clone())
                } else {
                    return;
                }
            };
            state.borrow_mut().config = new_cfg.clone();
            sync_widgets_from_config(&widgets, &new_cfg, &outputs_clone, &audio_sources_clone);
        });
    }

    // Output combo.
    {
        let state = Rc::clone(state);
        let output_combo_inner = output_combo.clone();
        output_combo.connect_changed(move |_| {
            let idx = output_combo_inner.active().unwrap_or(0) as usize;
            let mut st = state.borrow_mut();
            if idx == 0 {
                st.config.output.clear();
            } else if let Some(o) = st.outputs.get(idx - 1) {
                st.config.output = o.name.clone();
            }
        });
    }

    // Output refresh button.
    {
        let state = Rc::clone(state);
        let output_combo_clone = output_combo.clone();
        let output_error_label_clone = output_error_label.clone();
        output_refresh_button.connect_clicked(move |_| {
            match wayland::list_outputs() {
                Ok(outputs) => {
                    refresh_output_combo(&output_combo_clone, &outputs);
                    output_error_label_clone.set_visible(false);
                    state.borrow_mut().outputs = outputs;
                    log::info!("refreshed outputs: {} found", state.borrow().outputs.len());
                }
                Err(e) => {
                    let msg = format!("<span foreground='#cc0000'>No se pudieron listar los monitores: {e}</span>");
                    output_error_label_clone.set_markup(&msg);
                    output_error_label_clone.set_visible(true);
                }
            }
        });
    }

    // Region button → run slurp.
    {
        let state = Rc::clone(state);
        let region_label = region_label.clone();
        region_button.connect_clicked(move |_| {
            let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
            std::thread::spawn(move || {
                let r = wayland::pick_region().ok().flatten();
                let _ = tx.send(r);
            });
            let state = Rc::clone(&state);
            let region_label = region_label.clone();
            glib::timeout_add_local(Duration::from_millis(100), move || {
                if let Ok(geo) = rx.try_recv() {
                    let mut st = state.borrow_mut();
                    if let Some(g) = geo {
                        st.geometry = g.clone();
                        region_label.set_text(&g);
                    } else {
                        st.geometry.clear();
                        region_label.set_text("(pantalla completa)");
                    }
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            });
        });
    }

    // Audio combo.
    {
        let state = Rc::clone(state);
        let audio_combo_inner = audio_combo.clone();
        audio_combo.connect_changed(move |_| {
            let idx = audio_combo_inner.active().unwrap_or(0) as usize;
            let mut st = state.borrow_mut();
            if idx == 0 {
                st.config.audio_source.clear();
                st.config.audio_source_desc.clear();
            } else {
                let src = st.audio_sources.get(idx - 1).cloned();
                if let Some(s) = src {
                    st.config.audio_source = s.name;
                    st.config.audio_source_desc = s.description;
                }
            }
        });
    }

    // Audio refresh button.
    {
        let state = Rc::clone(state);
        let audio_combo_clone = audio_combo.clone();
        let audio_error_label_clone = audio_error_label.clone();
        audio_refresh_button.connect_clicked(move |_| {
            let sources = list_sources();
            refresh_audio_combo(&audio_combo_clone, &sources);
            if sources.is_empty() {
                let backend = detect_backend();
                let msg = format!(
                    "<span foreground='#cc0000'>No se encontraron fuentes. Backend: {}. ¿wpctl/pactl instalados?</span>",
                    backend.label()
                );
                audio_error_label_clone.set_markup(&msg);
                audio_error_label_clone.set_visible(true);
            } else {
                audio_error_label_clone.set_visible(false);
            }
            state.borrow_mut().audio_sources = sources;
        });
    }

    // Video format combo — also updates filename preview.
    {
        let state = Rc::clone(state);
        let widgets = Rc::clone(widgets);
        let video_format_combo_inner = video_format_combo.clone();
        video_format_combo.connect_changed(move |_| {
            let idx = video_format_combo_inner.active().unwrap_or(0) as usize;
            if let Some(f) = VideoFormat::ALL.get(idx) {
                let cfg_clone = {
                    let mut st = state.borrow_mut();
                    st.config.video_format = *f;
                    st.config.clone()
                };
                update_filename_preview(&widgets, &cfg_clone);
            }
        });
    }

    // Audio format combo.
    {
        let state = Rc::clone(state);
        let audio_format_combo_inner = audio_format_combo.clone();
        audio_format_combo.connect_changed(move |_| {
            let idx = audio_format_combo_inner.active().unwrap_or(0) as usize;
            if let Some(f) = AudioFormat::ALL.get(idx) {
                state.borrow_mut().config.audio_format = *f;
            }
        });
    }

    // FPS.
    {
        let state = Rc::clone(state);
        let fps_spin_inner = fps_spin.clone();
        fps_spin.connect_value_changed(move |_| {
            state.borrow_mut().config.fps = fps_spin_inner.value() as u32;
        });
    }

    // CRF scale.
    {
        let state = Rc::clone(state);
        let crf_scale_inner = crf_scale.clone();
        let crf_value_label = crf_value_label.clone();
        crf_scale.connect_value_changed(move |_| {
            let v = crf_scale_inner.value() as u32;
            state.borrow_mut().config.crf = v;
            crf_value_label.set_label(&v.to_string());
        });
    }

    // Bitrates.
    {
        let state = Rc::clone(state);
        let video_bitrate_spin_inner = video_bitrate_spin.clone();
        video_bitrate_spin.connect_value_changed(move |_| {
            state.borrow_mut().config.video_bitrate = video_bitrate_spin_inner.value() as u32;
        });
    }
    {
        let state = Rc::clone(state);
        let audio_bitrate_spin_inner = audio_bitrate_spin.clone();
        audio_bitrate_spin.connect_value_changed(move |_| {
            state.borrow_mut().config.audio_bitrate = audio_bitrate_spin_inner.value() as u32;
        });
    }

    // Output dir button.
    {
        let state = Rc::clone(state);
        let output_dir_button_inner = output_dir_button.clone();
        let window = window.clone();
        output_dir_button.connect_clicked(move |_| {
            let dialog = FileChooserDialog::new(
                Some("Elegir carpeta de salida"),
                Some(&window),
                FileChooserAction::SelectFolder,
            );
            dialog.add_buttons(&[("Cancelar", ResponseType::Cancel), ("Abrir", ResponseType::Accept)]);
            let current = state.borrow().config.output_dir.clone();
            dialog.set_filename(&current);
            let state = Rc::clone(&state);
            let output_dir_button_inner = output_dir_button_inner.clone();
            dialog.connect_response(move |d, resp| {
                if resp == ResponseType::Accept {
                    if let Some(path) = d.filename() {
                        state.borrow_mut().config.output_dir = path.clone();
                        output_dir_button_inner.set_label(&path.display().to_string());
                    }
                }
                d.close();
            });
            dialog.show();
        });
    }

    // Filename entry — updates custom_filename + preview on every change.
    {
        let state = Rc::clone(state);
        let widgets = Rc::clone(widgets);
        let filename_entry_inner = filename_entry.clone();
        filename_entry.connect_changed(move |_| {
            let text = filename_entry_inner.text().to_string();
            let cfg_clone = {
                let mut st = state.borrow_mut();
                st.config.custom_filename = text;
                st.config.clone()
            };
            update_filename_preview(&widgets, &cfg_clone);
        });
    }

    // Toggles.
    {
        let state = Rc::clone(state);
        let dmabuf_check_inner = dmabuf_check.clone();
        dmabuf_check.connect_toggled(move |_| {
            state.borrow_mut().config.use_dmabuf = dmabuf_check_inner.is_active();
        });
    }
    {
        let state = Rc::clone(state);
        let quality_combo_inner = quality_combo.clone();
        let manual_res_box_clone = manual_res_box.clone();
        quality_combo.connect_changed(move |_| {
            let idx = quality_combo_inner.active().unwrap_or(0) as usize;
            if let Some(q) = crate::config::VideoQuality::ALL.get(idx) {
                let is_manual = *q == crate::config::VideoQuality::Manual;
                state.borrow_mut().config.quality = *q;
                manual_res_box_clone.set_sensitive(is_manual);
            }
        });
    }
    {
        let state = Rc::clone(state);
        let manual_width_spin_inner = manual_width_spin.clone();
        manual_width_spin.connect_value_changed(move |_| {
            state.borrow_mut().config.manual_width = manual_width_spin_inner.value() as u32;
        });
    }
    {
        let state = Rc::clone(state);
        let manual_height_spin_inner = manual_height_spin.clone();
        manual_height_spin.connect_value_changed(move |_| {
            state.borrow_mut().config.manual_height = manual_height_spin_inner.value() as u32;
        });
    }
    {
        let state = Rc::clone(state);
        let cursor_check_inner = cursor_check.clone();
        cursor_check.connect_toggled(move |_| {
            state.borrow_mut().config.record_cursor = cursor_check_inner.is_active();
        });
    }

    // Countdown spin.
    {
        let state = Rc::clone(state);
        let countdown_spin_inner = countdown_spin.clone();
        countdown_spin.connect_value_changed(move |_| {
            state.borrow_mut().config.countdown_secs = countdown_spin_inner.value() as u32;
        });
    }

    // Record button.
    {
        let state = Rc::clone(state);
        let widgets = Rc::clone(widgets);
        let tray_state = Arc::clone(tray_state);
        let tray_handle = tray_handle.clone();
        let window = window.clone();
        record_button.connect_clicked(move |_| {
            start_recording(&window, &state, &widgets, &tray_state, &tray_handle);
        });
    }

    // Stop button.
    {
        let state = Rc::clone(state);
        let stop_button_inner = stop_button.clone();
        let record_button_inner = record_button.clone();
        let pause_button_inner = pause_button.clone();
        stop_button.connect_clicked(move |_| {
            let mut st = state.borrow_mut();
            if let Some(h) = st.handle.as_mut() {
                let _ = h.stop();
                stop_button_inner.set_sensitive(false);
                record_button_inner.set_sensitive(false);
                pause_button_inner.set_sensitive(false);
            }
        });
    }

    // Pause button (toggle) — syncs paused state to the tray menu.
    {
        let state = Rc::clone(state);
        let tray_state = Arc::clone(tray_state);
        let tray_handle = tray_handle.clone();
        let pause_button_inner = pause_button.clone();
        pause_button.connect_toggled(move |_| {
            let active = pause_button_inner.is_active();
            let mut st = state.borrow_mut();
            st.paused = active;
            // Sync to tray so its menu label updates.
            tray::set_paused(&tray_state, &tray_handle, active);
            if let Some(h) = st.handle.as_ref() {
                if let Ok(guard) = h.child.lock() {
                    if let Some(child) = guard.as_ref() {
                        let pid = child.id() as i32;
                        let sig = if active { 19 } else { 18 }; // SIGSTOP / SIGCONT
                        unsafe {
                            extern "C" { fn kill(pid: i32, sig: i32) -> i32; }
                            let _ = kill(pid, sig);
                        }
                    }
                }
            }
            // Update the button's label + icon via its child Box.
            let new_label = if active { "Reanudar" } else { "Pausar" };
            let new_icon = if active { "media-playback-start" } else { "media-playback-pause" };
            if let Some(box_widget) = pause_button_inner.child() {
                if let Some(box_) = box_widget.downcast_ref::<GtkBox>() {
                    let children = box_.children();
                    if children.len() == 2 {
                        if let Some(img) = children[0].downcast_ref::<Image>() {
                            img.set_from_icon_name(Some(new_icon), gtk::IconSize::Button);
                        }
                        if let Some(lbl) = children[1].downcast_ref::<Label>() {
                            lbl.set_text(new_label);
                        }
                    }
                }
            }
        });
    }

    // Save preset button.
    {
        let state = Rc::clone(state);
        let widgets = Rc::clone(widgets);
        let window = window.clone();
        save_preset_button.connect_clicked(move |_| {
            save_preset_dialog(&window, &state, &widgets);
        });
    }

    // About button → show about dialog.
    {
        let window = window.clone();
        about_button.connect_clicked(move |_| {
            show_about_dialog(&window);
        });
    }

    // Keybinds button → show keybinds editor dialog.
    {
        let state = Rc::clone(state);
        let window = window.clone();
        keybinds_button.connect_clicked(move |_| {
            show_keybinds_dialog(&window, &state);
        });
    }

    // Local hotkeys — use the user-customizable keybinds from config.
    {
        let state = Rc::clone(state);
        let widgets = Rc::clone(widgets);
        let tray_state = Arc::clone(tray_state);
        let tray_handle = tray_handle.clone();
        let window_clone = window.clone();
        window.connect_key_press_event(move |_w, ev| {
            let mods = ev.state();
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            let alt = mods.contains(gdk::ModifierType::MOD1_MASK);
            let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
            let key = ev.keyval();
            // Get the key name (e.g. "r", "R", "F5", "space") and lowercase it.
            let key_name = key.name().unwrap_or_default().to_lowercase();
            // Get the current keybinds from config.
            let (kb_start, kb_stop, kb_pause) = {
                let st = state.borrow();
                let k = &st.config_keybinds;
                (k.start.clone(), k.stop.clone(), k.pause.clone())
            };
            // Check each keybind. parse_accel returns (ctrl, alt, shift, key_char).
            // We compare key_char (lowercase) with key_name's first char.
            if let Some((kc, ka, ks, kk)) = crate::config::Keybinds::parse_accel(&kb_start) {
                if ctrl == kc && alt == ka && shift == ks
                    && key_name == kk
                {
                    start_recording(&window_clone, &state, &widgets, &tray_state, &tray_handle);
                }
            }
            if let Some((kc, ka, ks, kk)) = crate::config::Keybinds::parse_accel(&kb_stop) {
                if ctrl == kc && alt == ka && shift == ks
                    && key_name == kk
                {
                    let mut st = state.borrow_mut();
                    if let Some(h) = st.handle.as_mut() {
                        let _ = h.stop();
                    }
                }
            }
            if let Some((kc, ka, ks, kk)) = crate::config::Keybinds::parse_accel(&kb_pause) {
                if ctrl == kc && alt == ka && shift == ks
                    && key_name == kk
                {
                    let w = widgets.borrow();
                    let active = !w.pause_button.is_active();
                    w.pause_button.set_active(active);
                }
            }
            glib::Propagation::Proceed
        });
    }
}

fn start_recording(
    _window: &ApplicationWindow,
    state: &Rc<RefCell<AppState>>,
    widgets: &Rc<RefCell<Widgets>>,
    tray_state: &Arc<Mutex<TrayState>>,
    tray_handle: &ksni::Handle<tray::AppTray>,
) {
    {
        let mut st = state.borrow_mut();
        st.config.geometry = st.geometry.clone();
    }
    let cfg = state.borrow().config.clone();
    let countdown = cfg.countdown_secs;
    if countdown > 0 {
        let mut st = state.borrow_mut();
        st.countdown_ends_at = Some(Instant::now() + Duration::from_secs(countdown as u64));
        let label_clone = widgets.borrow().countdown_label.clone();
        label_clone.set_markup(&format!(
            "<span font_family='monospace' size='xx-large' weight='bold' foreground='#3584e4'>{}</span>",
            countdown
        ));
    } else {
        spawn_recorder(state, widgets, tray_state, tray_handle);
    }
}

fn spawn_recorder(
    state: &Rc<RefCell<AppState>>,
    widgets: &Rc<RefCell<Widgets>>,
    tray_state: &Arc<Mutex<TrayState>>,
    tray_handle: &ksni::Handle<tray::AppTray>,
) {
    let cfg = state.borrow().config.clone();
    match RecorderController::start(&cfg) {
        Ok(handle) => {
            let status = handle.status();
            state.borrow_mut().recording_started = Some(Instant::now());
            state.borrow_mut().handle = Some(handle);
            {
                let w = widgets.borrow();
                w.record_button.set_sensitive(false);
                w.stop_button.set_sensitive(true);
                w.pause_button.set_sensitive(true);
                w.status_label.set_text(&status.label());
                w.countdown_label.set_markup("");
            }
            tray::refresh(tray_state, tray_handle, status);
        }
        Err(e) => {
            let msg = e.to_string();
            log::error!("failed to start recording: {msg}");
            widgets.borrow().status_label.set_text(&format!("Error: {msg}"));
            show_error_dialog(&msg);
        }
    }
}

fn poll_recorder(
    state: &Rc<RefCell<AppState>>,
    widgets: &Rc<RefCell<Widgets>>,
    tray_state: &Arc<Mutex<TrayState>>,
    tray_handle: &ksni::Handle<tray::AppTray>,
) {
    // Handle countdown — show big number ticking down.
    let countdown_finished = {
        let mut st = state.borrow_mut();
        if let Some(end) = st.countdown_ends_at {
            let remaining = end.saturating_duration_since(Instant::now());
            let secs = remaining.as_secs();
            let label_clone = widgets.borrow().countdown_label.clone();
            if remaining.is_zero() {
                st.countdown_ends_at = None;
                label_clone.set_markup("");
                true
            } else {
                // Colored countdown: blue for 3+, yellow for 2, red for 1.
                let color = match secs {
                    0 => "#e01b24", // red
                    1 => "#e01b24",
                    2 => "#f5c211", // yellow
                    _ => "#3584e4", // blue
                };
                label_clone.set_markup(&format!(
                    "<span font_family='monospace' size='xx-large' weight='bold' foreground='{}'>{}</span>",
                    color, secs
                ));
                false
            }
        } else {
            false
        }
    };
    if countdown_finished {
        spawn_recorder(state, widgets, tray_state, tray_handle);
    }

    // Always update elapsed timer if recording.
    {
        let st = state.borrow();
        if let Some(started) = st.recording_started {
            if st.handle.is_some() {
                let elapsed = started.elapsed();
                let secs = elapsed.as_secs();
                let label_clone = widgets.borrow().elapsed_label.clone();
                let text = format!(
                    "{:02}:{:02}:{:02}",
                    secs / 3600,
                    (secs / 60) % 60,
                    secs % 60
                );
                label_clone.set_markup(&format!(
                    "<span font_family='monospace' weight='bold' size='large'>{}</span>",
                    text
                ));
            }
        }
    }

    // Poll for new status messages.
    let new_status = {
        let mut st = state.borrow_mut();
        if let Some(h) = st.handle.as_mut() {
            h.poll()
        } else {
            None
        }
    };

    if let Some(s) = new_status {
        let label = s.label();
        {
            let w = widgets.borrow();
            w.status_label.set_text(&label);
        }
        match &s {
            RecorderStatus::Idle => {
                let cfg;
                let output_path;
                {
                    let mut st = state.borrow_mut();
                    cfg = st.config.clone();
                    output_path = st.handle.as_ref().map(|h| h.output_path.clone()).unwrap_or_default();
                    st.handle = None;
                    st.paused = false;
                    st.recording_started = None;
                }
                {
                    let w = widgets.borrow();
                    w.record_button.set_sensitive(true);
                    w.stop_button.set_sensitive(false);
                    w.pause_button.set_sensitive(false);
                    w.pause_button.set_active(false);
                    w.elapsed_label.set_markup("<span font_family='monospace' weight='bold' size='large'>00:00:00</span>");
                }
                tray::set_paused(tray_state, tray_handle, false);
                if !output_path.as_os_str().is_empty() {
                    // Post-process: scale (if quality != Original) + faststart
                    // (MP4) or convert (GIF). Runs in a background thread.
                    let path_clone = output_path.clone();
                    std::thread::spawn(move || {
                        crate::recorder::post_process(&path_clone, &cfg);
                    });
                }
            }
            RecorderStatus::Recording { output_path, .. } => {
                let w = widgets.borrow();
                w.output_path_label.set_text(&output_path.display().to_string());
                w.output_path_label.set_tooltip_text(Some(&output_path.display().to_string()));
            }
            RecorderStatus::Failed { message } => {
                {
                    let mut st = state.borrow_mut();
                    st.handle = None;
                    st.paused = false;
                    st.recording_started = None;
                }
                let w = widgets.borrow();
                w.record_button.set_sensitive(true);
                w.stop_button.set_sensitive(false);
                w.pause_button.set_sensitive(false);
                w.status_label.set_text(&format!("Error: {message}"));
                w.elapsed_label.set_markup("<span font_family='monospace' weight='bold' size='large'>00:00:00</span>");
                let msg_clone = message.clone();
                glib::idle_add_local_once(move || {
                    show_error_dialog(&msg_clone);
                });
            }
            RecorderStatus::Stopping => {
                widgets.borrow().status_label.set_text("Deteniendo…");
            }
        }
        tray::refresh(tray_state, tray_handle, s);
    }
}

fn handle_tray_cmd(
    cmd: TrayCmd,
    window: &ApplicationWindow,
    state: &Rc<RefCell<AppState>>,
    widgets: &Rc<RefCell<Widgets>>,
    tray_state: &Arc<Mutex<TrayState>>,
    tray_handle: &ksni::Handle<tray::AppTray>,
) {
    match cmd {
        TrayCmd::ToggleWindow => {
            let visible = !window.is_visible();
            if visible { window.show_all(); } else { window.hide(); }
            tray::set_window_visible(tray_state, tray_handle, visible);
        }
        TrayCmd::Start => start_recording(window, state, widgets, tray_state, tray_handle),
        TrayCmd::Stop => {
            let mut st = state.borrow_mut();
            if let Some(h) = st.handle.as_mut() { let _ = h.stop(); }
        }
        TrayCmd::Pause => {
            let w = widgets.borrow();
            let active = !w.pause_button.is_active();
            w.pause_button.set_active(active);
        }
        TrayCmd::Quit => window.close(),
    }
}

fn handle_shortcut_event(
    ev: portals::ShortcutEvent,
    window: &ApplicationWindow,
    state: &Rc<RefCell<AppState>>,
    widgets: &Rc<RefCell<Widgets>>,
    tray_state: &Arc<Mutex<TrayState>>,
    tray_handle: &ksni::Handle<tray::AppTray>,
) {
    match ev {
        portals::ShortcutEvent::Activated { id } => match id.as_str() {
            "start" => start_recording(window, state, widgets, tray_state, tray_handle),
            "stop" => {
                let mut st = state.borrow_mut();
                if let Some(h) = st.handle.as_mut() { let _ = h.stop(); }
            }
            "pause" => {
                let w = widgets.borrow();
                let active = !w.pause_button.is_active();
                w.pause_button.set_active(active);
            }
            other => log::info!("unknown shortcut id: {other}"),
        },
        portals::ShortcutEvent::Deactivated { id } => log::debug!("shortcut deactivated: {id}"),
        portals::ShortcutEvent::Closed => log::info!("GlobalShortcuts session closed"),
    }
}

fn save_preset_dialog(
    window: &ApplicationWindow,
    state: &Rc<RefCell<AppState>>,
    widgets: &Rc<RefCell<Widgets>>,
) {
    use gtk::Dialog;
    let dialog = Dialog::with_buttons(
        Some("Guardar preset"),
        Some(window),
        gtk::DialogFlags::MODAL,
        &[("Cancelar", ResponseType::Cancel), ("Guardar", ResponseType::Accept)],
    );
    dialog.set_default_size(360, 200);
    let content = dialog.content_area();
    content.set_margins(12, 12, 12, 12);
    content.set_spacing(8);
    let name_label = Label::new(Some("Nombre del preset:"));
    let name_entry = Entry::new();
    let tier_label = Label::new(Some("Tier:"));
    let tier_combo = ComboBoxText::new();
    for t in ResourceTier::ALL { tier_combo.append_text(t.label()); }
    tier_combo.set_active(Some(1));
    content.add(&name_label);
    content.add(&name_entry);
    content.add(&tier_label);
    content.add(&tier_combo);
    content.show_all();
    let state = Rc::clone(state);
    let widgets = Rc::clone(widgets);
    dialog.connect_response(move |d, resp| {
        if resp == ResponseType::Accept {
            let name = name_entry.text().trim().to_string();
            if name.is_empty() {
                show_error_dialog("El nombre del preset no puede estar vacío.");
                d.close();
                return;
            }
            let tier_idx = tier_combo.active().unwrap_or(1) as usize;
            let tier = ResourceTier::ALL.get(tier_idx).copied().unwrap_or(ResourceTier::Medium);
            let id = slugify(&name);
            let cfg_clone = {
                let st = state.borrow();
                st.config.clone()
            };
            let preset = Preset { id: id.clone(), name: name.clone(), tier, builtin: false, config: cfg_clone };
            if let Err(e) = preset.save_user() {
                show_error_dialog(&format!("No se pudo guardar el preset: {e}"));
            } else {
                let all = presets::all();
                let filtered: Vec<_> = all.into_iter().filter(|p| p.tier == tier).collect();
                state.borrow_mut().presets = filtered.clone();
                {
                    let w = widgets.borrow();
                    w.preset_combo.remove_all();
                    w.preset_combo.append_text("(personalizado)");
                    for p in &filtered { w.preset_combo.append_text(&p.name); }
                    let pos = filtered.iter().position(|p| p.id == id).map(|i| (i + 1) as u32);
                    w.preset_combo.set_active(pos);
                }
            }
        }
        d.close();
    });
    dialog.show();
}

fn slugify(s: &str) -> String {
    s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect::<String>().trim_matches('-').to_string()
}

fn show_error_dialog(msg: &str) {
    let dialog = gtk::MessageDialog::builder()
        .buttons(gtk::ButtonsType::Ok)
        .message_type(gtk::MessageType::Error)
        .text("Error al grabar")
        .secondary_text(msg)
        .build();
    dialog.connect_response(move |d, _| { d.close(); });
    dialog.show();
}

fn show_fatal_dialog(msg: &str) {
    let dialog = gtk::MessageDialog::builder()
        .buttons(gtk::ButtonsType::Ok)
        .message_type(gtk::MessageType::Error)
        .text("wf-recorder-gui no pudo iniciarse")
        .secondary_text(msg)
        .build();
    dialog.connect_response(move |d, _| { d.close(); });
    dialog.show();
}

fn show_about_dialog(parent: &ApplicationWindow) {
    let dialog = gtk::AboutDialog::builder()
        .program_name(version::APP_NAME)
        .version(version::VERSION)
        .comments(version::DESCRIPTION)
        .license_type(gtk::License::MitX11)
        .logo_icon_name("io.github.wf-recorder-gui")
        .modal(true)
        .transient_for(parent)
        .build();
    dialog.show();
}

/// Dialog to edit keyboard shortcuts with live key capture.
///
/// Each field is a button that shows the current keybind. Clicking it puts
/// it in "listening" mode — the next key press (with modifiers) is captured
/// and assigned. Pressing Escape cancels the capture; pressing Enter
/// confirms and moves focus to the next field.
///
/// This is much more intuitive than typing GTK accelerator strings by hand.
fn show_keybinds_dialog(
    parent: &ApplicationWindow,
    state: &Rc<RefCell<AppState>>,
) {
    use gtk::Dialog;
    let dialog = Dialog::with_buttons(
        Some("Personalizar atajos"),
        Some(parent),
        gtk::DialogFlags::MODAL,
        &[("Cancelar", ResponseType::Cancel), ("Guardar", ResponseType::Accept)],
    );
    dialog.set_default_size(420, 280);
    let content = dialog.content_area();
    content.set_margins(12, 12, 12, 12);
    content.set_spacing(10);

    let info = Label::new(Some(
        "Pulsa un botón y luego la combinación de teclas que quieres asignar.\n\
         Enter confirma · Esc cancela la captura"
    ));
    info.set_line_wrap(true);
    content.add(&info);

    let kb = state.borrow().config_keybinds.clone();

    // Helper to build a labeled keybind button row.
    let make_row = |label_text: &str, current: &str| -> (Label, Button) {
        let label = Label::new(Some(label_text));
        label.set_halign(gtk::Align::End);
        label.set_hexpand(true);
        let btn = Button::with_label(current);
        btn.set_hexpand(true);
        btn.set_tooltip_text(Some("Clic para capturar una nueva combinación"));
        (label, btn)
    };

    let (start_label, start_btn) = make_row("Iniciar grabación:", &kb.start);
    let (stop_label, stop_btn) = make_row("Detener grabación:", &kb.stop);
    let (pause_label, pause_btn) = make_row("Pausar/reanudar:", &kb.pause);

    let grid = gtk::Grid::new();
    grid.set_column_spacing(8);
    grid.set_row_spacing(8);
    grid.attach(&start_label, 0, 0, 1, 1);
    grid.attach(&start_btn, 1, 0, 1, 1);
    grid.attach(&stop_label, 0, 1, 1, 1);
    grid.attach(&stop_btn, 1, 1, 1, 1);
    grid.attach(&pause_label, 0, 2, 1, 1);
    grid.attach(&pause_btn, 1, 2, 1, 1);
    content.add(&grid);
    content.show_all();

    // Helper: format a key press into a GTK accelerator string.
    let format_accel = |mods: gdk::ModifierType, key: gdk::keys::Key| -> String {
        let mut s = String::new();
        if mods.contains(gdk::ModifierType::CONTROL_MASK) {
            s.push_str("<Control>");
        }
        if mods.contains(gdk::ModifierType::MOD1_MASK) {
            s.push_str("<Alt>");
        }
        if mods.contains(gdk::ModifierType::SHIFT_MASK) {
            s.push_str("<Shift>");
        }
        if mods.contains(gdk::ModifierType::SUPER_MASK) {
            s.push_str("<Super>");
        }
        let key_name = key.name().unwrap_or_default().to_string();
        // Lowercase single-char keys for consistency (r not R).
        let key_name = if key_name.len() == 1 {
            key_name.to_lowercase()
        } else {
            key_name
        };
        s.push_str(&key_name);
        s
    };

    // Helper: attach a key-press handler to a button that captures the next
    // key combination and updates the button label.
    let attach_capture = |btn: &Button| {
        let btn_clone = btn.clone();
        btn.connect_clicked(move |_| {
            btn_clone.set_label("Presiona una tecla…");
            btn_clone.grab_focus();
            let btn_for_capture = btn_clone.clone();
            let btn_for_disconnect = btn_clone.clone();
            // Use Rc<RefCell<>> so the closure can clone the handle.
            let handler_id = Rc::new(std::cell::RefCell::new(None::<glib::SignalHandlerId>));
            let handler_id_clone = Rc::clone(&handler_id);
            let capture = move |_w: &Button, ev: &gdk::EventKey| -> glib::Propagation {
                let key = ev.keyval();
                let mods = ev.state();
                // Esc cancels capture without assigning.
                if key == gdk::keys::constants::Escape {
                    btn_for_capture.set_label("…");
                    if let Some(id) = handler_id_clone.borrow_mut().take() {
                        btn_for_capture.disconnect(id);
                    }
                    return glib::Propagation::Stop;
                }
                // Enter confirms without assigning (keeps current).
                if key == gdk::keys::constants::Return || key == gdk::keys::constants::KP_Enter {
                    btn_for_capture.set_label("…");
                    if let Some(id) = handler_id_clone.borrow_mut().take() {
                        btn_for_capture.disconnect(id);
                    }
                    return glib::Propagation::Stop;
                }
                // Ignore bare modifier presses (Ctrl, Alt, Shift alone).
                let key_name = key.name().unwrap_or_default();
                if matches!(key_name.as_str(), "Control_L" | "Control_R" | "Alt_L" | "Alt_R" | "Shift_L" | "Shift_R" | "Super_L" | "Super_R") {
                    return glib::Propagation::Stop;
                }
                // Capture: format and set as label.
                let accel = format_accel(mods, key);
                btn_for_capture.set_label(&accel);
                if let Some(id) = handler_id_clone.borrow_mut().take() {
                    btn_for_capture.disconnect(id);
                }
                glib::Propagation::Stop
            };
            let id = btn_for_disconnect.connect_key_press_event(capture);
            *handler_id.borrow_mut() = Some(id);
        });
    };

    attach_capture(&start_btn);
    attach_capture(&stop_btn);
    attach_capture(&pause_btn);

    let state = Rc::clone(state);
    dialog.connect_response(move |d, resp| {
        if resp == ResponseType::Accept {
            let mut st = state.borrow_mut();
            st.config_keybinds.start = start_btn.label().unwrap_or_default().to_string();
            st.config_keybinds.stop = stop_btn.label().unwrap_or_default().to_string();
            st.config_keybinds.pause = pause_btn.label().unwrap_or_default().to_string();
            // Persist to disk.
            let mut cfg = crate::config::AppConfig::load_or_default();
            cfg.keybinds = st.config_keybinds.clone();
            if let Err(e) = cfg.save() {
                log::warn!("failed to save keybinds: {e}");
            }
        }
        d.close();
    });
    dialog.show();
}
