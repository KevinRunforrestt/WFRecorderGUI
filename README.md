# wf-recorder-gui

Interfaz gráfica simple para [`wf-recorder`](https://github.com/ammen99/wf-recorder), inspirada en SimpleScreenRecorder. Funciona en compositores Wayland que implementan `wlr-screencopy-unstable-v1`: wlroots (Sway, Wayfire, Labwc, Cage, dwl) y Smithay (Niri).


> [!WARNING]
> Este es un proyecto AI generated, se usó GLM 5.2 como modelo, esto no busca ser un proyecto serio, solo le pedí a la IA que hiciera un programa que nesecitaba ya que no habia GUIs decentes para wf-recorder.

## Qué es

Un frontend en GTK3 + TUI que construye la línea de comandos correcta para `wf-recorder` y la lanza como subproceso. No reimplementa la captura de pantalla — usa wf-recorder por debajo.

## Características

- **GUI GTK3 sin CSD** — el gestor de ventanas dibuja la barra de título.
- **TUI con pestañas** tipo navegador: Config / Logs / Info / About.
- **Icono de bandeja** con estados: cámara (inactivo), punto rojo (grabando), advertencia (error). Menú con Iniciar/Detener/Pausar.
- **Presets** por nivel de hardware: bajos / medios / altos recursos, cada uno con 4 formatos.
- **Formatos**: MP4 (H.264), WebM (VP9), MKV (H.264), GIF (conversión two-pass vía ffmpeg).
- **Audio**: AAC, MP3, Vorbis, Opus. Auto-detección PipeWire/PulseAudio.
- **FPS** 15–144, **CRF** 0–51, bitrates de video y audio.
- **DMA-BUF** toggle con tooltip explicativo.
- **Cuenta regresiva** editable (0–30 s) antes de grabar.
- **Nombre de archivo editable** — si lo dejas vacío, se genera con timestamp.
- **Selección de monitor** vía `wlr-randr --json`.
- **Selección de región** vía `slurp`.
- **xdg-desktop-portal** FileChooser + GlobalShortcuts (mejor esfuerzo).
- **Detección de X11 / no-wlroots** con aviso claro.
- **Hotkeys locales**: Ctrl+Alt+R / S / P.

## Requisitos

### Runtime

| Herramienta | Para qué | Obligatoria |
|-------------|----------|-------------|
| `wf-recorder` | Grabación | Sí |
| `wlr-randr` | Listar monitores | Recomendada |
| `slurp` | Selección de región | Opcional |
| `ffmpeg` | Post-procesamiento GIF | Solo para GIF |
| `wpctl` o `pactl` | Listar fuentes de audio | Para audio |

### Compositor

wlroots: Sway, Wayfire, Labwc, Cage, dwl, swayfx, gamescope.
Smithay: Niri.

No funciona en GNOME, KDE Plasma (anterior a 6.x), X11 ni weston.

## Instalación


### Build desde fuente

```sh
# Instalar dependencias de build
./scripts/install_deps_void.sh        # Void Linux
./scripts/install_deps_debian.sh      # Debian/Ubuntu

# Construir e instalar
cargo build --release
sudo install -Dm755 target/release/wf-recorder-gui /usr/local/bin/
sudo ln -sf /usr/local/bin/wf-recorder-gui /usr/local/bin/wf-recorder-tui
sudo install -Dm644 data/io.github.wf-recorder-gui.desktop /usr/local/share/applications/
sudo install -Dm644 data/icons/io.github.wf-recorder-gui.svg /usr/local/share/icons/hicolor/scalable/apps/
```

### AppImage

```sh
./scripts/build_appimage.sh
```

### Binario musl

```sh
./scripts/install_deps_void.sh musl
./scripts/build_static_musl.sh
```

## Uso

### GUI

```sh
wf-recorder-gui
```

### TUI

```sh
wf-recorder-gui tui
# o, con el symlink:
wf-recorder-tui
```

Teclas del TUI:

| Tecla | Acción |
|-------|--------|
| `1` `2` `3` `4` | Cambiar pestaña (Config / Logs / Info / About) |
| `Tab` / `Shift+Tab` | Siguiente / anterior campo |
| `↑` `↓` `←` `→` o `h j k l` | Ajustar valor |
| `Enter` | Abrir selector |
| `Space` | Toggle checkbox |
| `r` | Grabar |
| `s` | Detener |
| `p` | Pausar / reanudar |
| `q` | Salir |

### CLI

```sh
wf-recorder-gui info     # diagnóstico para bugs
wf-recorder-gui about    # versión y características
wf-recorder-gui --help
```

## Configuración

`~/.config/wf-recorder-gui/config.toml` — guarda los últimos valores usados.

Presets de usuario en `~/.config/wf-recorder-gui/presets/` como JSON.

## Solución de problemas

### "wf-recorder terminó con código 1"

El diálogo muestra las últimas 5 líneas de stderr. Mensajes comunes:

- `compositor doesn't support wlr-screencopy` → No estás en wlroots/Smithay.
- `failed to open audio source` → El nombre de la fuente no coincide con el backend. Usa el botón ⟳ para refrescar.
- `cannot open display` → No hay sesión Wayland.

### Sin audio

1. Pulsa ⟳ junto al dropdown de audio para refrescar fuentes.
2. Verifica que el códec es compatible con el contenedor (Opus en MP4 no funciona; usa AAC para MP4).
3. Cierra otros grabadores que puedan tener la fuente ocupada (OBS, etc.).

### El icono de bandeja no aparece

Necesitas un host StatusNotifierItem. En Sway usa `waybar` con el módulo `tray`. En standalone WMs usa `stalonetray`.


## Estructura del proyecto

```
src/
├── main.rs              # Entry point + dispatch por argv0
├── cli.rs               # Clap CLI (gui/tui/info/about)
├── version.rs           # Versión y licencia
├── config.rs            # AppConfig + RecordingConfig (TOML)
├── presets.rs           # Presets built-in + JSON de usuario
├── recorder.rs          # Spawn/monitor/stop de wf-recorder
├── audio.rs             # Detección PipeWire/PulseAudio
├── wayland.rs           # Detección de sesión + wlr-randr + slurp
├── portals.rs           # xdg-desktop-portal
├── tray.rs              # Icono de bandeja (ksni)
├── shared.rs            # Helpers compartidos GUI/TUI
├── errors.rs            # Errores en español
└── ui/
    ├── gtk_app.rs       # GUI GTK3
    └── tui_app.rs       # TUI ratatui
```# WFRecorderGUI

**Versión 1.0-beta** · MIT

Interfaz gráfica para [`wf-recorder`](https://github.com/ammen99/wf-recorder), inspirada en SimpleScreenRecorder. Funciona en compositores Wayland con `wlr-screencopy-unstable-v1`: wlroots (Sway, Wayfire, Labwc, Cage, dwl) y Smithay (Niri).

## Qué es

Un frontend en GTK3 + TUI que construye la línea de comandos para `wf-recorder` y la lanza como subproceso. No reimplementa la captura — usa wf-recorder por debajo.

## Características

- **GUI GTK3 sin CSD** — el WM dibuja la barra de título
- **TUI con diseño tipo bluetui** — 6 paneles, navegación vim (h/j/k/l), soporte de ratón
- **Icono de bandeja** — cámara (inactivo), punto rojo (grabando), menú con Iniciar/Detener/Pausar
- **Presets por hardware** — bajos / medios / altos recursos, 4 formatos cada uno
- **Formatos** — MP4 (H.264), WebM (VP9), MKV (H.264), GIF (two-pass ffmpeg)
- **Audio** — AAC, MP3, Vorbis, Opus. Auto-detección PipeWire/PulseAudio
- **Calidad escalable** — Original, 144p, 240p, 360p, 720p, 1080p, Manual
- **Optimización de playback** — bf=0, faststart, tune=zerolatency (mpv reproduce sin lag)
- **FPS** 15–144, **CRF** 0–51, bitrates de video y audio
- **Cuenta regresiva** editable (0–30s) con colores
- **Nombre de archivo editable**
- **Atajos personalizables** — captura en vivo de teclas (GUI), vim-style (TUI)
- **Selección de monitor** vía wlr-randr, **región** vía slurp
- **Detección de X11/no-wlroots** con aviso claro

## Requisitos

| Herramienta | Para qué | Obligatoria |
|-------------|----------|-------------|
| wf-recorder | Grabación | Sí |
| wlr-randr | Listar monitores | Recomendada |
| slurp | Selección de región | Opcional |
| ffmpeg | Post-procesado GIF + escalado | Para GIF y escalado |
| wpctl o pactl | Listar fuentes de audio | Para audio |

Compositores soportados: Sway, Wayfire, Labwc, Cage, dwl, swayfx, gamescope, Niri.
No funciona en GNOME, KDE Plasma (anterior a 6.x), X11 ni weston.

## Instalación

El proyecto incluye scripts automatizados para todas las formas de instalación:

### Opción 1: Script de compilación e instalación directa

El método más simple. Compila con cargo e instala el binario, .desktop, iconos y symlink del TUI:

```sh
./scripts/install_deps_void.sh        # Instala dependencias (Void Linux)
# o
./scripts/install_deps_debian.sh      # Debian/Ubuntu

./scripts/build_and_install.sh        # Compila e instala en /usr/local
```

Para desinstalar:

```sh
./scripts/uninstall.sh                # Remueve binario + .desktop + iconos
./scripts/uninstall.sh --purge-config # También borra ~/.config/wf-recorder-gui/
```

### Opción 2: Paquete .xbps nativo para Void Linux (xbps-src)

Genera un paquete .xbps instalable con xbps-install, usando el sistema oficial de build de Void:

```sh
./scripts/make_xbps.sh                # Clona void-packages, compila y empaqueta
./scripts/make_xbps.sh --install      # Compila, empaqueta e instala
./scripts/make_xbps.sh --clean        # Compila, empaqueta y limpia el clone
```

El script automatiza todo: clona void-packages, copia el template, crea el tarball fuente con checksum correcto, y ejecuta `xbps-src pkg`. No requiere root (xbps-src usa user namespaces).

También puedes usar el template manualmente:

```sh
cp -r scripts/void-template /path/to/void-packages/srcpkgs/wf-recorder-gui
cd /path/to/void-packages
./xbps-src pkg wf-recorder-gui
sudo xi wf-recorder-gui
```

Ver `scripts/void-template/README.md` para detalles.

### Opción 3: AppImage portable

```sh
./scripts/build_appimage.sh
```

Genera un .AppImage autocontenido. Requiere appimagetool y linuxdeploy (el script los descarga si no están).

### Opción 4: Binario musl estático

```sh
./scripts/install_deps_void.sh musl
./scripts/build_static_musl.sh
```

Genera un binario vinculado a musl + tarball con módulos GTK. Para máxima portabilidad.

### Opción 5: Build manual con cargo

```sh
./scripts/install_deps_void.sh
cargo build --release
sudo install -Dm755 target/release/wf-recorder-gui /usr/local/bin/
sudo ln -sf /usr/local/bin/wf-recorder-gui /usr/local/bin/wf-recorder-tui
sudo install -Dm644 data/io.github.wf-recorder-gui.desktop /usr/local/share/applications/
sudo install -Dm644 data/icons/io.github.wf-recorder-gui.svg /usr/local/share/icons/hicolor/scalable/apps/
```

### Desinstalación de dependencias (Void Linux)

```sh
./scripts/uninstall_deps_void.sh                # Solo paquetes -devel + compilador
./scripts/uninstall_deps_void.sh --runtime      # También herramientas runtime
./scripts/uninstall_deps_void.sh --all          # También Rust
```

## Uso

### GUI

```sh
wf-recorder-gui
```

### TUI

```sh
wf-recorder-gui tui
# o con el symlink:
wf-recorder-tui
```

Atajos del TUI (ver `TUI_ATAJOS.txt` para la lista completa):

| Tecla | Acción |
|-------|--------|
| h/l o Tab | Panel anterior/siguiente |
| j/k o flechas | Mover selección |
| r | Grabar |
| s | Detener |
| p / Space | Pausar |
| Enter | Acción contextual |
| o | Editar ruta de guardado |
| ? | Ayuda completa |
| Click | Enfocar panel |
| q / Ctrl+c | Salir |

### CLI

```sh
wf-recorder-gui info     # Diagnóstico para bugs
wf-recorder-gui about    # Versión y características
wf-recorder-gui --help
```

## Scripts incluidos

| Script | Función |
|--------|---------|
| `install_deps_void.sh` | Instala dependencias de build + runtime en Void Linux |
| `install_deps_debian.sh` | Instala dependencias en Debian/Ubuntu |
| `build_and_install.sh` | Compila e instala el binario en el sistema |
| `uninstall.sh` | Desinstala el binario del sistema |
| `uninstall_deps_void.sh` | Remueve dependencias de build (-devel + compilador) |
| `make_xbps.sh` | Genera paquete .xbps usando xbps-src (Void Linux) |
| `build_appimage.sh` | Genera AppImage portable |
| `build_static_musl.sh` | Genera binario musl + tarball |

Todos los scripts detectan automáticamente `doas` o `sudo` (prefieren doas).

## Configuración

- Config principal: `~/.config/wf-recorder-gui/config.toml`
- Presets de usuario: `~/.config/wf-recorder-gui/presets/` (JSON)
- Atajos personalizados se guardan en el config

## Solución de problemas

### "wf-recorder terminó con código 1"

El diálogo muestra las últimas 5 líneas de stderr. Causas comunes:
- `compositor doesn't support wlr-screencopy` → No estás en wlroots/Smithay
- `failed to open audio source` → Usa el botón ⟳ para refrescar fuentes
- `cannot open display` → No hay sesión Wayland

### Sin audio

1. Pulsa ⟳ junto al dropdown de audio para refrescar
2. Verifica códec compatible con contenedor (Opus no en MP4, usa AAC)
3. Cierra otros grabadores (OBS, etc.)

### El icono de bandeja no aparece

Necesitas un host StatusNotifierItem. En Sway usa waybar con módulo tray. En standalone WMs usa stalonetray.

### El video se ve pesado al reproducir

Los videos se graban con bf=0 (sin B-frames) y faststart para playback fluido. Si aun así se siente pesado:
- Usa calidad 720p o menor
- Usa preset "Bajos recursos" (ultrafast + CRF 28)
- Verifica que el post-procesado faststart se aplicó (ver logs con `-vv`)

## Estructura del proyecto

```
src/
├── main.rs              # Entry point + dispatch por argv0
├── cli.rs               # Clap CLI (gui/tui/info/about)
├── version.rs           # Versión y licencia
├── config.rs            # AppConfig + RecordingConfig + Keybinds (TOML)
├── presets.rs           # Presets built-in + JSON de usuario
├── recorder.rs          # Spawn/monitor/stop de wf-recorder + post-procesado
├── audio.rs             # Detección PipeWire/PulseAudio
├── wayland.rs           # Detección de sesión + wlr-randr + slurp
├── portals.rs           # xdg-desktop-portal GlobalShortcuts
├── tray.rs              # Icono de bandeja (ksni)
├── shared.rs            # Helpers compartidos + sanitize_filename
├── errors.rs            # Errores en español
└── ui/
    ├── gtk_app.rs       # GUI GTK3
    └── tui_app.rs       # TUI ratatui (estilo bluetui)

scripts/
├── install_deps_void.sh
├── install_deps_debian.sh
├── build_and_install.sh
├── uninstall.sh
├── uninstall_deps_void.sh
├── make_xbps.sh
├── build_appimage.sh
├── build_static_musl.sh
└── void-template/       # Template xbps-src para Void Linux
```
