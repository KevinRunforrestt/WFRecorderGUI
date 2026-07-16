# Void Linux xbps-src template for wf-recorder-gui

Este template construye un paquete `.xbps` **desde el árbol local de fuente**, sin necesidad de subir nada a GitHub ni a ningún servidor.

## Opción A: `make_xbps.sh` (recomendado, más simple)

El script `../make_xbps.sh` automatiza todo el proceso: compila con cargo, crea un rootfs, y genera el `.xbps` con `xbps-create`. No necesitas clonar void-packages.

```sh
cd /path/to/wf-recorder-gui
./scripts/make_xbps.sh                # genera el .xbps
./scripts/make_xbps.sh --install      # genera e instala con xbps-install
```

El paquete resultante está en `target/xbps/wf-recorder-gui-0.1beta_1.x86_64.xbps`.

## Opción B: template xbps-src (build reproducible)

Si quieres usar el sistema completo de void-packages (con chroot aislado, dependencias resueltas automáticamente, etc.):

### 1. Clonar void-packages

```sh
git clone https://github.com/void-linux/void-packages.git
cd void-packages
./xbps-src binary-bootstrap
```

### 2. Copiar el template

```sh
cp -r /path/to/wf-recorder-gui/scripts/void-template srcpkgs/wf-recorder-gui
```

### 3. Copiar la fuente al build dir

El template usa `do_fetch() { :; }` y `do_extract() { :; }` para saltarse la descarga. Tienes que poner la fuente manualmente en el build dir:

```sh
# Crear el directorio de build con el nombre esperado
BUILD_DIR="masterdir/builddir/wf-recorder-gui-0.1beta"
mkdir -p "$BUILD_DIR"

# Copiar la fuente (sin target/ ni .git/)
rsync -av --exclude='target' --exclude='.git' --exclude='sysroot' \
    /path/to/wf-recorder-gui/ "$BUILD_DIR/"
```

### 4. Construir el paquete

```sh
./xbps-src pkg wf-recorder-gui
```

El paquete estará en `hostdir/binpkgs/wf-recorder-gui-0.1beta_1.x86_64.xbps`.

### 5. Instalar

```sh
sudo xbps-install hostdir/binpkgs/wf-recorder-gui-0.1beta_1.x86_64.xbps
# o
doas xbps-install hostdir/binpkgs/wf-recorder-gui-0.1beta_1.x86_64.xbps
```

## Construir desde un tarball local

Si prefieres tener un tarball en vez de copiar el árbol completo:

```sh
# Crear el tarball
cd /path/to/wf-recorder-gui
tar czf /tmp/wf-recorder-gui-0.1beta.tar.gz --exclude='target' --exclude='.git' .
```

Luego edita el `template` y descomenta las líneas:

```
distfiles="file://${HOME}/tmp/wf-recorder-gui-${version}.tar.gz"
checksum=SKIP
```

Y comenta las funciones `do_fetch` y `do_extract`:

```
# do_fetch() {
#     :
# }
#
# do_extract() {
#     :
# }
```

Ahora `xbps-src` descargará el tarball desde tu disco local.

## Dependencias

### Build (solo en el chroot, no en tu sistema base)

- `rust`, `pkg-config` (hostmakedepends)
- `gtk+3-devel`, `glib-devel`, `pango-devel`, `cairo-devel`, `atk-devel`, `at-spi2-atk-devel`
- `gdk-pixbuf-devel`, `wayland-devel`, `wayland-protocols`, `libxkbcommon-devel`
- `libepoxy-devel`, `MesaLib-devel`, `fribidi-devel`, `harfbuzz-devel`
- `pipewire-devel`, `pulseaudio-devel`, `dbus-devel`

### Runtime (instaladas automáticamente con el paquete)

- `wf-recorder`, `wlr-randr`, `slurp`, `ffmpeg`
- `xdg-desktop-portal`, `xdg-desktop-portal-wlr`

## Actualizar a una nueva versión

1. Cambia `version=` en el `template` (sin underscores en la versión).
2. Si cambiaste el template sin cambiar la versión, bump `revision=`.
3. Vuelve a copiar la fuente al build dir y ejecuta `./xbps-src pkg wf-recorder-gui`.

## Desinstalar

```sh
sudo xbps-remove -R wf-recorder-gui
# o
doas xbps-remove -R wf-recorder-gui
```
