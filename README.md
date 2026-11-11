# zellij-salto

Plugin de [Zellij](https://zellij.dev) para saltar entre los paneles de una pestaña con una sola letra, inspirado en [nvim-window](https://github.com/yorickpeterse/nvim-window).

Cuando una pestaña acumula muchos paneles, moverse con las flechas o con `Ctrl+p` es lento. Con `zellij-salto` pulsas un atajo, ves una letra por cada panel, pulsas la letra y el foco salta a ese panel.

## Características

- Salto directo a cualquier panel de la pestaña actual con una o dos teclas.
- Dos modos de selección en un solo archivo `.wasm`: **letras** sobre los bordes de los paneles o **mapa** a pantalla completa.
- El modo mapa funciona con `pane_frames false`, con esquinas redondeadas, colores de 24 bits configurables y el panel actual resaltado.
- Etiquetas de una letra (hasta 26 paneles) y de dos letras (hasta 676) cuando hay más paneles.
- Orden de lectura estable: de arriba abajo y de izquierda a derecha.
- Cancelación segura con `Esc` o con cualquier tecla no válida.

## Modos

| | Letras | Mapa |
|---|---|---|
| Qué muestra | Una etiqueta `[a]` en el título de cada panel | Un plano de la pestaña con un rectángulo por panel, su letra y su título |
| Requiere `pane_frames true` | Sí | No |
| Ejecución | En segundo plano, activado por mensaje | Panel flotante a pantalla completa |
| Permisos | `ReadApplicationState`, `ChangeApplicationState`, `InterceptInput` | `ReadApplicationState`, `ChangeApplicationState` |
| Toca los títulos de los paneles | Sí, de forma temporal | No |
| Intercepta el teclado | Sí, mientras eliges | No |
| Paneles flotantes | Se incluyen | Se ignoran |

En el modo mapa, el panel que tenía el foco aparece con su letra en verde.

## Requisitos

- Zellij 0.45.1. El plugin está compilado contra `zellij-tile =0.45.1`; otras versiones pueden requerir ajustes en la API.
- Rust con el target `wasm32-wasip1`.

## Instalación

```bash
rustup target add wasm32-wasip1
cargo build --release
mkdir -p ~/.config/zellij/plugins
cp target/wasm32-wasip1/release/zellij_salto.wasm ~/.config/zellij/plugins/
```

## Configuración

Edita `~/.config/zellij/config.kdl`. Sustituye `<usuario>` por tu nombre de usuario.

### 1. Alias del plugin

Un alias por modo, ambos apuntan al mismo `.wasm`. Va dentro del bloque `plugins`:

```kdl
plugins {
    // ...los plugins que ya tengas...
    salto location="file:/home/<usuario>/.config/zellij/plugins/zellij_salto.wasm"
    salto-mapa location="file:/home/<usuario>/.config/zellij/plugins/zellij_salto.wasm" {
        modo "mapa"
    }
}
```

### 2. Carga en segundo plano (solo modo letras)

```kdl
load_plugins {
    "salto"
}
```

### 3. Atajos

Dentro de `keybinds`, por ejemplo en `shared_except "locked"`. Las combinaciones son solo un ejemplo: usa las que prefieras.

```kdl
bind "Alt w" {
    MessagePlugin "salto" {
        name "salto"
    }
}
bind "Alt m" {
    LaunchOrFocusPlugin "salto-mapa" {
        floating true
        move_to_focused_tab true
    }
}
```

Comprueba la sintaxis antes de abrir Zellij:

```bash
zellij setup --check
```

Zellij guarda los plugins en caché: después de instalar o actualizar el `.wasm`, cierra la sesión (`zellij kill-session <nombre>`) y abre una nueva.

## Uso

1. Pulsa el atajo del modo que quieras (`Alt m` para el mapa, `Alt w` para las letras).
2. Pulsa la letra del panel al que quieres ir. Con más de 26 paneles se usan dos letras: pulsa las dos seguidas.
3. `Esc` o cualquier otra tecla cancelan sin cambiar de panel.

En el modo letras la selección se cancela sola a los 8 segundos.

## Permisos

Zellij pide aceptar los permisos la primera vez que se carga el plugin. En el modo mapa el aviso aparece dentro de su propio panel: acepta con `y`.

Para el modo letras, si el aviso no aparece, ábrelo una vez a mano y acepta:

```bash
zellij plugin -- file:/home/<usuario>/.config/zellij/plugins/zellij_salto.wasm
```

Zellij guarda los permisos por URL del plugin.

## Personalización

Constantes al principio de `src/main.rs`:

| Constante | Descripción |
|---|---|
| `CHARS` | Letras usadas para las etiquetas, por orden. Por defecto, fila central primero. |
| `TIMEOUT_SECS` | Segundos antes de cancelar solo (modo letras). |
| `RESTORE_WITH_EMPTY_NAME` | Cómo se restauran los títulos al terminar (modo letras). Con `false` cada panel conserva su título anterior como nombre fijo; con `true` se intenta volver al título dinámico. |

## Apariencia (modo mapa)

El mapa se dibuja con esquinas redondeadas, una insignia de color para cada letra, el panel actual resaltado en otro color y una barra inferior con el número de paneles y lo que llevas tecleado. Requiere una terminal con colores de 24 bits (truecolor).

Los colores se pueden cambiar con opciones del alias `salto-mapa`, todas opcionales y en formato `#rrggbb`:

```kdl
salto-mapa location="file:/home/<usuario>/.config/zellij/plugins/zellij_salto.wasm" {
    modo "mapa"
    color_borde "#8b7bd8"          // líneas de los paneles
    color_titulo "#c9c9e3"         // título de cada panel
    color_letra "#ff2a6d"          // insignia con la letra
    color_actual "#05d9e8"         // panel que tenía el foco
    color_fondo_actual "#102a3a"   // fondo del panel que tenía el foco
}
```

Si un valor no es un color válido, se usa el predeterminado. El modo letras no se ve afectado: solo cambia los títulos de los paneles.

## Cómo funciona

**Modo letras.** El plugin corre en segundo plano. Al recibir el mensaje `salto` toma la lista de paneles de la pestaña activa (`PaneUpdate`), renombra cada panel a `[letra] título` con `rename_terminal_pane` e intercepta las teclas con `intercept_key_presses`. Con la letra elegida restaura los títulos, libera el teclado y enfoca el panel con `focus_terminal_pane`.

**Modo mapa.** Se lanza como panel flotante y pide a Zellij que ocupe toda el área (`change_floating_panes_coordinates`). Con la posición y el tamaño de cada panel (`pane_x`, `pane_y`, `pane_columns`, `pane_rows`) dibuja un plano escalado al tamaño disponible, resolviendo las uniones de los marcos con caracteres de caja. Recibe las teclas como cualquier panel con foco, enfoca el panel elegido y se cierra con `close_self`.

## Limitaciones

- El modo mapa solo muestra paneles en mosaico; los paneles flotantes no aparecen.
- En el modo letras, al restaurar los títulos cada panel queda con un nombre fijo. Prueba `RESTORE_WITH_EMPTY_NAME = true` si necesitas que vuelvan a seguir el título del shell.
- El modo letras necesita `pane_frames true` para que se vean las etiquetas.
- Probado únicamente con Zellij 0.45.1.

## Solución de problemas

| Síntoma | Causa probable |
|---|---|
| El atajo no hace nada | Faltan permisos, el atajo está ocupado por otro `bind`, o tu terminal no envía bien las teclas con Alt. |
| En modo letras no se ven las etiquetas | `pane_frames false` oculta los títulos. Activa `pane_frames true` o usa el modo mapa. |
| Los cambios del `.wasm` no se aplican | Zellij cachea los plugins: reinicia la sesión. |
| Los colores del mapa se ven mal o sin color | La terminal no soporta colores de 24 bits. |
| El mapa sale pequeño o centrado | No se aplicó la petición de pantalla completa del panel flotante. |
| Error al arrancar Zellij | Revisa `config.kdl` con `zellij setup --check`; vigila que las llaves del bloque `plugins` estén cerradas. |

Para ver los mensajes del plugin, sigue el log mientras pulsas el atajo:

```bash
tail -f /tmp/zellij-$(id -u)/zellij-log/zellij.log
```

## Desarrollo

```
zellij-salto/
├── .cargo/config.toml   # target por defecto: wasm32-wasip1
├── Cargo.toml
├── README.md
└── src/main.rs
```

```bash
cargo build --release
```

El resultado queda en `target/wasm32-wasip1/release/zellij_salto.wasm`.

## Licencia

Sin licencia definida todavía. Añade un archivo `LICENSE` antes de publicar el proyecto.
